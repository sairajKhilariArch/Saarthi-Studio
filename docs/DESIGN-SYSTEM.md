# Sarthi One — unified design system

One look across desktop (web UI + IDE shell), the VS Code extension, the
Android client and the website. Every surface reads the same semantic
tokens; only the values differ per theme.

## Sources (the four-way blend)

| Reference | What it contributes |
|---|---|
| **Hermes** (Nous desktop, LENS_0 "Hermes Teal") | Deep teal-black canvas (`#041c1c` family), warm cream midground (`#ffe6cb`), emerald for success/output (`#34d399`), comfortable 1.5–1.55 line height, JetBrains Mono for code, restrained thin-border chrome |
| **Claude** (claude.ai) | Warm ivory light theme (`#f6f2ea` paper, `#fffdf8` cards), terracotta brand warmth (`#d97757`), serif display face for brand and welcome moments, soft 12–14px radii and gentle shadows |
| **DeepSeek** (chat.deepseek.com) | The primary interactive blue (`#4d6bfe`, lifted to `#6b7fff` on dark, deepened to `#3d56d9` on ivory for contrast), crisp minimal controls, clear focus rings |
| **Freebuff** (CodebuffAI) | Thread-first sidebar, agent/workspace status pills, calm composer card, quiet density — the agent-workbench posture |

The blend rule: **Hermes owns the dark canvas, Claude owns the light
canvas and the brand warmth, DeepSeek owns interaction, Freebuff owns the
workbench posture.** One accent for actions (blue), one for brand (terracotta
in the mark), status colors never borrowed from either.

## Tokens

Semantic names are unchanged from the previous system (`--bg`, `--panel`,
`--surface*`, `--line*`, `--text*`, `--accent*`, `--ok*`, `--danger*`, …)
so every component keeps working; the values are re-aimed. New:

- `--display` — serif stack (Georgia / Tiempos fallbacks) for the brand and
  welcome heading only; body stays Inter/system sans.
- `--radius` — 12px (was 10): Claude softness, Hermes restraint.

Dark (default): teal-charcoal surfaces `#061111 → #152827`, cream-white
text `#efede4`, blue accent `#6b7fff`, emerald ok `#34d399`.
Light: ivory surfaces `#f4f0e8 → #fffdf8`, warm charcoal text `#211d18`,
blue accent `#3d56d9`, terracotta brand `#c25e3e`.

The five user-selectable accents (settings → appearance) are preserved.

## The mark

The tardigrade is retired. The new mark is a **guide star over a horizon
arc** — *sarthi* means charioteer/guide: the star is the guidance, the arc
is the road. Two filled paths, no strokes, so it renders identically as an
inline `currentColor` symbol, a standalone SVG, an Android vector drawable
and a rasterized PNG/ICO.

- App icon / favicon: teal rounded square, cream star, terracotta arc.
- Inline UI symbol (`#i-tardigrade`, id kept for compatibility):
  `currentColor`, adapts to both themes.
- Website / standalone mark: terracotta monochrome (reads on ivory and on
  teal).
- Android adaptive icon: teal background layer, cream star + terracotta
  arc foreground.

## Platform mapping

| Platform | Where the tokens live |
|---|---|
| Desktop web UI + IDE shell | `app/static/styles.css` (`:root` + light block); `ide.css` inherits everything |
| VS Code extension | `media/ui` is a build-time copy of `app/static` (`build.mjs`) — same tokens, zero drift |
| Android | `MainActivity` color constants + `res/values/styles.xml` + adaptive-icon layers mirror the dark palette |
| Website | `website/src/styles/global.css` mirrors the same semantic values |

## Rules

1. No component carries a literal color — tokens only (existing rule).
2. Status colors (ok/warn/danger/info) are never re-aimed by accents.
3. The serif display face is for brand/welcome moments only, never body.
4. Both themes must pass contrast for text on every surface they name.
