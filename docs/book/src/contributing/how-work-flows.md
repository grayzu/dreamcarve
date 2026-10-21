# How work flows

Spec first, then test first. Each step has a visible artifact, so anyone can
see where a piece of work stands.

1. **Phase spec.** A phase opens with a functional spec (what, for whom,
   acceptance tests, safety requirements, test part) and a design spec (data
   model, algorithms, module layout, test plan, slices) in
   `docs/specs/phase-N/`. They land in one pull request and are reviewed
   before anything else happens.
2. **Slice issues.** Each slice in the design spec becomes a GitHub issue
   using the Slice template. Its **Tests** section lists test names and the
   assertion each makes, and names the crate they go in. An issue without
   one is labelled `needs-info`; a complete one is `ready-for-agent` or
   `ready-for-human`.
3. **Red.** The implementation pull request's first commit adds the listed
   tests. They fail. Tests ported from PureCutCNC count, and keep the
   upstream header.
4. **Green.** Following commits make them pass with the smallest change that
   does. Golden comparisons against the PureCutCNC corpus are part of the
   suite.
5. **Gate.** `cargo fmt`, `cargo clippy -D warnings`, `cargo test --all`,
   `cargo run -p check-headers`, `cargo deny` and `mdbook build docs/book`
   pass in CI, the Rust gates on all three platforms and on the toolchain
   `rust-toolchain.toml` pins (see
   [Getting started](getting-started.md#the-pinned-toolchain)).
6. **Docs.** Any user-visible change updates this book in the same pull
   request. A feature without a chapter is not done.
7. **Real cut.** Phases that change machine output (1, 2, 3, 7) end with a
   test part cut on a grblHAL machine, and the functional spec records the
   result. The phase's quick start is written on that test part, and runs
   as a test, before the tag.

A phase is tagged `v0.N.0` when its exit criteria are met.
