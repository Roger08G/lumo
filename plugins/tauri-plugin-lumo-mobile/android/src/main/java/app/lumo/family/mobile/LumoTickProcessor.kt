package app.lumo.family.mobile

import android.content.Context
import android.location.Location
import org.json.JSONArray
import org.json.JSONObject

internal enum class LumoBackgroundResultKind {
    SUCCESS,
    TRANSIENT_FAILURE,
    TRACKING_DISABLED,
    CREDENTIAL_REJECTED,
}

internal object LumoBackgroundErrorPolicy {
    private val credentialErrors =
        setOf(
            "credential_rejected",
            "credential_revoked",
        )

    fun classify(errorCode: String?, hasError: Boolean): LumoBackgroundResultKind =
        when {
            errorCode in credentialErrors -> LumoBackgroundResultKind.CREDENTIAL_REJECTED
            errorCode == "tracking_disabled" -> LumoBackgroundResultKind.TRACKING_DISABLED
            hasError -> LumoBackgroundResultKind.TRANSIENT_FAILURE
            else -> LumoBackgroundResultKind.SUCCESS
        }
}

private data class LumoBackgroundInvocation(
    val kind: LumoBackgroundResultKind,
    val response: JSONObject? = null,
)

/** An inaccessible backlog must not prevent this tick from delivering its current location. */
internal object LumoBacklogFlusher {
    private const val MAX_FLUSH_PER_TICK = 8

    fun flush(
        read: () -> List<String>,
        send: (String) -> LumoBackgroundResultKind,
        replace: (List<String>) -> Unit,
    ): LumoBackgroundResultKind? {
        // Keep ciphertext intact on provider/key failures; retry it on the next tick.
        val pending = runCatching(read).getOrElse { return null }
        if (pending.isEmpty()) return null
        var processed = 0
        var failure: LumoBackgroundResultKind? = null
        for (payload in pending.take(MAX_FLUSH_PER_TICK)) {
            when (val kind = send(payload)) {
                LumoBackgroundResultKind.SUCCESS -> processed += 1
                LumoBackgroundResultKind.TRANSIENT_FAILURE -> {
                    failure = kind
                    break
                }
                LumoBackgroundResultKind.TRACKING_DISABLED,
                LumoBackgroundResultKind.CREDENTIAL_REJECTED,
                -> return kind
            }
        }
        if (processed > 0) {
            // A failed local write may replay old entries later, but must not stop live delivery.
            runCatching { replace(pending.drop(processed)) }
        }
        return failure
    }
}

internal object LumoTickProcessor {

    @Synchronized
    fun process(context: Context, role: String, location: Location?) {
        val queue = LumoSecureQueue(context)
        val credential = runCatching { LumoCredentialVault.load(context) }.getOrElse {
            // Preserve the user's configuration and retry on the next scheduled tick.
            return
        }
        if (credential == null) {
            disableForCredentialRepair(context, queue, null)
            return
        }
        if (credential.role != role) return
        val pendingAcknowledgement =
            if (role == LumoServiceController.ROLE_CONTROLLER) {
                LumoEmergencyAlarm.pendingAcknowledgement(context)
            } else {
                null
            }
        val payload =
            createPayload(context, role, location, credential, pendingAcknowledgement)

        when (flushPending(context, queue, credential)) {
            LumoBackgroundResultKind.TRACKING_DISABLED -> {
                withCurrentCredential(context, credential) { disableTracking(context, queue, recordPause = true) }
                return
            }
            LumoBackgroundResultKind.CREDENTIAL_REJECTED -> {
                disableForCredentialRepair(context, queue, credential)
                return
            }
            LumoBackgroundResultKind.TRANSIENT_FAILURE -> {
                withCurrentCredential(context, credential) {
                    if (role == LumoServiceController.ROLE_CONTROLLED && location != null) {
                        queue.enqueue(payload)
                    }
                }
                return
            }
            else -> Unit
        }
        val invocation = invoke(payload, credential)
        withCurrentCredential(context, credential) {
            when (invocation.kind) {
                LumoBackgroundResultKind.SUCCESS -> {
                    pendingAcknowledgement?.let {
                        LumoEmergencyAlarm.completeAcknowledgement(context, it)
                    }
                    invocation.response?.let { publishNotifications(context, it) }
                }
                LumoBackgroundResultKind.TRANSIENT_FAILURE -> {
                    if (role == LumoServiceController.ROLE_CONTROLLED && location != null) {
                        queue.enqueue(payload)
                    }
                }
                LumoBackgroundResultKind.TRACKING_DISABLED -> disableTracking(context, queue, recordPause = true)
                LumoBackgroundResultKind.CREDENTIAL_REJECTED ->
                    disableForCredentialRepair(context, queue, credential)
            }
        }
    }

    fun acknowledgeEmergency(context: Context) {
        Thread {
            process(
                context.applicationContext,
                LumoServiceController.ROLE_CONTROLLER,
                location = null,
            )
        }.apply {
            name = "lumo-emergency-ack"
            isDaemon = true
            start()
        }
    }

    private fun flushPending(
        context: Context,
        queue: LumoSecureQueue,
        credential: LumoDeviceCredential,
    ): LumoBackgroundResultKind? =
        LumoBacklogFlusher.flush(
            read = queue::read,
            send = { payload ->
                if (!queuedPayloadBelongsTo(payload, credential)) {
                    LumoBackgroundResultKind.SUCCESS
                } else {
                    val invocation = invoke(payload, credential)
                    if (invocation.kind == LumoBackgroundResultKind.SUCCESS) {
                        val published = withCurrentCredential(context, credential) {
                            invocation.response?.let { publishNotifications(context, it) }
                        }
                        if (published) {
                            LumoBackgroundResultKind.SUCCESS
                        } else {
                            LumoBackgroundResultKind.CREDENTIAL_REJECTED
                        }
                    } else {
                        invocation.kind
                    }
                }
            },
            replace = { remaining ->
                withCurrentCredential(context, credential) { queue.replace(remaining) }
            },
        )

    private fun withCurrentCredential(
        context: Context,
        expected: LumoDeviceCredential,
        operation: () -> Unit,
    ): Boolean = LumoCredentialVault.withLock {
        val current = runCatching { LumoCredentialVault.load(context) }.getOrNull()
        if (
            current == null || !current.samePrincipal(expected) ||
                current.deviceToken != expected.deviceToken
        ) {
            false
        } else {
            operation()
            true
        }
    }

    private fun queuedPayloadBelongsTo(
        payload: String,
        credential: LumoDeviceCredential,
    ): Boolean =
        runCatching {
            val json = JSONObject(payload)
            LumoQueueCredentialPolicy.belongsTo(
                groupId = json.optString("credentialGroupId"),
                deviceId = json.optString("credentialDeviceId"),
                credential = credential,
            )
        }.getOrDefault(false)

    private fun createPayload(
        context: Context,
        role: String,
        location: Location?,
        credential: LumoDeviceCredential,
        acknowledgeAlarmId: String? = null,
    ): String {
        val payload =
            JSONObject()
                .put("role", role)
                .put("timestampMs", System.currentTimeMillis())
                .put("credentialGroupId", credential.groupId)
                .put("credentialDeviceId", credential.deviceId)
                // Tauri's Android app_data_dir resolves to Context.dataDir. Keeping this
                // exact root lets the foreground service and the UI share one repository.
                .put("dataDir", context.dataDir.absolutePath)
                .put("batteryPercent", LumoDeviceStatus.batteryPercent(context))
                .put(
                    "preciseLocationGranted",
                    LumoDeviceStatus.preciseLocationGranted(context),
                )
                .put(
                    "backgroundLocationGranted",
                    LumoDeviceStatus.backgroundLocationStatus(context) in
                        setOf("granted", "notRequired"),
                )
                .put(
                    "batteryOptimizationDisabled",
                    LumoDeviceStatus.batteryOptimizationDisabled(context),
                )
                .put("acknowledgeAlarmId", acknowledgeAlarmId ?: JSONObject.NULL)
        if (location != null) {
            payload.put(
                "location",
                JSONObject()
                    .put("latitude", location.latitude)
                    .put("longitude", location.longitude)
                    .put("accuracy", location.accuracy.toDouble())
                    .put("timestampMs", location.time),
            )
        } else {
            payload.put("location", JSONObject.NULL)
        }
        return payload.toString()
    }

    private fun invoke(
        payload: String,
        credential: LumoDeviceCredential,
    ): LumoBackgroundInvocation =
        runCatching {
            val tick =
                JSONObject(payload)
                    .put("deviceCredential", credential.toJson())
                    .toString()
            val response = JSONObject(LumoRustBridge.processBackgroundTick(tick))
            val hasError = response.has("error") && !response.isNull("error")
            val errorCode = response.optString("errorCode").trim().takeIf(String::isNotEmpty)
            val kind = LumoBackgroundErrorPolicy.classify(errorCode, hasError)
            LumoBackgroundInvocation(
                kind = kind,
                response = response.takeIf { kind == LumoBackgroundResultKind.SUCCESS },
            )
        }.getOrElse {
            LumoBackgroundInvocation(LumoBackgroundResultKind.TRANSIENT_FAILURE)
        }

    private fun disableTracking(context: Context, queue: LumoSecureQueue, recordPause: Boolean = false) {
        queue.replace(emptyList())
        if (recordPause) {
            // A server-confirmed pause must not be treated as an OEM interruption by the UI.
            LumoPreferences.recordControlledTrackingChoice(context, false)
        }
        LumoPreferences.setTracking(
            context,
            enabled = false,
            role = null,
            intervalSeconds = LumoPreferences.intervalSeconds(context),
        )
        LumoServiceController.stop(context)
    }

    private fun disableForCredentialRepair(
        context: Context,
        queue: LumoSecureQueue,
        rejected: LumoDeviceCredential?,
    ) = LumoCredentialVault.withLock {
        val current = runCatching { LumoCredentialVault.load(context) }.getOrElse {
            return@withLock
        }
        // An old HTTP response must not erase a replacement credential installed by the UI.
        if (
            current?.deviceToken != rejected?.deviceToken ||
                (current != null && rejected != null && !current.samePrincipal(rejected))
        ) {
            return@withLock
        }
        LumoCredentialVault.clear(context)
        LumoPreferences.clearControllerNotifications(context)
        LumoPreferences.clearControlledTrackingChoice(context)
        disableTracking(context, queue)
        LumoNotifications.show(
            context = context,
            id = "lumo-device-credential-repair",
            title = context.getString(R.string.lumo_repair_title),
            body = context.getString(R.string.lumo_repair_body),
            urgent = false,
            deduplicate = true,
        )
    }

    private fun publishNotifications(context: Context, response: JSONObject) {
        val notifications = response.optJSONArray("notifications") ?: JSONArray()
        for (index in 0 until notifications.length()) {
            val notification = notifications.optJSONObject(index) ?: continue
            val id = notification.optString("id").takeIf(String::isNotBlank) ?: continue
            val title = notification.optString("title").takeIf(String::isNotBlank) ?: continue
            val body = notification.optString("body")
            if (notification.optBoolean("urgent", false)) {
                val latitude =
                    notification
                        .takeIf { it.has("latitude") && !it.isNull("latitude") }
                        ?.optDouble("latitude")
                        ?.takeIf { it.isFinite() && it in -90.0..90.0 }
                val longitude =
                    notification
                        .takeIf { it.has("longitude") && !it.isNull("longitude") }
                        ?.optDouble("longitude")
                        ?.takeIf { it.isFinite() && it in -180.0..180.0 }
                LumoEmergencyAlarm.start(
                    context,
                    LumoPendingAlarm(
                        id = id,
                        title = title,
                        body = body,
                        phone = optionalText(notification, "phone"),
                        address = optionalText(notification, "address"),
                        latitude = latitude,
                        longitude = longitude,
                    ),
                )
            } else {
                LumoNotifications.show(
                    context = context,
                    id = id,
                    title = title,
                    body = body,
                    urgent = false,
                    deduplicate = true,
                )
            }
        }
    }

    private fun optionalText(source: JSONObject, key: String): String? =
        if (source.has(key) && !source.isNull(key)) {
            LumoAlarmPayloadPolicy.optionalText(source.optString(key))
        } else {
            null
        }
}
