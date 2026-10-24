plugins { id("com.android.application") }

android {
    namespace = "com.saarthi.studio"
    compileSdk = 35

    defaultConfig {
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        applicationId = "com.saarthi.studio"
        minSdk = 26
        targetSdk = 35
        versionCode = 16
        versionName = "1.19.0"
    }

    buildTypes {
        release {
            // Production release build: R8 code shrinking/obfuscation plus
            // resource shrinking. No signing config is committed; release
            // artifacts are assembled unsigned and signed at distribution time.
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
