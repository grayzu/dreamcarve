# DreamCarve

DreamCarve is an open source CAD / CAM application for CNC hobbyists, written in
Rust. The goal is a single workspace where you draw or import a design, assign
machining operations, preview the toolpaths, and export G-code. Its feature set is
modeled on what hobbyists use in commercial tools such as VCarve.

> **Status:** early alpha. `v0.4.0-alpha.1` is a pre-release for testers: a
> desktop app and a command line program that take an SVG or DXF drawing, or
> a picture, to G-code. Check every program in your sender's preview and with
> an air cut before it touches material, and expect breaking changes.

## What works today

- **Import:** SVG and DXF drawings, and PNG or JPEG pictures
- **Toolpaths:** Profile (with tabs, lead-ins, ramps and dogbones), Pocket,
  Drill, Bore, Engrave, V-Carve, Inlay, and Photo V-Carve
- **Tool library:** end mills, ball noses, V-bits, tapered ball noses and
  engravers, with feeds and speeds, and import of Fusion tool libraries
- **Preview:** the toolpaths over the drawing, coloured by operation or by
  depth, with time estimates
- **Output:** G-code for grblHAL, GRBL 1.1, LinuxCNC, Mach3, UCCNC, and
  generic RS274/NGC

Drawing in the app, 3D carving and the material-removal simulation are
planned; the roadmap is [`docs/plan.md`](docs/plan.md).

## Try it

The [user guide](https://grayzu.github.io/dreamcarve/) has the full
[installing](https://grayzu.github.io/dreamcarve/quick-start/installing.html)
chapter and a quick start that goes from a drawing to G-code in the app.

Install the app on macOS or Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/grayzu/dreamcarve/releases/download/v0.4.0-alpha.1/dreamcarve-app-installer.sh | sh
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/grayzu/dreamcarve/releases/download/v0.4.0-alpha.1/dreamcarve-app-installer.ps1 | iex"
```

Then open a new terminal and run `dreamcarve-app`. The programs are not
signed. The installers above avoid the warning a browser download gets;
if you download an archive from the
[releases page](https://github.com/grayzu/dreamcarve/releases) instead,
the installing chapter says how to get past macOS's and Windows's first
launch check.

Found a problem? Please open an
[issue](https://github.com/grayzu/dreamcarve/issues) with the project file
(`.dcv`) and what you expected.

## Building from source

DreamCarve builds on stable Rust 1.95 or newer. Install it with
[rustup](https://rustup.rs), which picks up the release `rust-toolchain.toml`
pins, then:

```sh
git clone https://github.com/grayzu/dreamcarve.git
cd dreamcarve
cargo run --release -p dreamcarve-app      # run the app
cargo install --locked --path crates/dreamcarve-app   # or install it
cargo install --locked --path crates/dreamcarve       # the command line program
```

To work on the code, `cargo build` and `cargo test` build and test
everything.

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
