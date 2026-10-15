# Build DreamCarve from source

DreamCarve builds with stable Rust on Windows, macOS, and Linux.

## Prerequisites

- [Rust](https://rustup.rs) 1.94 or newer, installed through rustup, which
  also provides `cargo`, `rustfmt`, and `clippy`.
- Git.

## Build

```sh
git clone https://github.com/grayzu/dreamcarve.git
cd dreamcarve
cargo build --release
```

The binary is at `target/release/dreamcarve` (`dreamcarve.exe` on Windows).

## Check it runs

```sh
target/release/dreamcarve --version
```

prints `dreamcarve` followed by the version. In phase 0 that is all the binary
does; phase 1 adds `init`, `export`, `check`, and `posts`.

## Run the tests

```sh
cargo test --all
```

A fresh clone passes on all three platforms. If it does not, that is a bug;
please open one with the Bug template.

## Prebuilt binaries

Tagged releases publish archives for Linux x86_64, macOS (Intel and Apple
silicon), and Windows x86_64 on the
[releases page](https://github.com/grayzu/dreamcarve/releases), with shell and
PowerShell installer scripts.
