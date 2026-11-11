// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

/**
 * Write the image-prep test drawings, `tests/fixtures/prep/*.svg`
 * (sub-phase 3.4, #596).
 *
 *   node --experimental-strip-types prep-fixture.ts
 *   npx tsx prep-fixture.ts
 *
 * `messy.svg` is acceptance 61a's drawing: a 100 mm tall page holding the
 * six things the import checks find (and the three they count), each once.
 * `clean.svg` is the "DC" monogram of `tests/fixtures/dc-inlay.svg` (Droid
 * Serif Bold, `inlay-fixture.ts`) as two filled compound paths, one per
 * letter, which the checks leave alone. `ring-cross.svg` is acceptance
 * 61e's line art (#598). Later slices add their drawings (`speckled-dc.svg`,
 * `jagged-dc.svg`) to this script.
 *
 * Dev-only tool, not part of the Cargo workspace. Run it twice: the output
 * must be byte-identical.
 */

import { readFileSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const repoRoot = resolve(here, '..', '..')
const outDir = join(repoRoot, 'tests/fixtures/prep')
mkdirSync(outDir, { recursive: true })

const messy = `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3.4 test drawing (docs/specs/phase-3/functional.md,
     acceptance 61a): what the import checks find. Written by
     tools/parity/prep-fixture.ts; do not edit by hand, re-run the tool.
     One user unit is one mm on a 100 x 100 mm page (SVG is Y-down).
       sq-a, sq-b      10 mm squares overlapping by 4 mm (shapes_overlap)
       outer, inner    a 20 mm square with a 10 mm square inside it, two
                       paths (svg_hole_as_separate_path); outer reaches the
                       bottom edge, so the design is 100 mm tall
       centreline      a 1.2 mm stroke with no fill (svg_stroke_only_path)
       old-layer       a display:none layer holding a square
                       (svg_hidden_object)
       text, image, clipped   one of each (svg_object_not_convertible)
       dot             a 0.1 mm filled dot (feature_below_size) -->
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100mm" height="100mm" viewBox="0 0 100 100">
  <defs>
    <clipPath id="clip">
      <rect x="70" y="40" width="10" height="10"/>
    </clipPath>
  </defs>
  <rect id="sq-a" x="10" y="0" width="10" height="10"/>
  <rect id="sq-b" x="16" y="0" width="10" height="10"/>
  <rect id="outer" x="40" y="80" width="20" height="20"/>
  <rect id="inner" x="45" y="85" width="10" height="10"/>
  <polyline id="centreline" points="10,30 50,60" fill="none" stroke="#000000" stroke-width="1.2"/>
  <g id="old-layer" style="display:none">
    <rect id="ghost" x="70" y="20" width="10" height="10"/>
  </g>
  <text id="label" x="70" y="70" font-size="8">Hi</text>
  <image id="photo" x="75" y="75" width="10" height="10" xlink:href="photo.png"/>
  <g id="clipped" clip-path="url(#clip)">
    <rect id="clipped-rect" x="70" y="40" width="20" height="20"/>
  </g>
  <circle id="dot" cx="85" cy="12" r="0.05"/>
</svg>
`
writeFileSync(join(outDir, 'messy.svg'), messy)

// The monogram: the two letters' outline data from dc-inlay.svg, filled.
const inlay = readFileSync(join(repoRoot, 'tests/fixtures/dc-inlay.svg'), 'utf8')
const letter = (id: string): string => {
  const m = inlay.match(new RegExp(`<path id="${id}" d="([^"]+)"`))
  if (!m) throw new Error(`dc-inlay.svg has no path ${id}`)
  return m[1]
}
const clean = `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3.4 test drawing (acceptance 61a): the monogram "DC",
     one filled compound path per letter, which the import checks leave
     alone. The letters are those of dc-inlay.svg (inlay-fixture.ts). Written
     by tools/parity/prep-fixture.ts; do not edit by hand, re-run the tool.
     SVG is Y-down. -->
<svg xmlns="http://www.w3.org/2000/svg" width="90mm" height="60mm" viewBox="0 0 90 60">
  <path id="letter-D" fill="#000000" fill-rule="evenodd" d="${letter('letter-D')}"/>
  <path id="letter-C" fill="#000000" fill-rule="evenodd" d="${letter('letter-C')}"/>
</svg>
`
writeFileSync(join(outDir, 'clean.svg'), clean)

// ring-cross.svg: acceptance 61e. A circle of radius 29 mm stroked 2 mm (ink
// from radius 28 to 30) and two straight lines 58 mm long, 2 mm wide,
// crossing at its centre, on a 60 mm square page. No fill anywhere, so the
// drawing is all stroke: default mode prepares one piece with four holes,
// `--regions` the four spaces between the lines.
const ringCross = `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3.4 test drawing (acceptance 61e, 61h): line art in a
     ring. Written by tools/parity/prep-fixture.ts; do not edit by hand,
     re-run the tool. One user unit is one mm on a 60 x 60 mm page (SVG is
     Y-down).
       ring     a circle of radius 29 mm stroked 2 mm: ink from radius 28
                to 30, so the drawing is 60 mm tall
       cross    two straight lines 58 mm long, 2 mm wide, through the
                centre, one across and one up and down (butt caps) -->
<svg xmlns="http://www.w3.org/2000/svg" width="60mm" height="60mm" viewBox="0 0 60 60">
  <g fill="none" stroke="#000000" stroke-width="2">
    <circle id="ring" cx="30" cy="30" r="29"/>
    <line id="cross-across" x1="1" y1="30" x2="59" y2="30"/>
    <line id="cross-up" x1="30" y1="1" x2="30" y2="59"/>
  </g>
</svg>
`
writeFileSync(join(outDir, 'ring-cross.svg'), ringCross)

// ---------------------------------------------------------------------------
// speckled-dc.svg and jagged-dc.svg: the calibration fixtures (#599, design
// spec "Calibration of the defaults"). Everything is seeded.

type Pt = [number, number]

/** mulberry32: a small seeded generator, so the files are reproducible. */
const seeded = (seed: number): (() => number) => {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/**
 * The closed polylines of a path made of absolute M, L, Q and Z commands (the
 * form of dc-inlay.svg), edges cut into pieces at most `step` mm long.
 */
const flattenPath = (d: string, step: number): Pt[][] => {
  const tokens = d.match(/[MLQZ]|-?\d+(?:\.\d+)?/g) ?? []
  const loops: Pt[][] = []
  let loop: Pt[] = []
  let at: Pt = [0, 0]
  let i = 0
  const num = (): number => Number(tokens[i++])
  while (i < tokens.length) {
    const c = tokens[i++]
    if (c === 'M') {
      if (loop.length > 0) loops.push(loop)
      at = [num(), num()]
      loop = [at]
    } else if (c === 'L') {
      const to: Pt = [num(), num()]
      const n = Math.max(1, Math.ceil(Math.hypot(to[0] - at[0], to[1] - at[1]) / step))
      for (let k = 1; k <= n; k++) {
        const t = k / n
        loop.push([at[0] + (to[0] - at[0]) * t, at[1] + (to[1] - at[1]) * t])
      }
      at = to
    } else if (c === 'Q') {
      const ctl: Pt = [num(), num()]
      const to: Pt = [num(), num()]
      const length = Math.hypot(ctl[0] - at[0], ctl[1] - at[1]) + Math.hypot(to[0] - ctl[0], to[1] - ctl[1])
      const n = Math.max(2, Math.ceil(length / step))
      for (let k = 1; k <= n; k++) {
        const t = k / n
        const u = 1 - t
        loop.push([
          u * u * at[0] + 2 * u * t * ctl[0] + t * t * to[0],
          u * u * at[1] + 2 * u * t * ctl[1] + t * t * to[1],
        ])
      }
      at = to
    } else if (c === 'Z') {
      loops.push(loop)
      loop = []
    }
  }
  if (loop.length > 0) loops.push(loop)
  // A closed loop ends where it began: drop the repeated point.
  return loops.map((l) => {
    const [a, b] = [l[0], l[l.length - 1]]
    return l.length > 1 && Math.abs(a[0] - b[0]) < 1e-9 && Math.abs(a[1] - b[1]) < 1e-9 ? l.slice(0, -1) : l
  })
}

const letterD = letter('letter-D')
const letterC = letter('letter-C')
const letterLoops = [...flattenPath(letterD, 0.05), ...flattenPath(letterC, 0.05)]
const allPoints = letterLoops.flat()
const inkTop = Math.min(...allPoints.map((p) => p[1]))
const inkBottom = Math.max(...allPoints.map((p) => p[1]))
const inkLeft = Math.min(...allPoints.map((p) => p[0]))
const inkRight = Math.max(...allPoints.map((p) => p[0]))
/** H: the design's height, mm. Every size below is a share of it. */
const designHeight = inkBottom - inkTop

const distanceToSegment = (p: Pt, a: Pt, b: Pt): number => {
  const [dx, dy] = [b[0] - a[0], b[1] - a[1]]
  const len2 = dx * dx + dy * dy
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2))
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy))
}
/** Distance from `p` to the nearest letter edge. */
const edgeDistance = (p: Pt): number => {
  let best = Infinity
  for (const loop of letterLoops) {
    for (let k = 0; k < loop.length; k++) {
      best = Math.min(best, distanceToSegment(p, loop[k], loop[(k + 1) % loop.length]))
    }
  }
  return best
}
/** Even-odd: is `p` in the ink? */
const inInk = (p: Pt): boolean => {
  let inside = false
  for (const loop of letterLoops) {
    for (let k = 0, j = loop.length - 1; k < loop.length; j = k++) {
      const [a, b] = [loop[k], loop[j]]
      if (a[1] > p[1] !== b[1] > p[1] && p[0] < ((b[0] - a[0]) * (p[1] - a[1])) / (b[1] - a[1]) + a[0]) {
        inside = !inside
      }
    }
  }
  return inside
}

interface Mark {
  kind: 'speck' | 'pinhole'
  band: 'noise' | 'small' | 'intended'
  id: string
  x: number
  y: number
  /** Widest point, mm. */
  diameter: number
}

// Widths, percent of H. Noise: under 0.2 % (Inkscape's 2 px speckle filter is
// 0.2 % of a 1000 px picture). Small: 0.25 to 0.8 %. Intended: from 0.9 %.
// None sits on a sweep value, so a flatten tolerance cannot move one across.
const widths: Record<Mark['band'], number[]> = {
  noise: [0.06, 0.08, 0.09, 0.11, 0.13, 0.15, 0.17, 0.19],
  small: [0.25, 0.35, 0.45, 0.55, 0.65, 0.8],
  intended: [0.9, 1.2, 2.0],
}
const random = seeded(599)
const placed: Mark[] = []
for (const kind of ['speck', 'pinhole'] as const) {
  for (const band of ['noise', 'small', 'intended'] as const) {
    widths[band].forEach((pct, n) => {
      const diameter = (pct / 100) * designHeight
      for (let tries = 0; ; tries++) {
        if (tries > 100000) throw new Error(`no place for ${kind} ${band} ${n}`)
        const p: Pt = [
          inkLeft + 1 + random() * (inkRight - inkLeft - 2),
          inkTop + 1 + random() * (designHeight - 2),
        ]
        const clear = edgeDistance(p) - diameter / 2
        // A speck stands in the open, a pinhole in the ink, 0.6 mm from any
        // edge and 2 mm from every mark already placed.
        if (inInk(p) !== (kind === 'pinhole') || clear < 0.6) continue
        if (placed.some((m) => Math.hypot(m.x - p[0], m.y - p[1]) < 2)) continue
        placed.push({ kind, band, id: `${kind}-${band}-${n + 1}`, x: p[0], y: p[1], diameter })
        break
      }
    })
  }
}

const fixed = (v: number, digits = 4): string => v.toFixed(digits)
const markLines = placed
  .map((m) => `mark ${m.kind} ${m.band} ${m.id} ${fixed(m.x)} ${fixed(m.y)} ${fixed(m.diameter, 5)}`)
  .join('\n')
const pinholePath = (m: Mark): string => {
  const r = fixed(m.diameter / 2, 5)
  return ` M ${fixed(m.x - m.diameter / 2, 5)},${fixed(m.y)} A ${r},${r} 0 1,0 ${fixed(m.x + m.diameter / 2, 5)},${fixed(m.y)} A ${r},${r} 0 1,0 ${fixed(m.x - m.diameter / 2, 5)},${fixed(m.y)} Z`
}
// The D is the left letter: a pinhole belongs to the letter it falls in.
const dRange = (() => {
  const loops = flattenPath(letterD, 0.05).flat()
  return [Math.min(...loops.map((p) => p[0])), Math.max(...loops.map((p) => p[0]))]
})()
const inD = (m: Mark): boolean => m.x >= dRange[0] && m.x <= dRange[1]
const holesIn = (isD: boolean): string =>
  placed
    .filter((m) => m.kind === 'pinhole' && inD(m) === isD)
    .map(pinholePath)
    .join('')
const dots = placed
  .filter((m) => m.kind === 'speck')
  .map((m) => `  <circle id="${m.id}" fill="#000000" cx="${fixed(m.x)}" cy="${fixed(m.y)}" r="${fixed(m.diameter / 2, 5)}"/>`)
  .join('\n')

const speckled = `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3.4 calibration drawing (#599; design spec,
     "Calibration of the defaults"): the monogram "DC" of clean.svg with
     seeded specks (filled dots) and pinholes (holes in the letters). The
     design is ${fixed(designHeight, 3)} mm tall (H); every width below is a share of H.
       noise      widest point 0.06 to 0.19 % of H: what a trace leaves
       small      0.25 to 0.8 %: small, but not trace noise
       intended   0.9, 1.2 and 2.0 %: detail the art means to keep
     The serif tips and the D's counter are in the letters themselves.
     Written by tools/parity/prep-fixture.ts (seed 599); do not edit by
     hand, re-run the tool. SVG is Y-down. The sweep test reads these
     lines, "mark <kind> <band> <id> <x mm> <y mm> <widest point mm>":
${markLines.split('\n').map((l) => `       ${l}`).join('\n')}
-->
<svg xmlns="http://www.w3.org/2000/svg" width="90mm" height="60mm" viewBox="0 0 90 60">
  <path id="letter-D" fill="#000000" fill-rule="evenodd" d="${letterD}${holesIn(true)}"/>
  <path id="letter-C" fill="#000000" fill-rule="evenodd" d="${letterC}${holesIn(false)}"/>
${dots}
</svg>
`
writeFileSync(join(outDir, 'speckled-dc.svg'), speckled)

// jagged-dc.svg: the same letters cut into straight steps 0.1 mm long, each
// vertex moved across the edge by a seeded share of a step of 0.01 to 0.1 %
// of H, as the staircase of a traced bitmap (one pixel is 0.1 % of a
// 1000 px picture): the offset is at most half the step, either way.
const jitterRandom = seeded(5990)
const dLoopCount = flattenPath(letterD, 0.1).length
const jaggedLoops = [...flattenPath(letterD, 0.1), ...flattenPath(letterC, 0.1)].map((loop) =>
  loop.map((p, k): Pt => {
    const prev = loop[(k + loop.length - 1) % loop.length]
    const next = loop[(k + 1) % loop.length]
    const [tx, ty] = [next[0] - prev[0], next[1] - prev[1]]
    const len = Math.hypot(tx, ty) || 1
    const step = ((0.01 + 0.09 * jitterRandom()) / 100) * designHeight
    const offset = (jitterRandom() * 2 - 1) * (step / 2)
    return [p[0] - (ty / len) * offset, p[1] + (tx / len) * offset]
  }),
)
const jaggedPath = (loops: Pt[][]): string =>
  loops.map((l) => `M ${l.map((p) => `${fixed(p[0], 3)},${fixed(p[1], 3)}`).join(' L ')} Z`).join(' ')
const jagged = `<?xml version="1.0" encoding="UTF-8"?>
<!-- DreamCarve phase 3.4 calibration drawing (#599): the monogram "DC" of
     clean.svg as a trace leaves it, every edge cut into straight steps of
     0.1 mm (${jaggedLoops.flat().length} vertices) with each vertex moved across the edge by
     at most half a step of 0.01 to 0.1 % of H (H = ${fixed(designHeight, 3)} mm), seed 5990.
     Written by tools/parity/prep-fixture.ts; do not edit by hand, re-run
     the tool. SVG is Y-down. -->
<svg xmlns="http://www.w3.org/2000/svg" width="90mm" height="60mm" viewBox="0 0 90 60">
  <path id="letter-D" fill="#000000" fill-rule="evenodd" d="${jaggedPath(jaggedLoops.slice(0, dLoopCount))}"/>
  <path id="letter-C" fill="#000000" fill-rule="evenodd" d="${jaggedPath(jaggedLoops.slice(dLoopCount))}"/>
</svg>
`
writeFileSync(join(outDir, 'jagged-dc.svg'), jagged)
