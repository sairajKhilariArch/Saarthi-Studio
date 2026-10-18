# Building Saarthi Studio Android

## Requirements
- JDK 17
- Android SDK Platform 35 and Build Tools 35.0.0
- Gradle 8.9 (Android Gradle Plugin 8.7.3)

## Build debug APK on Ubuntu

```bash
sudo apt update
sudo apt install -y openjdk-17-jdk
# Install Android Studio or Android command-line tools and SDK Platform 35.
export ANDROID_HOME="$HOME/Android/Sdk"  # adjust to your SDK path
export ANDROID_SDK_ROOT="$ANDROID_HOME"
./BUILD-APK-UBUNTU.sh
```

The script checks for Java, Gradle, and Android SDK environment configuration before calling Gradle. If you use Android Studio, opening this folder and selecting **Build APK(s)** is simpler.

## GitHub Actions

`.github/workflows/build-android.yml` builds on pushes to `android` when app files or Gradle settings change, and supports manual `workflow_dispatch`. The workflow installs SDK Platform 35 and uploads the debug APK as an artifact.

## Troubleshooting
- If SDK setup fails, verify the workflow's Android SDK action includes `packages: platform-tools` and that `sdkmanager` installs `platforms;android-35` and `build-tools;35.0.0`.
- If Gradle cannot find the SDK locally, set `ANDROID_HOME` / `ANDROID_SDK_ROOT` to your installed Android SDK directory.
- If Android refuses to install an update due to a signature mismatch, preserve the existing app and its data. Debug APKs signed on different machines can have different debug certificates.
