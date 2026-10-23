# Android connection transport abstraction — Step 6

## Goal

Introduce a small, testable boundary between Android UI/request orchestration and the selected backend while preserving Direct Router as the default and existing behavior.

## New classes

- `ConnectionMode`: persisted mode enum. Missing or unknown mode values default to `DIRECT_ROUTER` for backward compatibility.
- `ConnectionTransport`: common interface for configuration validation, endpoint resolution, credential selection, and streaming capability.
- `DirectRouterTransport`: preserves the OpenAI-compatible `/v1` endpoint and unified router API key; streaming is supported.
- `DesktopHostTransport`: validates HTTPS origin + dedicated host token and maps operations to `/api/host/v1/`; streaming is deliberately marked unsupported in this milestone.
- `ConnectionTransportFactory`: chooses a transport by persisted mode.

The Android settings are already encrypted as a single JSON blob using Android Keystore AES/GCM. Step 6 adds backward-compatible JSON fields `connection_mode`, `desktop_host_url`, and `desktop_host_token`; it does not expose a mode selector yet. This avoids switching users before Steps 7–9 have a complete host setup, diagnostics, host API client, and conversation mapping.

## Deliberate limits

- Direct Router remains the only active send path in this step.
- If a future build has `DESKTOP_HOST` selected before host-chat is fully integrated, sending is rejected before modifying local chat history; it must not silently fall back to the direct router or send the wrong credential.
- The host transport only resolves the versioned endpoint and credential profile here. Host health/capability calls and non-streaming chat are added in later steps.
- Existing local Android conversation IDs are numeric, whereas desktop conversation IDs are UUID strings. Do not use one as a substitute for the other; add a durable mapping in the conversation-integration step.
- This interface is an internal Java API, not a promise of compatibility for third-party plugins.

## Tests

`ConnectionTransportTest` verifies legacy config defaults, transport selection, Direct Router URL/key validation, base URL joining, credential separation, Desktop Host HTTPS enforcement, host route versioning, and streaming capability differences.

Run `gradle --no-daemon -p android testDebugUnitTest lintDebug assembleDebug` before merging.
