# Saarthi Studio platform architecture

## Product roles

- **Desktop is the primary product and host.** It keeps provider/router credentials, owns the authoritative host-side conversation store, and runs the local AI service.
- **Android is a companion client.** The intended flow is Android → authenticated desktop host API → the desktop's configured FreeLLMAPI/router. Android should not need the upstream provider key.
- **This monorepo migration is structural only.** The current Android code still has direct FreeLLMAPI endpoint settings, while the desktop Rust service currently binds to loopback. The remote-host integration described below is future work, not a feature already delivered by moving files.

## Repository layout

```text
app/                          # Existing Rust desktop service + static web UI (preserved)
android/                      # Independent Android Gradle project
  app/                        # Android app module, resources and tests
  scripts/ci/                 # Android instrumentation runner and regression tests
.github/workflows/            # Independent workflow files for each platform
PLATFORM-ARCHITECTURE.md
```

## Desktop-host integration plan

1. **Stabilize the desktop API.** Document and version the host API. Initial endpoints should cover health/capabilities, available models, chat requests, streaming, cancellation, and the required conversation/history operations.
2. **Add explicit remote-access configuration.** Keep loopback-only as the secure default. LAN/VPN listening must be opt-in and must bind to an explicitly selected interface; never silently bind to `0.0.0.0`.
3. **Add client pairing and authentication.** Generate a high-entropy per-host pairing secret or token, provide a deliberate pairing flow (QR/manual code), support revocation/rotation, and rate-limit authentication failures. Store the Android credential using Android Keystore-backed encryption. Do not send the upstream provider key to Android.
4. **Protect the transport.** Prefer HTTPS or a private VPN for remote access. Plain HTTP on an untrusted network must not be promoted as safe. Document firewall scope and avoid public internet exposure.
5. **Define data ownership.** The desktop host is authoritative for host-managed conversations and requests. Android may maintain an encrypted local cache/offline drafts, but syncing and conflict behavior must be explicitly defined before implementing it.
6. **Switch Android transport.** Add a desktop-host connection mode, health check, pairing UI, stream/cancel support, reconnect/error handling, and compatibility negotiation. Retain direct-router mode only if it is an intentional, documented fallback.
7. **Test security and behavior.** Add desktop API tests and Android integration tests for invalid/revoked tokens, remote disconnection, streaming cancellation, host restart, local network changes, and large/error responses. Test on a real phone before shipping.

## Release boundaries

- Do not claim that the Android app is a desktop-host client until steps 1–6 are implemented and verified.
- Do not add public network binding, credentials in URLs, or unauthenticated host endpoints as shortcuts.
- Keep the desktop and Android build workflows independently triggered by their own paths; changes to one application must not delete or replace the other.
