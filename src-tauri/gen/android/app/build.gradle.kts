import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("rust")
}

val tauriProperties = Properties().apply {
    val propFile = file("tauri.properties")
    if (propFile.exists()) {
        propFile.inputStream().use { load(it) }
    }
}

// Release signing for sideloaded APKs. keystore.properties is gitignored and
// written by scripts/android-release.sh. Without it, release stays unsigned.
val keystoreProperties: Properties? = rootProject.file("keystore.properties")
    .takeIf { it.isFile }
    ?.let { propFile -> Properties().apply { propFile.inputStream().use { load(it) } } }

fun ndkVersionFromLocalProperties(): String? {
    val local = rootProject.file("local.properties")
    if (!local.exists()) {
        return null
    }
    val props = Properties()
    local.inputStream().use { props.load(it) }
    val ndkDir = props.getProperty("ndk.dir") ?: return null
    val version = file(ndkDir).name
    return if (version.matches(Regex("""\d+\.\d+\.\d+"""))) version else null
}

android {
    compileSdk = 36
    namespace = "io.github.xraybox"
    ndkVersionFromLocalProperties()?.let { ndkVersion = it }
    defaultConfig {
        manifestPlaceholders["usesCleartextTraffic"] = "false"
        applicationId = "io.github.xraybox"
        minSdk = 24
        targetSdk = 36
        versionCode = tauriProperties.getProperty("tauri.android.versionCode", "1").toInt()
        versionName = tauriProperties.getProperty("tauri.android.versionName", "1.0")
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }
    externalNativeBuild {
        ndkBuild {
            path = file("src/main/jni/Android.mk")
        }
    }
    packaging {
        jniLibs {
            useLegacyPackaging = true
        }
    }
    signingConfigs {
        keystoreProperties?.let { properties ->
            create("release") {
                storeFile = rootProject.file(properties.getProperty("storeFile"))
                storePassword = properties.getProperty("storePassword")
                keyAlias = properties.getProperty("keyAlias")
                keyPassword = properties.getProperty("keyPassword")
            }
        }
    }
    buildTypes {
        getByName("debug") {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            signingConfigs.findByName("release")?.let { signingConfig = it }
            isMinifyEnabled = true
            proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

rust {
    rootDirRel = "../../../"
}

dependencies {
    implementation("androidx.webkit:webkit:1.14.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
    implementation("androidx.activity:activity-ktx:1.10.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-process:2.10.0")
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
    androidTestImplementation("androidx.test.ext:junit:1.1.4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.0")
}

apply(from = "tauri.build.gradle.kts")

// Single-ABI flavors only merge the Rust .so. Keep the Xray sidecar
// beside it so arm64 / tauri android dev APKs can exec libxray.so.
afterEvaluate {
    tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach {
        doLast {
            val sidecar = file("src/main/jniLibs/arm64-v8a/libxray.so")
            if (!sidecar.isFile) {
                return@doLast
            }
            outputs.files.forEach { output ->
                val destDir = file(output).resolve("arm64-v8a")
                if (destDir.isDirectory) {
                    copy {
                        from(sidecar)
                        into(destDir)
                    }
                }
            }
        }
    }
}