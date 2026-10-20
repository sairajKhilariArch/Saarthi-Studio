# Desktop Host API Security Hardening — Step 5

## Scope

This step wraps the already-authenticated `/api/host/v1/` router with common resource limits and security response headers. It does not alter the desktop UI router or enable the remote listener. Step 4 must keep HTTPS mandatory and remote access opt-in.

## Request budget

- Process-wide fixed window: 120 requests per 60 seconds by default.
- In-flight handler cap: 8 concurrent requests by default.
- A 429 response includes `Retry-After` and a generic `rate_limited` code.
- A 503 response is returned when the concurrency cap is reached, also with `Retry-After`.
- Failed authentication attempts also consume the request budget; this reduces brute-force and verifier-file churn.
- The middleware does not trust `X-Forwarded-For`, `Forwarded`, or other client-supplied IP headers.

This version intentionally has one global budget because the first remote companion supports one trusted profile. A process-wide cap is not per-client fairness: a malicious peer could exhaust the shared budget. Keep the listener behind a trusted private/VPN network while this limitation exists. Before multiple devices are supported, add peer-address and authenticated-device/token-specific quotas with bounded state and tests.

## Response headers

All host API responses, including rejected requests, receive:

- `Cache-Control: no-store`
- `X-Content-Type-Options: nosniff`
- `Referrer-Policy: no-referrer`
- `X-Frame-Options: DENY`
- `Content-Security-Policy: default-src 'none'; frame-ancestors 'none'`

No CORS allowance is added. The API must not expose the static desktop UI, `/api/config`, `/api/test`, `/api/usage`, deletion endpoints, or arbitrary command/file access.

## Limits and residual risks

The existing host API router is responsible for the JSON body-size cap, pagination caps, message/code validation, bearer-token verification, and sanitized error responses. These controls are regression-tested in `host_api.rs`; this step adds tests for the outer guard and its integration with the real host router.

The semaphore bounds time spent in handlers, including upstream chat waits, but it is not a complete connection-level denial-of-service defence. TLS handshake limits, operating-system firewalling, reverse-proxy policy (if one is ever introduced), per-peer quotas, and cancellation behavior must be reviewed separately. Do not claim protection against volumetric network attacks.

## Acceptance checklist

- [ ] Steps 2, 3, and 4 are applied in order and reviewed.
- [ ] Remote listener remains disabled by default and has no HTTP fallback.
- [ ] Rust host modules format check passes.
- [ ] `cargo test --manifest-path app/Cargo.toml` passes.
- [ ] CI format, test, and Clippy jobs pass.
- [ ] Missing/wrong/revoked token tests continue to pass.
- [ ] Route allow-list, sanitized errors, request-size limit, and paginated data tests continue to pass.
- [ ] New security-header, rate-limit, reset-window, concurrency, and unknown-route tests pass.
- [ ] Real device tests verify TLS validation and ensure no API key/token appears in Android logs.
