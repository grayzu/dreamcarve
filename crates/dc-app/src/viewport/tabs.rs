// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Placed tabs on the Viewport (`docs/specs/phase-4/design.md`,
//! "Architecture → Tabs"; functional spec, capability 69; ADR-0005): where
//! a dragged tab lands ([`tab_drag`]), which outline point the Tab tool
//! clicked ([`outline_at`]) and the tab it adds there ([`add_tab`]), which
//! tab a click lands on ([`tab_at`], so tabs win over the shape they sit
//! on), which tabs no enabled Profile crosses ([`crossings`], drawn
//! hollow), and the Properties panel's edits of a tab's size and shape
//! ([`edit`]).
//!
//! A tab's point is in its instance's definition space, so it follows the
//! instance when it moves, rotates, mirrors or scales. A drag and the Tab
//! tool both take the pointer's project point into definition space with
//! the instance's inverse transform and snap it to the nearest point of
//! the definition's profile there: on the outline, never off it.

use std::collections::BTreeMap;

use dc_geometry::{Point, Polyline, Vec2, nearest_point};
use dc_project::{InstanceId, Operation, OperationId, PlacedTab, Project, TabId, TabShape};

use crate::command::Command;
use crate::error::CommandError;
use crate::prefs::TabSize;
use crate::scene::{Scene, TabMark};
use crate::viewport::camera::{Camera, Pos};

/// A point of an instance's outline, as the Tab tool clicked it.
#[derive(Debug, Clone, PartialEq)]
pub struct OutlinePoint {
    /// The instance.
    pub instance: InstanceId,
    /// The outline: an index into its definition's profiles.
    pub profile: usize,
    /// The point, in the instance's definition space, mm, on the outline.
    pub at_mm: Point,
}

/// The definition-space profile `profile` of `instance`, and the
/// instance's inverse transform.
fn definition_profile<'p>(
    project: &'p Project,
    instance: &InstanceId,
    profile: usize,
) -> Option<(&'p Polyline, dc_geometry::Transform)> {
    let instance = project.instances.iter().find(|i| i.id == *instance)?;
    let outline = project
        .definitions
        .get(&instance.definition)?
        .profiles
        .get(profile)?;
    Some((outline, instance.transform.inverse()?))
}

/// Where dragging `tab` to project point `to_mm` puts it: the nearest
/// point of its own profile to `to_mm` taken into definition space, as
/// `(x_mm, y_mm)` for [`Command::MoveTab`]. The tab's own point when its
/// instance, definition or profile is not in the project, or the
/// instance's transform cannot be inverted.
#[must_use]
pub fn tab_drag(project: &Project, tab: &PlacedTab, to_mm: Point) -> (f64, f64) {
    definition_profile(project, &tab.instance, tab.profile)
        .and_then(|(outline, inverse)| nearest_point(outline, inverse.apply(to_mm)))
        .map_or((tab.x_mm, tab.y_mm), |near| (near.point.x, near.point.y))
}

/// Where `tab` is drawn, in project space: its point placed by its
/// instance's transform, and the unit direction of its outline at the
/// outline's point nearest it, along which its span runs (the X axis when
/// the outline has no length there). `None` when its instance is not in
/// the project.
#[must_use]
pub fn tab_frame(project: &Project, tab: &PlacedTab) -> Option<(Point, Vec2)> {
    let instance = project.instances.iter().find(|i| i.id == tab.instance)?;
    let at_mm = instance.transform.apply(Point::new(tab.x_mm, tab.y_mm));
    let tangent = project
        .definitions
        .get(&instance.definition)
        .and_then(|d| d.profiles.get(tab.profile))
        .and_then(|p| instance.transform.apply_polyline(p).ok())
        .and_then(|outline| {
            let near = nearest_point(&outline, at_mm)?;
            outline.point_at(near.along_mm).map(|(_, t)| t)
        })
        .unwrap_or(Vec2::new(1.0, 0.0));
    Some((at_mm, tangent))
}

/// The outline point under pane point `at`, for the Tab tool: of the
/// visible shapes' outlines within `tolerance_px`, the nearest (the
/// topmost on a tie), its instance and profile, and the point of it
/// nearest the click in definition space. `None` when no outline is that
/// close.
#[must_use]
pub fn outline_at(
    scene: &Scene,
    project: &Project,
    camera: &Camera,
    at: Pos,
    tolerance_px: f64,
) -> Option<OutlinePoint> {
    let p = camera.to_world(at);
    let tolerance_mm = camera.px_to_mm(tolerance_px);
    let mut best: Option<(f64, &InstanceId, usize, Point)> = None;
    for shape in scene.shapes() {
        let Some(bounds) = shape.bounds else {
            continue;
        };
        if p.x < bounds.min.x - tolerance_mm
            || p.x > bounds.max.x + tolerance_mm
            || p.y < bounds.min.y - tolerance_mm
            || p.y > bounds.max.y + tolerance_mm
        {
            continue;
        }
        for (profile, outline) in shape.outlines.iter().enumerate() {
            let Some(near) = nearest_point(outline, p) else {
                continue;
            };
            // Later shapes draw on top, so they win a tie.
            if near.gap_mm <= tolerance_mm && best.is_none_or(|(gap, ..)| near.gap_mm <= gap) {
                best = Some((near.gap_mm, &shape.instance, profile, near.point));
            }
        }
    }
    let (_, instance, profile, point) = best?;
    let (outline, inverse) = definition_profile(project, instance, profile)?;
    let at_mm = nearest_point(outline, inverse.apply(point))?.point;
    Some(OutlinePoint {
        instance: instance.clone(),
        profile,
        at_mm,
    })
}

/// The next free tab id, `tab<n>`: one more than the largest `n` the
/// project uses, as `tabs place` numbers them.
#[must_use]
pub fn next_tab_id(project: &Project) -> TabId {
    let n = project
        .tabs
        .iter()
        .filter_map(|t| t.id.as_str().strip_prefix("tab")?.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n.saturating_add(1));
    TabId::new(format!("tab{n}"))
}

/// The Tab tool's command: a tab at `at` with `size` (the last-used, from
/// [`Prefs::tab`](crate::Prefs)) under the next free id. Like a tab
/// `tabs place --at` puts on an outline it does not hold a slug
/// (`holds_slug: false`), so a tab on a hole warns `tab_on_hole`.
#[must_use]
pub fn add_tab(project: &Project, at: &OutlinePoint, size: TabSize) -> Command {
    Command::AddTab(PlacedTab {
        id: next_tab_id(project),
        instance: at.instance.clone(),
        profile: at.profile,
        x_mm: at.at_mm.x,
        y_mm: at.at_mm.y,
        width_mm: size.width_mm,
        height_mm: size.height_mm,
        shape: size.shape,
        holds_slug: Some(false),
    })
}

/// The enabled Profiles that cross each placed tab
/// (`dc_job::tabs::crossed_by`), in operation order, by tab. A Profile
/// that cannot be cut crosses none. A tab missing here is crossed by none
/// and is drawn hollow. Computed with the Scene, once per snapshot.
#[must_use]
pub fn crossings(project: &Project) -> BTreeMap<TabId, Vec<OperationId>> {
    let mut crossed: BTreeMap<TabId, Vec<OperationId>> = BTreeMap::new();
    if project.tabs.is_empty() {
        return crossed;
    }
    for op in &project.operations {
        let Operation::Profile(profile) = op else {
            continue;
        };
        if !profile.enabled {
            continue;
        }
        for id in dc_job::tabs::crossed_by(project, profile).unwrap_or_default() {
            crossed.entry(id).or_default().push(profile.id.clone());
        }
    }
    crossed
}

/// The placed tabs no enabled Profile crosses, in project order: those
/// the Viewport draws hollow (and `check` warns `tab_not_crossed` for).
#[must_use]
pub fn hollow(project: &Project) -> Vec<TabId> {
    let crossed = crossings(project);
    project
        .tabs
        .iter()
        .filter(|t| !crossed.contains_key(&t.id))
        .map(|t| t.id.clone())
        .collect()
}

/// The distance from `p` to the segment `a`–`b`, mm.
fn segment_distance_mm(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length2 = dx * dx + dy * dy;
    let t = if length2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance_to(Point::new(a.x + t * dx, a.y + t * dy))
}

/// The ends of `mark`'s span along its outline, mm in project space:
/// its width long, centred on its point.
#[must_use]
pub fn span(mark: &TabMark) -> (Point, Point) {
    let half = mark.width_mm / 2.0;
    let (tx, ty) = (mark.tangent.x * half, mark.tangent.y * half);
    (
        Point::new(mark.at_mm.x - tx, mark.at_mm.y - ty),
        Point::new(mark.at_mm.x + tx, mark.at_mm.y + ty),
    )
}

/// The tab under pane point `at`: of the tabs on visible shapes whose
/// span ([`span`]) is within `tolerance_px`, the nearest, the last in the
/// project on a tie. Tabs win a click over the shape they sit on
/// ([`crate::viewport::pick::pick`]).
#[must_use]
pub fn tab_at(scene: &Scene, camera: &Camera, at: Pos, tolerance_px: f64) -> Option<TabId> {
    let p = camera.to_world(at);
    let tolerance_mm = camera.px_to_mm(tolerance_px);
    let mut best: Option<(f64, &TabId)> = None;
    for mark in scene
        .tabs()
        .iter()
        .filter(|m| scene.shape(&m.instance).is_some())
    {
        let (a, b) = span(mark);
        let d = segment_distance_mm(p, a, b);
        if d <= tolerance_mm && best.is_none_or(|(gap, _)| d <= gap) {
            best = Some((d, &mark.id));
        }
    }
    best.map(|(_, id)| id.clone())
}

/// A change the Properties panel makes to the selected tabs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TabEdit {
    /// The width, mm.
    Width(f64),
    /// The height above the stock bottom, mm.
    Height(f64),
    /// The cross-section.
    Shape(TabShape),
}

/// What the selected tabs share: each value when every tab has it, else
/// `None` (the panel's *Mixed*).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabValues {
    /// The common width, mm.
    pub width_mm: Option<f64>,
    /// The common height, mm.
    pub height_mm: Option<f64>,
    /// The common shape.
    pub shape: Option<TabShape>,
}

/// The tabs of `project` named by `ids`, in project order.
fn selected<'p>(project: &'p Project, ids: &[TabId]) -> Vec<&'p PlacedTab> {
    project
        .tabs
        .iter()
        .filter(|t| ids.contains(&t.id))
        .collect()
}

/// The values the selected tabs share; `None` when none is in the project.
#[must_use]
pub fn values(project: &Project, ids: &[TabId]) -> Option<TabValues> {
    let tabs = selected(project, ids);
    let first = tabs.first()?;
    let common = |f: fn(&PlacedTab) -> f64| {
        let v = f(first);
        tabs.iter()
            .all(|t| f(t).to_bits() == v.to_bits())
            .then_some(v)
    };
    Some(TabValues {
        width_mm: common(|t| t.width_mm),
        height_mm: common(|t| t.height_mm),
        shape: tabs
            .iter()
            .all(|t| t.shape == first.shape)
            .then_some(first.shape),
    })
}

/// The command that applies `edit` to every selected tab, one undo step
/// ([`Command::UpdateTabs`]).
///
/// # Errors
///
/// [`CommandError::NothingSelected`] when none of `ids` is a tab of the
/// project; [`CommandError::InvalidValue`] for a width or height that is
/// not a positive number; [`CommandError::NothingToDo`] when every tab
/// already has the value.
pub fn edit(project: &Project, ids: &[TabId], edit: TabEdit) -> Result<Command, CommandError> {
    let tabs = selected(project, ids);
    if tabs.is_empty() {
        return Err(CommandError::NothingSelected);
    }
    match edit {
        TabEdit::Width(value) if !(value.is_finite() && value > 0.0) => {
            return Err(CommandError::InvalidValue {
                field: "tabs[].width_mm",
                value,
            });
        }
        TabEdit::Height(value) if !(value.is_finite() && value > 0.0) => {
            return Err(CommandError::InvalidValue {
                field: "tabs[].height_mm",
                value,
            });
        }
        _ => {}
    }
    let changed: Vec<PlacedTab> = tabs
        .into_iter()
        .map(|tab| {
            let mut next = tab.clone();
            match edit {
                TabEdit::Width(v) => next.width_mm = v,
                TabEdit::Height(v) => next.height_mm = v,
                TabEdit::Shape(s) => next.shape = s,
            }
            next
        })
        .filter(|next| project.tabs.iter().all(|t| t != next))
        .collect();
    if changed.is_empty() {
        return Err(CommandError::NothingToDo);
    }
    Ok(Command::UpdateTabs(changed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::sample_project;

    #[test]
    fn next_id_follows_the_largest_number() {
        let mut project = sample_project();
        project.tabs.clear();
        assert_eq!(next_tab_id(&project).as_str(), "tab1");
    }

    #[test]
    fn segment_distance_clamps_to_the_ends() {
        let (a, b) = (Point::new(0.0, 0.0), Point::new(10.0, 0.0));
        assert!((segment_distance_mm(Point::new(5.0, 3.0), a, b) - 3.0).abs() < 1e-12);
        assert!((segment_distance_mm(Point::new(13.0, 4.0), a, b) - 5.0).abs() < 1e-12);
        assert!((segment_distance_mm(Point::new(1.0, 1.0), a, a) - 2f64.sqrt()).abs() < 1e-12);
    }
}
