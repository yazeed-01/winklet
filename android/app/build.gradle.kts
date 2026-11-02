plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "app.winklet"
    compileSdk = 35

    defaultConfig {
        applicationId = "app.winklet"
        // TYPE_APPLICATION_OVERLAY needs 26
        minSdk = 26
        targetSdk = 35
        versionCode = 9
        versionName = "0.10.0"
    }

    buildTypes {
        release {
            // a public APK anyone can install: signed with the standard debug key, no release keystore
            signingConfig = signingConfigs.getByName("debug")
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        compose = true
    }
}

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.lifecycle:lifecycle-service:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.savedstate:savedstate-ktx:1.2.1")
    // PC deck: scan the QR code on the PC (Google's scanner UI, no camera permission needed)
    implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
    // the scanner brings an old Fragment; the permission prompts need 1.3+
    implementation("androidx.fragment:fragment:1.8.5")
}
