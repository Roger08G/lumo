package app.lumo.family.mobile

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.webkit.WebView
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.PermissionCallback
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin

private const val ALIAS_PRECISE_LOCATION = "preciseLocation"
private const val ALIAS_BACKGROUND_LOCATION = "backgroundLocation"
private const val ALIAS_NOTIFICATIONS = "notifications"

@InvokeArg
class RoleArgs {
    lateinit var role: String
}

@InvokeArg
class TrackingArgs {
    lateinit var role: String
    var enabled: Boolean = false
    var intervalSeconds: Long = LumoServiceController.DEFAULT_INTERVAL_SECONDS
}

@InvokeArg
class PhoneArgs {
    lateinit var number: String
}

@InvokeArg
class NotificationArgs {
    var id: String? = null
    lateinit var title: String
    lateinit var body: String
    var urgent: Boolean = false
}

@TauriPlugin(
    permissions = [
        Permission(
            strings = [
                Manifest.permission.ACCESS_COARSE_LOCATION,
                Manifest.permission.ACCESS_FINE_LOCATION,
            ],
            alias = ALIAS_PRECISE_LOCATION,
        ),
        Permission(
            strings = [Manifest.permission.ACCESS_BACKGROUND_LOCATION],
            alias = ALIAS_BACKGROUND_LOCATION,
        ),
        Permission(
            strings = [Manifest.permission.POST_NOTIFICATIONS],
            alias = ALIAS_NOTIFICATIONS,
        ),
    ],
)
class LumoMobilePlugin(private val activity: Activity) : Plugin(activity) {
    override fun load(webView: WebView) {
        super.load(webView)
        LumoNotifications.ensureChannels(activity.applicationContext)
    }

    @Command
    fun getStatus(invoke: Invoke) {
        invoke.resolve(LumoDeviceStatus.snapshot(activity.applicationContext))
    }

    @Command
    override fun requestPermissions(invoke: Invoke) {
        val role = runCatching { invoke.parseArgs(RoleArgs::class.java).role }.getOrNull()
        if (!validRole(role)) {
            invoke.reject("El modo solicitado no es compatible")
            return
        }
        if (
            role == LumoServiceController.ROLE_CONTROLLED &&
                !LumoDeviceStatus.preciseLocationGranted(activity)
        ) {
            requestPermissionForAlias(
                ALIAS_PRECISE_LOCATION,
                invoke,
                "preciseLocationPermissionCallback",
            )
            return
        }
        continueAfterPreciseLocation(invoke, requireNotNull(role))
    }

    @PermissionCallback
    private fun preciseLocationPermissionCallback(invoke: Invoke) {
        val role = invoke.parseArgs(RoleArgs::class.java).role
        if (!LumoDeviceStatus.preciseLocationGranted(activity)) {
            invoke.reject("Se necesita ubicación precisa para activar el seguimiento")
            return
        }
        continueAfterPreciseLocation(invoke, role)
    }

    @PermissionCallback
    private fun notificationPermissionCallback(invoke: Invoke) {
        val role = invoke.parseArgs(RoleArgs::class.java).role
        continueAfterNotifications(invoke, role)
    }

    @PermissionCallback
    private fun backgroundLocationPermissionCallback(invoke: Invoke) {
        invoke.resolve(LumoDeviceStatus.snapshot(activity.applicationContext))
    }

    @Command
    fun configureTracking(invoke: Invoke) {
        val args = runCatching { invoke.parseArgs(TrackingArgs::class.java) }.getOrElse {
            invoke.reject("La configuración de seguimiento no es válida")
            return
        }
        if (!validRole(args.role)) {
            invoke.reject("El modo solicitado no es compatible")
            return
        }

        val context = activity.applicationContext
        if (!args.enabled) {
            LumoServiceController.stop(context)
            LumoPreferences.setTracking(context, false, null, args.intervalSeconds)
            invoke.resolve(LumoDeviceStatus.snapshot(context))
            return
        }
        if (!LumoDeviceStatus.notificationsGranted(context)) {
            invoke.reject("Activa las notificaciones de Android antes de iniciar Lumo")
            return
        }
        if (args.role == LumoServiceController.ROLE_CONTROLLED) {
            if (!LumoDeviceStatus.preciseLocationGranted(context)) {
                invoke.reject("Concede ubicación precisa antes de iniciar el seguimiento")
                return
            }
            if (!LumoDeviceStatus.locationServicesEnabled(context)) {
                invoke.reject("Activa la ubicación del teléfono antes de iniciar el seguimiento")
                return
            }
        }

        val interval =
            args.intervalSeconds.coerceIn(
                LumoServiceController.MIN_INTERVAL_SECONDS,
                LumoServiceController.MAX_INTERVAL_SECONDS,
            )
        runCatching {
            LumoServiceController.stop(context)
            LumoPreferences.setTracking(context, false, null, interval)
            LumoServiceController.start(context, args.role, interval)
            LumoPreferences.setTracking(context, true, args.role, interval)
        }.onSuccess {
            invoke.resolve(LumoDeviceStatus.snapshot(context))
        }.onFailure {
            LumoPreferences.setTracking(context, false, null, interval)
            invoke.reject("Android no ha podido iniciar el servicio de Lumo")
        }
    }

    @Command
    fun openPhoneDialer(invoke: Invoke) {
        val number = runCatching { invoke.parseArgs(PhoneArgs::class.java).number.trim() }.getOrNull()
        val digits = number?.count(Char::isDigit) ?: 0
        if (number.isNullOrBlank() || digits !in 7..15 || !number.matches(Regex("^[+]?[0-9 ()-]+$"))) {
            invoke.reject("El número de teléfono no es válido")
            return
        }
        runCatching {
            activity.startActivity(Intent(Intent.ACTION_DIAL, Uri.fromParts("tel", number, null)))
        }.onSuccess {
            invoke.resolve()
        }.onFailure {
            invoke.reject("No hay una aplicación de llamadas disponible")
        }
    }

    @Command
    fun showNotification(invoke: Invoke) {
        val args = runCatching { invoke.parseArgs(NotificationArgs::class.java) }.getOrElse {
            invoke.reject("La notificación no es válida")
            return
        }
        if (args.title.isBlank() || args.body.isBlank()) {
            invoke.reject("La notificación necesita título y contenido")
            return
        }
        if (
            LumoNotifications.show(
                context = activity.applicationContext,
                id = args.id,
                title = args.title,
                body = args.body,
                urgent = args.urgent,
                deduplicate = args.id != null,
            )
        ) {
            invoke.resolve()
        } else {
            invoke.reject("Las notificaciones de Android están desactivadas")
        }
    }

    @Command
    fun openBatterySettings(invoke: Invoke) {
        runCatching {
            activity.startActivity(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
        }.onSuccess {
            invoke.resolve()
        }.onFailure {
            invoke.reject("Android no permite abrir los ajustes de batería")
        }
    }

    private fun continueAfterPreciseLocation(invoke: Invoke, role: String) {
        if (
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                !LumoDeviceStatus.notificationsGranted(activity)
        ) {
            requestPermissionForAlias(
                ALIAS_NOTIFICATIONS,
                invoke,
                "notificationPermissionCallback",
            )
            return
        }
        continueAfterNotifications(invoke, role)
    }

    private fun continueAfterNotifications(invoke: Invoke, role: String) {
        if (
            role != LumoServiceController.ROLE_CONTROLLED ||
                Build.VERSION.SDK_INT < Build.VERSION_CODES.Q ||
                LumoDeviceStatus.backgroundLocationStatus(activity) == "granted"
        ) {
            invoke.resolve(LumoDeviceStatus.snapshot(activity.applicationContext))
            return
        }

        if (Build.VERSION.SDK_INT == Build.VERSION_CODES.Q) {
            requestPermissionForAlias(
                ALIAS_BACKGROUND_LOCATION,
                invoke,
                "backgroundLocationPermissionCallback",
            )
            return
        }

        runCatching {
            activity.startActivity(
                Intent(
                    Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
                    Uri.fromParts("package", activity.packageName, null),
                ),
            )
        }.onSuccess {
            invoke.resolve(LumoDeviceStatus.snapshot(activity.applicationContext))
        }.onFailure {
            invoke.reject("Abre los ajustes de Android y permite la ubicación todo el tiempo")
        }
    }

    private fun validRole(role: String?): Boolean =
        role == LumoServiceController.ROLE_CONTROLLED || role == LumoServiceController.ROLE_CONTROLLER
}
