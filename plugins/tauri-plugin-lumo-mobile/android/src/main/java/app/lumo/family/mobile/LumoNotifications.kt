package app.lumo.family.mobile

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit

internal object LumoNotifications {
    const val LOCATION_FOREGROUND_ID = 4101
    const val CONTROLLER_FOREGROUND_ID = 4102

    private const val TRACKING_CHANNEL = "lumo_tracking"
    private const val SYNC_CHANNEL = "lumo_sync"
    private const val ALERTS_CHANNEL = "lumo_alerts"
    const val URGENT_CHANNEL_ID = "lumo_urgent"
    private const val DEDUPE_FILE = "lumo_notification_history"
    private const val RETENTION_MS = 24 * 60 * 60 * 1000L

    fun ensureChannels(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannels(
            listOf(
                NotificationChannel(
                    TRACKING_CHANNEL,
                    context.getString(R.string.lumo_tracking_channel),
                    NotificationManager.IMPORTANCE_LOW,
                ),
                NotificationChannel(
                    SYNC_CHANNEL,
                    context.getString(R.string.lumo_sync_channel),
                    NotificationManager.IMPORTANCE_LOW,
                ),
                NotificationChannel(
                    ALERTS_CHANNEL,
                    context.getString(R.string.lumo_alerts_channel),
                    NotificationManager.IMPORTANCE_DEFAULT,
                ),
                NotificationChannel(
                    URGENT_CHANNEL_ID,
                    context.getString(R.string.lumo_urgent_channel),
                    NotificationManager.IMPORTANCE_HIGH,
                ).apply {
                    enableVibration(true)
                },
            ),
        )
    }

    fun foreground(context: Context, role: String): Notification {
        ensureChannels(context)
        val controlled = role == LumoServiceController.ROLE_CONTROLLED
        return baseBuilder(context, if (controlled) TRACKING_CHANNEL else SYNC_CHANNEL)
            .setContentTitle(
                context.getString(
                    if (controlled) R.string.lumo_tracking_title else R.string.lumo_sync_title,
                ),
            )
            .setContentText(
                context.getString(
                    if (controlled) R.string.lumo_tracking_body else R.string.lumo_sync_body,
                ),
            )
            .setOngoing(true)
            .setCategory(NotificationCompat.CATEGORY_SERVICE)
            .setPriority(NotificationCompat.PRIORITY_LOW)
            .build()
    }

    fun show(
        context: Context,
        id: String?,
        title: String,
        body: String,
        urgent: Boolean,
        deduplicate: Boolean = false,
    ): Boolean {
        if (!canNotify(context)) return false
        val notificationId = id?.hashCode()?.and(Int.MAX_VALUE) ?: title.hashCode().and(Int.MAX_VALUE)
        if (deduplicate && id != null && !markIfNew(context, id)) return true
        ensureChannels(context)
        val notification =
            baseBuilder(context, if (urgent) URGENT_CHANNEL_ID else ALERTS_CHANNEL)
                .setContentTitle(title.take(120))
                .setContentText(body.take(500))
                .setStyle(NotificationCompat.BigTextStyle().bigText(body.take(500)))
                .setAutoCancel(true)
                .setCategory(if (urgent) NotificationCompat.CATEGORY_ALARM else NotificationCompat.CATEGORY_STATUS)
                .setPriority(if (urgent) NotificationCompat.PRIORITY_HIGH else NotificationCompat.PRIORITY_DEFAULT)
                .build()
        return try {
            NotificationManagerCompat.from(context).notify(notificationId, notification)
            true
        } catch (_: SecurityException) {
            false
        }
    }

    private fun baseBuilder(context: Context, channel: String): NotificationCompat.Builder {
        val launchIntent = context.packageManager.getLaunchIntentForPackage(context.packageName)
        val pendingIntent = launchIntent?.let {
            PendingIntent.getActivity(
                context,
                0,
                it.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        }
        return NotificationCompat.Builder(context, channel)
            .setSmallIcon(context.applicationInfo.icon)
            .setContentIntent(pendingIntent)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
    }

    private fun canNotify(context: Context): Boolean =
        (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
                PackageManager.PERMISSION_GRANTED) &&
            NotificationManagerCompat.from(context).areNotificationsEnabled()

    @Synchronized
    private fun markIfNew(context: Context, id: String): Boolean {
        val now = System.currentTimeMillis()
        val preferences = context.getSharedPreferences(DEDUPE_FILE, Context.MODE_PRIVATE)
        val retained =
            preferences.all.mapNotNull { (key, value) ->
                (value as? Long)?.takeIf { now - it <= RETENTION_MS }?.let { key to it }
            }.toMap()
        if (retained.containsKey(id)) return false
        preferences.edit(commit = true) {
            clear()
            retained.entries.toList().takeLast(199).forEach { putLong(it.key, it.value) }
            putLong(id, now)
        }
        return true
    }
}
