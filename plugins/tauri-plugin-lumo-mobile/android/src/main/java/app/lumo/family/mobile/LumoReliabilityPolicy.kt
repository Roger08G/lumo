package app.lumo.family.mobile

internal data class LumoTimedEntry<T>(
    val timestampMs: Long,
    val value: T,
)

internal object LumoLocationPolicy {
    private const val MIN_MAX_AGE_MS = 30 * 1000L
    private const val ABSOLUTE_MAX_AGE_MS = 2 * 60 * 1000L

    fun maxAgeMs(intervalSeconds: Long): Long =
        (intervalSeconds.coerceAtLeast(1L) * 2 * 1000L)
            .coerceIn(MIN_MAX_AGE_MS, ABSOLUTE_MAX_AGE_MS)

    fun isFresh(
        sourceTimestampMs: Long,
        nowMs: Long,
        intervalSeconds: Long,
    ): Boolean =
        sourceTimestampMs > 0L &&
            nowMs - sourceTimestampMs in 0..maxAgeMs(intervalSeconds)
}

internal object LumoQueuePolicy {
    const val RETENTION_MS = 24 * 60 * 60 * 1000L

    private const val RECENT_WINDOW_MS = 30 * 60 * 1000L
    private const val RECENT_SAMPLE_MS = 30 * 1000L
    private const val HISTORICAL_SAMPLE_MS = 5 * 60 * 1000L
    private const val MAX_ENTRIES = 384

    /**
     * Keeps detailed samples for the latest 30 minutes and one representative sample every
     * five minutes for the rest of the last 24 hours. The newest value wins inside each bucket,
     * so a long outage remains bounded without collapsing the most useful recent route.
     */
    fun <T> compact(entries: List<LumoTimedEntry<T>>, nowMs: Long): List<LumoTimedEntry<T>> {
        val buckets = linkedMapOf<String, LumoTimedEntry<T>>()
        entries
            .asSequence()
            .filter { entry ->
                entry.timestampMs > 0L && nowMs - entry.timestampMs in 0..RETENTION_MS
            }
            .sortedBy(LumoTimedEntry<T>::timestampMs)
            .forEach { entry ->
                val ageMs = nowMs - entry.timestampMs
                val sampleMs =
                    if (ageMs <= RECENT_WINDOW_MS) RECENT_SAMPLE_MS else HISTORICAL_SAMPLE_MS
                val period = if (ageMs <= RECENT_WINDOW_MS) "recent" else "historical"
                buckets["$period:${entry.timestampMs / sampleMs}"] = entry
            }
        return buckets.values.toList().takeLast(MAX_ENTRIES)
    }
}

internal object LumoQueueCredentialPolicy {
    fun belongsTo(
        groupId: String?,
        deviceId: String?,
        credential: LumoDeviceCredential,
    ): Boolean = groupId == credential.groupId && deviceId == credential.deviceId
}

internal object LumoControlledTrackingPolicy {
    fun mayAutoRecover(configured: Boolean, explicitlyDisabled: Boolean): Boolean =
        configured && !explicitlyDisabled
}

internal enum class LumoRestartAction {
    START,
    SHOW_REOPEN,
    IGNORE,
}

internal object LumoRestartPolicy {
    fun action(
        enabled: Boolean,
        role: String?,
        notificationsGranted: Boolean,
        preciseLocationGranted: Boolean,
        backgroundLocationGranted: Boolean,
    ): LumoRestartAction {
        if (!enabled || !notificationsGranted) return LumoRestartAction.IGNORE
        return when (role) {
            LumoServiceController.ROLE_CONTROLLER -> LumoRestartAction.START
            LumoServiceController.ROLE_CONTROLLED -> {
                if (
                    preciseLocationGranted &&
                        backgroundLocationGranted
                ) {
                    LumoRestartAction.START
                } else {
                    LumoRestartAction.SHOW_REOPEN
                }
            }
            else -> LumoRestartAction.SHOW_REOPEN
        }
    }
}
