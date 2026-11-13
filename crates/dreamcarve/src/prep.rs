// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve image-prep <svg|dxf>`: reads one SVG or DXF and writes one
//! SVG that is easy to inlay, then prints what it did (functional 49b to
//! 49e; `docs/specs/phase-3/design.md`, "CLI → Image prep and the checks";
//! DXF input, #669).
//!
//! Everything is decided by `dc_import::prep` (the pipeline, the writer
//! and its read-back check) and `dc_job::prep_report` (the lines, shared
//! with the app); this file reads and writes the files and prints. It
//! takes no tool, stock, depth, feed or size, by construction. Nothing is
//! written until every step that can fail has run.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dc_import::prep::{
    DEFAULT_MIN_DETAIL_PCT, DEFAULT_SMOOTH_PCT, PrepMode, PrepOptions, prepare, prepare_dxf,
    write_svg,
};
use dc_import::{ImportError, MAX_IMPORT_VERTICES};
use dc_job::prep_report;
use dreamcarve_shell::StagedFile;

use crate::cli::{Failure, ImagePrepArgs, read_file};
use crate::output::{errln, outln};

/// What a picture is called in the `prep_not_svg` message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Picture {
    Png,
    Jpeg,
}

impl Picture {
    const fn name(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
        }
    }
}

/// The drawing formats `image-prep` reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drawing {
    Svg,
    Dxf,
}

/// Runs `image-prep`.
pub fn run(args: &ImagePrepArgs) -> Result<ExitCode, Failure> {
    // Step 1: a picture is refused whatever it is called, with the way
    // forward (tracing is deferred; any future tracer must be permissively
    // licensed). A drawing is an SVG or a DXF, by its extension checked
    // against its contents.
    let input = &args.drawing;
    let bytes = read_file(input)?;
    if let Some(picture) = picture_of(input, &bytes) {
        return Err(Failure::usage(format!(
            "prep_not_svg: {} is a {} picture, not an SVG or DXF drawing; trace it to shapes \
             first, in Inkscape (Path > Trace Bitmap), save the result as SVG and run image-prep \
             on that",
            input.display(),
            picture.name()
        )));
    }
    let drawing = drawing_of(input, &bytes)?;

    // Step 2: where it goes, and never over a file that exists.
    let output = args.output.clone().unwrap_or_else(|| default_output(input));
    if output.exists() {
        return Err(Failure::error(format!(
            "{} already exists; remove it or choose another file with -o",
            output.display()
        )));
    }

    // Step 3: prepare. The library checks the thresholds again and reads
    // its own output back before it returns the pieces.
    let options = PrepOptions {
        mode: if args.regions {
            PrepMode::Regions
        } else {
            PrepMode::Shapes
        },
        min_detail_pct: args.min_detail.unwrap_or(DEFAULT_MIN_DETAIL_PCT),
        smooth_pct: args.smooth.unwrap_or(DEFAULT_SMOOTH_PCT),
        max_vertices: MAX_IMPORT_VERTICES,
    };
    let prepared = match drawing {
        Drawing::Svg => prepare(&bytes, &options),
        Drawing::Dxf => prepare_dxf(&bytes, &options),
    };
    let (prepared, warnings) = prepared.map_err(|e| prepare_failure(&e, input))?;
    let text = write_svg(&prepared);
    StagedFile::write(&output, text.as_bytes())?.commit()?;

    // Step 4: the warnings, the report, where the file went.
    for warning in &warnings {
        errln!("{}", prep_report::warning_line(warning));
    }
    let report = prep_report::render(
        &prepared.report,
        &input.display().to_string(),
        &output.display().to_string(),
    );
    outln!("{report}")?;
    outln!("Wrote {}", output.display())?;
    Ok(ExitCode::SUCCESS)
}

/// The input's name with `-inlay` added to its stem and `.svg` as its
/// extension, beside it.
fn default_output(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .map_or_else(|| "design".to_owned(), |s| s.to_string_lossy().into_owned());
    input.with_file_name(format!("{stem}-inlay.svg"))
}

/// The picture `path` is: by its extension (`.png`, `.jpg`, `.jpeg`) or by
/// the first bytes of the file.
fn picture_of(path: &Path, bytes: &[u8]) -> Option<Picture> {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G'];
    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF];
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("png") => return Some(Picture::Png),
        Some("jpg" | "jpeg") => return Some(Picture::Jpeg),
        _ => {}
    }
    if bytes.starts_with(PNG) {
        Some(Picture::Png)
    } else if bytes.starts_with(JPEG) {
        Some(Picture::Jpeg)
    } else {
        None
    }
}

/// Whether `bytes` read as a DXF: the binary sentinel, or (after an
/// optional byte-order mark) a first line holding the group code `0` or
/// `999`, as every ASCII DXF begins with `0 SECTION` or a comment.
fn looks_like_dxf(bytes: &[u8]) -> bool {
    if bytes.starts_with(b"AutoCAD Binary DXF") {
        return true;
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let first = bytes
        .split(|&b| b == b'\n' || b == b'\r')
        .find(|line| !line.trim_ascii().is_empty())
        .map(<[u8]>::trim_ascii);
    matches!(first, Some(b"0" | b"999"))
}

/// The drawing `path` holds: a `.dxf` must read as a DXF and a `.svg` must
/// not; any other name goes by the contents. A name and contents that
/// disagree are a usage error, so a misnamed file is never read as the
/// wrong format.
fn drawing_of(path: &Path, bytes: &[u8]) -> Result<Drawing, Failure> {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    let dxf = looks_like_dxf(bytes);
    let mismatch = |named: &str, holds: &str| {
        Failure::usage(format!(
            "prep_format_mismatch: {} is named as {named} but holds {holds}; rename it to match",
            path.display()
        ))
    };
    match (extension.as_deref(), dxf) {
        (Some("svg"), true) => Err(mismatch("an SVG", "a DXF")),
        (Some("dxf"), false) => Err(mismatch("a DXF", "something else")),
        (_, true) => Ok(Drawing::Dxf),
        (_, false) => Ok(Drawing::Svg),
    }
}

/// A threshold outside its range is a usage error (exit 2); every other
/// failure of the pipeline is an error (exit 1) that names the file and,
/// for the image-prep ones, its code.
fn prepare_failure(error: &ImportError, input: &Path) -> Failure {
    match (error, error.code()) {
        (ImportError::PrepThresholdRange { .. }, Some(code)) => {
            Failure::usage(format!("{code}: {error}"))
        }
        (_, Some(code)) => Failure::error(format!(
            "cannot prepare {}: {code}: {error}",
            input.display()
        )),
        (ImportError::UnitsUnknown { .. }, None) => Failure::error(format!(
            "cannot import {}: {error}; set the drawing's units in the CAD program and save it \
             again",
            input.display()
        )),
        (_, None) => Failure::error(format!("cannot import {}: {error}", input.display())),
    }
}
