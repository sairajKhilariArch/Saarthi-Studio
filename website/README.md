# Sarthi Studio website

The official static website. It is isolated from the desktop application: the
Rust server, its static UI, and its build workflow are not modified by website
work.

The audit and milestone plan is in [`PLAN.md`](./PLAN.md). Milestone 2 provides
the Astro foundation, design tokens, responsive shell, home page, 404 page, and
the Pages deployment workflow.

## Local development

```bash
cd website
npm ci
npm run dev
```

The local dev server uses the site root. Production is served from the
`/Saarthi-Studio/` project path on GitHub Pages.

```bash
# production-style build
SITE_URL=https://sairajkhilariarch.github.io \
SITE_BASE=/Saarthi-Studio/ \
npm run build

npm run check:build
```

`npm run check` runs Astro's type/content check. `npm run check:build` verifies
that every internal reference resolves under the configured base, every page
has one `h1` and the required metadata, and the sitemap and robots files exist.

## Deployment

The deploy workflow is `.github/workflows/website-deploy.yml`. It builds on
pushes under `website/`, on manual dispatch, and when called by the release
sync workflow.

**One-time repository setting:** Pages → Build and deployment → Source must be
**GitHub Actions**. Until it is switched, the workflow cannot deploy; the old
Jekyll site from the `website` branch remains live. The old branch is not
modified or deleted.

`SITE_URL` and `SITE_BASE` are workflow environment values, not secrets. When a
custom domain is added, set `SITE_BASE=/` and add the domain in Pages settings.

## Content rules

- The source tree can be ahead of the latest published build. Read versions
  from `app/Cargo.toml`; never hardcode a release version.
- Do not invent a feature, endpoint, install command, asset, screenshot, or
  platform. Every public status has a source path or release asset behind it.
- Screenshots must come from the running product. Do not create mock UI images.
- Release data will be generated into `data/releases.json` by the sync script;
  browsers must never call the GitHub API with a token.
- Keep the canonical brand colours: orange `#FF7A1A`, black `#111111`, white
  `#FFFFFF`. There is no purple in the brand assets.

## Version and commits

Website-only commits use the website's own version line:

```text
website 0.1.0: build the Astro foundation
```

Bump `website/package.json` for website work. Do not bump the desktop version
unless the commit changes the application. Commit messages carry no AI
attribution.
