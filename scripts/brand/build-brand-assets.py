#!/usr/bin/env python3
"""Generate every Sarthi Studio brand SVG from one tardigrade face.

The mark is the mascot's face, not its body: a plump head, two big eyes and a
smile, drawn flat in exactly three colours — orange, black and white. No
gradients, so it stays crisp at 16 px and reads the same on light and dark
surfaces.

Everything derives from the constants below, so the face changes in one place
and the app icon, favicon, mono marks and splash follow.

    python scripts/brand/build-brand-assets.py

Raster sizes (`app/icons/icon.png`, `icon.ico`, the packaging 256 png) render
from `app-icon.svg`; see docs/BRAND-ASSETS.md for that step.
"""

from pathlib import Path

OUT = Path("app/static/brand")
ORANGE = "#FF7A1A"
BLACK = "#111111"
WHITE = "#FFFFFF"

# Head: a plump rounded square, wider at the cheeks than the crown.
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
    """One-colour version: eyes punched out of the head with even-odd."""
    holes = "".join(
        f"M{cx + 12.4} {cy}a12.4 12.4 0 1 0-24.8 0a12.4 12.4 0 1 0 24.8 0Z"
        for cx, cy in EYES
    )
    return f'<path d="{HEAD} {holes}" fill="{fill}" fill-rule="evenodd"/>'


def doc(inner, size=128, background=""):
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" '
        f'width="{size}" height="{size}" role="img" aria-label="Sarthi Studio tardigrade">'
        f"{background}{inner}\n</svg>\n"
    )


def mark(size=24):
    """The 24 px glyph used inline by the UI: same face, tuned strokes."""
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="{size}" '
        f'height="{size}" role="img" aria-label="Sarthi Studio">'
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


def app_icon(size=512):
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}" width="{size}" height="{size}" role="img" aria-label="Sarthi Studio">
  <rect width="{size}" height="{size}" fill="{BLACK}"/>
  <g transform="translate({size/2} {size/2}) scale({size/128*0.92:.3f}) translate(-64 -64)">{face()}
  </g>
</svg>
"""


def favicon():
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64" role="img" aria-label="Sarthi Studio">
  <rect x="2" y="2" width="60" height="60" rx="13" fill="{BLACK}"/>
  <g transform="translate(32 33) scale(0.40) translate(-64 -64)">{face()}
  </g>
</svg>
"""


def splash():
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 640 360" width="640" height="360" role="img" aria-label="Sarthi Studio">
  <g transform="translate(104 180) scale(1.15) translate(-64 -64)">{face()}
  </g>
  <text x="212" y="174" font-family="Inter, 'Segoe UI', system-ui, sans-serif" font-size="58"
        font-weight="700" letter-spacing="-1.5" fill="{WHITE}">Sarthi Studio</text>
  <text x="216" y="210" font-family="Inter, 'Segoe UI', system-ui, sans-serif" font-size="18"
        letter-spacing="4.2" fill="{ORANGE}">LOCAL CODING AGENT</text>
</svg>
"""


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    written = {
        "tardigrade.svg": doc(face()),
        "tardigrade-mono-white.svg": doc(silhouette(WHITE)),
        "tardigrade-mono-black.svg": doc(silhouette(BLACK)),
        "tardigrade-mark.svg": mark(),
        "app-icon.svg": app_icon(),
        "favicon.svg": favicon(),
        "splash.svg": splash(),
    }
    for name, content in written.items():
        (OUT / name).write_text(content, encoding="utf-8", newline="\n")
    for stale in ("tardigrade-dark.svg", "tardigrade-light.svg"):
        (OUT / stale).unlink(missing_ok=True)
    print(f"wrote {len(written)} svg files to {OUT}")


if __name__ == "__main__":
    main()
