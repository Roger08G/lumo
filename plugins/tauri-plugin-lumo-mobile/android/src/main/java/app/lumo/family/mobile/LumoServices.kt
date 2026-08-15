package app.lumo.family.mobile

import android.Manifest
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.location.Location
import android.location.LocationListener
import android.location.LocationManager
import android.net.ConnectivityManager
import android.net.Network
import android.os.Build
import android.os.IBinder
import android.os.Looper
import android.os.PowerManager
import android.os.SystemClock
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

internal object LumoServiceController {
    const val ROLE_CONTROLLED = "controlled"
    const val ROLE_CONTROLLER = "controller"
    const val MIN_INTERVAL_SECONDS = 5L
    const val MAX_INTERVAL_SECONDS = 900L
    const val DEFAULT_INTERVAL_SECONDS = 5L
    const val EXTRA_INTERVAL_SECONDS = "intervalSeconds"

    fun start(context: Context, role: String, intervalSeconds: Long) {
        val serviceClass =
            when (role) {
                ROLE_CONTROLLED -> LumoLocationService::class.java
                ROLE_CONTROLLER -> LumoControllerService::class.java
                else -> throw IllegalArgumentException("unsupported mobile role")
            }
        val intent =
            Intent(context, serviceClass).putExtra(
                EXTRA_INTERVAL_SECONDS,
                intervalSeconds.coerceIn(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS),
            )
        ContextCompat.startForegroundService(context, intent)
    }

    fun stop(context: Context) {
        context.stopService(Intent(context, LumoLocationService::class.java))
        context.stopService(Intent(context, LumoControllerService::class.java))
    }

    fun restartIfConfigured(context: Context): Boolean {
        if (!LumoPreferences.isEnabled(context)) return false
        val role = LumoPreferences.role(context) ?: return false
        if (role != ROLE_CONTROLLED && role != ROLE_CONTROLLER) return false
        start(context, role, LumoPreferences.intervalSeconds(context))
        return true
    }
}

internal abstract class LumoForegroundService : Service() {
    protected abstract val role: String
    protected abstract val notificationId: Int
    protected abstract val lumoForegroundServiceType: Int

    private val scheduler = Executors.newSingleThreadScheduledExecutor()
    private var future: ScheduledFuture<*>? = null
    private val immediateTickPending = AtomicBoolean(false)
    private var networkCallback: ConnectivityManager.NetworkCallback? = null
    protected var foregroundStarted = false
        private set

    override fun onCreate() {
        super.onCreate()
        if (!prerequisitesMet()) {
            stopForUnavailablePrerequisites()
            return
        }
        runCatching {
            ServiceCompat.startForeground(
                this,
                notificationId,
                LumoNotifications.foreground(this, role),
                lumoForegroundServiceType,
            )
        }.onSuccess {
            foregroundStarted = true
            registerNetworkRecovery()
        }.onFailure {
            stopForUnavailablePrerequisites()
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!foregroundStarted) return START_NOT_STICKY
        val interval =
            intent?.getLongExtra(
                LumoServiceController.EXTRA_INTERVAL_SECONDS,
                LumoPreferences.intervalSeconds(this),
            ) ?: LumoPreferences.intervalSeconds(this)
        schedule(interval.coerceIn(LumoServiceController.MIN_INTERVAL_SECONDS, LumoServiceController.MAX_INTERVAL_SECONDS))
        return START_STICKY
    }

    override fun onTaskRemoved(rootIntent: Intent?) {
        // START_STICKY is the primary recovery mechanism. Some OEMs do not honour it when the
        // WebView task is dismissed, so request the same foreground service again while this
        // service is still allowed to start foreground work. A force-stop remains intentionally
        // unrecoverable until Android lets the user open the app again.
        if (LumoPreferences.role(this) == role) {
            runCatching { LumoServiceController.restartIfConfigured(this) }
        }
        super.onTaskRemoved(rootIntent)
    }

    override fun onDestroy() {
        future?.cancel(false)
        unregisterNetworkRecovery()
        scheduler.shutdownNow()
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    protected open fun sampleLocation(): Location? = null

    protected fun runTick() {
        if (!prerequisitesMet()) {
            stopForUnavailablePrerequisites()
            return
        }
        LumoTickProcessor.process(applicationContext, role, sampleLocation())
    }

    protected fun requestImmediateTick() {
        if (scheduler.isShutdown || !immediateTickPending.compareAndSet(false, true)) return
        runCatching {
            scheduler.execute {
                try {
                    runCatching(::runTickWhileAwake)
                } finally {
                    immediateTickPending.set(false)
                }
            }
        }.onFailure { immediateTickPending.set(false) }
    }

    protected open fun prerequisitesMet(): Boolean =
        LumoDeviceStatus.notificationsGranted(applicationContext)

    private fun stopForUnavailablePrerequisites() {
        // Keep the user's explicit enabled preference. Android/OEM restrictions and temporarily
        // disabled services can otherwise turn a recoverable interruption into a permanent one.
        stopSelf()
    }

    private fun schedule(intervalSeconds: Long) {
        future?.cancel(false)
        future =
            scheduler.scheduleWithFixedDelay(
                { runCatching(::runTickWhileAwake) },
                0,
                intervalSeconds,
                TimeUnit.SECONDS,
            )
    }

    private fun runTickWhileAwake() {
        val powerManager = getSystemService(Context.POWER_SERVICE) as PowerManager
        val wakeLock =
            powerManager.newWakeLock(
                PowerManager.PARTIAL_WAKE_LOCK,
                "${packageName}:lumo-background-sync",
            )
        wakeLock.setReferenceCounted(false)
        try {
            wakeLock.acquire(TimeUnit.SECONDS.toMillis(60))
            runTick()
        } finally {
            if (wakeLock.isHeld) wakeLock.release()
        }
    }

    private fun registerNetworkRecovery() {
        val manager = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        val callback =
            object : ConnectivityManager.NetworkCallback() {
                override fun onAvailable(network: Network) {
                    requestImmediateTick()
                }
            }
        runCatching { manager.registerDefaultNetworkCallback(callback) }
            .onSuccess { networkCallback = callback }
    }

    private fun unregisterNetworkRecovery() {
        val callback = networkCallback ?: return
        val manager = getSystemService(Context.CONNECTIVITY_SERVICE) as ConnectivityManager
        runCatching { manager.unregisterNetworkCallback(callback) }
        networkCallback = null
    }
}

internal class LumoLocationService : LumoForegroundService(), LocationListener {
    override val role = LumoServiceController.ROLE_CONTROLLED
    override val notificationId = LumoNotifications.LOCATION_FOREGROUND_ID
    override val lumoForegroundServiceType: Int
        get() =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION
            } else {
                0
            }

    @Volatile private var latestLocation: Location? = null
    private lateinit var locationManager: LocationManager

    override fun onCreate() {
        super.onCreate()
        if (!foregroundStarted) return
        locationManager = getSystemService(Context.LOCATION_SERVICE) as LocationManager
        startLocationUpdates()
    }

    override fun onDestroy() {
        if (::locationManager.isInitialized) {
            runCatching { locationManager.removeUpdates(this) }
        }
        super.onDestroy()
    }

    override fun onLocationChanged(location: Location) {
        if (!isFresh(location)) return
        if (latestLocation?.let { current -> isNewer(location, current) } != false) {
            latestLocation = location
            requestImmediateTick()
        }
    }

    override fun sampleLocation(): Location? = latestLocation?.takeIf(::isFresh)

    override fun prerequisitesMet(): Boolean =
        super.prerequisitesMet() &&
            LumoDeviceStatus.preciseLocationGranted(applicationContext)

    private fun startLocationUpdates() {
        if (
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) !=
                PackageManager.PERMISSION_GRANTED
        ) {
            stopSelf()
            return
        }
        val intervalMs = LumoPreferences.intervalSeconds(this) * 1000L
        listOf(
            LocationManager.GPS_PROVIDER,
            LocationManager.NETWORK_PROVIDER,
            LocationManager.PASSIVE_PROVIDER,
        ).forEach { provider ->
            runCatching {
                if (locationManager.isProviderEnabled(provider)) {
                    locationManager.getLastKnownLocation(provider)?.let(::onLocationChanged)
                }
                // Keep the listener registered while the provider is disabled. Android resumes
                // delivery automatically when the user enables location again, without requiring
                // the Lumo UI or a manual "Reactivar seguimiento" action.
                locationManager.requestLocationUpdates(
                    provider,
                    intervalMs,
                    0f,
                    this,
                    Looper.getMainLooper(),
                )
            }
        }
    }

    private fun isFresh(location: Location): Boolean {
        val maxAgeMs = LumoLocationPolicy.maxAgeMs(LumoPreferences.intervalSeconds(this))
        val elapsedRealtimeNanos = location.elapsedRealtimeNanos
        val nowElapsedRealtimeNanos = SystemClock.elapsedRealtimeNanos()
        if (elapsedRealtimeNanos > 0L && elapsedRealtimeNanos <= nowElapsedRealtimeNanos) {
            val ageMs =
                TimeUnit.NANOSECONDS.toMillis(nowElapsedRealtimeNanos - elapsedRealtimeNanos)
            return ageMs <= maxAgeMs
        }
        return LumoLocationPolicy.isFresh(
            sourceTimestampMs = location.time,
            nowMs = System.currentTimeMillis(),
            intervalSeconds = LumoPreferences.intervalSeconds(this),
        )
    }

    private fun isNewer(candidate: Location, current: Location): Boolean {
        val candidateElapsed = candidate.elapsedRealtimeNanos
        val currentElapsed = current.elapsedRealtimeNanos
        return if (candidateElapsed > 0L && currentElapsed > 0L) {
            candidateElapsed >= currentElapsed
        } else {
            candidate.time >= current.time
        }
    }
}

internal class LumoControllerService : LumoForegroundService() {
    override val role = LumoServiceController.ROLE_CONTROLLER
    override val notificationId = LumoNotifications.CONTROLLER_FOREGROUND_ID
    override val lumoForegroundServiceType: Int
        get() =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
            } else {
                0
            }
}
