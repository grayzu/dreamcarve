# Code review and improvement plan

Reviewed 2026-11-17 at commit `c8a83193` (v0.2.1, phase 3 in progress).
Scope: every crate and tool except the egui UI crate `dreamcarve-app`.
`dc-app` is included: it is the headless view-model layer and has no egui
dependency. Nothing was changed; this document is the only output.

## Verdict

The code is careful, safe by default, and not copy-pasted. It is also
over-wide: the same table, check, or helper is written two to six times,
and the source carries its own issue history. The line count is not the
problem it looks like, and it will not shrink much. The real problems are
a handful of structural and safety items that the line count hides.

1. **The "250k lines" is 76k lines of production code.** The rest is
   tests (117k), comments (45k), and blank lines.
2. **About 20k to 28k lines (8 to 11%) can go with no behaviour change.**
   Roughly 5.5k to 7.5k production code, 4k to 5k production comments, and
   10k to 15k test lines. Anyone promising more is guessing; there is no
   coverage or mutation data to justify cutting further.
3. **Literal duplication is low** (1.6% of production lines, 3.1% of test
   lines). The bloat is structural: enums expressed as four to six parallel
   match ladders, a preamble hand-copied into every generator, three
   definitions of the same preferences, and per-issue patches layered
   inside single functions.
4. **The lint policy has produced a fail-silent idiom.** Because
   `indexing_slicing` warns, the code uses `.get(i)` with `continue`,
   `unwrap_or_default()`, or `unwrap_or(0.0)` at 114 or more places in
   production generator, geometry, and verifier code. In a
   toolpath generator that silently drops a pass; in the safety verifier it
   silently passes. This is the most important finding in the review.
5. **The strongest safety nets do not run.** Three motion simulators, ten
   stress sweeps, and randomised property seeds exist but are not in CI.

Security posture is good: `unsafe` is forbidden, decoders have explicit
limits, output is written atomically, and G-code comments are sanitised.
The open security items are denial of service (quadratic DXF stitching, no
file-size cap), not memory safety or injection into G-code.

## Measurements

Counts are from a line classifier and Serena's symbol tables, cross-checked
by the area reviewers. Treat them as accurate to a few percent.

| Non-UI crates and tools | Lines |
| --- | ---: |
| Production code | 76,567 |
| Production comments and docs | 26,718 |
| Test code | 116,854 |
| Test comments | 18,385 |
| Blank | ~13,000 |
| **Total** | **~252,000** |

| Crate | Production code | Comments | Test code |
| --- | ---: | ---: | ---: |
| dc-cam | 22,577 | 7,994 | 31,488 |
| dc-app | 11,270 | 3,707 | 12,406 |
| dc-project | 9,487 | 3,634 | 11,723 |
| dc-gcode | 7,841 | 3,315 | 26,857 |
| dc-import | 6,238 | 1,852 | 6,376 |
| dc-geometry | 5,822 | 2,107 | 7,092 |
| dc-job | 3,902 | 1,190 | 1,874 |
| dreamcarve (CLI) | 2,073 | 630 | 14,483 |
| dc-library | 1,418 | 556 | 1,769 |
| dreamcarve-shell | 800 | 397 | 454 |
| tools (7) | 5,139 | 1,336 | 2,332 |

Other signals:

- 3,118 `#[test]` functions; 15 `proptest!` blocks.
- 3,498 comment lines cite an issue number (`#NNN`).
- 76 production functions exceed 100 lines; 14 exceed 200.
- 213 `#[allow]`/`#[expect]` attributes, all with reasons.
- 4 `TODO`/`FIXME`. No `unsafe`.
- 117 crates in the CLI's dependency tree; no duplicate versions there.
- Clippy pedantic (advisory run, not the gate): 79 `redundant_clone`,
  129 `match_same_arms`, 95 `too_many_lines`, 29 `cognitive_complexity`.

The ten largest production functions:

| Lines | Function | File |
| ---: | --- | --- |
| 775 | `Display for Warning` | `dc-cam/src/warning.rs:1470` |
| 763 | `Display for Problem` | `dc-project/src/validate.rs:1629` |
| 656 | `generate_profile_below` | `dc-cam/src/profile.rs:334` |
| 483 | `Validator::project` | `dc-project/src/validate.rs:2432` |
| 468 | `emit_enabled` | `dc-gcode/src/emit.rs:589` |
| 337 | `prepare` | `dc-import/src/prep/run.rs:88` |
| 298 | `check_rules` | `dc-gcode/src/safety.rs:373` |
| 282 | `Setup::cut` | `dc-cam/src/pocket.rs:364` |
| 273 | `generate_drill` | `dc-cam/src/drill.rs:104` |
| 270 | `blocks` | `dc-cam/src/vcarve.rs:419` |

## How the review was done

- Mechanical: line classification, a 10-line window duplicate detector,
  function sizes from Serena's symbol tables, a clippy pedantic run, and a
  reference-count scan for unused public items.
- Qualitative: seven reviewers, one per area, each reading symbol overviews
  and targeted function bodies through Serena and checking references
  before calling anything dead. Context7 was used to check dependency APIs
  (thiserror, usvg, zune-jpeg) before recommending them over hand-written
  code.
- Each finding below is marked **confirmed** (the code was read) or
  **suspected** (inferred, needs checking before work starts).
- I re-read six findings myself against the source: the missing `inlay`
  arm, the fail-open `walls_allow`, the dead `FactKey`, the digest without
  bitmaps, the `unwrap_or(0.0)` floor Z, and `post.rs` being test-only.
  All six held.
- Not done in the first pass: profiling, coverage, test timing. Coverage
  and timing were measured the next day ("Second pass" below). Nothing has
  been profiled; performance findings are from reading code, and say so.

## Second pass: measurements (2026-11-18)

Run with the tools `tools/install-dev-tools.sh` installs, on the non-UI
workspace (`dreamcarve-app` excluded), on an Apple Silicon laptop.

**Test timing (cargo-nextest).** 3,016 tests pass, 15 are ignored. The
whole suite runs in 74 seconds of wall time (10 minutes of CPU) on a warm
build. 24 tests take over 5 seconds and 8 over 10; the slowest is
`dc-gcode::stress tapered::tapered_inlays_seat` at 53 seconds. Test volume
is not what makes CI slow: compiling on three operating systems is.

**Coverage (cargo-llvm-cov).** Line coverage of non-test files, with
inline test modules counted as covered code, so the figures run a little
high:

| Crate | Line coverage |
| --- | ---: |
| dc-geometry | 97.4% |
| dc-gcode | 97.1% |
| dc-library | 96.9% |
| dc-cam | 96.5% |
| dc-project | 96.2% |
| dc-job | 95.1% |
| dc-import | 94.9% |
| dreamcarve (CLI) | 93.5% |
| dreamcarve-shell | 90.9% |
| dc-app (without the UI crate's tests) | 88.7% |
| tools | 35% to 94% |

`safety.rs` is at 98.1%, `emit.rs` 95.3%, `offset.rs` 99.0%,
`sequence.rs` 99.2%. Two conclusions. The suite executes nearly all of
the code, so coverage will not find large dead areas, and the test
reductions in stage 2 are consolidation, not deletion of unused tests.
And the simulator tools are the least covered code in the repository
(`grbl-sim-motion` 40%, `gcode-conformance` 35%), because they only run
by hand (stage 0.1).

**Fail-silent lookups (pattern search, cross-checked with ast-grep).** A
strict search for a lookup miss answered by `continue`, a default, zero,
or the origin finds 114 sites in production code of dc-cam, dc-gcode, and
dc-geometry: 63 `.unwrap_or(0.0 | 0 | false | true)`, 18
`.get(..).unwrap_or_default()`, 18 `let Some(..) = ...get(..) else {
continue }`, 9 early returns of an empty value, and 6
`map_or(Point::new(0.0, 0.0), ..)`. This is a lower bound: it misses
multi-statement forms. The files with the most are `dc-gcode/src/safety.rs`
(15), `dc-cam/src/profile.rs` (13), `dc-gcode/src/arcfit.rs` (10), and
`dc-cam/src/sequence.rs` (9). The first-pass counts in findings 1.2 (167
`.get(` calls, about 120 `continue` guards) are every lookup and every
`continue`, not only the silent ones; use 114 as the worklist size.

**Mutation testing (cargo-mutants).** A one-in-sixteen sample of the
1,643 mutants in `dc-gcode/src/safety.rs` and `emit.rs`, each run against
dc-gcode's own tests (unit, golden, property, stress): of 103 mutants, 82
were caught, 17 survived, and 4 did not compile. That is 83% of viable
mutants caught, against 95 to 98% line coverage for the same files, and
it extrapolates to roughly 270 survivors across the two files. The
survivors are almost all guard conditions and tolerance arithmetic:

- `safety.rs:333` `check_on_stock`: the −X edge overhang can be computed
  wrongly and no verifier test notices.
- `safety.rs:2410`, `:2428`, `:2433` `arc_follows` and `:2488`, `:2500`
  `ToolpathArc::turn_to`: the check that a fitted arc follows its
  toolpath.
- `safety.rs:1692` `Matching::search`, `:2051` `match_rapid_on_the_way`.
- `emit.rs:2851` `Writer::feed_at_most_plunge` (the step that lowers a
  feed to the plunge rate), `:2759` `holds_arc_to_plunge` (replaced by
  `true`), `:2721` `linear_held`.
- `emit.rs:3023` `arc_ij` and `:3058` `arc_r`: the acceptance guards on a
  written arc; `:2164` `ArcGeometry::part`.
- `emit.rs:1128` `Lifts::of`, `:1458` `canned_plan`, `:1673`
  `expansion_is_toolpath`.

Some will be equivalent mutants, and tests in dc-job or the CLI may catch
others; each needs a look. The takeaway is that the safety code's
boundary conditions are the least-tested part of a heavily tested crate.
A full run of both files is about three and a half hours on this laptop
and belongs in the phase gate (stage 0.4), with each survivor either
killed by a new test or recorded as equivalent.

**Dependencies.** cargo-machete finds no unused dependencies. `cargo deny
check bans` fails locally: `deny.toml` sets `wildcards = "deny"`, and
`tools/grbl-sim-motion` and `tools/linuxcnc-sim` depend on `rs274-motion`
by path with no version. CI does not see it because the workflow runs
only `check licenses advisories` (stage 0.7).

**Line counts.** tokei gives 251,042 lines for the non-UI crates and
tools, matching the table above.

## Root causes

Fixing the findings without fixing these will regrow them.

1. **One issue, one patch, one test, one comment.** The spec-first,
   test-first flow works, but nothing in it asks for consolidation. A
   function gains a branch per issue (about 20 issue numbers inside
   `generate_profile_below`), an enum gains a variant per issue, and
   `offset` gained a retry per issue.
2. **Comments record when, not why.** The guideline says comments explain
   why. In practice they cite the issue, the slice, the review round, and
   the maintainer decision date. That is git's job.
3. **A lint that forbids indexing, with no sanctioned alternative.** The
   result is defensive `.get()` code that hides defects instead of
   reporting them.
4. **Crate boundaries without shared vocabulary below them.** `dc-job`
   cannot see `dc-app`'s string keys, the shell cannot see `dc-app`'s
   preference types, and `dc-cam` re-derives geometry `dc-geometry` lacks.
   Each gap is bridged by a copy and a mapping layer.
5. **No size budget.** Nothing fails when a function reaches 600 lines or
   a file reaches 10,000.

## Plan

Five stages, in order. Stages 0 and 1 are small and matter most. Stage 2
is where the lines go. Stage 3 changes code that produces toolpaths and
needs byte-identical golden and parity proof for every step.

### Stage 0: turn on the safety nets (no production code changes)

| # | Action | Why |
| --- | --- | --- |
| 0.1 | Add `cargo run -p rs274-motion` to the `gcode-conformance` CI job, which already installs `rs274`. Add `grbl-sim-motion` and `linuxcnc-sim` where their simulators can be installed. | About 5,100 lines of simulator tooling run by hand only. CI runs only the weakest check (the program is accepted). |
| 0.2 | Add a nightly workflow: property tests with a random logged seed, and `--release -- --ignored` for the ten stress sweeps. | Seeds are pinned (`dc-gcode/tests/properties/main.rs:51`, `dc-geometry/tests/properties.rs:38`), so the properties are frozen examples. The sweeps found 13 or more generator bugs when last run by hand. |
| 0.3 | Add cargo-llvm-cov to the nightly run and publish per-crate coverage. | The first measurement is 94% overall (see "Second pass"); the job keeps it from slipping. |
| 0.4 | Run cargo-mutants on `dc-gcode/src/{safety,emit}.rs` and the depth and feed code in dc-cam. | The only evidence-based way to judge test value in safety-critical code. |
| 0.5 | Add cargo-fuzz targets for `import_svg`, `import_dxf`, bitmap decode, `fusion::import`, `dc_project::load`, and `parse_motion`. | Untrusted-file parsers are covered by example tests only. |
| 0.6 | Add a recapture script for `crates/dreamcarve/tests/expected/` that prints a motion-level diff, and a rule that a recapture commit contains nothing else. | 103 self-captured programs have been bulk-regenerated at least six times, 10 to 24 files per commit. A blanket recapture can hide a regression. |
| 0.7 | Run `cargo deny check` in full (add `bans`), and give the two simulator tools' `rs274-motion` path dependency a version. | `deny.toml` forbids wildcard dependencies but CI never checks bans; it fails locally today. |

Note on 0.1: `docs/testing/post-processor-e2e.md` says the agents' token
lacks `workflow` scope, so a human has to make the CI edits.

### Stage 1: correctness and safety fixes (small, behavioural)

| # | Sev | Finding | Fix |
| --- | --- | --- | --- |
| 1.1 | P1 | **The verifier fails open.** `walls_allow` (`dc-gcode/src/material.rs:2699`) does `area.edges.get(scope).unwrap_or_default().iter().all(..)`: an out-of-range scope returns "clear". Same idiom at `material.rs:2781`, `:2821` and `safety.rs:1622`, `:1706`, `:1723`. No reachable bug was found, since the ranges are built internally. Confirmed. | A helper that returns `false` or an error on a miss. Audit every `unwrap_or_default()` and `unwrap_or(0.0)` in `safety.rs` and `material.rs` for direction. |
| 1.2 | P1 | **Generators skip silently and invent values.** In dc-cam's 2.5D code: 167 `.get(` calls and about 120 `continue` guards. A missing twin path means that pass is not emitted (`profile.rs:742`). Floor Z falls back to `0.0` (`profile.rs:513`). A missing path start becomes `Point::new(0.0, 0.0)` in 8 places. V-carve and medial code add 195 `.get()` and 103 bails, where a miss yields a shallower carve. Confirmed. | Return an internal-defect `CamError` (precedent: `InvalidBlock`). Stage 3.1 removes most of the lookups. |
| 1.3 | P2 | **Schema errors inside an Inlay lose their path.** `operation_error` (`dc-project/src/serialize.rs:330`) has no `"inlay"` arm under the comment "One arm per `Operation` variant". Confirmed. | Add the arm; better, generate the dispatch from `Operation`. |
| 1.4 | P1 | **The generation digest omits bitmaps.** `digest` (`dc-app/src/generation.rs:225`) does not hash `project.bitmaps`, which Photo V-Carve reads. No command edits a bitmap today, so it cannot fire yet; export is unaffected because it rebuilds. Latent. | Hash the bitmap an operation names; extend `digest_covers_every_generator_input`. |
| 1.5 | P1 | **DXF stitch and dedupe are unbudgeted O(n²).** `dc-import/src/dxf/stitch.rs:25`, `:296`, `:374`, `:408`. A 100k-segment export costs about 10¹⁰ operations; a hostile file hangs the app. `drop_hatch_duplicates` next door already has a grid index and a work budget. Confirmed. | Spatial-hash endpoints; build chains in a deque; or apply the existing `MatchBudget`. Keep tie-breaking and output order. |
| 1.6 | P2 | **No size cap on any file read.** `dreamcarve/src/cli.rs:555`, `dreamcarve-shell/src/library.rs:29`, the bare-JSON Fusion path (`fusion.rs:343`). Confirmed. | One `read_capped(path, limit)` in the shell with a typed error. |
| 1.7 | P2 | **Rates are validated, then forgotten.** `Rates::of` (`emit.rs:1793`) checks feed and plunge but keeps only rpm; `emit.rs:1408` then does `plunge_mm_min.unwrap_or_default()`. "Never invent a feed" holds by check order, not by types. Confirmed. | `Rates { feed_mm_min: f64, plunge_mm_min: f64, rpm }`, passed down. |
| 1.8 | P3 | **Control characters reach the terminal.** DXF layer and block names (where `\U+001B` decodes to ESC) are printed raw in warnings. The G-code path is safe (`sanitize_comment`). Confirmed. | Strip control characters at import or at the print boundary. |
| 1.9 | P3 | **`init` checks then overwrites.** `init.rs:76`: `exists()` then a replacing rename. Confirmed. | A no-replace commit variant. |
| 1.10 | P3 | **Cuts outside every span get weaker checks.** Reachable only through a bundled definition's header or footer. Suspected. | Refuse `G1`/`G2`/`G3` outside a span. |
| 1.11 | P3 | **Wall-clock assertions in unit tests** (`dc-import/src/dxf.rs:1965`, `:2624`, `:2700`; `prep/stroke.rs:265`; `dc-cam/src/photo/tests.rs:720`). Flaky on shared runners. | Assert operation counts or bounded output. |
| 1.12 | P2 | **O(n²) `unique()` over DXF warning names** (`dxf.rs:611`). | A seen-set at push time; cap the names reported. |
| 1.13 | P1 | **A clipped Inlay cut-out multiplies its tab count.** Reported by the owner: 4 tabs requested, 12 cut. A request's `count` applies per tool path (`placed_tabs::place`). Where a male band runs past the board, `cut_out_paths` splits one cut-out into several open paths from edge to edge (`inlay.rs:1186`), and each gets the full count. The safety pass builds the same Profile, so it agrees and does not flag it. Code path read, not reproduced. | Lay the count out over the whole cut-out, across its pieces. Add a test with a band clipped on two or three edges. See stage 4, decision 2. |

### Stage 2: remove cruft with no behaviour change

Each row is one or more small PRs. Deltas are production lines unless
noted.

**Enums written several times over**

| # | Finding | Fix | Delta |
| --- | --- | --- | ---: |
| 2.1 | `Problem` (87 variants) is four hand-kept ladders, about 2,220 lines: `code()` (the variant name in snake case), `severity()`, `field()`, and a 763-line `Display`. thiserror is already a dependency and `LoadError` already uses it. About a dozen variants duplicate `OutOfRange` only to get a distinct code. | `#[derive(thiserror::Error)]` messages on the variants; a small macro for the code; `severity` as one `matches!`; give `OutOfRange` a `code` field. | −700 to −900 |
| 2.2 | `Warning` in dc-cam writes each of 63 variants six times (`WarningCode`, `as_str`, the variant, `code()`, `op()`, `shape()`, a 775-line `Display`). 53 of 63 warnings and 57 of 68 `CamError` variants carry `op`, producing 86 `/// The operation.` lines. | `struct Warning { op, shape, kind }`; one macro for code names; hoist `op` out of `CamError`. | −600 |
| 2.3 | `ImportWarning`: a 199-line `Display`, a 40-line `code()`, and a 140-line test asserting each variant has a code. | Co-locate message and code per variant. | −60 |
| 2.4 | Warning codes cross crates as string literals (`dc-job/src/build.rs:56`, `dc-app/src/editor/mod.rs:945`) although dc-cam has a typed `WarningCode`. | Carry `WarningCode` in `Finding`. | −20 |

**The same thing defined two or three times**

| # | Finding | Fix | Delta |
| --- | --- | --- | ---: |
| 2.5 | The generator preamble (tool lookup, rates, safe Z, step-down, thickness, depth, a 12-field `SequenceInput` literal, the off-stock check) is hand-copied in profile, pocket, engrave, drill, bore, vcarve, inlay, and photo. `SequenceInput {` is built in 9 files. | `OpContext::resolve(..)`, `SequenceInput::new(&ctx, blocks)`, `ctx.check_on_stock(..)`. Keep the check order; tests assert error precedence. | −450 |
| 2.6 | Three copies of the target walk (`profile.rs:2818`, `engrave.rs:187`, `drill.rs:577`). | One `placed_profiles(project, op_id, targets)` iterator. | −120 |
| 2.7 | Preferences are defined three times: `dc-app/src/prefs.rs` and `view.rs`, `dreamcarve-shell/src/gui_prefs.rs`, and two 60-line converters in `dreamcarve-app/src/shell/files.rs`. `unknown_keys` (`gui_prefs.rs:296`) also hand-lists every field name. | Derive serde on the dc-app types; make the shell's load and save generic; use `serde_ignored` for unknown keys. | −300 |
| 2.8 | The setup sheet has a second string-key system: `Label` (104 lines), `ALL`, `key()`, and a 49-arm map back to `StrKey` in `dc-app/src/export.rs:547`. `fill` copies `Strings::fmt`. | Move the string-key generation to a small crate below dc-job, or pass a lookup function. | −290 |
| 2.9 | Tool rules exist twice with different wording (`dc-library/src/library.rs:329` and `validate.rs:2528`). The V-bit angle predicate is written in at least ten places; step-over range in six. | `Tool::faults()` and predicates in `dc-project/src/tool.rs`. | −150 |
| 2.10 | Each carve fact is worded three times (`dc-job/src/summary.rs:482`, `dc-app/src/warnings.rs:319` and `:452`). | One template table once 2.8 lands. CLI text is golden-tested; wording must match. | −100 |
| 2.11 | Five number parsers and three formatters in dc-app and dc-job; `decimal` and `refused` are byte-identical in `tools.rs` and `properties.rs`; two XML escapers. | `units::parse_number(text, suffix)` and `units::trimmed`. | −100 |
| 2.12 | Geometry re-derived outside dc-geometry: point-to-segment distance in five places; three ray-cast point-in-polygon copies; two segment grids with an identical `clamped_index`; two `sub_path`s; two `Ord` wrappers duplicating `OrderedF64`; `Rect` lacks `intersects`/`grown`/`gap_to`, so the box test is rewritten six times; `once(outer).chain(holes)` appears 69 times. | Move to dc-geometry and delete the copies. `entry.rs` is an upstream port with exact-key ordering: move it verbatim. | −400 |
| 2.13 | Arc centre from I/J/R is computed in at least seven places in dc-gcode. | One `MachineMove::centre()`. | −40 |
| 2.14 | `MM_PER_INCH` is defined in five crates. `ON_BOUNDARY_MM` twice. Six separate `1e-9` epsilons and 32 unnamed `1e-N` literals in V-carve and geometry code. `REACH_SLACK_MM` means 0.5 mm in one file and 0.01 mm in another. | One constant each; name the literals (the guideline already requires it). | small |
| 2.15 | The three simulator tools repeat `options()`, `given_files()`, corpus building, and temp-dir handling. Both others already depend on `rs274-motion`. | Hoist into its library. Fold `gcode-conformance`'s strictness probes into `rs274-motion` (suspected feasible). | −500 to −700 |

**Tables written as code**

| # | Finding | Fix | Delta |
| --- | --- | --- | ---: |
| 2.16 | `dc-app/src/editor/fields.rs` is a sound declarative table written as 96 seven-line constructor calls. | A `rows!` macro or a `const fn` taking the label and help pair. | −550 |
| 2.17 | Three unrelated field-editing mechanisms in dc-app: the operation table, `project.rs` `enum Field` with seven parallel 14-arm matches, and separate ladders in `tools.rs` and `properties.rs`. | A const table for the project `Field`. | −100 |
| 2.18 | 28 migration steps with the same "insert key after anchor" loop pasted about ten times; helpers hard-code a version number in their messages so they cannot be reused; key order is preserved for a `Value` that only feeds deserialisation; a 130-line module-doc history restates the CHANGELOG. | One `add_field(..)` helper; a steps array. | −400 code, −130 docs |
| 2.19 | `model.rs:478` has eleven 8-arm `Operation` accessors; `post_processor.rs:645` spends 100 lines on 16 `include_str!` entries; `fusion.rs:578` walks JSON by hand for 225 lines; `library.rs:57` hand-copies the `id_type!` macro. | Macros and serde derives. | −300 |

**Dead and over-exposed API**

| # | Finding | Fix | Delta |
| --- | --- | --- | ---: |
| 2.20 | `FactKey` and `Fact::key` (`dc-job/src/summary.rs:332`, `:460`) have no references. `SubArc::centre_offsets`, `Command::UpdateTab`, and `impl From<&Row> for Slot` likewise. | Delete. | −80 |
| 2.21 | Twelve free functions in `dc-app/src/editor/mod.rs:417` forward to `Slot` methods; one production caller, about 75 test callers. Ten more public functions are test-only. `dc_gcode::emit()` is public for about 120 test call sites; production uses `emit_programs`. | Point tests at `Slot`; make the rest `pub(crate)` or test support. | −220 |
| 2.22 | `pub mod arcfit`, `canned`, `emit`, `format` in dc-gcode expose internals nothing outside imports, which also blinds rustc's `dead_code` lint. Several dc-cam items are `pub` but used in one file. | `pub(crate)`. | 0 |

**Comments**

| # | Finding | Fix | Delta |
| --- | --- | --- | ---: |
| 2.23 | 3,498 comment lines cite issue numbers; others cite "slice 27c", "security review H3", "#310 review", "maintainer decision 2026-10-24". `material.rs` opens with a 311-line module doc; `generate_profile` has a 146-line doc comment restating the spec; `validate.rs:4` is a 56-line changelog; `fields.rs:31` a 23-line one. | Keep the invariant, drop the provenance, link the spec. Cap function docs at about 15 lines. | −4,000 to −5,000 production, −5,000 to −8,000 test |
| 2.24 | Stale or misplaced: `model.rs:53` ("once it lands"; arc fitting exists), `post_processor.rs:749` ("six files"; there are 16), doc comments on the wrong function at `error.rs:1118` and `cone.rs:718`, and `bitmap.rs:211` claiming `Clone` stays cheap. | Fix. | 0 |

**Tests**

| # | Finding | Fix | Delta (test lines) |
| --- | --- | --- | ---: |
| 2.25 | 66k lines of tests live inside production source files (`validate.rs` 5.3k, `profile.rs` 4.4k, `safety.rs` 4.3k, `emit.rs` 3.5k, `pocket.rs` 3.0k). The files are 8k to 11k lines to read, and a test edit rebuilds every downstream crate. | Move each to `foo/tests.rs` behind `#[cfg(test)] mod tests;`, as `inlay/tests.rs` already does. | 0 |
| 2.26 | `crates/dreamcarve/tests/cli.rs` is one 13,097-line module with about 566 process spawns. Many tests mutate a `Project` in Rust, save it, spawn the binary, and parse the G-code: library tests routed through a process. | Split by subcommand under one `main.rs`. Move geometry-asserting tests to dc-job. Keep the CLI layer for arguments, exit codes, files, and printed text. | −1,000 |
| 2.27 | About 65 integration-test binaries (31 in dc-app). Each links the full tree at opt-level 2. `#![allow(dead_code)]` on the shared `common` modules exists only because of this, and hides dead helpers. | One `tests/main.rs` per crate; drop the allows. | small |
| 2.28 | 97 helper names are each defined in three or more files: 481 definitions, 6.3k lines (`rect` ×21, `project` ×17, `generate` ×16, `tool` ×12, `v_bit` ×8). | Parameterised builders in `dc_project::test_support` and dc-cam's `test_support`. | −3,000 to −4,000 |
| 2.29 | Version and migration tests accreted one per format: ten `version_N_is_newer_than_N-1` tests with the same runtime body, 29 fixture-load tests, about 14 "already present" tests, and 39 CLI version-pin tests (1,402 lines) that prove load and save idempotence by spawning. | Tables and one loop over fixtures in dc-project. Keep the committed-bytes tests. | −1,300 |
| 2.30 | Message text is pinned in several places: 170 long `.contains("…")` assertions, 98 `to_string()` equalities, an 870-line message fixture mirroring `Warning`'s `Display`. | One message snapshot per enum; assert codes and fields elsewhere. Mostly falls out of 2.1 to 2.3. | −1,500 |
| 2.31 | `golden.rs::phase_1_and_2_cases_are_unchanged` re-runs all 18 cases the individual tests already ran. Two tests exist only because a phrase hand-copies a constant. | Remove the double run; use `format!` and delete the compensating tests. | small |

### Stage 3: structural refactors (need golden and parity proof)

Do these one at a time, each proven byte-identical against `tests/golden`,
the expected programs, and the simulators from stage 0.

| # | Sev | Finding | Fix | Risk |
| --- | --- | --- | --- | --- |
| 3.1 | P1 | `generate_profile_below` is 656 lines on about 16 index-parallel vectors, with twins appended to the same vectors and addressed by index. `TabPlan` repeats the pattern. `profile.rs` plus `profile/*` is about 4,300 lines that grew by issue, not by design. | One `PathState` per path with an optional twin; split into stages (validate, place, tabs, last-pass twins, seams, blocks, warnings). Merge `EntrySetup`, `PassCtx`, `LeadSetup`; remove six `too_many_arguments` allows. About −350 to −500. | High. One stage per PR. |
| 3.2 | P1 | `offset` (`dc-geometry/src/offset.rs:68`) is a 12-attempt retry ladder over `cavalier_contours` 0.9, one attempt per issue. Upstream can panic; its debug assertions are disabled in `Cargo.toml`; there is no `catch_unwind`. "Library crates never panic" depends on a dependency known to panic. All retry inputs are built eagerly. `offset_open` has no ladder at all. | Fork or patch upstream with the regression contours as its tests; shrink to one call plus the `plausible` check; build retries lazily meanwhile. About −270. | High. Every offset feeds a toolpath. |
| 3.3 | P1 | Undo keeps up to 101 unshared `Project` copies, each holding definitions and base64 pictures by value. Every edit clones twice, validates, serialises the whole project for the dirty flag, and rebuilds the scene. Validation runs inlay polygon work. | `Arc` for definitions and bitmap payloads; a revision counter for dirty; a dedicated non-finite scan in `save()` instead of full validation. Serde output must stay byte-identical. | Medium |
| 3.4 | P1 | The operation editor serialises the operation to JSON on every field read (`dc-app/src/editor/mod.rs:200`), dozens of times per paint. The warnings panel rebuilds every row twice back to back. Path confirmed; per-frame cadence suspected. | Build the record once per revision; memoise warning rows. | Low |
| 3.5 | P1 | Quadratic point queries in V-carve: `Walls::holds` copies every loop on each query; `Walls::clearance_mm` scans all segments although it owns a grid; `even_odd` is brute force for every circumcentre. Code confirmed, impact not profiled. | Row-bucketed ray cast; route clearance through the grid. Profile first. | Low to medium |
| 3.6 | P2 | S6 in the verifier is a backtracking search for a line-to-move mapping the emitter already knows, holding five live copies of the program. | `Writer` records provenance as an untrusted hint; S6 verifies linearly and searches only on ambiguity. About −200. | The accepted set may only narrow. |
| 3.7 | P2 | Canned cycles are proven twice with bespoke code (`emit.rs:1556` `expansion_is_toolpath`, then S6 again). | Run the S6 matcher on the candidate span. About −200. | Could change Drill G-code. |
| 3.8 | P2 | `emit_enabled` (468 lines, a 6-tuple read as `.0`/`.1`), `check_rules` (298 lines, twelve mutable locals), `material::check` (223 lines, one enum mixing three protocols), `prepare` (337 lines), `vcarve::blocks` (270 lines, two strategies split by a `continue`), `medial_axis_of_loops` (197 lines). | Split along the step comments each already has; a `ModalState` struct; a `Planned` struct. | Low |
| 3.9 | P2 | Planning runs twice (`plan` then `sequence`, which calls `plan` again). `placed_tabs.rs` clones the whole `Project` three times to add tabs. An SVG is parsed four or more times per `init`. The JPEG marker walker duplicates the decoder's own limit. About ten allocations per G-code line in `motion_line`. | `sequence_in_order`; a tabs overlay; `usvg::Tree::from_xmltree`; `decode_headers`; one reused buffer. | Low; benchmark first. |
| 3.10 | P3 | Test hooks inside the safety-critical match (`FLIP_ARCS`, `Material::Unchecked`). All `cfg(test)`, not shipped. | A test-only `Writer` constructor. | None |

### Stage 4: decisions for the owner

All seven were decided on 2026-11-18. Each entry gives the background and
then the decision.

1. **Format version policy.** *Decided 2026-11-18: one format version per
   release or phase.* `CURRENT_VERSION` is 28. Reviewer-reported (not
   re-checked by me): only formats 1, 7, and 19 shipped in a tagged
   release, and phase 3 has bumped the format nine times unreleased.
   Still open: whether to drop the migrations for formats that never
   shipped (about −250 lines, breaks dev-era files).
2. **Tab requests in the generator.** ADR-0005 made tabs placed objects
   on the project. A tab request (`count` or `at_distances` on an
   operation) is the older form, and every shell converts requests to
   placed tabs before generating. The Profile generator still lays
   requests out itself (`plan_tabs`, `tabs::Request`), and one production
   caller still relies on that: the Inlay cut-out, which builds a
   synthetic Profile carrying the cut-out's request
   (`dc-cam/src/inlay.rs:1275`). *Decided: finish ADR-0005.* Place the
   cut-out's tabs before generation like every other Profile, and make the
   generator accept placed tabs only. That removes 200 to 300 lines and
   three warning paths, and it is where finding 1.13 gets fixed.
3. **Who owns which check.** Validation runs in dc-project, again in each
   generator, and again in dc-gcode. The last is by design. The middle
   layer is the question: its checks fire only when a generator is called
   without validating first, which in practice means tests. *Decided: keep
   the generator checks but in one copy, `OpContext::resolve` (item 2.5).*
   A validated-project type that removes them by construction is
   deferred until stage 2 is done, and needs a table of which layer owns
   which variant first.
4. **How independent the verifier should be.** The checker (3,700 lines)
   is now larger than the emitter (3,600), shares its transform, cycle
   model, and arc helpers, and gains a `Material` variant per generator
   tweak. *Decided: hold the line.* Keep the cheap modal rules (safe Z,
   depth, feeds, spindle). Stop growing S9: a new operation kind gets a
   coarse envelope check (inside the stock, no deeper than configured)
   instead of replicated geometry. Correct the independence claim in
   `safety.rs:9`. The simulators become the independent oracle once they
   run in CI (stage 0.1). The trade-off accepted: new operations lose the
   fine-grained S9 check and rely on the simulators and goldens.
5. **CAM code in the wrong crates.** dc-geometry holds `BitProfile` and
   inlay band logic (about 1,300 lines) against its own guideline;
   dc-project holds about 1,150 lines of inlay geometry because validation
   needs it. *Decided: move neither now.* Amend the guideline to describe
   what is true. The dc-geometry move is safe (verbatim, compiler-checked)
   but touches imports in about 23 files and would conflict with phase 3
   branches; do it after the phase 3 tag, if at all. The dc-project code
   cannot go to dc-cam without a dependency cycle, so moving it means
   splitting validation into schema and geometric checks, which changes
   where errors surface (`save()` runs full validation today). Do that
   only as part of item 3.3, where it pays for itself.
6. **The medial axis.** It is PureCutCNC's sampled construction (boundary
   samples, a Delaunay triangulation, circumcentres as the centreline)
   with DreamCarve's own corner repairs on top, each from an issue, on
   unnamed thresholds. An exact segment-Voronoi construction would need
   no repairs but would change every V-carve, break parity with the
   PureCutCNC goldens, and go against "port, don't invent". *Decided: keep
   the port and name the constants now.* Revisit only if corner-repair
   issues keep arriving after phase 3.
7. **VCarve named in comments.** About 33 comment lines in dc-cam,
   dc-project, and dc-job name VCarve as the reference for a behaviour
   (`profile.rs:101`, `lead.rs:28`, `relief.rs:44`, `model.rs:88`,
   `model.rs:899`). They read as references to documented, user-facing
   behaviour, not reverse engineering, but they sit badly beside the rule
   in AGENTS.md. *Decided:* keep the citations but move them to
   `docs/research` with URLs; reword the source comments to describe the
   behaviour itself; add a sentence to AGENTS.md that public documentation
   may be cited as prior art and product output may not. Still open, for
   the owner: confirm where the T-bone tie-break (`relief.rs:44`, `:180`)
   came from, since it is finer than a manual usually states.

## Process changes to stop regrowth

1. **Comments:** no issue numbers, slice names, or review rounds in
   comments, except `TODO(#n)`. Add a check to `check-headers` or CI.
2. **Size budgets:** set `too-many-lines-threshold` in `clippy.toml` (150
   is achievable after stage 3) and enable `clippy::too_many_lines`.
3. **Lints worth adopting:** `redundant_clone`, `match_same_arms`,
   `needless_pass_by_value`. Do **not** adopt `suboptimal_flops` (966
   hits): `mul_add` changes floating-point results and would change
   emitted coordinates.
4. **Indexing:** give the `indexing_slicing` rule a sanctioned escape that
   fails loudly: an internal-defect error variant per crate, and a rule
   that a lookup miss is never `continue` or a default in generator or
   verifier code.
5. **Tests:** tests live in `tests.rs`, not in the source file. A new
   variant of an existing case extends a table. Shared builders come from
   `test_support`.
6. **Refactor step:** the "Refactor and document" step in `docs/plan.md`
   gains one question: did this change add a parallel copy of something
   that exists?

## What is good and should be left alone

- `unsafe` is forbidden; no panics, printing, or I/O in library crates;
  units are in names throughout; every `allow` carries a reason.
- Import hardening: PNG and JPEG decoder limits, fallible allocation,
  usvg resolvers that do no file I/O, DXF nesting and visit budgets,
  finite checks on every coordinate.
- Output: atomic staged writes with rollback, export refusing to overwrite
  the project, `sanitize_comment` on everything reaching G-code.
- `sequence.rs` as the single order-and-link step with self-checks.
- The faithful upstream ports: `entry.rs`, `lead.rs`, `pocket/rings.rs`,
  `pocket/raster.rs`.
- `dc-job` as the one pipeline for CLI and GUI; export rebuilding from
  scratch instead of trusting the preview cache.
- Number formatting in one 43-line module; arc fitting falling back per
  run.
- The PureCutCNC golden harness, the 118 safety negative tests, the
  simulators, the stress and seat properties, and `quick_start.rs`, which
  executes the book.
- The 1.8:1 test-to-code ratio. For G-code that drives machines it is
  defensible. Consolidate tests; do not cut safety coverage.

## Expected result

| | Now | After stages 2 and 3 |
| --- | ---: | ---: |
| Production code | 76.6k | 69k to 71k |
| Production comments | 26.7k | 22k to 23k |
| Test code and comments | 135k | 120k to 125k |
| Functions over 200 lines | 14 | 0 to 2 |
| Largest source file | 10.9k lines | under 4k |

The line count drops by about a tenth. The larger gains are that a lookup
miss becomes an error instead of a missing pass, the simulators and sweeps
run on every change, and a new warning or preference is a one-place edit.

## Tooling that would sharpen the next pass

None were needed to produce this plan, but each turns a judgement above
into a measurement. `tools/install-dev-tools.sh` installs them (machete
for unused dependencies, samply as the profiler); run it with `--check` to
see what a machine has.

| Tool | Use |
| --- | --- |
| cargo-llvm-cov | Coverage per crate and per test binary (0.3). |
| cargo-mutants | Which tests actually catch a broken safety rule (0.4). |
| cargo-fuzz | Importer and loader fuzzing (0.5). |
| cargo-nextest | Per-test timing; the cost of 65 binaries and 566 spawns. |
| cargo-machete or cargo-udeps | Unused dependencies. |
| cargo-deny (local) | `deny.toml` exists but runs only in CI. |
| tokei or scc | Exact line classification. |
| ast-grep | Structural search for the `.get(..)` fail-silent idiom (1.1, 1.2). |
| A profiler (samply, cargo-flamegraph) | Confirm 3.3 to 3.5 before changing them. |
