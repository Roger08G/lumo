package app.lumo.family.mobile

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.location.Geocoder
import android.media.AudioAttributes
import android.media.Ringtone
import android.media.RingtoneManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.view.Gravity
import android.view.HapticFeedbackConstants
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.Button
import android.widget.LinearLayout
import android.widget.Space
import android.widget.TextView
import android.animation.ValueAnimator
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import androidx.core.graphics.ColorUtils
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import org.json.JSONObject
import java.util.Locale
import java.util.concurrent.TimeUnit

internal data class LumoPendingAlarm(
    val id: String,
    val title: String,
    val body: String,
    val phone: String?,
    val address: String?,
    val latitude: Double?,
    val longitude: Double?,
) {
    fun toBridgeObject(): JSONObject =
        JSONObject()
            .put("id", id)
            .put("title", title)
            .put("body", body)
            .put("phone", phone ?: JSONObject.NULL)
            .put("address", address ?: JSONObject.NULL)
            .put("latitude", latitude ?: JSONObject.NULL)
            .put("longitude", longitude ?: JSONObject.NULL)
}

internal object LumoAlarmPayloadPolicy {
    fun optionalText(value: String?): String? =
        value
            ?.trim()
            ?.takeIf(String::isNotEmpty)
            ?.takeUnless {
                it.equals("null", ignoreCase = true) ||
                    it.equals("undefined", ignoreCase = true)
            }
}

internal object LumoEmergencyAlarm {
    private const val FILE = "lumo_emergency_alarm"
    private const val KEY_ID = "id"
    private const val KEY_TITLE = "title"
    private const val KEY_BODY = "body"
    private const val KEY_PHONE = "phone"
    private const val KEY_ADDRESS = "address"
    private const val KEY_LATITUDE = "latitude"
    private const val KEY_LONGITUDE = "longitude"
    private const val ACK_FILE = "lumo_emergency_alarm_acknowledgements"
    private const val KEY_PENDING_ACK = "pending_ack"
    private const val ACK_PREFIX = "seen:"
    private val ACK_RETENTION_MS = TimeUnit.HOURS.toMillis(24)
    const val NOTIFICATION_ID = 4199
    const val ACTION_START = "app.lumo.family.mobile.ALARM_START"
    const val ACTION_STOP = "app.lumo.family.mobile.ALARM_STOP"

    fun start(context: Context, alarm: LumoPendingAlarm) {
        val normalized =
            alarm.copy(
                phone = LumoAlarmPayloadPolicy.optionalText(alarm.phone),
                address = LumoAlarmPayloadPolicy.optionalText(alarm.address),
            )
        if (wasAcknowledged(context, normalized.id)) return
        val current = load(context)
        if (current?.id == normalized.id) {
            val improved =
                normalized.copy(
                    address = current.address ?: normalized.address,
                    latitude = current.latitude ?: normalized.latitude,
                    longitude = current.longitude ?: normalized.longitude,
                )
            if (
                improved.address != current.address ||
                    improved.latitude != current.latitude ||
                    improved.longitude != current.longitude
            ) {
                persist(context, improved)
            }
            startAlarmService(context)
            return
        }
        persist(context, normalized)
        startAlarmService(context)
    }

    private fun startAlarmService(context: Context) {
        ContextCompat.startForegroundService(
            context,
            Intent(context, LumoAlarmService::class.java).setAction(ACTION_START),
        )
    }

    private fun persist(context: Context, alarm: LumoPendingAlarm) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit(commit = true) {
            putString(KEY_ID, alarm.id)
            putString(KEY_TITLE, alarm.title.take(120))
            putString(KEY_BODY, alarm.body.take(500))
            putString(KEY_PHONE, alarm.phone)
            putString(KEY_ADDRESS, alarm.address)
            alarm.latitude?.let { putLong(KEY_LATITUDE, it.toBits()) } ?: remove(KEY_LATITUDE)
            alarm.longitude?.let { putLong(KEY_LONGITUDE, it.toBits()) } ?: remove(KEY_LONGITUDE)
        }
    }

    fun acknowledge(context: Context, alarmId: String) {
        val now = System.currentTimeMillis()
        val acknowledgements = context.getSharedPreferences(ACK_FILE, Context.MODE_PRIVATE)
        acknowledgements.edit(commit = true) {
            acknowledgements.all.forEach { (key, value) ->
                if (key.startsWith(ACK_PREFIX) && value is Long && now - value > ACK_RETENTION_MS) {
                    remove(key)
                }
            }
            putLong(ACK_PREFIX + alarmId, now)
            putString(KEY_PENDING_ACK, alarmId)
        }
        clearCurrent(context)
        LumoTickProcessor.acknowledgeEmergency(context)
    }

    fun stop(context: Context) {
        load(context)?.id?.let { acknowledge(context, it) } ?: clearCurrent(context)
    }

    fun clear(context: Context) {
        clearCurrent(context)
    }

    private fun clearCurrent(context: Context) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit(commit = true) { clear() }
        context.stopService(Intent(context, LumoAlarmService::class.java))
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as android.app.NotificationManager
        manager.cancel(NOTIFICATION_ID)
    }

    fun pendingAcknowledgement(context: Context): String? =
        context
            .getSharedPreferences(ACK_FILE, Context.MODE_PRIVATE)
            .getString(KEY_PENDING_ACK, null)
            ?.takeIf(String::isNotBlank)

    fun completeAcknowledgement(context: Context, alarmId: String) {
        val preferences = context.getSharedPreferences(ACK_FILE, Context.MODE_PRIVATE)
        if (preferences.getString(KEY_PENDING_ACK, null) == alarmId) {
            preferences.edit(commit = true) { remove(KEY_PENDING_ACK) }
        }
    }

    fun updateAddress(context: Context, alarmId: String, address: String) {
        val normalized = LumoAlarmPayloadPolicy.optionalText(address) ?: return
        if (load(context)?.id != alarmId) return
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit(commit = true) {
            putString(KEY_ADDRESS, normalized.take(240))
        }
    }

    private fun wasAcknowledged(context: Context, alarmId: String): Boolean {
        val acknowledgedAt =
            context
                .getSharedPreferences(ACK_FILE, Context.MODE_PRIVATE)
                .getLong(ACK_PREFIX + alarmId, 0L)
        return acknowledgedAt > 0L && System.currentTimeMillis() - acknowledgedAt <= ACK_RETENTION_MS
    }

    fun load(context: Context): LumoPendingAlarm? {
        val values = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        val id = values.getString(KEY_ID, null)?.takeIf(String::isNotBlank) ?: return null
        val title = values.getString(KEY_TITLE, null)?.takeIf(String::isNotBlank) ?: return null
        return LumoPendingAlarm(
            id = id,
            title = title,
            body = values.getString(KEY_BODY, "").orEmpty(),
            phone = LumoAlarmPayloadPolicy.optionalText(values.getString(KEY_PHONE, null)),
            address = LumoAlarmPayloadPolicy.optionalText(values.getString(KEY_ADDRESS, null)),
            latitude = values.takeIf { it.contains(KEY_LATITUDE) }?.getLong(KEY_LATITUDE, 0L)?.let(Double::fromBits),
            longitude = values.takeIf { it.contains(KEY_LONGITUDE) }?.getLong(KEY_LONGITUDE, 0L)?.let(Double::fromBits),
        )
    }

    fun notification(context: Context, alarm: LumoPendingAlarm): Notification {
        LumoNotifications.ensureChannels(context)
        val open =
            PendingIntent.getActivity(
                context,
                alarm.id.hashCode(),
                Intent(context, LumoAlarmActivity::class.java)
                    .putExtra(LumoAlarmActivity.EXTRA_ALARM_ID, alarm.id)
                    .addFlags(
                        Intent.FLAG_ACTIVITY_NEW_TASK or
                            Intent.FLAG_ACTIVITY_CLEAR_TOP or
                            Intent.FLAG_ACTIVITY_SINGLE_TOP,
                    ),
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )
        val builder =
            NotificationCompat.Builder(context, LumoNotifications.URGENT_CHANNEL_ID)
                .setSmallIcon(context.applicationInfo.icon)
                .setContentTitle(alarm.title)
                .setContentText(alarm.body)
                .setStyle(NotificationCompat.BigTextStyle().bigText(alarm.body))
                .setCategory(NotificationCompat.CATEGORY_ALARM)
                .setPriority(NotificationCompat.PRIORITY_MAX)
                .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
                .setOngoing(true)
                .setAutoCancel(false)
                .setContentIntent(open)
                .setFullScreenIntent(open, true)
        alarm.phone?.let { phone ->
            val call =
                PendingIntent.getActivity(
                    context,
                    phone.hashCode(),
                    Intent(Intent.ACTION_DIAL, Uri.fromParts("tel", phone, null)),
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                )
            builder.addAction(context.applicationInfo.icon, "Llamar", call)
        }
        return builder.build()
    }
}

class LumoAlarmService : Service() {
    private var ringtone: Ringtone? = null
    private var vibrator: Vibrator? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == LumoEmergencyAlarm.ACTION_STOP) {
            stopSelf()
            return START_NOT_STICKY
        }
        val alarm = LumoEmergencyAlarm.load(this)
        if (alarm == null) {
            stopSelf()
            return START_NOT_STICKY
        }
        ServiceCompat.startForeground(
            this,
            LumoEmergencyAlarm.NOTIFICATION_ID,
            LumoEmergencyAlarm.notification(this, alarm),
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
                ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE
            } else {
                0
            },
        )
        startSignals()
        return START_STICKY
    }

    override fun onDestroy() {
        ringtone?.stop()
        vibrator?.cancel()
        ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE)
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    private fun startSignals() {
        if (ringtone?.isPlaying != true) {
            ringtone =
                RingtoneManager.getRingtone(
                    this,
                    RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
                        ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION),
                )?.apply {
                    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) isLooping = true
                    audioAttributes =
                        AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_ALARM)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                            .build()
                    play()
                }
        }
        vibrator =
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                getSystemService(VibratorManager::class.java).defaultVibrator
            } else {
                @Suppress("DEPRECATION")
                getSystemService(Context.VIBRATOR_SERVICE) as Vibrator
            }
        val pattern = longArrayOf(0, 700, 350, 700, 900)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            vibrator?.vibrate(VibrationEffect.createWaveform(pattern, 0))
        } else {
            @Suppress("DEPRECATION")
            vibrator?.vibrate(pattern, 0)
        }
    }
}

class LumoAlarmActivity : Activity() {
    companion object {
        const val EXTRA_ALARM_ID = "alarmId"
        private val COLOR_DANGER_LIGHT = Color.rgb(225, 126, 143)
        private val COLOR_DANGER = Color.rgb(180, 71, 88)
        private val COLOR_DANGER_DARK = Color.rgb(116, 39, 55)
    }

    private var alarm: LumoPendingAlarm? = null
    private lateinit var titleView: TextView
    private lateinit var statusView: TextView
    private lateinit var addressView: TextView
    private lateinit var swipeView: LumoSwipeToStopView
    private lateinit var actionsView: LinearLayout

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        prepareWindow()
        renderAlarm()
    }

    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        setIntent(intent)
        renderAlarm()
    }

    private fun prepareWindow() {
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O_MR1) {
            setShowWhenLocked(true)
            setTurnScreenOn(true)
        } else {
            @Suppress("DEPRECATION")
            window.addFlags(
                WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED or
                    WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON,
            )
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.VANILLA_ICE_CREAM) {
            @Suppress("DEPRECATION")
            window.statusBarColor = Color.TRANSPARENT
            @Suppress("DEPRECATION")
            window.navigationBarColor = COLOR_DANGER_DARK
        }
    }

    private fun renderAlarm() {
        val pending = LumoEmergencyAlarm.load(this)
        if (pending == null || intent?.getStringExtra(EXTRA_ALARM_ID)?.let { it != pending.id } == true) {
            finish()
            return
        }
        alarm = pending

        val root =
            LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER_HORIZONTAL
                setPadding(dp(22), dp(28), dp(22), dp(24))
                background =
                    GradientDrawable(
                        GradientDrawable.Orientation.TL_BR,
                        intArrayOf(COLOR_DANGER_LIGHT, COLOR_DANGER, COLOR_DANGER_DARK),
                    )
            }
        ViewCompat.setOnApplyWindowInsetsListener(root) { view, insets ->
            val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            view.setPadding(dp(22) + bars.left, dp(22) + bars.top, dp(22) + bars.right, dp(22) + bars.bottom)
            insets
        }

        root.addView(
            TextView(this).apply {
                text = getString(R.string.lumo_alarm_eyebrow)
                setTextColor(ColorUtils.setAlphaComponent(Color.WHITE, 215))
                textSize = 12f
                typeface = Typeface.DEFAULT_BOLD
                letterSpacing = .08f
                gravity = Gravity.CENTER
            },
            linearParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT),
        )
        root.addView(Space(this), linearParams(1, 0, 1f))
        root.addView(
            TextView(this).apply {
                text = "♥"
                textSize = 46f
                gravity = Gravity.CENTER
                setTextColor(COLOR_DANGER)
                background = roundedBackground(Color.rgb(255, 241, 244), dp(26).toFloat())
                elevation = dp(8).toFloat()
            },
            linearParams(dp(86), dp(86)).apply { gravity = Gravity.CENTER_HORIZONTAL },
        )
        root.addView(Space(this), linearParams(1, dp(22)))
        statusView =
            TextView(this).apply {
                text = getString(R.string.lumo_alarm_needs_help)
                setTextColor(Color.WHITE)
                textSize = 30f
                typeface = Typeface.DEFAULT_BOLD
                gravity = Gravity.CENTER
            }
        root.addView(statusView, linearParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        titleView =
            TextView(this).apply {
                text = pending.title
                setTextColor(ColorUtils.setAlphaComponent(Color.WHITE, 225))
                textSize = 16f
                gravity = Gravity.CENTER
                setPadding(0, dp(7), 0, 0)
            }
        root.addView(titleView, linearParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        root.addView(Space(this), linearParams(1, dp(24)))

        addressView =
            TextView(this).apply {
                text = displayAddress(pending)
                setTextColor(Color.rgb(55, 43, 57))
                textSize = 15f
                typeface = Typeface.DEFAULT_BOLD
                gravity = Gravity.CENTER
                setPadding(dp(18), dp(17), dp(18), dp(17))
                background = roundedBackground(ColorUtils.setAlphaComponent(Color.WHITE, 242), dp(21).toFloat())
            }
        root.addView(addressView, linearParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))
        root.addView(Space(this), linearParams(1, 0, 1f))

        actionsView = buildActions(pending).apply { visibility = View.GONE }
        root.addView(actionsView, linearParams(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT))

        swipeView =
            LumoSwipeToStopView(this).apply {
                contentDescription = getString(R.string.lumo_alarm_swipe_description)
                onCompleted = { acknowledgeAlarm() }
            }
        root.addView(swipeView, linearParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(76)))
        setContentView(root)
        resolveAddressIfNeeded(pending)
    }

    private fun acknowledgeAlarm() {
        val current = alarm ?: return
        LumoEmergencyAlarm.acknowledge(applicationContext, current.id)
        statusView.setText(R.string.lumo_alarm_stopped)
        titleView.setText(R.string.lumo_alarm_next_action)
        swipeView.visibility = View.GONE
        actionsView.visibility = View.VISIBLE
        actionsView.alpha = 0f
        actionsView.translationY = dp(12).toFloat()
        actionsView.animate().alpha(1f).translationY(0f).setDuration(240L).start()
    }

    private fun buildActions(pending: LumoPendingAlarm): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            addView(
                actionButton(getString(R.string.lumo_alarm_call), primary = true) {
                    pending.phone?.let { phone ->
                        startActivity(Intent(Intent.ACTION_DIAL, Uri.fromParts("tel", phone, null)))
                    }
                }.apply { isEnabled = !pending.phone.isNullOrBlank() },
                linearParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(58)),
            )
            addView(Space(context), linearParams(1, dp(10)))
            addView(
                actionButton(getString(R.string.lumo_alarm_locate), primary = false) { openMap(pending) }.apply {
                    isEnabled = pending.latitude != null && pending.longitude != null
                },
                linearParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(58)),
            )
            addView(Space(context), linearParams(1, dp(8)))
            addView(
                Button(context).apply {
                    setText(R.string.lumo_alarm_close)
                    textSize = 15f
                    setTextColor(Color.WHITE)
                    background = roundedBackground(Color.TRANSPARENT, dp(18).toFloat(), ColorUtils.setAlphaComponent(Color.WHITE, 100))
                    setOnClickListener { finish() }
                },
                linearParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(50)),
            )
        }

    private fun actionButton(label: String, primary: Boolean, action: () -> Unit): Button =
        Button(this).apply {
            text = label
            textSize = 16f
            typeface = Typeface.DEFAULT_BOLD
            isAllCaps = false
            setTextColor(if (primary) COLOR_DANGER_DARK else Color.WHITE)
            background =
                roundedBackground(
                    if (primary) Color.WHITE else ColorUtils.setAlphaComponent(Color.WHITE, 36),
                    dp(18).toFloat(),
                    if (primary) Color.TRANSPARENT else ColorUtils.setAlphaComponent(Color.WHITE, 105),
                )
            setOnClickListener { action() }
        }

    private fun openMap(pending: LumoPendingAlarm) {
        val latitude = pending.latitude ?: return
        val longitude = pending.longitude ?: return
        val label = Uri.encode(pending.address ?: getString(R.string.lumo_alarm_map_label))
        val uri = Uri.parse("geo:$latitude,$longitude?q=$latitude,$longitude($label)")
        runCatching { startActivity(Intent(Intent.ACTION_VIEW, uri)) }
    }

    private fun resolveAddressIfNeeded(pending: LumoPendingAlarm) {
        if (!pending.address.isNullOrBlank()) return
        val latitude = pending.latitude ?: return
        val longitude = pending.longitude ?: return
        LumoAddressResolver.resolve(applicationContext, latitude, longitude) { address ->
            if (address.isNullOrBlank() || isFinishing || alarm?.id != pending.id) return@resolve
            runOnUiThread {
                LumoEmergencyAlarm.updateAddress(applicationContext, pending.id, address)
                alarm = pending.copy(address = address)
                addressView.text = getString(R.string.lumo_alarm_address_exact, address)
            }
        }
    }

    private fun displayAddress(pending: LumoPendingAlarm): String =
        pending.address?.let { getString(R.string.lumo_alarm_address_exact, it) }
            ?: if (pending.latitude != null && pending.longitude != null) {
                getString(
                    R.string.lumo_alarm_address_loading,
                    "%.6f, %.6f".format(Locale.ROOT, pending.latitude, pending.longitude),
                )
            } else {
                getString(R.string.lumo_alarm_address_unavailable)
            }

    private fun roundedBackground(color: Int, radius: Float, stroke: Int = Color.TRANSPARENT): GradientDrawable =
        GradientDrawable().apply {
            setColor(color)
            cornerRadius = radius
            if (stroke != Color.TRANSPARENT) setStroke(dp(1), stroke)
        }

    private fun linearParams(width: Int, height: Int, weight: Float = 0f): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(width, height, weight)

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

}

internal object LumoAddressResolver {
    fun resolve(
        context: Context,
        latitude: Double,
        longitude: Double,
        callback: (String?) -> Unit,
    ) {
        if (!Geocoder.isPresent()) {
            callback(null)
            return
        }
        val geocoder = Geocoder(context, Locale.getDefault())
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            geocoder.getFromLocation(latitude, longitude, 1) { addresses ->
                callback(format(addresses.firstOrNull()))
            }
        } else {
            Thread {
                @Suppress("DEPRECATION")
                val address =
                    runCatching { geocoder.getFromLocation(latitude, longitude, 1)?.firstOrNull() }
                        .getOrNull()
                callback(format(address))
            }.apply {
                name = "lumo-alarm-geocoder"
                isDaemon = true
                start()
            }
        }
    }

    private fun format(address: android.location.Address?): String? =
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
}

internal object LumoAlarmGesturePolicy {
    fun completes(progress: Float): Boolean = progress.isFinite() && progress >= .82f
}

internal class LumoSwipeToStopView(context: Context) : View(context) {
    var onCompleted: (() -> Unit)? = null
    private val density = resources.displayMetrics.density
    private val trackPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = ColorUtils.setAlphaComponent(Color.WHITE, 235) }
    private val progressPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = Color.rgb(250, 220, 226) }
    private val thumbPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.rgb(180, 71, 88)
        setShadowLayer(8f * density, 0f, 3f * density, ColorUtils.setAlphaComponent(Color.BLACK, 55))
    }
    private val labelPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.rgb(90, 67, 79)
        textSize = 14f * density
        typeface = Typeface.create(Typeface.DEFAULT, Typeface.BOLD)
        textAlign = Paint.Align.CENTER
    }
    private val arrowPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        color = Color.WHITE
        textSize = 30f * density
        typeface = Typeface.DEFAULT_BOLD
        textAlign = Paint.Align.CENTER
    }
    private val trackBounds = RectF()
    private val fillBounds = RectF()
    private var progress = 0f
    private var dragging = false
    private var completed = false

    init {
        isFocusable = true
        setLayerType(LAYER_TYPE_SOFTWARE, null)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        val inset = 4f * density
        val track = trackBounds.apply { set(inset, inset, width - inset, height - inset) }
        val radius = track.height() / 2f
        canvas.drawRoundRect(track, radius, radius, trackPaint)
        if (progress > 0f) {
            val fill = fillBounds.apply { set(track.left, track.top, track.left + track.width() * progress, track.bottom) }
            canvas.save()
            canvas.clipRect(fill)
            canvas.drawRoundRect(track, radius, radius, progressPaint)
            canvas.restore()
        }
        val thumbRadius = radius - 5f * density
        val travel = track.width() - 2f * radius
        val thumbX = track.left + radius + travel * progress
        canvas.drawCircle(thumbX, track.centerY(), thumbRadius, thumbPaint)
        canvas.drawText("›", thumbX, track.centerY() - (arrowPaint.ascent() + arrowPaint.descent()) / 2f, arrowPaint)
        val labelStart = track.left + 2f * radius
        val labelCenter = labelStart + (track.right - labelStart) / 2f
        if (progress < .62f) {
            canvas.drawText(
                "Desliza para detener",
                labelCenter,
                track.centerY() - (labelPaint.ascent() + labelPaint.descent()) / 2f,
                labelPaint,
            )
        }
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (completed || !isEnabled) return false
        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                dragging = true
                parent?.requestDisallowInterceptTouchEvent(true)
                updateProgress(event.x)
                return true
            }
            MotionEvent.ACTION_MOVE -> {
                if (!dragging) return false
                updateProgress(event.x)
                return true
            }
            MotionEvent.ACTION_UP -> {
                if (!dragging) return false
                dragging = false
                parent?.requestDisallowInterceptTouchEvent(false)
                if (LumoAlarmGesturePolicy.completes(progress)) {
                    completed = true
                    progress = 1f
                    val feedback =
                        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                            HapticFeedbackConstants.CONFIRM
                        } else {
                            HapticFeedbackConstants.LONG_PRESS
                        }
                    performHapticFeedback(feedback)
                    invalidate()
                    onCompleted?.invoke()
                } else {
                    animateBack()
                }
                performClick()
                return true
            }
            MotionEvent.ACTION_CANCEL -> {
                dragging = false
                parent?.requestDisallowInterceptTouchEvent(false)
                animateBack()
                return true
            }
        }
        return super.onTouchEvent(event)
    }

    override fun performClick(): Boolean {
        super.performClick()
        return true
    }

    private fun updateProgress(x: Float) {
        val inset = 4f * density
        val radius = (height - 2f * inset) / 2f
        val travel = (width - 2f * inset - 2f * radius).coerceAtLeast(1f)
        progress = ((x - inset - radius) / travel).coerceIn(0f, 1f)
        invalidate()
    }

    private fun animateBack() {
        ValueAnimator.ofFloat(progress, 0f).apply {
            duration = 220L
            addUpdateListener {
                progress = it.animatedValue as Float
                invalidate()
            }
            start()
        }
    }
}
