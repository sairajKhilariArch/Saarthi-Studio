# Saarthi Studio

Rust + SQLite + static frontend desktop AI studio (Linux/Windows/macOS), with an
Android client, a VS Code extension, and a headless server mode. This file is the
agent-facing map of the repo; see `README.md` and `PLATFORM-ARCHITECTURE.md` for
the human-facing docs.

## Agent skills

### Issue tracker

Issues live as local markdown under `.scratch/<feature-slug>/`. See `docs/agents/issue-tracker.md`.

### Triage labels

The five default triage-role labels are used as-is. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: root `GLOSSARY.md` + `docs/adr/`, created lazily. See `docs/agents/domain.md`.

## Layout

- `app/` — the Rust server + desktop binary (`src/main.rs` routes, `src/router.rs`
  provider lifecycle, `src/workspace.rs` files/projects, `static/` the entire UI:
  `index.html`, `app.js`, `styles.css`).
- `vscode-extension/` — TypeScript extension; standalone Node server lives in
  `src/lib.ts` and only implements a subset of the app's routes.
- `android/` — React Native + native-emitter client that talks to the desktop
  Host API over SSH.
- `.github/workflows/` — release pipelines: `desktop-release.yml` (Linux, Windows),
  `linux-release.yml`, `windows-release.yml`, `vscode-release.yml`,
  `android-release.yml`.

## Conventions

- Config in `~/.config/free-ai-studio/config.json`; data in
  `~/.local/share/free-ai-studio/studio.db` (SQLite). Never move these paths.
- User-facing brand string is **Saarthi Studio**; internal identifiers
  (`free-ai-studio` crate/package/app paths) must stay.
- Frontend has no build step or package manager: hand-written ES modules served
  from `app/static/`. Style through CSS custom-property tokens in `styles.css`.
