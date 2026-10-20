# AGENTS.md — DreamCarve

Instructions for AI coding agents (and humans) working in this repository.
`CLAUDE.md` and other agent entry points defer to this file.

## What this is

DreamCarve is an open source CAD/CAM application for CNC hobbyists, written in
Rust. It aims to provide VCarve-like functionality: 2D design, toolpath
generation (profile, pocket, drill, V-carve, engrave), preview/simulation, and
G-code export. The roadmap is `docs/plan.md`; read it before planning any work.
Each phase has a functional and a design spec in `docs/specs/phase-N/`.
Vocabulary is in `CONTEXT.md`; use its terms in code, tests, and issues.

## Reference implementation: PureCutCNC

DreamCarve is a derivative work of
[PureCutCNC](https://github.com/PureCutCNC/purecutcnc) (TypeScript, Apache-2.0).
A local checkout is usually at `../purecutcnc`. When implementing a feature,
look at how PureCutCNC does it first. Do not design from scratch, and never
reverse-engineer VCarve or any other proprietary product.

- Read `../purecutcnc/INDEX.md` to find the relevant code, then read the
  matching folder's `INDEX.md`.
- Port behavior idiomatically to Rust. Do not transliterate TypeScript patterns
  that don't fit Rust.
- PureCutCNC's own process rules (GitHub board, fast lane, npm gates) apply to
  that repo only. Do not import them here.
- Geometry in PureCutCNC is Y-down; DreamCarve is Y-up (ADR-0001). When
  porting, re-derive every winding, inside/outside, and arc-direction rule
  rather than copying it. The golden corpus in `tests/golden/` is the check.
- Controller-facing data (post-processor definitions: commands, comment
  syntax, number formats, header and footer lines) is verified against the
  controller's official documentation or source, then checked against the
  safety rules. Cite those sources in the definition's README. PureCutCNC is
  where such data starts, not proof that it is right.

## File-format interoperability

Reading a data file a user owns is interoperability, not reverse engineering.
Import formats whose structure the owner has published (Fusion tool libraries,
SVG, DXF, STL). Formats with no published structure stay out, even when the
container is guessable, because inferring them means inspecting a proprietary
product's output. Never decompile, trace, or inspect proprietary software to
learn how it computes anything.

## Licensing rules (required)

DreamCarve is Apache-2.0. Apache-2.0 §4 sets these obligations for derived code:

1. **New files** start with the DreamCarve header:

   ```rust
   // Copyright 2026 The DreamCarve Contributors
   // SPDX-License-Identifier: Apache-2.0
   ```

2. **Files ported or adapted from PureCutCNC** keep the upstream copyright and
   add a modification notice (§4(b), §4(c)):

   ```rust
   // Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
   // Copyright 2026 The DreamCarve Contributors
   // SPDX-License-Identifier: Apache-2.0
   //
   // Ported to Rust and modified from PureCutCNC:
   //   src/path/to/original.ts
   ```

3. Keep `NOTICE` intact. When adding a third-party dependency whose license
   requires attribution, add it to `NOTICE`'s third-party section in the same
   change.
4. Do not copy code from sources that are not Apache-2.0-compatible. GPL code
   is not compatible. LGPL code needs a separate review before use.

## Code layout

Cargo workspace. Library crates are prefixed `dc-`; the CLI binary is
`dreamcarve`. `crates/dc-geometry` (primitives, offsets), `dc-project` (data
model, `.dcv` serialization), `dc-cam` (toolpath generators), `dc-gcode`
(post-processors), `dc-import` (SVG, DXF). `tools/` holds dev-only tools,
`tests/golden/` holds PureCutCNC outputs that `cargo test` compares against.
The design spec of each phase is the authority on where new code goes.

## How work flows

Spec first, then test first. A phase's specs merge before its issues open.
Every implementation issue has a **Tests** section listing test names and
assertions; an issue without one is `needs-info`. The first commit of an
implementation PR adds those tests, failing. Ported PureCutCNC tests count and
keep the upstream header. Full process: `docs/plan.md`, "How work flows".

## Build & verify

Before committing, all of these must pass:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo run -p check-headers
```

## Coding standards

Rust rules are in `docs/rust-guidelines.md`: lints, crate boundaries, error
and warning types, units in names, determinism, serde and migration rules,
test conventions, dependency policy. Read it before writing or reviewing Rust.
The rules that must stay in view on every change:

- Library crates (`dc-*`) never panic, print, or do I/O. Return `Result` with
  the crate's typed error; report non-fatal problems as `Vec<Warning>`.
- Internal geometry is Y-up, Z-up, millimetres (ADR-0001), and units are in
  every name (`depth_mm`, `feed_mm_min`). Importers and renderers flip;
  generators and post-processors never do.
- Geometry and toolpath code must have unit tests. Fix bugs test-first.
- **CNC safety:** G-code output drives real machines. Changes to toolpath
  generation or post-processors need tests that check the emitted moves,
  including safe Z, plunge depth, and feed rates. Never emit motion below the
  configured material bottom. Never invent a feed, plunge rate, or spindle
  speed: export fails when a tool lacks one.
- Every phase ships user docs and a quick start in `docs/book/` before its
  tag. A feature without a chapter is not done.

## Git

- Work on a feature branch, not `main`. Open a PR to merge.
- Use short, imperative commit messages.

## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues on `grayzu/dreamcarve` (via the `gh` CLI). See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `CONTEXT.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.
ADRs 0001–0004 (coordinates, project format, heightfield-only 3D,
post-processor reference) constrain every phase; read them before touching
geometry, serialization, rendering, or post-processors.
