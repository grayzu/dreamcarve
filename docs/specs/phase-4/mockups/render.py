#!/usr/bin/env python3
# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0
"""Renders the phase 4 mock-ups in this folder as SVG.

    python3 render.py          # writes NN-*.svg beside this script

Since the screenshot review of #549 (2026-11-10) the frames are drawn with
`../experience.md`'s light tokens, type scale and control sizes, so a frame
shows the layout, labels and field names *and* the look the review asked
for. They are still mock-ups: the egui build is the authority on pixels.
Edit a scene function and re-run; do not edit the SVGs by hand. Standard
library only.
"""

from __future__ import annotations

import math
import os
from dataclasses import dataclass, field

# ── Tokens (experience.md section 5, light) ─────────────────────────────

FONT = "Inter, -apple-system, 'Segoe UI', Helvetica, Arial, sans-serif"
MONO = "'JetBrains Mono', Menlo, Consolas, monospace"
WINDOW = "#ECEEF1"
PANEL = "#F7F8FA"
RAISED = "#FFFFFF"
SUNKEN = "#FFFFFF"
STRIPE = "#F2F3F6"
BORDER = "#D7DBE2"
STRONG = "#B9C0CA"
TEXT = "#1C2128"
TEXT2 = "#5B6472"
DIS = "#9AA3AF"
ACCENT = "#2563D0"
ACCENT_SOFT = "#E3ECFA"  # accent.soft composited on panel
HOVER = "#EAECEF"
ERROR = "#C62E27"
ERROR_TINT = "#FBEDEC"
WARN = "#D98A1E"
WARN_TEXT = "#A3580A"
INFO = "#2A6CC4"
SUCCESS = "#1F7F43"
VIEW_BG = "#FAFAF8"
STOCK_FILL = "#FFFFFF"
GRID_MINOR = "#ECEEF1"
GRID_MAJOR = "#D9DCE2"
AXIS = "#6B7380"
SHAPE = "#23272E"
RAPID = "#8A929E"
TAB = "#1C2128"
LABEL_BG = "#FFFFFF"
# The proposed light operation palette (experience.md, design Q5).
PALETTE = ["#1F77B4", "#D62728", "#2A8F2A", "#8E5FB8", "#D96A00", "#0E98A8", "#8C564B", "#C94FA8"]
RAMP = ["#AFC2EE", "#7C98E0", "#4A68C9", "#2A44A6", "#1A2E8C"]
NOTE_BG = "#FFF8E1"


def esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


@dataclass
class Svg:
    w: int
    h: int
    parts: list[str] = field(default_factory=list)

    def add(self, s: str) -> None:
        self.parts.append(s)

    def rect(self, x, y, w, h, fill=RAISED, stroke="none", rx=0, sw=1, dash=None, opacity=None, shadow=False):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        o = f' opacity="{opacity}"' if opacity is not None else ""
        f = ' filter="url(#shadow)"' if shadow else ""
        self.add(f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" rx="{rx}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"{d}{o}{f}/>')

    def line(self, x1, y1, x2, y2, stroke=BORDER, sw=1, dash=None, opacity=None):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        o = f' opacity="{opacity}"' if opacity is not None else ""
        self.add(f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{x2:.1f}" y2="{y2:.1f}" stroke="{stroke}" stroke-width="{sw}" stroke-linecap="round"{d}{o}/>')

    def circle(self, cx, cy, r, fill="none", stroke=TEXT, sw=1, dash=None, opacity=None):
        d = f' stroke-dasharray="{dash}"' if dash else ""
        o = f' opacity="{opacity}"' if opacity is not None else ""
        self.add(f'<circle cx="{cx:.1f}" cy="{cy:.1f}" r="{r:.1f}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"{d}{o}/>')

    def path(self, d, stroke=TEXT, sw=1, fill="none", dash=None, opacity=None):
        dd = f' stroke-dasharray="{dash}"' if dash else ""
        o = f' opacity="{opacity}"' if opacity is not None else ""
        self.add(f'<path d="{d}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}" stroke-linecap="round" stroke-linejoin="round"{dd}{o}/>')

    def text(self, x, y, s, size=13, fill=TEXT, weight=400, anchor="start", mono=False, italic=False, opacity=None):
        fam = MONO if mono else FONT
        st = ' font-style="italic"' if italic else ""
        o = f' opacity="{opacity}"' if opacity is not None else ""
        self.add(f'<text x="{x:.1f}" y="{y:.1f}" font-family="{fam}" font-size="{size}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}"{st}{o}>{esc(s)}</text>')

    def group(self, opacity):
        self.add(f'<g opacity="{opacity}">')

    def end(self):
        self.add("</g>")

    def render(self, title: str) -> str:
        head = (
            f'<svg xmlns="http://www.w3.org/2000/svg" width="{self.w}" height="{self.h}" '
            f'viewBox="0 0 {self.w} {self.h}" font-family="{FONT}">\n'
            f"<title>{esc(title)}</title>\n"
            '<defs><filter id="shadow" x="-10%" y="-10%" width="120%" height="130%">'
            '<feDropShadow dx="0" dy="4" stdDeviation="6" flood-color="#000" flood-opacity="0.14"/></filter>'
            '<pattern id="hatch" width="4" height="4" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">'
            f'<rect width="4" height="4" fill="{RAPID}" opacity="0.35"/><line x1="0" y1="0" x2="0" y2="4" stroke="{RAPID}" stroke-width="1.2"/></pattern>'
            "</defs>\n"
            f'<rect width="{self.w}" height="{self.h}" fill="{WINDOW}"/>\n'
        )
        return head + "\n".join(self.parts) + "\n</svg>\n"


def text_w(s: str, size=13) -> float:
    """A rough advance width for Inter at `size`."""
    return len(s) * size * 0.56


# ── Icons (Lucide shapes, simplified to a 16-px box at (x, y) top-left) ──


def icon(s: Svg, name: str, x, y, col=TEXT2, size=16):
    k = size / 16.0

    def P(px, py):
        return x + px * k, y + py * k

    def pth(d, fill="none", sw=1.4):
        # d uses 0..16 coordinates; scale them.
        out = []
        for tok in d.split():
            if "," in tok:
                a, b = tok.split(",")
                X, Y = P(float(a), float(b))
                out.append(f"{X:.1f},{Y:.1f}")
            else:
                out.append(tok)
        s.path(" ".join(out), stroke=col, sw=sw, fill=fill)

    cx, cy = P(8, 8)
    if name == "eye":
        pth("M 1.5,8 Q 8,1.5 14.5,8 Q 8,14.5 1.5,8 Z")
        s.circle(cx, cy, 2.2 * k, stroke=col, sw=1.4)
    elif name == "eye-off":
        pth("M 1.5,8 Q 8,1.5 14.5,8 Q 8,14.5 1.5,8 Z")
        pth("M 2,2 L 14,14")
    elif name in ("lock", "lock-open"):
        s.rect(x + 3 * k, y + 7 * k, 10 * k, 7 * k, fill="none", stroke=col, rx=1.5, sw=1.4)
        if name == "lock":
            pth("M 5,7 L 5,5 Q 5,2 8,2 Q 11,2 11,5 L 11,7")
        else:
            pth("M 5,7 L 5,5 Q 5,2 8,2 Q 11,2 11,4")
    elif name == "square":
        s.rect(x + 2.5 * k, y + 2.5 * k, 11 * k, 11 * k, fill="none", stroke=col, rx=2, sw=1.4)
    elif name == "circle":
        s.circle(cx, cy, 5.5 * k, stroke=col, sw=1.4)
    elif name == "spline":
        s.circle(x + 3 * k, y + 13 * k, 1.6 * k, stroke=col, sw=1.2)
        s.circle(x + 13 * k, y + 3 * k, 1.6 * k, stroke=col, sw=1.2)
        pth("M 4.2,11.8 Q 5,4 8,8 Q 11,12 11.8,4.2")
    elif name == "component":
        for dx, dy in ((8, 3), (13, 8), (8, 13), (3, 8)):
            pth(f"M {dx},{dy - 2} L {dx + 2},{dy} L {dx},{dy + 2} L {dx - 2},{dy} Z")
    elif name in ("link", "unlink"):
        pth("M 6.5,9.5 L 9.5,6.5")
        if name == "link":
            pth("M 7,4.5 L 8.5,3 Q 11,0.5 13,3 Q 15.5,5 13,7.5 L 11.5,9")
            pth("M 9,11.5 L 7.5,13 Q 5,15.5 3,13 Q 0.5,11 3,8.5 L 4.5,7")
        else:
            pth("M 8,3.5 L 9,2.5 Q 11,0.5 13,3 Q 15.5,5 13,7.5 L 12,8.5")
            pth("M 8,12.5 L 7,13.5 Q 5,15.5 3,13 Q 0.5,11 3,8.5 L 4,7.5")
            pth("M 2,2 L 4,4 M 12,12 L 14,14")
    elif name == "chevron-right":
        pth("M 6,4 L 10,8 L 6,12")
    elif name == "chevron-down":
        pth("M 4,6 L 8,10 L 12,6")
    elif name == "plus":
        pth("M 8,3 L 8,13 M 3,8 L 13,8")
    elif name == "copy":
        s.rect(x + 5 * k, y + 5 * k, 9 * k, 9 * k, fill="none", stroke=col, rx=1.5, sw=1.4)
        pth("M 3,11 L 2,11 L 2,2 L 11,2 L 11,3")
    elif name == "trash":
        pth("M 2.5,4 L 13.5,4 M 6,4 L 6,2.5 L 10,2.5 L 10,4 M 4,4 L 4.8,14 L 11.2,14 L 12,4")
    elif name == "target":
        s.circle(cx, cy, 6 * k, stroke=col, sw=1.3)
        s.circle(cx, cy, 3.3 * k, stroke=col, sw=1.3)
        s.circle(cx, cy, 0.9 * k, fill=col, stroke="none")
    elif name == "pencil":
        pth("M 3,13 L 3.5,10.5 L 11,3 L 13,5 L 5.5,12.5 Z")
    elif name == "ellipsis":
        for dx in (3.5, 8, 12.5):
            s.circle(x + dx * k, cy, 1.1 * k, fill=col, stroke="none")
    elif name == "layers":
        pth("M 8,2 L 14.5,5.5 L 8,9 L 1.5,5.5 Z")
        pth("M 1.5,8.5 L 8,12 L 14.5,8.5")
        pth("M 1.5,11.5 L 8,15 L 14.5,11.5")
    elif name == "rotate":
        s.rect(x + 4 * k, y + 6 * k, 8 * k, 8 * k, fill="none", stroke=col, rx=1.5, sw=1.4)
        pth("M 3,5 Q 5,1.5 9,2 L 12,2 M 10,0.5 L 12,2 L 10,3.5")
    elif name == "flip-h":
        pth("M 8,1.5 L 8,14.5", sw=1.2)
        pth("M 6,4 L 2,12 L 6,12 Z")
        pth("M 10,4 L 14,12 L 10,12 Z", fill=col)
    elif name == "flip-v":
        pth("M 1.5,8 L 14.5,8", sw=1.2)
        pth("M 4,6 L 12,2 L 12,6 Z")
        pth("M 4,10 L 12,14 L 12,10 Z", fill=col)
    elif name == "folder-open":
        pth("M 1.5,13 L 1.5,3 L 6,3 L 7.5,5 L 13,5 L 13,7 M 1.5,13 L 4,7.5 L 15,7.5 L 12.5,13 Z")
    elif name == "save":
        pth("M 2,2 L 11.5,2 L 14,4.5 L 14,14 L 2,14 Z")
        pth("M 5,2 L 5,6 L 10,6 L 10,2 M 4.5,14 L 4.5,9.5 L 11.5,9.5 L 11.5,14")
    elif name == "undo":
        pth("M 5,3 L 2,6 L 5,9 M 2,6 L 10,6 Q 14,6 14,10 Q 14,14 10,14 L 7,14")
    elif name == "redo":
        pth("M 11,3 L 14,6 L 11,9 M 14,6 L 6,6 Q 2,6 2,10 Q 2,14 6,14 L 9,14")
    elif name == "file-input":
        pth("M 4,9 L 4,14 L 14,14 L 14,5 L 10,1.5 L 4,1.5 L 4,4")
        pth("M 1,7.5 L 8,7.5 M 6,5.5 L 8,7.5 L 6,9.5")
    elif name == "image-plus":
        s.rect(x + 1.5 * k, y + 3 * k, 11 * k, 10 * k, fill="none", stroke=col, rx=1.5, sw=1.4)
        pth("M 1.5,11 L 5,7.5 L 9,11.5")
        pth("M 13,1 L 13,6 M 10.5,3.5 L 15.5,3.5")
    elif name == "maximize":
        pth("M 1.5,5.5 L 1.5,1.5 L 5.5,1.5 M 10.5,1.5 L 14.5,1.5 L 14.5,5.5 M 14.5,10.5 L 14.5,14.5 L 10.5,14.5 M 5.5,14.5 L 1.5,14.5 L 1.5,10.5")
    elif name == "bookmark-plus":
        pth("M 3.5,1.5 L 12.5,1.5 L 12.5,14.5 L 8,11 L 3.5,14.5 Z")
        pth("M 8,4.5 L 8,8.5 M 6,6.5 L 10,6.5")
    elif name == "download":
        pth("M 8,1.5 L 8,10 M 4.5,6.5 L 8,10 L 11.5,6.5 M 2,11 L 2,14 L 14,14 L 14,11")
    elif name == "circle-x":
        s.circle(cx, cy, 6.5 * k, fill=col, stroke="none")
        s.path(f"M{cx - 2.5 * k:.1f} {cy - 2.5 * k:.1f} L{cx + 2.5 * k:.1f} {cy + 2.5 * k:.1f} M{cx + 2.5 * k:.1f} {cy - 2.5 * k:.1f} L{cx - 2.5 * k:.1f} {cy + 2.5 * k:.1f}", stroke=RAISED, sw=1.6)
    elif name == "triangle-alert":
        pth("M 8,1.8 L 15,14 L 1,14 Z")
        pth("M 8,6 L 8,9.5")
        s.circle(cx, y + 11.8 * k, 0.8 * k, fill=col, stroke="none")
    elif name == "info":
        s.circle(cx, cy, 6.5 * k, stroke=col, sw=1.4)
        pth("M 8,7.5 L 8,11.5")
        s.circle(cx, y + 5 * k, 0.8 * k, fill=col, stroke="none")
    elif name == "circle-check":
        s.circle(cx, cy, 6.5 * k, stroke=col, sw=1.5)
        pth("M 5,8.2 L 7.2,10.4 L 11,6", sw=1.6)
    elif name == "x":
        pth("M 4,4 L 12,12 M 12,4 L 4,12")
    elif name == "search":
        s.circle(x + 7 * k, y + 7 * k, 4.8 * k, stroke=col, sw=1.4)
        pth("M 10.5,10.5 L 14,14")
    elif name == "file-text":
        pth("M 3,1.5 L 10,1.5 L 13.5,5 L 13.5,14.5 L 3,14.5 Z M 5.5,8 L 11,8 M 5.5,11 L 11,11")
    elif name == "grip":
        for dy in (4, 8, 12):
            for dx in (6, 10):
                s.circle(x + dx * k, y + dy * k, 0.9 * k, fill=col, stroke="none")
    elif name == "check":
        pth("M 3,8.5 L 6.5,12 L 13,4.5", sw=1.6)
    elif name == "cylinder":
        pth("M 3.5,3.5 L 3.5,12.5 Q 8,15.5 12.5,12.5 L 12.5,3.5")
        pth("M 3.5,3.5 Q 8,0.5 12.5,3.5 Q 8,6.5 3.5,3.5 Z")
    elif name == "triangle-down":
        pth("M 2.5,3 L 13.5,3 L 8,14 Z")
    elif name == "crosshair":
        s.circle(cx, cy, 6 * k, stroke=col, sw=1.3)
        pth("M 8,0.5 L 8,4 M 8,12 L 8,15.5 M 0.5,8 L 4,8 M 12,8 L 15.5,8")


def op_glyph(s: Svg, kind: str, x, y, col):
    """DreamCarve's operation-kind glyphs (experience.md section 6), 16 px."""
    def pth(d, fill="none"):
        out = []
        for tok in d.split():
            if "," in tok:
                a, b = tok.split(",")
                out.append(f"{x + float(a):.1f},{y + float(b):.1f}")
            else:
                out.append(tok)
        s.path(" ".join(out), stroke=col, sw=1.4, fill=fill)

    if kind == "Profile":
        s.rect(x + 2, y + 2, 9, 9, fill="none", stroke=col, rx=2, sw=1.4)
        pth("M 13.5,5 L 13.5,13.5 L 5,13.5")
    elif kind == "Pocket":
        s.rect(x + 2, y + 2, 12, 12, fill="none", stroke=col, rx=2, sw=1.4)
        pth("M 4.5,5.5 L 11.5,5.5 M 4.5,8 L 11.5,8 M 4.5,10.5 L 11.5,10.5")
    elif kind == "Drill":
        s.circle(x + 8, y + 8, 6, stroke=col, sw=1.4)
        s.circle(x + 8, y + 8, 1.4, fill=col, stroke="none")
    elif kind == "Bore":
        s.circle(x + 8, y + 8, 6, stroke=col, sw=1.4)
        s.circle(x + 8, y + 8, 3, stroke=col, sw=1.2)
    elif kind == "Engrave":
        pth("M 1.5,10 Q 4.5,3 8,8 Q 11.5,13 14.5,6")
    elif kind == "V-Carve":
        pth("M 3,3 L 8,11.5 L 13,3 M 3,14 L 13,14")
    elif kind == "Inlay":
        pth("M 2.5,2.5 L 8,8.5 L 13.5,2.5 M 2.5,14 L 8,9.5 L 13.5,14")
    elif kind == "Photo V-Carve":
        s.rect(x + 1.5, y + 2.5, 13, 11, fill="none", stroke=col, rx=2, sw=1.4)
        pth("M 4,11 L 7,5 M 7,11 L 10,5 M 10,11 L 13,5")


# ── Controls (experience.md section 7) ──────────────────────────────────

LABEL_W = 112


def caption(s: Svg, text: str) -> None:
    """The mock-up's caption strip along the top (not part of the app)."""
    s.rect(0, 0, s.w, 24, fill=NOTE_BG)
    s.text(10, 16, text, size=12, fill=TEXT2, italic=True)


def note(s: Svg, x, y, w, lines, title=None):
    """A designer's annotation (yellow), not app chrome."""
    h = 14 + len(lines) * 16 + (18 if title else 0)
    s.rect(x, y, w, h, fill=NOTE_BG, stroke="#E9D9A6", rx=6)
    yy = y + 18
    if title:
        s.text(x + 10, yy, title, size=12, weight=600, fill=TEXT)
        yy += 18
    for ln in lines:
        s.text(x + 10, yy, ln, size=12, fill=TEXT2)
        yy += 16
    return y + h


def panel_title(s: Svg, x, y, w, title, chevron=None, right=None, fill=PANEL):
    """A 32-px panel-title band; `chevron` True/False makes it collapsible."""
    s.rect(x, y, w, 32, fill=fill)
    tx = x + 12
    if chevron is not None:
        icon(s, "chevron-down" if chevron else "chevron-right", x + 8, y + 8, TEXT2)
        tx = x + 28
    s.text(tx, y + 21, title, size=13, weight=600)
    if right:
        s.text(x + w - 12, y + 21, right, size=12, fill=TEXT2, anchor="end")
    s.line(x, y + 32, x + w, y + 32, stroke=BORDER)
    return y + 33


def section(s: Svg, x, y, w, title, open_=True, summary=None, badge=None, chevron=True):
    """A 28-px disclosure header; closed headers may carry a summary."""
    s.rect(x, y, w, 28, fill=PANEL)
    tx = x + 12
    if chevron:
        icon(s, "chevron-down" if open_ else "chevron-right", x + 8, y + 6, TEXT2)
        tx = x + 28
    s.text(tx, y + 18, title, size=13, weight=600)
    if summary and not open_:
        sx = tx + text_w(title, 13) + 10
        room = (x + w - 16 - (40 if badge else 0)) - sx
        txt = summary
        while text_w(txt, 12) > room and len(txt) > 4:
            txt = txt[:-2]
        if txt != summary:
            txt = txt.rstrip(" ,·") + "…"
        s.text(sx, y + 18, txt, size=12, fill=TEXT2)
    if badge:
        kind, n = badge
        col = WARN if kind == "warning" else ERROR
        icon(s, "triangle-alert" if kind == "warning" else "circle-x", x + w - 40, y + 6, col)
        s.text(x + w - 20, y + 18, str(n), size=12, fill=WARN_TEXT if kind == "warning" else ERROR)
    return y + 28


def field(s: Svg, x, y, w, label, value="", unit="", state="normal", message=None, reset=False, label_w=LABEL_W, dim=False, readonly=False):
    """A 26-px row: label, then a field with its unit inside."""
    s.text(x, y + 17, label, size=13, fill=TEXT2)
    bx = x + label_w
    bw = w - label_w
    border = {"normal": STRONG, "error": ERROR, "required": ERROR, "warning": WARN, "placeholder": STRONG}[state]
    s.rect(bx, y, bw, 26, fill=PANEL if readonly else SUNKEN, stroke=BORDER if readonly else border, rx=4)
    ux = bx + bw - 8
    if unit:
        s.text(ux, y + 17, unit, size=12, fill=TEXT2, anchor="end")
        ux -= text_w(unit, 12) + 6
    if state == "required":
        icon(s, "circle-x", ux - 20, y + 5, ERROR)
        s.text(bx + 8, y + 17, "Required", size=13, fill=DIS)
    elif state == "placeholder":
        s.text(bx + 8, y + 17, value, size=13, fill=DIS)
        if reset:
            pass
    else:
        s.text(bx + 8, y + 17, value, size=13, fill=TEXT2 if (dim or readonly) else TEXT)
        if reset:
            s.path(f"M{ux - 12} {y + 9} a4 4 0 1 0 4 -1 m0 0 l0 -3 m0 3 l-3 0", stroke=TEXT2, sw=1.2)
    ny = y + 26 + 4
    if message:
        col = ERROR if state == "error" else WARN_TEXT
        icon(s, "circle-x" if state == "error" else "triangle-alert", bx, ny, ERROR if state == "error" else WARN, size=14)
        s.text(bx + 18, ny + 11, message, size=12, fill=col)
        ny += 20
    return ny


def dropdown(s: Svg, x, y, w, label, value, label_w=LABEL_W, disabled=False, lead=None):
    if label:
        s.text(x, y + 17, label, size=13, fill=DIS if disabled else TEXT2)
    bx = x + label_w if label else x
    bw = w - (label_w if label else 0)
    s.rect(bx, y, bw, 26, fill=SUNKEN, stroke=BORDER if disabled else STRONG, rx=4)
    tx = bx + 8
    if lead:
        icon(s, lead, bx + 6, y + 5, TEXT2)
        tx += 20
    s.text(tx, y + 17, value, size=13, fill=DIS if disabled else TEXT)
    icon(s, "chevron-down", bx + bw - 22, y + 5, TEXT2)
    return y + 30


def segmented(s: Svg, x, y, w, options, active, label=None, label_w=LABEL_W):
    if label:
        s.text(x, y + 17, label, size=13, fill=TEXT2)
        x += label_w
        w -= label_w
    s.rect(x, y, w, 26, fill=STRIPE, stroke=STRONG, rx=4)
    sw = w / len(options)
    for i, opt in enumerate(options):
        if i == active:
            s.rect(x + i * sw + 2, y + 2, sw - 4, 22, fill=RAISED, stroke=STRONG, rx=3)
        s.text(x + i * sw + sw / 2, y + 17, opt, size=13, weight=500 if i == active else 400, fill=TEXT if i == active else TEXT2, anchor="middle")
    return y + 30


def checkbox(s: Svg, x, y, label, checked=False, disabled=False):
    if checked:
        s.rect(x, y + 5, 16, 16, fill=ACCENT, stroke=ACCENT, rx=3)
        icon(s, "check", x, y + 5, RAISED)
    else:
        s.rect(x, y + 5, 16, 16, fill=SUNKEN, stroke=BORDER if disabled else STRONG, rx=3)
    s.text(x + 24, y + 18, label, size=13, fill=DIS if disabled else TEXT)
    return y + 26


def toggle(s: Svg, x, y, on=True):
    s.rect(x, y + 5, 28, 16, fill=ACCENT if on else STRIPE, stroke=ACCENT if on else STRONG, rx=8)
    s.circle(x + (20 if on else 8), y + 13, 6, fill=RAISED, stroke="none")


def button(s: Svg, x, y, label="", kind="secondary", w=None, icon_name=None, h=26, disabled=False, shortcut=None):
    """kind: primary, secondary, quiet, destructive."""
    lw = text_w(label, 13) if label else 0
    w = w or (lw + (28 if icon_name else 0) + (18 if label else 10))
    fill, stroke, col = {
        "primary": (ACCENT, ACCENT, RAISED),
        "secondary": (RAISED, STRONG, TEXT),
        "quiet": ("none", "none", TEXT),
        "destructive": (RAISED, STRONG, ERROR),
    }[kind]
    if disabled:
        fill, stroke, col = (PANEL if kind != "quiet" else "none"), (BORDER if kind != "quiet" else "none"), DIS
    s.rect(x, y, w, h, fill=fill, stroke=stroke, rx=4)
    if icon_name and label:
        icon(s, icon_name, x + 9, y + (h - 16) / 2, col if kind == "primary" else (DIS if disabled else TEXT2))
        s.text(x + 31, y + h / 2 + 4.5, label, size=13, fill=col, weight=500 if kind == "primary" else 400)
    elif icon_name:
        icon(s, icon_name, x + (w - 16) / 2, y + (h - 16) / 2, col if kind == "primary" else (DIS if disabled else TEXT2))
    else:
        s.text(x + w / 2, y + h / 2 + 4.5, label, size=13, fill=col, anchor="middle", weight=500 if kind == "primary" else 400)
    if shortcut:
        s.text(x + w - 10, y + h / 2 + 4.5, shortcut, size=12, fill=TEXT2, anchor="end")
    return x + w


def status_cell_sep(s: Svg, x, y):
    s.line(x, y + 5, x, y + 19, stroke=BORDER)


# ── Window chrome ───────────────────────────────────────────────────────


def os_title(s: Svg, y, title):
    """The OS title bar: the project's name lives here and only here."""
    s.rect(0, y, s.w, 28, fill="#E3E5E9")
    for i, c in enumerate(("#FF5F57", "#FEBC2E", "#28C840")):
        s.circle(14 + i * 20, y + 14, 6, fill=c, stroke="none")
    s.text(s.w / 2, y + 18, title, size=13, weight=500, fill=TEXT, anchor="middle")
    return y + 28


def menubar(s: Svg, y, open_menu=None):
    s.rect(0, y, s.w, 28, fill=PANEL)
    x = 12
    pos = {}
    for m in ("File", "Edit", "Arrange", "View", "Project", "Help"):
        if m == open_menu:
            s.rect(x - 6, y + 3, text_w(m) + 12, 22, fill=HOVER, rx=4)
        s.text(x, y + 19, m, size=13)
        pos[m] = x - 6
        x += text_w(m) + 20
    s.line(0, y + 28, s.w, y + 28, stroke=BORDER)
    return y + 28, pos


def toolbar(s: Svg, y, arrange_enabled=True):
    s.rect(0, y, s.w, 40, fill=PANEL)
    x = 8
    groups = [
        [("Open", "folder-open"), ("Save", "save")],
        [("Undo", "undo"), ("Redo", "redo")],
        [("Add drawing", "file-input"), ("Add picture", "image-plus")],
        "arrange",
        [("Fit", "maximize"), ("Tab tool", "bookmark-plus")],
    ]
    for gi, g in enumerate(groups):
        if g == "arrange":
            # Rotate split button, then the two flips; icon-only.
            dis = not arrange_enabled
            s.rect(x, y + 6, 46, 28, fill="none", stroke="none", rx=4)
            icon(s, "rotate", x + 6, y + 12, DIS if dis else TEXT2)
            s.line(x + 28, y + 11, x + 28, y + 29, stroke=BORDER)
            icon(s, "chevron-down", x + 30, y + 12, DIS if dis else TEXT2, size=14)
            x += 50
            for nm in ("flip-h", "flip-v"):
                icon(s, nm, x + 6, y + 12, DIS if dis else TEXT2)
                x += 30
        else:
            for lab, ic in g:
                x = button(s, x, y + 6, lab, "quiet", icon_name=ic, h=28) + 2
        if gi < len(groups) - 1:
            s.line(x + 7, y + 12, x + 7, y + 28, stroke=BORDER)
            x += 16
    button(s, s.w - 108, y + 7, "Export", "primary", w=96, icon_name="download", h=26)
    s.line(0, y + 40, s.w, y + 40, stroke=BORDER)
    return y + 40


def statusbar(s: Svg, y, cells, message=None, pressed=None):
    """`cells`: list of (kind, text). Kinds: units, cursor, toolpaths, gen, counts, est, z, plain."""
    s.rect(0, y, s.w, 24, fill=PANEL)
    s.line(0, y, s.w, y, stroke=BORDER)
    x = 12
    pos = {}
    for kind, txt in cells:
        start = x
        if kind == "units":
            s.text(x, y + 16, txt, size=12, fill=TEXT2)
            icon(s, "chevron-down", x + text_w(txt, 12) + 4, y + 5, TEXT2, size=14)
            x += text_w(txt, 12) + 22
        elif kind == "cursor":
            s.text(x, y + 16, txt, size=12, fill=TEXT2)
            x += 120
        elif kind == "toolpaths":
            if pressed == "toolpaths":
                s.rect(x - 6, y + 2, text_w(txt, 12) + 34, 20, fill=HOVER, rx=4)
            icon(s, "layers", x, y + 4, TEXT2)
            s.text(x + 20, y + 16, txt, size=12, fill=TEXT)
            x += text_w(txt, 12) + 22
        elif kind == "gen":
            s.circle(x + 6, y + 12, 5, stroke=ACCENT, sw=1.5, dash="18 8")
            s.text(x + 16, y + 16, txt, size=12, fill=TEXT2)
            x += text_w(txt, 12) + 20
            s.text(x, y + 16, "Stop", size=12, fill=TEXT)
            x += 30
        elif kind == "stopped":
            s.text(x, y + 16, "Stopped", size=12, fill=TEXT2)
            x += 52
            s.text(x, y + 16, "Regenerate all", size=12, fill=ACCENT)
            x += 86
        elif kind == "counts":
            for part in txt:
                k, n = part
                ic = {"error": "circle-x", "warning": "triangle-alert", "hint": "info", "ok": "circle-check"}[k]
                col = {"error": ERROR, "warning": WARN, "hint": INFO, "ok": TEXT2}[k]
                tcol = {"error": ERROR, "warning": WARN_TEXT, "hint": INFO, "ok": TEXT2}[k]
                icon(s, ic, x, y + 4, col, size=15)
                s.text(x + 19, y + 16, n, size=12, fill=tcol)
                x += 19 + text_w(n, 12) + 10
        else:
            s.text(x, y + 16, txt, size=12, fill=TEXT2)
            x += text_w(txt, 12) + 4
        pos[kind] = (start, x)
        x += 12
        status_cell_sep(s, x, y)
        x += 12
    if message:
        s.text(x, y + 16, message, size=12, fill=TEXT2)
    return pos


# ── The Viewport (the plaque from the phase 2 quick start, Y up) ────────


def viewport(s: Svg, x, y, w, h, scale=3.2, depth_mode=False, selected=False, extents=False, stale=None,
             targets_of=None, show_tabs=True, tab_hollow=False, show_arrows=False, origin="bl", toolpaths=True, ox=60, oy=60,
             target_names=None):
    """Stock 160 x 100 mm. `targets_of`: index of the selected operation."""
    s.rect(x, y, w, h, fill=VIEW_BG)
    sx, sy = x + ox, y + h - oy
    sw, sh = 160 * scale, 100 * scale

    def P(mx, my):
        return sx + mx * scale, sy - my * scale

    s.rect(sx, sy - sh, sw, sh, fill=STOCK_FILL)
    for gx in range(0, 161, 10):
        X, _ = P(gx, 0)
        s.line(X, sy - sh, X, sy, stroke=GRID_MAJOR if gx % 100 == 0 else GRID_MINOR)
    for gy in range(0, 101, 10):
        _, Y = P(0, gy)
        s.line(sx, Y, sx + sw, Y, stroke=GRID_MAJOR if gy % 100 == 0 else GRID_MINOR)
    s.rect(sx, sy - sh, sw, sh, fill="none", stroke=AXIS, sw=1.5, dash="6 4")
    # Origin crosshair at the chosen corner.
    oxm, oym = {"bl": (0, 0), "tl": (0, 100), "c": (80, 50)}[origin]
    OX, OY = P(oxm, oym)
    s.line(OX - 12, OY, OX - 2, OY, stroke=AXIS, sw=1.2)
    s.line(OX + 2, OY, OX + 12, OY, stroke=AXIS, sw=1.2)
    s.line(OX, OY - 12, OX, OY - 2, stroke=AXIS, sw=1.2)
    s.line(OX, OY + 2, OX, OY + 12, stroke=AXIS, sw=1.2)
    s.text(OX - 6, OY + 18 if oym == 0 else OY - 8, "0,0", size=12, fill=TEXT2, anchor="end")

    targets = {0: ["outline"], 1: ["panel", "island"], 2: ["hole-1", "hole-2", "hole-3", "hole-4"], 3: ["flourish"]}.get(targets_of, None)
    if target_names is not None:
        targets = target_names

    def shape_style(name):
        if targets is None:
            return SHAPE, 1.25, 1.0
        if name in targets:
            return PALETTE[targets_of], 2.2, 1.0
        return SHAPE, 1.25, 0.4

    col, w_, o = shape_style("outline")
    X0, Y0 = P(10, 10)
    s.rect(X0, Y0 - 80 * scale, 140 * scale, 80 * scale, fill="none", stroke=col, rx=6 * scale, sw=w_, opacity=o)
    px, py = P(30, 30)
    col, w_, o = shape_style("panel")
    s.rect(px, py - 50 * scale, 70 * scale, 50 * scale, fill="none", stroke=col, sw=w_, opacity=o)
    cx, cy = P(65, 55)
    col, w_, o = shape_style("island")
    s.circle(cx, cy, 10 * scale, stroke=col, sw=w_, opacity=o)
    for k, (hx, hy) in enumerate(((20, 20), (140, 20), (20, 80), (140, 80))):
        col, w_, o = shape_style(f"hole-{k + 1}")
        X, Y = P(hx, hy)
        s.circle(X, Y, 2.5 * scale, stroke=col, sw=w_, opacity=o)
    lx1, ly1 = P(110, 40)
    lx2, ly2 = P(140, 70)
    col, w_, o = shape_style("flourish")
    s.path(f"M{lx1:.1f} {ly1:.1f} q{15 * scale:.1f} {-40 * scale:.1f} {lx2 - lx1:.1f} {ly2 - ly1:.1f}", stroke=col, sw=w_, dash="4 3", opacity=o)

    if toolpaths:
        def c(i, z):
            if depth_mode:
                t = min(max(z / 12.0, 0.0), 1.0)
                return RAMP[min(int(t * 4 + 0.5), 4)]
            return PALETTE[i]

        fade = 1.0  # other toolpaths stay at full alpha (experience.md section 9)
        r = 3.175 * scale
        pw, ph = 140 * scale, 80 * scale
        s.group(1.0 if targets_of in (None, 0) else fade)
        s.rect(X0 - r, Y0 - ph - r, pw + 2 * r, ph + 2 * r, fill="none", stroke=c(0, 12), rx=6 * scale + r, sw=1.5)
        if show_tabs:
            for tx, ty in ((80, 10), (80, 90), (10, 50), (150, 50)):
                X, Y = P(tx, ty)
                if tx in (10, 150):
                    X += -r if tx == 10 else r
                    if tab_hollow and tx == 10:
                        s.rect(X - 2.5, Y - 4 * scale, 5, 8 * scale, fill=STOCK_FILL, stroke=TAB, sw=1.5)
                    else:
                        s.line(X, Y - 4 * scale, X, Y + 4 * scale, stroke=TAB, sw=4)
                else:
                    Y += r if ty == 10 else -r
                    s.line(X - 4 * scale, Y, X + 4 * scale, Y, stroke=TAB, sw=4)
        X, Y = P(10, 50)
        s.circle(X - r, Y, max(r, 6), stroke=c(0, 12), sw=1.2)
        s.line(X - r, Y, X - r, Y - 2 * r, stroke=c(0, 12), sw=1.2)
        if show_arrows:
            for t in (0.25, 0.5, 0.75):
                ax = X0 - r + (pw + 2 * r) * t
                ay = Y0 - ph - r
                s.path(f"M{ax - 4:.1f} {ay - 3:.1f} l5 3 l-5 3", stroke=c(0, 12), sw=1.4)
        s.end()
        pc = c(1, 6)
        s.group(0.35 if stale == 1 else (1.0 if targets_of in (None, 1) else fade))
        for i in range(1, 12):
            my = 30 + i * 50 / 12
            X1, Yl = P(30 + 3.2, my)
            X2, _ = P(100 - 3.2, my)
            if abs(my - 55) < 13.2:
                dx = math.sqrt(max(13.2 ** 2 - (my - 55) ** 2, 0)) * scale
                s.line(X1, Yl, cx - dx, Yl, stroke=pc, sw=1.2)
                s.line(cx + dx, Yl, X2, Yl, stroke=pc, sw=1.2)
            else:
                s.line(X1, Yl, X2, Yl, stroke=pc, sw=1.2)
        ri = 3.2 * scale
        s.rect(px + ri, py - 50 * scale + ri, 70 * scale - 2 * ri, 50 * scale - 2 * ri, fill="none", stroke=pc, sw=1.4)
        s.circle(cx, cy, 10 * scale + ri, stroke=pc, sw=1.4)
        s.end()
        s.group(1.0 if targets_of in (None, 2) else fade)
        for hx, hy in ((20, 20), (140, 20), (20, 80), (140, 80)):
            X, Y = P(hx, hy)
            s.circle(X, Y, 2.5 * scale, stroke=c(2, 12), sw=1.5)
        s.end()
        s.group(1.0 if targets_of in (None, 3) else fade)
        s.path(f"M{lx1:.1f} {ly1:.1f} q{15 * scale:.1f} {-40 * scale:.1f} {lx2 - lx1:.1f} {ly2 - ly1:.1f}", stroke=c(3, 1), sw=1.5)
        s.end()
        A, B, C = P(20, 80), P(140, 80), P(140, 20)
        s.line(A[0], A[1], B[0], B[1], stroke=RAPID, dash="4 3")
        s.line(B[0], B[1], C[0], C[1], stroke=RAPID, dash="4 3")
    if selected:
        X, Y = P(30, 30)
        bw_, bh_ = 70 * scale, 50 * scale
        s.rect(X, Y - bh_, bw_, bh_, fill="none", stroke=ACCENT, sw=1)
        for hx, hy in ((X, Y), (X + bw_, Y), (X, Y - bh_), (X + bw_, Y - bh_)):
            s.rect(hx - 4, hy - 4, 8, 8, fill=RAISED, stroke=ACCENT, sw=1.2)
        s.line(X + bw_ / 2, Y - bh_, X + bw_ / 2, Y - bh_ - 20, stroke=ACCENT)
        s.circle(X + bw_ / 2, Y - bh_ - 24, 4, fill=RAISED, stroke=ACCENT, sw=1.2)
    if extents:
        e1 = P(10 - 3.175, 10 - 3.175)
        e2 = P(150 + 3.175, 90 + 3.175)
        s.rect(e1[0], e2[1], e2[0] - e1[0], e1[1] - e2[1], fill="none", stroke="#5B6472", sw=1)
        e1 = P(10 - 6.35, 10 - 6.35)
        e2 = P(150 + 6.35, 90 + 6.35)
        s.rect(e1[0], e2[1], e2[0] - e1[0], e1[1] - e2[1], fill="none", stroke=RAPID, sw=1, dash="6 4")
        lab = "Program extents (bit edge, dashed)  X 3.65…156.35  Y 3.65…96.35"
        s.rect(e1[0], e2[1] - 22, text_w(lab, 12) + 16, 18, fill=LABEL_BG, rx=9, opacity=0.9)
        s.text(e1[0] + 8, e2[1] - 9, lab, size=12, fill=TEXT2)
    return P


def depth_legend(s: Svg, x, y):
    """Bottom-left legend in By depth mode, with the positive band (functional 73a)."""
    w, h = 176, 296
    s.rect(x, y, w, h, fill=LABEL_BG, stroke=BORDER, rx=6, opacity=0.96)
    s.text(x + 12, y + 20, "Z (mm)", size=12, weight=600)
    bx, by = x + 14, y + 32
    s.rect(bx, by, 12, 44, fill="url(#hatch)", stroke=RAPID, sw=0.8)
    ticks = [(0, "+10  clearance", TEXT2), (22, "+5  safe Z", TEXT2)]
    by2 = by + 44
    for i in range(60):
        t = i / 59
        idx = t * 4
        lo = int(min(idx, 3))
        f = idx - lo
        c1, c2 = RAMP[lo], RAMP[lo + 1]
        mix = "#" + "".join(f"{int(int(c1[k:k + 2], 16) * (1 - f) + int(c2[k:k + 2], 16) * f):02x}" for k in (1, 3, 5))
        s.rect(bx, by2 + i * 3, 12, 3, fill=mix)
    for dy, lab, col in ticks:
        s.line(bx + 12, by + dy, bx + 18, by + dy, stroke=TEXT2)
        s.text(bx + 24, by + dy + 4, lab, size=12, fill=col)
    for z, lab in ((0, "0  stock top"), (1, "−1  Flourish"), (6, "−6  Panel pocket"), (12, "−12.2  Outside cut +1")):
        yy = by2 + z / 12 * 177
        s.line(bx + 12, yy, bx + 18, yy, stroke=TEXT)
        s.text(bx + 24, yy + 4, lab, size=12, fill=TEXT if z else TEXT)
    s.text(x + 12, y + h - 10, "Grey: rapids, retracts", size=12, fill=TEXT2)


# ── Panels ──────────────────────────────────────────────────────────────

SHAPES = [
    ("outline", "square", "2", False),
    ("panel", "square", "1", False),
    ("island", "circle", "1", False),
    ("hole-1", "circle", "1", False),
    ("hole-2", "circle", "1", False),
    ("hole-3", "circle", "1", True),
    ("hole-4", "circle", "1", False),
    ("flourish", "spline", "1", False),
]


def design_tree(s: Svg, x, y, w, h, selected=None, targets=None, target_col=None, edit_mode=False, edit_set=()):
    s.rect(x, y, w, h, fill=PANEL)
    yy = panel_title(s, x, y, w, "Design Tree")
    s.rect(x, yy, w, 26, fill=PANEL)
    s.text(x + 12, yy + 17, "Shapes", size=13, weight=600)
    icon(s, "eye", x + w - 52, yy + 5, TEXT2)
    icon(s, "lock-open", x + w - 28, yy + 5, TEXT2)
    yy += 26
    for name, glyph, ops, locked in SHAPES:
        if name == selected:
            s.rect(x, yy, w, 26, fill=ACCENT_SOFT)
            s.rect(x, yy, 2, 26, fill=ACCENT)
        is_target = targets and name in targets
        if edit_mode:
            checkbox(s, x + 10, yy, "", checked=name in edit_set)
        else:
            icon(s, "eye", x + 10, yy + 5, TEXT2)
        if locked:
            icon(s, "lock", x + 30, yy + 5, TEXT2)
        if is_target and target_col:
            s.rect(x, yy + 2, 3, 22, fill=target_col)
        icon(s, glyph, x + 52, yy + 5, TEXT2)
        s.text(x + 74, yy + 17, name, size=13, weight=500 if name == selected else 400)
        if name == "panel" and not edit_mode:
            pass
        if is_target and not edit_mode:
            icon(s, "target", x + w - 40, yy + 5, target_col or TEXT2, size=14)
        s.text(x + w - 14, yy + 17, ops, size=12, fill=TEXT2, anchor="end")
        yy += 26
    for sec in ("Tabs", "Pictures"):
        s.text(x + 12, yy + 17, sec, size=13, weight=600)
        icon(s, "eye", x + w - 52, yy + 5, TEXT2)
        icon(s, "lock-open", x + w - 28, yy + 5, TEXT2)
        yy += 26
        if sec == "Tabs":
            icon(s, "bookmark-plus", x + 52, yy + 5, TEXT2)
            s.text(x + 74, yy + 17, "outline · 4 tabs", size=13)
            yy += 26
    return yy


def project_settings(s: Svg, x, y, w, open_=True, thickness_required=False):
    yy = panel_title(s, x, y, w, "Project Settings", chevron=open_)
    if not open_:
        return yy
    fx, fw = x + 12, w - 24
    yy += 6
    yy = segmented(s, fx, yy, fw, ["mm", "in"], 0, label="Units") + 2
    yy = section(s, x, yy, w, "Stock") + 4
    yy = field(s, fx, yy, fw, "Width", "160", "mm")
    yy = field(s, fx, yy, fw, "Height", "100", "mm")
    yy = field(s, fx, yy, fw, "Thickness", "" if thickness_required else "12", "mm", state="required" if thickness_required else "normal")
    yy = field(s, fx, yy, fw, "Margin", "10", "mm")
    button(s, fx + LABEL_W, yy, "Fit to shapes")
    yy += 32
    yy = section(s, x, yy, w, "Origin") + 4
    yy = dropdown(s, fx, yy, fw, "XY zero", "Bottom-left")
    yy = segmented(s, fx, yy, fw, ["Top", "Bottom"], 0, label="Z zero")
    yy = section(s, x, yy, w, "Heights", open_=False, summary="Clearance 10 mm, park 150, 95")
    yy = section(s, x, yy, w, "Tolerances", open_=False, summary="Flatten 0.01, arc fit 0.01 mm")
    yy = section(s, x, yy, w, "Post-processor", open_=False, summary="grblHAL v4, one program per tool")
    return yy


def properties(s: Svg, x, y, w, name="panel", locked=False):
    yy = panel_title(s, x, y, w, f"Properties · {name}", chevron=True)
    fx, fw = x + 12, w - 24 - 30
    yy += 6
    yy = field(s, fx, yy, fw, "X", "30", "mm", readonly=locked)
    yy = field(s, fx, yy, fw, "Y", "30", "mm", readonly=locked)
    wy = yy
    yy = field(s, fx, yy, fw, "W", "70", "mm", readonly=locked)
    hy = yy
    yy = field(s, fx, yy, fw, "H", "50", "mm", readonly=locked)
    # The proportion link: a bracket from W to H with the chain on it.
    bx = fx + fw + 8
    s.path(f"M{bx} {wy + 13} h6 v{hy - wy} h-6", stroke=STRONG, sw=1)
    s.rect(bx + 2, wy + 13 + (hy - wy) / 2 - 11, 22, 22, fill=PANEL, rx=4)
    icon(s, "link", bx + 5, wy + 13 + (hy - wy) / 2 - 8, TEXT)
    if locked:
        icon(s, "lock", fx, yy + 4, TEXT2)
        s.text(fx + 22, yy + 16, "Locked: cannot be moved or resized.", size=12, fill=TEXT2)
        s.text(fx + 22, yy + 32, "Operations still cut it.", size=12, fill=TEXT2)
        button(s, fx + 22, yy + 42, "Unlock")
        yy += 74
    return yy


OPS = [
    ("Profile", "Outside cut", "Profile · 6.35 mm end mill", None),
    ("Pocket", "Panel pocket", "Pocket · 6.35 mm end mill", ("warning", "1")),
    ("Drill", "Screw holes", "Drill · 3.175 mm end mill", None),
    ("Engrave", "Flourish", "Engrave · 3.175 mm end mill", None),
    ("V-Carve", "Lettering", "V-Carve · 90° V-bit", ("error", "")),
]


def cam_tabs(s: Svg, x, y, w, active="Operations", counts=(3, 5)):
    yy = panel_title(s, x, y, w, "CAM")
    s.rect(x, yy, w, 32, fill=PANEL)
    tw = w / 2
    for i, (lab, n) in enumerate((("Tools", counts[0]), ("Operations", counts[1]))):
        on = lab == active
        s.text(x + i * tw + tw / 2, yy + 21, f"{lab} · {n}", size=13, weight=600 if on else 400, fill=TEXT if on else TEXT2, anchor="middle")
        if on:
            s.rect(x + i * tw + 12, yy + 30, tw - 24, 2, fill=ACCENT)
    s.line(x, yy + 32, x + w, yy + 32, stroke=BORDER)
    return yy + 33


def op_list(s: Svg, x, y, w, selected=1, stale=None, rows=OPS):
    s.rect(x, y, w, 40 + 40 * len(rows), fill=PANEL)
    bx = button(s, x + 12, y + 8, "Add", icon_name="plus") + 6
    bx = button(s, bx, y + 8, "Duplicate", icon_name="copy") + 6
    button(s, bx, y + 8, "Delete", icon_name="trash")
    yy = y + 42
    for i, (kind, name, sub, st) in enumerate(rows):
        if i == selected:
            s.rect(x, yy, w, 40, fill=ACCENT_SOFT)
            s.rect(x, yy, 2, 40, fill=ACCENT)
        icon(s, "grip", x + 6, yy + 12, DIS)
        checkbox(s, x + 26, yy + 7, "", checked=(i != 3))
        op_glyph(s, kind, x + 52, yy + 12, PALETTE[i])
        s.text(x + 76, yy + 17, name, size=13, weight=500 if i == selected else 400, fill=TEXT if i != 3 else TEXT2)
        s.text(x + 76, yy + 33, sub, size=12, fill=TEXT2)
        icon(s, "eye" if i != 3 else "eye-off", x + w - 56, yy + 12, TEXT2)
        if stale == i:
            s.circle(x + w - 24, yy + 20, 6, stroke=ACCENT, sw=1.5, dash="22 8")
        elif st:
            k, n = st
            icon(s, "triangle-alert" if k == "warning" else "circle-x", x + w - 32, yy + 12, WARN if k == "warning" else ERROR)
            if n:
                s.text(x + w - 12, yy + 25, n, size=12, fill=WARN_TEXT, anchor="end")
        yy += 40
    s.line(x, yy, x + w, yy, stroke=BORDER)
    return yy + 1


def editor_header(s: Svg, x, y, w, kind, name, colour, status=None):
    s.rect(x, y, w, 32, fill=PANEL)
    op_glyph(s, kind, x + 12, y + 8, colour)
    s.text(x + 36, y + 21, name, size=13, weight=600)
    s.text(x + 36 + text_w(name, 13) + 6, y + 21, f"· {kind}", size=13, fill=TEXT2)
    if status:
        icon(s, "triangle-alert", x + w - 28, y + 8, WARN)
    s.line(x, y + 32, x + w, y + 32, stroke=BORDER)
    return y + 33


def targets_row(s: Svg, x, y, w, summary, warn=False, label="Targets"):
    s.text(x, y + 17, label, size=13, fill=TEXT2)
    bx = x + LABEL_W
    icon(s, "target", bx, y + 5, TEXT2)
    room = (x + w - (84 if warn else 62)) - (bx + 22)
    txt = summary
    while text_w(txt, 13) > room and len(txt) > 4:
        txt = txt[:-2]
    if txt != summary:
        txt = txt.rstrip(" ,·") + "…"
    s.text(bx + 22, y + 17, txt, size=13)
    if warn:
        icon(s, "triangle-alert", x + w - 76, y + 5, WARN)
    button(s, x + w - 56, y, "", "quiet", w=26, icon_name="pencil")
    button(s, x + w - 28, y, "", "quiet", w=26, icon_name="ellipsis")
    return y + 30


def pocket_editor(s: Svg, x, y, w, open_strategy=False):
    yy = editor_header(s, x, y, w, "Pocket", "Panel pocket", PALETTE[1], status=True)
    fx, fw = x + 12, w - 24
    yy = section(s, x, yy, w, "Setup", chevron=False) + 4
    yy = targets_row(s, fx, yy, fw, "2 shapes · panel, island")
    yy = dropdown(s, fx, yy, fw, "Tool", "6.35 mm end mill (T1)", lead="cylinder")
    yy = field(s, fx, yy, fw, "Depth", "6", "mm")
    yy = segmented(s, fx, yy, fw, ["Offset", "Raster"], 1, label="Clearance")
    yy += 2
    yy = section(s, x, yy, w, "Depth", open_=False, summary="step-down tool's 3 mm, leave 0, allowance 0")
    if open_strategy:
        yy = section(s, x, yy, w, "Strategy", open_=True) + 4
        yy = field(s, fx, yy, fw, "Raster angle", "0", "°")
        yy = segmented(s, fx, yy, fw, ["Last", "First", "None"], 0, label="Wall pass")
        yy = field(s, fx, yy, fw, "Stepover", "tool's: 40", "%", state="placeholder")
        yy = segmented(s, fx, yy, fw, ["Climb", "Conventional"], 0, label="Direction")
        yy = dropdown(s, fx, yy, fw, "Rest of", "None")
    else:
        yy = section(s, x, yy, w, "Strategy", open_=False, summary="Raster 0°, climb, wall last")
    yy = section(s, x, yy, w, "Entry", open_=False, summary="Helix 80 %, 15°")
    yy = section(s, x, yy, w, "Corners", open_=False, summary="Dogbone", badge=("warning", 1))
    yy = section(s, x, yy, w, "Order", open_=False, summary="Full depth first")
    yy = section(s, x, yy, w, "Heights", open_=False, summary="Safe Z 5 mm")
    return yy


# ── Scenes ──────────────────────────────────────────────────────────────

W, H = 1280, 820


def workspace_frame(s: Svg, title, open_menu=None, arrange_enabled=True):
    y = os_title(s, 24, title)
    y, menus = menubar(s, y, open_menu)
    y = toolbar(s, y, arrange_enabled)
    return y, menus


def scene_workspace():
    s = Svg(W, H)
    caption(s, "01 · The workspace after the review (50, 50b, 57, 58, 58a, 66, 67). The panel shape is selected; Panel pocket is the selected operation, its targets marked.")
    y, _ = workspace_frame(s, "plaque — DreamCarve")
    bottom = H - 24
    left_w, right_w = 280, 340
    s.rect(0, y, left_w, bottom - y, fill=PANEL)
    ty = design_tree(s, 0, y, left_w, 340, selected="panel", targets=["panel", "island"], target_col=PALETTE[1])
    s.line(0, y + 344, left_w, y + 344, stroke=BORDER, sw=4)
    ly = project_settings(s, 0, y + 346, left_w, open_=False)
    properties(s, 0, ly, left_w, name="panel")
    vx = left_w + 4
    vw = W - left_w - right_w - 8
    s.rect(vx - 4, y, 4, bottom - y, fill=WINDOW)
    viewport(s, vx, y, vw, bottom - y, scale=3.6, targets_of=1, selected=True)
    rx = W - right_w
    s.rect(rx - 4, y, 4, bottom - y, fill=WINDOW)
    s.rect(rx, y, right_w, bottom - y, fill=PANEL)
    yy = cam_tabs(s, rx, y, right_w)
    yy = op_list(s, rx, yy, right_w, selected=1)
    pocket_editor(s, rx, yy + 3, right_w)
    statusbar(s, bottom, [("units", "mm"), ("cursor", "X 64.2  Y 51.8"), ("toolpaths", "Toolpaths"), ("counts", [("error", "1"), ("warning", "1"), ("hint", "1")]), ("est", "Est. 14 min")],
              message="Panel pocket · flat floor at 6 mm, 2 islands kept")
    return s.render("DreamCarve phase 4 mock-up: workspace")


def sheet(s: Svg, x, y, w, h, title, right_controls=None):
    s.rect(x, y, w, h, fill=RAISED, stroke=BORDER, rx=8, shadow=True)
    s.text(x + 24, y + 32, title, size=18, weight=600)
    icon(s, "x", x + w - 36, y + 18, TEXT2)
    return y + 52


def scrim(s: Svg):
    s.rect(0, 0, s.w, s.h, fill="#000000", opacity=0.25)


def scene_import_dialog():
    s = Svg(720, 560)
    caption(s, "02 · Add a drawing to the project (55): the drawing alone in the preview, no stock box, no origin or placement note.")
    x, w = 80, 560
    yy = sheet(s, x, 50, w, 480, "Add drawing to project")
    fx, fw = x + 24, w - 48
    s.text(fx, yy + 17, "File", size=13, fill=TEXT2)
    s.rect(fx + LABEL_W, yy, fw - LABEL_W - 96, 26, fill=SUNKEN, stroke=STRONG, rx=4)
    s.text(fx + LABEL_W + 8, yy + 17, "~/cnc/plaque.dxf", size=12, mono=True)
    button(s, fx + fw - 88, yy, "Choose…", w=88)
    yy += 34
    s.text(fx + LABEL_W, yy + 12, "DXF · 14 closed shapes, 1 open path, 3 layers", size=12, fill=TEXT2)
    yy += 26
    yy = section(s, x, yy, w, "Options", chevron=False) + 4
    yy = dropdown(s, fx, yy, fw, "Units", "Millimetres (from the file)")
    yy = field(s, fx, yy, fw, "Join tolerance", "0.01", "mm")
    yy = section(s, x, yy + 2, w, "Layers", chevron=False) + 4
    ly = yy
    for lab, on, dis in (("CUT · 7 shapes", True, False), ("POCKET · 3 shapes", True, False), ("DRILL · 4 circles", True, False), ("NOTES · 2 TEXT, not read", False, True)):
        ly = checkbox(s, fx, ly, lab, on, disabled=dis)
    # Preview: the drawing alone, fitted.
    pvx, pvy, pvw, pvh = fx + 250, yy, fw - 250, 104
    s.rect(pvx, pvy, pvw, pvh, fill=VIEW_BG, stroke=BORDER, rx=4)
    s.rect(pvx + 18, pvy + 12, pvw - 36, pvh - 24, fill="none", stroke=SHAPE, rx=8)
    s.rect(pvx + 60, pvy + 28, 90, 48, fill="none", stroke=SHAPE)
    s.circle(pvx + 105, pvy + 52, 12, stroke=SHAPE)
    for hx, hy in ((pvx + 30, pvy + 22), (pvx + pvw - 30, pvy + 22), (pvx + 30, pvy + pvh - 22), (pvx + pvw - 30, pvy + pvh - 22)):
        s.circle(hx, hy, 4, stroke=SHAPE)
    s.path(f"M{pvx + 170} {pvy + 70} q12 -40 34 -24", stroke=SHAPE, dash="4 3")
    yy = ly + 14
    icon(s, "triangle-alert", fx, yy, WARN, size=14)
    s.text(fx + 20, yy + 11, "2 TEXT entities are not read; they will be listed in the Warnings panel.", size=12, fill=WARN_TEXT)
    s.line(x, 50 + 480 - 56, x + w, 50 + 480 - 56, stroke=BORDER)
    button(s, x + w - 24 - 140, 50 + 480 - 42, "Add 14 shapes", "primary", w=140, h=28)
    return s.render("DreamCarve phase 4 mock-up: add drawing")


def scene_stock_origin():
    s = Svg(900, 860)
    caption(s, "03 · Project Settings (53, 60–62): Units first; thickness required; changing XY zero moves the origin marker, not the part.")
    s.rect(20, 40, 300, 800, fill=PANEL, stroke=BORDER)
    yy = panel_title(s, 20, 40, 300, "Project Settings", chevron=True)
    fx, fw = 32, 276
    yy += 6
    yy = segmented(s, fx, yy, fw, ["mm", "in"], 0, label="Units") + 2
    yy = section(s, 20, yy, 300, "Stock") + 4
    yy = field(s, fx, yy, fw, "Width", "160", "mm")
    yy = field(s, fx, yy, fw, "Height", "100", "mm")
    yy = field(s, fx, yy, fw, "Thickness", "", "mm", state="required")
    yy = field(s, fx, yy, fw, "Margin", "10", "mm")
    button(s, fx + LABEL_W, yy, "Fit to shapes")
    yy += 34
    yy = section(s, 20, yy, 300, "Origin") + 4
    yy = dropdown(s, fx, yy, fw, "XY zero", "Top-left")
    yy = segmented(s, fx, yy, fw, ["Top", "Bottom"], 0, label="Z zero")
    yy = section(s, 20, yy, 300, "Heights") + 4
    yy = field(s, fx, yy, fw, "Clearance", "10", "mm")
    yy = field(s, fx, yy, fw, "Tool-change Z", "−5", "mm")
    yy = field(s, fx, yy, fw, "Park point", "150, 95", "mm")
    yy = section(s, 20, yy, 300, "Tolerances", open_=False, summary="Flatten 0.01, arc fit 0.01 mm")
    yy = section(s, 20, yy, 300, "Post-processor") + 4
    yy = dropdown(s, fx, yy, fw, "Controller", "grblHAL v4 (latest)")
    yy = field(s, fx, yy, fw, "Extension", ".nc", "")
    yy = segmented(s, fx, yy, fw, ["Automatic", "Manual"], 0, label="Spindle")
    yy = field(s, fx, yy, fw, "Spin-up dwell", "3", "s")
    yy = dropdown(s, fx, yy, fw, "Tool change", "One program per tool")
    yy = field(s, fx, yy, fw, "Decimals", "3 mm · 4 in", "")
    checkbox(s, fx + LABEL_W, yy, "Canned cycles", disabled=True)
    # Right: two Viewport fragments, before and after XY zero.
    for i, (org, lab) in enumerate((("bl", "XY zero: Bottom-left"), ("tl", "XY zero: Top-left"))):
        vy = 40 + i * 400
        s.text(350, vy + 14, lab, size=13, weight=600)
        viewport(s, 350, vy + 24, 520, 330, scale=2.6, origin=org, toolpaths=False, ox=40, oy=40)
    note(s, 350, 804 - 40, 520, ["The shapes, stock and toolpaths do not move; the 0,0 marker and the", "status bar's coordinates do. Thickness unset: Required, message in its tooltip."])
    return s.render("DreamCarve phase 4 mock-up: Project Settings")


LIB = [
    ("Amana", [("Amana 45702 90° V-bit", "V-bit", "6.35", "90°", "2", "1500", "500", "18000", "45702", True),
               ("Amana 45771 60° V-bit", "V-bit", "6.35", "60°", "2", "1500", "500", "18000", "45771", False)]),
    ("IDC Woodcraft", [("1/4 in Downcut", "End mill", "6.35", "", "2", "1800", "600", "18000", "DC-14", True),
                       ("1/8 in Upcut", "End mill", "3.175", "", "2", "1200", "400", "18000", "UC-18", False),
                       ("Tapered ball nose 0.015", "Ball nose", "0.762", "6.7°", "2", "1000", "300", "20000", "TBN-15", False)]),
    ("No vendor", [("Club 2 mm drill", "Drill", "2", "", "2", "", "300", "12000", "", False),
                   ("tool7", "End mill", "6.35", "", "2", "", "", "", "", False)]),
]


def library_table(s: Svg, x, y, w, selected="1/8 in Upcut", review=False):
    cols = [("", 32), ("Name", 172), ("Type", 74), ("Ø", 50), ("Angle", 48), ("Feed", 50), ("Plunge", 54), ("RPM", 54), ("Product", 0)]
    if review:
        cols = [("", 32), ("Name", 210), ("Type", 78), ("Ø", 50), ("Feed", 58), ("Status", 0)]
    s.rect(x, y, w, 28, fill=PANEL)
    cx = x
    for name, cw in cols:
        s.text(cx + 8, y + 18, name, size=12, fill=TEXT2)
        cx += cw
    s.line(x, y + 28, x + w, y + 28, stroke=BORDER)
    yy = y + 29
    for vendor, rows in LIB:
        s.rect(x, yy, w, 28, fill=PANEL)
        icon(s, "chevron-down", x + 8, yy + 6, TEXT2)
        s.text(x + 28, yy + 18, vendor, size=13, weight=600)
        s.text(x + 28 + text_w(vendor, 13) + 6, yy + 18, f"· {len(rows)}", size=13, fill=TEXT2)
        yy += 28
        for i, r in enumerate(rows):
            sel = r[0] == selected
            s.rect(x, yy, w, 28, fill=ACCENT_SOFT if sel else (STRIPE if i % 2 else RAISED))
            if sel:
                s.rect(x, yy, 2, 28, fill=ACCENT)
            in_proj = r[9]
            if review:
                status = {"Amana 45702 90° V-bit": ("Same", TEXT2), "1/8 in Upcut": ("Changed", WARN_TEXT), "tool7": ("Skipped: no cutter for thread mills", ERROR)}.get(r[0], ("New", SUCCESS))
                name = "Thread mill 1/4-20" if r[0] == "tool7" else r[0]
                vals = ["", name, "—" if r[0] == "tool7" else r[1], r[2], r[5], status[0]]
                cx = x
                for j, (name, cw) in enumerate(cols):
                    if j:
                        s.text(cx + 8, yy + 18, vals[j], size=13, fill=status[1] if j == 5 else TEXT)
                    cx += cw
            else:
                checkbox(s, x + 8, yy + 1, "", checked=r[0] in ("1/8 in Upcut", "Amana 45771 60° V-bit"))
                vals = ["", r[0], r[1], r[2], r[3], r[5], r[6], r[7], r[8]]
                cx = x
                for j, (name, cw) in enumerate(cols):
                    if j:
                        v = vals[j]
                        col = TEXT2 if in_proj else TEXT
                        if j in (5, 6, 7) and v == "":
                            s.text(cx + 8, yy + 18, "—", size=13, fill=ERROR)
                        else:
                            s.text(cx + 8, yy + 18, v, size=13, fill=col, mono=(j == 8))
                    cx += cw
                if in_proj:
                    icon(s, "check", x + w - 22, yy + 6, TEXT2)
            yy += 28
    return yy


def scene_tool_library():
    s = Svg(1040, 1330)
    caption(s, "04 · Tool Library (64, 64a, 64b): vendor groups, editing in place, its own units; below, the import review where Replace now lives.")
    x, w, h = 40, 960, 660
    y0 = 44
    yy = sheet(s, x, y0, w, h, "Tool Library")
    segmented(s, x + w - 420, y0 + 14, 90, ["mm", "in"], 0)
    button(s, x + w - 320, y0 + 14, "Import…", w=84)
    button(s, x + w - 228, y0 + 14, "Export library…", w=136)
    lw = w - 320 - 24
    lx = x + 24
    s.rect(lx, yy, 300, 26, fill=SUNKEN, stroke=STRONG, rx=4)
    icon(s, "search", lx + 8, yy + 5, TEXT2)
    s.text(lx + 32, yy + 17, "Search name, vendor, product, diameter", size=13, fill=DIS)
    dropdown(s, lx + 310, yy, 140, None, "All types")
    yy += 36
    bx = button(s, lx, yy, "New tool", icon_name="plus") + 6
    bx = button(s, bx, yy, "Duplicate", icon_name="copy") + 6
    button(s, bx, yy, "Delete", icon_name="trash")
    yy += 36
    ty = library_table(s, lx, yy, lw - 12)
    # Editing pane.
    px = x + w - 320
    s.rect(px, y0 + 52, 320, h - 52 - 56, fill=PANEL)
    s.line(px, y0 + 52, px, y0 + h - 56, stroke=BORDER)
    py = y0 + 52
    s.rect(px, py, 320, 32, fill=PANEL)
    icon(s, "cylinder", px + 12, py + 8, TEXT2)
    s.text(px + 36, py + 21, "1/8 in Upcut", size=13, weight=600)
    s.text(px + 36 + text_w("1/8 in Upcut", 13) + 6, py + 21, "· IDC Woodcraft", size=13, fill=TEXT2)
    py += 40
    fx, fw = px + 12, 296
    py = field(s, fx, py, fw, "Name", "1/8 in Upcut")
    py = dropdown(s, fx, py, fw, "Type", "End mill")
    py = field(s, fx, py, fw, "Diameter", "3.175", "mm")
    py = field(s, fx, py, fw, "Flutes", "2")
    py = field(s, fx, py, fw, "Feed", "1200", "mm/min")
    py = field(s, fx, py, fw, "Plunge rate", "400", "mm/min")
    py = field(s, fx, py, fw, "RPM", "18000", "rpm")
    py = field(s, fx, py, fw, "Step-down", "1.5", "mm")
    py = field(s, fx, py, fw, "Stepover", "40", "%")
    py = field(s, fx, py, fw, "Vendor", "IDC Woodcraft")
    py = field(s, fx, py, fw, "Product id", "UC-18")
    s.text(fx, py + 16, "Saved to the library as you edit.", size=12, fill=TEXT2)
    button(s, fx, py + 28, "Revert changes", w=132)
    # Footer.
    fy = y0 + h - 56
    s.line(x, fy, x + w, fy, stroke=BORDER)
    s.text(x + 24, fy + 32, "7 tools · 2 ticked · ✓ already in plaque", size=12, fill=TEXT2)
    button(s, x + w - 24 - 190, fy + 14, "Add 2 tools to project", "primary", w=190, h=28)

    # The import review, the same sheet with its panes replaced.
    y1 = y0 + h + 40
    s.text(x, y1 - 12, "After Import… and choosing club.tools: the panes are replaced in place (no second sheet).", size=12, fill=TEXT2, italic=True)
    yy = sheet(s, x, y1, w, 560, "Import club.tools")
    cx = x + 24
    for lab, n, col in (("New", "4", SUCCESS), ("Already in your library", "1", TEXT2), ("Changed", "1", WARN_TEXT), ("Skipped", "1", ERROR)):
        s.rect(cx, yy, 196, 52, fill=PANEL, stroke=BORDER, rx=6)
        s.text(cx + 14, yy + 22, n, size=18, weight=600, fill=col)
        s.text(cx + 14, yy + 42, lab, size=12, fill=TEXT2)
        cx += 210
    yy += 68
    s.text(x + 24, yy + 17, "1 tool changed since you last imported it:", size=13)
    segmented(s, x + 330, yy, 330, ["Keep mine", "Replace with imported"], 0)
    yy += 40
    library_table(s, x + 24, yy, w - 48, selected=None, review=True)
    s.text(x + 24, y1 + 560 - 72, "Keep mine adds the changed tool beside yours as “1/8 in Upcut (2)”; Replace overwrites yours, keeping its id.", size=12, fill=TEXT2)
    fy = y1 + 560 - 56
    s.line(x, fy, x + w, fy, stroke=BORDER)
    button(s, x + 24, fy + 14, "Back", w=72, h=28)
    button(s, x + w - 24 - 150, fy + 14, "Import 5 tools", "primary", w=150, h=28)
    return s.render("DreamCarve phase 4 mock-up: Tool Library")


def scene_operation_profile():
    s = Svg(1100, 700)
    caption(s, "05 · Operation editor (66, 66a, 69): name in the header, Setup first, the other groups closed with their values summarised; right, Corners and tabs open.")
    for col, open_ in ((0, False), (1, True)):
        x, w = 20 + col * 370, 340
        s.rect(x, 40, w, 640, fill=PANEL, stroke=BORDER)
        yy = editor_header(s, x, 40, w, "Profile", "Outside cut", PALETTE[0])
        fx, fw = x + 12, w - 24
        yy = section(s, x, yy, w, "Setup", chevron=False) + 4
        yy = targets_row(s, fx, yy, fw, "1 shape · outline")
        yy = dropdown(s, fx, yy, fw, "Tool", "6.35 mm end mill (T1)", lead="cylinder")
        yy = dropdown(s, fx, yy, fw, "Side", "Outside")
        yy = field(s, fx, yy, fw, "Depth", "12.2", "mm")
        yy = dropdown(s, fx, yy, fw, "Tabs", "Count: 4")
        yy += 2
        yy = section(s, x, yy, w, "Depth", open_=False, summary="step-down tool's 4 mm, allowance 0.2 mm")
        yy = section(s, x, yy, w, "Strategy", open_=False, summary="Climb, start auto, last pass off")
        yy = section(s, x, yy, w, "Entry and leads", open_=False, summary="Ramp 15°, arc lead 3 mm 90°")
        if open_:
            yy = section(s, x, yy, w, "Corners and tabs") + 4
            yy = dropdown(s, fx, yy, fw, "Corner relief", "None")
            yy = field(s, fx, yy, fw, "Tab width", "8", "mm")
            yy = field(s, fx, yy, fw, "Tab height", "3", "mm")
            yy = segmented(s, fx, yy, fw, ["Rect", "Smooth"], 0, label="Tab shape")
            yy = checkbox(s, fx + LABEL_W, yy, "On holes")
            yy = checkbox(s, fx + LABEL_W, yy, "Avoid corners", True)
            button(s, fx + LABEL_W, yy + 2, "Place tabs")
            s.text(fx + LABEL_W + 96, yy + 19, "4 placed on outline", size=12, fill=TEXT2)
            yy += 36
        else:
            yy = section(s, x, yy, w, "Corners and tabs", open_=False, summary="No relief, tabs 8 × 3 mm rect")
        yy = section(s, x, yy, w, "Order", open_=False, summary="Full depth first")
        yy = section(s, x, yy, w, "Heights", open_=False, summary="Safe Z 5 mm")
    ax = 770
    y = 48
    y = note(s, ax, y, 310, ["Kind glyph, the name (click or F2 to", "rename in place), the kind. Enabled is", "the list's checkbox, not here."], title="Header") + 12
    y = note(s, ax, y, 310, ["Targets, Tool and the settings this", "kind needs (66a). Never collapses."], title="Setup") + 12
    y = note(s, ax, y, 310, ["Closed by default; the header says what", "is inside. Every field of 68 is still", "here, once. A located warning opens its", "group and badges the header."], title="Other groups") + 12
    y = note(s, ax, y, 310, ["Hover a label for the chapter's", "definition and the .dcv field name.", "No ? buttons."], title="Help") + 12
    return s.render("DreamCarve phase 4 mock-up: Profile editor")


def scene_operation_vcarve():
    s = Svg(760, 740)
    caption(s, "06 · V-Carve and Inlay editors (66a, 68a, 70): Setup first; the Inlay's male board thickness is required and in Setup.")
    x, w = 20, 340
    s.rect(x, 40, w, 680, fill=PANEL, stroke=BORDER)
    yy = editor_header(s, x, 40, w, "V-Carve", "Lettering", PALETTE[4])
    fx, fw = x + 12, w - 24
    yy = section(s, x, yy, w, "Setup", chevron=False) + 4
    yy = targets_row(s, fx, yy, fw, "2 shapes · rect-frame, oval")
    yy = dropdown(s, fx, yy, fw, "V-bit", "90° V-bit 6.35 mm (T3)", lead="triangle-down")
    s.text(fx, yy + 17, "Flat depth", size=13, fill=TEXT2)
    toggle(s, fx + LABEL_W, yy)
    s.rect(fx + LABEL_W + 36, yy, fw - LABEL_W - 36, 26, fill=SUNKEN, stroke=STRONG, rx=4)
    s.text(fx + LABEL_W + 44, yy + 17, "2.5", size=13)
    s.text(fx + fw - 8, yy + 17, "mm", size=12, fill=TEXT2, anchor="end")
    yy += 30
    s.text(fx, yy + 17, "Clearing", size=13, fill=TEXT2)
    yy += 4
    for i, (tool, meta) in enumerate((("6.35 mm end mill (T1)", "Offset · climb · ramp 15°"), ("3.175 mm end mill (T2)", "Raster 45° · climb · plunge"))):
        cy = yy + i * 48
        s.rect(fx + LABEL_W, cy, fw - LABEL_W, 44, fill=RAISED, stroke=BORDER, rx=4)
        icon(s, "grip", fx + LABEL_W + 4, cy + 14, DIS)
        s.text(fx + LABEL_W + 24, cy + 18, tool, size=13)
        s.text(fx + LABEL_W + 24, cy + 35, meta, size=12, fill=TEXT2)
    yy += 2 * 48
    button(s, fx + LABEL_W, yy, "Add pass", icon_name="plus")
    yy += 34
    yy = section(s, x, yy, w, "Depth", open_=False, summary="start 0, allowance 0")
    yy = section(s, x, yy, w, "Strategy", open_=False, summary="Medial, floor stepover 40 %")
    yy = section(s, x, yy, w, "Order", open_=False, summary="Full depth first")
    yy = section(s, x, yy, w, "Heights", open_=False, summary="Safe Z 5 mm")

    x2 = 400
    s.rect(x2, 40, w, 680, fill=PANEL, stroke=BORDER)
    yy = editor_header(s, x2, 40, w, "Inlay", "DC", PALETTE[0], status=True)
    fx, fw = x2 + 12, w - 24
    yy = section(s, x2, yy, w, "Setup", chevron=False) + 4
    yy = targets_row(s, fx, yy, fw, "2 shapes · letter-D, letter-C")
    yy = dropdown(s, fx, yy, fw, "V-bit", "60° V-bit 6.35 mm (T1)", lead="triangle-down")
    yy = field(s, fx, yy, fw, "Inlay depth", "4", "mm")
    yy = field(s, fx, yy, fw, "Glue gap", "1", "mm")
    yy = field(s, fx, yy, fw, "Plug depth", "2", "mm")
    yy = field(s, fx, yy, fw, "Male thickness", "", "mm", state="required")
    yy += 2
    yy = section(s, x2, yy, w, "Depth", open_=True) + 4
    yy = field(s, fx, yy, fw, "Margin", "8", "mm")
    yy = field(s, fx, yy, fw, "Rounding", "10", "mm")
    yy = field(s, fx, yy, fw, "Allowance", "0", "mm")
    s.rect(fx, yy, fw, 44, fill=STRIPE, rx=4)
    s.text(fx + 10, yy + 18, "Male start plane 3 mm · band floor 5 mm", size=12, fill=TEXT2)
    s.text(fx + 10, yy + 35, "Flank engagement 5 of the cone's 5.5 mm", size=12, fill=TEXT2)
    yy += 52
    yy = section(s, x2, yy, w, "Male board", open_=False, summary="same as stock 110 × 80 mm")
    yy = section(s, x2, yy, w, "Clearing passes", open_=False, summary="T2 offset")
    yy = section(s, x2, yy, w, "Cut-out", open_=False, summary="T2, 4 tabs 6 × 3 mm")
    yy = section(s, x2, yy, w, "Order", open_=False, summary="Full depth first")
    yy = section(s, x2, yy, w, "Heights", open_=False, summary="Safe Z 5 mm")
    note(s, x2, yy + 12, w, ["No side to choose: one project cuts both", "boards (#498). The carve summary that was", "under the editor is now a note in the", "Warnings drawer (10) and the status bar."])
    return s.render("DreamCarve phase 4 mock-up: V-Carve and Inlay editors")


def scene_operation_fields():
    s = Svg(1440, 560)
    caption(s, "07 · Drill, Bore, Engrave, Photo V-Carve editors (66a, 68): each opens with its Setup rows; the rest are closed groups with summaries.")
    cols = [
        ("Drill", "Screw holes", 2, [("t", "4 shapes · hole-1 … hole-4"), ("dd", "Tool", "90° V-bit 6.35 mm (T3)"), ("dd", "Cycle", "Countersink"), ("f", "Mouth diameter", "6", "mm"), ("ro", "Depth", "derived 3.175", "mm")],
         [("Depth", "retract 2 mm, allowance 0"), ("Strategy", "start 0, dwell 0 s"), ("Heights", "Safe Z 5 mm")]),
        ("Bore", "Dowel holes", 5, [("t", "2 shapes · dowel-1, dowel-2"), ("dd", "Tool", "6.35 mm end mill (T1)"), ("f", "Depth", "12", "mm")],
         [("Depth", "pitch tool's 4 mm, leave 0, allowance 0.2"), ("Strategy", "Climb"), ("Heights", "Safe Z 5 mm")]),
        ("Engrave", "Flourish", 3, [("t", "1 shape · flourish"), ("dd", "Tool", "3.175 mm end mill (T2)"), ("f", "Depth", "1", "mm")],
         [("Depth", "step-down tool's 3 mm, allowance 0"), ("Strategy", "Alternate direction"), ("Order", "Full depth first"), ("Heights", "Safe Z 5 mm")]),
        ("Photo V-Carve", "Portrait", 6, [("dd", "Picture", "portrait.png"), ("dd", "Tool", "60° V-bit 6.35 mm (T4)"), ("f", "Max depth", "2", "mm")],
         [("Depth", "start 0, not inverted"), ("Strategy", "Raster 22.5°, spacing 110 %"), ("Heights", "Safe Z 5 mm, retract 1 mm")]),
    ]
    cw = 340
    for i, (kind, name, ci, setup, groups) in enumerate(cols):
        x = 20 + i * (cw + 14)
        s.rect(x, 40, cw, 500, fill=PANEL, stroke=BORDER)
        yy = editor_header(s, x, 40, cw, kind, name, PALETTE[ci])
        fx, fw = x + 12, cw - 24
        yy = section(s, x, yy, cw, "Setup", chevron=False) + 4
        for row in setup:
            if row[0] == "t":
                yy = targets_row(s, fx, yy, fw, row[1])
            elif row[0] == "dd":
                yy = dropdown(s, fx, yy, fw, row[1], row[2])
            elif row[0] == "f":
                yy = field(s, fx, yy, fw, row[1], row[2], row[3])
            elif row[0] == "ro":
                yy = field(s, fx, yy, fw, row[1], row[2], row[3], readonly=True)
        yy += 2
        for g, summ in groups:
            yy = section(s, x, yy, cw, g, open_=False, summary=summ)
    return s.render("DreamCarve phase 4 mock-up: more editors")


def scene_tabs_canvas():
    s = Svg(W, 640)
    caption(s, "08 · Placed tabs on the Viewport (69). Dragging a tab keeps it on its outline; a tab no cut crosses is hollow.")
    P = viewport(s, 0, 30, 900, 590, scale=4.2, tab_hollow=True)
    X, Y = P(80, 90)
    s.circle(X, Y - 3.175 * 4.2, 12, stroke=ACCENT, dash="3 2")
    s.path(f"M{X + 16} {Y - 26} l18 0 l-4 -4 m4 4 l-4 4", stroke=ACCENT, sw=1.4)
    s.text(X + 40, Y - 22, "drag along the outline", size=12, fill=ACCENT)
    X, Y = P(10, 50)
    s.text(X + 14, Y - 30, "hollow: tab_not_crossed", size=12, fill=TEXT2)
    s.rect(910, 30, 350, 220, fill=PANEL, stroke=BORDER)
    yy = panel_title(s, 910, 30, 350, "Design Tree · Tabs")
    for i, lab in enumerate(("outline · tab 1  (80, 90)", "outline · tab 2  (80, 10)", "outline · tab 3  (10, 50)", "outline · tab 4  (150, 50)")):
        if i == 0:
            s.rect(910, yy, 350, 26, fill=ACCENT_SOFT)
            s.rect(910, yy, 2, 26, fill=ACCENT)
        icon(s, "eye", 920, yy + 5, TEXT2)
        icon(s, "bookmark-plus", 944, yy + 5, TEXT2)
        s.text(966, yy + 17, lab, size=13)
        yy += 26
    s.rect(910, 270, 350, 210, fill=PANEL, stroke=BORDER)
    yy = panel_title(s, 910, 270, 350, "Properties · tab 1", chevron=True) + 6
    yy = field(s, 922, yy, 326, "Width", "8", "mm")
    yy = field(s, 922, yy, 326, "Height", "3", "mm")
    yy = segmented(s, 922, yy, 326, ["Rect", "Smooth"], 0, label="Shape")
    yy = field(s, 922, yy, 326, "Position", "80, 90", "mm")
    s.text(922, yy + 14, "Crossed by: Outside cut (Profile)", size=12, fill=TEXT2)
    note(s, 910, 500, 350, ["Place tabs turns a Profile's request into", "placed tabs on the part (ADR-0005).", "Deleting an instance deletes its tabs."])
    return s.render("DreamCarve phase 4 mock-up: tabs on the Viewport")


def toolpath_popover(s: Svg, x, y, depth_mode=False):
    """The toolpath view, opened from the status bar (functional 73)."""
    rows = ["Cuts", "Rapids", "Plunges", "Direction arrows", "Start points", "Tab marks", "Only the selected operation", "Program extents"]
    on = {"Cuts", "Rapids", "Plunges", "Direction arrows", "Start points", "Tab marks", "Program extents"}
    key_rows = 1 if depth_mode else len(OPS)
    h = 32 + 34 + key_rows * 24 + 10 + len(rows) * 24 + 40
    s.rect(x, y, 240, h, fill=RAISED, stroke=BORDER, rx=8, shadow=True)
    s.path(f"M{x + 40} {y + h} l8 8 l8 -8", fill=RAISED, stroke=BORDER)
    s.rect(x + 39, y + h - 2, 18, 3, fill=RAISED)
    s.text(x + 14, y + 22, "Toolpaths", size=13, weight=600)
    yy = segmented(s, x + 12, y + 32, 216, ["By operation", "By depth"], 1 if depth_mode else 0)
    if depth_mode:
        s.text(x + 14, yy + 14, "Pale is shallow, dark is deep.", size=12, fill=TEXT2)
        yy += 24
    else:
        for i, (kind, name, _, _) in enumerate(OPS):
            s.rect(x + 16, yy + 7, 10, 10, fill=PALETTE[i], rx=2)
            s.text(x + 34, yy + 16, name, size=13)
            yy += 24
    s.line(x + 12, yy + 4, x + 228, yy + 4, stroke=BORDER)
    yy += 10
    for r in rows:
        checkbox(s, x + 14, yy - 1, r, r in on)
        yy += 24
    button(s, x + 10, yy + 6, "Show all", "quiet", icon_name="eye")
    button(s, x + 118, yy + 6, "Hide all", "quiet", icon_name="eye-off")
    return h


def scene_preview_legend():
    s = Svg(W, 700)
    caption(s, "09 · Toolpath view (73, 73a, 74): a popover from the status bar; the depth legend bottom-left with the positive band; Pocket regenerating (faded).")
    P = viewport(s, 0, 30, W, 646, scale=4.0, depth_mode=True, extents=True, stale=1, show_arrows=True, ox=320, oy=110)
    depth_legend(s, 16, 676 - 16 - 296)
    pos = statusbar(s, 676, [("units", "mm"), ("cursor", "X 64.2  Y 51.8"), ("toolpaths", "Toolpaths"), ("gen", "Generating 1 of 5…"), ("counts", [("warning", "1")]), ("est", "Est. 14 min"), ("z", "Z −12.2…+10")], pressed="toolpaths")
    tx = pos["toolpaths"][0]
    h = 32 + 34 + 24 + 10 + 8 * 24 + 40
    toolpath_popover(s, tx - 36, 676 - 8 - h, depth_mode=True)
    return s.render("DreamCarve phase 4 mock-up: toolpath view")


def drawer_row(s: Svg, x, y, w, tier, code, msg, where):
    col = {"error": ERROR, "warning": WARN, "hint": INFO}[tier]
    ic = {"error": "circle-x", "warning": "triangle-alert", "hint": "info"}[tier]
    icon(s, ic, x + 12, y + 6, col)
    s.text(x + 36, y + 18, code, size=12, mono=True, fill=TEXT)
    s.text(x + 250, y + 18, msg, size=13)
    s.text(x + w - 12, y + 18, where, size=12, fill=ACCENT, anchor="end")
    return y + 28


def scene_warnings():
    s = Svg(W, 640)
    caption(s, "10 · Warnings drawer (76, 76a): headed groups, carve notes as labelled facts; no Regenerate button. Inset: one group, no heading.")
    x, w = 0, W
    y = 30
    s.rect(x, y, w, 430, fill=PANEL)
    s.line(x, y, x + w, y, stroke=BORDER)
    s.text(12, y + 21, "Warnings", size=13, weight=600)
    icon(s, "circle-x", 92, y + 8, ERROR)
    s.text(112, y + 21, "1", size=12, fill=ERROR)
    icon(s, "triangle-alert", 128, y + 8, WARN)
    s.text(148, y + 21, "2", size=12, fill=WARN_TEXT)
    icon(s, "info", 164, y + 8, INFO)
    s.text(184, y + 21, "1", size=12, fill=INFO)
    button(s, w - 124, y + 3, "Copy", icon_name="copy")
    button(s, w - 36, y + 4, "", "quiet", w=26, icon_name="x")
    s.line(x, y + 32, x + w, y + 32, stroke=BORDER)
    yy = y + 36
    groups = [
        ("Project", [("error", "stock_thickness_unset", "Stock thickness is not set; export refuses until it is.", "Project Settings ▸ Thickness"),
                     ("hint", "tool_change_avoidable", "T2 is used by Screw holes and Flourish, which are not adjacent.", "Operations")]),
        ("Panel pocket · Pocket", [("warning", "relief_corner_too_tight", "2 of panel's corners are too tight for a dogbone; skipped.", "Corners ▸ Corner relief")]),
        ("Lettering · V-Carve", [("warning", "v_carve_target_open", "flourish is an open path and is not carved.", "Targets")]),
    ]
    for g, rows in groups:
        s.text(12, yy + 18, g, size=13, weight=600)
        yy += 26
        for r in rows:
            yy = drawer_row(s, x, yy, w, *r)
        if g.startswith("Lettering"):
            icon(s, "info", 12, yy + 6, TEXT2)
            s.text(36, yy + 18, "Note", size=13, fill=TEXT2)
            facts = [("Deepest cut", "1.658 mm below the stock top"), ("Flat depth", "none (the V-bit's point reaches the floor)"), ("Clearing", "none")]
            fy = yy
            for k, v in facts:
                s.text(250, fy + 18, k, size=13, fill=TEXT2)
                s.text(390, fy + 18, v, size=13)
                fy += 22
            yy = fy + 8
    # Inset: single group.
    iy = 480
    s.text(12, iy, "Inset: the coaster, one group, so no heading; the row still names its operation.", size=12, fill=TEXT2, italic=True)
    s.rect(0, iy + 10, w, 100, fill=PANEL)
    s.text(12, iy + 31, "Warnings", size=13, weight=600)
    icon(s, "triangle-alert", 92, iy + 18, WARN)
    s.text(112, iy + 31, "1", size=12, fill=WARN_TEXT)
    s.line(0, iy + 42, w, iy + 42, stroke=BORDER)
    drawer_row(s, 0, iy + 48, w, "warning", "tab_on_hole", "Outside cut: a tab on hole-3 holds its slug; cutting it free is work by hand.", "Corners and tabs")
    statusbar(s, 616, [("units", "mm"), ("cursor", ""), ("toolpaths", "Toolpaths"), ("counts", [("error", "1"), ("warning", "2"), ("hint", "1")]), ("est", "Est. —")],
              message="Lettering · deepest cut 1.658 mm, no flat depth")
    return s.render("DreamCarve phase 4 mock-up: warnings drawer")


def export_sheet(s: Svg, x, y, blocked=False, details_open=False):
    w, h = 560, 390 if not details_open else 600
    yy = sheet(s, x, y, w, h, "Export G-code")
    fx, fw = x + 24, w - 48
    s.text(fx, yy + 17, "Destination", size=13, fill=TEXT2)
    s.rect(fx + LABEL_W, yy, fw - LABEL_W - 96, 26, fill=SUNKEN, stroke=STRONG, rx=4)
    s.text(fx + LABEL_W + 8, yy + 17, "~/cnc", size=12, mono=True)
    button(s, fx + fw - 88, yy, "Choose…", w=88)
    yy += 30
    yy = field(s, fx, yy, fw, "File name", "plaque")
    s.text(fx + LABEL_W, yy + 10, "Writes plaque-1-T1.nc, plaque-2-T2.nc, plaque-3-T3.nc", size=12, fill=TEXT2)
    yy += 22
    yy = dropdown(s, fx, yy, fw, "Post-processor", "grblHAL v4 (latest)")
    s.text(fx + LABEL_W, yy + 10, "Changing it here changes the project (undoable).", size=12, fill=TEXT2)
    yy += 22
    yy = dropdown(s, fx, yy, fw, "Programs", "As the project says: one per tool")
    yy += 8
    yy = section(s, x, yy, w, "Program Details", open_=details_open, summary="3 programs, run in this order")
    if details_open:
        for i, (fn, tool, ops, t, before) in enumerate((("plaque-1-T1.nc", "T1 6.35 mm end mill", "Panel pocket, Outside cut", "9 min 40 s", "Fit T1 and set work zero X, Y and Z."),
                                                        ("plaque-2-T2.nc", "T2 3.175 mm end mill", "Screw holes, Flourish", "2 min 05 s", "Change to T2, set Z zero. Keep X and Y."),
                                                        ("plaque-3-T3.nc", "T3 90° V-bit", "Lettering", "1 min 50 s", "Change to T3, set Z zero. Keep X and Y."))):
            s.rect(fx, yy + 4, fw, 64, fill=STRIPE if i % 2 else RAISED, rx=4)
            s.text(fx + 10, yy + 22, f"{i + 1}", size=13, fill=TEXT2)
            s.text(fx + 28, yy + 22, fn, size=12, mono=True)
            s.text(fx + fw - 10, yy + 22, t, size=13, anchor="end")
            s.text(fx + 28, yy + 40, f"{tool} · {ops}", size=12, fill=TEXT2)
            s.text(fx + 28, yy + 58, "Before: " + before, size=12, fill=TEXT2)
            yy += 68
    fy = y + h - 56
    if blocked:
        icon(s, "circle-x", fx, fy - 30, ERROR)
        s.text(fx + 22, fy - 18, "1 error blocks export", size=13, fill=ERROR)
        button(s, fx + 164, fy - 35, "Show", "quiet")
    s.line(x, fy, x + w, fy, stroke=BORDER)
    button(s, x + w - 24 - 110, fy + 14, "Export", "primary", w=110, h=28, icon_name="download", disabled=blocked)
    return h


def scene_export():
    s = Svg(1240, 680)
    caption(s, "11 · Export sheet (77): no preview tick, no warnings, no total, no footnote; Program Details closed by default. Right: details opened; below, blocked.")
    export_sheet(s, 40, 50)
    export_sheet(s, 640, 50, details_open=True)
    s.text(40, 470, "With an error: one line above the footer and a disabled Export (its tooltip gives the same reason).", size=12, fill=TEXT2, italic=True)
    s.rect(40, 482, 560, 120, fill=RAISED, stroke=BORDER, rx=8, shadow=True)
    icon(s, "circle-x", 64, 506, ERROR)
    s.text(86, 519, "1 error blocks export", size=13, fill=ERROR)
    button(s, 228, 502, "Show", "quiet")
    s.line(40, 546, 600, 546, stroke=BORDER)
    button(s, 600 - 24 - 110, 560, "Export", "primary", w=110, h=28, icon_name="download", disabled=True)
    return s.render("DreamCarve phase 4 mock-up: export sheet")


def scene_empty_state():
    s = Svg(W, H)
    caption(s, "12 · First run (79): two distinct paths, your drawing or a pictured example; shorter copy; nothing about phases or unsaved copies.")
    y, _ = workspace_frame(s, "Untitled — DreamCarve", arrange_enabled=False)
    bottom = H - 24
    left_w, right_w = 280, 340
    s.rect(0, y, left_w, bottom - y, fill=PANEL)
    yy = panel_title(s, 0, y, left_w, "Design Tree")
    s.text(left_w / 2, yy + 40, "No shapes yet.", size=13, fill=TEXT2, anchor="middle")
    s.line(0, y + 344, left_w, y + 344, stroke=BORDER, sw=4)
    project_settings(s, 0, y + 346, left_w, open_=True, thickness_required=True)
    vx, vw = left_w + 4, W - left_w - right_w - 8
    s.rect(vx, y, vw, bottom - y, fill=VIEW_BG)
    sx, sy = vx + 60, bottom - 50
    s.rect(sx, sy - 360, 360, 360, fill=STOCK_FILL, stroke=AXIS, dash="6 4", sw=1.5)
    rx = W - right_w
    s.rect(rx, y, right_w, bottom - y, fill=PANEL)
    yy = cam_tabs(s, rx, y, right_w, active="Tools", counts=(0, 0))
    s.text(rx + right_w / 2, yy + 46, "No tools yet.", size=13, fill=TEXT2, anchor="middle")
    button(s, rx + 52, yy + 62, "New tool", icon_name="plus")
    button(s, rx + 160, yy + 62, "Add from library…")
    # The card.
    cw, ch = 640, 420
    cx, cy = vx + (vw - cw) / 2, y + 60
    if cx < vx + 8:
        cx = vx + 8
    s.rect(cx, cy, cw, ch, fill=RAISED, stroke=BORDER, rx=8, shadow=True)
    icon(s, "x", cx + cw - 36, cy + 18, TEXT2)
    s.text(cx + 24, cy + 42, "Start a project", size=18, weight=600)
    s.text(cx + 24, cy + 66, "Bring in a drawing to make toolpaths from, or explore an example.", size=13, fill=TEXT2)
    colw = (cw - 48 - 16) / 2
    # Your drawing well.
    wx, wy = cx + 24, cy + 90
    s.rect(wx, wy, colw, ch - 114, fill=PANEL, rx=6)
    s.text(wx + 16, wy + 28, "Your drawing", size=15, weight=600)
    button(s, wx + 16, wy + 44, "Import a drawing…", "primary", w=colw - 32, h=40, icon_name="file-input")
    s.text(wx + 16, wy + 102, "SVG or DXF; the stock is sized round it.", size=12, fill=TEXT2)
    button(s, wx + 16, wy + 122, "Import a picture…", w=colw - 32, h=32, icon_name="image-plus")
    button(s, wx + 16, wy + 162, "Open a project…", w=colw - 32, h=32, icon_name="folder-open", shortcut="Ctrl O")
    # Examples well.
    ex = wx + colw + 16
    s.rect(ex, wy, colw, ch - 114, fill=PANEL, rx=6)
    s.text(ex + 16, wy + 28, "Examples", size=15, weight=600)
    for i, (name, line) in enumerate((("Coaster", "Profile with tabs"), ("Plaque", "Pockets and holes"), ("Sign", "V-carved lettering"))):
        ey = wy + 44 + i * 80
        s.rect(ex + 16, ey, colw - 32, 72, fill=RAISED, stroke=BORDER, rx=6)
        tx, ty = ex + 24, ey + 8
        s.rect(tx, ty, 96, 56, fill=STOCK_FILL, stroke=BORDER, rx=3)
        if name == "Coaster":
            s.circle(tx + 48, ty + 28, 22, stroke=SHAPE)
        elif name == "Plaque":
            s.rect(tx + 8, ty + 8, 80, 40, fill="none", stroke=SHAPE, rx=4)
            s.rect(tx + 20, ty + 16, 34, 24, fill="none", stroke=SHAPE)
            s.circle(tx + 37, ty + 28, 6, stroke=SHAPE)
            for hx, hy in ((tx + 13, ty + 13), (tx + 83, ty + 13), (tx + 13, ty + 43), (tx + 83, ty + 43)):
                s.circle(hx, hy, 2, stroke=SHAPE)
        else:
            s.rect(tx + 6, ty + 10, 84, 36, fill="none", stroke=SHAPE, rx=3)
            s.add(f'<ellipse cx="{tx + 48}" cy="{ty + 28}" rx="34" ry="13" fill="none" stroke="{SHAPE}"/>')
            s.text(tx + 48, ty + 33, "Sign", size=13, weight=600, fill=SHAPE, anchor="middle")
        s.text(tx + 108, ty + 24, name, size=13, weight=500)
        s.text(tx + 108, ty + 42, line, size=12, fill=TEXT2)
    statusbar(s, bottom, [("units", "mm"), ("counts", [("error", "1")])])
    return s.render("DreamCarve phase 4 mock-up: empty state")


def scene_export_result():
    s = Svg(1240, 900)
    caption(s, "13 · Export result (77a): Status, Files, Run instructions (closed); Save setup sheet… Right: the setup sheet (77b), an HTML page printed from the browser.")
    x, w, h = 40, 560, 600
    yy = sheet(s, x, 50, w, h, "Export G-code")
    fx, fw = x + 24, w - 48
    icon(s, "circle-check", fx, yy - 2, SUCCESS, size=20)
    s.text(fx + 28, yy + 14, "3 programs written", size=15, weight=600)
    yy += 36
    s.text(fx, yy + 12, "Files", size=13, weight=600)
    yy += 22
    s.rect(fx, yy, fw, 28, fill=PANEL)
    for cx, lab in ((fx + 8, "#"), (fx + 32, "File"), (fx + 220, "Tool"), (fx + fw - 8, "Time")):
        s.text(cx, yy + 18, lab, size=12, fill=TEXT2, anchor="end" if lab == "Time" else "start")
    yy += 28
    for i, (fn, tool, t) in enumerate((("plaque-1-T1.nc", "T1 6.35 mm end mill", "9 min 40 s"), ("plaque-2-T2.nc", "T2 3.175 mm end mill", "2 min 05 s"), ("plaque-3-T3.nc", "T3 90° V-bit", "1 min 50 s"))):
        s.rect(fx, yy, fw, 28, fill=STRIPE if i % 2 else RAISED)
        s.text(fx + 8, yy + 18, str(i + 1), size=13, fill=TEXT2)
        s.text(fx + 32, yy + 18, fn, size=12, mono=True)
        s.text(fx + 220, yy + 18, tool, size=13)
        s.text(fx + fw - 8, yy + 18, t, size=13, anchor="end")
        yy += 28
    yy += 12
    s.text(fx, yy + 12, "Folder", size=13, weight=600)
    yy += 20
    s.text(fx, yy + 14, "/Users/sam/cnc", size=12, mono=True)
    button(s, fx + fw - 186, yy - 2, "Show in folder", icon_name="folder-open", w=150)
    button(s, fx + fw - 28, yy - 2, "", "quiet", w=26, icon_name="copy")
    yy += 40
    yy = section(s, x, yy, w, "Run instructions", open_=True) + 6
    for i, st in enumerate(("Fit T1 6.35 mm end mill; set work zero X, Y and Z.", "Change to T2 3.175 mm end mill; set Z zero, keep X and Y.", "Change to T3 90° V-bit; set Z zero, keep X and Y.")):
        s.text(fx, yy + 14, f"{i + 1}.", size=12, fill=TEXT2)
        s.text(fx + 18, yy + 14, f"Before {['program 1', 'program 2', 'program 3'][i]}: {st}", size=12)
        yy += 20
    for ln in ("This program does not change the tool length offset (G43.1).", "If you used G51 on this controller, soft-reset it first."):
        s.rect(fx, yy + 4, fw, 28, fill=STRIPE, rx=6)
        icon(s, "info", fx + 8, yy + 10, TEXT2, size=14)
        s.text(fx + 28, yy + 22, ln, size=12, fill=TEXT2)
        yy += 34
    s.text(fx, yy + 16, "Extents (tool centre) X 6.8…153.2  Y 6.8…93.2 · Z −12.2…+10", size=12, fill=TEXT2)
    fy = 50 + h - 56
    s.line(x, fy, x + w, fy, stroke=BORDER)
    button(s, x + 24, fy + 14, "Save setup sheet…", icon_name="file-text", h=28)
    button(s, x + w - 24 - 90, fy + 14, "Done", "primary", w=90, h=28)
    s.text(x, 690, "Run instructions shown open here; the sheet opens with them closed.", size=12, fill=TEXT2, italic=True)

    # The setup sheet as printed (HTML, black on white; experience.md 15).
    px, py, pw, ph = 660, 50, 540, 820
    s.rect(px, py, pw, ph, fill="#FFFFFF", stroke=BORDER, shadow=True)
    tx = px + 40
    ty = py + 56
    s.text(tx, ty, "plaque — setup sheet", size=20, weight=700, fill="#000")
    s.text(tx, ty + 22, "Exported 2026-11-10 · grblHAL v4 · units mm", size=12, fill="#000")
    ty += 50
    for k, v in (("Stock", "160 × 100 × 12 mm"), ("XY zero", "Bottom-left corner"), ("Z zero", "Stock top"), ("Clearance", "10 mm")):
        s.text(tx, ty, k, size=12, weight=600, fill="#000")
        s.text(tx + 110, ty, v, size=12, fill="#000")
        ty += 18
    ty += 10
    for i, (fn, tool, meta, ops, before, t, ext) in enumerate((
        ("plaque-1-T1.nc", "T1 · 6.35 mm end mill", "IDC Woodcraft DC-14 · 2 flutes", "Panel pocket, Outside cut", "Fit T1 and set work zero X, Y and Z.", "9 min 40 s", "X 6.8…153.2 · Y 6.8…93.2 · Z −12.2…+10"),
        ("plaque-2-T2.nc", "T2 · 3.175 mm end mill", "IDC Woodcraft UC-18 · 2 flutes", "Screw holes, Flourish", "Change to T2, set Z zero. Keep X and Y.", "2 min 05 s", "X 18.4…141.6 · Y 18.4…81.6 · Z −12.2…+10"),
        ("plaque-3-T3.nc", "T3 · 90° V-bit 6.35 mm", "Amana 45702", "Lettering", "Change to T3, set Z zero. Keep X and Y.", "1 min 50 s", "X 40.1…120.0 · Y 40.0…60.2 · Z −2.5…+10"))):
        s.line(tx, ty, px + pw - 40, ty, stroke="#000", sw=0.6)
        ty += 20
        s.text(tx, ty, f"{i + 1}.  {fn}", size=13, weight=700, fill="#000", mono=True)
        s.text(px + pw - 40, ty, t, size=12, fill="#000", anchor="end")
        ty += 20
        for k, v in (("Tool", f"{tool} · {meta}"), ("Operations", ops), ("Before", before), ("Extents", ext)):
            s.text(tx + 20, ty, k, size=12, weight=600, fill="#000")
            s.text(tx + 110, ty, v, size=12, fill="#000")
            ty += 18
        ty += 8
    s.line(tx, ty, px + pw - 40, ty, stroke="#000", sw=0.6)
    ty += 22
    s.text(tx, ty, "Notes", size=13, weight=700, fill="#000")
    ty += 20
    for ln in ("This program does not change the tool length offset (G43.1).", "Warning tool_change_avoidable: T2 is used by two non-adjacent operations."):
        s.text(tx, ty, "•  " + ln, size=12, fill="#000")
        ty += 18
    s.text(tx, py + ph - 24, "plaque · 2026-11-10 · DreamCarve 0.4.0", size=10, fill="#000")
    return s.render("DreamCarve phase 4 mock-up: export result and setup sheet")


def menu(s: Svg, x, y, items, w=260):
    h = sum(9 if it == "|" else 28 for it in items) + 8
    s.rect(x, y, w, h, fill=RAISED, stroke=BORDER, rx=6, shadow=True)
    yy = y + 4
    for it in items:
        if it == "|":
            s.line(x + 8, yy + 4, x + w - 8, yy + 4, stroke=BORDER)
            yy += 9
            continue
        lab, sc, flags = (it + (None, None))[:3] if isinstance(it, tuple) else (it, None, None)
        if flags == "hover":
            s.rect(x + 4, yy, w - 8, 28, fill=HOVER, rx=4)
        if flags == "checked":
            icon(s, "check", x + 10, yy + 6, TEXT)
        s.text(x + 32, yy + 18, lab, size=13, fill=DIS if flags == "disabled" else TEXT)
        if sc == ">":
            icon(s, "chevron-right", x + w - 26, yy + 6, TEXT2)
        elif sc:
            s.text(x + w - 12, yy + 18, sc, size=12, fill=TEXT2, anchor="end")
        yy += 28
    return h


def scene_menus():
    s = Svg(1280, 760)
    caption(s, "14 · Menus (79a) and the unsaved-changes sheet (51): new-project and add-to-project apart; Appearance a three-way choice; Save, Don't Save, Cancel.")
    y, pos = menubar(s, 30, open_menu="File")
    menu(s, pos["File"], y + 2, [
        ("New", "Ctrl N"), ("New Project from Drawing…", None), ("New Project from Picture…", None), ("Open…", "Ctrl O"), ("Open Recent", ">"), "|",
        ("Save", "Ctrl S"), ("Save As…", "Shift Ctrl S"), "|",
        ("Add Drawing to Project…", "Ctrl I"), ("Add Picture to Project…", None), "|",
        ("Export G-code…", "Ctrl E"), ("Save Setup Sheet…", None, "disabled"), "|", ("Quit", "Ctrl Q")], w=300)
    ax = 340
    s.text(ax, 92, "Arrange", size=13, weight=600)
    menu(s, ax, 100, [("Rotate 90° Clockwise", "R"), ("Rotate 90° Anticlockwise", "Shift R"), ("Rotate…", None), "|",
                      ("Flip Horizontal", "Shift Ctrl H"), ("Flip Vertical", "Shift Ctrl V"), "|", ("Centre on Stock", None)], w=290)
    vx = 660
    s.text(vx, 92, "View", size=13, weight=600)
    menu(s, vx, 100, [("Design Tree and Settings", None, "checked"), ("CAM Panel", None, "checked"), "|", ("Toolpaths…", None), ("Warnings", None), "|",
                      ("Fit View", "F"), ("Zoom to Selection", "Shift F"), "|", ("Appearance", ">", "hover")], w=260)
    menu(s, vx + 256, 100 + 8 * 28 + 3 * 9 - 26, [("System", None, "checked"), ("Light", None), ("Dark", None)], w=150)
    # The unsaved-changes sheet.
    ux, uy, uw, uh = 760, 520, 420, 150
    s.rect(ux, uy, uw, uh, fill=RAISED, stroke=BORDER, rx=8, shadow=True)
    s.text(ux + 24, uy + 38, "Save changes to “plaque”?", size=18, weight=600)
    s.text(ux + 24, uy + 64, "Your changes are lost if you don't save them.", size=13, fill=TEXT2)
    button(s, ux + 24, uy + uh - 50, "Don't Save", "destructive", w=104, h=28)
    button(s, ux + uw - 24 - 76 - 8 - 84, uy + uh - 50, "Cancel", w=84, h=28)
    button(s, ux + uw - 24 - 76, uy + uh - 50, "Save", "primary", w=76, h=28)
    note(s, 40, 560, 660, ["Shown on close, quit, Open and every New when there are unsaved changes.",
                            "Enter is Save, Escape is Cancel. Save on an untitled project asks where first.",
                            "No autosave in phase 4 (maintainer, 2026-11-10).",
                            "Save As… is the standard save-a-copy; Export G-code… writes programs."])
    return s.render("DreamCarve phase 4 mock-up: menus and the unsaved-changes sheet")


def scene_targets():
    s = Svg(W, 760)
    caption(s, "15 · Targets (67): left, the Panel pocket selected, targets marked in tree and Viewport; right, target editing, clicking shapes to add or remove.")
    # Left frame: highlight.
    design_tree(s, 0, 30, 260, 360, targets=["panel", "island"], target_col=PALETTE[1])
    viewport(s, 264, 30, 360, 360, scale=2.0, targets_of=1, ox=20, oy=60)
    s.rect(0, 400, 624, 340, fill=PANEL)
    yy = editor_header(s, 0, 400, 624, "Pocket", "Panel pocket", PALETTE[1])
    yy = section(s, 0, yy, 624, "Setup", chevron=False) + 4
    yy = targets_row(s, 12, yy, 600, "2 shapes · panel, island")
    menu(s, 624 - 220, yy + 2, [("Use selection", None), ("Add selection", None), "|", ("Edit targets…", None)], w=200)
    # Right frame: editing mode.
    rx = 640
    design_tree(s, rx, 30, 260, 360, targets=["panel", "hole-3"], target_col=PALETTE[1], edit_mode=True, edit_set=("panel", "hole-3"))
    vx, vw = rx + 264, W - rx - 264
    viewport(s, vx, 30, vw, 360, scale=2.1, targets_of=1, ox=20, oy=40, target_names=["panel", "hole-3"])
    s.rect(vx, 30, vw, 32, fill=ACCENT_SOFT)
    s.line(vx, 62, vx + vw, 62, stroke=ACCENT)
    icon(s, "target", vx + 10, 38, ACCENT)
    s.text(vx + 32, 51, "Editing targets of “Panel pocket”", size=13)
    button(s, vx + vw - 150, 33, "Cancel", h=26)
    button(s, vx + vw - 74, 33, "Done", "primary", w=64, h=26)
    note(s, rx, 410, 620, ["Click a shape (or a tree row) to add or remove it; marquee adds, Alt-marquee removes.",
                            "Tree rows show checkboxes; nothing else can be selected or moved.",
                            "Enter is Done (one undo step: Set targets of “Panel pocket”), Escape is Cancel.",
                            "The highlight follows each click. Non-targets fade to 40 %."], title="Target editing")
    note(s, rx, 540, 620, ["The Setup row says how many and which, with a warning glyph when a target",
                            "carries one. Pencil enters target editing; ⋯ holds Use selection / Add selection.",
                            "The tree's context menu has Add to “Panel pocket” / Remove from “Panel pocket”."], title="Setup row")
    return s.render("DreamCarve phase 4 mock-up: targets")


def scene_tools_tab():
    s = Svg(760, 780)
    caption(s, "16 · Tools tab (63): New tool makes tool4 with init's geometry and no rates; rates show Required, the message in the tooltip.")
    x, w = 20, 340
    s.rect(x, 40, w, 720, fill=PANEL, stroke=BORDER)
    yy = cam_tabs(s, x, 40, w, active="Tools", counts=(4, 5))
    s.rect(x, yy, w, 40, fill=PANEL)
    bx = button(s, x + 12, yy + 7, "New tool", icon_name="plus") + 6
    bx = button(s, bx, yy + 7, "Add from library…") + 6
    yy += 40
    rows = [("6.35 mm end mill", "6.35 mm · T1", False), ("3.175 mm end mill", "3.175 mm · T2", False), ("90° V-bit", "6.35 mm · 90° · T3", False), ("tool4", "6.35 mm · T4", True)]
    for i, (n, sub, sel) in enumerate(rows):
        if sel:
            s.rect(x, yy, w, 40, fill=ACCENT_SOFT)
            s.rect(x, yy, 2, 40, fill=ACCENT)
        icon(s, "triangle-down" if "V-bit" in n else "cylinder", x + 12, yy + 12, TEXT2)
        s.text(x + 36, yy + 17, n, size=13, weight=500 if sel else 400)
        s.text(x + 36, yy + 33, sub, size=12, fill=TEXT2)
        if sel:
            s.circle(x + w - 20, yy + 20, 4, fill=ERROR, stroke="none")
        yy += 40
    s.line(x, yy, x + w, yy, stroke=BORDER)
    bx = button(s, x + 12, yy + 7, "Duplicate", icon_name="copy") + 6
    button(s, bx, yy + 7, "Delete", icon_name="trash")
    yy += 42
    fx, fw = x + 12, w - 24
    s.rect(fx + LABEL_W, yy, fw - LABEL_W, 26, fill=SUNKEN, stroke=ACCENT, rx=4)
    s.rect(fx + LABEL_W + 6, yy + 5, 40, 16, fill=ACCENT_SOFT)
    s.text(fx, yy + 17, "Name", size=13, fill=TEXT2)
    s.text(fx + LABEL_W + 8, yy + 17, "tool4", size=13)
    yy += 30
    yy = dropdown(s, fx, yy, fw, "Type", "End mill")
    yy = field(s, fx, yy, fw, "Diameter", "6.35", "mm")
    yy = field(s, fx, yy, fw, "Flutes", "2")
    yy = field(s, fx, yy, fw, "Feed", "", "mm/min", state="required")
    yy = field(s, fx, yy, fw, "Plunge rate", "", "mm/min", state="required")
    yy = field(s, fx, yy, fw, "RPM", "", "rpm", state="required")
    yy = field(s, fx, yy, fw, "Step-down", "", "mm")
    yy = field(s, fx, yy, fw, "Stepover", "", "%")
    yy = field(s, fx, yy, fw, "Tool number", "", "")
    yy = field(s, fx, yy, fw, "Vendor", "")
    yy = field(s, fx, yy, fw, "Product id", "")
    button(s, fx, yy + 4, "Save to library")
    # Tooltip on Feed.
    tx, ty = 380, 360
    s.rect(tx, ty, 340, 52, fill="#262B33", rx=6)
    s.text(tx + 12, ty + 22, "tools[t4].feed_mm_min is not set (mm/min)", size=12, fill="#E8EBF0")
    s.text(tx + 12, ty + 40, "feed_mm_min", size=12, fill="#A3ABB7", mono=True)
    note(s, 380, 430, 340, ["Name focused and selected: type to rename.", "Diameter is init's placeholder (the file", "needs one); feeds are never invented.", "Delete is refused while an operation uses", "the tool; its tooltip names them."], title="New tool")
    note(s, 380, 560, 340, ["Empty project: “No tools yet.” with", "New tool and Add from library… under it."], title="Empty list")
    return s.render("DreamCarve phase 4 mock-up: tools tab")


# ── Helpers ─────────────────────────────────────────────────────────────


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
    "13-export-result.svg": scene_export_result,
    "14-menus-and-sheets.svg": scene_menus,
    "15-targets.svg": scene_targets,
    "16-tools-tab.svg": scene_tools_tab,
}


def main() -> None:
    here = os.path.dirname(os.path.abspath(__file__))
    for name, fn in SCENES.items():
        with open(os.path.join(here, name), "w", encoding="utf-8") as f:
            f.write(fn())
        print(name)


if __name__ == "__main__":
    main()
