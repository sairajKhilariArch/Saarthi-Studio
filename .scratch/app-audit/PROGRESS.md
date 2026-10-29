# Progress

## Done
- M1 audit + persistent state (this directory).
- M2 privacy pass shared core (Rust `privacy.rs` + VS Code ported scanner), context packing integration.
- M3 VS Code agent parity: `agent.ts` runtime, `/api/runs` server, Runs webview panel; cancel flag now actually read mid-step (desktop + extension); all tests green (94/94 desktop, 22/22 extension).
- M4a Android streaming host chat: `DesktopHostTransport` SSE bridge, dual-format stream parsing in MainActivity, host_api `chat_stream` route + strict-shape tests + mock-router happy-path (delta/done frames, persisted messages).
- M4b Android agent runs: agent routes now also mounted at `/api/host/v1/runs*` on the host listener behind the host bearer wall; `agent_runs` advertised in `/api/host/v1/capabilities`; MainActivity Tools menu → "Agent runs" with capability-gated Runs screen (list, detail, pending diff monospace preview, Approve/Reject, Cancel-run, per-step Restore), New-run composer with permission checkboxes, graceful degradation for older hosts. Host boundary tests: unauth 401, list shape, masked 400s, shared 404s.

- M5 macOS packaging: desktop-release.yml macos job now builds BOTH arm64 and x86_64, merges a universal binary with `lipo`, verifies both slices, runs a headless HTTP smoke (`FREE_AI_STUDIO_PORT`), and packages one `macos-universal` unsigned tarball (no dmg: unsigned dmg is punished harder by Gatekeeper than a tarball — deviation from the roadmap wording, same goal). desktop-ci.yml gained a macOS build+test job so macOS-only breaks surface in CI. Signing/notarization stays a documented blocker (no credentials).
- Round 2: round 1 committed on main (v1.23). Website branch merged with main; one download page per platform (Linux, Windows, macOS, Android, VS Code) with install steps + homepage platform-card grid + shared.css. Runbooks added to README: live adaptive-routing verification and macOS signing/notarization prerequisites.

## Current
- Round 2 complete. Website pages + runbooks verified in preview (all links resolve, all pages 200, no console errors).

## Next
- Nothing. Remaining blockers unchanged: macOS signing (needs Apple credentials — runbook in README), Android device testing (needs hardware), live adaptive test by the user (3-minute runbook in README).

## Blockers
- Live Android device/emulator: none provisioned (gradle build + unit tests + lint only).
- Later-turn environment: `gradle` not on PATH — binary found at `~/.local/opt/gradle/gradle-8.9/bin/gradle`; JDK used: `/usr/lib/jvm/java-17-openjdk-amd64`.
- No macOS runner locally (M5 CI jobs execute only on GitHub's `macos-latest`); signing credentials absent.
- No provider credentials here: adaptive tiering verified by unit tests, not a live routed run.
