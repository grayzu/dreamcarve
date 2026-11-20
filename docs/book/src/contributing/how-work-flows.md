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
   does. Golden comparisons against the [golden corpus](golden-corpus.md)
   (PureCutCNC's and RasterCarve's programs) are part of the
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

## Testing the app

The app is tested at two levels, and neither needs a person at a screen.

- **Headless**, on `dc_app::App` through `dc_app::test_support::Harness`:
  commands run, the pending toolpaths are generated on the test thread,
  and the programs are read back. `crates/dreamcarve/tests/gui_equality.rs`
  compares them byte for byte with what the `dreamcarve` binary writes.
- **In the window**, with egui's kittest harness, in
  `crates/dreamcarve-app/tests/`: a test finds a control by its
  accessible role and label, clicks or types, and reads the result from
  the app's state. `kittest.rs` holds the behaviour tests; they run with
  no GPU.

## Screenshots in the book

Every picture of the app in this book is written by a test, from the
build, and never drawn or edited by hand. The tests are in
`crates/dreamcarve-app/tests/book.rs`. They are ignored in a normal run,
because they write files and need a GPU adapter; run them after a change
to the window or to a chapter's steps, and commit the pictures:

```sh
cargo test -p dreamcarve-app --test book -- --ignored
```

The pictures go to `figures/app/` beside the chapters that show them, at
2× in the dark appearance. `docs::book_builds_and_every_screenshot_exists`
fails when a chapter names a picture that is not there, or a picture is
left that no chapter shows.

The quick start [From SVG to G-code in the app](../quick-start/app.md)
goes further: its values are in tables marked with `<!-- app-test: … -->`
comments, which `crates/dreamcarve/tests/app_steps` reads.
`gui_equality::quick_start_parts_from_the_app` builds the four parts from
those tables and compares the programs with the command-line quick
starts', and `book.rs` drives the window with the same values to take
the chapter's pictures. Change a value in the chapter and both follow, or
fail.
