plugins { id("com.android.application") }

android {
    namespace = "com.saarthi.studio"
    compileSdk = 35

    defaultConfig {
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        applicationId = "com.saarthi.studio"
        minSdk = 26
        targetSdk = 35
        versionCode = 17
        versionName = "1.21.1"
    }

    buildTypes {
        release {
            // Distributable builds must be signed or Android refuses to
            // install them ("App not installed"). Sign releases with the
            // Android Gradle plugin's auto-generated debug keystore so the
            // release artifacts work without committing any secret.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
            signingConfig = signingConfigs.getByName("debug")
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
