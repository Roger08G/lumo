package app.lumo.family.mobile

import java.io.File
import javax.crypto.spec.SecretKeySpec
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class LumoDeviceCredentialTest {
    @Test
    fun credentialPolicyAcceptsOnlyTheVersionedHttpsWireContract() {
        assertTrue(LumoCredentialPolicy.isValid(credential()))
        assertFalse(LumoCredentialPolicy.isValid(credential(apiOrigin = "http://api.example.test")))
        assertFalse(
            LumoCredentialPolicy.isValid(
                credential(apiOrigin = "https://user:password@api.example.test"),
            ),
        )
        assertFalse(
            LumoCredentialPolicy.isValid(
                credential(apiOrigin = "https://api.example.test/v1"),
            ),
        )
        assertFalse(LumoCredentialPolicy.isValid(credential(role = "debug")))
        assertFalse(LumoCredentialPolicy.isValid(credential(deviceToken = "too-short")))
        assertFalse(LumoCredentialPolicy.isValid(credential(stateKey = "contains whitespace")))
    }

    @Test
    fun encryptedEnvelopeRoundTripsWithoutContainingPlaintext() {
        val plaintext =
            """{"deviceToken":"token-that-must-stay-private-1234567890"}"""
                .toByteArray(Charsets.UTF_8)
        val key = SecretKeySpec(ByteArray(32) { index -> (index + 1).toByte() }, "AES")

        val envelope = LumoCredentialCipher.encrypt(plaintext, key)

        assertTrue(envelope.startsWith("v1:"))
        assertFalse(envelope.contains("token-that-must-stay-private"))
        assertEquals(plaintext.toList(), LumoCredentialCipher.decrypt(envelope, key).toList())
    }

    @Test
    fun encryptedEnvelopeRejectsTampering() {
        val plaintext = "sensitive credential".toByteArray(Charsets.UTF_8)
        val key = SecretKeySpec(ByteArray(32) { index -> (index + 1).toByte() }, "AES")
        val envelope = LumoCredentialCipher.encrypt(plaintext, key)
        val replacement = if (envelope.last() == '0') '1' else '0'
        val tampered = envelope.dropLast(1) + replacement

        assertThrows(Exception::class.java) {
            LumoCredentialCipher.decrypt(tampered, key)
        }
    }

    @Test
    fun onlyStructuredCredentialErrorsDisableBackgroundTracking() {
        listOf("authentication_failed", "credential_invalid", "credential_revoked").forEach {
            errorCode ->
            assertEquals(
                LumoBackgroundResultKind.CREDENTIAL_REJECTED,
                LumoBackgroundErrorPolicy.classify(errorCode, hasError = true),
            )
        }
        listOf(null, "timeout", "remote_server_error", "service_unavailable").forEach {
            errorCode ->
            assertEquals(
                LumoBackgroundResultKind.TRANSIENT_FAILURE,
                LumoBackgroundErrorPolicy.classify(errorCode, hasError = true),
            )
        }
        assertEquals(
            LumoBackgroundResultKind.TRACKING_DISABLED,
            LumoBackgroundErrorPolicy.classify("tracking_disabled", hasError = true),
        )
        assertEquals(
            LumoBackgroundResultKind.SUCCESS,
            LumoBackgroundErrorPolicy.classify(errorCode = null, hasError = false),
        )
    }

    @Test
    fun manifestAndExtractionRulesExcludeAllPrivateCredentialStorage() {
        val projectDir = File(requireNotNull(System.getProperty("lumo.plugin.projectDir")))
        val manifest = projectDir.resolve("src/main/AndroidManifest.xml").readText()
        val legacy = projectDir.resolve("src/main/res/xml/lumo_backup_rules.xml").readText()
        val extraction =
            projectDir.resolve("src/main/res/xml/lumo_data_extraction_rules.xml").readText()

        assertTrue(manifest.contains("android:allowBackup=\"false\""))
        assertTrue(manifest.contains("android.permission.USE_FULL_SCREEN_INTENT"))
        assertTrue(manifest.contains("android:name=\".LumoAlarmActivity\""))
        assertTrue(manifest.contains("android:showWhenLocked=\"true\""))
        assertTrue(manifest.contains("android:turnScreenOn=\"true\""))
        assertTrue(manifest.contains("android:fullBackupContent=\"@xml/lumo_backup_rules\""))
        assertTrue(
            manifest.contains(
                "android:dataExtractionRules=\"@xml/lumo_data_extraction_rules\"",
            ),
        )
        val domains =
            listOf(
                "root",
                "file",
                "database",
                "sharedpref",
                "external",
                "device_root",
                "device_file",
                "device_database",
                "device_sharedpref",
            )
        domains.forEach { domain ->
            val rule = "<exclude domain=\"$domain\" path=\".\" />"
            assertTrue("legacy backup must exclude $domain", legacy.contains(rule))
            assertEquals(
                "cloud and device transfer must exclude $domain",
                2,
                extraction.windowed(rule.length).count { it == rule },
            )
        }
    }

    private fun credential(
        apiOrigin: String = "https://api.example.test",
        role: String = "controlled",
        deviceToken: String = "A".repeat(43),
        stateKey: String = "B".repeat(43),
    ): LumoDeviceCredential =
        LumoDeviceCredential(
            version = 1,
            apiOrigin = apiOrigin,
            groupId = "0f4dc689-1c52-4189-bd5a-393dc0c655bf",
            deviceId = "0cfbbc0d-9b61-44f5-a307-7869bd42af59",
            role = role,
            deviceToken = deviceToken,
            stateKey = stateKey,
        )
}
