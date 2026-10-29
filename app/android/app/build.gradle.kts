import java.io.FileInputStream
import java.util.Properties

plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "io.github.bensalk.cipherdesk"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "io.github.bensalk.cipherdesk"
        // Android 10+ only: blocks background apps from reading the clipboard (decision 7).
        minSdk = 29
        targetSdk = flutter.targetSdkVersion
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    // Release signing (S12): the keystore is never in the repository. CI provides it from
    // GitHub Actions secrets through environment variables; developers may use an
    // untracked android/key.properties file. Without either, release builds use the debug
    // key and must NOT be published.
    val keyProps = Properties().apply {
        val f = rootProject.file("key.properties")
        if (f.exists()) FileInputStream(f).use { load(it) }
    }
    fun signingValue(env: String, prop: String): String? =
        System.getenv(env)?.takeIf { it.isNotBlank() } ?: keyProps.getProperty(prop)
    val releaseStoreFile = signingValue("RDPV_KEYSTORE_PATH", "storeFile")

    signingConfigs {
        if (releaseStoreFile != null) {
            create("release") {
                storeFile = file(releaseStoreFile)
                storePassword = signingValue("RDPV_KEYSTORE_PASSWORD", "storePassword")
                keyAlias = signingValue("RDPV_KEY_ALIAS", "keyAlias")
                keyPassword = signingValue("RDPV_KEY_PASSWORD", "keyPassword")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = if (releaseStoreFile != null) {
                signingConfigs.getByName("release")
            } else {
                signingConfigs.getByName("debug")
            }
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}

dependencies {
    // System fingerprint / face / screen-lock prompt for the password vault (S5).
    implementation("androidx.biometric:biometric:1.1.0")
}
