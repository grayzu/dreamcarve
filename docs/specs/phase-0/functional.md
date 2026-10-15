# Phase 0: Scaffold — Functional Spec

**Status:** accepted

## Goal

A contributor, human or agent, can clone the repository, run one command to
verify it, open a slice issue, and land a test-first PR with every gate
enforced automatically. Nothing cuts anything yet.

## Users and scenarios

- **A contributor** clones the repo on Windows, macOS, or Linux, runs
  `cargo test --all`, and it passes. They read `docs/book/` to learn the
  crate map and how work flows.
- **An agent** picks up a `ready-for-agent` issue, finds a Tests section, and
  knows exactly which crate the tests go in.
- **The maintainer** pushes a tag and gets binaries for three platforms and a
  published docs site without touching a build machine.

## Capabilities

1. The repository is a Cargo workspace with the phase 1 crates present and
   compiling, each with its license header and a one-line crate doc.
2. CI runs `cargo fmt --check`, `cargo clippy -D warnings`, and
   `cargo test --all` on Windows, macOS, and Linux for every PR.
3. CI fails a PR when any `.rs` file lacks the DreamCarve or upstream license
   header.
4. Pushing a `v*` tag builds release binaries for all three platforms and
   attaches them to a GitHub release.
5. The mdBook site builds on every PR and publishes from `main` to GitHub
   Pages, with the roadmap, contributor guide, and an empty user guide.
6. GitHub has issue templates for a slice (with a required Tests section), a
   bug, and a phase spec, and a PR template with the review checklist.
7. `tools/parity/` contains a runnable script that loads one PureCutCNC
   fixture and writes a golden G-code file into `tests/golden/`.
8. `CONTEXT.md` and ADRs 0001–0003 exist and the contributor guide points at
   them.

## Out of scope

Any geometry, CAM, G-code, or import behaviour. Those start in phase 1.

## Acceptance tests

1. Fresh clone on each OS: `cargo test --all` exits 0.
2. A PR that removes a license header fails CI with a message naming the
   file.
3. A PR with a `rustfmt` violation fails CI.
4. Tag `v0.0.1`: the release has one archive per platform containing a
   `dreamcarve` binary that prints its version.
5. The docs site URL serves the roadmap within ten minutes of a merge to
   `main`.
6. Running the parity script against `../purecutcnc` produces
   `tests/golden/smoke/program.nc` and a second run produces an identical
   file.

## Documentation deliverables

- `docs/book/`: Introduction, Roadmap (includes `docs/plan.md`),
  Contributing: Getting started, Crate map, How work flows, Porting from
  PureCutCNC, Licensing.
- Quick start: "Build DreamCarve from source".

## Exit criteria

- [x] All acceptance tests pass.
- [x] `v0.0.1` tagged with release artifacts.
- [x] Docs site live.
- [x] Phase 1 spec PR opened.

## Results

Phase 0 closed 2026-10-15.

| # | Acceptance test | Evidence |
| --- | --- | --- |
| 1 | Fresh clone passes on each OS | PR #2 CI: `check` green on ubuntu, macOS, Windows |
| 2 | Missing header fails, naming the file | `check-headers` with a header removed from `dc-cam/src/lib.rs` fails and prints that path; CI runs the same command |
| 3 | `rustfmt` violation fails | `cargo fmt --all -- --check` exits 1 on a misformatted line; CI runs the same command |
| 4 | Release archives print the version | v0.0.1 release: four archives, checksums verified, each contains `dreamcarve`; macOS arm64 and x86_64 (Rosetta) print `dreamcarve 0.0.1` |
| 5 | Docs site serves the roadmap | https://grayzu.github.io/dreamcarve/ and `/roadmap.html` return 200 after the merge-to-main deploy |
| 6 | Parity capture is reproducible | Two captures of `smoke` are byte-identical (`diff -r`) |

The phase 1 specs landed as drafts in PR #1.
