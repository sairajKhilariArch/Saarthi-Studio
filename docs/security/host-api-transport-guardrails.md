# Step 4 — Host API transport and listener guardrails

## Scope

This step mounts the Step 3 restricted API only on a second listener. It does not replace the desktop UI listener and does not enable remote mode by default.

- Existing UI listener: `127.0.0.1:<FREE_AI_STUDIO_PORT or 8765>` over local HTTP, unchanged.
- Remote host API: HTTPS only, default disabled, default port `8766` when enabled.
- Remote router: `host_api::build_router(...)`; never the desktop UI router.
- Authentication: active Step 2 bearer token; startup fails if no active token exists.

## Enable variables

Set all variables only for a deliberate test/deployment:

- `FREE_AI_STUDIO_HOST_API_ENABLED=true`
- `FREE_AI_STUDIO_HOST_API_BIND=<specific loopback/private/link-local/VPN IP>`
- `FREE_AI_STUDIO_HOST_API_PORT=8766` (optional)
- `FREE_AI_STUDIO_HOST_API_TLS_CERT=/absolute/path/to/fullchain.pem`
- `FREE_AI_STUDIO_HOST_API_TLS_KEY=/absolute/path/to/private-key.pem`

`FREE_AI_STUDIO_HOST_API_ENABLED` defaults to disabled. Its accepted values are exactly `true` and `false`; an invalid value fails closed. Binding requires a literal IP address, not a hostname. Unspecified addresses (`0.0.0.0`, `::`) and public addresses are rejected. Loopback is accepted for HTTPS testing; RFC1918 private, IPv4 link-local, shared `100.64.0.0/10` VPN ranges, IPv6 ULA and IPv6 link-local addresses are allowed.

The TLS private key must be a regular, non-symlink file with Unix permissions no broader than `0600`. The certificate must be a non-empty regular, non-symlink file. Malformed certificates, mismatched keys, or an unavailable IP/port abort startup. Do not use `curl -k`, Android trust-all code, or any other certificate-verification bypass.

## Trust model

TLS encrypts and authenticates the host endpoint only when the Android client validates a chain that it trusts and the certificate SAN matches the hostname/IP used by the client. A random self-signed certificate will not be trusted automatically. Use a certificate from a trusted CA or configure a private CA/certificate trust policy on Android in the later client-hardening step. Token authentication is additional to TLS, not a replacement for TLS.

A VPN IP is accepted as a bind target only when it is in the explicit private/link-local/shared ranges above. This code cannot prove that a given interface is a VPN or that OS firewall rules are correct. Keep network/firewall rules restricted to trusted devices. Do not configure router port forwarding or expose this API to the public internet.

On non-Unix platforms, enabling the listener fails until private-key ACL validation is implemented. This is deliberate fail-closed behavior.

## Startup and shutdown

The remote socket is bound before serving begins; configuration, token existence, certificate/key file checks, TLS parsing, or bind errors cause startup failure. Ctrl+C broadcasts shutdown to both listeners; the TLS server uses a 10-second graceful-shutdown window. No token or private-key contents are logged.

## Acceptance checks

- [ ] With `FREE_AI_STUDIO_HOST_API_ENABLED` unset, no host API listener starts and desktop UI still binds loopback only.
- [ ] Explicit false disables remote mode.
- [ ] Invalid enable values fail startup.
- [ ] True without bind/certificate/key settings fails startup.
- [ ] Wildcard and public bind IPs are rejected; only the documented address classes are accepted.
- [ ] Missing token verifier fails startup and tells the operator to run `host-token create`.
- [ ] Missing, symlinked, empty, broadly permissioned (for private key), malformed or mismatched TLS files fail closed.
- [ ] The remote listener serves HTTPS and only the allow-listed host router.
- [ ] Plain HTTP is never offered on the remote listener.
- [ ] Ctrl+C ends both listeners.
- [ ] Existing desktop UI still uses the original loopback-only listener.
- [ ] Test with a real trusted/appropriately configured certificate and an Android client that verifies TLS. A unit test of IP policy does not prove certificate validation or network firewall posture.
