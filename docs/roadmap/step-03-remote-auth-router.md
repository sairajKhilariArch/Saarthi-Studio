# Roadmap Step 3 — Remote Authentication and Route Allow-list

## Scope

Build a separate, authenticated Axum router for the Android host API. Do not start a remote listener in this step.

## Changed files

- `app/src/host_api.rs` — restricted route set, bearer middleware, safe response shaping, stable host identity file, and router tests.
- `app/src/main.rs` — declares the `host_api` module while leaving listener startup unchanged.
- `app/Cargo.toml` — adds Tower's `util` feature as a test dependency for `Router::oneshot` integration tests.
- `docs/security/host-api-auth-router.md` — security boundary and acceptance checklist.

## Review checklist

- [ ] Confirm Step 2 (`host_auth.rs`, token CLI and dependencies) is already present in this branch.
- [ ] Run `cargo test --manifest-path app/Cargo.toml`.
- [ ] Review every registered route; no local UI/static/config/usage/delete route should appear.
- [ ] Review that token values are never logged or included in responses.
- [ ] Review that raw upstream errors/metadata do not escape through the host API.
- [ ] Confirm `main.rs` still binds the existing desktop UI to `127.0.0.1` only.
- [ ] Keep `build_router` separate and unmounted until Step 4 adds the explicitly opt-in secured listener.
- [ ] Do not mark Step 3 complete before the tests pass locally and in CI.

## Exit condition

The in-process router passes authentication and allow-list tests, uses the existing desktop state/history/chat path, produces stable sanitized contract responses, and is still not exposed on a network socket.
