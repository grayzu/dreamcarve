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
      own program. No `G43 H`: DreamCarve writes no tool table. No `G49`
      either, unless the controller's documentation shows that a Z zero
      set with an offset active does not include it: the user's zero may
      include an offset, and cancelling it moves every Z (#286; definitions
      README, "Tool length offset at touch-off").
- [ ] **Stale modes.** If the controller keeps a mode that changes every
      coordinate (scaling, rotation, a canned-cycle motion mode, cutter
      compensation) after the program ends, the header cancels it with the
      code the documentation gives, on a line the emitter can write again
      after each stop, and the safety pass accepts that code only from
      this definition's header (#279, #312, #316; definitions README,
      "Header modal state", "Scaling and rotation", "Scaling on grblHAL").
      Never write a code the controller does not document. Where the
      source is the reference, read what the cancel code actually clears,
      not only that it is accepted: grblHAL's `G50` misses a scale that
      leaves X at 1.
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
- [ ] For an RS274/NGC-style controller, the definition's programs run
      through LinuxCNC's `rs274` with the same motion DreamCarve reads:
      `DREAMCARVE_RS274=podman:dreamcarve-rs274 cargo test -p rs274-motion`
      (see "End-to-end testing" below). A word the controller has and
      LinuxCNC does not goes into the bridge in
      `tools/rs274-motion/src/bridge.rs`, with the reason, and into the
      manual procedure.

## End-to-end testing

Unit tests show DreamCarve writes what the definition says; they cannot
show a controller reads it that way. The
[post-processor end-to-end plan](https://github.com/grayzu/dreamcarve/blob/main/docs/testing/post-processor-e2e.md)
sets out, for each controller, what is checked automatically, what
cannot be and why, and the manual procedure for the rest:

- **Automatic.** LinuxCNC's interpreter, `rs274`, must accept the
  program (PR #264). Its motion must also match `dc-gcode`'s reading,
  line by line (`tools/rs274-motion`). Mach3 and UCCNC programs go
  through a small, listed bridge.
- **Manual.** Mach3 in Simulation mode and UCCNC in demo mode on
  Windows, and LinuxCNC's own AXIS simulator.
- **A real machine.** The trustworthy layer is an air cut, then foam or
  scrap, on a real machine: today the maintainer's grblHAL and GRBL.

Record each manual run, on the controller's software or on a machine,
with the [controller run form](https://github.com/grayzu/dreamcarve/issues/new?template=controller-run.yml): one issue per run, with the
program and project attached and the result of each check in the plan's
procedure for that controller.

To run the automatic check without LinuxCNC installed:

```sh
podman build -t dreamcarve-rs274 tools/rs274-motion   # or docker build
DREAMCARVE_RS274=podman:dreamcarve-rs274 cargo test -p rs274-motion -- --nocapture
```

## Docs

- [ ] The [Post-processors](../user-guide/post-processors.md) chapter
      lists the controller in "Choosing one" and "Controllers compared",
      with what was verified, its canned cycles and its tool changes.
- [ ] "Not yet checked on a real machine" in that chapter names it until
      a passed layer 5 report exists for it: a
      [controller run form](https://github.com/grayzu/dreamcarve/issues/new?template=controller-run.yml) issue for a real machine whose result
      is Pass.
