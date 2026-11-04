// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A drawing imported into an existing project (`docs/specs/phase-4/design.md`,
//! "Data model → dc-job"; functional spec, capability 55): its shapes
//! appended as new feature definitions and instances, placed as drawn,
//! with the drawing's origin at the stock's bottom-left corner. The
//! importers of `dc-import` read the bytes; this module owns what happens
//! to their result.

use std::collections::BTreeMap;

use dc_geometry::Rect;
use dc_import::{DxfImport, SvgImport};
use dc_project::{DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Project};

/// What an importer produced, whichever format it read.
#[derive(Debug, Clone, PartialEq)]
pub enum Drawing {
    /// An SVG, flipped to Y-up by the importer.
    Svg(SvgImport),
    /// A DXF, in its declared or given units.
    Dxf(DxfImport),
}

impl Drawing {
    /// The bounds of every imported shape, mm; `None` when nothing was
    /// imported.
    #[must_use]
    pub fn bounds(&self) -> Option<Rect> {
        match self {
            Self::Svg(import) => import.bounds,
            Self::Dxf(import) => import.bounds,
        }
    }

    /// The imported definitions and their instances, one per definition in
    /// the same order, with the importer's ids (`d1`, `i1`, …).
    #[must_use]
    pub fn into_shapes(self) -> (Vec<FeatureDefinition>, Vec<FeatureInstance>) {
        match self {
            Self::Svg(import) => (import.definitions, import.instances),
            Self::Dxf(import) => (import.definitions, import.instances),
        }
    }
}

/// `project` with `import`'s definitions and instances appended, placed as
/// drawn (functional spec, capability 55): the drawing's origin is the
/// stock's bottom-left corner, so the instances keep the importer's
/// identity transform. The new shapes take the next free `d<n>` and
/// `i<n>` ids after the project's own, so a second import of one drawing
/// sits beside the first; the returned ids are the new instances', in
/// drawing order, for the caller to select.
#[must_use]
pub fn drawing_into(project: &Project, import: Drawing) -> (Project, Vec<InstanceId>) {
    let (definitions, instances) = import.into_shapes();
    let mut project = project.clone();
    let first_definition = next_free(project.definitions.keys().map(DefinitionId::as_str), 'd');
    let first_instance = next_free(project.instances.iter().map(|i| i.id.as_str()), 'i');
    let mut renamed: BTreeMap<DefinitionId, DefinitionId> = BTreeMap::new();
    for (n, mut definition) in (first_definition..).zip(definitions) {
        let id = DefinitionId::new(format!("d{n}"));
        renamed.insert(definition.id.clone(), id.clone());
        definition.id = id.clone();
        project.definitions.insert(id, definition);
    }
    let mut new_ids = Vec::with_capacity(instances.len());
    for (n, mut instance) in (first_instance..).zip(instances) {
        if let Some(definition) = renamed.get(&instance.definition) {
            instance.definition = definition.clone();
        }
        instance.id = InstanceId::new(format!("i{n}"));
        new_ids.push(instance.id.clone());
        project.instances.push(instance);
    }
    (project, new_ids)
}

/// One past the highest `<prefix><n>` among `ids`, or 1 when none has that
/// form, so every id handed out after it is free.
fn next_free<'a>(ids: impl Iterator<Item = &'a str>, prefix: char) -> u64 {
    ids.filter_map(|id| id.strip_prefix(prefix)?.parse::<u64>().ok())
        .max()
        .map_or(1, |highest| highest.saturating_add(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_free_follows_the_highest_numbered_id() {
        assert_eq!(next_free(["d1", "d2"].into_iter(), 'd'), 3);
        assert_eq!(next_free(["d7", "coaster", "d3"].into_iter(), 'd'), 8);
        assert_eq!(next_free(std::iter::empty(), 'i'), 1);
        assert_eq!(next_free(["oval"].into_iter(), 'i'), 1);
    }
}
