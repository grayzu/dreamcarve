#!/usr/bin/env python3
# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0
"""Renders the phase 4 wireframes in this folder as SVG.

    python3 render.py          # writes NN-*.svg beside this script

The wireframes are low-fidelity on purpose: grey boxes, real labels, real
field names, no colour except where the spec gives a colour a meaning (the
operation palette, the error red, the selection blue). Edit a scene function
and re-run; do not edit the SVGs by hand. Standard library only.
"""

from __future__ import annotations

import math
import os
from dataclasses import dataclass, field

# ── Style ───────────────────────────────────────────────────────────────

FONT = "Helvetica, Arial, sans-serif"
MONO = "Menlo, Consolas, monospace"
INK = "#2b2b2b"
MUTED = "#7a7a7a"
LINE = "#9a9a9a"
PANEL = "#f4f4f4"
HEADER = "#e6e6e6"
WHITE = "#ffffff"
SELECT = "#2f6fdb"
ERROR = "#c62828"
WARN = "#b26a00"
STOCK = "#7f7f7f"
# The --preview palette (crates/dreamcarve/src/preview.rs), so the mock-ups
# and the SVG preview agree.
PALETTE = ["#1f77b4", "#d62728", "#2ca02c", "#9467bd", "#ff7f0e", "#17becf", "#8c564b", "#e377c2"]
RAPID = "#808080"


def esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


@dataclass
class Svg:
    w: int
    h: int
    parts: list[str] = field(default_factory=list)

    def add(self, s: str) -> None:
        self.parts.append(s)

    def rect(self, x, y, w, h, fill=WHITE, stroke=LINE, rx=0, sw=1, dash=None, opacity=None):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        o = f' opacity="{opacity}"' if opacity is not None else ""
        self.add(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{rx}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"{d}{o}/>')

    def line(self, x1, y1, x2, y2, stroke=LINE, sw=1, dash=None):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        self.add(f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{stroke}" stroke-width="{sw}"{d}/>')

    def circle(self, cx, cy, r, fill="none", stroke=INK, sw=1, dash=None):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        self.add(f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"{d}/>')

    def ellipse(self, cx, cy, rx, ry, fill="none", stroke=INK, sw=1):
        self.add(f'<ellipse cx="{cx}" cy="{cy}" rx="{rx}" ry="{ry}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"/>')

    def path(self, d, stroke=INK, sw=1, fill="none", dash=None):
        dd = f' stroke-dasharray="{dash}"' if dash else ""
        self.add(f'<path d="{d}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"{dd}/>')

    def text(self, x, y, s, size=12, fill=INK, weight="normal", anchor="start", mono=False, italic=False):
        fam = MONO if mono else FONT
        st = ' font-style="italic"' if italic else ""
        self.add(f'<text x="{x}" y="{y}" font-family="{fam}" font-size="{size}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}"{st}>{esc(s)}</text>')

    def render(self, title: str) -> str:
        head = (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.w}" height="{self.h}" '
            f'viewBox="0 0 {self.w} {self.h}" font-family="{FONT}">\n'
            f"<title>{esc(title)}</title>\n"
            f'<rect width="{self.w}" height="{self.h}" fill="{WHITE}"/>\n'
        )
        return head + "\n".join(self.parts) + "\n</svg>\n"


# ── Widgets ─────────────────────────────────────────────────────────────


def caption(s: Svg, text: str) -> None:
    """The wireframe's caption strip along the top."""
    s.rect(0, 0, s.w, 22, fill="#fff8e1", stroke="none")
    s.text(8, 15, text, size=11, fill=MUTED, italic=True)


def panel(s: Svg, x, y, w, h, title=None, fill=PANEL):
    s.rect(x, y, w, h, fill=fill, stroke=LINE)
    if title:
        s.rect(x, y, w, 24, fill=HEADER, stroke=LINE)
        s.text(x + 8, y + 16, title, size=12, weight="bold")


def section(s: Svg, x, y, w, title, open_=True):
    """A collapsible group header. Returns the y below it."""
    s.rect(x, y, w, 20, fill=HEADER, stroke="none")
    chev = "▾" if open_ else "▸"
    s.text(x + 6, y + 14, f"{chev}  {title}", size=11, weight="bold")
    return y + 24


def field_row(s: Svg, x, y, w, label, value="", unit="", placeholder=False, error=None, reset=False, label_w=138, help_=True):
    """Label on the left, box on the right. Returns y of the next row."""
    s.text(x, y + 14, label, size=11)
    if help_:
        s.circle(x + label_w - 14, y + 10, 5, stroke=LINE)
        s.text(x + label_w - 14, y + 13, "?", size=8, fill=MUTED, anchor="middle")
    bx = x + label_w
    bw = w - label_w - (18 if reset else 0)
    s.rect(bx, y, bw, 20, fill=WHITE, stroke=ERROR if error else LINE, rx=2)
    if value or placeholder:
        s.text(bx + 6, y + 14, value, size=11, fill=MUTED if placeholder else INK, italic=placeholder)
    if unit:
        s.text(bx + bw - 6, y + 14, unit, size=10, fill=MUTED, anchor="end")
    if reset:
        s.text(bx + bw + 4, y + 15, "⟲", size=12, fill=MUTED)
    ny = y + 26
    if error:
        s.text(bx, ny + 4, error, size=10, fill=ERROR)
        ny += 16
    return ny


def dropdown(s: Svg, x, y, w, label, value, label_w=138, help_=True, disabled=False, note=None):
    s.text(x, y + 14, label, size=11, fill=MUTED if disabled else INK)
    if help_:
        s.circle(x + label_w - 14, y + 10, 5, stroke=LINE)
        s.text(x + label_w - 14, y + 13, "?", size=8, fill=MUTED, anchor="middle")
    bx = x + label_w
    bw = w - label_w
    s.rect(bx, y, bw, 20, fill=HEADER if disabled else WHITE, stroke=LINE, rx=2)
    s.text(bx + 6, y + 14, value, size=11, fill=MUTED if disabled else INK)
    s.text(bx + bw - 8, y + 14, "▾", size=10, fill=MUTED, anchor="end")
    ny = y + 26
    if note:
        s.text(bx, ny + 4, note, size=10, fill=MUTED, italic=True)
        ny += 16
    return ny


def checkbox(s: Svg, x, y, label, checked=False, disabled=False):
    s.rect(x, y + 2, 12, 12, fill=HEADER if disabled else WHITE, stroke=LINE, rx=2)
    if checked:
        s.path(f"M{x + 2} {y + 8} l3 3 l5 -6", stroke=INK, sw=1.5)
    s.text(x + 18, y + 13, label, size=11, fill=MUTED if disabled else INK)
    return y + 22


def radio(s: Svg, x, y, label, on=False):
    s.circle(x + 6, y + 8, 6, fill=WHITE, stroke=LINE)
    if on:
        s.circle(x + 6, y + 8, 3, fill=INK, stroke="none")
    s.text(x + 18, y + 13, label, size=11)
    return y + 20


def button(s: Svg, x, y, w, label, primary=False, disabled=False, h=24):
    fill = SELECT if primary else WHITE
    if disabled:
        fill = HEADER
    s.rect(x, y, w, h, fill=fill, stroke=SELECT if primary and not disabled else LINE, rx=3)
    s.text(x + w / 2, y + h / 2 + 4, label, size=11, fill=WHITE if primary and not disabled else (MUTED if disabled else INK), anchor="middle")


def tabs(s: Svg, x, y, w, labels, active):
    tw = w / len(labels)
    for i, lab in enumerate(labels):
        on = i == active
        s.rect(x + i * tw, y, tw, 24, fill=WHITE if on else HEADER, stroke=LINE)
        s.text(x + i * tw + tw / 2, y + 16, lab, size=12, weight="bold" if on else "normal", anchor="middle")


def eye(s: Svg, x, y, on=True):
    col = INK if on else LINE
    s.ellipse(x, y, 6, 3.5, stroke=col)
    if on:
        s.circle(x, y, 1.6, fill=col, stroke="none")
    else:
        s.line(x - 6, y + 4, x + 6, y - 4, stroke=col)


def lock(s: Svg, x, y, on=False):
    col = INK if on else LINE
    s.rect(x - 4, y - 1, 8, 6, fill=WHITE, stroke=col, rx=1)
    if on:
        s.path(f"M{x - 2.5} {y - 1} v-2 a2.5 2.5 0 0 1 5 0 v2", stroke=col)
    else:
        s.path(f"M{x - 2.5} {y - 1} v-2 a2.5 2.5 0 0 1 5 0 v0.5", stroke=col)


def grip(s: Svg, x, y):
    for dy in (-4, 0, 4):
        for dx in (-2, 2):
            s.circle(x + dx, y + dy, 0.9, fill=LINE, stroke="none")


def warn_badge(s: Svg, x, y, n, colour=WARN):
    s.path(f"M{x} {y - 6} l6 11 h-12 z", fill=colour, stroke="none")
    s.text(x + 10, y + 4, str(n), size=10, fill=colour)


def error_dot(s: Svg, x, y):
    s.circle(x, y, 4, fill=ERROR, stroke="none")


def spinner(s: Svg, x, y):
    s.circle(x, y, 5, stroke=SELECT, sw=1.5, dash="4 3")


def kind_icon(s: Svg, x, y, kind):
    """A tiny glyph per operation kind."""
    if kind == "Profile":
        s.rect(x - 5, y - 5, 10, 10, fill="none", stroke=INK)
    elif kind == "Pocket":
        s.rect(x - 5, y - 5, 10, 10, fill=LINE, stroke=INK)
    elif kind == "Drill":
        s.circle(x, y, 4.5, stroke=INK)
        s.circle(x, y, 1.2, fill=INK, stroke="none")
    elif kind == "Bore":
        s.circle(x, y, 5, stroke=INK)
        s.circle(x, y, 2.5, stroke=INK)
    elif kind == "Engrave":
        s.path(f"M{x - 5} {y + 3} q5 -12 10 0", stroke=INK)
    elif kind == "V-Carve":
        s.path(f"M{x - 5} {y - 4} l5 8 l5 -8", stroke=INK)
    elif kind == "Inlay":
        s.path(f"M{x - 5} {y - 4} l5 8 l5 -8 z", fill=LINE, stroke=INK)
    elif kind == "Photo V-Carve":
        s.rect(x - 5, y - 5, 10, 10, fill="none", stroke=INK)
        for i in range(3):
            s.line(x - 4, y - 3 + i * 3, x + 4, y - 3 + i * 3, stroke=INK)


def menubar(s: Svg, y=22):
    s.rect(0, y, s.w, 24, fill=HEADER, stroke=LINE)
    x = 10
    for m in ("File", "Edit", "View", "Project", "Help"):
        s.text(x, y + 16, m, size=12)
        x += len(m) * 7 + 18
    return y + 24


def toolbar(s: Svg, y, title="plaque.dcv •"):
    s.rect(0, y, s.w, 32, fill=PANEL, stroke=LINE)
    x = 8
    for lab, w in (("Open", 44), ("Save", 44), ("|", 6), ("Undo", 44), ("Redo", 44), ("|", 6), ("Import", 54), ("Picture", 54), ("|", 6), ("Fit", 36), ("Tab tool", 60), ("|", 6), ("Export", 56)):
        if lab == "|":
            s.line(x + 3, y + 6, x + 3, y + 26, stroke=LINE)
        else:
            button(s, x, y + 4, w, lab, h=24)
        x += w + 6
    s.text(s.w - 10, y + 20, title, size=12, fill=MUTED, anchor="end")
    return y + 32


def statusbar(s: Svg, y, items):
    s.rect(0, y, s.w, 24, fill=HEADER, stroke=LINE)
    x = 10
    for it in items:
        s.text(x, y + 16, it, size=11)
        x += len(it) * 6.2 + 24
        s.line(x - 12, y + 5, x - 12, y + 19, stroke=LINE)


# ── Viewport drawing (the plaque from the phase 2 quick start) ──────────


def viewport(s: Svg, x, y, w, h, scale=3.2, show_tabs=True, show_arrows=False, depth_mode=False, selected=True, extents=False, stale=None, tab_hollow=False):
    """Stock 160 x 100 mm plaque, Y up, drawn at `scale` px per mm."""
    s.rect(x, y, w, h, fill=WHITE, stroke=LINE)
    # Grid.
    sx, sy = x + 60, y + h - 60  # stock bottom-left on screen
    sw, sh = 160 * scale, 100 * scale

    def P(mx, my):  # project mm -> screen, the one Y flip
        return sx + mx * scale, sy - my * scale

    for gx in range(0, 161, 10):
        X, _ = P(gx, 0)
        s.line(X, sy - sh, X, sy, stroke="#eeeeee")
    for gy in range(0, 101, 10):
        _, Y = P(0, gy)
        s.line(sx, Y, sx + sw, Y, stroke="#eeeeee")
    # Stock, dashed grey as --preview.
    s.rect(sx, sy - sh, sw, sh, fill="none", stroke=STOCK, sw=1.2, dash="6 4")
    # Origin crosshair.
    s.line(sx - 10, sy, sx + 10, sy, stroke=INK, sw=1.2)
    s.line(sx, sy - 10, sx, sy + 10, stroke=INK, sw=1.2)
    s.text(sx - 14, sy + 14, "0,0", size=9, fill=MUTED)
    # Park point.
    px, py = P(150, 95)
    s.path(f"M{px - 5} {py} h10 M{px} {py - 5} v10", stroke=MUTED)
    s.circle(px, py, 5, stroke=MUTED)
    # Shapes (black, thin): outline, panel, island circle, 4 holes, a line.
    ox, oy = P(10, 10)
    s.rect(ox, oy - 80 * scale, 140 * scale, 80 * scale, fill="none", stroke=INK, rx=6 * scale, sw=1)
    panel_x, panel_y = P(30, 30)
    s.rect(panel_x, panel_y - 50 * scale, 70 * scale, 50 * scale, fill="none", stroke=SELECT if selected else INK, sw=1.4 if selected else 1)
    if selected:  # handles
        for hx, hy in ((30, 30), (100, 30), (30, 80), (100, 80)):
            X, Y = P(hx, hy)
            s.rect(X - 3, Y - 3, 6, 6, fill=WHITE, stroke=SELECT)
        X, Y = P(65, 80)
        s.circle(X, Y - 14, 4, fill=WHITE, stroke=SELECT)
        s.line(X, Y, X, Y - 10, stroke=SELECT)
    cx, cy = P(65, 55)
    s.circle(cx, cy, 10 * scale, stroke=INK)
    for hx, hy in ((20, 20), (140, 20), (20, 80), (140, 80)):
        X, Y = P(hx, hy)
        s.circle(X, Y, 2.5 * scale, stroke=INK)
    lx1, ly1 = P(110, 40)
    lx2, ly2 = P(140, 70)
    s.path(f"M{lx1} {ly1} q{15 * scale} {-40 * scale} {lx2 - lx1} {ly2 - ly1}", stroke=INK, dash="3 2")

    col = (lambda i, z: PALETTE[i]) if not depth_mode else (lambda i, z: depth_colour(z))
    # Toolpaths. Profile (op 0): outside the outline by 3.175 mm.
    r = 3.175 * scale
    X0, Y0 = P(10, 10)
    pw, ph = 140 * scale, 80 * scale
    s.rect(X0 - r, Y0 - ph - r, pw + 2 * r, ph + 2 * r, fill="none", stroke=col(0, 12), rx=6 * scale + r, sw=1.6)
    if show_tabs:
        for tx, ty in ((80, 10), (80, 90), (10, 50), (150, 50)):
            X, Y = P(tx, ty)
            if tx in (10, 150):
                X += -r if tx == 10 else r
                s.line(X, Y - 4 * scale, X, Y + 4 * scale, stroke=WHITE if tab_hollow else col(0, 12), sw=5)
                if tab_hollow:
                    s.rect(X - 2.5, Y - 4 * scale, 5, 8 * scale, fill="none", stroke=col(0, 12))
            else:
                Y += r if ty == 10 else -r
                s.line(X - 4 * scale, Y, X + 4 * scale, Y, stroke=col(0, 12), sw=5)
    # Start point on the profile.
    X, Y = P(10, 50)
    s.circle(X - r, Y, r, stroke=col(0, 12))
    s.line(X - r, Y, X - r, Y - 2 * r, stroke=col(0, 12))
    # Pocket (op 1): raster lines inside the panel, around the island.
    pc = col(1, 6)
    op = 0.35 if stale == 1 else 1.0
    s.add(f'<g opacity="{op}">')
    for i in range(1, 12):
        my = 30 + i * 50 / 12
        X1, Y = P(30 + 3.2, my)
        X2, _ = P(100 - 3.2, my)
        # skip across the island
        if abs(my - 55) < 13.2:
            dx = math.sqrt(max(13.2 ** 2 - (my - 55) ** 2, 0)) * scale
            s.line(X1, Y, cx - dx, Y, stroke=pc, sw=1.2)
            s.line(cx + dx, Y, X2, Y, stroke=pc, sw=1.2)
        else:
            s.line(X1, Y, X2, Y, stroke=pc, sw=1.2)
    ri = 3.2 * scale
    s.rect(panel_x + ri, panel_y - 50 * scale + ri, 70 * scale - 2 * ri, 50 * scale - 2 * ri, fill="none", stroke=pc, sw=1.4)
    s.circle(cx, cy, 10 * scale + ri, stroke=pc, sw=1.4)
    s.add("</g>")
    # Drill (op 2): bit circles.
    for hx, hy in ((20, 20), (140, 20), (20, 80), (140, 80)):
        X, Y = P(hx, hy)
        s.circle(X, Y, 2.5 * scale, stroke=col(2, 12), sw=1.4)
    # Engrave (op 3).
    s.path(f"M{lx1} {ly1} q{15 * scale} {-40 * scale} {lx2 - lx1} {ly2 - ly1}", stroke=col(3, 1), sw=1.6)
    # Rapids: dashed grey between a few points.
    A = P(20, 80)
    B = P(140, 80)
    s.line(A[0], A[1], B[0], B[1], stroke=RAPID, dash="4 3")
    C = P(140, 20)
    s.line(B[0], B[1], C[0], C[1], stroke=RAPID, dash="4 3")
    if show_arrows:
        for t in (0.25, 0.5, 0.75):
            ax = X0 - r + (pw + 2 * r) * t
            ay = Y0 - ph - r
            s.path(f"M{ax - 4} {ay - 3} l5 3 l-5 3", stroke=col(0, 12))
    if extents:
        ex1, ey1 = P(10 - 3.175, 10 - 3.175)
        ex2, ey2 = P(150 + 3.175, 90 + 3.175)
        s.rect(ex1, ey2, ex2 - ex1, ey1 - ey2, fill="none", stroke=INK, dash="2 2")
        ex1, ey1 = P(10 - 6.35, 10 - 6.35)
        ex2, ey2 = P(150 + 6.35, 90 + 6.35)
        s.rect(ex1, ey2, ex2 - ex1, ey1 - ey2, fill="none", stroke=MUTED, dash="6 3")
        s.text(ex1, ey2 - 6, "Program extents  X −6.35..156.35  Y −6.35..96.35  Z −12.2..5  (edge of the bit, dashed)", size=10, fill=MUTED)
    return P


def depth_colour(z_mm: float) -> str:
    """Pale at the top, dark at 12 mm."""
    t = min(max(z_mm / 12.0, 0.0), 1.0)
    r = int(200 - 150 * t)
    g = int(220 - 190 * t)
    b = int(255 - 120 * t)
    return f"#{r:02x}{g:02x}{b:02x}"


def view_controls(s: Svg, x, y, depth_mode=False):
    """The visibility panel in the Viewport's corner."""
    panel(s, x, y, 190, 236, "Toolpaths")
    yy = y + 32
    yy = radio(s, x + 10, yy, "By operation", on=not depth_mode)
    yy = radio(s, x + 10, yy, "By depth", on=depth_mode)
    yy += 4
    for lab, on in (("Cuts", True), ("Rapids", True), ("Plunges", True), ("Direction arrows", False), ("Start points", True), ("Tab marks", True)):
        yy = checkbox(s, x + 10, yy, lab, on)
    yy = checkbox(s, x + 10, yy, "Only the selected operation", False)
    button(s, x + 10, yy + 2, 80, "Show all", h=20)
    button(s, x + 98, yy + 2, 80, "Hide all", h=20)


# ── Panels ──────────────────────────────────────────────────────────────


def design_tree(s: Svg, x, y, w, h):
    panel(s, x, y, w, h, "Design Tree")
    rows = [
        ("Shapes", None, None, None, True),
        ("outline", True, False, "2", False),
        ("panel", True, False, "1", False),
        ("island", True, False, "1", False),
        ("hole-1", True, False, "1", False),
        ("hole-2", True, False, "1", False),
        ("hole-3", True, True, "1", False),
        ("hole-4", True, False, "1", False),
        ("flourish  open", True, False, "1", False),
        ("Tabs", None, None, None, True),
        ("outline · 4 tabs", True, False, "", False),
        ("Pictures", None, None, None, True),
        ("(none)", None, None, None, False),
    ]
    yy = y + 30
    for name, vis, lk, ops, header in rows:
        if header:
            s.rect(x + 1, yy - 2, w - 2, 18, fill=HEADER, stroke="none")
            s.text(x + 8, yy + 11, name, size=11, weight="bold")
            eye(s, x + w - 36, yy + 7, True)
            lock(s, x + w - 16, yy + 7, False)
        elif vis is None:
            s.text(x + 24, yy + 11, name, size=11, fill=MUTED, italic=True)
        else:
            if name == "panel":
                s.rect(x + 1, yy - 2, w - 2, 18, fill="#e3ecfb", stroke="none")
            eye(s, x + 14, yy + 7, vis)
            lock(s, x + 30, yy + 7, lk)
            label = name
            if "open" in name:
                label = name.split("  ")[0]
            s.text(x + 44, yy + 11, label, size=11)
            if "open" in name:
                s.rect(x + 100, yy, 30, 13, fill=WHITE, stroke=LINE, rx=6)
                s.text(x + 115, yy + 10, "open", size=9, fill=MUTED, anchor="middle")
            if ops:
                s.text(x + w - 12, yy + 11, ops, size=10, fill=MUTED, anchor="end")
        yy += 20


def properties_placement(s: Svg, x, y, w, h):
    panel(s, x, y, w, h, "Properties · panel")
    yy = y + 32
    yy = field_row(s, x + 8, yy, w - 16, "X", "30", "mm", label_w=70)
    yy = field_row(s, x + 8, yy, w - 16, "Y", "30", "mm", label_w=70)
    yy = field_row(s, x + 8, yy, w - 16, "W", "70", "mm", label_w=70)
    lock(s, x + 8 + 70 - 26, yy - 4, True)
    yy = field_row(s, x + 8, yy, w - 16, "H", "50", "mm", label_w=70)
    yy = field_row(s, x + 8, yy, w - 16, "Rotation", "0", "°", label_w=70)
    s.text(x + 8, yy + 14, "Mirror", size=11)
    button(s, x + 78, yy, 52, "↔", h=20)
    button(s, x + 136, yy, 52, "↕", h=20)
    yy += 30
    yy = checkbox(s, x + 8, yy, "Visible", True)
    yy = checkbox(s, x + 8, yy, "Locked", False)
    s.text(x + 8, yy + 12, "Targeted by: Pocket 1", size=10, fill=MUTED)


def operations_list(s: Svg, x, y, w, h, selected=1, stale=None):
    s.rect(x, y, w, h, fill=WHITE, stroke=LINE)
    button(s, x + 6, y + 6, 56, "Add ▾", h=20)
    button(s, x + 66, y + 6, 72, "Duplicate", h=20)
    button(s, x + 142, y + 6, 56, "Delete", h=20)
    eye(s, x + w - 42, y + 16, True)
    eye(s, x + w - 20, y + 16, False)
    yy = y + 34
    rows = [
        ("Profile", "Outside cut", "6.35 mm end mill", "ok"),
        ("Pocket", "Panel pocket", "6.35 mm end mill", "warn"),
        ("Drill", "Screw holes", "3.175 mm end mill", "ok"),
        ("Engrave", "Flourish", "3.175 mm end mill", "ok"),
        ("V-Carve", "Lettering", "90° V-bit", "err"),
    ]
    for i, (kind, name, tool, st) in enumerate(rows):
        if i == selected:
            s.rect(x + 1, yy, w - 2, 30, fill="#e3ecfb", stroke="none")
        grip(s, x + 10, yy + 15)
        checkbox(s, x + 20, yy + 6, "", checked=(i != 3))
        kind_icon(s, x + 48, yy + 15, kind)
        s.text(x + 62, yy + 13, name, size=11, weight="bold")
        s.text(x + 62, yy + 25, f"{kind} · {tool}", size=9, fill=MUTED)
        eye(s, x + w - 42, yy + 15, i != 3)
        if stale == i:
            spinner(s, x + w - 18, yy + 15)
        elif st == "warn":
            warn_badge(s, x + w - 22, yy + 13, 1)
        elif st == "err":
            error_dot(s, x + w - 18, yy + 15)
        yy += 32
    s.text(x + w - 8, yy + 14, f"{len(rows)} operations · est. 14 min", size=10, fill=MUTED, anchor="end")
    return yy + 24


def op_editor_profile(s: Svg, x, y, w, with_errors=False):
    """The Profile editor, every group open."""
    yy = y
    yy = section(s, x, yy, w, "Identity")
    yy = field_row(s, x + 8, yy, w - 16, "Name", "Outside cut", help_=False)
    yy = checkbox(s, x + 8, yy, "Enabled", True) + 4
    yy = section(s, x, yy, w, "Targets")
    s.rect(x + 8, yy, w - 16, 42, fill=WHITE, stroke=LINE)
    s.text(x + 14, yy + 14, "outline", size=11)
    s.text(x + w - 14, yy + 14, "✕", size=10, fill=MUTED, anchor="end")
    s.text(x + 14, yy + 30, "1 target · hover to highlight", size=9, fill=MUTED, italic=True)
    yy += 48
    button(s, x + 8, yy, 86, "Add selected", h=20)
    button(s, x + 100, yy, 110, "Replace with selected", h=20)
    button(s, x + 216, yy, 50, "Pick…", h=20)
    yy += 30
    yy = section(s, x, yy, w, "Tool")
    yy = dropdown(s, x + 8, yy, w - 16, "Tool", "6.35 mm end mill (T1)")
    button(s, x + 8 + 138, yy - 4, 120, "Add from library…", h=20)
    yy += 24
    yy = section(s, x, yy, w, "Depth")
    yy = field_row(s, x + 8, yy, w - 16, "Depth", "12.2", "mm", error="depth_mm: must be at most the thickness plus the through-cut allowance (12.2 mm)" if with_errors else None)
    yy = field_row(s, x + 8, yy, w - 16, "Step-down", "tool's: 4 mm", "mm", placeholder=True, reset=True)
    yy = field_row(s, x + 8, yy, w - 16, "Through-cut allowance", "0.2", "mm")
    yy = section(s, x, yy + 2, w, "Strategy")
    yy = dropdown(s, x + 8, yy, w - 16, "Side", "Outside")
    yy = dropdown(s, x + 8, yy, w - 16, "Direction", "Climb")
    yy = dropdown(s, x + 8, yy, w - 16, "Start point", "Auto")
    yy = dropdown(s, x + 8, yy, w - 16, "Last pass", "Off")
    yy = section(s, x, yy + 2, w, "Entry and leads")
    yy = dropdown(s, x + 8, yy, w - 16, "Entry", "Ramp")
    yy = field_row(s, x + 8, yy, w - 16, "Ramp angle", "15", "°")
    yy = dropdown(s, x + 8, yy, w - 16, "Lead", "Arc")
    yy = field_row(s, x + 8, yy, w - 16, "Lead radius", "3", "mm")
    yy = field_row(s, x + 8, yy, w - 16, "Lead sweep", "90", "°")
    yy = checkbox(s, x + 8 + 138, yy, "Lead out", True)
    yy = field_row(s, x + 8, yy, w - 16, "Overcut", "0", "mm")
    yy = section(s, x, yy + 2, w, "Corners and tabs")
    yy = dropdown(s, x + 8, yy, w - 16, "Corner relief", "None")
    yy = dropdown(s, x + 8, yy, w - 16, "Tabs", "Count")
    yy = field_row(s, x + 8, yy, w - 16, "Count", "4", "")
    yy = field_row(s, x + 8, yy, w - 16, "Width", "8", "mm")
    yy = field_row(s, x + 8, yy, w - 16, "Height", "3", "mm")
    yy = dropdown(s, x + 8, yy, w - 16, "Shape", "Rect")
    yy = checkbox(s, x + 8 + 138, yy, "On holes", False)
    yy = checkbox(s, x + 8 + 138, yy, "Avoid corners", True)
    button(s, x + 8 + 138, yy, 100, "Place tabs", primary=True, h=20)
    s.text(x + 8 + 138 + 108, yy + 14, "4 placed on outline", size=10, fill=MUTED)
    yy += 28
    yy = section(s, x, yy, w, "Order")
    yy = dropdown(s, x + 8, yy, w - 16, "Machining order", "Full depth first")
    yy = section(s, x, yy + 2, w, "Heights")
    yy = field_row(s, x + 8, yy, w - 16, "Safe Z", "5", "mm")
    return yy


def cam_panel(s: Svg, x, y, w, h, active=0, stale=None):
    panel(s, x, y, w, h, None)
    tabs(s, x, y, w, ["Operations", "Tools"], active)
    if active == 0:
        ly = operations_list(s, x, y + 24, w, 212, selected=1, stale=stale)
        s.rect(x, ly, w, 22, fill=HEADER, stroke=LINE)
        s.text(x + 8, ly + 15, "Panel pocket · Pocket", size=12, weight="bold")
        warn_badge(s, x + w - 60, ly + 12, 1)
        s.text(x + w - 8, ly + 15, "⤢", size=12, fill=MUTED, anchor="end")
        yy = ly + 28
        yy = section(s, x, yy, w, "Identity", open_=False)
        yy = section(s, x, yy, w, "Targets", open_=False)
        yy = section(s, x, yy, w, "Tool")
        yy = dropdown(s, x + 8, yy, w - 16, "Tool", "6.35 mm end mill (T1)")
        yy = section(s, x, yy, w, "Depth")
        yy = field_row(s, x + 8, yy, w - 16, "Depth", "6", "mm")
        yy = field_row(s, x + 8, yy, w - 16, "Step-down", "tool's: 3 mm", "mm", placeholder=True, reset=True)
        yy = field_row(s, x + 8, yy, w - 16, "Stock to leave", "0", "mm")
        yy = field_row(s, x + 8, yy, w - 16, "Through-cut allowance", "0", "mm")
        yy = section(s, x, yy, w, "Strategy")
        yy = dropdown(s, x + 8, yy, w - 16, "Clearance", "Raster")
        yy = field_row(s, x + 8, yy, w - 16, "Raster angle", "0", "°")
        yy = dropdown(s, x + 8, yy, w - 16, "Wall pass", "Last")
        yy = field_row(s, x + 8, yy, w - 16, "Stepover", "tool's: 40 %", "%", placeholder=True, reset=True)
        yy = dropdown(s, x + 8, yy, w - 16, "Direction", "Climb")
        yy = dropdown(s, x + 8, yy, w - 16, "Rest of", "None")
        yy = section(s, x, yy, w, "Entry")
        yy = dropdown(s, x + 8, yy, w - 16, "Entry", "Helix")
        yy = field_row(s, x + 8, yy, w - 16, "Helix diameter", "80", "% of bit")
        yy = field_row(s, x + 8, yy, w - 16, "Helix angle", "15", "°")
        yy = section(s, x, yy, w, "Corners")
        yy = dropdown(s, x + 8, yy, w - 16, "Corner relief", "Dogbone")
        s.path(f"M{x + 12} {yy + 4} l5 9 h-10 z", fill=WARN, stroke="none")
        s.text(x + 24, yy + 13, "relief_corner_too_tight: 2 corners of panel skipped", size=10, fill=WARN)
        yy += 20
        yy = section(s, x, yy, w, "Order", open_=False)
        yy = section(s, x, yy, w, "Heights", open_=False)
    else:
        tools_list(s, x, y + 24, w, h - 24)


def tools_list(s: Svg, x, y, w, h):
    s.rect(x, y, w, h, fill=WHITE, stroke=LINE)
    button(s, x + 6, y + 6, 120, "Add from library…", h=20)
    button(s, x + 130, y + 6, 72, "Duplicate", h=20)
    button(s, x + 206, y + 6, 56, "Remove", h=20)
    yy = y + 34
    rows = [
        ("6.35 mm end mill", "End mill · 2 flutes · T1", "ok"),
        ("3.175 mm end mill", "End mill · 2 flutes · T2", "ok"),
        ("90° V-bit 6.35 mm", "V-bit · 90° · T3", "err"),
    ]
    for i, (name, meta, st) in enumerate(rows):
        if i == 2:
            s.rect(x + 1, yy, w - 2, 30, fill="#e3ecfb", stroke="none")
        s.text(x + 14, yy + 13, name, size=11, weight="bold")
        s.text(x + 14, yy + 25, meta, size=9, fill=MUTED)
        if st == "err":
            error_dot(s, x + w - 18, yy + 15)
        yy += 32
    s.rect(x, yy + 6, w, 22, fill=HEADER, stroke=LINE)
    s.text(x + 8, yy + 21, "90° V-bit 6.35 mm", size=12, weight="bold")
    yy += 34
    yy = field_row(s, x + 8, yy, w - 16, "Name", "90° V-bit 6.35 mm", help_=False)
    yy = dropdown(s, x + 8, yy, w - 16, "Type", "V-bit", help_=False)
    yy = field_row(s, x + 8, yy, w - 16, "Included angle", "90", "°")
    yy = field_row(s, x + 8, yy, w - 16, "Diameter", "6.35", "mm")
    yy = field_row(s, x + 8, yy, w - 16, "Flutes", "2", "")
    yy = field_row(s, x + 8, yy, w - 16, "Feed", "1200", "mm/min")
    yy = field_row(s, x + 8, yy, w - 16, "Plunge rate", "", "mm/min", error="plunge_mm_min: unset. Export refuses until it is set. The app never guesses a feed.")
    yy = field_row(s, x + 8, yy, w - 16, "RPM", "18000", "rpm")
    yy = field_row(s, x + 8, yy, w - 16, "Step-down", "", "mm", placeholder=True)
    yy = field_row(s, x + 8, yy, w - 16, "Stepover", "", "%", placeholder=True)
    yy = field_row(s, x + 8, yy, w - 16, "Tool number", "3", "")
    yy = field_row(s, x + 8, yy, w - 16, "Vendor", "IDC Woodcraft", help_=False)
    yy = field_row(s, x + 8, yy, w - 16, "Product id", "ED-90", help_=False)
    button(s, x + 8, yy + 4, 110, "Save to library", h=20)
    s.text(x + 126, yy + 18, "used by: Lettering", size=10, fill=MUTED)


# ── Scenes ──────────────────────────────────────────────────────────────

W, H = 1280, 800


def scene_workspace():
    s = Svg(W, H)
    caption(s, "01 · The workspace (capability 50). Plaque project; the panel is selected; the Pocket is being edited. Y up, origin bottom-left.")
    y = menubar(s)
    y = toolbar(s, y)
    bottom = H - 24
    left_w, right_w = 250, 320
    design_tree(s, 0, y, left_w, 300)
    properties_placement(s, 0, y + 300, left_w, bottom - y - 300)
    vx, vw = left_w, W - left_w - right_w
    P = viewport(s, vx, y, vw, bottom - y)
    view_controls(s, vx + vw - 198, y + 8)
    s.text(vx + 8, bottom - 8, "Pan: middle drag or Space-drag · Zoom: wheel · F fits · click a toolpath to select its operation", size=10, fill=MUTED)
    cam_panel(s, W - right_w, y, right_w, bottom - y, active=0)
    statusbar(s, bottom, ["mm", "Stock 160 × 100 × 12 mm", "X 64.2  Y 51.8", "Idle", "⚠ 1 warning · ● 1 error", "Est. 14 min"])
    return s.render("DreamCarve phase 4 wireframe: workspace")


def scene_import_dialog():
    s = Svg(720, 540)
    caption(s, "02 · Import a drawing (capability 55). DXF shown; an SVG has no options.")
    panel(s, 20, 40, 680, 480, "Import drawing")
    s.text(680, 57, "✕", size=12, fill=MUTED, anchor="end")
    yy = 76
    s.text(36, yy + 14, "File", size=11, weight="bold")
    s.rect(36, yy + 22, 540, 22, fill=WHITE, stroke=LINE, rx=2)
    s.text(42, yy + 37, "/Users/sam/cnc/plaque.dxf", size=11, mono=True)
    button(s, 584, yy + 21, 100, "Choose…", h=24)
    yy += 56
    s.text(36, yy + 14, "DXF · 14 closed shapes, 1 open path, 3 layers · units from the file: mm", size=11, fill=MUTED)
    yy += 30
    s.text(36, yy + 14, "Options", size=11, weight="bold")
    yy += 24
    yy = dropdown(s, 36, yy, 320, "Units", "Millimetres (from the file)", help_=False)
    yy = field_row(s, 36, yy, 320, "Join tolerance", "0.01", "mm", help_=False)
    s.text(36, yy + 4, "Units are required when the file declares none ($INSUNITS).", size=10, fill=MUTED, italic=True)
    yy += 14
    yy += 4
    s.text(36, yy + 14, "Layers", size=11, weight="bold")
    yy += 24
    s.rect(36, yy, 320, 90, fill=WHITE, stroke=LINE)
    ly = yy + 6
    for lab, on in (("CUT · 7 shapes", True), ("POCKET · 3 shapes", True), ("DRILL · 4 circles", True), ("NOTES · 2 TEXT (not read)", False)):
        ly = checkbox(s, 44, ly, lab, on, disabled=("NOTES" in lab))
    # Preview thumbnail.
    s.rect(380, 196, 300, 190, fill=WHITE, stroke=LINE)
    s.text(386, 210, "Preview", size=10, fill=MUTED)
    s.rect(400, 230, 260, 130, fill="none", stroke=INK, rx=8)
    s.rect(440, 250, 120, 80, fill="none", stroke=INK)
    s.circle(500, 290, 18, stroke=INK)
    for cx, cy in ((415, 245), (645, 245), (415, 345), (645, 345)):
        s.circle(cx, cy, 5, stroke=INK)
    s.path("M590 320 q20 -60 50 -30", stroke=INK, dash="3 2")
    yy = 396
    s.text(36, yy + 14, "Shapes are placed as drawn, the drawing's origin at the stock's bottom-left, and selected after import.", size=10, fill=MUTED, italic=True)
    s.text(36, yy + 30, "Warnings: 2 TEXT entities are not read and will be listed in the Warnings panel.", size=10, fill=WARN)
    button(s, 500, 480, 80, "Cancel", h=26)
    button(s, 590, 480, 94, "Import", primary=True, h=26)
    return s.render("DreamCarve phase 4 wireframe: import dialog")


def scene_stock_origin():
    s = Svg(720, 780)
    caption(s, "03 · Project panel: stock, origin, heights, post-processor (capabilities 60–62). Also the Properties panel with nothing selected.")
    panel(s, 20, 40, 330, 720, "Project · plaque.dcv")
    x, w = 28, 314
    yy = 72
    yy = section(s, 20, yy, 330, "Stock")
    yy = field_row(s, x, yy, w, "Width", "160", "mm")
    yy = field_row(s, x, yy, w, "Height", "100", "mm")
    yy = field_row(s, x, yy, w, "Thickness", "", "mm", error="stock.thickness_mm: unset; export refuses")
    button(s, x + 138, yy, 110, "Fit to shapes…", h=20)
    yy += 30
    yy = section(s, 20, yy, 330, "Origin")
    s.text(x, yy + 14, "XY zero", size=11)
    # 3x3 preset picker
    gx, gy = x + 138, yy
    s.rect(gx, gy, 60, 40, fill=WHITE, stroke=LINE)
    for px, py, on in ((gx, gy + 40, True), (gx + 60, gy + 40, False), (gx, gy, False), (gx + 60, gy, False), (gx + 30, gy + 20, False)):
        s.circle(px, py, 4, fill=SELECT if on else WHITE, stroke=SELECT if on else LINE)
    s.text(gx + 72, yy + 14, "Bottom-left", size=11)
    yy += 48
    yy = radio(s, x + 138, yy, "Custom  X [      ]  Y [      ] mm", on=False)
    yy = dropdown(s, x, yy, w, "Z zero", "Stock top")
    yy = section(s, 20, yy + 2, 330, "Heights")
    yy = field_row(s, x, yy, w, "Clearance", "10", "mm")
    yy = field_row(s, x, yy, w, "Tool-change machine Z", "-5", "mm")
    s.text(x + 138, yy + 4, "G53 lift at each tool change and the end; needs a homed machine.", size=8, fill=MUTED, italic=True)
    yy += 12
    yy = field_row(s, x, yy, w, "Park point", "150, 95", "mm")
    button(s, x + 138, yy - 4, 110, "Click on the view", h=18)
    yy += 20
    yy = section(s, 20, yy, 330, "Tolerances", open_=False)
    yy = section(s, 20, yy, 330, "Post-processor")
    yy = dropdown(s, x, yy, w, "Controller", "grblHAL  v3 (latest)", note="Switching keeps the settings the new one can use.")
    yy = field_row(s, x, yy, w, "File extension", ".nc", "")
    yy = dropdown(s, x, yy, w, "Spindle control", "Automatic")
    yy = field_row(s, x, yy, w, "Spin-up dwell", "3", "s")
    yy = dropdown(s, x, yy, w, "Tool change", "One program per tool")
    s.text(x + 138, yy + 4, "With M6 is offered because grblHAL documents M6.", size=8, fill=MUTED, italic=True)
    yy += 12
    yy = field_row(s, x, yy, w, "Decimals mm / inch", "3 / 4", "")
    yy = checkbox(s, x + 138, yy, "Canned drill cycles", False, disabled=True)
    s.text(x + 138, yy + 4, "Not offered: grblHAL has no canned cycles.", size=8, fill=MUTED, italic=True)
    # Right: the viewport fragment showing origin and park point.
    s.rect(370, 40, 330, 300, fill=WHITE, stroke=LINE)
    s.text(378, 56, "Viewport", size=10, fill=MUTED)
    sx, sy = 400, 310
    s.rect(sx, sy - 180, 288, 180, fill="none", stroke=STOCK, dash="6 4")
    s.line(sx - 8, sy, sx + 8, sy, stroke=INK, sw=1.5)
    s.line(sx, sy - 8, sx, sy + 8, stroke=INK, sw=1.5)
    s.text(sx + 10, sy + 14, "origin: bottom-left, Z at stock top", size=9, fill=MUTED)
    s.circle(sx + 270, sy - 170, 5, stroke=MUTED)
    s.path(f"M{sx + 265} {sy - 170} h10 M{sx + 270} {sy - 175} v10", stroke=MUTED)
    s.text(sx + 160, sy - 160, "park point (150, 95)", size=9, fill=MUTED)
    s.rect(370, 360, 330, 120, fill=PANEL, stroke=LINE)
    s.text(378, 378, "On a GRBL post the tool-change height is offered too;", size=10, fill=MUTED)
    s.text(378, 392, "on Mach3, UCCNC and generic it is greyed with:", size=10, fill=MUTED)
    s.text(378, 410, "“Needs a post-processor that documents G53.”", size=10, fill=MUTED, italic=True)
    s.text(378, 440, "Every field validates as the file does and shows the", size=10, fill=MUTED)
    s.text(378, 454, "file's message under itself.", size=10, fill=MUTED)
    return s.render("DreamCarve phase 4 wireframe: stock and origin")


def scene_tool_library():
    s = Svg(900, 570)
    caption(s, "04 · Tool Library window (capability 64), with the Fusion import report open.")
    panel(s, 20, 40, 860, 512, "Tool Library · ~/Library/Application Support/dreamcarve/tools.json")
    s.text(870, 57, "✕", size=12, fill=MUTED, anchor="end")
    s.rect(32, 74, 300, 22, fill=WHITE, stroke=LINE, rx=11)
    s.text(44, 89, "Search name, vendor, diameter…", size=11, fill=MUTED, italic=True)
    dropdown(s, 344, 75, 160, "Type", "All", label_w=40, help_=False)
    button(s, 560, 74, 80, "Import…", h=22)
    button(s, 648, 74, 80, "Export…", h=22)
    button(s, 736, 74, 70, "Remove", h=22)
    # Table
    cols = [("", 24), ("Name", 230), ("Type", 70), ("Ø", 60), ("Angle", 50), ("Flutes", 50), ("Feed", 70), ("Plunge", 70), ("RPM", 60), ("Vendor", 110)]
    x = 32
    yy = 108
    s.rect(32, yy, 816, 20, fill=HEADER, stroke=LINE)
    for name, cw in cols:
        s.text(x + 4, yy + 14, name, size=10, weight="bold")
        x += cw
    rows = [
        (True, "IDC Woodcraft 1/4\" Downcut (Hardwood)", "End mill", "6.35", "", "2", "1800", "600", "18000", "IDC Woodcraft", "in project"),
        (True, "IDC Woodcraft 1/8\" Upcut", "End mill", "3.175", "", "2", "1200", "400", "18000", "IDC Woodcraft", ""),
        (False, "Amana 45771 60° V-bit", "V-bit", "6.35", "60°", "2", "1500", "500", "18000", "Amana", ""),
        (False, "Amana 45702 90° V-bit", "V-bit", "6.35", "90°", "2", "1500", "500", "18000", "Amana", "in project"),
        (False, "Club 2 mm drill", "Drill", "2", "", "2", "", "300", "12000", "", "no feed"),
        (False, "Club 6 mm ball nose", "Ball nose", "6", "", "2", "1200", "400", "16000", "", ""),
    ]
    yy += 20
    for r in rows:
        if r[0]:
            s.rect(32, yy, 816, 22, fill="#e3ecfb", stroke="none")
        x = 32
        for i, (name, cw) in enumerate(cols):
            if i == 0:
                checkbox(s, x + 6, yy + 3, "", r[0])
            else:
                val = r[i]
                s.text(x + 4, yy + 15, val, size=10, fill=ERROR if (i == 6 and val == "") else INK)
            x += cw
        if r[10]:
            s.rect(x - 2, yy + 4, 64, 14, fill=WHITE, stroke=ERROR if r[10] == "no feed" else LINE, rx=7)
            s.text(x + 30, yy + 14, r[10], size=9, fill=ERROR if r[10] == "no feed" else MUTED, anchor="middle")
        s.line(32, yy + 22, 848, yy + 22, stroke="#e8e8e8")
        yy += 22
    s.text(32, yy + 30, "23 tools · 2 selected · rows marked “in project” are already embedded in plaque.dcv", size=10, fill=MUTED)
    button(s, 700, yy + 16, 148, "Add 2 tools to project", primary=True, h=26)
    # Import report sheet
    panel(s, 400, 318, 470, 210, "Import report · club.tools")
    ry = 350
    for line, col in (
        ("Imported 23 tools from Fusion library club.tools", INK),
        ("  21 end mills, V-bits, ball noses and drills, each preset one tool", MUTED),
        ("Skipped 2:", INK),
        ("  Thread mill 1/4-20 — no DreamCarve cutter for thread mills", WARN),
        ("  Face mill 50 mm — no DreamCarve cutter for face mills", WARN),
        ("Already present 0 (Replace existing: off)", MUTED),
        ("3 imported presets have no feed or plunge rate; export refuses them until set.", ERROR),
    ):
        s.text(412, ry, line, size=10, fill=col, mono=True)
        ry += 18
    checkbox(s, 412, 482, "Replace existing tools with the same source", False)
    button(s, 780, 498, 80, "Close", h=24)
    return s.render("DreamCarve phase 4 wireframe: tool library")


def scene_operation_profile():
    s = Svg(700, 1000)
    caption(s, "05 · Operation editor, Profile, every group open (capabilities 66–69). Field names are the file's; labels are the chapters'.")
    panel(s, 20, 40, 330, 940, "Outside cut · Profile")
    s.text(340, 57, "⤢", size=12, fill=MUTED, anchor="end")
    op_editor_profile(s, 20, 72, 330, with_errors=False)
    # Annotations on the right.
    ax = 380
    notes = [
        ("Groups", "Identity · Targets · Tool · Depth · Strategy · Entry and leads · Corners and tabs · Order · Heights. A group with no field for the kind is not shown. Open state is remembered."),
        ("Tool-derived defaults", "Step-down, stepover and pitch show the tool's value as grey placeholder text and write null. ⟲ clears back to it."),
        ("Help", "? beside a label shows the chapter's one-sentence definition on hover. Labels are the chapters' names; the file's field name is in the tooltip."),
        ("Located warnings", "A warning check locates at a field shows under that field, in the warning colour, as well as in the Warnings panel (10)."),
        ("Targets", "Hovering the group highlights the targets on the Viewport. Pick… lets the user click shapes until Escape."),
        ("Tabs", "The request (count or distances, size, shape, on holes, avoid corners) is the file's tabs field. Place tabs runs tabs place and the request becomes placed tabs on the part (ADR-0005), listed in the Design Tree and draggable on the Viewport (08)."),
        ("Never offered", "Nothing the generator ignores for this kind appears; nothing it reads is hidden. Acceptance test 73 checks the table in capability 68 against the editor."),
    ]
    yy = 80
    for title, body in notes:
        s.text(ax, yy, title, size=11, weight="bold")
        yy += 16
        for ln in wrap(body, 52):
            s.text(ax, yy, ln, size=10, fill=MUTED)
            yy += 14
        yy += 10
    return s.render("DreamCarve phase 4 wireframe: Profile editor")


def scene_operation_vcarve():
    s = Svg(700, 860)
    caption(s, "06 · Operation editor, V-Carve with clearing passes (capabilities 68, 70). The Inlay editor shares the Clearing group.")
    x, w = 20, 330
    panel(s, x, 40, w, 800, "Lettering · V-Carve")
    yy = 72
    yy = section(s, x, yy, w, "Identity", open_=False)
    yy = section(s, x, yy, w, "Targets")
    s.rect(x + 8, yy, w - 16, 44, fill=WHITE, stroke=LINE)
    s.text(x + 14, yy + 14, "rect-frame", size=11)
    s.text(x + 14, yy + 30, "oval  (nested: stands as an island)", size=11)
    yy += 52
    yy = section(s, x, yy, w, "Tool")
    yy = dropdown(s, x + 8, yy, w - 16, "V-bit", "90° V-bit 6.35 mm (T3)")
    s.text(x + 8 + 138, yy + 4, "cone height 3.175 mm", size=9, fill=MUTED, italic=True)
    yy += 14
    yy = section(s, x, yy, w, "Depth")
    yy = dropdown(s, x + 8, yy, w - 16, "Flat depth", "On")
    yy = field_row(s, x + 8, yy, w - 16, "Flat depth", "2.5", "mm")
    yy = field_row(s, x + 8, yy, w - 16, "Start depth", "0", "mm")
    yy = checkbox(s, x + 8 + 138, yy, "Start plane cleared externally", False)
    yy = field_row(s, x + 8, yy, w - 16, "Through-cut allowance", "0", "mm")
    yy = section(s, x, yy + 2, w, "Strategy")
    yy = dropdown(s, x + 8, yy, w - 16, "Strategy", "Medial")
    yy = field_row(s, x + 8, yy, w - 16, "Floor stepover", "40", "%")
    yy = section(s, x, yy + 2, w, "Clearing passes")
    s.text(x + 8, yy + 12, "Cut before the V-bit, largest tool first; each is its own tool run.", size=10, fill=MUTED, italic=True)
    yy += 20
    for i, (tool, strat, dirn, entry) in enumerate((("6.35 mm end mill (T1)", "Offset", "Climb", "Ramp 15°"), ("3.175 mm end mill (T2)", "Raster 45°", "Climb", "Plunge"))):
        s.rect(x + 8, yy, w - 16, 58, fill=WHITE, stroke=LINE)
        grip(s, x + 16, yy + 29)
        s.text(x + 28, yy + 15, f"Pass {i + 1}", size=11, weight="bold")
        s.text(x + w - 14, yy + 15, "✕", size=10, fill=MUTED, anchor="end")
        s.text(x + 28, yy + 31, f"Tool  {tool}", size=10)
        s.text(x + 28, yy + 47, f"{strat} · {dirn} · {entry}", size=10, fill=MUTED)
        yy += 64
    button(s, x + 8, yy, 90, "Add pass", h=20)
    s.text(x + 106, yy + 14, "next smaller end mill is picked", size=10, fill=MUTED, italic=True)
    yy += 30
    yy = section(s, x, yy, w, "Order")
    yy = dropdown(s, x + 8, yy, w - 16, "Machining order", "Full depth first")
    yy = section(s, x, yy + 2, w, "Heights")
    yy = field_row(s, x + 8, yy, w - 16, "Safe Z", "5", "mm")
    s.rect(x + 8, yy + 6, w - 16, 60, fill=PANEL, stroke=LINE)
    s.text(x + 14, yy + 22, "check says:", size=10, weight="bold")
    s.text(x + 14, yy + 38, "deepest cut 2.5 mm · flat floor, cleared by T1 then T2", size=10, fill=MUTED)
    s.text(x + 14, yy + 54, "⚠ clearing_not_descending would show here if T2 were above T1", size=10, fill=WARN)

    # Right: Inlay male, abbreviated.
    x2 = 370
    panel(s, x2, 40, w, 560, "DC · Inlay (male)")
    yy = 72
    yy = section(s, x2, yy, w, "Strategy")
    s.text(x2 + 8, yy + 14, "Side", size=11)
    radio(s, x2 + 126, yy, "Female", on=False)
    radio(s, x2 + 200, yy, "Male", on=True)
    yy += 26
    s.text(x2 + 126, yy + 8, "Mirrored in place about the design's centre line;", size=9, fill=MUTED, italic=True)
    s.text(x2 + 126, yy + 20, "inlay_male_mirrored at X = 55 mm.", size=9, fill=MUTED, italic=True)
    yy += 28
    yy = section(s, x2, yy, w, "Depth")
    yy = field_row(s, x2 + 8, yy, w - 16, "Inlay depth", "4", "mm")
    yy = field_row(s, x2 + 8, yy, w - 16, "Glue gap", "1", "mm")
    yy = field_row(s, x2 + 8, yy, w - 16, "Plug depth", "2", "mm")
    yy = field_row(s, x2 + 8, yy, w - 16, "Margin", "8", "mm")
    yy = dropdown(s, x2 + 8, yy, w - 16, "Outline rounding", "On")
    yy = field_row(s, x2 + 8, yy, w - 16, "Rounding radius", "10", "mm")
    yy = field_row(s, x2 + 8, yy, w - 16, "Through-cut allowance", "0", "mm")
    yy = section(s, x2, yy + 2, w, "Clearing passes", open_=False)
    yy = section(s, x2, yy, w, "Cut-out")
    yy = dropdown(s, x2 + 8, yy, w - 16, "Cut-out", "On")
    yy = dropdown(s, x2 + 8, yy, w - 16, "Tool", "3.175 mm end mill (T2)")
    yy = dropdown(s, x2 + 8, yy, w - 16, "Tabs", "Count 4 · 6 × 3 mm rect")
    yy = field_row(s, x2 + 8, yy, w - 16, "Through-cut allowance", "0.2", "mm")
    yy = section(s, x2, yy + 2, w, "Order", open_=False)
    yy = section(s, x2, yy, w, "Heights", open_=False)
    s.text(x2 + 8, yy + 20, "On Female: Margin, Outline rounding and Cut-out are hidden;", size=10, fill=MUTED)
    s.text(x2 + 8, yy + 34, "the female ignores them (capability 45).", size=10, fill=MUTED)
    return s.render("DreamCarve phase 4 wireframe: V-Carve and Inlay editors")


def scene_operation_fields():
    s = Svg(1100, 640)
    caption(s, "07 · Operation editors side by side, abbreviated: Drill (countersink), Bore, Engrave, Photo V-Carve (capability 68).")
    cols = [
        ("Screw holes · Drill", [
            ("Tool", "dropdown", "90° V-bit 6.35 mm (T3)"),
            ("Depth", "section", None),
            ("Depth", "field", ("derived: 3.175", "mm", True)),
            ("Retract height", "field", ("2", "mm", False)),
            ("Through-cut allowance", "field", ("0", "mm", False)),
            ("Strategy", "section", None),
            ("Cycle", "dropdown", "Countersink"),
            ("Mouth diameter", "field", ("6", "mm", False)),
            ("Start depth", "field", ("0", "mm", False)),
            ("Start plane cleared externally", "check", False),
            ("Dwell", "field", ("0", "s", False)),
            ("Allow tool wider than hole", "check", False),
            ("Heights", "section", None),
            ("Safe Z", "field", ("5", "mm", False)),
            ("note", "note", "Peck shows Peck depth (tool's) and Retract: Full | Chip break gap. Simple shows neither. Countersink's depth is derived, never typed (S18)."),
        ]),
        ("Dowel holes · Bore", [
            ("Tool", "dropdown", "6.35 mm end mill (T1)"),
            ("Depth", "section", None),
            ("Depth", "field", ("12", "mm", False)),
            ("Pitch", "field", ("tool's: 4 mm", "mm/turn", True)),
            ("Stock to leave", "field", ("0", "mm", False)),
            ("Through-cut allowance", "field", ("0.2", "mm", False)),
            ("Strategy", "section", None),
            ("Direction", "dropdown", "Climb"),
            ("Heights", "section", None),
            ("Safe Z", "field", ("5", "mm", False)),
            ("note", "note", "A hole the bit has no room to spiral in, or wider than twice the bit, is skipped with bore_no_room / bore_leaves_core under the Targets group."),
        ]),
        ("Flourish · Engrave", [
            ("Tool", "dropdown", "3.175 mm end mill (T2)"),
            ("Depth", "section", None),
            ("Depth", "field", ("1", "mm", False)),
            ("Step-down", "field", ("tool's: 3 mm", "mm", True)),
            ("Through-cut allowance", "field", ("0", "mm", False)),
            ("Strategy", "section", None),
            ("Alternate direction", "check", True),
            ("Order", "section", None),
            ("Machining order", "dropdown", "Full depth first"),
            ("Heights", "section", None),
            ("Safe Z", "field", ("5", "mm", False)),
            ("note", "note", "Open and closed paths alike; the bit's centre on the line."),
        ]),
        ("Portrait · Photo V-Carve", [
            ("Picture", "dropdown", "portrait.png (b1)"),
            ("Tool", "dropdown", "60° V-bit 6.35 mm (T4)"),
            ("Depth", "section", None),
            ("Max depth", "field", ("2", "mm", False)),
            ("Start depth", "field", ("0", "mm", False)),
            ("Invert (light is deep)", "check", False),
            ("Strategy", "section", None),
            ("Line angle", "field", ("22.5", "°", False)),
            ("Line spacing", "field", ("110", "% of groove", False)),
            ("Strategy", "dropdown", "Raster"),
            ("Sample", "field", ("0.25", "mm", False)),
            ("Heights", "section", None),
            ("Safe Z", "field", ("5", "mm", False)),
            ("Stepover retract", "field", ("1", "mm", False)),
            ("note", "note", "Lands with phase 3.4 (format v25); this editor is one slice gated on it. check's groove count and full-depth width show in the status line."),
        ]),
    ]
    cw = 260
    for i, (title, rows) in enumerate(cols):
        x = 20 + i * (cw + 10)
        panel(s, x, 40, cw, 580, title)
        yy = 72
        for label, kind, val in rows:
            if kind == "section":
                yy = section(s, x, yy, cw, label)
            elif kind == "dropdown":
                yy = dropdown(s, x + 8, yy, cw - 16, label, val, label_w=124, help_=False)
            elif kind == "field":
                v, u, ph = val
                yy = field_row(s, x + 8, yy, cw - 16, label, v, u, placeholder=ph, label_w=124, help_=False, reset=ph)
            elif kind == "check":
                yy = checkbox(s, x + 8, yy, label, val)
            elif kind == "note":
                yy += 6
                for ln in wrap(val, 44):
                    s.text(x + 8, yy + 10, ln, size=9, fill=MUTED, italic=True)
                    yy += 12
    return s.render("DreamCarve phase 4 wireframe: more editors")


def scene_tabs_canvas():
    s = Svg(W, 620)
    caption(s, "08 · Placed tabs on the Viewport (capability 69). Dragging a tab keeps it on its outline; a tab no cut crosses is hollow.")
    P = viewport(s, 0, 30, 900, 566, scale=4.2, show_tabs=True, selected=False, tab_hollow=True)
    # Drag affordance on the top tab.
    X, Y = P(80, 90)
    s.circle(X, Y - 3.175 * 4.2, 10, stroke=SELECT, dash="3 2")
    s.path(f"M{X + 14} {Y - 24} l16 0 l-4 -4 m4 4 l-4 4", stroke=SELECT)
    s.text(X + 36, Y - 20, "drag along the outline", size=10, fill=SELECT)
    X, Y = P(10, 50)
    s.text(X + 12, Y - 30, "hollow: tab_not_crossed", size=10, fill=MUTED)
    s.text(X + 12, Y - 17, "(no enabled Profile cuts here)", size=10, fill=MUTED)
    # Tab tool cursor
    X, Y = P(120, 10)
    s.circle(X, Y, 6, stroke=INK)
    s.text(X - 60, Y + 3.175 * 4.2 + 40, "Tab tool: click an outline to add a tab with the last-used size", size=10, fill=INK)
    # Right: Design Tree tabs section and properties.
    panel(s, 920, 30, 340, 200, "Design Tree · Tabs")
    yy = 60
    for lab, sel in (("outline · tab 1  (80, 90)", True), ("outline · tab 2  (80, 10)", False), ("outline · tab 3  (10, 50)", False), ("outline · tab 4  (150, 50)", False)):
        if sel:
            s.rect(921, yy - 2, 338, 18, fill="#e3ecfb", stroke="none")
        eye(s, 934, yy + 7, True)
        s.text(950, yy + 11, lab, size=11)
        yy += 20
    s.text(930, yy + 14, "Points are in the instance's definition space, so tabs follow it.", size=9, fill=MUTED, italic=True)
    panel(s, 920, 250, 340, 190, "Properties · tab 1")
    yy = 282
    yy = field_row(s, 928, yy, 324, "Width", "8", "mm", label_w=80, help_=False)
    yy = field_row(s, 928, yy, 324, "Height", "3", "mm", label_w=80, help_=False)
    yy = dropdown(s, 928, yy, 324, "Shape", "Rect", label_w=80, help_=False)
    yy = field_row(s, 928, yy, 324, "Position", "80, 90", "mm", label_w=80, help_=False)
    s.text(928, yy + 10, "Crossed by: Outside cut (Profile). Delete removes the tab.", size=10, fill=MUTED)
    panel(s, 920, 460, 340, 136, "Rules (ADR-0005)")
    for i, ln in enumerate((
        "Place tabs on a Profile turns its request into placed tabs.",
        "Every enabled Profile cutting the outline rises over the tab.",
        "A disabled Profile keeps its request until it is placed.",
        "Deleting an instance deletes its tabs; deleting an operation does not.",
        "The saved file holds placed tabs only, as tabs place leaves it.",
    )):
        s.text(930, 492 + i * 18, ln, size=10, fill=MUTED)
    return s.render("DreamCarve phase 4 wireframe: tabs on the Viewport")


def scene_preview_legend():
    s = Svg(W, 620)
    caption(s, "09 · Toolpath visibility and depth colouring (capabilities 72–74): By depth mode, arrows on, program extents on, the Pocket regenerating (faded).")
    P = viewport(s, 0, 30, 1000, 566, scale=4.2, show_arrows=True, depth_mode=True, selected=False, extents=True, stale=1)
    view_controls(s, 1000 - 198, 38, depth_mode=True)
    # Depth legend bar.
    lx, ly = 1020, 60
    s.text(lx, ly - 8, "Depth", size=11, weight="bold")
    for i in range(60):
        z = 12 * i / 59
        s.rect(lx, ly + i * 4, 18, 4, fill=depth_colour(z), stroke="none")
    for z, lab in ((0, "0  stock top"), (1, "1  Flourish"), (6, "6  Panel pocket"), (12, "12  Outside cut, Screw holes")):
        yy = ly + z / 12 * 236
        s.line(lx + 18, yy, lx + 26, yy, stroke=INK)
        s.text(lx + 30, yy + 4, f"−{lab} mm", size=10)
    s.text(lx, ly + 270, "By depth colours every cutting move from pale", size=10, fill=MUTED)
    s.text(lx, ly + 284, "at the stock top to dark at the deepest cut.", size=10, fill=MUTED)
    s.text(lx, ly + 298, "By operation uses the --preview palette.", size=10, fill=MUTED)
    s.text(lx, ly + 330, "Marks (as --preview draws them):", size=11, weight="bold")
    yy = ly + 350
    s.line(lx, yy, lx + 30, yy, stroke=PALETTE[0], sw=1.6)
    s.text(lx + 40, yy + 4, "cutting move, operation colour", size=10)
    yy += 20
    s.line(lx, yy, lx + 30, yy, stroke=RAPID, dash="4 3")
    s.text(lx + 40, yy + 4, "rapid at safe Z or clearance", size=10)
    yy += 20
    s.line(lx, yy, lx + 30, yy, stroke=PALETTE[0], sw=5)
    s.text(lx + 40, yy + 4, "tab mark: a pass rising over a tab", size=10)
    yy += 20
    s.circle(lx + 8, yy, 7, stroke=PALETTE[0])
    s.line(lx + 8, yy, lx + 8, yy - 14, stroke=PALETTE[0])
    s.text(lx + 40, yy + 4, "start point: bit circle, tick = first cut", size=10)
    yy += 22
    s.circle(lx + 8, yy, 6, stroke=PALETTE[2], sw=1.4)
    s.text(lx + 40, yy + 4, "drill hole: the bit at each hole", size=10)
    yy += 22
    s.path(f"M{lx + 4} {yy - 3} l5 3 l-5 3", stroke=PALETTE[0])
    s.text(lx + 40, yy + 4, "direction arrow (toggle)", size=10)
    yy += 22
    s.rect(lx, yy - 6, 30, 12, fill="none", stroke=MUTED, dash="6 3")
    s.text(lx + 40, yy + 4, "program extents, bit edge (toggle)", size=10)
    yy += 30
    s.text(lx, yy, "Faded = regenerating; the row shows a spinner", size=10, fill=MUTED, italic=True)
    s.text(lx, yy + 14, "and the export dialog waits (S20).", size=10, fill=MUTED, italic=True)
    statusbar(s, 596, ["mm", "Stock 160 × 100 × 12 mm", "X 64.2  Y 51.8", "Generating 1 of 5…", "⚠ 1 warning", "Z −12.2..5"])
    return s.render("DreamCarve phase 4 wireframe: preview controls")


def scene_warnings():
    s = Svg(W, 420)
    caption(s, "10 · Warnings panel as a drawer above the status bar (capability 76). It is check, live. Click a row to go to the operation and field.")
    panel(s, 0, 30, W, 366, "Warnings · 1 error · 3 warnings · 1 hint")
    button(s, W - 170, 34, 60, "Copy", h=18)
    button(s, W - 104, 34, 96, "Regenerate all", h=18)
    rows = [
        ("Project", None, None, None),
        ("error", "stock_thickness_unset", "Stock thickness is not set. Export refuses until it is.", "Project ▸ Stock ▸ Thickness"),
        ("hint", "tool_change_avoidable", "T2 is used by Screw holes and Flourish, which are not adjacent; one tool change could be saved.", "Operations list"),
        ("Panel pocket · Pocket", None, None, None),
        ("warning", "relief_corner_too_tight", "2 of panel's corners are too tight for a dogbone with a 6.35 mm bit; skipped.", "Corners ▸ Corner relief"),
        ("Lettering · V-Carve", None, None, None),
        ("error", "tool_missing_plunge", "90° V-bit 6.35 mm has no plunge rate. Export refuses until it is set.", "Tools ▸ 90° V-bit ▸ Plunge rate"),
        ("warning", "v_carve_target_open", "flourish is an open path and is not carved.", "Targets"),
        ("Outside cut · Profile", None, None, None),
        ("warning", "tab_on_hole", "A tab on hole-3 holds its slug; cutting it free is work by hand.", "Design Tree ▸ Tabs"),
    ]
    yy = 64
    for tier, code, msg, where in rows:
        if code is None:
            s.rect(1, yy - 2, W - 2, 18, fill=HEADER, stroke="none")
            s.text(10, yy + 11, tier, size=11, weight="bold")
            yy += 22
            continue
        if code == "tool_missing_plunge":
            s.rect(1, yy - 4, W - 2, 26, fill="#e3ecfb", stroke="none")
        col = {"error": ERROR, "warning": WARN, "hint": MUTED}[tier]
        if tier == "error":
            error_dot(s, 20, yy + 7)
        elif tier == "warning":
            s.path(f"M20 {yy + 1} l6 11 h-12 z", fill=WARN, stroke="none")
        else:
            s.circle(20, yy + 7, 4, stroke=MUTED)
        s.text(36, yy + 11, tier, size=10, fill=col)
        s.text(96, yy + 11, code, size=11, mono=True, fill=col)
        s.text(330, yy + 11, msg, size=11)
        s.text(W - 12, yy + 11, where, size=10, fill=SELECT, anchor="end")
        yy += 26
    s.text(10, yy + 20, "Same codes, same text and same tiers as `dreamcarve check` (acceptance 82). Codes link to the Warnings chapter.", size=10, fill=MUTED, italic=True)
    statusbar(s, 396, ["mm", "Stock 160 × 100 × — mm", "X —  Y —", "Idle", "⚠ 3 warnings · ● 1 error · 1 hint  ▴", "Est. —"])
    return s.render("DreamCarve phase 4 wireframe: warnings panel")


def scene_export():
    s = Svg(820, 700)
    caption(s, "11 · Export dialog (capability 77). What export prints, before it writes; the override is not saved.")
    panel(s, 20, 40, 780, 640, "Export G-code · plaque.dcv")
    s.text(790, 57, "✕", size=12, fill=MUTED, anchor="end")
    x, w = 36, 748
    yy = 76
    s.text(x, yy + 14, "Destination", size=11, weight="bold")
    yy += 22
    s.rect(x, yy, 560, 22, fill=WHITE, stroke=LINE, rx=2)
    s.text(x + 6, yy + 15, "/Users/sam/cnc/plaque", size=11, mono=True)
    s.text(x + 400, yy + 15, "+ -1-T1.nc, -2-T2.nc …", size=10, fill=MUTED)
    button(s, 604, yy - 1, 90, "Choose…", h=24)
    yy += 34
    yy = dropdown(s, x, yy, 400, "Post-processor", "grblHAL  v3", label_w=120, help_=False, note="Changing it here changes the project (undoable) and keeps your settings.")
    s.text(x, yy + 14, "Programs", size=11)
    yy2 = radio(s, x + 120, yy, "As the project says: one program per tool", on=True)
    yy2 = radio(s, x + 120, yy2, "One program per tool, this export only", on=False)
    yy2 = radio(s, x + 120, yy2, "One program with M6 tool changes, this export only", on=False)
    yy = yy2 + 4
    yy = checkbox(s, x + 120, yy, "Also write the toolpath preview (plaque-preview.svg)", True)
    yy += 8
    s.text(x, yy + 14, "Programs to write", size=11, weight="bold")
    yy += 22
    s.rect(x, yy, w, 20, fill=HEADER, stroke=LINE)
    for cx, lab in ((x + 6, "#"), (x + 30, "File"), (x + 200, "Tool"), (x + 360, "Operations"), (x + 560, "Est."), (x + 620, "Z range")):
        s.text(cx, yy + 14, lab, size=10, weight="bold")
    yy += 20
    progs = [
        ("1", "plaque-1-T1.nc", "T1 6.35 mm end mill", "Panel pocket, Outside cut", "9 min 40 s", "−12.2..5"),
        ("2", "plaque-2-T2.nc", "T2 3.175 mm end mill", "Screw holes, Flourish", "2 min 05 s", "−12.2..5"),
        ("3", "plaque-3-T3.nc", "T3 90° V-bit", "Lettering", "1 min 50 s", "−2.5..5"),
    ]
    for p in progs:
        s.rect(x, yy, w, 36, fill=WHITE, stroke=LINE)
        for cx, val in zip((x + 6, x + 30, x + 200, x + 360, x + 560, x + 620), p):
            s.text(cx, yy + 14, val, size=10, mono=(cx == x + 30))
        step = "Zero the tool on the stock top, then run." if p[0] == "1" else f"Change to {p[2]}, re-zero Z, then run."
        s.text(x + 30, yy + 29, "▸ " + step, size=9, fill=MUTED, italic=True)
        yy += 36
    s.text(x, yy + 16, "Extents, tool centre: X 6.8..153.2  Y 6.8..93.2 · bit edge: X 3.7..156.3  Y 3.7..96.3 · total 13 min 35 s (rapids not included: set [estimate] rapid_mm_min)", size=9, fill=MUTED)
    yy += 30
    s.text(x, yy + 14, "Errors (export is disabled until fixed)", size=11, weight="bold", fill=ERROR)
    yy += 22
    error_dot(s, x + 8, yy + 7)
    s.text(x + 20, yy + 11, "tool_missing_plunge  90° V-bit 6.35 mm has no plunge rate.", size=10, mono=True, fill=ERROR)
    s.text(w - 60, yy + 11, "Fix →", size=10, fill=SELECT, anchor="end")
    yy += 28
    s.text(x, yy + 14, "Warnings (listed, do not block)", size=11, weight="bold", fill=WARN)
    yy += 22
    for wl in ("relief_corner_too_tight  2 of panel's corners skipped.", "tab_on_hole  A tab on hole-3 holds its slug.", "tool_change_avoidable  T2 in two non-adjacent operations."):
        s.path(f"M{x + 8} {yy + 1} l6 11 h-12 z", fill=WARN, stroke="none")
        s.text(x + 20, yy + 11, wl, size=10, mono=True, fill=WARN)
        yy += 18
    yy += 10
    s.text(x, yy + 12, "Every file is written or none is. The project file is not changed by exporting.", size=10, fill=MUTED, italic=True)
    button(s, 600, 640, 80, "Cancel", h=28)
    button(s, 690, 640, 94, "Export", primary=True, disabled=True, h=28)
    return s.render("DreamCarve phase 4 wireframe: export dialog")


def scene_empty_state():
    s = Svg(W, H)
    caption(s, "12 · First run and the empty project (capability 79).")
    y = menubar(s)
    y = toolbar(s, y)
    bottom = H - 24
    left_w, right_w = 250, 320
    panel(s, 0, y, left_w, 300, "Design Tree")
    s.text(20, y + 60, "No shapes yet.", size=11, fill=MUTED, italic=True)
    panel(s, 0, y + 300, left_w, bottom - y - 300, "Project · Untitled")
    s.text(20, y + 340, "Stock 100 × 100 mm, thickness unset", size=10, fill=MUTED)
    s.text(20, y + 356, "Origin bottom-left, Z at stock top", size=10, fill=MUTED)
    s.text(20, y + 372, "grblHAL v3", size=10, fill=MUTED)
    vx, vw = left_w, W - left_w - right_w
    s.rect(vx, y, vw, bottom - y, fill=WHITE, stroke=LINE)
    sx, sy = vx + 120, bottom - 100
    s.rect(sx, sy - 400, 400, 400, fill="none", stroke=STOCK, dash="6 4")
    s.line(sx - 8, sy, sx + 8, sy, stroke=INK)
    s.line(sx, sy - 8, sx, sy + 8, stroke=INK)
    # Card
    cx, cy, cw, ch = vx + vw / 2 - 230, y + 120, 460, 330
    s.rect(cx, cy, cw, ch, fill=WHITE, stroke=LINE, rx=8)
    s.text(cx + cw - 16, cy + 22, "✕", size=12, fill=MUTED, anchor="end")
    s.text(cx + cw / 2, cy + 40, "Start with a drawing", size=18, weight="bold", anchor="middle")
    s.text(cx + cw / 2, cy + 62, "DreamCarve machines shapes you draw elsewhere. Drawing tools arrive in phase 5.", size=11, fill=MUTED, anchor="middle")
    for i, (title, meta) in enumerate((("Import a drawing…", "SVG from Inkscape, DXF from CAD  ·  Ctrl I"), ("Import a picture…", "PNG or JPEG for a Photo V-Carve"), ("Open a project…", ".dcv  ·  Ctrl O"))):
        bx, by = cx + 30, cy + 84 + i * 56
        s.rect(bx, by, cw - 60, 46, fill=PANEL, stroke=LINE, rx=4)
        s.text(bx + 14, by + 20, title, size=12, weight="bold")
        s.text(bx + 14, by + 36, meta, size=10, fill=MUTED)
    s.text(cx + 30, cy + 272, "Or open an example", size=11, weight="bold")
    for i, ex in enumerate(("Coaster", "Plaque", "Sign")):
        button(s, cx + 30 + i * 100, cy + 282, 90, ex, h=24)
    s.text(cx + 340, cy + 298, "open as unsaved copies", size=9, fill=MUTED, italic=True)
    panel(s, W - right_w, y, right_w, bottom - y)
    tabs(s, W - right_w, y, right_w, ["Operations", "Tools"], 0)
    s.text(W - right_w + 20, y + 70, "Select shapes on the drawing, then Add an operation.", size=11, fill=MUTED, italic=True)
    statusbar(s, bottom, ["mm", "Stock 100 × 100 × — mm", "X —  Y —", "Idle", "No warnings", ""])
    return s.render("DreamCarve phase 4 wireframe: empty state")


# ── Helpers ─────────────────────────────────────────────────────────────


def wrap(text: str, width: int) -> list[str]:
    words = text.split()
    lines, cur = [], ""
    for wd in words:
        if len(cur) + len(wd) + 1 > width and cur:
            lines.append(cur)
            cur = wd
        else:
            cur = f"{cur} {wd}".strip()
    if cur:
        lines.append(cur)
    return lines


SCENES = {
    "01-workspace.svg": scene_workspace,
    "02-import-dialog.svg": scene_import_dialog,
    "03-stock-origin.svg": scene_stock_origin,
    "04-tool-library.svg": scene_tool_library,
    "05-operation-profile.svg": scene_operation_profile,
    "06-operation-vcarve.svg": scene_operation_vcarve,
    "07-operation-fields.svg": scene_operation_fields,
    "08-tabs-on-canvas.svg": scene_tabs_canvas,
    "09-preview-legend.svg": scene_preview_legend,
    "10-warnings-panel.svg": scene_warnings,
    "11-export-dialog.svg": scene_export,
    "12-empty-state.svg": scene_empty_state,
}


def main() -> None:
    here = os.path.dirname(os.path.abspath(__file__))
    for name, fn in SCENES.items():
        with open(os.path.join(here, name), "w", encoding="utf-8") as f:
            f.write(fn())
        print(name)


if __name__ == "__main__":
    main()
