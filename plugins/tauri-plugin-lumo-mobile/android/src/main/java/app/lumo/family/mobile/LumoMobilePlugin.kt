package app.lumo.family.mobile

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.location.Address
import android.location.Geocoder
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
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import org.json.JSONObject
import java.util.Locale

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

@InvokeArg
class CoordinatesArgs {
    var latitude: Double = Double.NaN
    var longitude: Double = Double.NaN
}

@InvokeArg
class EmergencyAlarmArgs {
    lateinit var id: String
    lateinit var title: String
    lateinit var body: String
    var phone: String? = null
    var address: String? = null
    var latitude: Double? = null
    var longitude: Double? = null
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
    private var webView: WebView? = null

    override fun load(webView: WebView) {
        super.load(webView)
        this.webView = webView
        LumoSystemInsets.install(webView)
        LumoNotifications.ensureChannels(activity.applicationContext)
    }

    override fun onResume() {
        webView?.let(LumoSystemInsets::refresh)
    }

    @Command
    fun getStatus(invoke: Invoke) {
        webView?.let(LumoSystemInsets::refresh)
        invoke.resolve(LumoDeviceStatus.snapshot(activity.applicationContext))
    }

    @Command
    fun storeCredential(invoke: Invoke) {
        val args = runCatching { invoke.parseArgs(DeviceCredentialArgs::class.java) }.getOrNull()
        val credential = args?.let(LumoDeviceCredential::fromArgs)
        if (credential == null) {
            invoke.reject("La credencial del dispositivo no es válida")
            return
        }

        val context = activity.applicationContext
        val previous = LumoCredentialVault.load(context)
        val principalChanged = previous?.samePrincipal(credential) != true
        if (principalChanged) {
            // Stop an old scheduler before rotating identity. Pending samples are also tagged and
            // checked during flush, but clearing on both sides of the write closes the common race.
            LumoSecureQueue(context).replace(emptyList())
            LumoPreferences.setTracking(
                context,
                enabled = false,
                role = null,
                intervalSeconds = LumoPreferences.intervalSeconds(context),
            )
            LumoServiceController.stop(context)
            LumoPreferences.clearControllerNotifications(context)
            LumoPreferences.clearControlledTrackingChoice(context)
        }
        if (!LumoCredentialVault.store(context, credential)) {
            invoke.reject("Android no ha podido proteger la credencial del dispositivo")
            return
        }
        if (principalChanged) {
            // Pending coordinates must never cross a group, device, role, or API boundary.
            LumoSecureQueue(context).replace(emptyList())
        }
        invoke.resolve()
    }

    @Command
    fun loadCredential(invoke: Invoke) {
        val credential = LumoCredentialVault.load(activity.applicationContext)
        invoke.resolve(
            JSObject().put(
                "credential",
                credential?.toBridgeObject() ?: JSONObject.NULL,
            ),
        )
    }

    @Command
    fun clearCredential(invoke: Invoke) {
        val context = activity.applicationContext
        val cleared = LumoCredentialVault.clear(context)
        LumoSecureQueue(context).replace(emptyList())
        LumoPreferences.setTracking(
            context,
            enabled = false,
            role = null,
            intervalSeconds = LumoPreferences.intervalSeconds(context),
        )
        LumoPreferences.clearControllerNotifications(context)
        LumoPreferences.clearControlledTrackingChoice(context)
        LumoServiceController.stop(context)
        LumoEmergencyAlarm.clear(context)
        if (cleared) {
            invoke.resolve()
        } else {
            invoke.reject("Android no ha podido borrar la credencial del dispositivo")
        }
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
            if (args.role == LumoServiceController.ROLE_CONTROLLER) {
                LumoPreferences.setControllerNotifications(context, false)
            } else {
                LumoPreferences.recordControlledTrackingChoice(context, false)
            }
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
            if (LumoDeviceStatus.backgroundLocationStatus(context) == "denied") {
                invoke.reject("Selecciona Permitir siempre en los ajustes de ubicación de Lumo")
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
            if (args.role == LumoServiceController.ROLE_CONTROLLER) {
                LumoPreferences.setControllerNotifications(context, true)
            } else {
                LumoPreferences.recordControlledTrackingChoice(context, true)
            }
            invoke.resolve(LumoDeviceStatus.snapshot(context))
        }.onFailure {
            LumoPreferences.setTracking(context, false, null, interval)
            if (args.role == LumoServiceController.ROLE_CONTROLLER) {
                LumoPreferences.setControllerNotifications(context, false)
            }
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
    fun reverseGeocode(invoke: Invoke) {
        val args = runCatching { invoke.parseArgs(CoordinatesArgs::class.java) }.getOrElse {
            invoke.reject("Las coordenadas no son válidas")
            return
        }
        if (
            !args.latitude.isFinite() || args.latitude !in -90.0..90.0 ||
                !args.longitude.isFinite() || args.longitude !in -180.0..180.0
        ) {
            invoke.reject("Las coordenadas no son válidas")
            return
        }
        if (!Geocoder.isPresent()) {
            invoke.resolve(JSObject().put("address", JSONObject.NULL))
            return
        }

        val geocoder = Geocoder(activity.applicationContext, Locale.getDefault())
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            geocoder.getFromLocation(args.latitude, args.longitude, 1) { addresses ->
                resolveAddress(invoke, addresses.firstOrNull())
            }
        } else {
            Thread {
                @Suppress("DEPRECATION")
                val address =
                    runCatching {
                        geocoder.getFromLocation(args.latitude, args.longitude, 1)?.firstOrNull()
                    }.getOrNull()
                resolveAddress(invoke, address)
            }.apply {
                name = "lumo-reverse-geocoder"
                isDaemon = true
                start()
            }
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
    fun startEmergencyAlarm(invoke: Invoke) {
        val args = runCatching { invoke.parseArgs(EmergencyAlarmArgs::class.java) }.getOrElse {
            invoke.reject("La alarma no es válida")
            return
        }
        if (args.id.isBlank() || args.title.isBlank() || args.body.isBlank()) {
            invoke.reject("La alarma no es válida")
            return
        }
        val phone =
            args.phone
                ?.trim()
                ?.takeIf { value ->
                    value.count(Char::isDigit) in 7..15 &&
                        value.matches(Regex("^[+]?[0-9 ()-]+$"))
                }
        runCatching {
            LumoEmergencyAlarm.start(
                activity.applicationContext,
                LumoPendingAlarm(
                    id = args.id,
                    title = args.title,
                    body = args.body,
                    phone = phone,
                    address = args.address?.trim()?.take(240)?.takeIf(String::isNotEmpty),
                    latitude = args.latitude?.takeIf { it.isFinite() && it in -90.0..90.0 },
                    longitude = args.longitude?.takeIf { it.isFinite() && it in -180.0..180.0 },
                ),
            )
        }.onSuccess { invoke.resolve() }
            .onFailure { invoke.reject("Android no ha podido iniciar la alarma") }
    }

    @Command
    fun getPendingAlarm(invoke: Invoke) {
        invoke.resolve(
            JSObject().put(
                "alarm",
                LumoEmergencyAlarm.load(activity.applicationContext)?.toBridgeObject()
                    ?: JSONObject.NULL,
            ),
        )
    }

    @Command
    fun stopEmergencyAlarm(invoke: Invoke) {
        LumoEmergencyAlarm.stop(activity.applicationContext)
        invoke.resolve()
    }

    @Command
    @SuppressLint("BatteryLife") // Family-safety tracking is an accepted exemption use case.
    fun openBatterySettings(invoke: Invoke) {
        val packageUri = Uri.fromParts("package", activity.packageName, null)
        val intents =
            buildList {
                if (!LumoDeviceStatus.batteryOptimizationDisabled(activity)) {
                    add(Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS, packageUri))
                }
                add(Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS))
                add(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, packageUri))
            }
        val opened =
            intents.any { intent ->
                runCatching {
                    activity.startActivity(intent)
                    true
                }.getOrDefault(false)
            }

        if (opened) {
            invoke.resolve()
        } else {
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
            role == LumoServiceController.ROLE_CONTROLLER &&
                Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE &&
                LumoDeviceStatus.fullScreenAlertsStatus(activity) == "denied"
        ) {
            val packageUri = Uri.fromParts("package", activity.packageName, null)
            runCatching {
                activity.startActivity(
                    Intent(Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, packageUri),
                )
            }.onSuccess {
                invoke.resolve(LumoDeviceStatus.snapshot(activity.applicationContext))
            }.onFailure {
                invoke.reject("Activa las alarmas a pantalla completa en los ajustes de Android")
            }
            return
        }

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

    private fun resolveAddress(invoke: Invoke, address: Address?) {
        val formatted =
            address?.getAddressLine(0)?.trim()?.takeIf(String::isNotEmpty)
                ?: address
                    ?.let {
                        listOfNotNull(it.thoroughfare, it.locality, it.adminArea, it.countryName)
                            .map(String::trim)
                            .filter(String::isNotEmpty)
                            .distinct()
                            .joinToString(", ")
                    }
                    ?.takeIf(String::isNotEmpty)
        invoke.resolve(JSObject().put("address", formatted?.take(240) ?: JSONObject.NULL))
    }
}
