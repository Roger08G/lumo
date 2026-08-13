package app.lumo.family.mobile

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.location.LocationManager
import android.os.BatteryManager
import android.os.Build
import android.os.PowerManager
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.core.location.LocationManagerCompat
import app.tauri.plugin.JSObject

internal object LumoDeviceStatus {
    fun preciseLocationGranted(context: Context): Boolean =
        ContextCompat.checkSelfPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) ==
            PackageManager.PERMISSION_GRANTED

    fun backgroundLocationStatus(context: Context): String {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return "notRequired"
        return if (
            ContextCompat.checkSelfPermission(
                context,
                Manifest.permission.ACCESS_BACKGROUND_LOCATION,
            ) == PackageManager.PERMISSION_GRANTED
        ) {
            "granted"
        } else {
            "denied"
        }
    }

    fun notificationsGranted(context: Context): Boolean {
        val runtimePermissionGranted =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                ContextCompat.checkSelfPermission(
                    context,
                    Manifest.permission.POST_NOTIFICATIONS,
                ) == PackageManager.PERMISSION_GRANTED
        return runtimePermissionGranted && NotificationManagerCompat.from(context).areNotificationsEnabled()
    }

    fun batteryOptimizationDisabled(context: Context): Boolean {
        val powerManager = context.getSystemService(Context.POWER_SERVICE) as PowerManager
        return powerManager.isIgnoringBatteryOptimizations(context.packageName)
    }

    fun batteryPercent(context: Context): Int {
        val battery = context.registerReceiver(null, android.content.IntentFilter(Intent.ACTION_BATTERY_CHANGED))
            ?: return 0
        val level = battery.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = battery.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
        if (level < 0 || scale <= 0) return 0
        return ((level * 100f) / scale).toInt().coerceIn(0, 100)
    }

    fun locationServicesEnabled(context: Context): Boolean {
        val manager = context.getSystemService(Context.LOCATION_SERVICE) as LocationManager
        return LocationManagerCompat.isLocationEnabled(manager)
    }

    fun snapshot(context: Context): JSObject =
        JSObject()
            .put("platform", "android")
            .put("trackingEnabled", LumoPreferences.isEnabled(context))
            .put("role", LumoPreferences.role(context))
            .put("preciseLocation", if (preciseLocationGranted(context)) "granted" else "denied")
            .put("backgroundLocation", backgroundLocationStatus(context))
            .put("notifications", if (notificationsGranted(context)) "granted" else "denied")
            .put("batteryOptimizationDisabled", batteryOptimizationDisabled(context))
            .put("batteryPercent", batteryPercent(context))
            .put("locationServicesEnabled", locationServicesEnabled(context))
}
