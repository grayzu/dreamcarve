# Installing

DreamCarve is two programs that share one engine: the app,
`dreamcarve-app`, a window you work in with mouse and keyboard, and the
command line program, `dreamcarve`, for scripts and for working in a text
editor. Both read and write the same project files and write the same
G-code. They run on Windows, macOS, and Linux.

## Prebuilt binaries

Tagged releases publish archives of both programs for Linux x86_64, macOS
(Intel and Apple silicon), and Windows x86_64 on the
[releases page](https://github.com/grayzu/dreamcarve/releases), with shell
and PowerShell installer scripts that put the program on your `PATH`.

The app is in the releases from `v0.4.0-alpha.2`; the newest is
`v0.6.0-alpha.1`, a pre-release for testers. GitHub's "latest" link skips
pre-releases, so name the version. To install the app on macOS or Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/grayzu/dreamcarve/releases/download/v0.6.0-alpha.1/dreamcarve-app-installer.sh | sh
```

On Windows, in PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/grayzu/dreamcarve/releases/download/v0.6.0-alpha.1/dreamcarve-app-installer.ps1 | iex"
```

Then open a new terminal and run `dreamcarve-app`. The command line
program installs the same way: put `dreamcarve-installer` where the lines
above have `dreamcarve-app-installer`. Each installs one program to
Cargo's `bin` folder (`~/.cargo/bin`, or `%USERPROFILE%\.cargo\bin` on
Windows) and neither needs Rust.

The first release that can write G-code is `v0.1.0`. The latest release
that is not a pre-release has the command line program alone; for it, use
`https://github.com/grayzu/dreamcarve/releases/latest/download/` in place
of the versioned address.

### Downloading an archive by hand

Each archive on the releases page holds the one program, the licence
files and this project's changelog, and has a `.sha256` file to check the
download against. Unpack it and run the program from a terminal, or move
it to a folder on your `PATH`.

The programs are not signed, so a copy downloaded with a browser is
stopped the first time:

- **macOS** says the program cannot be opened because Apple could not
  verify it. Open System Settings, Privacy & Security, and choose *Open
  Anyway* beside its name; or clear the mark the browser put on it:

  ```sh
  xattr -d com.apple.quarantine ./dreamcarve-app
  ```

- **Windows** shows "Windows protected your PC". Choose *More info*, then
  *Run anyway*.

The installer scripts above download the same archives without that
mark. On Linux the app needs a desktop with Vulkan or OpenGL drivers and
glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36).

## Build from source

### Prerequisites

- [Rust](https://rustup.rs) 1.95 or newer, installed through rustup, which
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

## The app

A release from `v0.4.0-alpha.2` has the app ready built, above. To build
it from source instead, with the prerequisites above, in the checkout:

```sh
cargo install --locked --path crates/dreamcarve-app
```

This builds `dreamcarve-app` and copies it to the same folder as
`dreamcarve`. Start it from a terminal:

```sh
dreamcarve-app
```

It opens a window on the start screen. `dreamcarve-app plaque.dcv` opens
a project, and `dreamcarve-app --version` prints the version and exits.
To run it without installing, use `cargo run --release -p dreamcarve-app`.

The app keeps your tool library where the command line keeps it, so a
tool you add in one is there in the other. See
[Tool library](../user-guide/tool-library.md).

Next: [From SVG to G-code in the app](app.md).

## Check it runs

```sh
dreamcarve --version
```

prints `dreamcarve` followed by the version. A release prints its version
alone, for example `dreamcarve 0.2.0`. A build from source prints the
version it is working toward, then `-dev.` and the time it was built in UTC
as `YYYYMMDDHHMMSS`, then the commit it was built from, for example
`dreamcarve 0.2.1-dev.20261018234512+g1402e19`: built 2026-10-18 at
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

Next: [From SVG to G-code in the app](app.md), or the same part from the
command line in [Your first cut from an SVG](first-cut.md).

## Run the tests

If you built from source, you can run the test suite:

```sh
cargo test --all
```

A fresh clone passes on all three platforms. If it does not, that is a bug;
please open one with the Bug template.
