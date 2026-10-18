# Rust guidelines

How DreamCarve code is written. **Must** rules are checked in review and, where
a lint exists, by CI. **Prefer** rules are the default; depart from one only
with a comment saying why. Vocabulary comes from `CONTEXT.md`; coordinates,
file format, and 3D model from ADRs 0001–0003.

The rules exist for three reasons that recur below: the output drives a real
machine, so **no panic and no invented value**; results are compared against a
golden corpus, so **determinism**; the engine is a library the CLI and later
the GUI both call, so **pure core, thin shell**.

## Toolchain and workspace

**Must**

- Stable Rust, 2024 edition. `rust-version` in the workspace `Cargo.toml` is
  the only MSRV statement; bump it deliberately, in its own commit.
- One `[workspace.lints]` table, inherited by every crate with
  `lints.workspace = true`. Library crates (`dc-*`): `unsafe_code = "forbid"`,
  `clippy::unwrap_used = "deny"`, `clippy::expect_used = "deny"`,
  `clippy::panic = "deny"`, `clippy::indexing_slicing = "warn"`,
  `clippy::cast_possible_truncation = "warn"`, `clippy::float_cmp = "deny"`,
  `missing_docs = "warn"`. Tests and the `dreamcarve` binary may
  `#[allow]` the unwrap, expect, and panic lints at module level with a
  one-line reason.
- `cargo fmt` with the default `rustfmt.toml`; no per-file formatting
  attributes.
- Dependency versions live in `[workspace.dependencies]` once; crates use
  `dep.workspace = true`. `Cargo.lock` is committed.

**Prefer**

- `#![doc = include_str!("../README.md")]` on each `dc-*` crate root so the
  crate's README is also its rustdoc front page.

## Crate boundaries

Dependency direction is one way:

```
dc-geometry → dc-project → { dc-cam, dc-import } → dreamcarve (CLI, later GUI)
dc-project → dc-cam → dc-gcode → dreamcarve
```

`dc-gcode` depends on `dc-cam` for the `Toolpath` type it writes out, and for
nothing else.

**Must**

- A lower crate never imports a higher one. `dc-geometry` knows nothing about
  tools or stock; `dc-cam` knows nothing about G-code text; `dc-gcode` uses
  only `dc-cam`'s toolpath data types, never its generators, and knows nothing
  about how a toolpath was generated.
- Library crates do no file, terminal, clock, or network I/O. They take
  `&str` or `&[u8]` and return values. `dreamcarve` owns `std::fs`, `std::env`,
  exit codes, and all printing. (`dc-project::load` parses bytes; the binary
  reads the file.)
- Libraries report problems as values: `Err(E)` when the result cannot be
  trusted, `Vec<Warning>` alongside an `Ok` when it can. No `println!`,
  `eprintln!`, or logging macros in `dc-*`.
- Nothing GUI-specific (egui, wgpu, winit types) appears in a `dc-*` crate.
  The GUI adapts engine types at its own edge.

## Errors and warnings

**Must**

- Each `dc-*` crate exposes one `thiserror` error enum at its public boundary
  (`GeometryError`, `LoadError`, `CamError`, `ExportError`, `ImportError`).
  Wrap a lower crate's error with `#[from]`; never flatten it to a string.
- Error variants carry what the user needs to fix the problem: the tool name
  and the missing field, the operation name, the contour index, the byte
  offset in a file. A variant with no fields is only for conditions that need
  none.
- `anyhow` is allowed in the `dreamcarve` binary only, to add context at the
  edge and print one message.
- A **warning** is something the user should know but that still yields safe
  G-code (an unused tool, a tab wider than a short edge, a reduced heightmap).
  An **error** is anything that would make emitted motion wrong or unsafe
  (missing feed, depth below stock without allowance, unclosed contour for a
  pocket). When in doubt it is an error.
- Export preconditions are checked in `dc-gcode::emit` and fail as
  `ExportError`, whatever upstream code already checked. The post-processor is
  the last line and does not trust its callers.

**Prefer**

- `Result<T, E>` type aliases per crate (`pub type Result<T> = core::result::Result<T, CamError>;`)
  only when a module returns one error type everywhere.
- `impl Display` messages are one sentence, no trailing period, start with the
  subject ("tool `Set me` has no feed rate").

## Types

**Must**

- Units are in the name: `depth_mm`, `feed_mm_min`, `rpm`, `angle_deg`,
  `step_over_fraction`. A bare `f64` with no unit in its name is a bug.
- `Option<T>` means "not set". Sentinels (`0.0`, `-1`, `NaN`, empty string)
  never stand in for absence. In particular, a tool's feed, plunge, and RPM
  are `Option` and stay `None` until a user supplies them.
- IDs are newtypes (`DefinitionId`, `InstanceId`, `ToolId`, `OperationId`),
  never raw `String` or `u32`, so they cannot be mixed up.
- Two-way choices are enums, not `bool`: `Side::{Inside, Outside, OnLine}`,
  `CutDirection::{Climb, Conventional}`, `TabShape::{Rect, Smooth}`. The call
  site must read without consulting the signature.
- Geometry is `f64`. `f32` appears only in GPU vertex buffers at the renderer
  edge.
- Invariants are enforced by constructors, and fields that carry an invariant
  are private. `Contour::new` normalises winding (outer CCW, hole CW,
  ADR-0001); `Polyline::closed()` is a method on a checked value. A `pub`
  field is a promise that any value is valid.
- Every model type in `dc-project` derives `Debug, Clone, PartialEq,
  Serialize, Deserialize`. Snapshot undo relies on cheap, correct `Clone`.
- Collections that are serialized, iterated to produce output, or compared in
  tests are ordered (`Vec`, `BTreeMap`, `BTreeSet`). `HashMap` is for private
  lookup tables only.

**Prefer**

- Small `Copy` value types for `Point`, `Vec2`, `Point3`, `Vertex`,
  `Transform`.
- Struct literals with named fields over builders until a type has three or
  more optional parts; then a builder with `build() -> Result`.
- `#[must_use]` on functions whose result is the whole point
  (`offset`, `generate_profile`, `emit`).

## Floating point and determinism

**Must**

- No `==` or `!=` between floats in logic. Use `approx_eq(a, b, EPS_MM)` from
  `dc-geometry` with a named tolerance. The tolerances are constants with a
  doc comment saying what they bound (`EPS_MM`, `FLATTEN_TOLERANCE_DEFAULT_MM`,
  `ARC_FIT_TOLERANCE_MM`).
- Reject non-finite numbers at the boundary: `dc-project::load` and every
  importer fail on `NaN` or `±inf`; after that, code may assume finiteness.
  Bound magnitudes there too, by `dc_geometry::MAX_COORDINATE_MM` (6 m):
  finite but huge input overflows or loses precision deeper in, and
  `cavalier_contours` panics on it.
- Generators and post-processors are pure functions of their inputs: no
  `HashMap` iteration order, no system time, no randomness, no thread-count
  dependence in results. Two runs on two machines produce identical G-code.
- Numbers are formatted for output in one place (`dc-gcode::format`), with a
  fixed decimal count from the post-processor, and negative zero normalised to
  `0`. Tests compare the formatted text.
- Arithmetic that can shed precision (`as u32`, `as usize`, `f64 as f32`) uses
  a checked or saturating helper with a comment on the range it assumes, or
  `TryFrom`.

## Ownership and API shape

**Must**

- Public functions take borrowed inputs (`&Project`, `&[Contour]`, `&str`) and
  return owned results. No `&mut` to a caller's data in the engine; callers
  decide what to replace.
- No `Rc<RefCell<_>>`, no `static mut`, no global state in `dc-*`. State
  travels as parameters.
- No `async` and no runtime in `dc-*`. The GUI runs the synchronous engine on
  a worker thread and passes results back over a channel.

**Prefer**

- `impl Iterator` returns for walks over geometry when the caller may stop
  early; `Vec` when the result is stored or sorted anyway.
- `Cow<str>` is not worth it here; `String` is fine.
- `Arc<T>` for large immutable inputs shared with a worker thread (meshes,
  height grids), introduced when a profiler asks for it.

## Serialization (`.dcv` and post-processor JSON)

**Must**

- `serde` + `serde_json`, pretty-printed, keys in declaration order
  (`preserve_order` feature). A `.dcv` writes each vertex on one line
  through `dc-project`'s formatter; only whitespace differs from
  `to_string_pretty`. Field names are `snake_case` in JSON; enums use
  `#[serde(rename_all = "snake_case")]`. An enum with any unit variant keeps
  serde's default external tagging, so a unit variant reads as a plain string
  (`"bottom_left"`) and a variant with data as a one-key object
  (`{ "custom": { "x_mm": 20.0, "y_mm": 5.0 } }`). An enum whose every
  variant carries data is internally tagged (`#[serde(tag = "kind")]`).
- `#[serde(deny_unknown_fields)]` on every persisted struct, so a typo in a
  hand-edited file fails loudly instead of silently reverting to a default.
- `Option` fields serialize as `null`, never omitted, so a saved file shows
  every parameter a user can set.
- Schema changes follow ADR-0002: bump `version`, add a migration function
  from N−1 to N, add a fixture at `tests/fixtures/dcv/vN-1/*.dcv` that the
  migration test loads. `#[serde(default)]` is not a substitute for a
  migration.
- Round-trip test for every persisted type: `from_str(to_string(x)) == x`.

## Tests

**Must**

- Unit tests live in `#[cfg(test)] mod tests` beside the code; behaviour tests
  that span modules live in `crates/<name>/tests/`; golden comparisons live in
  `tests/golden/` and run under `cargo test --all` with no Node.
- Test names state the behaviour, not the function:
  `profile_outside_climb_is_clockwise`, `export_fails_when_tool_has_no_feed`,
  `offset_of_ccw_contour_stays_ccw`.
- A test whose name does not explain it fully carries a `///` comment above
  `#[test]` saying what it shows and why the expected value is right, citing
  its source: a spec rule (`S4`), a ported PureCutCNC test, or a controller
  fact (grblHAL `gcode.c`). Comments in the body explain single steps.
- Assertions in loops, and any whose failure would not name the case, carry
  a message with the case and the offending value:
  `assert!(z >= floor, "move {i} cuts below the floor: {mv:?}")`.
- Float assertions use `assert_approx_eq!(a, b, tol)` from the shared test
  helper with an explicit tolerance. `assert_eq!` on `f64` is a lint error.
- Every emitted-motion test checks at least: no Z below
  `stock_bottom − through_cut_allowance`, every XY move at cutting depth has a
  feed, and every operation starts and ends at or above safe Z. A shared
  `assert_safe(program, project)` helper carries these; use it rather than
  re-deriving them.
- Golden comparison normalises before diffing (line endings, trailing
  whitespace, the header timestamp line) and prints a unified diff on failure.
  A golden file changes only in a commit whose message says why.
- `#[ignore]` carries a reason and an issue: `#[ignore = "arc fitting lands in
  #42"]`.
- Ported PureCutCNC tests keep the upstream header and cite the source test
  file. Where the Y-up flip changes an expected value, the test comment says
  what changed and why.

**Prefer**

- `proptest` for geometry invariants (offset preserves winding, flatten stays
  within tolerance, `point_at(length) == end`). Keep case counts small enough
  that `cargo test` stays under a minute per crate.
- Fixture builders in a `test_support` module (`coaster_project()`,
  `tool_6_35_upcut()`) over long literal structs in every test.
- No snapshot-testing crate. The golden corpus is the snapshot mechanism, and
  one mechanism is enough.

## Dependencies

**Must**

- Licenses: Apache-2.0, MIT, BSD, Zlib, ISC, Unicode-3.0, or
  MPL-2.0 only when nothing else fits and the file-level copyleft is
  understood. GPL never; LGPL only after the review `AGENTS.md` requires.
  `cargo deny check licenses advisories` runs in CI and is the gate.
- A new dependency is justified in the PR description: what it replaces, why
  std or an existing dependency does not cover it, and its license line in
  `NOTICE` if attribution is required.
- Minimal features: `default-features = false` and list what is used, when
  the crate's defaults pull in I/O, async, or a second allocator.

**Prefer**

- One crate per job: `cavalier_contours` for offsets, `i_overlay` for boolean
  ops, `kurbo` for curves, `usvg` for SVG, `dxf` for DXF, `ttf-parser` for
  fonts, `thiserror`/`anyhow` for errors, `serde`/`serde_json` for files,
  `clap` for the CLI. Introducing a second crate for the same job needs an
  ADR.

## Documentation and comments

**Must**

- Every `pub` item in a `dc-*` crate has a `///` comment. For anything taking
  or returning geometry it states units and, where it matters, winding and
  which side `+delta` grows toward. `missing_docs` is on.
- Each module opens with `//!` saying what it owns and which spec section
  defines it (`docs/specs/phase-1/design.md`, "Profile contour").
- Ported code cites its source in the file header per `AGENTS.md`, and the
  module `//!` notes what was re-derived for Y-up.
- `TODO` and `FIXME` carry an issue: `// TODO(#17): arc fitting`. A bare TODO
  fails review.
- Comments explain why, not what. A comment that restates the next line is
  deleted.

## Performance

**Must**

- Correctness and determinism first. A parallel version of a generator
  (`rayon`) is accepted only with a benchmark showing the gain and a test
  showing identical output to the serial version.
- No optimisation PR without a `criterion` benchmark in `benches/` or a
  profiler trace in the PR description.

**Prefer**

- Pre-size `Vec`s in loops over contours and passes; avoid per-vertex
  allocation.
- Flatten once at import to project tolerance (settled decision) and keep
  polylines with bulge arcs through the pipeline; do not re-flatten in
  generators.

## GUI (phase 4 onward)

**Must**

- The app owns one `Project` and one `History`. Every mutation is a named
  command that produces a new `Project` snapshot pushed to `History`; the UI
  never edits model fields in place.
- The viewport flips Y exactly once, in the world-to-screen transform.
  Nothing else in the GUI knows about Y-down.
- Long work (toolpath generation, simulation, import) runs on a worker thread
  and reports progress and results over a channel. The UI thread never blocks
  on the engine.
- User-visible strings go through the string table from the first GUI phase,
  even while English is the only language.

## Review checklist

Before requesting review on a Rust PR, confirm:

1. The four `AGENTS.md` build commands pass locally on your OS.
2. Tests were committed first and failed for the right reason.
3. New `pub` items have docs with units; new persisted fields have a version
   bump, migration, and fixture.
4. No `unwrap`, `expect`, `panic!`, `println!`, or float `==` in a `dc-*`
   crate outside `#[cfg(test)]`.
5. Any change to toolpath generation or emission ran the emitted-motion
   safety assertions and the golden corpus.
6. New dependencies are justified in the description and pass `cargo deny`.
