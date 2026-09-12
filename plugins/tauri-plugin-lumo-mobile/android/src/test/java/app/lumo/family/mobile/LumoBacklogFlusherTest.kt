package app.lumo.family.mobile

import java.security.KeyStoreException
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class LumoBacklogFlusherTest {
    @Test
    fun unreadableBacklogDefersWithoutDeletingItOrBlockingTheCurrentSample() {
        listOf(KeyStoreException("provider unavailable"), IllegalStateException("key unavailable"))
            .forEach { failure ->
                var persisted = listOf("pending-location")
                val sent = mutableListOf<String>()
                val result = LumoBacklogFlusher.flush(
                    read = { throw failure },
                    send = { sent += it; LumoBackgroundResultKind.SUCCESS },
                    replace = { persisted = it },
                )
                assertEquals(null, result)
                // The processor proceeds to its live invocation when backlog work is deferred.
                if (result == null) sent += "current-location"
                assertEquals(listOf("current-location"), sent)
                assertEquals(listOf("pending-location"), persisted)

                val recovered = LumoBacklogFlusher.flush(
                    read = { persisted },
                    send = { sent += it; LumoBackgroundResultKind.SUCCESS },
                    replace = { persisted = it },
                )
                assertEquals(null, recovered)
                assertEquals(listOf("current-location", "pending-location"), sent)
                assertEquals(emptyList<String>(), persisted)
            }
    }

    @Test
    fun storageWriteFailurePreservesBacklogAndAllowsLiveDelivery() {
        val persisted = listOf("old-location")
        val sent = mutableListOf<String>()
        val result = LumoBacklogFlusher.flush(
            read = { persisted },
            send = { sent += it; LumoBackgroundResultKind.SUCCESS },
            replace = { throw KeyStoreException("temporarily unavailable") },
        )
        assertEquals(null, result)
        assertEquals(listOf("old-location"), persisted)
        assertEquals(listOf("old-location"), sent)
    }

    @Test
    fun revokedCredentialsAndDisabledTrackingRemainTerminal() {
        listOf(
            LumoBackgroundResultKind.CREDENTIAL_REJECTED,
            LumoBackgroundResultKind.TRACKING_DISABLED,
        ).forEach { terminal ->
            var replaced = false
            val sent = mutableListOf<String>()
            val result = LumoBacklogFlusher.flush(
                read = { listOf("first", "second") },
                send = { sent += it; terminal },
                replace = { replaced = true },
            )
            assertEquals(terminal, result)
            assertEquals(listOf("first"), sent)
            assertFalse(replaced)
        }
    }

    @Test
    fun transientNetworkFailureKeepsUnsentEntriesForRetry() {
        var persisted = listOf("delivered", "offline", "later")
        val result = LumoBacklogFlusher.flush(
            read = { persisted },
            send = {
                if (it == "delivered") LumoBackgroundResultKind.SUCCESS
                else LumoBackgroundResultKind.TRANSIENT_FAILURE
            },
            replace = { persisted = it },
        )
        assertEquals(LumoBackgroundResultKind.TRANSIENT_FAILURE, result)
        assertEquals(listOf("offline", "later"), persisted)
    }
}
