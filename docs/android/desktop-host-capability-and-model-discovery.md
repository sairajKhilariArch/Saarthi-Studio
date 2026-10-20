# Step 8 — Desktop Host capability and model discovery

## Purpose

Expose a small authenticated `GET /api/host/v1/models` endpoint on the Rust host and expand the Android connection diagnostic to validate health, capabilities, and models. This is discovery only; the Android app still does not choose a host model or send host-mode chat in this step.

## Rust API contract

- `GET /api/host/v1/health`: existing host ID and API version.
- `GET /api/host/v1/capabilities`: advertises `features.model_list=true` after the new endpoint is installed.
- `GET /api/host/v1/models`: reads models from the desktop's configured OpenAI-compatible router, using the router API key only on the desktop.
- The host response contains only `api_version`, `object`, `count`, and a bounded array of `{id, name}`. It does not expose provider ownership metadata, raw upstream error bodies, the router URL, or either credential.
- The model endpoint is protected by the existing host bearer token middleware and the existing restricted router. It is not added to the local UI router.

## Android diagnostic behavior

The Desktop Host test runs three authenticated requests in order: health, capabilities, then models. It requires API version `1`, the non-streaming chat capability, and model-list support. It displays a bounded summary, not raw server response bodies. The Direct Router connection flow remains unchanged.

## Limitations and security

- Model IDs are informational in this milestone. The configured desktop router/model remains authoritative.
- An empty but valid model list is treated as compatible; the diagnostic warns that no models were reported.
- Network exceptions are converted to generic messages. TLS certificate validation is not bypassed.
- The host token is never replaced with the FreeLLMAPI key.
- Apply only after Steps 3–7 are present. Keep host access disabled unless the HTTPS listener is explicitly configured.

## Validation

Run with JDK 17:

```bash
cargo test --manifest-path app/Cargo.toml
gradle --no-daemon -p android testDebugUnitTest lintDebug assembleDebug
```

Review `git diff --check`, the Rust route/tests, and the Android diagnostic patch before staging. The installer refuses to apply when expected source anchors differ.
