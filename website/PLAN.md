# Sarthi Studio official website — audit and implementation plan

**Status:** approved 2026-11-11; Milestone 2 foundation implemented.
**Date:** 2026-11-11
**Audited commit:** `main` @ `147096d` (`3.3.0: reorder a queued run one place at a time`)
**Repository:** <https://github.com/sairajKhilariArch/Saarthi-Studio>

This document is Milestone 1 only. It records what exists, what the website may
honestly say, which stack and hosting fit the repository, and the task breakdown
for Milestones 2–7. It deliberately contains no frontend implementation.

---

## 0. How this audit was performed

- Read the root `README.md`, `PLATFORM-ARCHITECTURE.md`, and the current
  engineering docs: `ARCHITECTURE.md`, `FEATURE-STATUS.md`, `PROGRESS.md`,
  `MASTER-ROADMAP.md`, `IMPLEMENTATION-SPECIFICATION.md`, `KNOWN-ISSUES.md`,
  `TESTING-STRATEGY.md`, `BRAND-ASSETS.md`, `BRANDING-AUDIT.md`.
- Inspected every workflow under `.github/workflows/` and the release scripts
  under `scripts/ci/`.
- Inspected the real GitHub repository and release metadata through the public
  REST API (unauthenticated, read-only): repository settings, 66 releases, and
  their assets.
- Inspected the live GitHub Pages site and the old `website` branch.
- Inspected the available brand SVGs and screenshots.

**Workspace correction:** the working tree was checked out on the old
`website` branch at `17299d5` (`1.22.1`), 186 commits behind `main`. That branch
has no current docs and an older release workflow. The audit was moved to
`main` (clean tree) before reading; the `website` branch was not modified.

---

## 1. Repository and product reality

### 1.1 Branches and versions

| Fact | Value |
|---|---|
| Default branch | `main` |
| `main` head at audit | `147096d`, app version `3.3.0` |
| Old `website` branch | `17299d5`, app version `1.22.1`, 14 ahead / 186 behind `main` |
| Website directory on `main` | **does not exist** |
| Website directory on `website` branch | old hand-written static site (`index.html`, five per-platform download pages, `shared.css`) |
| Latest published GitHub release | `v1.71.0`, published 2026-11-06, **0 assets** |
| Newest release with any assets | `v1.61.0`, published 2026-11-06, **8 assets** |
| Latest app version in source | `3.3.0` (no `2.x`/`3.x` release assets exist) |

Two independent version lines are visible: the source tree is at `3.3.0`, while
the newest tag is `v1.71.0`. The site must show the **release tag** for
downloads and the **source version** for docs/status, and must not imply that a
`v1.71.0` download contains `3.3.0` code.

### 1.2 Runtime and clients (verified in `docs/ARCHITECTURE.md`)

- One Rust binary (`app/`, crate/binary `free-ai-studio`) serves a loopback UI
  and API at `127.0.0.1:8765`; the `gui` feature adds a Tauri v2 window.
- The frontend is hand-written static HTML/CSS/JS in `app/static/`; no build
  step and no package manager.
- SQLite lives at `~/.local/share/free-ai-studio/studio.db`; config at
  `~/.config/free-ai-studio/config.json`. These paths are fixed.
- An opt-in Host API (`/api/host/v1/*`) is a separate listener with a bearer
  token and TLS. Android is a companion client; its host-mode chat is
  **non-streaming** and advertises `streaming: false` / `attachments: false`.
- The VS Code extension is a TypeScript client with an embedded Node server
  implementing a subset of the desktop routes. It has no PTY terminal and no
  process supervision.

### 1.3 Maturity snapshot

`docs/FEATURE-STATUS.md` is the maintained evidence matrix (615 lines). It
records 18 phases and an overall 92% derived score, but the website must **not**
publish that percentage as a marketing claim. What matters for the website is
the per-row status and evidence:

- Working: agent loop, approvals, checkpoints, queue/priorities/dependencies,
  retries, worktrees, MCP (stdio/HTTP/SSE, schema validation), memory/index/RAG,
  Git, terminal, router lifecycle, OpenAI Responses/Thinking, permissions,
  audit, keychain, usage/cost.
- Partial: model context/price where the provider does not publish them,
  per-skill permissions (skills are prompt text and never executed), embeddings
  as a router proxy, editor engine/debugger/watch mode, client parity, Windows
  MSI/macOS `.app` signing and notarization, cross-process run admission.
- Missing: debugger, arbitrary command presets, provider adapters/UI
  extensions, signed/notarized macOS builds, a macOS `.dmg`, and install smokes
  for several formats.
- The Android app supports Direct Router and Desktop Host modes, but host-mode
  chat is non-streaming; Android has no on-device agent or router management.
- macOS builds are unsigned; Windows installers are unsigned. The site must
  state the Gatekeeper/SmartScreen consequences, not hide them.

`docs/KNOWN-ISSUES.md` also records the old `website` branch divergence (#9) and
the installer/signing gaps (#16, #34–#36). These are public limitations, not
things to paper over.

---

## 2. Existing website and hosting audit

| Item | Finding |
|---|---|
| Live URL | <https://sairajkhilariarch.github.io/Saarthi-Studio/> returns HTTP 200 |
| Current source | GitHub Pages is enabled (`has_pages: true`) and is serving a **Jekyll 3.10 build of the old `website` branch** — the live page is the branch README rendered as a site |
| Old site in repo | `website/index.html` + `website/download-{linux,windows,macos,android,vscode}.html` + `website/shared.css` on the `website` branch |
| Old site version | 1.22.1; its download pages predate the current artifact names and the 3.x feature set |
| Pages deployment workflow | **none** in `.github/workflows/` |
| Custom domain | none configured or verified |
| Consequence | Deploying the new site from `main` requires switching Pages → Build and deployment → Source to **GitHub Actions**, which replaces the Jekyll site. The old branch can stay untouched for history. |

The project Pages URL is a subpath (`/Saarthi-Studio/`), not a root domain. All
internal links, assets, search indexes and canonical URLs must be base-aware.
A custom domain can remove the subpath later; the build must accept
`SITE_URL`/`SITE_BASE` configuration rather than hardcoding either.

---

## 3. GitHub Releases audit

### 3.1 Release history

Read through the public REST API on 2026-11-11:

- 66 published releases were returned.
- 56 have assets; the 10 newest (`v1.62.0`–`v1.71.0`) have **zero** assets.
- `latest` is `v1.71.0`, 2026-11-06T22:26:14Z, 0 assets.
- The newest release with assets is `v1.61.0`, 2026-11-06T22:25:41Z, 8 assets.
- No prereleases or drafts were present in the fetched set.

The real `v1.61.0` assets are:

```text
free-ai-studio-android.aab                     125,256
free-ai-studio-android.apk                      72,564
free-ai-studio-vscode.vsix                     195,614
free-ai-suite_1.61.0_linux-x86_64.tar.gz     6,907,960
free-ai-suite_1.61.0_macos-universal.tar.gz  12,508,502
free-ai-suite_1.61.0_windows-x86_64.zip       7,415,985
free-ai-suite_1.61.0_x86_64.AppImage          7,416,312
SHA256SUMS.txt                                     214
```

There is **no `.deb`, `.msi`, `.app.zip` or `.dmg`** in any published release
fetched. Those formats exist in the current release workflow but have not
appeared in a successful release since the workflow gained them.

### 3.2 Naming is inconsistent, and the README is not a source of truth

The current `desktop-release.yml` on `main` produces:

| Artifact | Name produced by the workflow |
|---|---|
| Linux portable | `sarthi-studio_<version>_linux-x86_64.tar.gz` |
| AppImage | `sarthi-studio_<version>_x86_64.AppImage` |
| Windows zip | `sarthi-studio_<version>_windows-x86_64.zip` |
| Windows MSI | `sarthi-studio_<version>_windows-x86_64.msi` |
| macOS universal | `sarthi-studio_<version>_macos-universal.tar.gz` |
| macOS app bundle | `sarthi-studio_<version>_macos-universal.app.zip` |
| Debian package | `dist/*.deb` from `cargo deb` (Cargo package is `free-ai-studio`, so the filename is not `sarthi-studio_*`) |
| VS Code | `free-ai-studio-vscode.vsix` |
| Android | `free-ai-studio-android.apk`, `free-ai-studio-android.aab` |

But the **actual published** assets from `v1.61.0` use `free-ai-suite_*` for
Linux/macOS/Windows/AppImage. The README on `main` claims
`sarthi-studio_*` and a `macos-arm64` tarball, while the workflow builds a
`macos-universal` tarball. The README also claims a `.deb` in every release; the
published set has none.

**Therefore:** the website must derive everything from release metadata and
classify by filename tokens/extensions. It must preserve original filenames and
URLs, and must never hardcode a name, version, date or URL. Documentation must
show only formats present in the manifest.

### 3.3 What visitors will see on day one

A truthful download page today says:

- Latest stable release: `v1.71.0`, published 2026-11-06, **no build assets
  attached**.
- Latest release with downloadable builds: `v1.61.0`, clearly labelled as an
  older build, not as "latest".
- Linux: portable tarball + AppImage available (1.61.0); `.deb` built by the
  pipeline but not currently published.
- Windows: portable zip available; MSI built by the pipeline but not published.
- macOS: universal tarball available, unsigned; `.app.zip` built by the
  pipeline but not published.
- Android: APK/AAB available, unsigned.
- VS Code: VSIX available.
- Source/docs: 3.3.0, ahead of every published build.

This is the honest state. The site must not fabricate a "Download 3.3.0"
button.

---

## 4. CI/CD audit

`.github/workflows/` contains four workflows, all app/platform CI, no web
deployment:

- `desktop-release.yml` — triggered by `v*` tag pushes and manual dispatch;
  `contents: write`; concurrency `desktop-release-${{ github.ref }}`; six jobs
  attach artifacts with `softprops/action-gh-release@v2`.
- `desktop-ci.yml` — Rust clippy/tests/layout/version/frontend/e2e gates on
  Ubuntu and macOS; `contents: read`.
- `rust-host-api-ci.yml` — rustfmt on host modules, tests, clippy.
- `android-ci.yml` — unit, lint, assemble, emulator instrumentation, release
  validation; `contents: read`.

Important for the website automation:

1. The release workflow is **tag-push triggered**, not `release: published`.
   `softprops/action-gh-release` creates/updates the release and uploads assets
   job by job, so a `release: published` event can arrive before all assets
   exist. The sync workflow needs a `workflow_run` trigger on **Desktop Release
   completed** and a short retry/backoff, as well as manual and scheduled runs.
2. Existing workflows pin some actions by tag; the new workflows should pin
   third-party actions to commit SHAs (with version comments) and use minimal
   permissions.
3. No workflow currently writes a release manifest or deploys Pages.

---

## 5. Documentation inventory

Root docs on `main`:

| File | Use for the website |
|---|---|
| `README.md` | Product overview, feature list, paths, setup, known limits; install table is stale (see §3.2) |
| `PLATFORM-ARCHITECTURE.md` | Desktop-host vs Android companion model, security requirements, release boundaries |
| `docs/ARCHITECTURE.md` | Verified runtime shape, modules, HTTP surface, persistence, delivery — the primary source for the architecture/API centre |
| `docs/FEATURE-STATUS.md` | Per-requirement status + evidence; the source for `features.json` |
| `docs/PROGRESS.md` | Shipped-version checklist and per-commit evidence; useful for "introduced in" only when explicit |
| `docs/MASTER-ROADMAP.md` | Phase ordering and remaining work; planned features only |
| `docs/IMPLEMENTATION-SPECIFICATION.md` | Engineering contracts and definition of done |
| `docs/KNOWN-ISSUES.md` | Public limitations, signing status, website branch, platform gaps |
| `docs/TESTING-STRATEGY.md` | Build/test/contribution instructions |
| `docs/BRAND-ASSETS.md` | Canonical mascot geometry, exactly three colours, screenshot inventory |
| `docs/BRANDING-AUDIT.md` | Retained internal identifiers and migration constraints |
| `docs/adr/`, `docs/agents/`, `docs/android/`, `docs/roadmap/`, `docs/security/` | Architecture decisions and host-API security material |

There is **no** existing public docs site, search index, API reference site, or
feature registry JSON. The engineering docs are detailed but written for
maintainers; the website needs a public layer that cites them rather than
copying stale prose.

---

## 6. Brand, screenshots and content assets

Verified brand rules (`docs/BRAND-ASSETS.md`):

- Mascot: tardigrade face, exactly three colours: orange `#FF7A1A`, black
  `#111111`, white `#FFFFFF`. No gradients in the mascot; the tile behind it is
  solid black.
- Canonical SVGs in `app/static/brand/`: `tardigrade.svg`,
  `tardigrade-mono-white.svg`, `tardigrade-mono-black.svg`,
  `tardigrade-mark.svg`, `app-icon.svg`, `favicon.svg`, `splash.svg`.
- The master prompt mentions an "orange and purple" identity, but there is **no
  purple in the canonical brand assets**. Use orange as the single accent over
  near-black/white neutrals. Do not invent a second brand colour.

Real screenshots available:

- `screenshots/brand-shell-dark.png` — shell, dark theme.
- `screenshots/brand-shell-light.png` — shell, light theme.
- `screenshots/local-ai-dark.png` — local AI panel.
- `screenshots/local-ai-hardware.png` — local hardware panel.
- Older "before" crops are not for the site.

There are **no** real screenshots of a full agent run, terminal, Git pane,
workflow canvas or mobile app. The site must not fabricate them. Phase 1 uses
the real shell and local-AI captures with accurate captions; other areas use
diagrams or text until real captures are made from a running build and stored
with their source commit/date. A screenshot-capture checklist belongs in the
content tasks, not in this milestone.

---

## 7. Constraints

- The owner asked for **no local Rust builds/tests** (the laptop is slow). The
  website's checks must therefore be lightweight locally and complete in CI.
  A visual review still requires a dev server or a deployed build; see the
  approval questions.
- The main application must stay untouched. The website is isolated under
  `website/` and must not change `app/` build architecture, CI gates or data
  paths.
- GitHub Pages is already serving the old Jekyll site. Switching the source is
  a repository setting, not a file commit.
- No secrets in client code. The release manifest is generated at build/sync
  time; browsers never call the GitHub API with a token.
- No fake features, install commands, assets, screenshots, endpoints,
  testimonials or statistics.
- Repo convention: version-first commit subjects and a version bump per commit.
  Website-only commits need an explicit policy decision (see §18).

---

## 8. Decisions

### 8.1 Stack: Astro static site + MDX content collections + Pagefind

Use **Astro** (current stable, pinned in `website/package-lock.json`) with:

- TypeScript and MDX content collections for docs/features.
- Static output (no SSR, no server runtime).
- `@astrojs/sitemap` for the sitemap; a small post-build step for Pagefind.
- No React/Vue/Svelte integration by default. Theme toggle, mobile nav, copy
  buttons, OS selector and search are small vanilla islands.
- Shiki code highlighting from Astro's built-in markdown pipeline.
- `astro:assets`/Sharp for responsive screenshots.

Why:

- Docs, marketing pages and a release data layer fit content collections and a
  single component system.
- Output is plain HTML/CSS/JS, cheap to host on GitHub Pages, SEO-friendly and
  accessible by default.
- Pagefind gives a static, offline-capable search index without a service.
- The app's "no build step" rule applies to `app/static/`, not to an isolated
  marketing site; the website still gets a reproducible lockfile and CI build.

Rejected: Next.js static export (heavier runtime/config for no benefit),
Starlight (constrains the marketing design into a docs theme), Eleventy
(possible, but weaker typed content collections and image pipeline), hand-written
HTML (recreates navigation/search/TOC/maintenance by hand, which is the old
branch's failure mode).

### 8.2 Hosting: GitHub Pages via Actions, base-path aware

- Deploy with `actions/upload-pages-artifact` + `actions/deploy-pages`.
- Build with `SITE_URL=https://sairajkhilariarch.github.io` and
  `SITE_BASE=/Saarthi-Studio/` for production; `SITE_BASE=/` for local preview.
- All links/assets/search paths go through one `withBase()` helper.
- A custom domain can later set `SITE_BASE=/` and a `CNAME`, without rewriting
  pages.
- The Pages source must be changed from the old branch to **GitHub Actions**
  once, by the repository owner or an authenticated admin. The old branch is
  not deleted.

Why not Cloudflare Pages/Netlify: GitHub Pages is already enabled, free, and
the requested preference. The project subpath is a solvable constraint; the
site remains portable because URLs are base-aware.

### 8.3 Data flow: generated manifest, never a browser API dependency

```text
GitHub Releases API
        │  (server-side sync, token optional in Actions)
        ▼
website/data/releases.json   ← committed last-known-good manifest
        │  (build-time import)
        ▼
static pages + /data/releases.json (for client-side OS selection)
```

- The sync script is the only code that talks to GitHub.
- The committed manifest keeps the site functional if the API is down.
- A failed sync never overwrites the last good manifest.
- The client fetches only the small, same-origin manifest; no token, no GitHub
  API calls from the browser.

### 8.4 Status vocabulary

Public statuses are exactly:

- **Implemented** — Working in `FEATURE-STATUS.md`, evidence path exists.
- **Partially implemented** — Partial in the matrix, or a Working row with a
  named limitation that blocks a stated requirement.
- **Experimental** — shipped but explicitly unstable/incomplete in a way that
  needs a warning beyond "partial". Used only when the evidence says so.
- **Planned** — Missing/roadmap only; no download or "available now" claim.
- **Deprecated** — removed or superseded, with a replacement if one exists.

No percentages on the public site. `Broken` rows are not currently present; if
one appears, it is published as Partially implemented (if a working subset
exists) or Planned, with the failure stated in limitations. The registry stores
the exact evidence path and review date.

### 8.5 Screenshots

Use only the four verified captures initially. Compose them in a real browser
frame with captions naming what is shown. Do not label them as agent/terminal
views. Capture new screenshots later from a real running build; never mock a UI.

---

## 9. Website architecture

### 9.1 Proposed tree

```text
website/
├── astro.config.mjs
├── package.json
├── package-lock.json
├── tsconfig.json
├── PLAN.md
├── src/
│   ├── components/        # header, footer, cards, badges, code block, search, download UI
│   ├── layouts/           # BaseLayout, MarketingLayout, DocsLayout
│   ├── pages/
│   │   ├── index.astro
│   │   ├── features/…
│   │   ├── docs/…
│   │   ├── download.astro
│   │   ├── releases.astro
│   │   ├── changelog.astro
│   │   ├── architecture.astro
│   │   ├── security.astro
│   │   ├── developers.astro
│   │   ├── faq.astro
│   │   ├── about.astro
│   │   └── 404.astro
│   ├── content.config.ts
│   ├── styles/            # tokens.css, base.css, docs.css, downloads.css
│   └── lib/               # releases, classification, os-detect, features, seo, base path
├── content/
│   ├── docs/              # MDX, category folders
│   ├── features/          # MDX for the eight category pages and deep dives
│   └── faq/
├── data/
│   ├── features.json      # registry (status, evidence, platforms, limitations)
│   └── releases.json      # generated last-known-good manifest
├── public/
│   ├── brand/             # copied canonical SVGs
│   ├── screenshots/       # real captures only
│   ├── og/                # social images (real brand/screenshot based)
│   ├── robots.txt
│   └── favicon.svg
├── scripts/
│   ├── sync-releases.mjs
│   ├── classify-assets.mjs
│   ├── validate-content.mjs
│   └── check-links.mjs
├── tests/                 # node:test for pure logic + fixtures
└── .gitignore             # node_modules, dist, .astro, pagefind temp
```

### 9.2 Routes

```text
/
├── /features
│   ├── /features/ai-and-models
│   ├── /features/agents
│   ├── /features/routers
│   ├── /features/skills
│   ├── /features/mcp
│   ├── /features/memory
│   ├── /features/developer-environment
│   └── /features/security
├── /docs
│   ├── /docs/getting-started/…
│   ├── /docs/installation/{linux,windows,macos,android,vscode}
│   ├── /docs/user-guide/…
│   ├── /docs/agents/…
│   ├── /docs/providers/…
│   ├── /docs/developer-guide/…
│   ├── /docs/api/…
│   ├── /docs/configuration/…
│   └── /docs/troubleshooting/…
├── /download
├── /releases
├── /changelog
├── /architecture
├── /security
├── /developers
├── /faq
├── /about
└── /404
```

**Feature-page policy:** the eight category routes are canonical and contain
every named capability as a structured section with status, how it works,
configuration, limitations and evidence. The eight flagship capabilities
(AI chat, agents, router manager, skills, MCP, memory/index, developer
environment, permissions/safety) also get standalone deep-dive pages under
their category. This avoids 60 thin duplicate pages while keeping every
capability documented and addressable. Category anchors are stable
(`#provider-management`, etc.) for deep links.

### 9.3 Design system

- Tokens in `styles/tokens.css`: background, surface, raised surface, border,
  text, muted, accent orange, accent-soft, success/warning/danger states;
  spacing, radii, shadows, type scale, content widths, z-index.
- Dark theme default; light theme via `data-theme`; system preference respected;
  a tiny inline head script prevents a theme flash; toggle persists in
  `localStorage`.
- Typography: system UI stack and `ui-monospace` (no remote font requests, no
  font dependency). Display sizes with tight tracking; body at comfortable
  1.65 line-height.
- Components: `Button`, `IconButton`, `Badge`, `StatusBadge`, `Card`,
  `FeatureCard`, `PlatformCard`, `DownloadCard`, `Callout`, `Steps`, `Tabs`,
  `CodeBlock` (copy button), `Table`, `Breadcrumbs`, `DocsSidebar`, `Toc`,
  `PrevNext`, `SearchDialog`, `ThemeToggle`, `MobileNav`, `Footer`.
- Motion: CSS transitions only; `prefers-reduced-motion` disables non-essential
  motion; no scroll-jacking, no glow-heavy hero.
- Icons: inline accessible SVG with consistent stroke; the tardigrade is the
  only mascot and keeps its three colours.
- Every component has a server-rendered fallback; JavaScript enhances, never
  gates content.

---

## 10. Release manifest and asset classifier

### 10.1 Manifest shape (illustrative only)

```json
{
  "schema_version": 1,
  "repository": "sairajKhilariArch/Saarthi-Studio",
  "generated_at": "ISO-8601",
  "source": "github-releases-api",
  "latest_stable": "v1.71.0",
  "latest_build": "v1.61.0",
  "releases": [
    {
      "id": 0,
      "tag_name": "vX.Y.Z",
      "name": "release title",
      "published_at": "ISO-8601",
      "html_url": "https://github.com/.../releases/tag/vX.Y.Z",
      "prerelease": false,
      "draft": false,
      "body": "release notes markdown, bounded",
      "assets": [
        {
          "name": "actual-asset-name",
          "browser_download_url": "https://github.com/.../download/...",
          "size": 123456,
          "content_type": "application/octet-stream",
          "platform": "linux",
          "architecture": "x86_64",
          "package_type": "appimage",
          "kind": "portable",
          "checksum_sha256": null,
          "classification": "matched"
        }
      ],
      "asset_summary": {
        "total": 8,
        "classified": 8,
        "unknown": 0,
        "has_installable": true
      }
    }
  ],
  "warnings": []
}
```

Rules:

- Drafts excluded. Prereleases included but never used as `latest_stable`.
- All published releases are stored (cap 200; fail loudly if more), so older
  versions are browsable without the API.
- Original `name`, `browser_download_url`, `size`, `content_type` preserved.
- `latest_stable` = newest non-draft, non-prerelease release, **even when it
  has zero assets**. `latest_build` = newest release with at least one
  installable asset (or `null`). They may differ, and the UI must say so.
- Release notes are bounded (e.g. 40k chars) and sanitized at render time.
- `generated_at` and a content hash support stale-data warnings and idempotent
  sync.

### 10.2 Classification rules (tested, conservative)

Classify on lowercase filename tokens, never on a single prefix:

| Token/extension | Platform | Architecture | Package type |
|---|---|---|---|
| `.apk` | android | null | apk |
| `.aab` | android | null | aab (not directly installable) |
| `.vsix` | vscode | null | vsix |
| `.deb` + `amd64`/`x86_64` | linux | x86_64 | deb |
| `.deb` + `arm64`/`aarch64` | linux | arm64 | deb |
| `.AppImage` (case-insensitive) | linux | token or null | appimage |
| `.tar.gz` with `linux` | linux | token (`x86_64`, `arm64`, `universal`) or null | tar.gz |
| `.tar.gz` with `macos`/`darwin` | macos | token or null | tar.gz |
| `.zip` with `windows`/`win` | windows | token or null | zip |
| `.zip` with `macos`/`darwin` and `.app` | macos | token | app-bundle-zip |
| `.msi`/`.exe` | windows | token or null | msi/exe |
| `.dmg`/`.pkg` | macos | token or null | dmg/pkg |
| `SHA256SUMS.txt` / `*.sha256` | — | — | checksum |
| anything else | null | null | other |

- Ambiguous names get `platform: null`, `architecture: null`, and are shown
  under **Other assets** with their original name and URL.
- Do not infer architecture from "universal" as x86_64 or arm64; use
  `universal`.
- Do not assume a `.tar.gz` is Linux without a Linux token.
- Do not assume a missing arch means x86_64.
- `content_type` is advisory; extension/token classification wins, original
  content type is retained.
- Checksums: parse any published `SHA256SUMS.txt` (`<hex>  <basename>`), match
  exact basenames, and display the digest only for matched assets. Never
  compute or invent a checksum for an asset that has none published. If several
  checksum files exist, merge and record a warning on conflicts.
- Unknown assets remain downloadable under "Other assets". The site never hides
  a published asset.
- Every rule gets a fixture test using real names (`free-ai-suite_1.61.0_*`,
  `sarthi-studio_3.3.0_*`, `free-ai-studio-android.apk`, the VSIX) plus
  ambiguous names. No test may depend on the live API.

### 10.3 Failure and fallback behavior

- Sync failure: keep the committed manifest; the job fails visibly; the site
  shows the last `generated_at` and a small "release data last synced …" note.
- Latest stable has no assets: show the version/date and an explicit "no build
  assets are attached to this release" callout, plus the newest release with
  assets as **"latest available build"**, never as latest.
- No assets anywhere: disable download actions, explain why, and link to the
  release workflow status. Do not render a button for a missing file.
- Prereleases: available in `/releases` with a filter/badge; excluded from
  `latest_stable`.
- GitHub API unavailable during sync: no commit, no deploy of a broken
  manifest; the previous site stays live.

---

## 11. GitHub Actions automation

### 11.1 Workflow A — release synchronization

File: `.github/workflows/website-release-sync.yml`

Triggers:

- `release: types: [published, edited]` (notes/asset edits).
- `workflow_run` for **Desktop Release**, `types: [completed]` — the reliable
  trigger because the release workflow uploads assets job by job.
- `workflow_dispatch` with an optional `tag` input.
- Called by the reconciliation workflow (or the reconcile job runs the same
  script).

Steps:

1. Checkout `main` (not the tag ref) with `fetch-depth: 1`.
2. Set up Node (version pinned in `website/package.json`).
3. `npm ci` in `website/`.
4. Run `node scripts/sync-releases.mjs` with `GITHUB_TOKEN` (contents: read is
   enough for reading; add `contents: write` only for the manifest commit).
5. Validate the new manifest against the schema and run classifier tests.
6. If `website/data/releases.json` changed, commit only that file with
   `website: sync release data` and `[skip ci]`; otherwise do nothing.
7. Call the deploy workflow (`workflow_call`) so the new manifest is built and
   published in the same run. A manifest-only push must not create a deploy
   loop.

Robustness:

- If the target release has zero assets and was published recently, retry the
  API a few times with backoff (asset uploads race the event); log each attempt.
- Idempotent: same API data produces byte-identical JSON (stable key order,
  stable release order, no timestamps inside content except `generated_at` —
  omit the commit if only `generated_at` would change, or accept an explicit
  `--force`).
- Fail visibly with a non-zero exit; print a summary (release tag, asset counts,
  classified/unknown, manifest hash).
- Concurrency group `website-release-sync`, no cancellation (serialize).

Permissions: `contents: write`, `actions: read`; no `pages` or `id-token` here.

### 11.2 Workflow B — website deployment

File: `.github/workflows/website-deploy.yml`

Triggers:

- `push` to `main` with paths `website/**` and this workflow file.
- `workflow_dispatch`.
- `workflow_call` (from sync and reconcile).

Steps:

1. Checkout, set up Node, `npm ci`.
2. `npm run check` — content validation, unit tests, type check.
3. `npm run build` with production `SITE_URL`/`SITE_BASE`.
4. `npm run check:links` — crawl `dist/`, validate internal paths, anchors,
   sitemap/robots presence, no broken `/download` targets.
5. `actions/configure-pages`, `actions/upload-pages-artifact`, then
   `actions/deploy-pages`.
6. Post-deploy smoke: retry the Pages URL, assert HTTP 200, expected title,
   `/data/releases.json` parses, and a known docs route renders. Fail if not.
7. Write a job summary with the deployed URL, manifest version, and route
   count.

Permissions: `contents: read`, `pages: write`, `id-token: write`.
Concurrency group `pages`, `cancel-in-progress: false`. Do not deploy from
pull requests.

### 11.3 Workflow C — scheduled reconciliation

File: `.github/workflows/website-reconcile.yml`

Triggers: `schedule` (daily, off-peak UTC) and `workflow_dispatch`.

Behavior:

1. Fetch all releases.
2. Compare the generated manifest (ignoring `generated_at`) with the committed
   one.
3. If identical, exit successfully with a "no drift" summary (no commit, no
   deploy).
4. If different, run the full sync, commit, and call deploy.
5. Detect the specific drifts named in the brief: a published release missing
   from the manifest, an asset added after publication, changed release
   metadata, stale/missing manifest, and a release with zero assets.
6. Idempotent: a second run against unchanged data is a no-op.

Permissions: `contents: write` (only if drift requires a commit), `actions:
read`. No issue-creation permission by default; failures are visible as a red
scheduled run. A future optional enhancement is an issue on repeated failure;
it is not in scope now.

### 11.4 Action pinning and secrets

- Pin third-party actions to full commit SHAs, with the version in a comment;
  update deliberately.
- `GITHUB_TOKEN` only; no PAT, no client-side token.
- Release notes are rendered with a sanitizer; no raw HTML injection.
- Workflow logs must not print secrets or full environment dumps.
- Fail the workflow, not the site: the last deployed manifest remains live.

---

## 12. Feature registry

`website/data/features.json` is the single public source of truth. Shape:

```json
{
  "id": "agent-checkpoints",
  "name": "Checkpoints and restore",
  "category": "agents",
  "summary": "One sentence a visitor can verify.",
  "status": "implemented",
  "platforms": ["desktop"],
  "docs_url": "/features/agents/#checkpoints",
  "evidence": ["app/src/agent.rs", "docs/FEATURE-STATUS.md"],
  "introduced": "1.70.0",
  "limitations": ["..."],
  "last_reviewed": "2026-11-11"
}
```

Validation at build:

- `id` unique, kebab-case; `category` in the eight categories.
- `status` in the five-value enum.
- `docs_url` resolves to a real route/anchor.
- Every `evidence` path exists in the repo (for code/docs paths); external
  evidence is a full URL.
- `introduced` is null unless a specific version is recorded in
  `FEATURE-STATUS.md`/`PROGRESS.md`; otherwise the UI says "version not
  recorded", never a guess.
- `last_reviewed` is an ISO date.
- No percentage fields.

The registry is seeded by walking the phase tables in `FEATURE-STATUS.md` at
3.3.0, then grouping sub-behaviors under the eight public categories. Planned
roadmap items are included only when they help explain a limitation, and are
labelled Planned. Every category page renders from the registry plus MDX prose,
so the statuses cannot drift from a separate marketing page.

Because the source is 3.3.0 and the newest published build is 1.61.0, features
introduced after 1.61.0 carry an additional honest note: "Verified in source;
not included in the latest published build." This is a build-time comparison of
the registry's `introduced` version against `latest_build`, not a guess.

---

## 13. Content plan

### 13.1 Home

1. Hero: mascot, one-line positioning, primary download CTA (driven by the
   manifest), "Explore the docs", release/version indicator, platform chips.
2. Product preview: real shell/local-AI captures in a browser frame with
   captions; no fake agent screen.
3. Capabilities: six cards (AI/models, agents, routers, skills/MCP, memory/index,
   developer environment), each with status and a link.
4. Platforms: Linux, Windows, macOS, Android, VS Code with verified status and
   the exact available formats; missing formats named, not hidden.
5. How it works: install → configure a provider/router → open a workspace →
   run an agent/task → review/approve → commit/apply.
6. Latest release: tag, date, notes summary, assets count, download or honest
   empty-state callout, link to `/releases`.
7. Documentation preview: six links to the most useful pages.
8. GitHub/community: repository, issues, license, contribution guide.
9. Footer: product, docs, downloads, project, legal/security.

### 13.2 Docs (topics mapped to ~35–40 pages, not 60 thin pages)

- Getting started: introduction, system requirements, installation, first
  launch/setup, first conversation, first agent task, updating/uninstall.
- Installation: one page per verified platform; commands only for formats in
  the manifest; Gatekeeper/SmartScreen/unsigned warnings.
- User guide: interface, workspaces, chat/models, providers, agents, skills,
  MCP, memory/index, terminal, Git, monitoring, settings, troubleshooting.
- Providers: provider architecture, API keys/keychain, local Ollama, discovery
  and capabilities, fallback/thinking, OpenAI Platform, usage/cost,
  troubleshooting.
- Agents: architecture/lifecycle, tools, permissions/approvals, queue/
  dependencies/retries, checkpoints/recovery, worktrees/multi-agent, failure
  handling.
- Developer guide: architecture, repository layout, backend modules, frontend,
  database/migrations, API surface, Android, VS Code, build/test/debug,
  contribution.
- API reference: Host API v1 (auth, health, capabilities, models, chat,
  conversations/messages) and a local API overview that labels loopback routes
  local-only, origin-guarded, and not a public remote contract. Request/response
  examples only where the source and tests support them.
- Configuration: config file, environment variables, provider/router settings,
  MCP, workspace, permissions, storage paths.
- Troubleshooting: the 13 categories in the brief, each with symptoms, cause,
  diagnosis, steps, related issue.
- FAQ: searchable, grouped, no marketing answers.

Docs have a left sidebar, right TOC, breadcrumbs, prev/next, edit-this-page link
to the website content, copy buttons, a version banner ("Docs for source
3.3.0; latest published build 1.61.0"), and Pagefind search with `/` and
Cmd/Ctrl+K.

### 13.3 Changelog

Driven by GitHub release notes. Render the body as sanitized markdown. Show
New/Improved/Fixed/Breaking/Security groupings **only when the notes contain
those sections**; never invent categories or entries. If GitHub notes are
empty, show "No release notes were published" with the GitHub link.

### 13.4 Architecture, security, developers, about

- Architecture diagrams are hand-authored, accessible HTML/SVG based on
  `docs/ARCHITECTURE.md`: runtime shape, request/data flow, agent lifecycle,
  router ownership, MCP transports, retrieval, platform clients. Each diagram
  cites its source and the commit it was verified against.
- Security explains local-first defaults, loopback binding, Host API bearer/TLS,
  path confinement, permissions/audit, keychain, worktrees, and explicit
  limitations (agent commands run with the user's privileges, unsigned builds,
  no OS sandbox for arbitrary processes, remote MCP servers are trusted by
  configuration).
- Developers: repo links, setup, build/test commands, contribution rules,
  release process, issue reporting.
- About: product purpose, MIT license, repository link, maintainer/project
  identity only from verified public repo data. No invented team, users,
  testimonials or numbers.

---

## 14. SEO, performance, accessibility, security

- Per-page title/description, canonical URLs (base-aware), Open Graph/Twitter
  metadata, one real brand/screenshot-based OG image initially.
- `sitemap-index.xml` via `@astrojs/sitemap`; `robots.txt` with the sitemap URL;
  consistent trailing slashes; `404` page.
- JSON-LD `SoftwareApplication` with name, application category, operating
  systems actually published, license, code repository and free offer. No
  aggregateRating.
- Semantic landmarks, skip link, keyboard-operable nav/search/TOC/copy,
  visible focus, `aria-current`, labelled controls, no color-only status.
- Contrast checked against the token palette; reduced-motion support; images
  with meaningful alt text; tables with captions.
- No framework runtime; small vanilla islands; Pagefind loaded on demand;
  screenshots responsive/lazy; no remote fonts; CSS/JS minified by the build.
- No secrets, no tokens, no GitHub API calls from the browser; release notes
  sanitized; external links `rel="noopener noreferrer"`; dependency audit in CI.
- GitHub Pages cannot set custom response headers; document that limitation
  rather than promising HSTS/CSP headers. A meta CSP can be evaluated later.

---

## 15. Milestones and tasks

### Milestone 2 — Design system and foundation

**Implementation status (2026-11-11):** foundation built with the isolated
website toolchain. `npm run check` (0 errors), `npm run build`, and
`npm run check:build` pass locally under `/Saarthi-Studio/`. Visual review is
pending; no app build or test was run.

Deliverables:

- `website/` Astro project, lockfile, tsconfig, `.gitignore`.
- Tokens, base/global styles, dark/light theme with no flash, responsive
  header/mobile nav/footer.
- Base/marketing layouts, SEO component, 404, home skeleton with honest empty
  release state.
- Core components (button, badge, card, callout, section, code block).
- Deploy workflow (so each later milestone can be reviewed on Pages), with the
  Pages source switched to GitHub Actions.

Acceptance: production build green in CI; responsive at 360/768/1440; keyboard
navigation and theme persistence; no fake content; deploy smoke passes.

### Milestone 3 — Product pages and feature centre

Deliverables:

- Complete home page with real screenshots and manifest-driven CTAs (download
  CTA may point to GitHub Releases until `/download` lands in M5).
- `/features` index + eight category pages, each rendering registry statuses.
- `features.json` seeded from `FEATURE-STATUS.md` with evidence/limitations.
- Platform overview with verified formats and gaps.
- Initial `/architecture`, `/security`, `/developers`, `/faq`, `/about`.
- Real screenshot compositions; no fabricated UI.

Acceptance: every public feature claim has a registry/evidence entry; statuses
match the matrix; navigation and internal links work; CI build/link check green.

### Milestone 4 — Documentation platform

Deliverables:

- MDX docs collections, docs layout, sidebar/TOC/breadcrumbs/prev-next/copy,
  edit links, version banner.
- Pagefind search with keyboard shortcuts and grouped results.
- Getting started, installation, user guide, providers, agents, developer
  guide, API reference, configuration, troubleshooting, FAQ (the topic map in
  §13.2; split into two reviewable batches if needed).
- Architecture diagrams and code examples sourced from verified code.
- Search indexing and docs validation.

Acceptance: all categories navigable and searchable; no broken internal links
or anchors; commands/formats exist in the manifest or are explicitly marked
unpublished; API docs match verified routes; CI checks green.

### Milestone 5 — Release integration and downloads

Deliverables:

- Sync script, classifier, checksum parser, manifest schema/validator,
  fixtures and unit tests.
- Generated `releases.json` from the real API (including the current
  `v1.71.0` empty state and `v1.61.0` assets).
- `/download`: OS/architecture detection with manual override, recommended
  asset only when it exists, all available packages, sizes, checksums,
  installation steps, empty/fallback states.
- `/releases` history with filters and older versions; `/changelog` from real
  notes; home latest-release section wired to the manifest.

Acceptance: real assets classified correctly; no download button without an
asset; unknown assets accessible; latest-with-no-assets and API failure states
verified; OS detection/classifier tests pass in CI.

### Milestone 6 — Automation

Deliverables:

- Sync, deploy and reconcile workflows as designed; pinned actions; minimal
  permissions; concurrency.
- Manifest commit-on-change with no deploy loops; last-known-good retention.
- Manual sync and reconciliation run against the live API; deploy smoke.
- Documentation of the release/site operations (how to publish, re-sync,
  inspect a failure).

Acceptance: manual sync is idempotent; a failed sync leaves the previous
manifest and site intact; reconciliation detects drift; deploy publishes the
current manifest; logs are readable.

### Milestone 7 — Quality, security and launch

Deliverables:

- Playwright smoke + axe accessibility checks on key routes in CI; link
  checker; sitemap/robots/canonical/OG validation.
- Performance pass (image sizes, JS budget, Pagefind on demand).
- Security review (no secrets, sanitized notes, dependency audit, external
  links).
- Production deployment, final docs, acceptance report with evidence.

Acceptance: all critical checks pass; no broken links; keyboard and mobile
navigation verified; downloads and statuses truthful; the site is live at the
Pages URL.

---

## 16. Verification policy

- No Rust builds/tests on the owner's laptop. The website's lightweight checks
  may run locally if approved: `node --check`, classifier unit tests, content
  validation. The full `npm ci`/build/link check runs in GitHub Actions.
- Every milestone report lists: files created/modified, commands run, results,
  what was not verified, and the next milestone.
- A feature is not reported as complete without its acceptance criteria.
- Visual checks require a running preview; see the approval question. If
  local preview is not allowed, the deployed Pages URL is the review surface,
  and visual claims will be limited to what CI/screenshots can prove.

---

## 17. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Latest 10 releases have no assets | Download page looks empty; users may think the project is dead | Show the latest stable plus "latest available build" fallback; explain build status; link to Actions/releases |
| Published names differ from README/workflow (`free-ai-suite_*` vs `sarthi-studio_*`) | Wrong file offered or install command | Classifier by tokens/extension; tests with real names; preserve originals |
| `.deb`, `.msi`, `.app.zip` exist in the workflow but not in any published release | Docs could promise non-existent installers | Render formats from the manifest; mark unpublished formats explicitly |
| Source 3.3.0 vs latest build 1.61.0 | Feature claims may imply downloads include new features | Show both versions; "verified in source, not in latest published build" notes |
| Pages source switch replaces the live Jekyll site | Old site disappears | Owner approval; old branch retained; deploy smoke before announcing |
| Project Pages subpath | Broken assets/links/search | One base-path helper; build/tests for both `/` and `/Saarthi-Studio/` |
| Unsigned macOS/Windows builds | Users hit Gatekeeper/SmartScreen | Installation docs state it plainly; no "signed" claim |
| No real agent/terminal screenshots | Temptation to mock | Use diagrams/real shell captures; capture from a real build later |
| No local app tests | Unverified backend changes | Website work does not change the app; report the existing 3.1–3.3 uncompiled state separately |
| Release event races asset uploads | Empty manifest cached | `workflow_run` trigger + retries + reconciliation |
| GitHub API rate limits/outage | Sync fails | Token in Actions; committed last-known-good manifest; no client API calls |
| Astro dependency supply chain | Build compromise/noise | Lockfile, pinned versions, minimal dependencies, audit in CI |

---

## 18. Approval record

Owner decisions on 2026-11-11:

- **Stack and hosting:** approved Astro + Pagefind, static output, and GitHub
  Pages at `/Saarthi-Studio/`, with a base-path-aware build.
- **Pages source:** approved switching from the old `website` branch to
  **GitHub Actions**. The old branch is left untouched.
- **Downloads:** when the latest stable release has no assets, show the newest
  release with assets separately as **latest available build**.
- **Commit policy:** website-only work uses a separate version line and does
  not bump the desktop version. The exception is recorded in `AGENTS.md`.
- **Local preview:** the question was not answered. The M2 foundation was
  built locally with the isolated website toolchain; no Rust build or app test
  was run. Further checks stay in CI/Pages unless the owner opts in.

### Original approval questions (historical)

1. **Stack and hosting:** approve Astro + Pagefind, static output, GitHub Pages
   at `/Saarthi-Studio/` with a base-path-aware build (custom domain later)?
2. **Pages switch:** approve changing the repository's Pages source from the
   old `website` branch to **GitHub Actions**, which replaces the live Jekyll
   site? The old branch will be left untouched.
3. **Download fallback:** when the latest stable release has no assets (today,
   `v1.71.0`), show `v1.61.0` separately as "latest available build" with all
   its real formats, while stating no builds are attached to `v1.71.0`?
4. **Commit/version policy:** website-only commits do not bump the desktop app
   version (they bump `website/package.json` and use a `website:` subject), or
   should every commit keep the repo's app-version-first rule? This needs a
   decision before M2.
5. **Local preview:** may I run the website's `npm install`/dev server or a
   static build locally for visual review, or should all builds and visual
   checks happen only in CI/Pages? The app's no-build rule will still be
   respected either way.

Milestone 2 is implemented and committed. No deployment has been made: the
Pages source is still the old Jekyll branch until the owner switches it to
GitHub Actions.
