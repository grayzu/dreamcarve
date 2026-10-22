<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
# rs274-motion

Dev-only tool for issue #299 (layer 2 of
[`docs/testing/post-processor-e2e.md`](../../docs/testing/post-processor-e2e.md)).

It runs DreamCarve's LinuxCNC, Mach3, UCCNC and generic programs through
LinuxCNC's standalone interpreter, `rs274`. Then it compares the moves
`rs274` makes with DreamCarve's own reading of the program, which is the
reading the safety pass checks (`dc-gcode/src/parse.rs`). The
comparison goes source line by source line, and any of these is
reported as a difference:

- the move kind;
- the end point;
- the arc centre;
- the feed;
- a dwell;
- the expansion of a canned cycle (`G81`, `G82`, with `G98` or `G99`);
- the machine Z of a `G53` lift.

LinuxCNC is GPL-2.0. It runs as a separate program, natively or in a
container, and nothing of it is copied into this repository.

## Getting `rs274`

Pick one:

- **Debian or Ubuntu with `linuxcnc-uspace` installed.** `rs274` is then
  on `PATH`, and nothing else is needed.
- **A container**, on any OS with podman or docker:

  ```sh
  podman build -t dreamcarve-rs274 tools/rs274-motion
  export DREAMCARVE_RS274=podman:dreamcarve-rs274
  ```

  The `Containerfile` installs Debian 13's own `linuxcnc-uspace`, which is
  2.9.4 for amd64 and arm64. Containers run with `--network none`.

`DREAMCARVE_RS274`, or `--rs274`, can also be a path to the binary.

## Usage

```sh
cargo run -p rs274-motion                          # the corpus, with a report
cargo run -p rs274-motion -- --post mach3 a.tap    # your own files
cargo run -p rs274-motion -- --post uccnc@2 --units inch b.txt
cargo test -p rs274-motion -- --nocapture          # the test
```

The exit codes:

- 0: every program agrees.
- 1: one differs, or `rs274` rejects one.
- 2: nothing could be compared.

When `cargo test` finds no `rs274`, the tests that need it pass and print
why they skipped.

## What `rs274` reads

The bridge (`src/bridge.rs`) rewrites a program only where it must, and
the report lists every line it changes:

- Every line gets `N<line>`, so each canonical call names its source
  line. This replaces Mach3's own numbers, which are labels only.
- `G50` and `G69` are dropped (Mach3 and UCCNC). LinuxCNC has neither
  scaling nor `G68` rotation.
- On UCCNC, the `P` of a `G4` is divided by 1000, because UCCNC counts
  milliseconds and LinuxCNC seconds.
- On UCCNC, `G21` or `G20` is added to the first line with code, as
  `--units` says. UCCNC writes no units word.

The bridge cannot check the lines it rewrites. That is left to the manual
procedures in the plan.

`rs274` runs with a parameter file that sets a G54 offset of (1, 2, -3)
inches. With no offset, a `G53` move looks like any other move.

## The corpus

`src/corpus.rs` builds the programs the tool runs:

- The committed `post-*-coaster` goldens.
- The expected programs that earlier definition versions must still
  write.
- The quick-start plaque, exported in-process for each controller. It
  goes out plain, with `G81` and with `G82` canned cycles (LinuxCNC and
  Mach3), with a `G53` lift (LinuxCNC), and in inches (LinuxCNC and
  UCCNC).

`corpus_covers_each_controller_and_feature` fails if an export change
drops any of these features from the corpus.
