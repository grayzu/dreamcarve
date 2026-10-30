<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Test fixtures

Drawings and projects the CLI tests and the quick start chapters use. The
V-Carve text fixtures have their own README in `vcarve/`.

## `sign.svg` and `sign.dcv`

The phase 3 sign test part (`docs/specs/phase-3/functional.md`, "Sign";
#411), drawn by `tools/parity/sign-fixture.ts`. Do not edit it by hand;
re-run the tool. `sign.dcv` is the project the quick start "A V-carved
sign from SVG outlines" builds from it.

| Shape | Size, mm | Where (SVG, Y-down) |
| --- | --- | --- |
| `outline` | 200 × 80 rectangle | the page |
| `background` | 190 × 70 rectangle | 5 mm inside the outline |
| `oval` | 150 × 46 ellipse | centred at (100, 40) |
| `hole-left`, `hole-right` | 3.175 circles | (15, 15) and (185, 15), the background's top corners |
| `letter-D` … `letter-e-2` | "DreamCarve", 14 mm cap height | inside the oval, centred on it |

The lettering is set on an arc of radius 500 mm bulging upward, the
"gentle curve" of the functional spec's scenario, each glyph turned to
the arc's tangent at its middle. The word runs 125.7 mm along the arc,
with the font's own advance widths and no kerning. Glyph curves are
written as the font's quadratic curves (`Q`), so DreamCarve flattens them
on import as it would an Inkscape drawing.

**Strokes and the cone.** The face is bold, and the sign carves it with
no flat depth with a 90° 6.35 mm V-bit, whose cone is 3.175 mm tall: any
part of a letter wider than 6.35 mm would need more. At 14 mm cap height
the widest part of any letter (a junction of stem and bowl) is about
3.3 mm, so the deepest cut is 1.658 mm; no scaling or change of face was
needed. `cli::sign_lettering_is_within_the_cone` holds it to 3 mm.

**The hanging holes** are in the background, which is carved 2.5 mm deep,
not in the 5 mm border (too narrow for a hole and the 6 mm countersink
the finished sign has) or the oval (the lettering fills it).

## `star.svg`, `star-female.dcv` and `star-male.dcv`

The phase 3 inlay test part (`docs/specs/phase-3/functional.md`, "Inlay
pair"; #437), drawn by `tools/parity/star-fixture.ts`. Do not edit it by
hand; re-run the tool. `star-female.dcv` and `star-male.dcv` are the two
projects the quick start "An inlay pair" builds from it; they differ only
in the Inlay's `side`.

| Shape | Size, mm | Where (SVG, Y-down) |
| --- | --- | --- |
| `star` | four points, 60 tip to tip; tips 30 out on the axes, inner vertices 12 out on the diagonals | centred at (40, 40) |
| `circle` | 12 across | centred at (40, 40) |
| `frame` | 80 × 80 square | the page; not cut |

It is the star `cli::inlay_exports_and_passes_safety` and the
`dc-cam::inlay` tests build in code. The frame is there only so that
`init`, which makes the stock the drawing plus 10 mm all round, makes the
test part's 100 × 100 mm stock with the star at its centre, (50, 50) in
project space. Each project sets `thickness_mm` to 12; a user measures
each board and sets its own.

## Font notice

The lettering is converted outline data of Droid Serif Bold, from
three.js 0.183.2, `examples/fonts/droid/droid_serif_bold.typeface.json`,
the copy a PureCutCNC checkout installs (the V-Carve text fixtures used
the same files). Its directory's NOTICE and README read:

```text
Copyright (C) 2008 The Android Open Source Project

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

     http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```

The Apache License, Version 2.0 is this repository's `LICENSE`. The
outlines are modified: scaled, rotated and moved onto the arc by the tool.
The typeface file's own metadata still carries the original vendor's
end-user licence text (Ascender Corporation); the Android Open Source
Project released the Droid fonts under Apache-2.0, which is the licence
three.js distributes them under and the one used here. The fixtures are
test data and not part of any DreamCarve binary.
