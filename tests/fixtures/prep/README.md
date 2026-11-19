<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Image prep fixtures and the calibration of the defaults

Drawings for `dc_import::prep` (sub-phase 3.4). All are synthetic and
written by `tools/parity/prep-fixture.ts` (and `compass-fixture.ts`); do not
edit them by hand, re-run the tool. The maintainer's own traced art is
never committed.

| File | What it is |
| --- | --- |
| `messy.svg`, `messy.dcv` | acceptance 61a: the six things the import checks find |
| `clean.svg` | the "DC" monogram (Droid Serif Bold, as `dc-inlay.svg`), two filled compound paths |
| `ring-cross.svg` | acceptance 61e: line art, a stroked ring and cross |
| `compass.svg`, `compass-regions.svg` | the line-art inlay test part |
| `speckled-dc.svg` | `clean.svg`'s letters with 34 seeded marks (below) |
| `jagged-dc.svg` | the same letters as a trace leaves them: 4185 vertices, 0.1 mm apart, each moved across the edge by at most half a step of 0.01 to 0.1 % of H |

## `speckled-dc.svg`

The design is 41.13 mm tall (H). Seventeen marks are filled dots (`speck`),
seventeen are holes in the letters (`pinhole`). Their widest points, percent
of H, are in three bands; each width is listed once per kind:

| Band | Widths, % of H | Why |
| --- | --- | --- |
| noise | 0.06, 0.08, 0.09, 0.11, 0.13, 0.15, 0.17, 0.19 | what a trace leaves: Inkscape's Trace Bitmap suppresses specks of 2 px, 0.2 % of a 1000 px picture |
| small | 0.25, 0.35, 0.45, 0.55, 0.65, 0.8 | small, but not trace noise |
| intended | 0.9, 1.2, 2.0 | detail the art means to keep |

No width sits on a swept value, so a flatten tolerance cannot move a mark
across a threshold. The serif tips and the D's counter are in the letters
themselves; the test checks that the counter survives and that the drawing
changes by at most 0.1 % of its box at the two defaults. The file's header
lists every mark (kind, band, centre, widest point), which the test reads.

## The sweep

Rerun it with

```sh
cargo test -p dc-import --lib print_the_sweep -- --ignored --nocapture
```

after `node --experimental-strip-types tools/parity/prep-fixture.ts`, which
needs Node 22.6 or newer and nothing installed (or `npx tsx` on an older
Node). Its output is byte-identical run to run, so `git status` shows no
change to the fixtures. The tests
`prep::defaults::fixtures_keep_their_intent_at_the_defaults` and
`prep::defaults::defaults_are_the_smallest_grid_values_that_pass` hold the
constants to it: moving either constant to a neighbouring swept value fails
at least one of them.

### `min_detail`, on `speckled-dc.svg` (smoothing off)

"Gone" counts the marks absent from the result (a dot with no piece over
it, a pinhole filled in); each cell is gone / seeded. A mark goes exactly
when its widest point is under the threshold; the test checks that for
every mark at every value.

| `--min-detail` % | noise gone | small gone | intended gone | passes |
| --- | --- | --- | --- | --- |
| 0.05 | 0 / 16 | 0 / 12 | 0 / 6 | no, noise left |
| 0.1 | 6 / 16 | 0 / 12 | 0 / 6 | no, noise left |
| **0.2** | 16 / 16 | 0 / 12 | 0 / 6 | **yes** |
| 0.3 | 16 / 16 | 2 / 12 | 0 / 6 | yes |
| 0.5 | 16 / 16 | 6 / 12 | 0 / 6 | yes |
| 1.0 | 16 / 16 | 12 / 12 | 2 / 6 | no, intended detail lost |

A value passes when every noise mark is gone and no intended detail is. The
smallest is 0.2. On `ring-cross.svg` (strokes 3.3 % of H wide) every value
keeps one piece with four holes and drops and fills nothing: the threshold
never touches line art that is thicker than it.

### `smooth`, on `jagged-dc.svg` (`--min-detail 0`)

| `--smooth` % | vertices removed | changed area, % of the design's box | % of the ink | passes |
| --- | --- | --- | --- | --- |
| 0 | 0.0 % | 0 | 0 | no |
| 0.02 | 54.2 % | 0.024 | 0.056 | no, under 80 % |
| **0.05** | 89.1 % | 0.077 | 0.177 | **yes** |
| 0.1 | 95.0 % | 0.117 | 0.268 | no, over 0.1 % |
| 0.2 | 96.5 % | 0.254 | 0.585 | no, over 0.1 % |

A value passes when at least 80 % of the vertices go, every output vertex is
within `eps` (the value, in mm) of the input outline, and the changed area
is at most 0.1 % of the design's box. Simplification keeps a subset of the
input's vertices, so the `eps` condition holds at every value; the test
keeps it as a guard on the simplifier. The smallest passing value is 0.05,
which is also at most half of the chosen `--min-detail`, so the two never
fight (`prep_smooth_exceeds_detail`).

The spec's "0.1 % of the design" is scored against the design's box (its
height by its width). That reading decides the result. The letters' ink is
43 % of the box, and against the ink's own area (the last column, reported
and not scored) 0.05 changes 0.18 % and no swept value passes: 0.02 removes
too few vertices and 0.05 changes too much.

### Both defaults together

The sweep takes each value with the other step off. `image-prep` runs both,
so the pinning test also runs each fixture at `--min-detail 0.2 --smooth
0.05`, against itself with both off:

| Fixture | pieces | holes | dropped | filled | vertices removed | changed area, % of the box | % of the ink |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `speckled-dc.svg` | 11 | 10 | 8 | 8 | 68.2 % | 0.072 | 0.165 |
| `jagged-dc.svg` | 2 | 1 | 0 | 0 | 89.1 % | 0.077 | 0.177 |

On `speckled-dc.svg` the sixteen noise marks go and every other mark stays
(a mark is still decided by the threshold alone with the smoothing on), and
the D's counter stays. On `jagged-dc.svg` the threshold drops nothing.

## Result

| Flag | Default | Source |
| --- | --- | --- |
| `--min-detail` | 0.2 % of the design's height | smallest swept value that clears the noise band and keeps the intended detail |
| `--smooth` | 0.05 % of the design's height | smallest swept value that removes 80 % of the jitter within 0.1 % of changed area |

The proposals of functional open questions 16 and 17 stand on the synthetic
fixtures. The constants, the specs, the book and the command's help all say
0.2 and 0.05.

## Still to confirm

The two values are calibrated on the synthetic fixtures only. Step 4 of the
procedure, the maintainer's run on their own traced art (never committed),
has not been done, so 0.2 and 0.05 are still proposals (issue #599). From
the repository root, with `art` set to where the art is, outside the
repository:

```sh
art=~/art/eagle.svg
out=$(dirname "$art")/prep-check
mkdir "$out"
for t in 0.1 0.2 0.3; do for s in 0.02 0.05 0.1; do
  cargo run -q -p dreamcarve -- image-prep "$art" --min-detail $t --smooth $s -o "$out/eagle-$t-$s.svg"
done; done
```

`image-prep` never overwrites a file, so remove `prep-check` before a
second run. In each run look at the `Dropped:` line and the
`prep_speck_dropped` and `prep_hole_filled` warnings (how many went, and
the widest of them): at 0.2, is anything gone that the art means to keep,
and at 0.1 are specks left? Then lay each written SVG over the original: at
0.05, are the staircases gone with the corners and thin tips still as
drawn, is 0.1 visibly rounder, is 0.02 still jagged? Do not judge by the
`Narrowest gap` line: on these fixtures it moves between 0.19 and 3.0 mm
with the `--smooth` value for the same letters. The defaults stand if 0.2
and 0.05 clear the specks, lose nothing meant to be kept and leave the
edges true. Otherwise the constants, the specs and the book move to the
neighbour that does.
