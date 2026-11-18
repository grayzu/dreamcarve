#!/usr/bin/env bash
#
# Copyright 2026 The DreamCarve Contributors
# SPDX-License-Identifier: Apache-2.0
#
# Installs the developer tools `docs/code-review-plan.md` measures with
# ("Tooling that would sharpen the next pass"). None of them is needed to
# build DreamCarve or to pass the gate in AGENTS.md.
#
# Run it from anywhere inside the repository, on macOS or Linux:
#
#   tools/install-dev-tools.sh            # install or update everything
#   tools/install-dev-tools.sh --check    # list what is installed, change nothing
#
# It needs rustup and a C toolchain, the same as building the project.
# Everything is built from source with `cargo install --locked` into
# ~/.cargo/bin, so there is no Homebrew or apt step and no prebuilt binary
# to trust. A first run compiles for 10 to 20 minutes; a later run only
# rebuilds a tool that has a newer release.
#
# Beyond ~/.cargo/bin it makes two rustup changes:
#   - adds `llvm-tools-preview` to the toolchain rust-toolchain.toml pins,
#     which cargo-llvm-cov reads coverage with;
#   - installs the `nightly` toolchain, which cargo-fuzz builds with. The
#     pinned toolchain stays the default in this repository.

set -euo pipefail

# Crate to install, then the command that shows it works.
TOOLS=(
    "cargo-llvm-cov|cargo llvm-cov --version"   # test coverage
    "cargo-nextest|cargo nextest --version"     # per-test timing
    "cargo-mutants|cargo mutants --version"     # which tests catch a broken rule
    "cargo-machete|cargo machete --version"     # unused dependencies
    "cargo-deny|cargo deny --version"           # deny.toml, as CI runs it
    "cargo-fuzz|cargo fuzz --version"           # fuzz targets (nightly)
    "tokei|tokei --version"                     # line counts by kind
    "ast-grep|ast-grep --version"               # structural code search
    "samply|samply --version"                   # sampling profiler
)

check_only=false
case "${1:-}" in
    "") ;;
    --check) check_only=true ;;
    *)
        echo "usage: $0 [--check]" >&2
        exit 2
        ;;
esac

for needed in rustup cargo; do
    if ! command -v "$needed" >/dev/null 2>&1; then
        echo "error: $needed is not on PATH; install Rust from https://rustup.rs first" >&2
        exit 1
    fi
done

# rustup reads rust-toolchain.toml from the repository root.
cd "$(dirname "$0")/.."

version_of() {
    # The first line a tool's version command prints, or nothing.
    bash -c "$1" 2>/dev/null | head -n 1 || true
}

if $check_only; then
    missing=0
    for entry in "${TOOLS[@]}"; do
        crate="${entry%%|*}"
        found="$(version_of "${entry#*|}")"
        if [ -n "$found" ]; then
            printf '  ok       %-16s %s\n' "$crate" "$found"
        else
            printf '  MISSING  %-16s\n' "$crate"
            missing=1
        fi
    done
    if rustup component list --installed | grep -q '^llvm-tools'; then
        echo "  ok       llvm-tools-preview"
    else
        echo "  MISSING  llvm-tools-preview"
        missing=1
    fi
    if rustup toolchain list | grep -q '^nightly'; then
        echo "  ok       nightly toolchain"
    else
        echo "  MISSING  nightly toolchain"
        missing=1
    fi
    exit "$missing"
fi

echo "==> llvm-tools-preview on $(rustup show active-toolchain | cut -d' ' -f1)"
rustup component add llvm-tools-preview

echo "==> nightly toolchain (for cargo-fuzz)"
rustup toolchain install nightly --profile minimal

failed=()
for entry in "${TOOLS[@]}"; do
    crate="${entry%%|*}"
    echo "==> $crate"
    # cargo skips a crate that is already at its newest release.
    if ! cargo install --locked "$crate"; then
        failed+=("$crate")
    fi
done

echo
echo "Installed:"
for entry in "${TOOLS[@]}"; do
    printf '  %-16s %s\n' "${entry%%|*}" "$(version_of "${entry#*|}")"
done

if [ "${#failed[@]}" -gt 0 ]; then
    echo >&2
    echo "error: these did not install: ${failed[*]}" >&2
    exit 1
fi
