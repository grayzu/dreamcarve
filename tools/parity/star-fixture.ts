// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

/**
 * Write the inlay test part, `tests/fixtures/star.svg` (#437).
 *
 *   npx tsx star-fixture.ts
 *
 * The phase 3 functional spec's "Inlay pair" test part: a four-point star
 * 60 mm tip to tip, its tips 30 mm out on the axes and its inner vertices
 * 12 mm out on the diagonals, with a circle of radius 6 mm at its centre.
 * It is the star `cli::inlay_exports_and_passes_safety` and the
 * `dc-cam::inlay` tests build in code.
 *
 * The drawing also has an 80 × 80 mm square, `frame`, round the star. No
 * operation cuts it: `init` makes the stock the drawing plus 10 mm all
 * round, so the frame gives the 100 × 100 mm stock of the test part, with
 * the star at its centre, (50, 50) mm. The male's surround reaches 38 mm
 * from the centre (the 30 mm tips plus the 8 mm margin), so the board
 * keeps a 12 mm border for clamping.
 *
 * Dev-only tool, not part of the Cargo workspace; it reads nothing. Run it
 * twice: the output must be byte-identical.
 */

import { writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '..', '..')
const outPath = join(repoRoot, 'tests/fixtures/star.svg')

/** The drawing, mm, in SVG's Y-down page frame. */
const PAGE_MM = 80
const CENTRE: [number, number] = [PAGE_MM / 2, PAGE_MM / 2]
const TIP_MM = 30
const INNER_MM = 12
const CIRCLE_RADIUS_MM = 6

type Pt = [number, number]

/**
 * The star's vertices, counter-clockwise as the Y-up project reads them
 * (clockwise on the Y-down page), from the tip on +X: tip, inner vertex,
 * tip, … An inner vertex is on the diagonal between its two tips.
 */
function star(): Pt[] {
  const out: Pt[] = []
  for (let k = 0; k < 4; k++) {
    const tip = (k * Math.PI) / 2
    const inner = tip + Math.PI / 4
    // Y-up angle; the page's Y runs down, so its sine is negated.
    out.push([CENTRE[0] + TIP_MM * Math.cos(tip), CENTRE[1] - TIP_MM * Math.sin(tip)])
    out.push([CENTRE[0] + INNER_MM * Math.cos(inner), CENTRE[1] - INNER_MM * Math.sin(inner)])
  }
  return out
}

/** A coordinate to 6 decimal places, trailing zeros dropped. */
const f = (x: number): string => {
  const s = x.toFixed(6).replace(/\.?0+$/, '')
  return s === '-0' ? '0' : s
}

const points = star()
const d = `M ${points.map((p) => `${f(p[0])},${f(p[1])}`).join(' L ')} Z`

const svg = `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3 test part (docs/specs/phase-3/functional.md, "Inlay
     pair"): a four-point star ${2 * TIP_MM} mm tip to tip with a ${2 * CIRCLE_RADIUS_MM} mm circle at its
     centre. Written by tools/parity/star-fixture.ts; do not edit by hand,
     re-run the tool. The tips are ${TIP_MM} mm out on the axes, the inner
     vertices ${INNER_MM} mm out on the diagonals. The ${PAGE_MM} × ${PAGE_MM} mm square "frame" is not
     cut: it makes init's stock 100 × 100 mm with the star at its centre.
     SVG is Y-down. -->
<svg xmlns="http://www.w3.org/2000/svg" width="${PAGE_MM}mm" height="${PAGE_MM}mm" viewBox="0 0 ${PAGE_MM} ${PAGE_MM}">
  <g fill="none" stroke="#000000" stroke-width="0.2">
    <path id="star" d="${d}"/>
    <circle id="circle" cx="${f(CENTRE[0])}" cy="${f(CENTRE[1])}" r="${f(CIRCLE_RADIUS_MM)}"/>
    <rect id="frame" x="0" y="0" width="${PAGE_MM}" height="${PAGE_MM}"/>
  </g>
</svg>
`

writeFileSync(outPath, svg)
console.log(`wrote ${outPath}: a ${2 * TIP_MM} mm star, inner vertices ${INNER_MM} mm out, a ${2 * CIRCLE_RADIUS_MM} mm circle, in a ${PAGE_MM} mm frame`)
