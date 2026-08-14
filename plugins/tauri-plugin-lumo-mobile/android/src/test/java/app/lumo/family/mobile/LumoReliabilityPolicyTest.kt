package app.lumo.family.mobile

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LumoReliabilityPolicyTest {
    @Test
    fun locationFreshnessScalesWithIntervalAndNeverExceedsTwoMinutes() {
        assertEquals(30_000L, LumoLocationPolicy.maxAgeMs(15))
        assertEquals(60_000L, LumoLocationPolicy.maxAgeMs(30))
        assertEquals(120_000L, LumoLocationPolicy.maxAgeMs(60))
        assertEquals(120_000L, LumoLocationPolicy.maxAgeMs(900))

        assertTrue(LumoLocationPolicy.isFresh(940_000L, 1_000_000L, 30))
        assertFalse(LumoLocationPolicy.isFresh(939_999L, 1_000_000L, 30))
        assertFalse(LumoLocationPolicy.isFresh(1_000_001L, 1_000_000L, 30))
        assertFalse(LumoLocationPolicy.isFresh(0L, 1_000_000L, 30))
    }

    @Test
    fun queueCoversOneDayWhileCompactingRawThirtySecondSamples() {
        val start = 1_000_000L
        val now = start + LumoQueuePolicy.RETENTION_MS
        val raw =
            (0..2_880).map { index ->
                val timestamp = start + index * 30_000L
                LumoTimedEntry(timestamp, "sample-$index")
            }

        val compacted = LumoQueuePolicy.compact(raw, now)

        assertTrue(compacted.size <= 384)
        assertTrue(
            now - compacted.first().timestampMs >=
                LumoQueuePolicy.RETENTION_MS - 5 * 60 * 1000L,
        )
        assertEquals(now, compacted.last().timestampMs)
        assertTrue(compacted.count { now - it.timestampMs <= 30 * 60 * 1000L } >= 60)
        assertTrue(compacted.zipWithNext().all { (left, right) -> left.timestampMs < right.timestampMs })
    }

    @Test
    fun queueKeepsNewestValuePerBucketAndDropsInvalidTimes() {
        val now = 10_020_000L
        val entries =
            listOf(
                LumoTimedEntry(0L, "invalid"),
                LumoTimedEntry(now - LumoQueuePolicy.RETENTION_MS - 1L, "expired"),
                LumoTimedEntry(now - 1_000L, "old-in-bucket"),
                LumoTimedEntry(now - 500L, "new-in-bucket"),
                LumoTimedEntry(now + 1L, "future"),
            )

        assertEquals(
            listOf("new-in-bucket"),
            LumoQueuePolicy.compact(entries, now).map(LumoTimedEntry<String>::value),
        )
    }

    @Test
    fun restartPolicyRestartsControllerWithoutLocationPermissions() {
        assertEquals(
            LumoRestartAction.START,
            LumoRestartPolicy.action(
                enabled = true,
                role = "controller",
                notificationsGranted = true,
                preciseLocationGranted = false,
                backgroundLocationGranted = false,
                locationServicesEnabled = false,
            ),
        )
    }

    @Test
    fun restartPolicyRequiresAllControlledLocationPrerequisites() {
        assertEquals(
            LumoRestartAction.START,
            controlledRestartAction(),
        )
        assertEquals(
            LumoRestartAction.SHOW_REOPEN,
            controlledRestartAction(backgroundLocationGranted = false),
        )
        assertEquals(
            LumoRestartAction.SHOW_REOPEN,
            controlledRestartAction(locationServicesEnabled = false),
        )
        assertEquals(
            LumoRestartAction.IGNORE,
            controlledRestartAction(enabled = false),
        )
        assertEquals(
            LumoRestartAction.IGNORE,
            controlledRestartAction(notificationsGranted = false),
        )
    }

    private fun controlledRestartAction(
        enabled: Boolean = true,
        notificationsGranted: Boolean = true,
        backgroundLocationGranted: Boolean = true,
        locationServicesEnabled: Boolean = true,
    ): LumoRestartAction =
        LumoRestartPolicy.action(
            enabled = enabled,
            role = "controlled",
            notificationsGranted = notificationsGranted,
            preciseLocationGranted = true,
            backgroundLocationGranted = backgroundLocationGranted,
            locationServicesEnabled = locationServicesEnabled,
        )
}
