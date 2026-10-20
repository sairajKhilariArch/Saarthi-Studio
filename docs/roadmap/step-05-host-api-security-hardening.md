# Step 5 — Host API Security Hardening and Integration Tests

**Prerequisites:** Steps 2, 3, and 4 must be applied and reviewed first. This package deliberately fails closed when `host_api.rs`, `host_transport.rs`, or the expected Step 4 router mount is missing.

## Changes

- Add `app/src/host_security.rs` for a process-wide rate budget, bounded concurrent handler execution, generic overload responses, `Retry-After`, and API security headers.
- Wrap the remote-only authenticated router in the Step 4 listener setup.
- Add integration tests for the real host router, rate-limit accounting including failed authentication, budget reset, concurrency rejection, headers, and unknown routes.
- Add `.github/workflows/rust-host-api-ci.yml` to run Rust module formatting, tests, and Clippy on pushes and pull requests.

## Order of operations

1. Apply/review Step 2 (token lifecycle).
2. Apply/review Step 3 (authenticated route allow-list and input validation).
3. Apply/review Step 4 (TLS-only, opt-in listener).
4. Apply this Step 5 package.
5. Run `cargo test --manifest-path app/Cargo.toml` and review CI before merging.

## Completion criteria

- No listener is enabled by this step unless Step 4's explicit environment switch is set.
- All remote API paths pass through authentication from Step 3 and global request limits from this step.
- Existing allow-list, token rotation/revocation, pagination, body cap, and error sanitization regressions remain covered.
- No secrets or raw router details are added to logs or public overload errors.
- CI is green before Step 5 is marked complete.
