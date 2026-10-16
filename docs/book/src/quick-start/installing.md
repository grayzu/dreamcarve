# Installing

DreamCarve is one command line program, `dreamcarve`. It runs on Windows,
macOS, and Linux.

## Prebuilt binaries

Tagged releases publish archives for Linux x86_64, macOS (Intel and Apple
silicon), and Windows x86_64 on the
[releases page](https://github.com/grayzu/dreamcarve/releases), with shell
and PowerShell installer scripts that put `dreamcarve` on your `PATH`.

The first release that can write G-code is `v0.1.0`. Until it is published,
the releases are scaffolding only, so build from source.

## Build from source

### Prerequisites

- [Rust](https://rustup.rs) 1.94 or newer, installed through rustup, which
  also provides `cargo`, `rustfmt`, and `clippy`.
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

prints `dreamcarve` followed by the version.

```sh
dreamcarve posts
```

lists the controllers DreamCarve can write G-code for:

```text
grblhal  grblHAL
grbl     GRBL 1.1
```

Next: [Your first cut from an SVG](first-cut.md).

## Run the tests

If you built from source, you can run the test suite:

```sh
cargo test --all
```

A fresh clone passes on all three platforms. If it does not, that is a bug;
please open one with the Bug template.
