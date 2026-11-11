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

## `dc-inlay.svg` and `dc.dcv`

The phase 3 inlay test part (`docs/specs/phase-3/functional.md`,
"Lettered inlay pair"; #446, replacing #437's star), drawn by
`tools/parity/inlay-fixture.ts`. Do not edit it by hand; re-run the tool.
`dc.dcv` is the one project the quick start "An inlay pair" builds from
it (#498): its Inlay cuts the female on the stock and the male, with its
`cut_out`, on its `male_board`. It replaced the two projects
`dc-female.dcv` and `dc-male.dcv` of format 24, which differed only in
the Inlay's `side` and the male's `cut_out`; `dc.dcv` exports their
programs byte for byte, under the board's names. The pair, and the
star pair of format 23 before it, are kept as migration fixtures in
`crates/dreamcarve/tests/fixtures/inlay-migration/`, with their programs
in `crates/dreamcarve/tests/expected/inlay-migration/`.

| Shape | Size, mm | Where (SVG, Y-down) |
| --- | --- | --- |
| `letter-D` | Droid Serif Bold "D", 40 cap height, with its counter | from X 8.3 to 46.6 |
| `letter-C` | "C", 41.1 tall with its overshoot | from X 49.6 to 81.7, 3 mm right of the D |
| `frame` | 90 × 60 rectangle | the page; not cut |

The two letters together are 73.4 × 41.1 mm, centred on the page by
their exact extent (the curves' extremes, not their control points). Both
face right, so the male, mirrored, reads as a reversed "DC". The D's
counter is a design hole: an island in the female's pocket, a hole in the
male's plug. The frame is there only so that `init`, which makes the
stock the drawing plus 10 mm all round, makes the test part's
110 × 80 mm stock with the letters at its centre, (55, 40) in project
space. The male's band, `margin_mm` 8 plus the V wall above the start
plane, keeps more than 8 mm of board at every edge. The project sets the
stock's `thickness_mm` and `male_board.thickness_mm` to 12, the male
board's width and height `null` (the stock's); a user measures each
board and sets its own.

The unit tests (`cli::inlay_exports_and_passes_safety`, the `dc-cam`
inlay tests) still build a four-point star in code: they test geometry,
not the tutorial.

## `dc-boundary.dcv`

`dc.dcv` with the drawn boundary (#449, #605): its `frame` instance
(`i3`, the 90 × 60 mm rectangle that nothing else cuts) is the Inlay's
`boundary`, and `margin_mm` and `outline_rounding_mm` are `null`. Those
are the only three lines that differ; `cli::dc_boundary_fixture_is_the_frame_as_boundary`
holds it to that, so a change to `dc.dcv` must be made here too. It is
what the Inlay chapter's boundary section builds
(`quick_start::inlay_boundary_chapter_runs`). The frame is 8.319 mm from
the letters at its nearest (the band is the frame less the mirrored
letters, so the band's edge is the frame mirrored about X = 55, and the
frame is symmetric); the edge is open, cut by the end mill, and the
finished plug is the frame inset by the end mill's diameter, 3.175 mm.

## `prep/compass.svg` and `prep/compass-regions.svg`

The phase 3 line-art inlay test part (`docs/specs/phase-3/functional.md`,
"Line-art inlay"; #605), drawn by `tools/parity/compass-fixture.ts`. Do
not edit them by hand; re-run the tool. A synthetic compass in line work,
standing in for the maintainer's own art, which is never committed.

| Shape | Size, mm | Where (SVG, Y-down) |
| --- | --- | --- |
| `ring` | 70 mm tall, strokes 2.5 wide (centre line radius 33.75) | centred on the page, (48, 42) |
| `spokes` | eight lines from the centre to the ring, every 45 degrees, 2.5 wide | the cross and the four diagonals |
| `oval` | 96 × 84 ellipse, the boundary | the page |

`compass.svg` is the line art as strokes: what `image-prep --regions`
takes. `compass-regions.svg` is what that command returns for it, worked
out from the geometry by the same tool: the eight pockets between the
strokes, filled polygons (the ring's inner circle, radius 32.5, as 40
chords each, at most 0.01 mm off it), and the oval unchanged. The ridges
between the pockets are the strokes' 2.5 mm. It stands in for the
preparation until `image-prep` exists: `cli::compass_inlay_exports_both_boards`
imports it with `init`.

The test part's stock and male board are 130 × 100 × 12 mm. `init` makes
the stock the drawing plus 10 mm all round, 116 × 104 mm here, so the
test sets the stock and moves the drawing to its centre, as a user does by
hand. The pockets reach 32.5 mm from the centre, so the oval is 9.5 mm
from them at its nearest (top and bottom) and the cut-out runs 1.5875 mm
inside the oval.

## Font notice

The lettering of `sign.svg` and `dc-inlay.svg` is converted outline data
of Droid Serif Bold, from three.js 0.183.2,
`examples/fonts/droid/droid_serif_bold.typeface.json`, the copy a
PureCutCNC checkout installs (the V-Carve text fixtures used the same
files). Its directory's NOTICE and README read:

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
