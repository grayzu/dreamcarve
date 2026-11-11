<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# Parity capture

Dev-only tool. Captures PureCutCNC's output for a case into `tests/golden/`,
where `cargo test` compares DreamCarve against it. Contributors without Node
never need this: the golden files are committed.

## Requirements

- Node 20 or newer.
- A PureCutCNC checkout with its dependencies installed (`npm install`),
  by default at `../purecutcnc` relative to this repository.

## Usage

```sh
cd tools/parity
npx tsx capture.ts --purecutcnc ../../../purecutcnc --case smoke
```

Writes `tests/golden/smoke/program.nc`, `toolpath.json`, and `README.md`.
Run it twice: the output must be byte-identical, which is the phase 0
acceptance test for this tool.

## Adding a case

1. Save a PureCutCNC project as `cases/<name>.camj`. Keep it small and
   freeze `meta.created` and `meta.modified` so re-captures are stable.
2. Optionally write `cases/<name>.notes.md`: README bullets the script
   cannot derive, such as the tolerance, the equivalent DreamCarve project,
   and any intentional difference from PureCutCNC. The capture copies them
   into the case's README as written.
3. Run the capture.
4. Write the equivalent DreamCarve project as
   `tests/golden/<name>/dreamcarve.dcv` and its test in
   `crates/dc-gcode/tests/golden.rs`. The capture never writes that file.
   See the Golden corpus chapter (`docs/book/src/contributing/golden-corpus.md`).
5. Commit the case and the golden files together, with a message that says
   what the case covers.

A golden file changes only in a commit whose message says why.

## V-Carve text fixtures

`vcarve-fixtures.ts` writes the outline-text regions the ported medial-axis
tests read (#402): PureCutCNC's resolved polygons for "Rag", "g" and the
`gA` noise-regression project, in DreamCarve's millimetre, Y-up frame,
with PureCutCNC's own medial-axis counts on each region, and their
README with the font notice.

```sh
cd tools/parity
npx tsx vcarve-fixtures.ts --purecutcnc ../../../purecutcnc
```

Writes `tests/fixtures/vcarve/{rag,g,ga-small,ga-large}.json` and
`README.md`. The output is byte-identical run to run. PureCutCNC's own
`node_modules/.bin/tsx` runs it too, when this directory has no
`node_modules`.

## The sign fixture

`sign-fixture.ts` writes `tests/fixtures/sign.svg`, the phase 3 sign test
part (#411): the outline, background, oval and hanging holes, and the word
"DreamCarve" as outlines of Droid Serif Bold, read from the three.js typeface
file a PureCutCNC checkout installs. It reads no PureCutCNC code.

```sh
cd tools/parity
npx tsx sign-fixture.ts --purecutcnc ../../../purecutcnc
```

`--font <file>` reads another `typeface.json` instead. The output is
byte-identical run to run. Node 23.6 or newer also runs it directly, as
`node sign-fixture.ts`. After changing it, rebuild
`tests/fixtures/sign.dcv`: the quick start test
(`quick_start::v_carved_sign_chapter_runs`) says how when it differs.

## The inlay fixture

`inlay-fixture.ts` writes `tests/fixtures/dc-inlay.svg`, the phase 3
inlay test part (#446, which replaced #437's star): the monogram "DC" in
Droid Serif Bold, 40 mm cap height, the letters 3 mm apart, centred in
the 90 × 60 mm frame that makes `init`'s stock 110 × 80 mm. It reads the
same typeface file as `sign-fixture.ts`, from a PureCutCNC checkout:

```sh
cd tools/parity
npx tsx inlay-fixture.ts --purecutcnc ../../../purecutcnc
```

The output is byte-identical run to run; `node inlay-fixture.ts` runs it
too, on Node 23.6 or newer. It writes only the drawing; the project
built from it is the quick start's. After changing it, rebuild
`tests/fixtures/dc.dcv`, the one project that cuts both boards (#498):
the quick start test (`quick_start::inlay_pair_chapter_runs`) says how
when it differs. The two-file pair of format 24 it replaced is a
migration fixture in `crates/dreamcarve/tests/fixtures/inlay-migration/`
and is not rebuilt.

## The compass fixture

`compass-fixture.ts` writes `tests/fixtures/prep/compass.svg` and
`tests/fixtures/prep/compass-regions.svg`, the phase 3 line-art inlay test
part (#605): a synthetic 70 mm compass in 2.5 mm line work (a ring and
eight spokes) with a 96 × 84 mm oval round it, and the eight pockets
between the strokes that `image-prep --regions` returns for it. It reads no
font and no other code, so it needs nothing installed:

```sh
cd tools/parity
node compass-fixture.ts
```

The output is byte-identical run to run (Node 23.6 or newer, or
`npx tsx compass-fixture.ts`). It writes only the drawings; the project
built from them is the test's (`cli::compass_inlay_exports_both_boards`).
`tests/fixtures/README.md` lists the shapes.

## The photo fixtures

`photo-fixtures.ts` writes the Photo V-Carve test pictures (#645):
`tests/fixtures/photo/gradient.png`, acceptance 58 and 59's 64 × 32
gradient with a white and a black block, and `colour.jpg` and
`colour-progressive.jpg`, a 16 × 16 picture of four flat colour blocks as
a baseline and a progressive JPEG. It encodes both formats itself (the
PNG with Node's `zlib`, the JPEG as flat 8 × 8 blocks of DC coefficients
alone), so it needs nothing installed:

```sh
cd tools/parity
node photo-fixtures.ts
```

The output is byte-identical run to run (Node 23.6 or newer, or
`npx tsx photo-fixtures.ts`). The pictures are synthetic: no photograph
is ever committed. `tests/fixtures/README.md` gives their pixel values.
