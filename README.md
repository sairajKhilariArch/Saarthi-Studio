# Saarthi Studio for Android

A native Android companion app for Saarthi Studio. It connects to a FreeLLMAPI/OpenAI-compatible router running on your Ubuntu computer or another server. The app does not run the AI model on the phone; model/provider routing remains on the server.

## Features

- Native Android chat interface and optional code context panel
- Persistent local chat history (SQLite)
- Sends only the latest four stored messages plus a short system instruction, while keeping longer history on the phone
- OpenAI-compatible `POST /v1/chat/completions` support, `model: auto` by default
- Connection test using the `GET /v1/models` endpoint
- API key encrypted at rest using Android Keystore + AES-GCM
- Displays provider and token/latency metadata when returned by FreeLLMAPI
- No third-party Android runtime libraries; uses Android platform APIs

## Build an APK

This repository contains the Android Studio project source. The build environment used to prepare this archive did not have the Android SDK installed and external package downloads were unavailable, so a compiled APK could not be produced here.

### Option A: Android Studio (recommended)

1. Install Android Studio on Linux, Windows, or macOS.
2. Open this folder as a project.
3. Allow Android Studio to install Android SDK Platform 35 and the compatible Gradle plugin if prompted.
4. Wait for Gradle sync to finish.
5. Choose **Build → Build Bundle(s) / APK(s) → Build APK(s)**.
6. The debug APK will be at `app/build/outputs/apk/debug/app-debug.apk`.
7. Transfer that APK to your Android device and install it. For a directly transferred APK, Android may ask you to allow installs from that file manager/browser.

### Option B: Terminal

Use JDK 17 and Android SDK Platform 35. From this folder, run:

```bash
gradle assembleDebug
```

This requires a compatible Gradle installation (Gradle 8.9 or newer compatible with Android Gradle Plugin 8.7.3) and a configured Android SDK. If you use Android Studio, the IDE can manage the SDK and build tools for you.

## Connect the phone to FreeLLMAPI

### 1. Get your Ubuntu computer's LAN IP

On Ubuntu, run:

```bash
hostname -I
```

Use the LAN address for the same Wi-Fi network (often starts with `192.168.` or `10.`). Do not use `localhost` or `127.0.0.1` in the Android app: those addresses point to the phone itself.

### 2. Make FreeLLMAPI reachable from the phone

If FreeLLMAPI is published on port 3001, ensure the container is running and that the host firewall allows port 3001 from your trusted local network. In `~/freellmapi`, you can check the service with:

```bash
sudo docker compose ps
```

If the server binds only to loopback or a firewall blocks the port, the phone cannot reach it. Do not expose port 3001 directly to the public internet without appropriate TLS and access controls.

### 3. Configure Saarthi Studio

Open Settings in the Android app and enter:

- **Base URL:** `http://YOUR_UBUNTU_LAN_IP:3001/v1` when the phone and server share a trusted private Wi-Fi. Example format: `http://192.168.1.10:3001/v1` (replace with your real IP).
- **Model:** `auto` unless your FreeLLMAPI router uses a different model name.
- **Unified API key:** create/copy the router key from the FreeLLMAPI dashboard. This is not the upstream provider key.

Tap **Test connection**. A successful test depends on FreeLLMAPI exposing its OpenAI-compatible `GET /v1/models` route. If your server does not implement that route, chat requests may still work; inspect its logs and API compatibility.

For a remote server outside your trusted local network, use an HTTPS endpoint and proper TLS. Cleartext HTTP is enabled to support local-network installations, so only use it on a network you trust.

## Token usage and fallback

Saarthi keeps chat history locally but sends only the latest four messages to the router with each request. The router's `auto` model and fallback configuration are controlled by FreeLLMAPI; Saarthi cannot create free quotas or guarantee a provider switch. Free tiers have usage limits.

## Security and privacy

- The unified key is encrypted with Android Keystore and not placed in source code.
- Chat history is stored in this app's private SQLite database on the phone. Uninstalling the app normally removes this local history and settings.
- Android backup is disabled for this app.
- Do not share your unified key, upstream provider keys, `.env` file, or router logs containing secrets.
- For public/server use, use HTTPS. The HTTP support exists for a trusted local Wi-Fi setup only.

## Project details

- Package/application ID: `com.saarthi.studio`
- Display name: Saarthi Studio
- Minimum Android: Android 8.0 (API 26)
- Compile/target SDK: 35
- Version: 1.0.0
- Android native UI written in Java; provider requests use `HttpURLConnection`.

## Add Android to an existing Saarthi-Studio repository (keep the Rust app)

The Android app is intended to live in an `android/` subfolder alongside the Rust desktop app. That keeps the existing Rust source intact.

If you have cloned the Saarthi-Studio repository to `~/Projects/Saarthi-Studio`, extract this ZIP and run from the extracted folder:

```bash
./integrate-into-repo.sh ~/Projects/Saarthi-Studio
```

Then run the Git commands printed by the script. The script refuses to overwrite an existing `android/app` directory. Pushing the project triggers `.github/workflows/build-android.yml`, which compiles the debug APK on a GitHub-hosted runner and uploads it as an Actions artifact. Open the repository's **Actions** tab, choose the completed **Build Saarthi Studio Android APK** run, then download the `saarthi-studio-android-debug-apk` artifact ZIP. Extract that artifact ZIP to find `app-debug.apk`.
