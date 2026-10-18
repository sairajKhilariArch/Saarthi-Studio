# Saarthi Studio Android 1.4.0 — Usage & Diagnostics

This update adds a local Usage & Diagnostics screen. It records AI request success/failure, configured model, HTTP status when available, latency, usage tokens reported by FreeLLMAPI, routed-provider metadata when supplied, and a short redacted error detail for recent failed requests. The connection-test button checks the configured `/v1/models` endpoint.

## Apply to your existing repository

1. Download and extract this ZIP.
2. Keep your existing repository and phone installation. Do not uninstall the app.
3. From Ubuntu Terminal, switch your local clone to the Android branch:

   ```bash
   cd ~/Projects/Saarthi-Studio
   git switch android
   git pull --ff-only origin android
   ```

4. Run the upgrade script from the extracted folder, passing your local repository path:

   ```bash
   ./apply-usage-diagnostics-upgrade.sh ~/Projects/Saarthi-Studio
   ```

   The script requires branch `android`, makes a timestamped backup, updates the main Android source, and bumps the app to version 1.4.0.

5. Review and push:

   ```bash
   cd ~/Projects/Saarthi-Studio
   git diff --check
   git diff --stat
   git add app/src/main/java/com/saarthi/studio/MainActivity.java app/build.gradle.kts
   git commit -m "Add usage and diagnostics dashboard"
   git push origin android
   ```

6. Open the repository's Actions tab, run **Build Saarthi Studio Android APK** on `android`, and download `saarthi-studio-android-debug-apk` only after it succeeds.

## What is tracked

- Request count, success/failure count, average observed request duration, and total tokens reported by the router.
- Up to 12 recent request records on screen.
- Recent failed-request details are shortened and redact the configured unified API key before adding to the diagnostics log.
- Existing assistant message metadata is used to backfill the diagnostics table during the database upgrade, where possible.

This is local app telemetry, not an authoritative provider quota report. If FreeLLMAPI does not return token counts or a routed-provider header, those values are shown as unreported. The app does not query or infer provider-side remaining quota.

## Build verification

This package has not been compiled with the Android SDK in the packaging environment. Use the GitHub Actions build result as the compile check before installing. Preserve the existing installation so local conversations and encrypted settings remain available; Android requires compatible signing to install an update over the current app.
