package app.lumo.family.mobile

import android.content.Context
import androidx.core.content.edit

internal object LumoPreferences {
    private const val FILE_NAME = "lumo_mobile_runtime"
    private const val KEY_ENABLED = "tracking_enabled"
    private const val KEY_ROLE = "tracking_role"
    private const val KEY_INTERVAL = "tracking_interval_seconds"

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
