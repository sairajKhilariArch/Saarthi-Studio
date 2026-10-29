# Decisions (user-approved 2026-10-29)

1. **Run isolation:** in place + per-file checkpoints/restore. No git
   branch/worktree machinery. Rationale: restore round-trip already verified;
   lower risk than branch plumbing.
2. **Privacy:** scan + block obvious secrets before context leaves the
   machine; per-workspace exclusion list; blocked sends are surfaced, never
   silent. Providers may still receive project files otherwise.
3. **Parity order:** full cross-platform parity is required — VS Code first,
   then Android, then macOS packaging.
4. **Round scope:** milestones 1–4 PLUS whatever-desktop-takes-ships-everywhere
   (user feedback during approval) ⇒ project graph:
   M1 audit → M2 privacy → M3 VS Code → M4 Android → M5 macOS → M6 routing.
5. **Adaptive routing:** opt-in, off by default; explicit user model selection
   always wins; every choice recorded.

## Open questions / blockers
- macOS code-signing credentials: unavailable. Artifacts build unsigned;
  documented in TEST_REPORT.
- Live Android device testing: none provisioned in this environment; M4
  verification = build + emulator-less integration checks via host contract
  tests. Documented honestly.
