# Upgrade guide: Saarthi Studio 1.11.0 → 1.11.1

## Before upgrading
1. Export all chats as JSON and keep a second copy outside the app.
2. Confirm the FreeLLMAPI URL and unified key in Settings.
3. Install the APK over the current app. Do not uninstall first; uninstalling normally removes the app-private database and encrypted settings.

## Database and backup compatibility
- App version: `1.11.1`, version code `14`.
- SQLite schema advances from v5 to v6. Existing conversations, messages, prompt presets, and request logs are retained. v4→v5 organization migration and v5→v6 backup tracker migration are idempotent helper methods.
- Backup export remains schema v3. Import continues to accept schemas v1, v2, and v3.
- Restoring the exact same backup bytes again is skipped; a separately exported/different file remains eligible for import. Chat, message, custom-preset, and fingerprint writes are transactional.

## Build
Use JDK 17, Gradle 8.9, Android SDK Platform 35, and Build Tools 35.0.0. Run:

```bash
gradle --no-daemon testDebugUnitTest lintDebug assembleDebug
```

Expected APK: `app/build/outputs/apk/debug/app-debug.apk`.

## Post-install acceptance checklist
- Confirm old chat message contents and custom prompt presets survive the upgrade.
- Import v1/v2 backups from earlier versions and v3 backups from v1.11.0. Test invalid and oversized files.
- Restore the exact same backup twice; the second import should add no duplicate chats/presets.
- Test network connect, stream, Stop, retry, fallback, and unsupported quota endpoint handling.
- On a physical phone, test voice dictation, read-aloud and Stop speech, plus an image request to a known vision-capable model.
- Check CI lint output and instrumentation results before distributing.

Keep the FreeLLMAPI port on trusted local network/hotspot access. Do not expose the unified key or server port publicly; use authenticated HTTPS for remote access.
