# Production Deployment Report — Phase 9

**Version:** 3.88.15 build evidence; final docs/CI commit 3.88.16
**Date:** 2026-11-25/26
**State:** website production build **PASS**; live deployment **NOT PERFORMED** (not authorized)

## 1. What is deployable

Saarthi Studio has two different “production” surfaces:

| Surface | Artifact | Hosting model |
|---|---|---|
| Desktop application | `.deb`, tarball, AppImage, Windows zip/MSI, macOS app, Android APK/AAB | User-installed; no server operated by the project |
| Website | `website/dist` (Astro static output) | GitHub Pages via `website-deploy.yml` on `main` |
| Local web UI/backend | Same Rust binary | Loopback `127.0.0.1` only, served from the installed binary |

There is no remote Node/Python backend to deploy. The desktop binary is the
backend, and its data stays in the user's home.

## 2. Website production build

Executed locally and in Desktop CI run `45694547969` (job **Website production
build: success**):

```
npm ci
npm run check        # astro check: 0 errors, 0 warnings, 12 hints
npm run build        # 37 pages built
npm run check:build  # 37 HTML pages, base "/", local references resolved
```

The CI job installs from `website/package-lock.json`, uses Node 24 (project
requires >=22.23.4), and fails on a type error, build error or broken local
reference. The build is static; it contains no environment secrets, API keys or
backend credentials.

## 3. Environment separation

| Environment | Definition | State |
|---|---|---|
| Development | `npm run dev` on loopback | Available; not a production claim |
| Staging | No separate staging URL or environment exists | **NOT CONFIGURED** |
| Production | GitHub Pages from `main` via `website-deploy.yml` | Workflow exists; **not run/deployed in Phase 9** |
| Desktop production | Signed/notarized installers | **BLOCKED** by missing signing credentials |

The brief asks for separate development/staging/production environments. This
repository only has development and a single GitHub Pages production target.
No staging host was invented. A staging deployment would require a separate
Pages project or host and an explicit authorization; both are outside this phase.

## 4. Security and connectivity

- **HTTPS:** GitHub Pages serves HTTPS. The local app intentionally binds
  loopback HTTP; it is not exposed to a network interface.
- **CORS:** the app has no permissive CORS. Same-origin only; foreign `Origin`
  and `Host` are rejected with 403 by the existing guard.
- **Authentication:** there is no account system for the loopback UI. It is a
  single-user local service; binding it publicly is outside the threat model.
- **Secrets:** website build has no secrets. Provider keys stay in the local
  keychain/config and are never injected into website pages.
- **Error handling:** API errors are JSON with status codes; the UI has explicit
  failure states. Existing E2E covers provider failure, database failure and
  unwritable data directory.
- **Health checks:** `GET /api/ready` reports database, schema, data directory,
  provider, routers and MCP. Verified from the packaged payload with all checks
  true.
- **Logging:** local process logs go to the user's terminal/journal; no remote
  telemetry is added by Phase 9.

## 5. Deployment and rollback procedure (not executed)

The Pages workflow deploys on `main` only:

1. Merge an approved change to `main`.
2. `website-deploy.yml` runs `npm ci` + build + uploads Pages artifact.
3. GitHub Pages publishes the new artifact.

Rollback: revert the offending commit on `main` and let the workflow redeploy,
or re-run the workflow from the last known-good commit. GitHub Pages retains the
previous deployment until the new one succeeds. No deployment was performed, so
no live rollback was tested.

Desktop rollback is different: keep the prior installer and user data; the
installer replaces binaries while data remains outside the package. The app's
update engine also has a tested `rollback` operation for a displaced binary.
See `UPDATE_SYSTEM_VALIDATION.md`.

## 6. Production readiness verdict

| Gate | State |
|---|---|
| Production website build reproducible from lockfile | **PASS** (CI) |
| No development credentials in website output | **PASS** (static, no env) |
| HTTPS at production host | **PASS** by GitHub Pages configuration (not deployed) |
| Staging environment | **BLOCKED / NOT CONFIGURED** |
| Live production deployment | **NOT PERFORMED** (authorization required) |
| Desktop production signing | **BLOCKED** |
| Desktop Windows/macOS install validation | **BLOCKED** |

The website build is deployable. The desktop application is **not** production-
distributable while signing and platform install validation remain blocked.
