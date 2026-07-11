#!/usr/bin/env python3
"""Generate the GameGene brand mark: a flat, line-only radar + gene-helix motif
with a steampunk (brass + verdigris) palette, in light and dark variants.

Single source of truth. Emits, from the repo root:
  - assets/gamegene.svg          light logo (README default)
  - assets/gamegene-dark.svg     dark logo (README dark mode)
  - assets/gamegene-logo.png     light raster preview
  - assets/gamegene-logo-dark.png
  - crates/gamegene-app/assets/icon.rgba   256x256 RGBA window icon (light)

Run:  python3 assets/make_icon.py
"""
import math
import os

SIZE = 256
C = SIZE / 2
RINGS = [68, 106]              # few, faint radar rings
HELIX_A = 30
HELIX_Y0, HELIX_Y1 = 54, 202
HELIX_TURNS = 1.6
HELIX_N = 48
RUNG_TS = [0.12, 0.32, 0.5, 0.68, 0.88]

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

# Steampunk palette: brass / copper line work, verdigris rungs.
LIGHT = dict(
    bg=(244, 237, 223), border=(214, 201, 174),
    ring=(176, 137, 78), ring_a=70, sweep=(176, 137, 78), sweep_a=110,
    strand=(168, 106, 44), strand2=(196, 138, 62), rung=(46, 125, 107),
)
DARK = dict(
    bg=(33, 30, 24), border=(64, 58, 46),
    ring=(199, 154, 90), ring_a=70, sweep=(199, 154, 90), sweep_a=120,
    strand=(217, 161, 92), strand2=(232, 196, 138), rung=(79, 179, 154),
)


def strands():
    s1, s2 = [], []
    for i in range(HELIX_N):
        t = i / (HELIX_N - 1)
        y = HELIX_Y0 + t * (HELIX_Y1 - HELIX_Y0)
        x = HELIX_A * math.cos(t * 2 * math.pi * HELIX_TURNS)
        s1.append((C + x, y))
        s2.append((C - x, y))
    return s1, s2


def rungs():
    out = []
    for t in RUNG_TS:
        y = HELIX_Y0 + t * (HELIX_Y1 - HELIX_Y0)
        x = HELIX_A * math.cos(t * 2 * math.pi * HELIX_TURNS)
        if abs(2 * x) >= 10:  # skip near crossings
            out.append(((C + x, y), (C - x, y)))
    return out


def rgb(c):
    return f"rgb({c[0]},{c[1]},{c[2]})"


def rgba(c, a):
    return f"rgba({c[0]},{c[1]},{c[2]},{a/255:.2f})"


def poly(points):
    return "M " + " L ".join(f"{x:.1f} {y:.1f}" for x, y in points)


def build_svg(p):
    s1, s2 = strands()
    e = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}">']
    e.append(f'<rect x="8" y="8" width="{SIZE-16}" height="{SIZE-16}" rx="54" '
             f'fill="{rgb(p["bg"])}" stroke="{rgb(p["border"])}" stroke-width="2"/>')
    for r in RINGS:
        e.append(f'<circle cx="{C}" cy="{C}" r="{r}" fill="none" '
                 f'stroke="{rgba(p["ring"], p["ring_a"])}" stroke-width="2"/>')
    e.append(f'<line x1="{C}" y1="{C}" x2="{C}" y2="{C-RINGS[-1]}" '
             f'stroke="{rgba(p["sweep"], p["sweep_a"])}" stroke-width="2" '
             f'stroke-linecap="round"/>')
    for a, b in rungs():
        e.append(f'<line x1="{a[0]:.1f}" y1="{a[1]:.1f}" x2="{b[0]:.1f}" y2="{b[1]:.1f}" '
                 f'stroke="{rgb(p["rung"])}" stroke-width="5" stroke-linecap="round"/>')
    for pts, col in ((s2, p["strand2"]), (s1, p["strand"])):
        e.append(f'<path d="{poly(pts)}" fill="none" stroke="{rgb(col)}" '
                 f'stroke-width="6" stroke-linecap="round" stroke-linejoin="round"/>')
    e.append('</svg>')
    return "\n".join(e) + "\n"


def build_png(p):
    from PIL import Image, ImageDraw
    ss = 4
    n = SIZE * ss
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def S(v):
        return v * ss

    def sp(pts):
        return [(S(x), S(y)) for x, y in pts]

    d.rounded_rectangle([S(8), S(8), S(SIZE - 8), S(SIZE - 8)], radius=S(54),
                        fill=p["bg"] + (255,), outline=p["border"] + (255,), width=S(2))
    for r in RINGS:
        d.ellipse([S(C - r), S(C - r), S(C + r), S(C + r)],
                  outline=p["ring"] + (p["ring_a"],), width=S(2))
    d.line([S(C), S(C), S(C), S(C - RINGS[-1])], fill=p["sweep"] + (p["sweep_a"],), width=S(2))
    for a, b in rungs():
        d.line([S(a[0]), S(a[1]), S(b[0]), S(b[1])], fill=p["rung"] + (255,), width=S(5))
    s1, s2 = strands()
    for pts, col in ((s2, p["strand2"]), (s1, p["strand"])):
        d.line(sp(pts), fill=col + (255,), width=S(6), joint="curve")

    return img.resize((SIZE, SIZE), Image.LANCZOS)


def main():
    with open(os.path.join(ROOT, "assets", "gamegene.svg"), "w") as f:
        f.write(build_svg(LIGHT))
    with open(os.path.join(ROOT, "assets", "gamegene-dark.svg"), "w") as f:
        f.write(build_svg(DARK))
    light = build_png(LIGHT)
    dark = build_png(DARK)
    light.save(os.path.join(ROOT, "assets", "gamegene-logo.png"))
    dark.save(os.path.join(ROOT, "assets", "gamegene-logo-dark.png"))
    app_assets = os.path.join(ROOT, "crates", "gamegene-app", "assets")
    os.makedirs(app_assets, exist_ok=True)
    with open(os.path.join(app_assets, "icon.rgba"), "wb") as f:
        f.write(light.tobytes())
    # Multi-resolution .ico for the Windows executable resource, so the taskbar,
    # Explorer, and pinned shortcuts show the brand icon (the runtime window
    # icon set via with_icon does not cover those).
    light.save(
        os.path.join(app_assets, "icon.ico"),
        format="ICO",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print("wrote light/dark svg + png, icon.rgba and icon.ico")


if __name__ == "__main__":
    main()
