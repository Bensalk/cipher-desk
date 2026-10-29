package io.github.securerdpviewer.app

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Bundle
import android.os.PersistableBundle
import android.view.WindowManager
import io.flutter.embedding.android.FlutterFragmentActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

// FlutterFragmentActivity (not FlutterActivity) is required by the system
// biometric prompt used by the password vault.
class MainActivity : FlutterFragmentActivity() {
    private lateinit var vault: PasswordVault

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // S10: block screenshots, screen recording and the recent-apps thumbnail by default.
        setSecure(true)
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val messenger = flutterEngine.dartExecutor.binaryMessenger
        vault = PasswordVault(this)

        // Lets the Settings screen turn screenshot blocking on or off.
        MethodChannel(messenger, "rdpv/secure_window").setMethodCallHandler { call, result ->
            when (call.method) {
                "setSecure" -> {
                    val enabled = call.arguments as? Boolean
                    if (enabled == null) {
                        result.error("bad_args", "Expected a boolean", null)
                    } else {
                        setSecure(enabled)
                        result.success(null)
                    }
                }
                else -> result.notImplemented()
            }
        }

        // Text copied on the remote computer, marked sensitive (S6).
        MethodChannel(messenger, "rdpv/clipboard").setMethodCallHandler { call, result ->
            if (call.method != "setSensitiveText") return@setMethodCallHandler result.notImplemented()
            val text = call.arguments as? String
                ?: return@setMethodCallHandler result.error("bad_args", "Expected text", null)
            val clip = ClipData.newPlainText("Remote computer", text)
            val extras = PersistableBundle()
            val key = if (Build.VERSION.SDK_INT >= 33) {
                ClipDescription.EXTRA_IS_SENSITIVE
            } else {
                "android.content.extra.IS_SENSITIVE"
            }
            extras.putBoolean(key, true)
            clip.description.extras = extras
            val cm = getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            cm.setPrimaryClip(clip)
            result.success(null)
        }

        // Saved passwords in the Android Keystore (S4, S5).
        MethodChannel(messenger, "rdpv/vault").setMethodCallHandler { call, result ->
            val id = call.argument<String>("id")
            if (id == null) {
                result.error("bad_args", "Expected an id", null)
                return@setMethodCallHandler
            }
            when (call.method) {
                "save" -> {
                    val password = call.argument<String>("password")
                    if (password == null) {
                        result.error("bad_args", "Expected a password", null)
                    } else {
                        vault.save(id, password) { code -> result.success(mapOf("code" to code)) }
                    }
                }
                "load" -> vault.load(id) { code, password ->
                    result.success(mapOf("code" to code, "password" to password))
                }
                "exists" -> result.success(mapOf("code" to vault.exists(id)))
                "delete" -> result.success(mapOf("code" to vault.delete(id)))
                else -> result.notImplemented()
            }
        }
    }

    private fun setSecure(enabled: Boolean) {
        if (enabled) {
            window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        } else {
            window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
        }
    }
}
