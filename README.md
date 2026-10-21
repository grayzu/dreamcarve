# DreamCarve

DreamCarve is an open source CAD / CAM application for CNC hobbyists, written in
Rust. The goal is a single workspace where you draw or import a design, assign
machining operations, preview the toolpaths, and export G-code. Its feature set is
modeled on what hobbyists use in commercial tools such as VCarve.

> **Status:** early development. Nothing is usable yet. Expect breaking changes.

## Planned features

- **2D design:** lines, arcs, rectangles, circles, polygons, splines, and text,
  with snapping and node editing
- **Import:** SVG and DXF vector artwork, and bitmaps as tracing backdrops
- **Toolpaths:** profile (inside/outside/on-line, with tabs), pocket, drilling,
  V-carve, and engraving
- **Tool library:** end mills, ball noses, V-bits, and engravers, with feeds and speeds
- **Preview:** 2D toolpath view, plus a 3D material-removal simulation
- **Output:** configurable post-processors that emit G-code for common hobby
  controllers (GRBL, Mach3/4, LinuxCNC, and others)

## Building

DreamCarve builds on stable Rust 1.94 or newer. Install it with
[rustup](https://rustup.rs), which picks up the release `rust-toolchain.toml`
pins for the build gate, then run:

```sh
cargo build
cargo test
```

## Contributing

Contributions are welcome. Read [`AGENTS.md`](AGENTS.md) for the project
workflow and coding rules; they apply to human and AI contributors alike.

## Acknowledgements

DreamCarve is a derivative work of
[**PureCutCNC**](https://github.com/PureCutCNC/purecutcnc) by Franja (Frank)
Povazanj, a browser-based 2.5D CAD/CAM application licensed under the Apache
License 2.0. DreamCarve uses PureCutCNC as its reference implementation:
portions of its design, algorithms, and code are adapted and ported to Rust. Many
thanks to the PureCutCNC project and its contributors.

DreamCarve is not affiliated with or endorsed by Vectric Ltd. VCarve is a
trademark of Vectric Ltd., and it is named here only to describe the kind of
functionality DreamCarve aims to provide.

## License

DreamCarve is licensed under the [Apache License, Version 2.0](LICENSE).
Upstream attribution is in [`NOTICE`](NOTICE).

Unless you explicitly state otherwise, any contribution you intentionally submit
for inclusion in DreamCarve is licensed as above, without any additional terms or
conditions.
