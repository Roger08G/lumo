package app.lumo.family.mobile

import android.annotation.SuppressLint
import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import app.tauri.annotation.InvokeArg
import app.tauri.plugin.JSObject
import java.net.URI
import java.security.KeyStore
import java.util.UUID
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject

@InvokeArg
internal class DeviceCredentialArgs {
    var version: Int = 0
    lateinit var apiOrigin: String
    lateinit var groupId: String
    lateinit var deviceId: String
    lateinit var role: String
    lateinit var deviceToken: String
    lateinit var stateKey: String
}

internal class LumoDeviceCredential(
    val version: Int,
    val apiOrigin: String,
    val groupId: String,
    val deviceId: String,
    val role: String,
    val deviceToken: String,
    val stateKey: String,
) {
    fun samePrincipal(other: LumoDeviceCredential): Boolean =
        version == other.version &&
            apiOrigin == other.apiOrigin &&
            groupId == other.groupId &&
            deviceId == other.deviceId &&
            role == other.role

    fun toJson(): JSONObject =
        JSONObject()
            .put("version", version)
            .put("apiOrigin", apiOrigin)
            .put("groupId", groupId)
            .put("deviceId", deviceId)
            .put("role", role)
            .put("deviceToken", deviceToken)
            .put("stateKey", stateKey)

    fun toBridgeObject(): JSObject =
        JSObject()
            .put("version", version)
            .put("apiOrigin", apiOrigin)
            .put("groupId", groupId)
            .put("deviceId", deviceId)
            .put("role", role)
            .put("deviceToken", deviceToken)
            .put("stateKey", stateKey)

    companion object {
        fun fromArgs(args: DeviceCredentialArgs): LumoDeviceCredential? =
            runCatching {
                LumoDeviceCredential(
                    version = args.version,
                    apiOrigin = args.apiOrigin,
                    groupId = args.groupId,
                    deviceId = args.deviceId,
                    role = args.role,
                    deviceToken = args.deviceToken,
                    stateKey = args.stateKey,
                )
            }.getOrNull()?.takeIf(LumoCredentialPolicy::isValid)

        fun fromJson(json: JSONObject): LumoDeviceCredential? =
            runCatching {
                LumoDeviceCredential(
                    version = json.getInt("version"),
                    apiOrigin = json.getString("apiOrigin"),
                    groupId = json.getString("groupId"),
                    deviceId = json.getString("deviceId"),
                    role = json.getString("role"),
                    deviceToken = json.getString("deviceToken"),
                    stateKey = json.getString("stateKey"),
                )
            }.getOrNull()?.takeIf(LumoCredentialPolicy::isValid)
    }
}

internal object LumoCredentialPolicy {
    const val VERSION = 1
    const val MAX_PLAINTEXT_BYTES = 16 * 1024

    private val urlSafeSecret = Regex("^[A-Za-z0-9_-]{43}$")

    fun isValid(credential: LumoDeviceCredential): Boolean =
        credential.version == VERSION &&
            isHttpsOrigin(credential.apiOrigin) &&
            isUuid(credential.groupId) &&
            isUuid(credential.deviceId) &&
            credential.role in
                setOf(
                    LumoServiceController.ROLE_CONTROLLER,
                    LumoServiceController.ROLE_CONTROLLED,
                ) &&
            urlSafeSecret.matches(credential.deviceToken) &&
            urlSafeSecret.matches(credential.stateKey)

    private fun isHttpsOrigin(value: String): Boolean {
        val uri = runCatching { URI(value) }.getOrNull() ?: return false
        return uri.scheme.equals("https", ignoreCase = true) &&
            !uri.host.isNullOrBlank() &&
            uri.userInfo == null &&
            (uri.rawPath.isNullOrEmpty() || uri.rawPath == "/") &&
            uri.rawQuery == null &&
            uri.rawFragment == null &&
            (uri.port == -1 || uri.port in 1..65_535)
    }

    private fun isUuid(value: String): Boolean =
        runCatching { UUID.fromString(value).toString().equals(value, ignoreCase = true) }
            .getOrDefault(false)
}

internal object LumoCredentialCipher {
    private const val TRANSFORMATION = "AES/GCM/NoPadding"
    private const val PREFIX = "v1"
    private const val IV_BYTES = 12
    private const val TAG_BYTES = 16
    private val associatedData = "app.lumo.family|device-credential|v1".toByteArray(Charsets.UTF_8)

    fun encrypt(plaintext: ByteArray, key: SecretKey): String {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key)
        cipher.updateAAD(associatedData)
        val ciphertext = cipher.doFinal(plaintext)
        return "$PREFIX:${encodeHex(cipher.iv)}:${encodeHex(ciphertext)}"
    }

    fun decrypt(envelope: String, key: SecretKey): ByteArray {
        val parts = envelope.split(':', limit = 3)
        require(parts.size == 3 && parts[0] == PREFIX) { "invalid credential envelope" }
        val iv = decodeHex(parts[1])
        val ciphertext = decodeHex(parts[2])
        require(iv.size == IV_BYTES && ciphertext.size >= TAG_BYTES) {
            "invalid credential envelope"
        }
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, iv))
        cipher.updateAAD(associatedData)
        return cipher.doFinal(ciphertext)
    }

    private fun encodeHex(bytes: ByteArray): String {
        val alphabet = "0123456789abcdef"
        return CharArray(bytes.size * 2).also { output ->
            bytes.forEachIndexed { index, byte ->
                val value = byte.toInt() and 0xff
                output[index * 2] = alphabet[value ushr 4]
                output[index * 2 + 1] = alphabet[value and 0x0f]
            }
        }.concatToString()
    }

    private fun decodeHex(value: String): ByteArray {
        require(value.length % 2 == 0) { "invalid credential envelope" }
        return ByteArray(value.length / 2) { index ->
            val high = Character.digit(value[index * 2], 16)
            val low = Character.digit(value[index * 2 + 1], 16)
            require(high >= 0 && low >= 0) { "invalid credential envelope" }
            ((high shl 4) or low).toByte()
        }
    }
}

internal object LumoCredentialVault {
    private const val PREFERENCES_FILE = "lumo_secure_device_credential"
    private const val KEY_CREDENTIAL = "credential_v1"
    private const val KEY_ALIAS = "lumo.device.credential.v1"
    private const val ANDROID_KEY_STORE = "AndroidKeyStore"
    private const val MAX_ENVELOPE_CHARS = LumoCredentialPolicy.MAX_PLAINTEXT_BYTES * 2 + 128
    private val lock = Any()

    @SuppressLint("ApplySharedPref", "UseKtx")
    fun store(context: Context, credential: LumoDeviceCredential): Boolean =
        synchronized(lock) {
            if (!LumoCredentialPolicy.isValid(credential)) return@synchronized false
            val plaintext = credential.toJson().toString().toByteArray(Charsets.UTF_8)
            try {
                if (plaintext.size > LumoCredentialPolicy.MAX_PLAINTEXT_BYTES) {
                    return@synchronized false
                }
                val envelope =
                    runCatching { LumoCredentialCipher.encrypt(plaintext, key()) }.getOrNull()
                        ?: return@synchronized false
                context.getSharedPreferences(PREFERENCES_FILE, Context.MODE_PRIVATE)
                    .edit()
                    .putString(KEY_CREDENTIAL, envelope)
                    .commit()
            } finally {
                plaintext.fill(0)
            }
        }

    fun load(context: Context): LumoDeviceCredential? =
        synchronized(lock) {
            val preferences =
                context.getSharedPreferences(PREFERENCES_FILE, Context.MODE_PRIVATE)
            val envelope = preferences.getString(KEY_CREDENTIAL, null) ?: return@synchronized null
            if (envelope.length > MAX_ENVELOPE_CHARS) {
                clearLocked(context)
                return@synchronized null
            }
            val plaintext =
                runCatching { LumoCredentialCipher.decrypt(envelope, key()) }.getOrElse {
                    clearLocked(context)
                    return@synchronized null
                }
            try {
                if (plaintext.size > LumoCredentialPolicy.MAX_PLAINTEXT_BYTES) {
                    clearLocked(context)
                    return@synchronized null
                }
                val credential =
                    runCatching {
                        LumoDeviceCredential.fromJson(
                            JSONObject(plaintext.toString(Charsets.UTF_8)),
                        )
                    }.getOrNull()
                if (credential == null) clearLocked(context)
                credential
            } finally {
                plaintext.fill(0)
            }
        }

    fun clear(context: Context): Boolean = synchronized(lock) { clearLocked(context) }

    @SuppressLint("ApplySharedPref", "UseKtx")
    private fun clearLocked(context: Context): Boolean {
        val removed =
            context.getSharedPreferences(PREFERENCES_FILE, Context.MODE_PRIVATE)
                .edit()
                .remove(KEY_CREDENTIAL)
                .commit()
        runCatching {
            val keyStore = KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }
            if (keyStore.containsAlias(KEY_ALIAS)) keyStore.deleteEntry(KEY_ALIAS)
        }
        return removed
    }

    private fun key(): SecretKey {
        val keyStore = KeyStore.getInstance(ANDROID_KEY_STORE).apply { load(null) }
        (keyStore.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEY_STORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .setUserAuthenticationRequired(false)
                .build(),
        )
        return generator.generateKey()
    }
}
