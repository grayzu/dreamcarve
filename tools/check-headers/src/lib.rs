// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! License header checker for DreamCarve sources (`AGENTS.md`, "Licensing
//! rules"). Every `.rs` file must open with either the DreamCarve header or
//! the upstream PureCutCNC attribution header. Files under `target/`, `.git/`,
//! `node_modules/`, and files marked `@generated` in their head are skipped.
//!
//! Reimplements the idea of PureCutCNC's `scripts/check-license-headers.ts`
//! in Rust so CI needs no Node.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SPDX: &str = "// SPDX-License-Identifier: Apache-2.0";
const DREAMCARVE_SUFFIX: &str = " The DreamCarve Contributors";
const UPSTREAM_SUFFIX: &str = " Franja (Frank) Povazanj (PureCutCNC)";
const PORTED_MARKER: &str = "// Ported to Rust and modified from PureCutCNC:";
const GENERATED_MARKER: &str = "@generated";
/// Only the head of a file can carry the header or the generated marker.
const HEAD_BYTES: usize = 2048;
const SKIP_DIRS: &[&str] = &["target", ".git", "node_modules"];

/// Why a source file failed the header check. `Display` always names the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    /// The file does not start with a `// Copyright` comment block.
    Missing {
        /// The offending file.
        path: PathBuf,
    },
    /// The file starts with a copyright block that is not one of the two
    /// accepted forms.
    Malformed {
        /// The offending file.
        path: PathBuf,
        /// What was expected and not found.
        reason: &'static str,
    },
}

impl fmt::Display for HeaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { path } => {
                write!(f, "{}: missing license header", path.display())
            }
            Self::Malformed { path, reason } => {
                write!(f, "{}: malformed license header: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for HeaderError {}

/// Checks that `contents` (the text of the file at `path`) opens with an
/// accepted header. `path` is only used for the error message.
pub fn check_source(path: &Path, contents: &str) -> Result<(), HeaderError> {
    let block: Vec<&str> = contents
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .take_while(|l| l.starts_with("//"))
        .collect();

    let malformed = |reason| HeaderError::Malformed {
        path: path.to_path_buf(),
        reason,
    };

    let Some(first) = block.first() else {
        return Err(HeaderError::Missing {
            path: path.to_path_buf(),
        });
    };
    if !first.starts_with("// Copyright ") {
        return Err(HeaderError::Missing {
            path: path.to_path_buf(),
        });
    }

    if is_copyright(first, DREAMCARVE_SUFFIX) {
        if block.get(1) != Some(&SPDX) {
            return Err(malformed("SPDX line must follow the DreamCarve copyright"));
        }
        return Ok(());
    }

    if is_copyright(first, UPSTREAM_SUFFIX) {
        if !block
            .get(1)
            .is_some_and(|l| is_copyright(l, DREAMCARVE_SUFFIX))
        {
            return Err(malformed(
                "DreamCarve copyright must follow the PureCutCNC copyright",
            ));
        }
        if block.get(2) != Some(&SPDX) {
            return Err(malformed("SPDX line must follow the copyright lines"));
        }
        if !block.contains(&PORTED_MARKER) {
            return Err(malformed(
                "ported files need a 'Ported to Rust and modified from PureCutCNC:' line",
            ));
        }
        return Ok(());
    }

    Err(malformed(
        "first line must be a DreamCarve or PureCutCNC copyright",
    ))
}

fn is_copyright(line: &str, suffix: &str) -> bool {
    line.strip_prefix("// Copyright ")
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|year| year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()))
}

/// Walks `root` and returns every `.rs` file that must carry a header,
/// skipping build output, VCS metadata, and files marked `@generated`.
pub fn find_sources(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(root, &mut out)?;
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if name.to_str().is_some_and(|n| SKIP_DIRS.contains(&n)) {
                continue;
            }
            walk(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") && !is_generated(&path)? {
            out.push(path);
        }
    }
    Ok(())
}

fn is_generated(path: &Path) -> io::Result<bool> {
    let bytes = fs::read(path)?;
    let head = bytes.get(..HEAD_BYTES.min(bytes.len())).unwrap_or(&bytes);
    Ok(String::from_utf8_lossy(head).contains(GENERATED_MARKER))
}

/// Checks every source under `root`. Returns the number of files checked, or
/// every failure found.
pub fn check_tree(root: &Path) -> io::Result<Result<usize, Vec<HeaderError>>> {
    let files = find_sources(root)?;
    let mut failures = Vec::new();
    for path in &files {
        let contents = fs::read_to_string(path)?;
        if let Err(e) = check_source(path, &contents) {
            failures.push(e);
        }
    }
    if failures.is_empty() {
        Ok(Ok(files.len()))
    } else {
        failures.sort_by_key(ToString::to_string);
        Ok(Err(failures))
    }
}
