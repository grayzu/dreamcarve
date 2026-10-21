# Installing

DreamCarve is one command line program, `dreamcarve`. It runs on Windows,
macOS, and Linux.

## Prebuilt binaries

Tagged releases publish archives for Linux x86_64, macOS (Intel and Apple
silicon), and Windows x86_64 on the
[releases page](https://github.com/grayzu/dreamcarve/releases), with shell
and PowerShell installer scripts that put `dreamcarve` on your `PATH`.

The first release that can write G-code is `v0.1.0`. To install the latest
release on macOS or Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/grayzu/dreamcarve/releases/latest/download/dreamcarve-installer.sh | sh
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/grayzu/dreamcarve/releases/latest/download/dreamcarve-installer.ps1 | iex"
```

Each archive on the releases page also has a `.sha256` file to check a
manual download against.

## Build from source

### Prerequisites

- [Rust](https://rustup.rs) 1.94 or newer, installed through rustup, which
  also provides `cargo`, `rustfmt`, and `clippy`. Inside the checkout, rustup
  uses the release `rust-toolchain.toml` pins and downloads it on first use.
- Git.

### Build and install

```sh
git clone https://github.com/grayzu/dreamcarve.git
cd dreamcarve
cargo install --locked --path crates/dreamcarve
```

This builds a release binary and copies it to Cargo's `bin` folder
(`~/.cargo/bin`, or `%USERPROFILE%\.cargo\bin` on Windows), which rustup put
on your `PATH`.

To build without installing, run `cargo build --release`. The binary is then
at `target/release/dreamcarve` (`dreamcarve.exe` on Windows).

## Check it runs

```sh
dreamcarve --version
```

prints `dreamcarve` followed by the version. A release prints its version
alone, for example `dreamcarve 0.1.0`. A build from source prints the
version it is working toward, then `-dev.` and the time it was built in UTC
as `YYYYMMDDHHMMSS`, then the commit it was built from, for example
`dreamcarve 0.1.0-dev.20261018234512+g1402e19`: built 2026-10-18 at
23:45:12 UTC from commit `1402e19`. A later build has a larger time. A build
from a source archive has no commit to name and leaves off the `+g` part.

```sh
dreamcarve posts
```

lists the controllers DreamCarve can write G-code for:

```text
grblhal   grblHAL
grbl      GRBL 1.1
linuxcnc  LinuxCNC
mach3     Mach3
uccnc     UCCNC
generic   Generic RS274/NGC
```

Next: [Your first cut from an SVG](first-cut.md).

## Run the tests

If you built from source, you can run the test suite:

```sh
cargo test --all
```

A fresh clone passes on all three platforms. If it does not, that is a bug;
please open one with the Bug template.
