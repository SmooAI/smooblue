# /// script
# requires-python = ">=3.11"
# ///
"""Render the Smooblue app icon in the Smoo AI ringed family.

Same frame as Big Smooth, SmoothFlow and the Smoo AI desktop app (smooth/scripts/gen-ring-icons.py): a navy disc
inside the Smoo AI avatar's gradient band, on a transparent canvas, with the band's outer edge at 404 of 1024 so
macOS frames all four in the same grey tile. Inside: the Smooblue butterfly.

Writes assets/icon.svg (scripts/build-icons.sh and scripts/bundle-macos.sh rasterize it).

Run from the repo root:  uv run scripts/gen-icon.py [--variant face|plain] [--out PATH]
"""

import argparse
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SIZE = 1024
GROUND = "#0f1527"
# Band stops by math angle (deg, CCW from +x), sampled from the Smoo AI social avatar (same as the smooth generator).
STOPS = [(-60, (0x00, 0xA6, 0xA6)), (80, (0xFF, 0x6B, 0x6C)), (180, (0xF4, 0x9F, 0x0A)), (300, (0x00, 0xA6, 0xA6))]

# One wing pair, right side, in a 100-unit box centred on the body (mirrored for the left).
UPPER = "M2.5,-4 C9,-23 29,-41 46,-39 C53,-38 51,-21 43,-9 C35,2 15,4 2.5,1.5 Z"
UPPER_PANEL = "M7,-6 C13,-20 27,-32 39,-32 C43,-31 42,-21 37,-13 C31,-5 18,-2 7,-2 Z"
LOWER = "M2.5,4 C13,3.5 31,8 35.5,21 C38,31 29,39.5 19,36 C10,32.5 5,20 2.5,8 Z"
LOWER_PANEL = "M7,8 C15,8.5 27,12 30,21 C31.5,28 25.5,32.5 19.5,30.5 C13,28 9,18 7,11 Z"
VEINS = ["M4,-2 C16,-12 28,-24 40,-33", "M4,-1 C18,-6 30,-10 42,-12", "M4,6 C14,12 22,20 27,30"]


def color(a):
    a = (a + 60) % 360 - 60
    for (a0, c0), (a1, c1) in zip(STOPS, STOPS[1:]):
        if a0 <= a <= a1:
            t = (a - a0) / (a1 - a0)
            return "#%02x%02x%02x" % tuple(round(x + (y - x) * t) for x, y in zip(c0, c1))


def ring(cx, cy, r, width, n=120):
    step = 360 / n
    out = []
    for i in range(n):
        a0 = i * step
        a1 = a0 + step + 0.6  # overlap hides seams
        x0, y0 = cx + r * math.cos(math.radians(a0)), cy - r * math.sin(math.radians(a0))
        x1, y1 = cx + r * math.cos(math.radians(a1)), cy - r * math.sin(math.radians(a1))
        out.append(f'<path d="M{x0:.3f},{y0:.3f}A{r:.3f},{r:.3f} 0 0 0 {x1:.3f},{y1:.3f}" fill="none" stroke="{color(a0 + step / 2)}" stroke-width="{width:.3f}"/>')
    return out


def wings():
    out = []
    for sx in (1, -1):
        g = f'<g transform="scale({sx},1)">'
        out.append(g)
        out.append(f'<path d="{UPPER}" fill="url(#wing)" stroke="#9fe6ff" stroke-width=".7" stroke-linejoin="round"/>')
        out.append(f'<path d="{LOWER}" fill="url(#wing2)" stroke="#9fe6ff" stroke-width=".7" stroke-linejoin="round"/>')
        out.append(f'<path d="{UPPER_PANEL}" fill="url(#panel)"/>')
        out.append(f'<path d="{LOWER_PANEL}" fill="url(#panel)"/>')
        out += [f'<path d="{v}" fill="none" stroke="#e6fbff" stroke-opacity=".35" stroke-width=".55" stroke-linecap="round"/>' for v in VEINS]
        # Wing-tip sparks: the Smoo band's warm colours, so the butterfly belongs in the frame.
        out.append('<circle cx="43" cy="-33" r="1.6" fill="#ff8a5c"/>')
        out.append('<circle cx="31" cy="30" r="1.2" fill="#f4a90a"/>')
        out.append("</g>")
    return out


def head(variant):
    out = [
        '<path d="M-1.6,-19 C-4,-26 -8,-31 -12,-33" fill="none" stroke="#7fd8ff" stroke-width="1" stroke-linecap="round"/>',
        '<path d="M1.6,-19 C4,-26 8,-31 12,-33" fill="none" stroke="#7fd8ff" stroke-width="1" stroke-linecap="round"/>',
        '<circle cx="-12" cy="-33" r="1.8" fill="#bff3ff"/>',
        '<circle cx="12" cy="-33" r="1.8" fill="#bff3ff"/>',
        '<ellipse cy="5" rx="2.6" ry="19" fill="url(#body)"/>',
    ]
    # The face variant gets a bigger head so the face still reads at dock size.
    out.append('<g transform="translate(0 -15) scale(1.35) translate(0 15)">' if variant == "face" else "<g>")
    out.append('<circle cy="-15" r="6" fill="url(#head)" stroke="#bff3ff" stroke-width=".5"/>')
    if variant == "face":
        # Almond eyes with a glint: still the friendly alien, less cartoon.
        for x, rot in ((-2.3, 20), (2.3, -20)):
            out.append(f'<ellipse cx="{x}" cy="-15.6" rx="1.9" ry="1.15" transform="rotate({rot} {x} -15.6)" fill="#061226"/>')
            out.append(f'<circle cx="{x + 0.5}" cy="-16.1" r=".45" fill="#fff"/>')
        out.append('<path d="M-1.3,-12.3 Q0,-11.2 1.3,-12.3" fill="none" stroke="#061226" stroke-width=".5" stroke-linecap="round"/>')
    out.append("</g>")
    return out


def svg(variant="face"):
    c = SIZE / 2
    pad = 100 / SIZE
    disc = SIZE * (1 - 2 * pad)
    ring_w, gap = disc * 6 / 128, disc * 4 / 128
    outer = c * (1 - 2 * pad) - SIZE / 128
    r = outer - ring_w / 2
    inner = r - ring_w / 2 - gap
    k = inner * 0.0175  # 100-unit butterfly box -> ~87% of the inner disc
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}" width="{SIZE}" height="{SIZE}">',
        "<defs>",
        '<linearGradient id="wing" x1="0" y1="1" x2="1" y2="0"><stop offset="0" stop-color="#5ff2ff"/><stop offset=".45" stop-color="#1185fe"/><stop offset="1" stop-color="#4b3dff"/></linearGradient>',
        '<linearGradient id="wing2" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#3fd7ff"/><stop offset=".5" stop-color="#0f6fe6"/><stop offset="1" stop-color="#3a2fd6"/></linearGradient>',
        '<linearGradient id="panel" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ffffff" stop-opacity=".32"/><stop offset="1" stop-color="#ffffff" stop-opacity="0"/></linearGradient>',
        '<linearGradient id="body" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#bff3ff"/><stop offset="1" stop-color="#1185fe"/></linearGradient>',
        '<radialGradient id="head" cx=".35" cy=".3" r=".8"><stop offset="0" stop-color="#e8fbff"/><stop offset=".5" stop-color="#5cc8ff"/><stop offset="1" stop-color="#1167e8"/></radialGradient>',
        '<radialGradient id="glow"><stop offset="0" stop-color="#1e90ff" stop-opacity=".55"/><stop offset=".6" stop-color="#1e5bff" stop-opacity=".15"/><stop offset="1" stop-color="#1e5bff" stop-opacity="0"/></radialGradient>',
        f'<clipPath id="disc"><circle cx="{c}" cy="{c}" r="{inner:.3f}"/></clipPath>',
        "</defs>",
        f'<circle cx="{c}" cy="{c}" r="{outer:.3f}" fill="{GROUND}"/>',
        *ring(c, c, r, ring_w),
        f'<g clip-path="url(#disc)"><circle cx="{c}" cy="{c * 1.0}" r="{inner * 0.95:.3f}" fill="url(#glow)"/></g>',
        f'<g transform="translate({c},{c + inner * 0.04:.3f}) scale({k:.4f})">',
        *wings(),
        *head(variant),
        "</g>",
        "</svg>",
    ]
    return "\n".join(out) + "\n"


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--variant", choices=["face", "plain"], default="face")
    p.add_argument("--out", default=str(ROOT / "assets/icon.svg"))
    a = p.parse_args()
    Path(a.out).write_text(svg(a.variant))
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
