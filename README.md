# Saarthi Studio for Android

**Version 1.6.0** — native Android companion for Saarthi Studio.

Saarthi Studio connects to a FreeLLMAPI / OpenAI-compatible router running on your Ubuntu computer or another server. The Android app does not run AI models locally; requests are routed by the configured server.

## Features

- Native Android chat UI with dark styling
- Multiple saved conversations, automatic titles, switch/open, rename and delete
- **Search conversation titles and message contents** from the Chats dialog
- Persistent local conversation history using SQLite
- Compact request context: latest four messages plus a short system instruction are sent to the router; older history stays on the phone
- Markdown-style answer formatting and copy buttons for answers/code blocks
- Optional code context panel for code review and debugging prompts
- Model picker that queries `/v1/models`, with `auto` default and manual model IDs
- Usage and diagnostics panel: request outcomes, reported tokens, latency, and recent errors
- Export a conversation to Markdown
- Export all chats as JSON and restore backups (imported chats are appended)
- Router connectivity test and dashboard shortcut
- Unified API key encrypted at rest with Android Keystore and AES-GCM
- No third-party Android runtime libraries; Android platform APIs are used

## Project layout

```text
Saarthi-Studio-Android/
├── .github/workflows/build-android.yml
├── app/
│   ├── build.gradle.kts
│   └── src/main/
│       ├── AndroidManifest.xml
│       ├── java/com/saarthi/studio/MainActivity.java
│       └── res/values/styles.xml
├── build.gradle.kts
├── gradle.properties
├── settings.gradle.kts
├── BUILD-APK-UBUNTU.sh
├── CHANGELOG.md
└── README.md
```

## Build the APK

### GitHub Actions (easiest)

The workflow is configured for the dedicated `android` branch. Place this project at the root of that branch, commit your changes, and push to `android`. The workflow builds `app-debug.apk` and uploads it as the artifact `saarthi-studio-android-debug-apk`.

To start a build manually: open the repository's **Actions** tab → **Build Saarthi Studio Android APK** → **Run workflow** → select `android`.

### Android Studio

1. Install Android Studio and JDK 17.
2. Open this folder as a project.
3. Let Android Studio install Android SDK Platform 35 and the compatible Android Gradle Plugin.
4. Build → **Build Bundle(s) / APK(s)** → **Build APK(s)**.
5. The debug APK is produced at `app/build/outputs/apk/debug/app-debug.apk`.

### Ubuntu terminal

Install JDK 17, Gradle 8.9, and Android SDK Platform 35. Set `ANDROID_HOME` or `ANDROID_SDK_ROOT`, then run:

```bash
./BUILD-APK-UBUNTU.sh
```

The build script uses the system `gradle` command. This source archive does not include a compiled APK because Android SDK build tools are needed to produce one.

## Configure FreeLLMAPI

1. Run FreeLLMAPI on Ubuntu/server.
2. Find the server's LAN IP using `hostname -I` on Ubuntu. Use the IP assigned to the same network as your phone; don't use `localhost` or `127.0.0.1` in Android.
3. In the FreeLLMAPI compose `.env`, set `HOST_BIND=0.0.0.0` only if you want to reach it over your trusted local network, then recreate the service. Keep port 3001 off the public internet.
4. In Saarthi Studio → Settings, enter:
   - Base URL, e.g. `http://192.168.1.10:3001/v1` (replace with your server's actual LAN IP)
   - Model: `auto` or a model returned by the router
   - FreeLLMAPI unified API key
5. Tap **Test connection**.

Use HTTPS and proper access control when connecting to a server outside a trusted private network. Cleartext HTTP is enabled for local-network setups only.

## Search conversations

Open **Chats**. The search field filters by conversation title and message content as you type. If a message matched the query, the matching message excerpt is shown in the result card. Clear the query to return to the full conversation list. Search uses the existing SQLite database and does not require a database migration.

## Backup and restore

Use **Backup** to export the current conversation as Markdown, save all chats as JSON, or restore a previously exported JSON backup. A backup does not include your router API key or server settings. Restoring the same backup more than once creates duplicate imported conversations.

## Data, privacy, and upgrades

- Chat history and usage records are stored in the app's private SQLite database on your phone.
- The router key is encrypted using Android Keystore (AES-GCM); no API key is hard-coded into source.
- The Android application ID is `com.saarthi.studio`.
- Minimum Android version: Android 8.0 (API 26).
- Compile SDK: 35; Java 17.
- When updating the installed app, install an APK signed with a compatible certificate. Do not uninstall the existing app before backing up chats, since uninstalling normally deletes local app data.
- This package is a full **Android source project**, not a patch script. Desktop/macOS sources are maintained separately on the `main` branch.

## Known limitations

- Provider fallback/quota policy is configured in FreeLLMAPI, not in this mobile UI.
- Token figures are shown only when the router reports usage; they do not indicate the provider's remaining free quota.
- The project was statically inspected and package structure validated in the preparation environment, but a full Android SDK build has not been run here. Confirm the GitHub Actions build succeeds before installing the new APK.
