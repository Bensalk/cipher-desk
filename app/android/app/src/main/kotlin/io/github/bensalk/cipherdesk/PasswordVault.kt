package io.github.bensalk.cipherdesk

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.UserNotAuthenticatedException
import android.util.Base64
import androidx.biometric.BiometricManager.Authenticators
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Saved passwords on Android (S4, S5, S11).
 *
 * - Each password is encrypted with AES-256-GCM using a key that lives in the
 *   Android Keystore (hardware-backed where available) and never leaves it.
 * - The key can only be used for 30 seconds after the user confirms with
 *   fingerprint, face or the device screen lock, so saving and reading always
 *   ask for confirmation (S5).
 * - Ciphertext is kept in private app storage, which is excluded from backup
 *   and device transfer (S11). Without a screen lock no key can be created and
 *   nothing is saved (I8).
 * - If new biometrics are enrolled, Android invalidates the key; all saved
 *   passwords are then deleted and must be typed again.
 */
class PasswordVault(private val activity: FragmentActivity) {
    companion object {
        private const val KEYSTORE = "AndroidKeyStore"
        private const val KEY_ALIAS = "rdpv_vault_v1"
        private const val PREFS = "rdpv_vault"
        private const val AUTH_SECONDS = 30
        private const val GCM_TAG_BITS = 128
        private val ID_PATTERN = Regex("^[A-Za-z0-9_-]{1,64}$")
    }

    /** Result codes shared with Dart (see lib/platform/password_vault.dart). */
    object Code {
        const val OK = "ok"
        const val NOT_FOUND = "notFound"
        const val UNAVAILABLE = "unavailable"
        const val LOCKED = "locked"
        const val INVALID_ID = "invalidId"
    }

    private val prefs = activity.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

    private fun entryName(id: String) = "connection-$id"

    fun exists(id: String): String {
        if (!ID_PATTERN.matches(id)) return Code.INVALID_ID
        return if (prefs.contains(entryName(id))) Code.OK else Code.NOT_FOUND
    }

    fun delete(id: String): String {
        if (!ID_PATTERN.matches(id)) return Code.INVALID_ID
        prefs.edit().remove(entryName(id)).commit()
        return Code.OK
    }

    /** Encrypts and saves; asks the user to confirm if needed. */
    fun save(id: String, password: String, done: (String) -> Unit) {
        if (!ID_PATTERN.matches(id)) return done(Code.INVALID_ID)
        if (password.isEmpty()) return done(Code.NOT_FOUND)
        withAuthenticatedKey("Save password", done) { key ->
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.ENCRYPT_MODE, key)
            val ct = cipher.doFinal(password.toByteArray(Charsets.UTF_8))
            val blob = cipher.iv + ct
            prefs.edit().putString(entryName(id), Base64.encodeToString(blob, Base64.NO_WRAP)).commit()
            Code.OK
        }
    }

    /** Asks the user to confirm, then decrypts. Calls back with (code, password?). */
    fun load(id: String, done: (String, String?) -> Unit) {
        if (!ID_PATTERN.matches(id)) return done(Code.INVALID_ID, null)
        val stored = prefs.getString(entryName(id), null) ?: return done(Code.NOT_FOUND, null)
        var result: String? = null
        withAuthenticatedKey("Use saved password", { code -> done(code, result) }) { key ->
            val blob = Base64.decode(stored, Base64.NO_WRAP)
            if (blob.size <= 12) return@withAuthenticatedKey Code.NOT_FOUND
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, blob, 0, 12))
            result = String(cipher.doFinal(blob, 12, blob.size - 12), Charsets.UTF_8)
            Code.OK
        }
    }

    /**
     * Runs [op] with the vault key. If the user has not confirmed recently,
     * shows the system prompt first and retries once.
     */
    private fun withAuthenticatedKey(title: String, done: (String) -> Unit, op: (SecretKey) -> String) {
        val key = try {
            getOrCreateKey()
        } catch (e: Exception) {
            // Typically: no secure screen lock set up on this device.
            return done(Code.UNAVAILABLE)
        } ?: return done(Code.UNAVAILABLE)

        fun attempt(): String? = try {
            op(key)
        } catch (e: UserNotAuthenticatedException) {
            null
        } catch (e: KeyPermanentlyInvalidatedException) {
            resetAll()
            Code.UNAVAILABLE
        } catch (e: Exception) {
            Code.UNAVAILABLE
        }

        val first = attempt()
        if (first != null) return done(first)
        prompt(title) { confirmed ->
            if (!confirmed) done(Code.LOCKED) else done(attempt() ?: Code.LOCKED)
        }
    }

    private fun prompt(title: String, result: (Boolean) -> Unit) {
        val allowed = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            Authenticators.BIOMETRIC_STRONG or Authenticators.DEVICE_CREDENTIAL
        } else {
            // Android 10: strong biometrics cannot be combined with the screen lock.
            Authenticators.BIOMETRIC_WEAK or Authenticators.DEVICE_CREDENTIAL
        }
        val info = BiometricPrompt.PromptInfo.Builder()
            .setTitle(title)
            .setSubtitle("Confirm it's you")
            .setAllowedAuthenticators(allowed)
            .build()
        val callback = object : BiometricPrompt.AuthenticationCallback() {
            override fun onAuthenticationSucceeded(r: BiometricPrompt.AuthenticationResult) = result(true)
            override fun onAuthenticationError(code: Int, msg: CharSequence) = result(false)
        }
        BiometricPrompt(activity, ContextCompat.getMainExecutor(activity), callback).authenticate(info)
    }

    private fun getOrCreateKey(): SecretKey? {
        val ks = KeyStore.getInstance(KEYSTORE).apply { load(null) }
        (ks.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val builder = KeyGenParameterSpec.Builder(
            KEY_ALIAS,
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setKeySize(256)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            builder.setUserAuthenticationParameters(
                AUTH_SECONDS,
                KeyProperties.AUTH_BIOMETRIC_STRONG or KeyProperties.AUTH_DEVICE_CREDENTIAL,
            )
        } else {
            @Suppress("DEPRECATION")
            builder.setUserAuthenticationValidityDurationSeconds(AUTH_SECONDS)
        }
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, KEYSTORE)
        gen.init(builder.build())
        return gen.generateKey()
    }

    /** The key was invalidated: delete it and every password encrypted with it. */
    private fun resetAll() {
        try {
            KeyStore.getInstance(KEYSTORE).apply { load(null) }.deleteEntry(KEY_ALIAS)
        } catch (_: Exception) {
        }
        prefs.edit().clear().commit()
    }
}
