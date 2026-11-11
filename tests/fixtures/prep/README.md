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
themselves; the tests check that the counter survives and that the letters
change by the smoothing alone. The file's header lists every mark (kind,
band, centre, widest point), which the test reads.

## The sweep

Rerun it with

```sh
cargo test -p dc-import --lib print_the_sweep -- --ignored --nocapture
```

after `node --experimental-strip-types tools/parity/prep-fixture.ts` (the
output is byte-identical run to run). The tests
`prep::defaults::fixtures_keep_their_intent_at_the_defaults` and
`prep::defaults::defaults_are_the_smallest_grid_values_that_pass` hold the
constants to it.

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

| `--smooth` % | vertices removed | changed area, % of the design's box | passes |
| --- | --- | --- | --- |
| 0 | 0.0 % | 0 | no |
| 0.02 | 54.2 % | 0.024 | no, under 80 % |
| **0.05** | 89.1 % | 0.077 | **yes** |
| 0.1 | 95.0 % | 0.117 | no, over 0.1 % |
| 0.2 | 96.5 % | 0.254 | no, over 0.1 % |

A value passes when at least 80 % of the vertices go, every output vertex is
within `eps` (the value, in mm) of the input outline, and the changed area
is at most 0.1 % of the design's box. Simplification keeps a subset of the
input's vertices, so the `eps` condition holds at every value; the test
keeps it as a guard on the simplifier. The smallest passing value is 0.05,
which is also at most half of the chosen `--min-detail`, so the two never
fight (`prep_smooth_exceeds_detail`).

## Result

| Flag | Default | Source |
| --- | --- | --- |
| `--min-detail` | 0.2 % of the design's height | smallest swept value that clears the noise band and keeps the intended detail |
| `--smooth` | 0.05 % of the design's height | smallest swept value that removes 80 % of the jitter within 0.1 % of changed area |

The proposals of functional open questions 16 and 17 stand. The values
are confirmed on the maintainer's own traced art (issue #599), which is run
with the proposal and its neighbours and reported on the issue, never
committed.
