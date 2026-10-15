<!--
Copyright 2026 The DreamCarve Contributors
SPDX-License-Identifier: Apache-2.0
-->
## What

<!-- One or two sentences. Link the slice issue: Closes #N -->

## Checklist

- [ ] Tests from the issue's **Tests** section landed in the first commit and failed before the implementation.
- [ ] Every new `.rs` file has the DreamCarve header; ported files keep the PureCutCNC header and name the source file.
- [ ] If toolpath generation or G-code emission changed: emitted-motion safety assertions are present and the golden corpus still passes.
- [ ] `docs/book/` updated for any user-visible change.
- [ ] `CONTEXT.md` updated if a term was added or changed.
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `cargo test --all`, `cargo run -p check-headers` pass locally.
