#!/usr/bin/env python3
"""Generate every SAARTHI-STUDIO brand SVG from one geometry.

The product logo is the guide-path **S**: a rounded route in orange on a black
tile, with a white start node. It replaces the old mascot-face mark in the
header, the app shell, the favicon, the installer icons and the Android
launcher.

The tardigrade face remains the mascot. It is still generated here because the
assistant avatar, the welcome state and the release artwork use it.

    python scripts/brand/build-brand-assets.py

Raster sizes (`app/icons/icon.png`, `icon.ico`, the packaging 256 png) render
from `app-icon.svg`; see docs/BRAND-ASSETS.md for that step.
"""

from pathlib import Path

OUT = Path("app/static/brand")
WEBSITE_OUT = Path("website/public/brand")
EXTENSION_OUT = Path("vscode-extension/media/ui/brand")
ORANGE = "#FF7A1A"
BLACK = "#111111"
WHITE = "#FFFFFF"

# The guide-path S. Coordinates live in a 128-unit square; the glyph is centred
# on y=74 so the curved route sits optically in the middle of the tile.
LOGO_PATH = "M92 38C60 38 40 46 40 64s48 18 48 36-14 16-30 14"
LOGO_STROKE = 14
LOGO_DOT = (92, 38, 6)

# Mascot face: a plump rounded head with big eyes and a smile.
HEAD = (
    "M64 11c23.2 0 39 15.6 39 38.4v9.8c0 9.4-2.6 17.4-7.8 23.6"
    "C89 90.6 78.6 95 64 95s-25-4.4-31.2-12.2C27.6 76.6 25 68.6 25 59.2v-9.8"
    "C25 26.6 40.8 11 64 11Z"
)
EYES = ((48.2, 55.4), (79.8, 55.4))
PUPILS = ((49.4, 56.6), (81, 56.6))


def face(orange=ORANGE, black=BLACK, white=WHITE, outline=True):
    eye_parts = []
    for (cx, cy), (px, py) in zip(EYES, PUPILS):
        eye_parts.append(
            f'<circle cx="{cx}" cy="{cy}" r="12.4" fill="{white}" stroke="{black}" stroke-width="3"/>'
            f'<circle cx="{px}" cy="{py}" r="5.8" fill="{black}"/>'
            f'<circle cx="{px - 1.9}" cy="{py - 2.4}" r="1.7" fill="{white}"/>'
        )
    stroke = f' stroke="{black}" stroke-width="4.6"' if outline else ""
    return f"""
  <path d="{HEAD}" fill="{orange}"{stroke} stroke-linejoin="round"/>
  <path d="M38 30c4.6-6 11-9.4 18.6-10.6" fill="none" stroke="{white}"
        stroke-width="4.4" stroke-linecap="round" opacity="0.92"/>
  {''.join(eye_parts)}
  <path d="M52 76.5c3.6 4.6 8.4 6.9 12 6.9s8.4-2.3 12-6.9" fill="none"
        stroke="{black}" stroke-width="4.4" stroke-linecap="round"/>"""


def silhouette(fill):
    """One-colour mascot: eyes punched out of the head with even-odd."""
    holes = "".join(
        f"M{cx + 12.4} {cy}a12.4 12.4 0 1 0-24.8 0a12.4 12.4 0 1 0 24.8 0Z"
        for cx, cy in EYES
    )
    return f'<path d="{HEAD} {holes}" fill="{fill}" fill-rule="evenodd"/>'


def logo_glyph(color=ORANGE, dot=None):
    dot_svg = ""
    if dot:
        cx, cy, radius = LOGO_DOT
        dot_svg = f'<circle cx="{cx}" cy="{cy}" r="{radius}" fill="{dot}"/>'
    return (
        '<g transform="translate(64 64) translate(-64 -74)">'
        f'<path d="{LOGO_PATH}" fill="none" stroke="{color}" '
        f'stroke-width="{LOGO_STROKE}" stroke-linecap="round" stroke-linejoin="round"/>'
        f"{dot_svg}</g>"
    )


def doc(inner, size=128, background="", label="SAARTHI-STUDIO"):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" '
        f'width="{size}" height="{size}" role="img" aria-label="{label}">'
        f"{background}{inner}\n</svg>\n"
    )


def logo(rounded=True, size=128, inset=0):
    radius = 28 if rounded else 0
    if rounded:
        radius = 26 if inset else 28
    rect = (
        f'<rect x="{inset}" y="{inset}" width="{128 - inset * 2}" '
        f'height="{128 - inset * 2}" rx="{radius}" fill="{BLACK}"/>'
    )
    return doc(
        rect + logo_glyph(dot=WHITE),
        size=size,
        label="SAARTHI-STUDIO logo",
    )


def mono(color):
    return doc(logo_glyph(color=color), label="SAARTHI-STUDIO logo")


def mark(size=24):
    """The 24 px mascot glyph used inline by the assistant avatar."""
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="{size}" '
        f'height="{size}" role="img" aria-label="SAARTHI-STUDIO mascot">'
        '<path d="M12 2.4c4.6 0 7.7 3.1 7.7 7.6v2c0 1.9-.5 3.5-1.6 4.7'
        '-1.2 1.5-3.3 2.4-6.1 2.4s-4.9-.9-6.1-2.4C4.8 15.5 4.3 13.9 4.3 12v-2'
        f'c0-4.5 3.1-7.6 7.7-7.6Z" fill="{ORANGE}" stroke="{BLACK}" stroke-width="1.3"/>'
        f'<circle cx="9.4" cy="10.3" r="2.5" fill="{WHITE}" stroke="{BLACK}" stroke-width="0.9"/>'
        f'<circle cx="14.6" cy="10.3" r="2.5" fill="{WHITE}" stroke="{BLACK}" stroke-width="0.9"/>'
        f'<circle cx="9.7" cy="10.6" r="1.15" fill="{BLACK}"/>'
        f'<circle cx="14.3" cy="10.6" r="1.15" fill="{BLACK}"/>'
        f'<path d="M9.6 15.1c.8 1 1.6 1.5 2.4 1.5s1.6-.5 2.4-1.5" fill="none" '
        f'stroke="{BLACK}" stroke-width="1.1" stroke-linecap="round"/></svg>\n'
    )


def app_icon():
    return logo(rounded=False, size=512)


def favicon():
    return logo(rounded=True, size=64, inset=4)


def splash():
    glyph = logo_glyph(dot=WHITE)
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360" width="640" height="360" role="img" aria-label="SAARTHI-STUDIO">
  <g transform="translate(72 116)">
    <rect width="128" height="128" rx="28" fill="{BLACK}"/>
    {glyph}
  </g>
  <text x="224" y="180" font-family="Inter, 'Segoe UI', system-ui, sans-serif" font-size="58"
        font-weight="700" letter-spacing="-1.5" fill="{WHITE}">SAARTHI-STUDIO</text>
  <text x="228" y="216" font-family="Inter, 'Segoe UI', system-ui, sans-serif" font-size="18"
        letter-spacing="4.2" fill="{ORANGE}">LOCAL CODING AGENT</text>
</svg>
"""


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    written = {
        "logo.svg": logo(),
        "logo-mono-white.svg": mono(WHITE),
        "logo-mono-black.svg": mono(BLACK),
        "tardigrade.svg": doc(face(), label="SAARTHI-STUDIO mascot"),
        "tardigrade-mono-white.svg": doc(silhouette(WHITE), label="SAARTHI-STUDIO mascot"),
        "tardigrade-mono-black.svg": doc(silhouette(BLACK), label="SAARTHI-STUDIO mascot"),
        "tardigrade-mark.svg": mark(),
        "app-icon.svg": app_icon(),
        "favicon.svg": favicon(),
        "splash.svg": splash(),
    }
    for name, content in written.items():
        (OUT / name).write_text(content, encoding="utf-8", newline="\n")

    for out in (WEBSITE_OUT, EXTENSION_OUT):
        out.mkdir(parents=True, exist_ok=True)
        for name in ("logo.svg", "logo-mono-white.svg", "logo-mono-black.svg"):
            (out / name).write_text(written[name], encoding="utf-8", newline="\n")

    for stale in ("tardigrade-dark.svg", "tardigrade-light.svg"):
        (OUT / stale).unlink(missing_ok=True)
    print(f"wrote {len(written)} svg files to {OUT}, plus the logo to the website and extension")


if __name__ == "__main__":
    main()
