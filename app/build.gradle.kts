plugins { id("com.android.application") }

android {
    namespace = "com.saarthi.studio"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.saarthi.studio"
        minSdk = 26
        targetSdk = 34
        versionCode = 5
        versionName = "1.4.0"
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
