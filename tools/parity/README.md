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

## The star fixture

`star-fixture.ts` writes `tests/fixtures/star.svg`, the phase 3 inlay test
part (#437): a four-point star 60 mm tip to tip with a 12 mm circle at its
centre, and the 80 × 80 mm frame that makes `init`'s stock 100 × 100 mm. It
reads nothing.

```sh
cd tools/parity
npx tsx star-fixture.ts
```

The output is byte-identical run to run; `node star-fixture.ts` runs it
too, on Node 23.6 or newer. After changing it, rebuild
`tests/fixtures/star-female.dcv` and `star-male.dcv`: the quick start test
(`quick_start::inlay_pair_chapter_runs`) says how when they differ.
