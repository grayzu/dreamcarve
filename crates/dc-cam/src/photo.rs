// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Photo V-Carve (`docs/specs/phase-3/design.md`, "Algorithms → Photo
//! V-Carve"): a picture carved as parallel V-bit grooves whose depth
//! follows its darkness. Format 27 (#644) brought the record,
//! [`dc_project::PhotoVCarveOp`], and the bitmap it carves; the generator
//! is slice 24 (#646).
//!
//! Until then [`generate_photo`] has its final signature and refuses every
//! operation with [`CamError::PhotoNotBuilt`], so a project that holds a
//! Photo V-Carve loads, validates and saves, and `check` and `export` say
//! in one line why nothing is cut for it: never a panic, and never an
//! empty toolpath that would export as a program which cuts nothing.

use crate::error::CamError;
use crate::toolpath::Toolpath;
use crate::warning::Warning;
use dc_project::{PhotoVCarveOp, Project};

/// Generates the toolpath of Photo V-Carve `op` of `project`.
///
/// Not built yet (#646): always [`CamError::PhotoNotBuilt`], naming the
/// operation. No bitmap is decoded and no move is made.
// TODO(#646): the generator; then remove `CamError::PhotoNotBuilt`.
pub fn generate_photo(
    _project: &Project,
    op: &PhotoVCarveOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    Err(CamError::PhotoNotBuilt { op: op.id.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::OperationId;
    use dc_project::test_support::{photo_project, sample_photo_v_carve};

    /// #644: until the generator lands (#646), generating a Photo V-Carve
    /// is a typed error that names the operation. It is refused whatever
    /// the project holds, so it never yields a toolpath, empty or not.
    #[test]
    fn generation_is_not_built_yet() {
        let project = photo_project();
        assert_eq!(dc_project::validate(&project), vec![]);
        let op = sample_photo_v_carve();
        let err = generate_photo(&project, &op).unwrap_err();
        assert_eq!(
            err,
            CamError::PhotoNotBuilt {
                op: OperationId::new("o8")
            }
        );
        assert_eq!(
            err.to_string(),
            "operation `o8` is a Photo V-Carve, which this version of DreamCarve cannot cut yet; disable it to export the project's other operations"
        );
        // A project without the bitmap it names is refused the same way.
        let mut bare = project;
        bare.bitmaps.clear();
        assert_eq!(generate_photo(&bare, &op).unwrap_err(), err);
    }
}
