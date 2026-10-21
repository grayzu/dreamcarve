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
