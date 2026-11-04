// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A drawing imported into a project (`docs/specs/phase-4/design.md`,
//! "Test plan → dc-job (slice 1)"; functional spec, acceptance 64 and
//! capability 55).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_geometry::Transform;
use dc_import::{DxfImportOptions, SvgImportOptions, import_dxf, import_svg};
use dc_job::import::{Drawing, drawing_into};
use dc_job::project_from;
use dc_project::{DefinitionId, InstanceId, PostProcessorRef, Units};

use common::repo_root;

fn svg(path: &str) -> Drawing {
    let bytes = std::fs::read(repo_root().join(path)).unwrap();
    Drawing::Svg(import_svg(&bytes, &SvgImportOptions::new()).unwrap().0)
}

fn dxf(path: &str) -> Drawing {
    let bytes = std::fs::read(repo_root().join(path)).unwrap();
    Drawing::Dxf(import_dxf(&bytes, &DxfImportOptions::new()).unwrap().0)
}

/// Acceptance 64: importing the plaque DXF into the coaster project adds
/// the shapes `init` would, named as `init` names them, placed as drawn
/// (the drawing's origin at the stock's bottom-left corner: the importer's
/// identity transform, not `init`'s margin shift), under ids that follow
/// the project's own, and returns the new instances' ids in drawing order;
/// nothing else in the project changes. A second import lands beside the
/// first.
#[test]
fn drawing_into_places_as_drawn_and_returns_the_new_ids() {
    let post = PostProcessorRef::latest("grblhal").unwrap();
    let base = project_from::drawing(
        "coaster",
        svg("fixtures/coaster.svg"),
        post.clone(),
        Units::Mm,
        true,
    )
    .unwrap();
    assert_eq!(base.instances.len(), 1);
    assert!(base.definitions.contains_key(&DefinitionId::new("d1")));

    let plaque = dxf("tests/fixtures/plaque.dxf");
    let (fresh_definitions, fresh_instances) = plaque.clone().into_shapes();
    let (project, ids) = drawing_into(&base, plaque);

    // The new instances, in drawing order, as drawn.
    assert_eq!(ids.len(), fresh_instances.len());
    assert_eq!(
        project.instances.len(),
        base.instances.len() + fresh_instances.len()
    );
    assert_eq!(project.instances[..1], base.instances[..]);
    for (k, (id, fresh)) in ids.iter().zip(&fresh_instances).enumerate() {
        let added = &project.instances[base.instances.len() + k];
        assert_eq!(&added.id, id);
        assert_eq!(added.id, InstanceId::new(format!("i{}", k + 2)), "after i1");
        assert_eq!(added.name, fresh.name, "named as `init` names it");
        assert_eq!(added.transform, Transform::IDENTITY, "placed as drawn");
        assert_eq!(added.transform, fresh.transform);
        // Its definition is the imported geometry under a fresh id.
        let definition = project.definitions.get(&added.definition).unwrap();
        let fresh_definition = fresh_definitions
            .iter()
            .find(|d| d.id == fresh.definition)
            .unwrap();
        assert_eq!(definition.profiles, fresh_definition.profiles);
        assert_eq!(definition.name, fresh_definition.name);
        assert!(!base.definitions.contains_key(&added.definition));
    }
    // Everything else is as it was.
    assert_eq!(project.stock, base.stock);
    assert_eq!(project.tools, base.tools);
    assert_eq!(project.operations, base.operations);
    assert_eq!(project.tabs, base.tabs);
    assert_eq!(project.post_processor, base.post_processor);
    assert_eq!(
        project.definitions.len(),
        base.definitions.len() + fresh_definitions.len()
    );
    assert_eq!(
        dc_job::validation(&project).errors,
        dc_job::validation(&base).errors
    );

    // A second import takes the ids after the first's.
    let (again, more) = drawing_into(&project, dxf("tests/fixtures/plaque.dxf"));
    assert_eq!(more.len(), ids.len());
    assert!(
        more.iter()
            .all(|id| !ids.contains(id) && !base.instances.iter().any(|i| &i.id == id))
    );
    assert_eq!(again.instances.len(), project.instances.len() + more.len());
    let mut all: Vec<&InstanceId> = again.instances.iter().map(|i| &i.id).collect();
    all.sort();
    all.dedup();
    assert_eq!(all.len(), again.instances.len(), "every id is unique");
}
