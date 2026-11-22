// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `.dcv` load and save (ADR-0002; `docs/specs/phase-1/design.md`, "Data
//! model → dc-project").
//!
//! [`save`] writes pretty-printed JSON with keys in declaration order, the
//! settings first and the geometry last, each vertex on one line
//! ([`crate::layout`]), and a trailing newline. [`load`] parses bytes,
//! reads `version`, migrates to [`crate::CURRENT_VERSION`], deserializes
//! strictly (unknown fields and missing fields both fail), checks that each
//! bitmap's picture is base64 holding a PNG header (format 27; no image
//! data is decoded), and then validates. A schema error names the path to
//! the bad value and, when the file has a value there, its line and column.
//!
//! `load` fails only when the bytes cannot become a [`Project`]. Validation
//! problems, errors included, come back beside the project in [`Loaded`] so
//! a caller such as the CLI `check` command can report all of them; callers
//! that machine the project must refuse it when [`Loaded::has_errors`].

use crate::bitmap::BitmapError;
use crate::ids::BitmapId;
use crate::layout::DcvFormatter;
use crate::locate::{PathSegment, SourceMap, TextPosition};
use crate::migrate::{MigrationNote, migrate_with_notes};
use crate::model::{
    BoreOp, DrillOp, EngraveOp, InlayOp, Operation, PhotoVCarveOp, PocketOp, ProfileOp, Project,
    VCarveOp,
};
use crate::post_processor::PostProcessorError;
use crate::validate::{Problem, validate};
use serde::Serialize;
use serde_json::Value;
use serde_path_to_error::Segment;

/// Why bytes could not be loaded as a project.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LoadError {
    /// The bytes are not valid JSON. NaN and infinite numbers fail here,
    /// since JSON cannot express them.
    #[error("file is not valid JSON at line {line}, column {column}: {message}")]
    Json {
        /// 1-based line of the error.
        line: usize,
        /// 1-based column of the error.
        column: usize,
        /// The parser's description.
        message: String,
    },
    /// The JSON root is not an object.
    #[error("file is not a DreamCarve project: the top level is not a JSON object")]
    NotAnObject,
    /// The root object has no `version` field.
    #[error("file has no `version` field")]
    MissingVersion,
    /// `version` is not a non-negative integer that fits in 32 bits.
    #[error("file version `{found}` is not a valid version number")]
    InvalidVersion {
        /// The `version` value as written.
        found: String,
    },
    /// The file was written by a newer DreamCarve.
    #[error("file version {found} is newer than this build supports (version {supported})")]
    NewerVersion {
        /// The file's version.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
    /// A migration step could not rewrite the file.
    #[error("migration from version {from} to {to} failed{}: {message}", schema_where("", *at))]
    Migration {
        /// Version migrated from.
        from: u32,
        /// Version migrated to.
        to: u32,
        /// What went wrong.
        message: String,
        /// Where in the file's text the value the message names first
        /// is (`operations[0].last_pass`, in backquotes), when [`load`]
        /// read it from text and the file has that value. `None` from
        /// [`crate::migrate`](fn@crate::migrate), which has no text.
        at: Option<TextPosition>,
    },
    /// The JSON does not match the project schema (unknown or missing
    /// field, wrong type).
    #[error("file does not match the project format{}: {message}", schema_where(path, *at))]
    Schema {
        /// Where in the project the mismatch is, as keys and indices:
        /// `operations[0].tabs.placement`. Empty for the top level.
        path: String,
        /// Where in the file's text the mismatch is, when the file has a
        /// value at `path`.
        at: Option<TextPosition>,
        /// serde's description, naming the offending field.
        message: String,
    },
    /// A number is NaN or infinite.
    #[error("{field} is not a finite number")]
    NonFinite {
        /// Path to the field.
        field: String,
    },
    /// A bitmap's stored picture cannot be read (format 27): its
    /// `png_base64` is not base64 or not a PNG, which [`load`] refuses,
    /// or, from [`crate::Bitmap::pixels`], its PNG is not 8-bit grayscale
    /// of the bitmap's size within [`crate::MAX_BITMAP_SIDE_PX`].
    #[error("bitmap `{id}` cannot be read: {reason}")]
    Bitmap {
        /// The bitmap.
        id: BitmapId,
        /// What is wrong with its picture.
        reason: BitmapError,
    },
}

/// Why a project could not be saved.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SaveError {
    /// A number is NaN or infinite, which JSON cannot hold.
    #[error("{field} is not a finite number and cannot be saved")]
    NonFinite {
        /// Path to the field.
        field: String,
    },
    /// The serializer failed.
    #[error("project could not be serialized: {message}")]
    Serialize {
        /// serde's description.
        message: String,
    },
}

/// A successfully parsed project and everything [`validate`] found in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The project, migrated to [`crate::CURRENT_VERSION`].
    pub project: Project,
    /// Validation problems, errors and warnings.
    pub problems: Vec<Problem>,
    /// What migrating the file changed that the user should hear about;
    /// empty for a file of the current version.
    pub notes: Vec<MigrationNote>,
}

impl Loaded {
    /// Whether any problem is error-class, so the project must not be
    /// machined until it is fixed.
    pub fn has_errors(&self) -> bool {
        self.problems.iter().any(Problem::is_error)
    }
}

/// Serializes `project` as `.dcv` text: pretty JSON, keys in declaration
/// order, each vertex on one line, `Option` fields as `null`, ending in a
/// newline.
///
/// Fails with [`SaveError::NonFinite`] rather than writing a NaN or
/// infinity, which serde_json would silently turn into `null`.
pub fn save(project: &Project) -> Result<String, SaveError> {
    if let Some(field) = first_non_finite(&validate(project)) {
        return Err(SaveError::NonFinite { field });
    }
    let mut bytes = Vec::new();
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut bytes, DcvFormatter::default());
    project
        .serialize(&mut serializer)
        .map_err(|e| SaveError::Serialize {
            message: e.to_string(),
        })?;
    bytes.push(b'\n');
    String::from_utf8(bytes).map_err(|e| SaveError::Serialize {
        message: e.to_string(),
    })
}

/// Parses `.dcv` bytes into a project, migrating older versions and then
/// validating. See the module docs for which failures are errors here and
/// which are returned as [`Loaded::problems`].
pub fn load(bytes: &[u8]) -> Result<Loaded, LoadError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|e| LoadError::Json {
        line: e.line(),
        column: e.column(),
        message: e.to_string(),
    })?;
    let version = read_version(&root)?;
    let (root, notes) =
        migrate_with_notes(root, version).map_err(|err| locate_migration(err, bytes))?;
    let project = deserialize_project(&root, bytes)?;
    check_post_processor(&project, bytes)?;
    // Format 27: each picture's text is base64 and starts with a
    // PNG header, or the file does not load. The header's contents, and
    // the image data behind it, are not `load`'s: validation reads the
    // header and `Bitmap::pixels` the data, so opening a file costs
    // nothing per pixel.
    for bitmap in &project.bitmaps {
        bitmap.check_stored()?;
    }
    let problems = validate(&project);
    // serde_json rejects NaN and out-of-range numbers while parsing, so this
    // is a backstop for the "reject non-finite at the boundary" rule.
    if let Some(field) = first_non_finite(&problems) {
        return Err(LoadError::NonFinite { field });
    }
    Ok(Loaded {
        project,
        problems,
        notes,
    })
}

/// `err` with the position of the first field its message names, in
/// backquotes as `operations[0].last_pass`, when it is a
/// [`LoadError::Migration`] and `bytes` has a value there,
/// as [`LoadError::Schema`] gives one.
fn locate_migration(err: LoadError, bytes: &[u8]) -> LoadError {
    let LoadError::Migration {
        from,
        to,
        message,
        at: None,
    } = err
    else {
        return err;
    };
    let at = message.split('`').skip(1).step_by(2).find_map(|field| {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| SourceMap::parse(text).ok())
            .and_then(|map| map.locate(field))
    });
    LoadError::Migration {
        from,
        to,
        message,
        at,
    }
}

/// The serde error that stopped a deserialization, with the path to it.
type PathError = serde_path_to_error::Error<serde_json::Error>;

/// Deserializes the migrated `root` strictly. On a mismatch, says where it
/// is: the path from serde_path_to_error, and the line and column of
/// that path in `bytes`, the file as the user has it, found with
/// [`SourceMap`]. The same lookup serves a file at the current version and
/// a migrated one; a path only the migration created has no position.
fn deserialize_project(root: &Value, bytes: &[u8]) -> Result<Project, LoadError> {
    let err = match serde_path_to_error::deserialize::<_, Project>(root) {
        Ok(project) => return Ok(project),
        Err(err) => err,
    };
    let mut path = path_segments(&err);
    let mut message = err.inner().to_string();
    if let Some(inner) = operation_error(root, &path) {
        path.extend(path_segments(&inner));
        message = inner.inner().to_string();
    }
    let at = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| SourceMap::parse(text).ok())
        .and_then(|map| schema_error_position(&map, &path, &message));
    Err(LoadError::Schema {
        path: path_text(&path),
        at,
        message,
    })
}

/// A `post_processor` that names no bundled definition version is a
/// schema error at its `id` or `definition_version`: the file is
/// well formed, but nothing can export it. The message lists what is
/// bundled.
fn check_post_processor(project: &Project, bytes: &[u8]) -> Result<(), LoadError> {
    let err = match project.post_processor.definition() {
        Ok(_) => return Ok(()),
        Err(err) => err,
    };
    let mut path = vec![PathSegment::Key("post_processor".into())];
    match err {
        PostProcessorError::UnknownId { .. } => path.push(PathSegment::Key("id".into())),
        PostProcessorError::UnknownVersion { .. } => {
            path.push(PathSegment::Key("definition_version".into()));
        }
        PostProcessorError::BundledDefinition { .. } => {}
    }
    let at = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| SourceMap::parse(text).ok())
        .and_then(|map| map.position(&path));
    Err(LoadError::Schema {
        path: path_text(&path),
        at,
        message: err.to_string(),
    })
}

/// The path of `err` as [`PathSegment`]s, up to the first step that is
/// not a key or an index.
fn path_segments(err: &PathError) -> Vec<PathSegment> {
    err.path()
        .iter()
        .map_while(|segment| match segment {
            Segment::Map { key } => Some(PathSegment::Key(key.clone())),
            // An externally tagged variant is the key of a one-key object.
            Segment::Enum { variant } => Some(PathSegment::Key(variant.clone())),
            Segment::Seq { index } => Some(PathSegment::Index(*index)),
            Segment::Unknown => None,
        })
        .collect()
}

/// serde reads an internally tagged enum such as
/// [`crate::model::Operation`] whole before
/// it picks the variant, so serde_path_to_error's path stops at
/// `operations[i]`. Reading that operation's fields again as its variant's
/// struct gives the error with the rest of the path. `None` when `path` is
/// not an operation or the error is not inside its fields.
fn operation_error(root: &Value, path: &[PathSegment]) -> Option<PathError> {
    let [PathSegment::Key(key), PathSegment::Index(i)] = path else {
        return None;
    };
    if key != "operations" {
        return None;
    }
    let mut fields = root.get(key)?.get(*i)?.as_object()?.clone();
    let kind = fields.shift_remove("kind")?;
    variant_error(kind.as_str()?, &Value::Object(fields))
}

/// Builds `variant_error` from [`Operation`]'s variants, each listed as
/// `Variant(Struct) = "kind"`. The list cannot fall behind the enum: the
/// `match` on an `Operation` below has no wildcard arm, so a variant the
/// list lacks does not compile, nor does a struct that is not its
/// variant's. That each `kind` is the tag serde writes is the test
/// `every_operation_kind_reports_its_field`'s to hold.
macro_rules! operation_variants {
    ($($variant:ident($fields:ty) = $kind:literal),+ $(,)?) => {
        /// The error of reading `fields`, an operation's without its
        /// `kind`, as the struct of the variant `kind` tags. `None` when
        /// they read, or no variant has that tag.
        fn variant_error(kind: &str, fields: &Value) -> Option<PathError> {
            match kind {
                $($kind => serde_path_to_error::deserialize::<_, $fields>(fields).err(),)+
                _ => None,
            }
        }

        const _: fn(&Operation) = |operation| match operation {
            $(Operation::$variant(fields) => {
                let _: &$fields = fields;
            })+
        };
    };
}

operation_variants! {
    Profile(ProfileOp) = "profile",
    Pocket(PocketOp) = "pocket",
    Drill(DrillOp) = "drill",
    Bore(BoreOp) = "bore",
    Engrave(EngraveOp) = "engrave",
    VCarve(VCarveOp) = "v_carve",
    Inlay(InlayOp) = "inlay",
    PhotoVCarve(PhotoVCarveOp) = "photo_v_carve",
}

/// Where the value at `path` starts in the file. An unknown field inside
/// an internally tagged enum is not in the path (see [`operation_error`]),
/// so when `message` names one that the object at `path` has, the
/// position is that field's.
fn schema_error_position(
    map: &SourceMap,
    path: &[PathSegment],
    message: &str,
) -> Option<TextPosition> {
    let unknown = message
        .strip_prefix("unknown field `")
        .and_then(|rest| rest.split('`').next());
    if let Some(field) = unknown {
        let mut field_path = path.to_vec();
        field_path.push(PathSegment::Key(field.to_owned()));
        if let Some(at) = map.position(&field_path) {
            return Some(at);
        }
    }
    map.position(path)
}

/// `path` written as keys and indices: `operations[0].tabs.placement`.
fn path_text(path: &[PathSegment]) -> String {
    let mut out = String::new();
    for segment in path {
        match segment {
            PathSegment::Key(key) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            PathSegment::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

/// ` at `path` (line L, column C)`, as much of it as is known, for
/// [`LoadError::Schema`].
fn schema_where(path: &str, at: Option<TextPosition>) -> String {
    let mut out = String::new();
    if !path.is_empty() {
        out.push_str(&format!(" at `{path}`"));
    }
    if let Some(at) = at {
        out.push_str(&format!(" (line {}, column {})", at.line, at.column));
    }
    out
}

/// Reads the root object's `version` without interpreting anything else.
fn read_version(root: &Value) -> Result<u32, LoadError> {
    let obj = root.as_object().ok_or(LoadError::NotAnObject)?;
    let value = obj.get("version").ok_or(LoadError::MissingVersion)?;
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| LoadError::InvalidVersion {
            found: value.to_string(),
        })
}

fn first_non_finite(problems: &[Problem]) -> Option<String> {
    problems.iter().find_map(|p| match p {
        Problem::NonFinite { field } => Some(field.clone()),
        // A tapered ball nose's values are checked by their own rule; a NaN
        // there must not be saved as `null`, which reads back as unset.
        Problem::TaperedBallNoseRange { value, .. } if !value.is_finite() => p.field(),
        _ => None,
    })
}

#[cfg(test)]
mod tests;
