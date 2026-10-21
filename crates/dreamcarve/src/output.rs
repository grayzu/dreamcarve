// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Everything the binary prints goes through [`outln!`], [`out!`] and
//! [`errln!`] (#216). `println!` panics when a write to stdout fails, and
//! it fails as soon as the reader of a pipe quits (`dreamcarve check
//! big.dcv | head`, or quitting `less`).
//!
//! - A broken pipe (`EPIPE`; `ERROR_NO_DATA` or `ERROR_BROKEN_PIPE` on
//!   Windows, which Rust reports as [`io::ErrorKind::BrokenPipe`] too)
//!   means nobody reads any more. The rest of the output is dropped, the
//!   command finishes its work (a file it writes is still written), and
//!   the process exits with the command's own code, so `check` still
//!   fails on errors under `set -o pipefail`. Nothing is printed about the
//!   pipe.
//! - Any other error writing stdout, such as a full disk under `> file`,
//!   fails the command: exit 1, with `error: cannot write to standard
//!   output: …` on stderr.
//! - A write to stderr that fails is ignored: there is nowhere left to
//!   report it.

use std::fmt;
use std::io::{self, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::cli::Failure;

/// Set once a write to stdout found the pipe closed; later output is
/// dropped.
static STDOUT_CLOSED: AtomicBool = AtomicBool::new(false);

/// Writes `args` to stdout. A broken pipe is not an error; see the module
/// documentation.
pub fn stdout(args: fmt::Arguments<'_>) -> Result<(), Failure> {
    if STDOUT_CLOSED.load(Ordering::Relaxed) {
        return Ok(());
    }
    settle(io::stdout().lock().write_fmt(args))
}

/// Flushes stdout, so an error writing its last buffered bytes is handled
/// like any other write's.
pub fn flush() -> Result<(), Failure> {
    if STDOUT_CLOSED.load(Ordering::Relaxed) {
        return Ok(());
    }
    settle(io::stdout().lock().flush())
}

/// Writes `args` to stderr, ignoring a failure.
pub fn stderr(args: fmt::Arguments<'_>) {
    let _ = io::stderr().lock().write_fmt(args);
}

/// The outcome of a write to stdout: a broken pipe stops output and is
/// otherwise ignored; any other error fails the command.
fn settle(result: io::Result<()>) -> Result<(), Failure> {
    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
            STDOUT_CLOSED.store(true, Ordering::Relaxed);
            Ok(())
        }
        Err(e) => Err(Failure::error(format!(
            "cannot write to standard output: {e}"
        ))),
    }
}

/// `print!` that returns `Result<(), Failure>` instead of panicking.
macro_rules! out {
    ($($arg:tt)*) => {
        $crate::output::stdout(format_args!($($arg)*))
    };
}

/// `println!` that returns `Result<(), Failure>` instead of panicking.
macro_rules! outln {
    () => {
        $crate::output::stdout(format_args!("\n"))
    };
    ($($arg:tt)*) => {
        $crate::output::stdout(format_args!("{}\n", format_args!($($arg)*)))
    };
}

/// `eprintln!` that ignores a failure instead of panicking.
macro_rules! errln {
    ($($arg:tt)*) => {
        $crate::output::stderr(format_args!("{}\n", format_args!($($arg)*)))
    };
}

pub(crate) use {errln, out, outln};
