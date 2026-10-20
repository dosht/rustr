plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "dev.taskr.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.taskr.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }

        // Signed-in mode: gradle assembleDebug -Ptaskr.apiUrl=http://192.168.240.1:8787/api
        // (Waydroid reaches the host at its gateway). Leave it empty for local tasks.
        fun prop(name: String) = "\"" + (project.findProperty(name) as String? ?: "") + "\""
        buildConfigField("String", "TASKR_API_URL", prop("taskr.apiUrl"))
        buildConfigField("String", "TASKR_AUTH_URL", prop("taskr.authUrl"))
        buildConfigField("String", "TASKR_AUTH_KEY", prop("taskr.authKey"))
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

// Any JDK 17+ builds it; the bytecode targets 17 either way.
kotlin { compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) } }

// The Rust core is built by scripts/build-android.sh (cargo-ndk) into
// src/main/jniLibs, and the Kotlin bindings into src/main/java/dev/taskr/core.
val buildRustCore by tasks.registering(Exec::class) {
    workingDir = rootProject.projectDir.resolve("../..")
    commandLine("scripts/build-android.sh")
}
tasks.named("preBuild") { dependsOn(buildRustCore) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.09.01")
    implementation(composeBom)
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("androidx.activity:activity-compose:1.11.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.9.4")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.4")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("net.java.dev.jna:jna:5.18.1@aar")
}
