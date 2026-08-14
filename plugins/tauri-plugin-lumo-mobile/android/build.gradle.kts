plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "app.lumo.family.mobile"
    compileSdk = 36

    defaultConfig {
        minSdk = 24
        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    kotlinOptions {
        jvmTarget = "1.8"
    }
}

dependencies {
    // core-ktx 1.19 is built with Kotlin 2.1 metadata, while Tauri's Android toolchain uses 1.9.
    //noinspection GradleDependency
    implementation("androidx.core:core-ktx:1.16.0")
    implementation(project(":tauri-android"))
    testImplementation("junit:junit:4.13.2")
}
