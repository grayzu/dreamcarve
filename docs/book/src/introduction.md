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
[Roadmap](roadmap.md) lists them. Phase 0 built the scaffold: a Cargo
workspace, continuous integration on Windows, macOS, and Linux, releases,
and this site. Phase 1 brought the first G-code: a command line tool that
imports an SVG and writes a Profile toolpath, with tabs, for a
grblHAL or GRBL machine. Phase 2 adds every 2.5D operation except V-Carve
(Pocket, Drill, Engrave, and Profile entries, leads and corner relief), DXF
import, a tool library with Fusion import, and six controllers. Phase 3,
in progress, adds V-Carve: lettering and signs carved with a V-bit, with
flat floors an end mill can clear first, countersinks, and V-bit inlays
of one wood in another. Start with
[Your first cut from an SVG](quick-start/first-cut.md), then
[A pocketed plaque from DXF](quick-start/plaque.md),
[A V-carved sign from SVG outlines](quick-start/sign.md) and
[An inlay pair](quick-start/inlay.md). The
[Changelog](changelog.md) lists what changed in each release.

## Who this site is for

- **Users** start at the Quick start and the User guide. The user guide fills
  in as features land; a feature is not done until it has a chapter here.
- **Contributors**, human or AI agent, start at
  [Contributing: Getting started](contributing/getting-started.md).

## API documentation

Rustdoc for every crate is published alongside this book at
[`/api/`](api/dc_geometry/index.html).
