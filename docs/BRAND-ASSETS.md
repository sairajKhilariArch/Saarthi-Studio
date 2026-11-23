# Brand assets — the guide-path logo and the tardigrade mascot

The product logo is the **guide path**: a rounded orange **S** on a black tile
with a white start node. It reads as a route through a project, and it stays
legible from 16 px in a browser tab to a 512 px installer icon.

The tardigrade is still the mascot. Its face appears in the assistant avatar,
the welcome state and the website CTA; it is not the product logo.

Both are drawn flat in exactly three colours: orange `#FF7A1A`, black
`#111111`, white `#FFFFFF`. No gradients.

Everything is generated from one geometry in
`scripts/brand/build-brand-assets.py`; do not hand-edit the SVGs.

```bash
python scripts/brand/build-brand-assets.py
```

The script writes `app/static/brand/`, plus the logo set into
`website/public/brand/` and `vscode-extension/media/ui/brand/`.

## Inventory

| File | Purpose | Used by |
|---|---|---|
| `app/static/brand/logo.svg` | guide-path S on a rounded black tile | website header/footer, app sidebar/welcome, extension webview |
| `app/static/brand/logo-mono-white.svg` | one-colour white S | dark surfaces, single-colour print |
| `app/static/brand/logo-mono-black.svg` | one-colour black S | light surfaces |
| `app/static/brand/tardigrade.svg` | full-colour mascot face | README, release artwork |
| `app/static/brand/tardigrade-mono-white.svg` | one-colour mascot silhouette | dark surfaces, single-colour print |
| `app/static/brand/tardigrade-mono-black.svg` | one-colour mascot silhouette | light surfaces |
| `app/static/brand/tardigrade-mark.svg` | 24 px mascot glyph | reference for the inline assistant avatar |
| `app/static/brand/app-icon.svg` | full-bleed black tile + logo | source for `icon.png`, `icon.ico`, the packaging PNG, Android |
| `app/static/brand/favicon.svg` | 64 px rounded black tile + logo | browser tab (`<link rel="icon">`) |
| `app/static/brand/splash.svg` | logo + wordmark, 640×360, transparent | splash / release banner |
| `app/icons/icon.png` | 512×512 RGBA raster of `app-icon.svg` | Tauri window/app icon |
| `app/icons/icon.ico` | 16–256 multi-size ICO of the same render | Windows executable, MSI |
| `app/packaging/icons/SAARTHI-STUDIO-256.png` | 256×256 PNG | AppImage, `.deb` hicolor theme |

The inline mascot geometry lives in `app/static/index.html` as `<symbol
id="i-tardigrade">`; it is used only by the assistant avatar and chat welcome
state. The sidebar brand uses `logo.svg`.

## Rasterising

Render from `app-icon.svg`, then build the PNG and ICO with ImageMagick. On
Linux and macOS:

```bash
rsvg-convert -w 512 -h 512 app/static/brand/app-icon.svg -o /tmp/icon.png

magick /tmp/icon.png -background none -alpha on app/icons/icon.png
magick app/icons/icon.png -resize 256x256 \
  app/packaging/icons/SAARTHI-STUDIO-256.png
magick app/icons/icon.png \
  -define icon:auto-resize=256,128,64,48,32,24,16 app/icons/icon.ico
```

`icon.png` must stay 512×512 RGBA; Tauri rejects a non-RGBA app icon. The ICO
frames inherit the alpha channel.

## Platform mapping

| Platform | Asset |
|---|---|
| Browser tab | `/static/brand/favicon.svg` |
| Sidebar / welcome | `logo.svg` |
| Assistant avatar | inline `#i-tardigrade` symbol |
| Windows executable + MSI | `app/icons/icon.ico` (Tauri `bundle.icon`) |
| Linux .deb / AppImage | `app/static/brand/app-icon.svg` (installed as `usr/share/icons/hicolor/scalable/apps/SAARTHI-STUDIO.svg`) and `SAARTHI-STUDIO-256.png`; desktop entry `Icon=SAARTHI-STUDIO` |
| macOS bundle | `app/icons/icon.png` via Tauri, `CFBundleName` = SAARTHI-STUDIO |
| Android | `res/drawable/ic_launcher_foreground.xml` (orange S, centred in the 66 dp safe zone) over `ic_launcher_background` `#111111`; adaptive + monochrome layers |
| VS Code webview | `vscode-extension/media/ui/brand/` (copied from the same script) |

## Screenshots

| File | What it shows |
|---|---|
| `screenshots/brand-shell-before.png` | the shell at 2.51.0, before the brand change (spark glyph) |
| `screenshots/brand-shell-dark.png` | the shell in dark theme with the sidebar mark |
| `screenshots/brand-shell-light.png` | the same in light theme |
| `screenshots/brand-mark-before.png` | sidebar brand crop at 2.51.0 (spark) |
| `screenshots/brand-mark-dark.png` | sidebar brand crop, dark |
| `screenshots/brand-mark-light.png` | sidebar brand crop, light |

## Rules

- Three colours only. No gradients in the logo or mascot; the tile behind the
  logo is solid black.
- The S must stay inside the middle 66 dp on Android.
- Check every logo change at 16, 24, 32, 48, 128 and 256 px before shipping.
- Keep the mascot for personality, but do not use it as the header or installer
  mark.
