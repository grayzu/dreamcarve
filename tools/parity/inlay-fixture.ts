// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

/**
 * Write the inlay test part, `tests/fixtures/dc-inlay.svg` (#446).
 *
 *   npx tsx inlay-fixture.ts --purecutcnc ../../../purecutcnc
 *
 * The phase 3 functional spec's "Lettered inlay pair" test part: the
 * monogram "DC" in Droid Serif Bold, 40 mm cap height (the "D" is 992 font
 * units tall), the two letters 3 mm apart at their nearest, centred in a
 * 90 × 60 mm `frame` that nothing cuts. `init` makes the stock the drawing
 * plus 10 mm all round, so the frame gives the 110 × 80 mm board of the
 * test part with the monogram at its centre.
 *
 * The lettering is Droid Serif Bold, from three.js's typeface file
 * (`examples/fonts/droid/droid_serif_bold.typeface.json`, Apache-2.0), read
 * from the three.js that PureCutCNC's checkout installs, the copy
 * `sign-fixture.ts` reads. Only the glyph outlines are used: each glyph's
 * quadratic curves are written as SVG `Q` segments, scaled and moved, which
 * maps every curve exactly. The letters' extents are the curves' own
 * extremes, not their control points.
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
const outPath = join(repoRoot, 'tests/fixtures/dc-inlay.svg')

/** The monogram, its cap height and the gap between its letters, mm. */
const WORD = 'DC'
const CAP_HEIGHT_MM = 40
const GAP_MM = 3

/** The frame, mm: init's stock is this plus 10 mm all round, 110 × 80. */
const PAGE = { width: 90, height: 60 }

interface Glyph { ha: number; o?: string }
interface Typeface {
  familyName: string
  cssFontWeight: string
  glyphs: Record<string, Glyph>
}

type Pt = [number, number]
/** One contour: a start point and its segments, each a line or a quadratic curve. */
interface Contour { start: Pt; segments: Array<{ to: Pt; control?: Pt }> }
interface Box { minX: number; minY: number; maxX: number; maxY: number }

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

/**
 * The exact bounding box of contours: each segment's end points, and a
 * quadratic's extreme in X or Y where its derivative is zero inside the
 * segment.
 */
function bounds(cs: Contour[]): Box {
  const box: Box = { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity }
  const add = (p: Pt): void => {
    box.minX = Math.min(box.minX, p[0])
    box.minY = Math.min(box.minY, p[1])
    box.maxX = Math.max(box.maxX, p[0])
    box.maxY = Math.max(box.maxY, p[1])
  }
  for (const c of cs) {
    let from = c.start
    add(from)
    for (const s of c.segments) {
      add(s.to)
      if (s.control) {
        for (const axis of [0, 1]) {
          const denom = from[axis] - 2 * s.control[axis] + s.to[axis]
          if (denom === 0) continue
          const t = (from[axis] - s.control[axis]) / denom
          if (t <= 0 || t >= 1) continue
          const u = 1 - t
          add([
            u * u * from[0] + 2 * u * t * s.control[0] + t * t * s.to[0],
            u * u * from[1] + 2 * u * t * s.control[1] + t * t * s.to[1],
          ])
        }
      }
      from = s.to
    }
  }
  return box
}

const font = JSON.parse(readFileSync(fontPath, 'utf8')) as Typeface
const glyphs = [...WORD].map((ch) => {
  const g = font.glyphs[ch]
  if (!g || !g.o) throw new Error(`the font has no outline for \`${ch}\``)
  const cs = contours(g.o)
  return { ch, contours: cs, box: bounds(cs) }
})
const capUnits = glyphs[0].box.maxY
const k = CAP_HEIGHT_MM / capUnits

/** Moves font-unit contours by `dx` units and scales them to mm, Y-up. */
function scaled(cs: Contour[], dx: number): Contour[] {
  const at = (p: Pt): Pt => [(p[0] + dx) * k, p[1] * k]
  return cs.map((c) => ({
    start: at(c.start),
    segments: c.segments.map((s) => ({ to: at(s.to), control: s.control ? at(s.control) : undefined })),
  }))
}

// Set each letter GAP_MM after the last at their nearest X extents (both
// letters' sides are upright near the gap, so that is their nearest).
const set: Array<{ ch: string; contours: Contour[] }> = []
let right = -GAP_MM
for (const g of glyphs) {
  const placed = scaled(g.contours, (right + GAP_MM) / k - g.box.minX)
  right = bounds(placed).maxX
  set.push({ ch: g.ch, contours: placed })
}

// Centre the monogram's exact extent on the page, and flip to SVG's Y-down.
const all = bounds(set.flatMap((g) => g.contours))
const dx = PAGE.width / 2 - (all.minX + all.maxX) / 2
const dy = PAGE.height / 2 + (all.minY + all.maxY) / 2
const page = (p: Pt): Pt => [p[0] + dx, dy - p[1]]

const f = (x: number): string => {
  const s = x.toFixed(3).replace(/\.?0+$/, '')
  return s === '-0' ? '0' : s
}
const pt = (p: Pt): string => `${f(p[0])},${f(p[1])}`

function pathData(cs: Contour[]): string {
  return cs
    .map((c) => {
      const parts = [`M ${pt(page(c.start))}`]
      for (const s of c.segments) {
        parts.push(s.control ? `Q ${pt(page(s.control))} ${pt(page(s.to))}` : `L ${pt(page(s.to))}`)
      }
      return `${parts.join(' ')} Z`
    })
    .join(' ')
}

const letters = set.map((g) => `    <path id="letter-${g.ch}" d="${pathData(g.contours)}"/>`)
const width = all.maxX - all.minX
const height = all.maxY - all.minY

const svg = `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3 test part (docs/specs/phase-3/functional.md,
     "Lettered inlay pair"): the monogram "DC" for a V-bit inlay. Written by
     tools/parity/inlay-fixture.ts; do not edit by hand, re-run the tool.
     Droid Serif Bold (Apache-2.0, The Android Open Source Project;
     tests/fixtures/README.md), ${CAP_HEIGHT_MM} mm cap height, converted to outlines, the
     letters ${GAP_MM} mm apart: ${f(width)} × ${f(height)} mm, centred on the page. The ${PAGE.width} × ${PAGE.height} mm
     rectangle "frame" is not cut: it makes init's stock ${PAGE.width + 20} × ${PAGE.height + 20} mm with
     the monogram at its centre. SVG is Y-down. -->
<svg xmlns="http://www.w3.org/2000/svg" width="${PAGE.width}mm" height="${PAGE.height}mm" viewBox="0 0 ${PAGE.width} ${PAGE.height}">
  <g fill="none" stroke="#000000" stroke-width="0.2">
${letters.join('\n')}
    <rect id="frame" x="0" y="0" width="${PAGE.width}" height="${PAGE.height}"/>
  </g>
</svg>
`

writeFileSync(outPath, svg)
console.log(`wrote ${outPath}: ${font.familyName} ${font.cssFontWeight} "${WORD}", cap height ${CAP_HEIGHT_MM} mm (${capUnits} units), ${f(width)} × ${f(height)} mm, in a ${PAGE.width} × ${PAGE.height} mm frame`)
