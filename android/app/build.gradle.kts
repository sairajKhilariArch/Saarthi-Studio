plugins { id("com.android.application") }

android {
    namespace = "com.saarthi.studio"
    compileSdk = 35

    defaultConfig {
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        applicationId = "com.saarthi.studio"
        minSdk = 26
        targetSdk = 35
        versionCode = 62
        versionName = "1.52.0"
    }

    buildTypes {
        release {
            // R8 minifies and shrinks the distributable, but this build does
            // not sign it. The previous debug-keystore signing produced
            // app-release.apk while CI and BUILDING.md expect the unsigned
            // app-release-unsigned.apk, and shipping a debug key is not a
            // release identity. Sign the artifact with the project's release
            // keystore at distribution time (see android/BUILDING.md).
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}


dependencies {
    testImplementation("junit:junit:4.13.2")
    androidTestImplementation("androidx.test:runner:1.6.2")
    androidTestImplementation("androidx.test.ext:junit:1.2.1")
    androidTestImplementation("androidx.test:core:1.6.1")
}
