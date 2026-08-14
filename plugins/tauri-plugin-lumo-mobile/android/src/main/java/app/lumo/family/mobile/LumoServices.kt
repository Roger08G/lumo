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
import android.os.Build
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import java.util.concurrent.Executors
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit

internal object LumoServiceController {
    const val ROLE_CONTROLLED = "controlled"
    const val ROLE_CONTROLLER = "controller"
    const val MIN_INTERVAL_SECONDS = 15L
    const val MAX_INTERVAL_SECONDS = 900L
    const val DEFAULT_INTERVAL_SECONDS = 30L
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
}

internal abstract class LumoForegroundService : Service() {
    protected abstract val role: String
    protected abstract val notificationId: Int
    protected abstract val lumoForegroundServiceType: Int

    private val scheduler = Executors.newSingleThreadScheduledExecutor()
    private var future: ScheduledFuture<*>? = null
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
        // Keep the explicitly enabled foreground service alive when the UI task is dismissed.
        super.onTaskRemoved(rootIntent)
    }

    override fun onDestroy() {
        future?.cancel(false)
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
                { runCatching(::runTick) },
                0,
                intervalSeconds,
                TimeUnit.SECONDS,
            )
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
        }
    }

    override fun sampleLocation(): Location? = latestLocation?.takeIf(::isFresh)

    override fun prerequisitesMet(): Boolean =
        super.prerequisitesMet() &&
            LumoDeviceStatus.preciseLocationGranted(applicationContext) &&
            LumoDeviceStatus.locationServicesEnabled(applicationContext)

    private fun startLocationUpdates() {
        if (
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) !=
                PackageManager.PERMISSION_GRANTED
        ) {
            stopSelf()
            return
        }
        val intervalMs = LumoPreferences.intervalSeconds(this) * 1000L
        listOf(LocationManager.GPS_PROVIDER, LocationManager.NETWORK_PROVIDER).forEach { provider ->
            runCatching {
                if (locationManager.isProviderEnabled(provider)) {
                    locationManager.getLastKnownLocation(provider)?.let(::onLocationChanged)
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
