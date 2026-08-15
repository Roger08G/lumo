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
            "authentication_failed",
            "credential_invalid",
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

internal object LumoTickProcessor {
    private const val MAX_FLUSH_PER_TICK = 8

    fun process(context: Context, role: String, location: Location?) {
        val queue = LumoSecureQueue(context)
        val credential = LumoCredentialVault.load(context)
        if (credential == null) {
            disableForCredentialRepair(context, queue)
            return
        }
        val payload = createPayload(context, role, location, credential)

        when (flushPending(context, queue, credential)) {
            LumoBackgroundResultKind.TRACKING_DISABLED -> {
                disableTracking(context, queue)
                return
            }
            LumoBackgroundResultKind.CREDENTIAL_REJECTED -> {
                disableForCredentialRepair(context, queue)
                return
            }
            else -> Unit
        }
        val invocation = invoke(payload, credential)
        when (invocation.kind) {
            LumoBackgroundResultKind.SUCCESS ->
                invocation.response?.let { publishNotifications(context, it) }
            LumoBackgroundResultKind.TRANSIENT_FAILURE -> {
                if (role == LumoServiceController.ROLE_CONTROLLED && location != null) {
                    queue.enqueue(payload)
                }
            }
            LumoBackgroundResultKind.TRACKING_DISABLED -> disableTracking(context, queue)
            LumoBackgroundResultKind.CREDENTIAL_REJECTED ->
                disableForCredentialRepair(context, queue)
        }
    }

    private fun flushPending(
        context: Context,
        queue: LumoSecureQueue,
        credential: LumoDeviceCredential,
    ): LumoBackgroundResultKind? {
        val pending = queue.read()
        if (pending.isEmpty()) return null
        var processed = 0
        for (payload in pending.take(MAX_FLUSH_PER_TICK)) {
            if (!queuedPayloadBelongsTo(payload, credential)) {
                processed += 1
                continue
            }
            val invocation = invoke(payload, credential)
            when (invocation.kind) {
                LumoBackgroundResultKind.SUCCESS -> {
                    invocation.response?.let { publishNotifications(context, it) }
                    processed += 1
                }
                LumoBackgroundResultKind.TRANSIENT_FAILURE -> break
                LumoBackgroundResultKind.TRACKING_DISABLED,
                LumoBackgroundResultKind.CREDENTIAL_REJECTED,
                -> return invocation.kind
            }
        }
        if (processed > 0) queue.replace(pending.drop(processed))
        return null
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

    private fun disableTracking(context: Context, queue: LumoSecureQueue) {
        queue.replace(emptyList())
        LumoPreferences.setTracking(
            context,
            enabled = false,
            role = null,
            intervalSeconds = LumoPreferences.intervalSeconds(context),
        )
        LumoServiceController.stop(context)
    }

    private fun disableForCredentialRepair(context: Context, queue: LumoSecureQueue) {
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
                LumoEmergencyAlarm.start(
                    context,
                    LumoPendingAlarm(
                        id = id,
                        title = title,
                        body = body,
                        phone = notification.optString("phone").takeIf(String::isNotBlank),
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
}
