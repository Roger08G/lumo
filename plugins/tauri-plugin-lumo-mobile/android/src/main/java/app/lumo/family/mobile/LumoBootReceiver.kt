package app.lumo.family.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

internal class LumoBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        if (intent.action !in setOf(Intent.ACTION_BOOT_COMPLETED, Intent.ACTION_MY_PACKAGE_REPLACED)) return
        val role = LumoPreferences.role(context) ?: return
        when (
            LumoRestartPolicy.action(
                enabled = LumoPreferences.isEnabled(context),
                role = role,
                notificationsGranted = LumoDeviceStatus.notificationsGranted(context),
                preciseLocationGranted = LumoDeviceStatus.preciseLocationGranted(context),
                backgroundLocationGranted =
                    LumoDeviceStatus.backgroundLocationStatus(context) in
                        setOf("granted", "notRequired"),
            )
        ) {
            LumoRestartAction.START -> {
                runCatching {
                    LumoServiceController.start(
                        context,
                        role,
                        LumoPreferences.intervalSeconds(context),
                    )
                }.onFailure { showReopenNotification(context) }
            }
            LumoRestartAction.SHOW_REOPEN -> showReopenNotification(context)
            LumoRestartAction.IGNORE -> Unit
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
