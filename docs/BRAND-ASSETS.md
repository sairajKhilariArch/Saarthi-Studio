# Brand assets — the Sarthi Studio tardigrade

The mascot is a tardigrade (water bear). The mark is its **face**, drawn flat in
exactly three colours: orange `#FF7A1A`, black `#111111`, white `#FFFFFF`.

Everything is generated from one geometry in
`scripts/brand/build-brand-assets.py`; do not hand-edit the SVGs.

```bash
python scripts/brand/build-brand-assets.py
```

## Inventory

| File | Purpose | Used by |
|---|---|---|
| `app/static/brand/tardigrade.svg` | canonical full-colour face, transparent background | docs, README, release artwork |
| `app/static/brand/tardigrade-mono-white.svg` | one-colour silhouette (eyes cut out) | dark surfaces, single-colour print |
| `app/static/brand/tardigrade-mono-black.svg` | one-colour silhouette | light surfaces |
| `app/static/brand/tardigrade-mark.svg` | 24 px glyph, same face with tuned strokes | reference for the inline UI symbol |
| `app/static/brand/app-icon.svg` | black tile + face, full bleed | source for `icon.png`, `icon.ico`, the packaging PNG, Android |
| `app/static/brand/favicon.svg` | 64 px rounded black tile + face | browser tab (`<link rel="icon">`) |
| `app/static/brand/splash.svg` | face + wordmark, 640×360, transparent | splash / release banner |
| `app/icons/icon.png` | 512×512 raster of `app-icon.svg` | Tauri window/app icon |
| `app/icons/icon.ico` | 16–256 multi-size ICO of the same render | Windows executable, MSI |
| `app/packaging/icons/sarthi-studio-256.png` | 256×256 PNG | AppImage, `.deb` hicolor theme |

Inline UI geometry lives in `app/static/index.html` as `<symbol
id="i-tardigrade">` (fixed brand colours, not `currentColor`, so the mascot is
always orange/black/white). It is used by the sidebar brand and the chat welcome
state; other feature icons stay as they were.

## Rasterising

There is no SVG rasteriser in the project's toolchain, so the PNG/ICO files are
rendered from `app-icon.svg` with headless Microsoft Edge and downsampled with
Pillow:

```bash
# 1. render the canonical SVG at 512×512 (no overlays)
"/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" \
  --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
  --window-size=512,512 --screenshot=app-icons-512.png \
  file:///<repo>/app/static/brand/app-icon.svg

# 2. downsample + build the ICO
python - <<'PY'
from PIL import Image
# Tauri rejects a non-RGBA app icon, so keep an alpha channel even though
# the tile is full-bleed opaque.
icon = Image.open("app-icons-512.png").convert("RGBA")
icon.save("app/icons/icon.png", optimize=True)
icon.resize((256, 256), Image.LANCZOS).save(
    "app/packaging/icons/sarthi-studio-256.png", optimize=True)
icon.save("app/icons/icon.ico", sizes=[
    (256,256),(128,128),(64,64),(48,48),(32,32),(24,24),(16,16)])
# icon.png must stay RGBA (512x512); the ICO frames inherit the alpha channel
PY
```

## Platform mapping

| Platform | Asset |
|---|---|
| Browser tab | `/static/brand/favicon.svg` |
| Sidebar / welcome | inline `#i-tardigrade` symbol |
| Windows executable + MSI | `app/icons/icon.ico` (Tauri `bundle.icon`) |
| Linux .deb / AppImage | `app/static/brand/app-icon.svg` (installed as `usr/share/icons/hicolor/scalable/apps/sarthi-studio.svg`) and `sarthi-studio-256.png`; desktop entry `Icon=sarthi-studio` |
| macOS bundle | `app/icons/icon.png` via Tauri, `CFBundleName` = Sarthi Studio |
| Android | `res/drawable/ic_launcher_foreground.xml` (face, same geometry scaled ×3 into the 66 dp safe zone) over `ic_launcher_background` `#111111`; adaptive + monochrome layers |

## Screenshots

| File | What it shows |
|---|---|
| `screenshots/brand-shell-before.png` | the shell at 2.51.0, before the change (spark glyph) |
| `screenshots/brand-shell-dark.png` | the shell in dark theme with the mark in the sidebar and the welcome state |
| `screenshots/brand-shell-light.png` | the same in light theme |
| `screenshots/brand-mark-before.png` | sidebar brand crop at 2.51.0 (spark) |
| `screenshots/brand-mark-dark.png` | sidebar brand crop, dark |
| `screenshots/brand-mark-light.png` | sidebar brand crop, light |

## Rules

- Three colours only. No gradients in the mascot; the tile behind it is solid
  black.
- The face must stay inside the middle 66 dp on Android, and occupy ≥ 90 % of
  the tile height on desktop icons.
- Check every change at 16, 24, 32, 48, 128 and 256 px before shipping.
