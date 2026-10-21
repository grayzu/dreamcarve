# Verifying a post-processor

A post-processor definition is controller-facing data: every line of it
reaches a real machine. This checklist is for adding a definition, or a
new version of one. Work through it in order; the pull request says which
items it covers.

The rules behind it are in `AGENTS.md` ("Reference implementation", the
controller-facing data rule),
[ADR-0004](https://github.com/grayzu/dreamcarve/blob/main/docs/adr/0004-post-processor-reference.md)
(versioned bundled definitions), and the
[definitions README](https://github.com/grayzu/dreamcarve/blob/main/crates/dc-project/src/definitions/README.md),
which records what was checked for each bundled definition and why it
differs from PureCutCNC's. Read the README's "Rules for the slice 17
definitions" before starting: those choices apply to every new
controller.

## Sources

- [ ] Find the controller's **own** documentation or source for the
      release you target: the manual, the G-code reference, or the
      firmware source. Note the version and the date you read it.
- [ ] Treat PureCutCNC's definition as the starting point only, never as
      evidence that a line is right. Never inspect a proprietary product's
      output to learn its dialect.
- [ ] Where the documentation is silent or ambiguous, take the
      conservative setting: plain moves rather than a canned cycle, no
      `M6`, no feature the document does not name. Record that it was
      silent.

## The definition file

- [ ] A new definition is `crates/dc-project/src/definitions/<id>/v1.json`;
      a change to one that changes emitted bytes is a new
      `<id>/v<n+1>.json`. A released version is never edited (the one
      exception, a schema-wide field added with the value that keeps the
      bytes, is in the README's "Versions").
- [ ] Add it to `SOURCES` in `dc-project/src/post_processor.rs` and pin
      its hash in `released_definition_versions_are_unchanged`.
- [ ] Start from the upstream file with the README's "Modifications"
      applied: snake_case keys, `"vendor": null`, the header naming
      DreamCarve, the user's settings moved into `default_settings`.
- [ ] Every line of the header, footer, tool change, spindle and dwell
      commands, the comment syntax, the number format, line numbers and
      the units words is checked against the sources, and cited in the
      definition's README section, in a table with one row per line
      (`post::every_definition_line_is_cited_in_readme` fails otherwise).
- [ ] List every change from PureCutCNC under "Changed from PureCutCNC"
      in that section, with the reason.

## The safety rules

Check the definition against each rule on the text it makes, not on
intent. The rules are in the phase 1 and phase 2 functional specs
("Safety requirements").

- [ ] **S1, S2.** Nothing in the header, footer or tool change moves the
      machine: no `G53` move, no `G28`/`G30`, no move to machine Z0 (soft
      limits or an unhomed machine make that unsafe).
- [ ] **S3.** No feed, plunge rate or spindle speed comes from the
      definition; they come from the tool. No default the user did not
      set.
- [ ] **S4.** The spindle starts with the tool's RPM and waits for speed
      (`spindle_on_dwell`, in the units the controller's dwell takes:
      seconds or milliseconds), and the footer stops it (`M5`). No `M4`,
      no `M7`.
- [ ] **S5.** The header sets the units, absolute coordinates, the plane,
      units-per-minute feed and a work coordinate system, so a controller
      left in another state by an earlier program is corrected. A
      controller with no units word (UCCNC) is documented as such.
- [ ] **S6.** The axis map and origin are a translation and axis mapping
      only: no mirror. `arc_interpolation` is on only where the controller
      documents `G2`/`G3` in the definition's `arc_format`; with `I`/`J`
      the header selects incremental centres where the controller has the
      mode (`G91.1`), and never `G90.1`.
- [ ] **S11.** `canned_cycles` is set only for the cycles the
      documentation describes, with their exact words. `G83` and `G73`
      need a documented re-entry or retract distance
      (`canned_cycles.reentry_gap_mm`); without one they stay off and the
      Drill is plain moves.
- [ ] **S12.** The controller's arc check is no stricter than the one
      `dc-gcode/src/arcfit.rs` reproduces (`ARC_RADIUS_CONSISTENCY_MM`,
      `ARC_RADIUS_GROSS_MM`, `ARC_RADIUS_RELATIVE`) for the definition's
      `arc_format` and number format; if the documentation gives a
      tighter one, raise it in an issue before setting
      `arc_interpolation`.
- [ ] **Comments.** User text in comments cannot open a command comment
      (`( Project: {programName} )`, not `( {programName} )`;
      `post::user_text_never_starts_a_comment`) and holds no byte the
      controller acts on inside comments (real-time commands on GRBL and
      grblHAL).
- [ ] **Tool changes.** `M6` only where the controller documents a stop at
      which the user can fit the bit and set Z zero; otherwise the tool
      change has no `M6`, `"m6"` mode is refused, and each tool run is its
      own program. No `G43 H`: DreamCarve writes no tool table.
- [ ] **Pauses.** A manual-spindle or tool-change pause is `M0`, with a
      message only in a syntax the controller documents; `export` lists
      every pause either way.

## Tests

- [ ] `dc-gcode`'s `post` tests pass over every version, including the
      controller facts in `dialect_facts_per_controller` and, for a new
      controller, a test of the dialect lines that make it differ (as
      `mach3_numbers_every_line` and `uccnc_writes_no_line_numbers_percent_or_units_word`
      do).
- [ ] `linuxcnc_mach3_uccnc_generic_differ_only_in_dialect` (or a test
      like it) shows the new definition's XYZ motion is the grblHAL
      program's.
- [ ] `canned_cycles_only_where_documented` holds, and a definition with
      canned cycles has an S11 test that expands each cycle and compares
      it with the plain moves.
- [ ] A golden coaster case, `tests/golden/post-<id>-coaster/`, captured
      from DreamCarve after the checks above, with a README that cites the
      sources, compared byte for byte by `crates/dreamcarve/tests/cli.rs`
      (`mod golden`). See [Golden corpus](golden-corpus.md).
- [ ] `cli::posts_lists_six` (or its successor) lists the new definition.
- [ ] A project still exports the bytes it did before: the phase 1
      expected programs in `crates/dreamcarve/tests/expected/` and every
      golden case are unchanged unless the change is a new version.
- [ ] For a LinuxCNC-dialect definition (LinuxCNC itself, or one
      LinuxCNC's interpreter reads, as it reads generic), its golden
      coaster is in `tools/gcode-conformance`'s corpus and the
      `gcode-conformance` CI job passes. Run it locally as below.

## LinuxCNC's interpreter

The `gcode-conformance` CI job (#173) runs DreamCarve's LinuxCNC and
generic programs through LinuxCNC's own standalone interpreter, `rs274`,
and fails on any error or warning it reports, or a program it does not
read to its `M2`/`M30`. The corpus is the two golden coasters, the
canned-cycle coaster pinned in
`crates/dreamcarve/tests/expected/coaster-holes-linuxcnc-canned.ngc`,
and the quick-start plaque (`tests/fixtures/plaque.dcv`) exported for
both posts by the `dreamcarve` binary.

Before it trusts `rs274`, the tool runs probe programs, following
PureCutCNC's harness: a trivial program and a valid arc it must accept,
and an arc 1 mm off its circle and `G90.1` followed by an arc with
incremental centres that it must reject. If a probe goes the wrong way,
the tool exits 2 and says the interpreter setup is wrong; a rejected
DreamCarve program exits 1.

`rs274` comes from the `linuxcnc-uspace` package. It has no macOS or
Windows build, and Ubuntu 22.04 and 24.04 do not package it, so
`tools/gcode-conformance/install-rs274.sh` installs the pinned release
(2.9.10) on Debian 12 from LinuxCNC's official apt repository, the way
LinuxCNC's
[install guide](https://linuxcnc.org/docs/stable/html/getting-started/getting-linuxcnc.html)
does. On any other system, run it in a `debian:bookworm` container. From
the repository root, with Docker or Podman:

```sh
docker run --rm -it -v "$PWD":/src -w /src debian:bookworm bash
```

Then, inside the container:

```sh
apt-get update && apt-get install -y --no-install-recommends ca-certificates curl gcc libc6-dev
tools/gcode-conformance/install-rs274.sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none
. "$HOME/.cargo/env"
rustup toolchain install               # the one rust-toolchain.toml pins
export CARGO_TARGET_DIR=target/linux   # keep the host's target/ apart
cargo build -p dreamcarve
cargo run -p gcode-conformance
```

To check another program too, pass it after `--`, for example
`cargo run -p gcode-conformance -- my-export.ngc`. `--rs274` and
`--dreamcarve` (or `RS274_BIN` and `DREAMCARVE_BIN`) point at other
binaries. The corpus and the probe files are left in
`target/linux/gcode-conformance/` (CI uploads them when the job fails).

Mach3 and UCCNC have no offline interpreter; run their programs in the
controller's own simulation before release.

## Docs

- [ ] The [Post-processors](../user-guide/post-processors.md) chapter
      lists the controller in "Choosing one" and "Controllers compared",
      with what was verified, its canned cycles and its tool changes.
- [ ] "Not yet checked on a real machine" in that chapter names it until
      a program from it has run on the controller.
