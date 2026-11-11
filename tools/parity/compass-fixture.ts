// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

/**
 * Write the line-art inlay test part's drawings, `tests/fixtures/prep/
 * compass.svg` and `tests/fixtures/prep/compass-regions.svg` (#605).
 *
 *   node compass-fixture.ts
 *
 * The phase 3 functional spec's "Line-art inlay" test part: a 70 mm tall
 * compass in line work, an outer ring, a cross of four straight lines and
 * four diagonals to the ring, every stroke 2.5 mm wide, so the board ridges
 * between the pockets are 2.5 mm; and an oval, 96 × 84 mm, drawn round the
 * ring as a separate shape, which `init` imports as the instance the
 * Inlay names as its `boundary`.
 *
 * Two files, both from the same numbers:
 *
 * - `compass.svg` is the line art: the ring, the eight spokes and the
 *   oval, as strokes. It is what `image-prep --regions` takes.
 * - `compass-regions.svg` is what `image-prep --regions` returns for it,
 *   worked out here from the geometry: the eight pockets between the
 *   strokes, each as one filled polygon (the ring's inner circle written
 *   as 40 chords per pocket, 0.01 mm off the circle at most), and the oval
 *   unchanged. It stands in for the preparation until `image-prep` is
 *   built, and the `cli::compass_inlay_exports_both_boards` test imports it.
 *
 * Synthetic: nothing here is anyone's art. Dev-only tool, not part of the
 * Cargo workspace; it reads no other code and no font. Run it twice: the
 * output must be byte-identical.
 */

import { writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '..', '..')
const dir = join(repoRoot, 'tests/fixtures/prep')

/** The page, mm: the oval's extent. SVG is Y-down. */
const PAGE = { width: 96, height: 84 }
const CENTRE = { x: PAGE.width / 2, y: PAGE.height / 2 }
/** The ring's outer radius (70 mm tall) and every stroke's width, mm. */
const OUTER_RADIUS_MM = 35
const STROKE_MM = 2.5
/** The strokes' centre line on the ring, and the ring's inner radius. */
const RING_RADIUS_MM = OUTER_RADIUS_MM - STROKE_MM / 2
const INNER_RADIUS_MM = OUTER_RADIUS_MM - STROKE_MM
const HALF_MM = STROKE_MM / 2
/** Spokes: the cross and the diagonals, every 45 degrees. */
const SPOKES = 8
/** Chords along each pocket's arc. */
const CHORDS = 40

const f = (x: number): string => {
  const s = x.toFixed(3).replace(/\.?0+$/, '')
  return s === '-0' ? '0' : s
}

type Pt = [number, number]
const at = (r: number, angle: number, across = 0): Pt => [
  CENTRE.x + r * Math.cos(angle) - across * Math.sin(angle),
  CENTRE.y + r * Math.sin(angle) + across * Math.cos(angle),
]
const pt = (p: Pt): string => `${f(p[0])},${f(p[1])}`

/** The pocket between the spokes at `angle` and `angle` + 45 degrees. */
function pocket(k: number): string {
  const step = (2 * Math.PI) / SPOKES
  const a = k * step
  const b = a + step
  // Each edge runs HALF_MM beside its spoke's centre line, towards the
  // pocket; they meet on the bisector.
  const apex = at(HALF_MM / Math.tan(step / 2), a, HALF_MM)
  const reach = Math.sqrt(INNER_RADIUS_MM ** 2 - HALF_MM ** 2)
  const out: Pt[] = [apex, at(reach, a, HALF_MM)]
  const lo = a + Math.asin(HALF_MM / INNER_RADIUS_MM)
  const hi = b - Math.asin(HALF_MM / INNER_RADIUS_MM)
  for (let i = 1; i < CHORDS; i++) {
    out.push(at(INNER_RADIUS_MM, lo + ((hi - lo) * i) / CHORDS, 0))
  }
  out.push(at(reach, b, -HALF_MM))
  return `M ${out.map(pt).join(' L ')} Z`
}

const oval = `<ellipse id="oval" cx="${f(CENTRE.x)}" cy="${f(CENTRE.y)}" rx="${f(PAGE.width / 2)}" ry="${f(PAGE.height / 2)}" fill="none" stroke="#000000" stroke-width="0.2"/>`
const header = (what: string): string => `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3 test part (docs/specs/phase-3/functional.md,
     "Line-art inlay"): ${what} Written by
     tools/parity/compass-fixture.ts; do not edit by hand, re-run the tool.
     Synthetic. The compass is ${2 * OUTER_RADIUS_MM} mm tall; every stroke is ${f(STROKE_MM)} mm wide;
     the oval is ${PAGE.width} × ${PAGE.height} mm round the ring. SVG is Y-down. -->
<svg xmlns="http://www.w3.org/2000/svg" width="${PAGE.width}mm" height="${PAGE.height}mm" viewBox="0 0 ${PAGE.width} ${PAGE.height}">`

const spokes = Array.from({ length: SPOKES }, (_, k) => {
  const angle = (k * 2 * Math.PI) / SPOKES
  return `M ${pt([CENTRE.x, CENTRE.y])} L ${pt(at(RING_RADIUS_MM, angle))}`
}).join(' ')

const lineArt = `${header('the line art, as strokes: a ring, eight spokes from its centre, and the oval.')}
  <g fill="none" stroke="#000000" stroke-width="${f(STROKE_MM)}" stroke-linecap="butt">
    <circle id="ring" cx="${f(CENTRE.x)}" cy="${f(CENTRE.y)}" r="${f(RING_RADIUS_MM)}"/>
    <path id="spokes" d="${spokes}"/>
  </g>
  ${oval}
</svg>
`

const pockets = Array.from({ length: SPOKES }, (_, k) => `    <path id="pocket-${k + 1}" d="${pocket(k)}"/>`)
const regions = `${header('what image-prep returns in regions mode for compass.svg: the eight pockets between the strokes, as filled polygons, and the oval.')}
  <g fill="#000000" stroke="none">
${pockets.join('\n')}
  </g>
  ${oval}
</svg>
`

writeFileSync(join(dir, 'compass.svg'), lineArt)
writeFileSync(join(dir, 'compass-regions.svg'), regions)
console.log(`wrote ${dir}/compass.svg and compass-regions.svg: ${SPOKES} pockets, ${STROKE_MM} mm strokes, ${2 * OUTER_RADIUS_MM} mm tall, oval ${PAGE.width} × ${PAGE.height} mm`)
