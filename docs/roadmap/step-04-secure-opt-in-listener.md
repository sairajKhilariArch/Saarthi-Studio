# Step 4 — Secure, opt-in Host API listener

Status: implementation package prepared; apply and run the local Rust test suite before considering this step complete.

## Changes

- Added `host_transport.rs` for strict environment parsing, bind-address allow-listing, TLS file checks, and Rustls configuration.
- Added `axum-server` with Rustls TLS support.
- Integrated a second listener that mounts only `host_api::build_router`.
- Kept the desktop UI listener on `127.0.0.1` and unchanged.
- Default host API state remains disabled; enabling requires explicit `true`, bind IP, certificate, key, and active host token.
- Added unit tests for opt-in parsing and address policy.
- Coordinated shutdown for both listeners.

## Exit criteria

1. Step 3 (`host_api.rs`) and Step 2 (`host_auth.rs`) are installed and reviewed.
2. `cargo fmt --manifest-path app/Cargo.toml -- --check` passes.
3. `cargo test --manifest-path app/Cargo.toml` passes.
4. Manual checks cover listener disabled/enabled, invalid settings, certificate/key errors, HTTP-vs-HTTPS behavior, and graceful shutdown.
5. Before production use, the Android client validates TLS certificates correctly; no trust-all implementation is allowed.
6. The operator's firewall/VPN is restricted to trusted devices; no public port forwarding.

## Not included yet

- Android host-mode connection UI and certificate trust strategy.
- Rate limiting, multi-device tokens, token scopes, pairing/QR workflow.
- Streaming, cancellation, idempotency, and cross-device history reconciliation.
