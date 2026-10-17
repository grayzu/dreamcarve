# Getting started

This chapter is for anyone, human or AI agent, who wants to change DreamCarve.

## Read first

1. `AGENTS.md` at the repository root: the rules every change follows,
   including licensing, coding standards, and the build gate.
2. `docs/plan.md` (the [Roadmap](../roadmap.md)): where the project is and
   what the current phase delivers.
3. `docs/specs/phase-N/` for the current phase: the functional spec says what,
   the design spec says how and where.
4. `CONTEXT.md`: the glossary. Code, tests, and issues use its terms.
5. `docs/adr/`: decisions that constrain every phase, starting with ADR-0001
   (Y-up internal coordinates), ADR-0002 (own project format), and ADR-0003
   (heightfield-only 3D).
6. `docs/rust-guidelines.md`: Rust patterns the code follows.

## Set up

Follow [Installing: Build from source](../quick-start/installing.md#build-from-source). Then install
the two extra tools the build gate uses:

```sh
cargo install cargo-deny
cargo install mdbook
```

(`brew install cargo-deny mdbook` on macOS also works.)

## The build gate

Before committing, all of these pass:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo run -p check-headers
cargo deny check licenses advisories
mdbook build docs/book
```

CI runs the same commands on Windows, macOS, and Linux for every pull
request. `cargo test` includes `quick_start_runs_as_written`, which runs the
[quick start](../quick-start/first-cut.md) as written; if you change a
command's behaviour or output file, update that chapter in the same pull
request.

## Picking up work

Work is tracked in GitHub Issues. An issue labelled `ready-for-agent` has a
**Tests** section naming the tests to write and the crate they belong in.
Start there: the first commit of your pull request adds those tests, failing.
See [How work flows](how-work-flows.md).
