// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 17 of phase 4 (`docs/specs/phase-4/design.md`, "Architecture →
//! Tabs" and "Test plan"; functional capability 69, acceptance 78; #546):
//! *Place tabs* writes what `tabs place` writes and clears the request,
//! a dragged tab stays on its outline in definition space, the Tab tool
//! adds a tab at the clicked outline point with the last-used size, and a
//! tab no enabled Profile crosses is drawn hollow.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::prefs::TabSize;
use dc_app::test_support::Harness;
use dc_app::viewport::pick::{self, PICK_TOLERANCE_PX};
use dc_app::viewport::tabs::{self, OutlinePoint};
use dc_app::{Camera, Command, Hit, Selection};
use dc_geometry::{Point, Polyline, Transform, Vec2, nearest_point};
use dc_project::{
    Operation, OperationId, PlacedTab, Project, TabPlacement, TabShape, TabSpec,
};

use common::coaster;

/// The coaster's one operation, its Profile.
fn profile_id(project: &Project) -> OperationId {
    project.operations[0].id().clone()
}

/// The request `tabs place --count 4` stands for, with its defaults:
/// four 8 × 3 mm rect tabs, off corners, none on holes. `init`'s request
/// is the same.
fn count_four() -> TabSpec {
    TabSpec {
        placement: TabPlacement::Count { count: 4 },
        width_mm: 8.0,
        height_mm: 3.0,
        shape: TabShape::Rect,
        on_holes: false,
        avoid_corners: true,
    }
}

/// The definition-space profile `tab` sits on.
fn outline_of<'p>(project: &'p Project, tab: &PlacedTab) -> &'p Polyline {
    let instance = project
        .instances
        .iter()
        .find(|i| i.id == tab.instance)
        .expect("the tab's instance");
    &project.definitions[&instance.definition].profiles[tab.profile]
}

/// The coaster moved off `init`'s place and turned, so its definition
/// space is not project space, with its request placed.
fn moved_coaster_with_tabs() -> Harness {
    let mut h = Harness::from_project(coaster());
    let id = h.app.project().instances[0].id.clone();
    h.run(Command::TransformInstances {
        ids: vec![id],
        delta: Transform::translate(7.0, -3.0).then(Transform::rotate(0.4)),
    })
    .expect("moved");
    let op = profile_id(h.app.project());
    h.app.place_tabs(&op).expect("the request places");
    h
}

/// A camera that shows the whole stock in a 1200 × 800 pane.
fn camera_on(project: &Project) -> Camera {
    let mut camera = Camera {
        centre_mm: Point::new(0.0, 0.0),
        px_per_mm: 1.0,
        viewport_px: Vec2::new(1200.0, 800.0),
    };
    camera.fit(dc_geometry::Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(project.stock.width_mm, project.stock.height_mm),
    });
    camera
}

/// Acceptance 78, first part: *Place tabs* on the coaster's Profile with
/// `init`'s request of four writes the four tabs `tabs place --count 4`
/// writes (`dc_job::tabs::place` of that request) and clears the request,
/// so the saved file holds placed tabs only; the step is one undo step,
/// *Place 4 tabs*. What placing found reaches the Warnings panel.
#[test]
fn place_tabs_equals_tabs_place_and_clears_the_request() {
    let project = coaster();
    let op = profile_id(&project);
    match &project.operations[0] {
        Operation::Profile(p) => assert_eq!(p.tabs, Some(count_four()), "init's request"),
        other => panic!("a Profile, not {}", other.kind()),
    }
    let cli = dc_job::tabs::place(&project, &op, &count_four()).expect("tabs place --count 4");
    assert_eq!(cli.tabs.len(), 4);

    let mut h = Harness::from_project(project.clone());
    let label = h.app.place_tabs(&op).expect("placed");
    assert_eq!(label, "Place 4 tabs");
    assert_eq!(h.app.undo_label(), Some("Place 4 tabs"));
    assert_eq!(h.app.project().tabs, cli.tabs, "the tabs `tabs place` writes");
    match &h.app.project().operations[0] {
        Operation::Profile(p) => assert_eq!(p.tabs, None, "the request is cleared"),
        other => panic!("a Profile, not {}", other.kind()),
    }
    // The saved file is what `place_request` (the CLI's `tabs place`)
    // leaves.
    let (placed, _) = dc_job::tabs::place_request(&project, &op).expect("place_request");
    assert_eq!(
        h.save().unwrap(),
        dc_project::save(&placed).expect("saves")
    );
    // A Profile with no request left has nothing to place.
    assert!(h.app.place_tabs(&op).is_err());

    // A disabled Profile keeps its request until it is placed by name.
    let mut disabled = project.clone();
    if let Operation::Profile(p) = &mut disabled.operations[0] {
        p.enabled = false;
    }
    let mut h = Harness::from_project(disabled);
    assert!(h.app.project().tabs.is_empty());
    h.app.place_tabs(&op).expect("placed by name");
    assert_eq!(h.app.project().tabs, cli.tabs);

    // Placing's warnings reach the Warnings panel: fifty 8 mm tabs on the
    // coaster's path cover more than half of it.
    let mut crowded = project;
    if let Operation::Profile(p) = &mut crowded.operations[0] {
        p.tabs = Some(TabSpec {
            placement: TabPlacement::Count { count: 50 },
            ..count_four()
        });
    }
    let (_, warnings) = dc_job::tabs::place_request(&crowded, &op).expect("place_request");
    assert!(!warnings.is_empty(), "the crowded request warns");
    let mut h = Harness::from_project(crowded);
    h.app.place_tabs(&op).expect("placed");
    for w in &warnings {
        let code = w.code().as_str();
        assert!(
            h.app.notes().iter().any(|n| n.contains(code)),
            "{code} in the notes: {:?}",
            h.app.notes()
        );
    }
}

/// Acceptance 78: dragging a tab moves it to the nearest point of its own
/// outline in definition space, whatever its instance's transform; the
/// release is one `MoveTab` that changes only that tab's point. Deleting
/// one of the four leaves three.
#[test]
fn tab_drag_snaps_to_the_outline_in_definition_space() {
    let mut h = moved_coaster_with_tabs();
    let project = h.app.project().clone();
    let tab = project.tabs[0].clone();
    let instance = &project.instances[0];
    let outline = outline_of(&project, &tab);

    // Somewhere off the outline, in project space: 6 mm out from the
    // coaster's centre past its edge, a quarter turn round from the tab.
    let centre = instance.transform.apply(Point::new(
        outline.bounds().map_or(0.0, |b| (b.min.x + b.max.x) / 2.0),
        outline.bounds().map_or(0.0, |b| (b.min.y + b.max.y) / 2.0),
    ));
    let to_mm = Point::new(centre.x + 6.0, centre.y + 51.0);
    let (x_mm, y_mm) = tabs::tab_drag(&project, &tab, to_mm);

    // The nearest point of the definition's profile to the drag point
    // taken into definition space.
    let inverse = instance.transform.inverse().expect("invertible");
    let want = nearest_point(outline, inverse.apply(to_mm)).expect("nearest");
    assert!(
        Point::new(x_mm, y_mm).distance_to(want.point) < 1e-9,
        "({x_mm}, {y_mm}), want {:?}",
        want.point
    );
    let on = nearest_point(outline, Point::new(x_mm, y_mm)).expect("nearest");
    assert!(on.gap_mm < 1e-9, "on the outline: {}", on.gap_mm);

    h.run(Command::MoveTab {
        id: tab.id.clone(),
        x_mm,
        y_mm,
    })
    .expect("moved");
    assert_eq!(h.app.undo_label(), Some("Move tab"));
    let after = h.app.project();
    let moved = after.tabs.iter().find(|t| t.id == tab.id).unwrap();
    assert_eq!(
        moved,
        &PlacedTab {
            x_mm,
            y_mm,
            ..tab.clone()
        },
        "only the point changes"
    );
    assert_eq!(after.tabs[1..], project.tabs[1..], "the others stay");

    // A tab whose instance is missing stays where it is.
    let mut orphan = tab.clone();
    orphan.instance = dc_project::InstanceId::new("gone");
    assert_eq!(
        tabs::tab_drag(&project, &orphan, to_mm),
        (orphan.x_mm, orphan.y_mm)
    );

    // Delete removes one; three are left.
    h.run(Command::DeleteTabs(vec![tab.id.clone()])).expect("deleted");
    assert_eq!(h.app.project().tabs.len(), 3);
}

/// Acceptance 78: the Tab tool adds a tab where the user clicks an
/// outline (the instance, the nearest profile and that point taken into
/// definition space), with the last-used width, height and shape from
/// `Prefs`; a click away from every outline adds nothing. A tab wins the
/// pick over the shape it sits on, and selects as a tab.
#[test]
fn add_tab_at_clicked_point_with_last_size() {
    let mut h = moved_coaster_with_tabs();
    let size = TabSize {
        width_mm: 11.0,
        height_mm: 2.5,
        shape: TabShape::Smooth,
    };
    h.app.prefs_mut().tab = size;
    let project = h.app.project().clone();
    let instance = &project.instances[0];
    let outline = &project.definitions[&instance.definition].profiles[0];
    let camera = camera_on(&project);

    // A point of the outline away from every tab, in project space, and
    // a click 2 px off it.
    let along = outline.length_mm() * 0.6;
    let (on_def, _) = outline.point_at(along).expect("a point");
    let on = instance.transform.apply(on_def);
    let mut click = camera.to_screen(on);
    click.x_px += 2.0;

    let found = tabs::outline_at(h.app.scene(), &project, &camera, click, PICK_TOLERANCE_PX)
        .expect("the click is on the outline");
    let OutlinePoint {
        instance: hit,
        profile,
        at_mm,
    } = found.clone();
    assert_eq!(hit, instance.id);
    assert_eq!(profile, 0);
    let gap = nearest_point(outline, at_mm).expect("nearest").gap_mm;
    assert!(gap < 1e-6, "in definition space, on the outline: {gap}");
    let clicked = instance
        .transform
        .inverse()
        .expect("invertible")
        .apply(camera.to_world(click));
    assert!(
        at_mm.distance_to(clicked) <= camera.px_to_mm(2.5),
        "{at_mm:?} is the outline's point nearest the click {clicked:?}"
    );

    let command = tabs::add_tab(&project, &found, h.app.prefs().tab);
    h.run(command).expect("added");
    assert_eq!(h.app.undo_label(), Some("Add tab"));
    let after = h.app.project();
    assert_eq!(after.tabs.len(), project.tabs.len() + 1);
    let added = after.tabs.last().unwrap();
    assert!(
        project.tabs.iter().all(|t| t.id != added.id),
        "a fresh id: {}",
        added.id
    );
    assert_eq!(added.instance, instance.id);
    assert_eq!(added.profile, 0);
    assert_eq!((added.x_mm, added.y_mm), (at_mm.x, at_mm.y));
    assert_eq!(added.width_mm, 11.0);
    assert_eq!(added.height_mm, 2.5);
    assert_eq!(added.shape, TabShape::Smooth);

    // Nowhere near an outline: nothing to add to.
    let away = camera.to_screen(Point::new(1.0, 1.0));
    assert_eq!(
        tabs::outline_at(h.app.scene(), after, &camera, away, PICK_TOLERANCE_PX),
        None
    );

    // The new tab wins the pick over the coaster it sits on.
    let added_id = added.id.clone();
    let at = h.app.scene().tab(&added_id).expect("in the Scene").at_mm;
    let hit = pick::pick(h.app.scene(), &camera, camera.to_screen(at), PICK_TOLERANCE_PX, 0);
    assert_eq!(hit, Hit::Tab(added_id.clone()));
    assert_eq!(
        Selection::Nothing.clicked(&hit, dc_app::ClickMode::Replace),
        Selection::Tabs(vec![added_id])
    );
}

/// The Viewport draws a tab no enabled Profile crosses hollow: every tab
/// the Profile placed is crossed while it is enabled and hollow once it
/// is disabled, and a tab added on an outline no Profile cuts is hollow.
/// The Scene holds the answer, computed with it, never per frame.
#[test]
fn hollow_when_no_profile_crosses() {
    let mut h = moved_coaster_with_tabs();
    let op = profile_id(h.app.project());
    let scene = h.app.scene();
    assert_eq!(scene.tabs().len(), 4);
    for mark in scene.tabs() {
        assert_eq!(mark.crossed_by, vec![op.clone()], "{} is crossed", mark.id);
        assert!(!mark.hollow());
    }
    assert!(tabs::hollow(h.app.project()).is_empty());

    h.run(Command::SetOperationEnabled {
        id: op.clone(),
        enabled: false,
    })
    .expect("disabled");
    for mark in h.app.scene().tabs() {
        assert!(mark.hollow(), "{} is hollow with the Profile off", mark.id);
    }
    let all: Vec<_> = h.app.project().tabs.iter().map(|t| t.id.clone()).collect();
    assert_eq!(tabs::hollow(h.app.project()), all);

    h.run(Command::SetOperationEnabled { id: op, enabled: true })
        .expect("enabled");
    // A copy of the coaster no Profile targets: a tab on its outline is
    // hollow, and the Profile's own stay crossed.
    let original = h.app.project().instances[0].id.clone();
    h.run(Command::DuplicateInstances(vec![original])).expect("duplicated");
    let project = h.app.project().clone();
    let copy = project.instances.last().unwrap();
    let outline = &project.definitions[&copy.definition].profiles[0];
    let (on, _) = outline.point_at(10.0).expect("a point");
    let at = OutlinePoint {
        instance: copy.id.clone(),
        profile: 0,
        at_mm: on,
    };
    h.run(tabs::add_tab(&project, &at, TabSize::default()))
        .expect("added");
    let added = h.app.project().tabs.last().unwrap().id.clone();
    let marks = h.app.scene().tabs();
    assert!(marks.iter().find(|m| m.id == added).unwrap().hollow());
    assert_eq!(marks.iter().filter(|m| m.hollow()).count(), 1);
    assert_eq!(tabs::hollow(h.app.project()), vec![added]);
}
