package app.lumo.family.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

internal class LumoBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action !in setOf(Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED)) return
        if (!LumoPreferences.isEnabled(context)) return

        val role = LumoPreferences.role(context) ?: return
        if (
            role == LumoServiceController.ROLE_CONTROLLED &&
                LumoDeviceStatus.preciseLocationGranted(context) &&
                LumoDeviceStatus.backgroundLocationStatus(context) == "granted" &&
                LumoDeviceStatus.notificationsGranted(context)
        ) {
            runCatching {
                LumoServiceController.start(context, role, LumoPreferences.intervalSeconds(context))
            }.onFailure { showReopenNotification(context) }
        } else if (LumoDeviceStatus.notificationsGranted(context)) {
            showReopenNotification(context)
        }
    }

    private fun showReopenNotification(context: Context) {
        LumoNotifications.show(
            context = context,
            id = "lumo-reopen-after-boot",
            title = context.getString(R.string.lumo_reopen_title),
            body = context.getString(R.string.lumo_reopen_body),
            urgent = false,
            deduplicate = true,
        )
    }
}
