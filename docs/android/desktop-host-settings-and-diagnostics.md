# Step 7 — Android Desktop Host settings and diagnostics

## Scope

This step adds Desktop Host mode selection to the existing Android settings dialog and keeps the existing Direct Router settings and behavior available. Desktop Host settings include an HTTPS origin and a separate host token stored in the existing Android Keystore AES/GCM-protected settings blob added in Step 6. The host token field is intentionally blank when the settings dialog opens; leaving it blank preserves the saved token.

The connection test calls only `GET /api/host/v1/health`, with the host token as a bearer credential. It does not send the Direct Router API key, does not call `/api/config`, and does not enable chat submission. A successful diagnostic requires a 2xx response, `ok: true`, a non-empty `host_id`, and a non-empty `api_version`.

## Important security properties

- Direct Router remains the default for existing installs and when an unknown saved mode is read.
- Desktop Host accepts HTTPS origins only; no HTTP downgrade or certificate-verification override is added.
- The host token is not written to logs or included in UI error text.
- The health-test UI maps network exceptions and server status codes to safe messages; it does not display the raw server response body.
- Saving Desktop Host settings does not change or erase the saved Direct Router URL, model preferences, or router key.
- This step does not implement host chat, conversation synchronization, or streaming.

## Validation

Run `gradle --no-daemon -p android testDebugUnitTest lintDebug assembleDebug` using JDK 17. Review the staged diff before committing.
