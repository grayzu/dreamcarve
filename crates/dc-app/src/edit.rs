// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/helpers/profileEdit.ts (applyLineCornerFillet)

//! The commands that change a shape's outline as a whole
//! (`docs/specs/phase-5/design.md`, "Module layout"; functional
//! capabilities 93 to 97): **Shape ▸ Fillet Corners…**, **Offset…**,
//! **Weld**, **Subtract**, **Intersect** and **Join** here; Trim is a
//! tool (`dc_app::tool`). Each is a
//! pure function of the project that gives the one command to run, as
//! `dc_app::properties` and `dc_app::shape` do.
//!
//! A shape's outline is read as paths (`paths_of`): its own, where it
//! keeps curves, else each profile exactly. A shape that keeps its sizes
//! is no path until it is converted (capability 84).

use dc_geometry::{BooleanOp, Contour, CornerError, Path, Polyline, Region};
use dc_project::{FeatureDefinition, FeatureInstance, InstanceId, Project, Shape, Units};

use crate::command::{Command, Edit, NewShape, copy_name, find_instance};
use crate::error::CommandError;
use crate::strings::{StrKey, Strings};
use crate::units;

/// The paths a definition's outline is: its own, when it keeps curves,
/// else each profile as a path. `None` for a shape that keeps its sizes.
pub(crate) fn paths_of(definition: &FeatureDefinition) -> Option<Vec<Path>> {
    match &definition.shape {
        Some(Shape::Path { paths }) => Some(paths.clone()),
        Some(_) => None,
        None => Some(
            definition
                .profiles
                .iter()
                .map(Path::from_polyline)
                .collect(),
        ),
    }
}

/// The outline `paths` give, flattened to `tolerance_mm`, and the shape
/// that keeps them: the paths where one has a curve, else nothing, since
/// lines and arcs are the outline itself (ADR-0006).
pub(crate) fn outline_of(paths: Vec<Path>, tolerance_mm: f64) -> (Option<Shape>, Vec<Polyline>) {
    let profiles = paths
        .iter()
        .map(|path| path.to_polyline(tolerance_mm))
        .collect();
    let shape = paths
        .iter()
        .any(Path::has_curves)
        .then_some(Shape::Path { paths });
    (shape, profiles)
}

/// Why a corner was left as it is, a limit in it in project units and
/// `scale` times as the shape is placed.
pub(crate) fn corner_reason(
    strings: &Strings,
    units: Units,
    error: CornerError,
    scale: f64,
) -> String {
    match error {
        CornerError::TooLarge { max_mm } => strings.fmt(
            StrKey::NodeRefusedCornerTooLarge,
            &[("max", &units::limit_text(max_mm * scale, units))],
        ),
        other => other.to_string(),
    }
}

/// Every corner of `paths` rounded by `radius_mm` that can take it
/// (capability 93): the paths, how many corners were rounded, and why
/// the first that was not was left.
pub(crate) fn fillet_all(
    paths: &[Path],
    radius_mm: f64,
) -> (Vec<Path>, usize, Option<CornerError>) {
    let mut out = Vec::with_capacity(paths.len());
    let mut rounded = 0;
    let mut left = None;
    for path in paths {
        let mut path = path.clone();
        // From the last node back, so the nodes before keep their
        // numbers as each corner becomes two nodes.
        for node in (0..path.nodes().len()).rev() {
            match path.fillet(node, radius_mm) {
                Ok(filleted) => {
                    path = filleted;
                    rounded += 1;
                }
                // An open path's end is no corner to speak of.
                Err(CornerError::End) => {}
                Err(error) => left = left.or(Some(error)),
            }
        }
        out.push(path);
    }
    (out, rounded, left)
}

/// **Shape ▸ Fillet Corners…** on shape `id` with the radius typed as
/// `text`: every corner between two straight sides that can take the
/// radius is rounded, the way an inside pocket is prepared for a round
/// tool. One `SetOutline`.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is no length above 0,
/// when the shape keeps its sizes (it is converted to a path first), and
/// when no corner can take the radius, with the reason.
pub fn fillet_corners(
    project: &Project,
    strings: &Strings,
    id: &InstanceId,
    text: &str,
) -> Result<Command, CommandError> {
    let refused = |reason: String| CommandError::NotAQuantity {
        text: text.trim().to_owned(),
        reason,
    };
    let instance = find_instance(project, id)?;
    let units = project.display_units;
    let radius_mm = units::parse_length_mm(text, units)?;
    if radius_mm <= 0.0 {
        return Err(refused(
            strings.get(StrKey::DrawProblemNotPositive).to_owned(),
        ));
    }
    let Some(paths) = project
        .definitions
        .get(&instance.definition)
        .and_then(paths_of)
    else {
        return Err(refused(
            strings.fmt(StrKey::NodePromptSized, &[("name", &instance.name)]),
        ));
    };
    // The radius is the one seen: as the shape is placed.
    let Some(scale) = crate::shape::even_scale(instance.transform) else {
        return Err(CommandError::Stretched {
            name: instance.name.clone(),
        });
    };
    let (paths, rounded, left) = fillet_all(&paths, radius_mm / scale);
    if rounded == 0 {
        let reason = left.map_or_else(
            || strings.get(StrKey::NodeRefusedNoCorner).to_owned(),
            |error| corner_reason(strings, units, error, scale),
        );
        return Err(refused(reason));
    }
    let (shape, profiles) = outline_of(paths, project.flatten_tolerance_mm);
    Ok(Command::SetOutline {
        instance: id.clone(),
        shape,
        profiles,
        what: Edit::Fillet { count: rounded },
    })
}

/// Which side of an outline an offset is made on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OffsetSide {
    /// Outside a closed outline; left of an open path's direction.
    #[default]
    Outward,
    /// Inside a closed outline; right of an open path's direction.
    Inward,
    /// Both.
    Both,
}

/// What **Shape ▸ Offset…** asks for (capability 94).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Offset {
    /// How far from the outline, mm, as the shape is placed; above 0.
    pub distance_mm: f64,
    /// Which side.
    pub side: OffsetSide,
    /// Whether the shape offset stays.
    pub keep_original: bool,
}

/// The command an offset makes, and the shapes it made nothing of.
#[derive(Debug, Clone, PartialEq)]
pub struct Offsets {
    /// One `ReplaceShapes`: a new path beside, or in place of, each shape
    /// that has an offset.
    pub command: Command,
    /// The shapes whose offset vanished (inward by more than half their
    /// width), by name.
    pub vanished: Vec<String>,
}

/// A definition's closed outlines as the regions they enclose, holes
/// known as holes; `None` when they cross each other or themselves.
fn regions(profiles: &[Polyline]) -> Option<Vec<Region>> {
    let contours: Vec<Contour> = profiles
        .iter()
        .filter(|profile| profile.is_closed())
        .map(|profile| Contour::new(profile.clone()))
        .collect::<Result<_, _>>()
        .ok()?;
    dc_geometry::nest(contours).ok().map(|(regions, _)| regions)
}

/// The offsets of `profiles`, a definition's outlines, by `distance_mm`
/// in definition space on `side`: a closed outline grown or shrunk with
/// its holes the other way, arcs kept and outside corners rounded
/// (`dc_geometry::offset`, the engine's own); an open path moved to its
/// left or its right.
fn offset_profiles(profiles: &[Polyline], distance_mm: f64, side: OffsetSide) -> Vec<Polyline> {
    let signs: &[f64] = match side {
        OffsetSide::Outward => &[1.0],
        OffsetSide::Inward => &[-1.0],
        OffsetSide::Both => &[1.0, -1.0],
    };
    let mut out = Vec::new();
    let closed = regions(profiles);
    for sign in signs {
        let delta = sign * distance_mm;
        if let Some(closed) = &closed {
            for region in closed {
                // The outline moves away from the material it bounds, or
                // into it; a hole's edge moves the same way through the
                // material, which is the other way for the hole.
                let outer = dc_geometry::offset(region.outer(), delta);
                out.extend(outer.into_iter().map(Contour::into_polyline));
                for hole in region.holes() {
                    let moved = dc_geometry::offset(hole, -delta);
                    out.extend(moved.into_iter().map(Contour::into_polyline));
                }
            }
        }
        for open in profiles.iter().filter(|profile| !profile.is_closed()) {
            out.extend(dc_geometry::offset_open(open, delta));
        }
    }
    out
}

/// A name for a shape made from `instance`: its name and `suffix`
/// (*Plate offset*), or the next copy of that when it is taken.
fn derived_name(strings: &Strings, taken: &[String], name: &str, suffix: StrKey) -> String {
    let derived = strings.fmt(suffix, &[("name", name)]);
    if taken.iter().any(|t| t == &derived) {
        let taken: Vec<&str> = taken.iter().map(String::as_str).collect();
        copy_name(&taken, &derived)
    } else {
        derived
    }
}

/// The offsets of shapes `ids` as [`offset`] would make them, in project
/// space: the popover's preview. Empty for a distance that is not above
/// 0.
#[must_use]
pub fn offset_outlines(project: &Project, ids: &[InstanceId], offset: &Offset) -> Vec<Polyline> {
    if !(offset.distance_mm.is_finite() && offset.distance_mm > 0.0) {
        return Vec::new();
    }
    ids.iter()
        .filter_map(|id| find_instance(project, id).ok())
        .flat_map(|instance| {
            let scale = crate::shape::even_scale(instance.transform);
            let profiles = project
                .definitions
                .get(&instance.definition)
                .map(|definition| definition.profiles.as_slice())
                .unwrap_or_default();
            scale
                .map(|scale| offset_profiles(profiles, offset.distance_mm / scale, offset.side))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|outline| instance.transform.apply_polyline(&outline).ok())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// **Shape ▸ Offset…** on shapes `ids` (capability 94): each offset by
/// itself, the result a new plain path named `<name> offset`, placed as
/// its source is, beside it or, with `keep_original` off, in its place
/// with what cut it.
///
/// # Errors
///
/// [`CommandError::NothingSelected`] for no shapes;
/// [`CommandError::InvalidValue`] for a distance that is not above 0;
/// [`CommandError::Stretched`] for a shape whose placement is not an even
/// scale; [`CommandError::NotAQuantity`] with the reason when every
/// offset vanishes.
pub fn offset(
    project: &Project,
    strings: &Strings,
    ids: &[InstanceId],
    offset: &Offset,
) -> Result<Offsets, CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    if !(offset.distance_mm.is_finite() && offset.distance_mm > 0.0) {
        return Err(CommandError::InvalidValue {
            field: "distance",
            value: offset.distance_mm,
        });
    }
    let mut taken: Vec<String> = project.instances.iter().map(|i| i.name.clone()).collect();
    let mut made = Vec::new();
    let mut from = Vec::new();
    let mut inherit = Vec::new();
    let mut vanished = Vec::new();
    for id in ids {
        let instance = find_instance(project, id)?;
        let scale = crate::shape::even_scale(instance.transform).ok_or_else(|| {
            CommandError::Stretched {
                name: instance.name.clone(),
            }
        })?;
        let profiles = project
            .definitions
            .get(&instance.definition)
            .map(|definition| definition.profiles.as_slice())
            .unwrap_or_default();
        let outlines = offset_profiles(profiles, offset.distance_mm / scale, offset.side);
        if outlines.is_empty() {
            vanished.push(instance.name.clone());
            continue;
        }
        let name = derived_name(strings, &taken, &instance.name, StrKey::EditNameOffset);
        taken.push(name.clone());
        inherit.push((id.clone(), made.len()));
        from.push(id.clone());
        made.push(derived(instance, name, outlines));
    }
    if made.is_empty() {
        let units = project.display_units;
        return Err(CommandError::NotAQuantity {
            text: units::field_text(offset.distance_mm, units),
            reason: strings.fmt(
                StrKey::EditRefusedOffsetNothing,
                &[("names", &vanished.join(", "))],
            ),
        });
    }
    Ok(Offsets {
        command: Command::ReplaceShapes {
            from,
            keep: offset.keep_original,
            add: made,
            inherit,
            what: Edit::Offset,
        },
        vanished,
    })
}

/// A plain path made from `instance`: placed, filed and marked as it is.
fn derived(instance: &FeatureInstance, name: String, profiles: Vec<Polyline>) -> NewShape {
    NewShape {
        name,
        shape: None,
        profiles,
        transform: instance.transform,
        construction: instance.construction,
        folder: instance.folder.clone(),
    }
}

/// What **Shape ▸ Weld**, **Subtract** and **Intersect** make of two or
/// more closed shapes (capability 95).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boolean {
    /// Their union, one shape.
    Weld,
    /// The first less every other.
    Subtract,
    /// What all of them share.
    Intersect,
}

/// The area shape `instance` encloses, in project space.
fn placed_regions(
    project: &Project,
    strings: &Strings,
    instance: &FeatureInstance,
) -> Result<Vec<Region>, CommandError> {
    let not_closed = || CommandError::NotAQuantity {
        text: instance.name.clone(),
        reason: strings.fmt(StrKey::EditRefusedNotClosed, &[("name", &instance.name)]),
    };
    let profiles = project
        .definitions
        .get(&instance.definition)
        .map(|definition| definition.profiles.as_slice())
        .unwrap_or_default();
    if profiles.is_empty() || profiles.iter().any(|profile| !profile.is_closed()) {
        return Err(not_closed());
    }
    let placed: Vec<Polyline> = profiles
        .iter()
        .map(|profile| instance.transform.apply_polyline(profile))
        .collect::<Result<_, _>>()?;
    regions(&placed)
        .filter(|r| !r.is_empty())
        .ok_or_else(not_closed)
}

/// **Shape ▸ Weld**, **Subtract** or **Intersect** on shapes `ids`, in
/// the order they were selected: the originals are replaced by the one
/// result, a plain path named for the first (`<name> weld`), placed as
/// the first is, with holes where the result has holes, and what cut the
/// first cuts it. Curved edges come out as straight segments within the
/// project's flatten tolerance, as the engine's boolean makes them.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] with the reason: fewer than two
/// shapes, one that is not closed, or a result of nothing (no overlap to
/// intersect, a first shape wholly taken away).
pub fn boolean(
    project: &Project,
    strings: &Strings,
    ids: &[InstanceId],
    op: Boolean,
) -> Result<Command, CommandError> {
    let refused = |key: StrKey| CommandError::NotAQuantity {
        text: String::new(),
        reason: strings.get(key).to_owned(),
    };
    let [first_id, rest @ ..] = ids else {
        return Err(refused(StrKey::EditRefusedTwoShapes));
    };
    if rest.is_empty() {
        return Err(refused(StrKey::EditRefusedTwoShapes));
    }
    let first = find_instance(project, first_id)?;
    let tolerance_mm = project.flatten_tolerance_mm;
    let mut result = placed_regions(project, strings, first)?;
    for id in rest {
        let other = placed_regions(project, strings, find_instance(project, id)?)?;
        let engine = match op {
            Boolean::Weld => BooleanOp::Union,
            Boolean::Subtract => BooleanOp::Difference,
            Boolean::Intersect => BooleanOp::Intersection,
        };
        result = dc_geometry::boolean(&result, &other, engine, tolerance_mm);
    }
    if result.is_empty() {
        return Err(refused(match op {
            Boolean::Intersect => StrKey::EditRefusedNoOverlap,
            Boolean::Weld | Boolean::Subtract => StrKey::EditRefusedNothingLeft,
        }));
    }
    // Back into the first shape's own space, where the result is placed.
    let to_definition = first
        .transform
        .inverse()
        .ok_or(CommandError::SingularTransform)?;
    let mut profiles = Vec::new();
    for region in &result {
        profiles.push(to_definition.apply_polyline(region.outer().polyline())?);
        for hole in region.holes() {
            profiles.push(to_definition.apply_polyline(hole.polyline())?);
        }
    }
    let (suffix, what) = match op {
        Boolean::Weld => (StrKey::EditNameWeld, Edit::Weld),
        Boolean::Subtract => (StrKey::EditNameSubtract, Edit::Subtract),
        Boolean::Intersect => (StrKey::EditNameIntersect, Edit::Intersect),
    };
    let taken: Vec<String> = project.instances.iter().map(|i| i.name.clone()).collect();
    let name = derived_name(strings, &taken, &first.name, suffix);
    Ok(Command::ReplaceShapes {
        from: ids.to_vec(),
        keep: false,
        add: vec![derived(first, name, profiles)],
        inherit: vec![(first_id.clone(), 0)],
        what,
    })
}

/// How near two ends must be for **Shape ▸ Join** to join them, mm, until
/// another is typed (capability 97).
pub const JOIN_TOLERANCE_MM: f64 = 0.1;

/// The command a join makes, and how many of the selected shapes it left
/// as they were.
#[derive(Debug, Clone, PartialEq)]
pub struct Joins {
    /// One `ReplaceShapes`: each chain of paths one shape, named, placed
    /// and cut as the first of them was.
    pub command: Command,
    /// The selected shapes that reached nothing, or are no single open
    /// path of their own: left alone, to be counted in the status bar.
    pub alone: usize,
}

/// **Shape ▸ Join** on shapes `ids` (capability 97): the open paths among
/// them whose ends meet, or lie within `tolerance_mm` as they are placed,
/// become one path, end to end, a straight segment bridging any gap; a
/// chain whose two ends meet closes. A shape takes part when it is one
/// open outline of its own, with no linked copy.
///
/// # Errors
///
/// [`CommandError::NothingSelected`] for no shapes;
/// [`CommandError::NotAQuantity`] with the reason when no two ends are
/// within the tolerance.
pub fn join(
    project: &Project,
    strings: &Strings,
    ids: &[InstanceId],
    tolerance_mm: f64,
) -> Result<Joins, CommandError> {
    if ids.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    // The shapes that can take part, each with its outline as placed.
    let mut members: Vec<(&FeatureInstance, Polyline)> = Vec::new();
    for id in ids {
        let instance = find_instance(project, id)?;
        let Some(definition) = project.definitions.get(&instance.definition) else {
            continue;
        };
        let linked = project
            .instances
            .iter()
            .filter(|i| i.definition == instance.definition)
            .count()
            > 1;
        if let [outline] = definition.profiles.as_slice()
            && !outline.is_closed()
            && !linked
            && !instance.locked
            && let Ok(placed) = instance.transform.apply_polyline(outline)
        {
            members.push((instance, placed));
        }
    }
    let placed: Vec<Polyline> = members.iter().map(|(_, outline)| outline.clone()).collect();
    let joined = dc_geometry::join_open(&placed, tolerance_mm);
    let mut from = Vec::new();
    let mut made = Vec::new();
    let mut inherit = Vec::new();
    for chain in &joined.chains {
        let Some((first, _)) = chain.from.first().and_then(|index| members.get(*index)) else {
            continue;
        };
        // The chain in its first path's own space, placed as that is.
        let Some(to_definition) = first.transform.inverse() else {
            continue;
        };
        let outline = to_definition.apply_polyline(&chain.outline)?;
        inherit.push((first.id.clone(), made.len()));
        made.push(derived(first, first.name.clone(), vec![outline]));
        from.extend(
            chain
                .from
                .iter()
                .filter_map(|index| members.get(*index))
                .map(|(instance, _)| instance.id.clone()),
        );
    }
    if made.is_empty() {
        let units = project.display_units;
        return Err(CommandError::NotAQuantity {
            text: units::field_text(tolerance_mm, units),
            reason: strings.fmt(
                StrKey::EditRefusedJoinNothing,
                &[("tolerance", &units::field_text(tolerance_mm, units))],
            ),
        });
    }
    let alone = ids.len() - from.len();
    Ok(Joins {
        command: Command::ReplaceShapes {
            from,
            keep: false,
            add: made,
            inherit,
            what: Edit::Join,
        },
        alone,
    })
}
