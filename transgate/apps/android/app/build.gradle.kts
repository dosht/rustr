plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.transgate.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.transgate.android"
        // MediaStore.Downloads (the save port) needs API 29.
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        // The host a build talks to, e.g. for Waydroid (the host is its gateway):
        //   gradle assembleDebug -Ptransgate.apiUrl=http://192.168.240.1:8080/api \
        //     -Ptransgate.authUrl=http://192.168.240.1:54321/auth/v1 -Ptransgate.authKey=<anon key> \
        //     -Ptransgate.webUrl=http://192.168.240.1:8080
        fun prop(name: String, fallback: String = "") = "\"" + (project.findProperty(name) as String? ?: fallback) + "\""
        buildConfigField("String", "TRANSGATE_API_URL", prop("transgate.apiUrl"))
        buildConfigField("String", "TRANSGATE_AUTH_URL", prop("transgate.authUrl"))
        buildConfigField("String", "TRANSGATE_AUTH_KEY", prop("transgate.authKey"))
        // The web app, for the browser handoffs of invoices and API keys (D6).
        buildConfigField("String", "TRANSGATE_WEB_URL", prop("transgate.webUrl"))
        // Where GoTrue returns the Custom Tab (D7); the host must allow-list it.
        buildConfigField("String", "SITE_URL", prop("transgate.siteUrl", "dev.transgate.app://auth"))
        manifestPlaceholders["authScheme"] = "dev.transgate.app"
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    // D10: the document fonts and the invoice logo ship inside the app, from the one copy the
    // core's export crate owns, never duplicated here.
    sourceSets["main"].assets.srcDirs("../../../export/fonts", "../../../export/assets")
    // Both asset folders carry a provenance.json; it is licensing bookkeeping, not a bundled asset.
    // The OFL license texts next to the fonts do ship, as the licenses require.
    androidResources { ignoreAssetsPattern = "!provenance.json" }
    testOptions { animationsDisabled = true }
}

// Any JDK 17+ builds it; the bytecode targets 17 either way.
kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }

// The Rust core is built by scripts/build-transgate-android.sh (cargo-ndk) into src/main/jniLibs
// and is not a Gradle task: it needs the NDK and a long cross build.

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.09.01")
    implementation(composeBom)
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("androidx.activity:activity-compose:1.11.0")
    implementation("androidx.browser:browser:1.9.0")
    implementation("androidx.core:core-ktx:1.17.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.4")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.4")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("net.java.dev.jna:jna:5.18.1@aar")

    androidTestImplementation("androidx.test:runner:1.7.0")
    androidTestImplementation("androidx.test.ext:junit:1.3.0")
    androidTestImplementation("androidx.test.uiautomator:uiautomator:2.3.0")
}
