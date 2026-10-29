// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

/**
 * Write the sign test part, `tests/fixtures/sign.svg` (#411).
 *
 *   npx tsx sign-fixture.ts --purecutcnc ../../../purecutcnc
 *
 * The phase 3 functional spec's "Sign" test part: a 200 × 80 mm outline; a
 * 190 × 70 mm background rectangle 5 mm inside it; a 150 × 46 mm oval
 * centred in it; the word "DreamCarve" as outline paths inside the oval;
 * and two 3.175 mm hanging holes in the background's top corners.
 *
 * The lettering is Droid Serif Bold, from three.js's typeface file
 * (`examples/fonts/droid/droid_serif_bold.typeface.json`, Apache-2.0), read
 * from the three.js that PureCutCNC's checkout installs, the same copy the
 * V-Carve text fixtures came from (`vcarve-fixtures.ts`). Only the glyph
 * outlines are used: each glyph's quadratic curves are written as SVG `Q`
 * segments, moved onto a gentle arc by one rotation and translation per
 * glyph, which maps every curve exactly. Cap height 14 mm (the "D" is 992
 * font units tall), set on its advance widths with no kerning, the
 * baseline an arc of radius 500 mm bulging upward, centred in the oval.
 *
 * Dev-only tool, not part of the Cargo workspace; it reads no PureCutCNC
 * code. Run it twice: the output must be byte-identical.
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '..', '..')

function arg(name: string, fallback: string): string {
  const i = process.argv.indexOf(`--${name}`)
  if (i !== -1 && process.argv[i + 1]) return process.argv[i + 1]
  return fallback
}

const checkout = resolve(arg('purecutcnc', join(repoRoot, '..', 'purecutcnc')))
const fontPath = resolve(
  arg('font', join(checkout, 'node_modules/three/examples/fonts/droid/droid_serif_bold.typeface.json')),
)
const outPath = join(repoRoot, 'tests/fixtures/sign.svg')

/** The word, its cap height and the baseline arc's radius, mm. */
const WORD = 'DreamCarve'
const CAP_HEIGHT_MM = 14
const ARC_RADIUS_MM = 500

/** The sign, mm, in SVG's Y-down page frame. */
const PAGE = { width: 200, height: 80 }
const BACKGROUND = { x: 5, y: 5, width: 190, height: 70 }
const OVAL = { cx: 100, cy: 40, rx: 75, ry: 23 }
const HOLE_RADIUS_MM = 3.175 / 2
const HOLES = [
  { id: 'hole-left', cx: 15, cy: 15 },
  { id: 'hole-right', cx: 185, cy: 15 },
]

interface Glyph { ha: number; o?: string }
interface Typeface {
  familyName: string
  cssFontWeight: string
  glyphs: Record<string, Glyph>
}

type Pt = [number, number]
/** One contour: a start point and its segments, each a line or a quadratic curve. */
interface Contour { start: Pt; segments: Array<{ to: Pt; control?: Pt }> }

/** The contours of a glyph's outline string, in font units, Y-up. */
function contours(outline: string): Contour[] {
  const t = outline.trim().split(/\s+/)
  const out: Contour[] = []
  let i = 0
  const num = (): number => Number(t[i++])
  while (i < t.length) {
    const cmd = t[i++]
    if (cmd === 'm') {
      out.push({ start: [num(), num()], segments: [] })
    } else if (cmd === 'l') {
      out[out.length - 1].segments.push({ to: [num(), num()] })
    } else if (cmd === 'q') {
      // three.js order: the end point, then the control point.
      const to: Pt = [num(), num()]
      const control: Pt = [num(), num()]
      out[out.length - 1].segments.push({ to, control })
    } else if (cmd === 'z') {
      // Contours close implicitly.
    } else {
      throw new Error(`unsupported outline command \`${cmd}\``)
    }
  }
  for (const c of out) {
    const last = c.segments[c.segments.length - 1]
    if (last && !last.control && last.to[0] === c.start[0] && last.to[1] === c.start[1]) {
      c.segments.pop()
    }
  }
  return out.filter((c) => c.segments.length > 0)
}

const font = JSON.parse(readFileSync(fontPath, 'utf8')) as Typeface
const capUnits = (() => {
  let top = -Infinity
  for (const c of contours(font.glyphs.D.o ?? '')) {
    for (const p of [c.start, ...c.segments.flatMap((s) => (s.control ? [s.control, s.to] : [s.to]))]) {
      top = Math.max(top, p[1])
    }
  }
  return top
})()
const k = CAP_HEIGHT_MM / capUnits

const glyphs = [...WORD].map((ch) => {
  const g = font.glyphs[ch]
  if (!g || !g.o) throw new Error(`the font has no outline for \`${ch}\``)
  return { ch, ha: g.ha, contours: contours(g.o) }
})
const totalUnits = glyphs.reduce((sum, g) => sum + g.ha, 0)

/**
 * Places a font-unit point of the glyph whose advance starts `pen` units
 * along the baseline: arc length from the word's middle to the glyph's
 * middle sets the angle; the glyph is turned to the arc's tangent there.
 * Returns page coordinates (Y-down) before the vertical centring.
 */
function place(pen: number, ha: number, p: Pt, baselineY: number): Pt {
  const s = (pen + ha / 2 - totalUnits / 2) * k
  const theta = s / ARC_RADIUS_MM
  const centre: Pt = [OVAL.cx, baselineY + ARC_RADIUS_MM]
  const base: Pt = [centre[0] + ARC_RADIUS_MM * Math.sin(theta), centre[1] - ARC_RADIUS_MM * Math.cos(theta)]
  const tangent: Pt = [Math.cos(theta), Math.sin(theta)]
  const up: Pt = [Math.sin(theta), -Math.cos(theta)]
  const u = (p[0] - ha / 2) * k
  const v = p[1] * k
  return [base[0] + u * tangent[0] + v * up[0], base[1] + u * tangent[1] + v * up[1]]
}

function layout(baselineY: number): Array<{ ch: string; contours: Contour[] }> {
  let pen = 0
  return glyphs.map((g) => {
    const at = (p: Pt): Pt => place(pen, g.ha, p, baselineY)
    const placed = g.contours.map((c) => ({
      start: at(c.start),
      segments: c.segments.map((s) => ({ to: at(s.to), control: s.control ? at(s.control) : undefined })),
    }))
    pen += g.ha
    return { ch: g.ch, contours: placed }
  })
}

// Centre the word's extent (its points and control points) on the oval.
const trial = layout(0)
let minY = Infinity
let maxY = -Infinity
for (const g of trial) {
  for (const c of g.contours) {
    for (const p of [c.start, ...c.segments.map((s) => s.to)]) {
      minY = Math.min(minY, p[1])
      maxY = Math.max(maxY, p[1])
    }
  }
}
const word = layout(OVAL.cy - (minY + maxY) / 2)

const f = (x: number): string => {
  const s = x.toFixed(3).replace(/\.?0+$/, '')
  return s === '-0' ? '0' : s
}
const pt = (p: Pt): string => `${f(p[0])},${f(p[1])}`

function pathData(cs: Contour[]): string {
  return cs
    .map((c) => {
      const parts = [`M ${pt(c.start)}`]
      for (const s of c.segments) parts.push(s.control ? `Q ${pt(s.control)} ${pt(s.to)}` : `L ${pt(s.to)}`)
      return `${parts.join(' ')} Z`
    })
    .join(' ')
}

const counts = new Map<string, number>()
const letters = word.map((g) => {
  const n = (counts.get(g.ch) ?? 0) + 1
  counts.set(g.ch, n)
  return `    <path id="letter-${g.ch}${n > 1 ? `-${n}` : ''}" d="${pathData(g.contours)}"/>`
})

const svg = `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3 test part (docs/specs/phase-3/functional.md, "Sign"):
     a 200 × 80 mm sign. Written by tools/parity/sign-fixture.ts; do not edit
     by hand, re-run the tool. A 190 × 70 mm background 5 mm inside the
     outline, V-carved round a raised 150 × 46 mm oval; two 3.175 mm hanging
     holes in the background's top corners; and "DreamCarve" inside the oval,
     V-carved inset: Droid Serif Bold (Apache-2.0, The Android Open Source
     Project; tests/fixtures/README.md), ${CAP_HEIGHT_MM} mm cap height, converted to
     outlines and set on an arc of radius ${ARC_RADIUS_MM} mm. SVG is Y-down. -->
<svg xmlns="http://www.w3.org/2000/svg" width="${PAGE.width}mm" height="${PAGE.height}mm" viewBox="0 0 ${PAGE.width} ${PAGE.height}">
  <g fill="none" stroke="#000000" stroke-width="0.2">
    <rect id="outline" x="0" y="0" width="${PAGE.width}" height="${PAGE.height}"/>
    <rect id="background" x="${BACKGROUND.x}" y="${BACKGROUND.y}" width="${BACKGROUND.width}" height="${BACKGROUND.height}"/>
    <ellipse id="oval" cx="${OVAL.cx}" cy="${OVAL.cy}" rx="${OVAL.rx}" ry="${OVAL.ry}"/>
${HOLES.map((h) => `    <circle id="${h.id}" cx="${h.cx}" cy="${h.cy}" r="${HOLE_RADIUS_MM}"/>`).join('\n')}
${letters.join('\n')}
  </g>
</svg>
`

writeFileSync(outPath, svg)
console.log(`wrote ${outPath}: ${font.familyName} ${font.cssFontWeight}, ${WORD.length} glyphs, cap height ${CAP_HEIGHT_MM} mm (${capUnits} units), word ${f(totalUnits * k)} mm along the arc`)
