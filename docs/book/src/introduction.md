# Introduction

DreamCarve is an open source CAD/CAM application for CNC hobbyists, written in
Rust and licensed Apache-2.0. It aims at the workflow a VCarve user expects:
draw or import 2D shapes, pick tools, add Profile, Pocket, Drill, V-Carve, and
Engrave operations, preview the cut, and export G-code for a GRBL or grblHAL
controller.

DreamCarve is a derivative work of
[PureCutCNC](https://github.com/PureCutCNC/purecutcnc), a TypeScript CAM
application also under Apache-2.0. Its algorithms are the reference for ours,
and its outputs form the golden corpus our tests compare against.

## Where the project is

Development runs in phases, each ending in a working product. The
[Roadmap](roadmap.md) lists them. Phase 0 is the scaffold: a Cargo workspace,
continuous integration on Windows, macOS, and Linux, releases, and this site.
Nothing cuts anything yet. Phase 1 brings the first G-code: a command line
tool that imports an SVG and writes a Profile toolpath for a grblHAL machine.

## Who this site is for

- **Users** start at the Quick start and the User guide. The user guide fills
  in as features land; a feature is not done until it has a chapter here.
- **Contributors**, human or AI agent, start at
  [Contributing: Getting started](contributing/getting-started.md).

## API documentation

Rustdoc for every crate is published alongside this book at
[`/api/`](api/dc_geometry/index.html).
