# Roadmap checkpoint — Step 2: host-token lifecycle

This checkpoint accompanies the Step 2 implementation package. It does not mark the step completed until the patch has been installed in the working repository and its Rust test suite passes.

## Acceptance checklist

- [ ] Integrate `app/src/host_auth.rs` and the three pinned dependencies.
- [ ] Run `cargo fmt --manifest-path app/Cargo.toml`.
- [ ] Run `cargo test --manifest-path app/Cargo.toml` and confirm all host-auth tests pass.
- [ ] Review the implementation for your current target OS; Windows ACL hardening is not implemented.
- [ ] Confirm normal startup still binds only to `127.0.0.1:8765`.
- [ ] Confirm no remote HTTP route calls `HostTokenStore` yet; that belongs to Step 3.
- [ ] Inspect and commit the `app/Cargo.lock` change if you choose to track a lockfile for this executable crate.
- [ ] Commit and push the reviewed branch.

## CLI acceptance tests

Run `create`, `status`, `rotate`, `status`, `revoke`, and `status` using a test home/config directory or a separate test installation. Do not rotate/revoke the token used by a real mobile client while testing. Verify that rotation rejects the old token and that revoke leaves no configured token.
