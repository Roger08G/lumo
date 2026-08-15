package app.lumo.family.mobile

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.media.AudioAttributes
import android.media.Ringtone
import android.media.RingtoneManager
import android.net.Uri
import android.os.Build
import android.os.IBinder
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.core.content.edit
import org.json.JSONObject

internal data class LumoPendingAlarm(
    val id: String,
    val title: String,
    val body: String,
    val phone: String?,
) {
    fun toBridgeObject(): JSONObject =
        JSONObject()
            .put("id", id)
            .put("title", title)
            .put("body", body)
            .put("phone", phone ?: JSONObject.NULL)
}

internal object LumoEmergencyAlarm {
    private const val FILE = "lumo_emergency_alarm"
    private const val KEY_ID = "id"
    private const val KEY_TITLE = "title"
    private const val KEY_BODY = "body"
    private const val KEY_PHONE = "phone"
    const val NOTIFICATION_ID = 4199
    const val ACTION_START = "app.lumo.family.mobile.ALARM_START"
    const val ACTION_STOP = "app.lumo.family.mobile.ALARM_STOP"

    fun start(context: Context, alarm: LumoPendingAlarm) {
        val current = load(context)
        if (current?.id == alarm.id) return
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit(commit = true) {
            putString(KEY_ID, alarm.id)
            putString(KEY_TITLE, alarm.title.take(120))
            putString(KEY_BODY, alarm.body.take(500))
            putString(KEY_PHONE, alarm.phone)
        }
        ContextCompat.startForegroundService(
            context,
            Intent(context, LumoAlarmService::class.java).setAction(ACTION_START),
        )
    }

    fun stop(context: Context) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit(commit = true) { clear() }
        context.stopService(Intent(context, LumoAlarmService::class.java))
    }

    fun load(context: Context): LumoPendingAlarm? {
        val values = context.getSharedPreferences(FILE, Context.MODE_PRIVATE)
        val id = values.getString(KEY_ID, null)?.takeIf(String::isNotBlank) ?: return null
        val title = values.getString(KEY_TITLE, null)?.takeIf(String::isNotBlank) ?: return null
        return LumoPendingAlarm(
            id = id,
            title = title,
            body = values.getString(KEY_BODY, "").orEmpty(),
            phone = values.getString(KEY_PHONE, null)?.takeIf(String::isNotBlank),
        )
    }

    fun notification(context: Context, alarm: LumoPendingAlarm): Notification {
        LumoNotifications.ensureChannels(context)
        val launchIntent =
            context.packageManager
                .getLaunchIntentForPackage(context.packageName)
                ?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        val open =
            launchIntent?.let {
                PendingIntent.getActivity(
                    context,
                    alarm.id.hashCode(),
                    it,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
                )
            }
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
