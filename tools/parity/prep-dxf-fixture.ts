// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

/**
 * Write the image-prep DXF test drawings, `tests/fixtures/prep/*.dxf`
 * (#669).
 *
 *   node --experimental-strip-types prep-dxf-fixture.ts
 *   npx tsx prep-dxf-fixture.ts
 *
 * `tool-tray.dxf` is a small tray drawn in inches, as a CAD program would
 * leave it for `image-prep`: overlapping closed shapes, a speck, a hole
 * drawn as a separate shape, and a hatched pocket with an island.
 * `tool-tray-mm.dxf` is the same drawing in millimetres (every inch
 * coordinate times 25.4), so the two must prepare to the same SVG.
 * `open-paths.dxf` is one closed square with a LINE and an open
 * LWPOLYLINE beside it, which image-prep cannot outline.
 *
 * Every drawing is synthetic. Dev-only tool, not part of the Cargo
 * workspace. Run it twice: the output must be byte-identical.
 */

import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '..', '..')
const outDir = join(repoRoot, 'tests/fixtures/prep')
mkdirSync(outDir, { recursive: true })

type Point = [number, number]

/** A number as DXF writes it: always with a decimal point. */
function num(v: number): string {
  const s = String(Math.round(v * 1e9) / 1e9)
  return s.includes('.') || s.includes('e') ? s : `${s}.0`
}

/** Group code and value pairs as ASCII DXF lines. */
function groups(pairs: Array<[number, string]>): string {
  return pairs.map(([code, value]) => `${code}\n${value}\n`).join('')
}

function lwpolyline(layer: string, points: Point[], closed: boolean, s: number): string {
  const pairs: Array<[number, string]> = [
    [0, 'LWPOLYLINE'],
    [8, layer],
    [90, String(points.length)],
    [70, closed ? '1' : '0'],
  ]
  for (const [x, y] of points) {
    pairs.push([10, num(x * s)], [20, num(y * s)])
  }
  return groups(pairs)
}

function circle(layer: string, [x, y]: Point, r: number, s: number): string {
  return groups([
    [0, 'CIRCLE'],
    [8, layer],
    [10, num(x * s)],
    [20, num(y * s)],
    [30, '0.0'],
    [40, num(r * s)],
  ])
}

function line(layer: string, [x1, y1]: Point, [x2, y2]: Point, s: number): string {
  return groups([
    [0, 'LINE'],
    [8, layer],
    [10, num(x1 * s)],
    [20, num(y1 * s)],
    [11, num(x2 * s)],
    [21, num(y2 * s)],
  ])
}

/** A solid HATCH: a polyline boundary `outer` and a circle edge boundary. */
function hatchWithIsland(
  handle: string,
  layer: string,
  outer: Point[],
  centre: Point,
  r: number,
  s: number,
): string {
  const pairs: Array<[number, string]> = [
    [0, 'HATCH'],
    [5, handle],
    [100, 'AcDbEntity'],
    [8, layer],
    [100, 'AcDbHatch'],
    [10, '0.0'],
    [20, '0.0'],
    [30, '0.0'],
    [210, '0.0'],
    [220, '0.0'],
    [230, '1.0'],
    [2, 'SOLID'],
    [70, '1'],
    [71, '0'],
    [91, '2'],
    [92, '7'],
    [72, '0'],
    [73, '1'],
    [93, String(outer.length)],
  ]
  for (const [x, y] of outer) {
    pairs.push([10, num(x * s)], [20, num(y * s)])
  }
  pairs.push(
    [97, '0'],
    [92, '1'],
    [93, '1'],
    [72, '2'],
    [10, num(centre[0] * s)],
    [20, num(centre[1] * s)],
    [40, num(r * s)],
    [50, '0.0'],
    [51, '360.0'],
    [73, '1'],
    [97, '0'],
    [75, '1'],
    [76, '1'],
    [98, '0'],
  )
  return groups(pairs)
}

/** A whole DXF: header with `$INSUNITS`, then the entities. */
function dxf(insunits: number, entities: string): string {
  return (
    groups([
      [0, 'SECTION'],
      [2, 'HEADER'],
      [9, '$ACADVER'],
      [1, 'AC1032'],
      [9, '$INSUNITS'],
      [70, String(insunits)],
      [0, 'ENDSEC'],
      [0, 'SECTION'],
      [2, 'ENTITIES'],
    ]) +
    entities +
    groups([
      [0, 'ENDSEC'],
      [0, 'EOF'],
    ])
  )
}

function rect(x: number, y: number, w: number, h: number): Point[] {
  return [
    [x, y],
    [x + w, y],
    [x + w, y + h],
    [x, y + h],
  ]
}

/**
 * The tray, in inches, scaled by `s` (1 for inches, 25.4 for mm):
 *   tray      a 2.5 x 1.5 in outline, and on the same layer
 *   hole      a 0.25 in radius circle inside it (a hole drawn as a shape)
 *   pockets   two rectangles that overlap, which prep merges
 *   speck     a 0.001 in radius dot, under the minimum detail
 *   hatched   a hatched pocket with a round island
 */
function tray(s: number): string {
  return (
    lwpolyline('tray', rect(0, 0, 2.5, 1.5), true, s) +
    circle('tray', [0.75, 0.75], 0.25, s) +
    lwpolyline('pockets', rect(3, 0, 1, 0.75), true, s) +
    lwpolyline('pockets', rect(3.75, 0.375, 1, 0.875), true, s) +
    circle('speck', [5.25, 0.125], 0.001, s) +
    hatchWithIsland('2F', 'hatched', rect(5.5, 0.25, 1.25, 1.25), [6.125, 0.875], 0.25, s)
  )
}

const files: Array<[string, string]> = [
  ['tool-tray.dxf', dxf(1, tray(1))],
  ['tool-tray-mm.dxf', dxf(4, tray(25.4))],
  [
    'open-paths.dxf',
    dxf(
      4,
      lwpolyline('square', rect(0, 0, 20, 20), true, 1) +
        line('cut', [30, 0], [50, 20], 1) +
        lwpolyline('trace', [[30, 30], [40, 35], [50, 30]], false, 1),
    ),
  ],
]
for (const [name, text] of files) {
  writeFileSync(join(outDir, name), text)
  console.log(`wrote tests/fixtures/prep/${name}`)
}
