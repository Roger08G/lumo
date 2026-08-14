package app.lumo.family.mobile

import android.content.Context
import android.location.Location
import org.json.JSONArray
import org.json.JSONObject

internal object LumoTickProcessor {
    private const val MAX_FLUSH_PER_TICK = 8

    fun process(context: Context, role: String, location: Location?) {
        val queue = LumoSecureQueue(context)
        flushPending(context, queue)

        val payload = createPayload(context, role, location)
        val response = invoke(payload)
        if (response == null && role == LumoServiceController.ROLE_CONTROLLED && location != null) {
            queue.enqueue(payload)
        } else if (response != null) {
            publishNotifications(context, response)
        }
    }

    private fun flushPending(context: Context, queue: LumoSecureQueue) {
        val pending = queue.read()
        if (pending.isEmpty()) return
        var processed = 0
        for (payload in pending.take(MAX_FLUSH_PER_TICK)) {
            val response = invoke(payload) ?: break
            publishNotifications(context, response)
            processed += 1
        }
        if (processed > 0) queue.replace(pending.drop(processed))
    }

    private fun createPayload(context: Context, role: String, location: Location?): String {
        val payload =
            JSONObject()
                .put("role", role)
                .put("timestampMs", System.currentTimeMillis())
                // Tauri's Android app_data_dir resolves to Context.dataDir. Keeping this
                // exact root lets the foreground service and the UI share one repository.
                .put("dataDir", context.dataDir.absolutePath)
                .put("batteryPercent", LumoDeviceStatus.batteryPercent(context))
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

    private fun invoke(payload: String): JSONObject? =
        runCatching {
            val response = JSONObject(LumoRustBridge.processBackgroundTick(payload))
            if (response.has("error") && !response.isNull("error")) null else response
        }.getOrNull()

    private fun publishNotifications(context: Context, response: JSONObject) {
        val notifications = response.optJSONArray("notifications") ?: JSONArray()
        for (index in 0 until notifications.length()) {
            val notification = notifications.optJSONObject(index) ?: continue
            val id = notification.optString("id").takeIf(String::isNotBlank) ?: continue
            val title = notification.optString("title").takeIf(String::isNotBlank) ?: continue
            val body = notification.optString("body")
            LumoNotifications.show(
                context = context,
                id = id,
                title = title,
                body = body,
                urgent = notification.optBoolean("urgent", false),
                deduplicate = true,
            )
        }
    }
}
