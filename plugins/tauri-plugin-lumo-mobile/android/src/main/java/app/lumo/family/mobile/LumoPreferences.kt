package app.lumo.family.mobile

import android.content.Context
import androidx.core.content.edit

internal object LumoPreferences {
    private const val FILE_NAME = "lumo_mobile_runtime"
    private const val KEY_ENABLED = "tracking_enabled"
    private const val KEY_ROLE = "tracking_role"
    private const val KEY_INTERVAL = "tracking_interval_seconds"
    private const val KEY_CONTROLLER_NOTIFICATIONS_CONFIGURED =
        "controller_notifications_configured"
    private const val KEY_CONTROLLER_NOTIFICATIONS_ENABLED = "controller_notifications_enabled"
    private const val KEY_CONTROLLED_TRACKING_CONFIGURED = "controlled_tracking_configured"
    private const val KEY_CONTROLLED_TRACKING_EXPLICITLY_DISABLED =
        "controlled_tracking_explicitly_disabled"

    private fun preferences(context: Context) =
        context.getSharedPreferences(FILE_NAME, Context.MODE_PRIVATE)

    fun isEnabled(context: Context): Boolean =
        preferences(context).getBoolean(KEY_ENABLED, false)

    fun role(context: Context): String? =
        preferences(context).getString(KEY_ROLE, null)

    fun intervalSeconds(context: Context): Long =
        preferences(context).getLong(KEY_INTERVAL, LumoServiceController.DEFAULT_INTERVAL_SECONDS)
            .coerceIn(
                LumoServiceController.MIN_INTERVAL_SECONDS,
                LumoServiceController.MAX_INTERVAL_SECONDS,
            )

    fun controllerNotificationsConfigured(context: Context): Boolean =
        preferences(context).getBoolean(KEY_CONTROLLER_NOTIFICATIONS_CONFIGURED, false)

    fun controllerNotificationsEnabled(context: Context): Boolean =
        preferences(context).getBoolean(KEY_CONTROLLER_NOTIFICATIONS_ENABLED, false)

    fun setControllerNotifications(context: Context, enabled: Boolean) {
        preferences(context).edit(commit = true) {
            putBoolean(KEY_CONTROLLER_NOTIFICATIONS_CONFIGURED, true)
            putBoolean(KEY_CONTROLLER_NOTIFICATIONS_ENABLED, enabled)
        }
    }

    fun clearControllerNotifications(context: Context) {
        preferences(context).edit(commit = true) {
            remove(KEY_CONTROLLER_NOTIFICATIONS_CONFIGURED)
            remove(KEY_CONTROLLER_NOTIFICATIONS_ENABLED)
        }
    }

    fun controlledTrackingMayAutoRecover(context: Context): Boolean =
        LumoControlledTrackingPolicy.mayAutoRecover(
            configured = preferences(context).getBoolean(KEY_CONTROLLED_TRACKING_CONFIGURED, false),
            explicitlyDisabled =
                preferences(context).getBoolean(
                    KEY_CONTROLLED_TRACKING_EXPLICITLY_DISABLED,
                    false,
                ),
        )

    fun recordControlledTrackingChoice(context: Context, enabled: Boolean) {
        preferences(context).edit(commit = true) {
            putBoolean(KEY_CONTROLLED_TRACKING_CONFIGURED, true)
            putBoolean(KEY_CONTROLLED_TRACKING_EXPLICITLY_DISABLED, !enabled)
        }
    }

    fun clearControlledTrackingChoice(context: Context) {
        preferences(context).edit(commit = true) {
            remove(KEY_CONTROLLED_TRACKING_CONFIGURED)
            remove(KEY_CONTROLLED_TRACKING_EXPLICITLY_DISABLED)
        }
    }

    fun setTracking(context: Context, enabled: Boolean, role: String?, intervalSeconds: Long) {
        preferences(context).edit(commit = true) {
            putBoolean(KEY_ENABLED, enabled)
            putString(KEY_ROLE, if (enabled) role else null)
            putLong(
                KEY_INTERVAL,
                intervalSeconds.coerceIn(
                    LumoServiceController.MIN_INTERVAL_SECONDS,
                    LumoServiceController.MAX_INTERVAL_SECONDS,
                ),
            )
        }
    }
}
