# Licensing

DreamCarve is licensed under the Apache License 2.0. It is a derivative work
of PureCutCNC, also Apache-2.0, which creates obligations under section 4 of
the license. `AGENTS.md` states them as rules; this page is the explanation.

## Headers

Every `.rs` file opens with one of two headers. New files:

```rust
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
```

Files ported or adapted from PureCutCNC keep the upstream copyright and
record the modification, as section 4(b) and 4(c) require:

```rust
// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/path/to/original.ts
```

`cargo run -p check-headers` enforces both forms and fails CI on any file
without one.

## NOTICE

The `NOTICE` file carries PureCutCNC's attribution and the third-party
notices our dependencies require. It stays intact. When adding a dependency
whose license asks for attribution, add it to `NOTICE` in the same change.

## Dependencies

`cargo deny check licenses` allows Apache-2.0, MIT, BSD, Zlib, ISC, and
Unicode-3.0. GPL code is never compatible. LGPL needs a separate review
before use. The allowed list is in `deny.toml`.

The same holds for code read while porting. A GPL program may still be a
behaviour reference, as RasterCarve is for Photo V-Carve: its
documentation and its output on our own inputs may be used, its source
may not ([What is not a port](porting.md#what-is-not-a-port)).

## Bundled fonts

The Text tool's fonts are compiled into `dc-text` from
`crates/dc-text/fonts/`. Each is open-licensed, as its publisher released
it, with its licence in its folder; `fonts/README.md` gives each file's
source, version and checksum, and `NOTICE` lists them all. Most are under
the SIL Open Font License 1.1, which lets a font be bundled and embedded
with software and asks that its copyright notice and licence travel with
it; Roboto Slab is under the Apache License 2.0. A font is added only
with its licence read first: one that forbids bundling or embedding stays
out, and a font with a Reserved Font Name is never modified. The
interface's own typefaces, under `crates/dreamcarve-app/assets/`, follow
the same rules.

The app never reads the system's fonts. A font the user adds is copied
to the app's own fonts folder and stays the user's: it is not bundled,
and a project carries the outline it made, not the font.

## Interoperability, not reverse engineering

Reading a data file a user owns is interoperability. DreamCarve imports
formats whose owners have published their structure: SVG, DXF, STL, Fusion
tool libraries. Formats with no published structure stay out, and nothing in
DreamCarve is derived from inspecting a proprietary product.
