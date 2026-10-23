# Step 3 — Authenticated Desktop Host API Router

## State of this change

This step adds a **separately constructed and authenticated Axum router** for the Android companion. It intentionally does not mount that router on any listening socket. The existing desktop UI listener remains bound to `127.0.0.1`; remote listener configuration belongs to Step 4.

## Design

- The remote router is built by `host_api::build_router(AppState, HostTokenStore)` and contains only `/api/host/v1/*` routes.
- Each registered route is wrapped by bearer-token middleware. Missing, malformed, wrong, revoked, or unverifiable credentials receive a generic `401`, `WWW-Authenticate: Bearer`, and the documented error envelope.
- The token store is consulted on every request. A token rotated or revoked after the router is built is no longer accepted without restarting the process.
- The host router shares `AppState`, the same SQLite database, and the existing desktop `chat` handler. It does not create a second database or upstream client.
- Health and capabilities responses do not include the router URL, router API key, or local filesystem paths.
- Conversation listing excludes message previews. Conversation history is bounded and paginated. Chat response metadata is allow-listed rather than forwarding provider diagnostics.
- Bodies are limited to 512 KiB. Invalid framework extraction responses are normalized to the public error schema.
- The router has no root/static route, no config read/write, no usage diagnostics, no router-test endpoint, no conversation deletion, and no model override.
- A stable, non-secret host UUID is saved to `host-id` in the existing private config directory. Corrupt identity files and symlinks fail closed.
- There is no permissive CORS configuration or listener bind in this step.

## Routes implemented

| Method | Path | Behavior |
|---|---|---|
| GET | `/api/host/v1/health` | Authenticated health, API version, stable host UUID |
| GET | `/api/host/v1/capabilities` | Feature flags and payload/page limits |
| GET | `/api/host/v1/conversations` | Opaque cursor pagination, without last-message content |
| POST | `/api/host/v1/conversations` | Creates a conversation in the existing desktop database |
| GET | `/api/host/v1/conversations/{id}/messages` | Bounded message history with `before_id` pagination |
| POST | `/api/host/v1/chat` | Calls existing desktop chat service; sanitizes public errors and metadata |

API v1 intentionally does not support model/provider selection, attachments, streaming, deletion, usage endpoints, or arbitrary host commands. The upstream router key remains on the desktop.

## Security caveats and deferred work

- **No remote listener yet:** this router is testable in-process but not attached to `axum::serve`. Do not add a bind address or merge the router into the local UI router as a shortcut.
- **Transport is not provided by this module:** Step 4 must require HTTPS with proper certificate validation or an explicitly supported private encrypted VPN/tunnel, and must keep remote mode disabled by default.
- **No rate limiter yet:** rate-limit policy and tests remain part of the backend security/test phase. Do not expose the listener before that gate is addressed.
- **Windows ACL enforcement is not implemented by Step 2:** remote host mode must remain disabled on Windows until a platform-specific private secret-store/ACL implementation has been reviewed and tested.
- The current chat implementation is non-idempotent. Android must not auto-retry a chat POST after an ambiguous timeout.
- The API router itself does not perform pairing or token delivery. Token generation/revocation remains local-only via `host-token` commands.

## Acceptance checklist

- [ ] Missing token receives 401.
- [ ] Wrong token receives the same public 401 shape as missing token.
- [ ] Correct token can call health, capabilities, and allowed conversation/chat routes.
- [ ] Rotation and revocation take effect while the router object remains alive.
- [ ] `/`, `/static/*`, `/api/config`, `/api/test`, `/api/usage`, local-only aliases, delete routes, and unknown paths are unavailable on the host router.
- [ ] Health does not include router URL, API key, or filesystem paths and returns a stable UUID.
- [ ] Message previews are not returned from conversation listing.
- [ ] Conversation history is paginated and bounded; malformed cursors fail with a stable error shape.
- [ ] Chat errors do not leak provider response bodies, URL strings, or configuration internals.
- [ ] Oversized request bodies are rejected.
- [ ] Existing desktop UI routes remain wired only to the existing loopback router.
- [ ] Rust tests pass on the user's machine and in CI.

## Validation status

The package includes router-level tests for authentication, token rotation/revocation, route allow-listing, stable health identity, capabilities, conversation create/list/history, malformed cursors, oversized bodies, and sanitized chat failures. This environment does not have `cargo`/`rustc`, so those Rust tests have not been executed here; run the included installer/test script and inspect its output locally.
