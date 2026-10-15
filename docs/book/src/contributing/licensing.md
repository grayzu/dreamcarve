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

## Interoperability, not reverse engineering

Reading a data file a user owns is interoperability. DreamCarve imports
formats whose owners have published their structure: SVG, DXF, STL, Fusion
tool libraries. Formats with no published structure stay out, and nothing in
DreamCarve is derived from inspecting a proprietary product.
