# Desktop Host Token Lifecycle (Step 2)

Status: implementation candidate; review and run the included Rust tests on the target checkout before merging.

## Scope

Step 2 adds a local, offline CLI to create, inspect, rotate, and revoke one bearer token for the future Android-to-desktop host API. It does **not** start a remote listener, add HTTP authentication middleware, change the existing listener bind address, or modify the Android app.

## Commands

Run these from the repository root:

```bash
cargo run --manifest-path app/Cargo.toml -- host-token create
cargo run --manifest-path app/Cargo.toml -- host-token status
cargo run --manifest-path app/Cargo.toml -- host-token rotate
cargo run --manifest-path app/Cargo.toml -- host-token revoke
```

`create` refuses to overwrite an active token. `rotate` replaces the verifier and invalidates the prior token. `revoke` removes the verifier; repeating it is safe. `status` displays only whether a token exists and the creation timestamp. The newly generated token is printed once by the explicit local command because Android pairing is not implemented yet; the CLI must not be invoked from a network request or logged by the application.

## Storage and verification

- Verifier path: `~/.config/free-ai-studio/host-auth.json` (alongside the desktop config, but as a separate file).
- Token format: `saarthi_host_v1_` followed by 32 cryptographically random bytes encoded as 64 lowercase hex characters (256 bits of entropy).
- Disk stores only SHA-256 of the complete token, schema version and creation timestamp. It never stores the raw bearer token.
- File mode is enforced as `0600`; containing directory is enforced as `0700` on Unix. Existing permissive verifier-file modes are rejected. Symlinks and non-regular verifier paths are rejected during verification.
- A private same-directory temporary file is created with exclusive creation, flushed, and atomically renamed; verifier checks read the current file each time, so local rotation/revocation does not require restarting the server.
- Token hash comparison uses a constant-time equality primitive.
- One active token is supported in this phase. There is no expiry policy, multi-device registry, per-token scope, or audit history yet.

## Security constraints

1. Do not enable host networking until Step 3 authentication middleware uses `HostTokenStore::verify()` for every protected request and the remote router exposes only the contract allow-list.
2. Never accept the host token in a URL, query string, command-line argument, application log, crash report, or analytics event. The CLI prints it to stdout only during a user-invoked create/rotate operation.
3. Android host mode should persist the token through its existing encrypted settings mechanism; this patch does not send any upstream FreeLLMAPI/router API key to Android.
4. Remote traffic must use validated TLS or a private encrypted VPN/tunnel. This token lifecycle alone does not encrypt traffic and is not proof of end-to-end encryption.
5. Unix mode-bit enforcement is implemented here. Windows ACL hardening is explicitly not implemented; remote access on Windows must remain disabled until appropriate ACL checks are added.
6. Since only one token is supported, rotation immediately invalidates every client using the old token. Communicate that to the user before rotation.

## Test coverage included

- Token format and successful verification.
- Raw token absent from verifier storage.
- Malformed/wrong token rejection.
- Refusal to create over an active token.
- Rotation invalidates the old token.
- Revocation invalidates the token and is idempotent.
- Status reports metadata without the token.
- Corrupt verifier fails closed.
- Unix permissive file permissions are rejected.

## References

- OWASP Secrets Management Cheat Sheet: https://cheatsheetseries.owasp.org/cheatsheets/Secrets_Management_Cheat_Sheet.html
- Rust `getrandom` documentation (OS random source): https://docs.rs/getrandom/0.2.15/getrandom/
- RustCrypto `sha2` documentation: https://docs.rs/sha2/0.10.9/sha2/
- Rust `subtle` documentation: https://docs.rs/subtle/2.6.1/subtle/
