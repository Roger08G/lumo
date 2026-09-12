package app.lumo.family.mobile

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import androidx.core.content.edit
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.AEADBadTagException
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONArray
import org.json.JSONObject

internal class LumoSecureQueue(private val context: Context) {
    private val preferences = context.getSharedPreferences(PREFERENCES_FILE, Context.MODE_PRIVATE)

    fun read(): List<String> = synchronized(lock) { readLocked() }

    private fun readLocked(): List<String> {
        val encrypted = preferences.getString(KEY_QUEUE, null) ?: return emptyList()
        if (encrypted.length > MAX_QUEUE_CHARS) {
            preferences.edit(commit = true) { remove(KEY_QUEUE) }
            return emptyList()
        }
        val plaintext = runCatching { decrypt(encrypted) }.getOrElse {
            if (it !is AEADBadTagException && it !is IllegalArgumentException) throw it
            preferences.edit(commit = true) { remove(KEY_QUEUE) }
            return emptyList()
        }
        val array = runCatching { JSONArray(plaintext) }.getOrElse {
            preferences.edit(commit = true) { remove(KEY_QUEUE) }
            return emptyList()
        }
        val values = buildList {
            for (index in 0 until array.length()) {
                array.optString(index, null)?.let(::add)
            }
        }
        val now = System.currentTimeMillis()
        val retained = compact(values, now)
        if (retained != values) replace(retained)
        return retained
    }

    fun enqueue(payload: String) = synchronized(lock) {
        replaceLocked(readLocked() + payload)
    }

    fun replace(payloads: List<String>) = synchronized(lock) { replaceLocked(payloads) }

    private fun replaceLocked(payloads: List<String>) {
        if (payloads.isEmpty()) {
            preferences.edit(commit = true) { remove(KEY_QUEUE) }
            return
        }
        val now = System.currentTimeMillis()
        val retained = compact(payloads, now)
        if (retained.isEmpty()) {
            preferences.edit(commit = true) { remove(KEY_QUEUE) }
            return
        }
        val serialized = JSONArray(retained).toString()
        val encrypted = runCatching { encrypt(serialized) }.getOrNull() ?: return
        preferences.edit(commit = true) { putString(KEY_QUEUE, encrypted) }
    }

    private fun compact(payloads: List<String>, nowMs: Long): List<String> =
        LumoQueuePolicy.compact(
            payloads.asSequence().filter { it.length <= MAX_PAYLOAD_CHARS }.mapNotNull { payload ->
                runCatching {
                    LumoTimedEntry(
                        timestampMs = JSONObject(payload).optLong("timestampMs", 0L),
                        value = payload,
                    )
                }.getOrNull()
            }.toList(),
            nowMs,
        ).map(LumoTimedEntry<String>::value)

    private fun encrypt(value: String): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key(create = true))
        val ciphertext = cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        return listOf(cipher.iv, ciphertext)
            .joinToString(SEPARATOR) { Base64.encodeToString(it, Base64.NO_WRAP) }
    }

    private fun decrypt(value: String): String {
        val parts = value.split(SEPARATOR, limit = 2)
        require(parts.size == 2) { "invalid encrypted queue" }
        val iv = Base64.decode(parts[0], Base64.NO_WRAP)
        val ciphertext = Base64.decode(parts[1], Base64.NO_WRAP)
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key(create = false), GCMParameterSpec(128, iv))
        return cipher.doFinal(ciphertext).toString(Charsets.UTF_8)
    }

    private fun key(create: Boolean): SecretKey {
        val keyStore = KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        check(create) { "location queue key is temporarily unavailable" }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEY_STORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return generator.generateKey()
    }

    private companion object {
        val lock = Any()
        const val PREFERENCES_FILE = "lumo_secure_location_queue"
        const val KEY_QUEUE = "pending_ticks"
        const val KEY_ALIAS = "lumo.location.queue.v1"
        const val ANDROID_KEY_STORE = "AndroidKeyStore"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val SEPARATOR = "."
        const val MAX_PAYLOAD_CHARS = 2_048
        const val MAX_QUEUE_CHARS = 2 * 1024 * 1024
    }
}
