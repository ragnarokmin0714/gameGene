#!/usr/bin/env python3
"""Generate the GameGene brand mark: a radar + gene-helix motif.

Single source of truth for the icon. Emits:
  - assets/gamegene.svg               vector logo (README / branding)
  - crates/gamegene-app/assets/icon.rgba   raw RGBA 256x256 (window icon)
  - assets/gamegene-logo.png          raster preview

Run from the repo root:  python3 assets/make_icon.py
"""
import math
import os
import struct

SIZE = 256
C = SIZE / 2
RINGS = [46, 80, 113]          # radar ring radii
HELIX_A = 32                   # helix amplitude
HELIX_Y0, HELIX_Y1 = 46, 210   # helix vertical span
HELIX_TURNS = 1.5
HELIX_N = 14

# palette
BG = (14, 42, 71)
RING = (74, 144, 217)
SWEEP = (92, 170, 236)
DOT = (130, 195, 255)
STRAND_A = (245, 250, 255)
STRAND_B = (120, 175, 235)
RUNG = (150, 200, 245)

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)


def helix_points():
    pts = []
    for i in range(HELIX_N):
        t = i / (HELIX_N - 1)
        y = HELIX_Y0 + t * (HELIX_Y1 - HELIX_Y0)
        phase = t * 2 * math.pi * HELIX_TURNS
        x1 = C + HELIX_A * math.cos(phase)
        x2 = C - HELIX_A * math.cos(phase)
        front_a = math.sin(phase) >= 0   # which strand is in front
        pts.append((x1, x2, y, front_a))
    return pts


def rgba(color, a=255):
    return f"rgba({color[0]},{color[1]},{color[2]},{a/255:.2f})"


def write_svg():
    p = helix_points()
    e = []
    e.append(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}">')
    e.append(f'<rect x="8" y="8" width="{SIZE-16}" height="{SIZE-16}" rx="52" '
             f'fill="rgb({BG[0]},{BG[1]},{BG[2]})"/>')
    # radar rings
    for r in RINGS:
        e.append(f'<circle cx="{C}" cy="{C}" r="{r}" fill="none" '
                 f'stroke="{rgba(RING,150)}" stroke-width="3"/>')
    # sweep line + origin dot
    e.append(f'<line x1="{C}" y1="{C}" x2="{C}" y2="{C-RINGS[-1]}" '
             f'stroke="{rgba(SWEEP,220)}" stroke-width="3" stroke-linecap="round"/>')
    e.append(f'<circle cx="{C}" cy="{C}" r="5" fill="rgb({DOT[0]},{DOT[1]},{DOT[2]})"/>')
    # rungs
    for i, (x1, x2, y, _) in enumerate(p):
        if i % 2 == 1:
            e.append(f'<line x1="{x1:.1f}" y1="{y:.1f}" x2="{x2:.1f}" y2="{y:.1f}" '
                     f'stroke="{rgba(RUNG,150)}" stroke-width="2.5"/>')
    # strand dots (draw back first)
    for (x1, x2, y, front_a) in p:
        back, frontc = (STRAND_B, STRAND_A) if front_a else (STRAND_A, STRAND_B)
        bx, fx = (x2, x1) if front_a else (x1, x2)
        e.append(f'<circle cx="{bx:.1f}" cy="{y:.1f}" r="4.5" '
                 f'fill="rgb({back[0]},{back[1]},{back[2]})"/>')
        e.append(f'<circle cx="{fx:.1f}" cy="{y:.1f}" r="6" '
                 f'fill="rgb({frontc[0]},{frontc[1]},{frontc[2]})"/>')
    e.append('</svg>')
    with open(os.path.join(ROOT, "assets", "gamegene.svg"), "w") as f:
        f.write("\n".join(e) + "\n")


def draw_raster():
    from PIL import Image, ImageDraw
    ss = 4
    n = SIZE * ss
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    def S(v):
        return v * ss

    def circle(cx, cy, r, fill=None, outline=None, width=1):
        d.ellipse([S(cx - r), S(cy - r), S(cx + r), S(cy + r)],
                  fill=fill, outline=outline, width=S(width))

    d.rounded_rectangle([S(8), S(8), S(SIZE - 8), S(SIZE - 8)], radius=S(52), fill=BG + (255,))
    for r in RINGS:
        circle(C, C, r, outline=RING + (150,), width=3)
    d.line([S(C), S(C), S(C), S(C - RINGS[-1])], fill=SWEEP + (230,), width=S(3))
    circle(C, C, 5, fill=DOT + (255,))

    p = helix_points()
    for i, (x1, x2, y, _) in enumerate(p):
        if i % 2 == 1:
            d.line([S(x1), S(y), S(x2), S(y)], fill=RUNG + (150,), width=S(2))
    for (x1, x2, y, front_a) in p:
        back, frontc = (STRAND_B, STRAND_A) if front_a else (STRAND_A, STRAND_B)
        bx, fx = (x2, x1) if front_a else (x1, x2)
        circle(bx, y, 4.5, fill=back + (255,))
        circle(fx, y, 6, fill=frontc + (255,))

    img = img.resize((SIZE, SIZE), Image.LANCZOS)
    os.makedirs(os.path.join(ROOT, "crates", "gamegene-app", "assets"), exist_ok=True)
    with open(os.path.join(ROOT, "crates", "gamegene-app", "assets", "icon.rgba"), "wb") as f:
        f.write(img.tobytes())          # raw RGBA, row-major
    img.save(os.path.join(ROOT, "assets", "gamegene-logo.png"))


if __name__ == "__main__":
    write_svg()
    draw_raster()
    print("wrote gamegene.svg, icon.rgba, gamegene-logo.png")
