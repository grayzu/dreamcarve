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
 * letter, which the checks leave alone. Later slices add their drawings
 * (`ring-cross.svg`, `speckled-dc.svg`, `jagged-dc.svg`) to this script.
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
