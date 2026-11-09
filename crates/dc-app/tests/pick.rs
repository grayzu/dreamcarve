// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/canvas/hitTest.test.ts

//! Slice 4 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan →
//! Viewport geometry"; #533): picking on the Viewport (functional
//! capability 75). The first test carries PureCutCNC's `hitTest.test.ts`
//! cases, numbered as upstream; the rest are DreamCarve's.
//!
//! Upstream draws Y-down. None of these cases depends on direction (each
//! is a containment or a distance), so the shapes and points keep their
//! numbers; a `rectProfile(x, y, w, h)` is `Polyline::rect` at `(x, y)`.
//! Upstream's `scale` is our `px_per_mm`; its 8-px tolerance is
//! `PICK_TOLERANCE_PX`. Where upstream answers *ambiguous* (several
//! outlines under the pointer, case 8) DreamCarve answers the first of the
//! candidates and lets a second click cycle, so those cases assert the
//! cycle order instead.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::collections::BTreeMap;

use dc_app::test_support::Harness;
use dc_app::viewport::camera::{Camera, Pos};
use dc_app::viewport::pick::{self, ClickCycle, Hit, PICK_TOLERANCE_PX};
use dc_app::{ClickMode, Command, Scene, Selection};
use dc_geometry::{Point, Polyline, Rect, Transform, Vec2};
use dc_project::test_support::sample_project;
use dc_project::{DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Project};

use common::plaque;

/// One shape of a test project: an instance of its own definition.
struct Shape {
    id: &'static str,
    profile: Polyline,
    transform: Transform,
    visible: bool,
    locked: bool,
}

/// `makeFeature(id, rectProfile(x, y, w, h))`.
fn rect(id: &'static str, x: f64, y: f64, w: f64, h: f64) -> Shape {
    Shape {
        id,
        profile: Polyline::rect(Point::new(x, y), w, h),
        transform: Transform::IDENTITY,
        visible: true,
        locked: false,
    }
}

/// `makeProject(features, transforms)`: the shapes in draw order, first
/// at the bottom, on the sample project's 100 × 100 mm stock.
fn project(shapes: Vec<Shape>) -> Project {
    let mut project = sample_project();
    project.operations.clear();
    project.tabs.clear();
    project.definitions = BTreeMap::new();
    project.instances = Vec::new();
    for shape in shapes {
        let definition = DefinitionId::new(shape.id);
        project.definitions.insert(
            definition.clone(),
            FeatureDefinition {
                id: definition.clone(),
                name: shape.id.to_owned(),
                profiles: vec![shape.profile],
            },
        );
        project.instances.push(FeatureInstance {
            id: InstanceId::new(shape.id),
            definition,
            transform: shape.transform,
            name: shape.id.to_owned(),
            visible: shape.visible,
            locked: shape.locked,
            z_top_mm: None,
            z_bottom_mm: None,
        });
    }
    project
}

fn scene_of(project: Project) -> Scene {
    Harness::from_project(project).app.scene().clone()
}

/// A camera at `px_per_mm` whose screen point for project (0, 0) is
/// (100, 500): far enough in that every case's points are on screen.
fn view(px_per_mm: f64) -> Camera {
    let viewport_px = Vec2::new(2000.0, 1000.0);
    let mut camera = Camera {
        centre_mm: Point::new(0.0, 0.0),
        px_per_mm,
        viewport_px,
    };
    // From the centre (1000, 500) to (100, 500).
    camera.pan(-900.0, 0.0);
    camera
}

/// The candidates at project point `(x, y)`, as instance ids, in the order
/// clicks cycle through them.
fn ids_at(scene: &Scene, camera: &Camera, x: f64, y: f64) -> Vec<String> {
    let at = camera.to_screen(Point::new(x, y));
    pick::candidates(scene, camera, at, PICK_TOLERANCE_PX)
        .into_iter()
        .map(|id| id.as_str().to_owned())
        .collect()
}

/// What a first click at `(x, y)` picks.
fn hit_at(scene: &Scene, camera: &Camera, x: f64, y: f64) -> Hit {
    let at = camera.to_screen(Point::new(x, y));
    pick::pick(scene, camera, at, PICK_TOLERANCE_PX, 0)
}

fn instance(id: &str) -> Hit {
    Hit::Instance(InstanceId::new(id))
}

/// Upstream's `vt` (scale 1) and `preciseVt` (scale 10).
const SCALE_1: f64 = 1.0;
const SCALE_10: f64 = 10.0;

/// The cases of PureCutCNC's `hitTest.test.ts`, which pick through the
/// resolver (world-space outlines), with the nearest outline inside the
/// tolerance winning, then the innermost shape whose interior holds the
/// point, then the topmost.
#[test]
fn nearest_outline_within_tolerance_wins() {
    let vt = view(SCALE_1);
    let precise = view(SCALE_10);

    // 1. Identity-placed feature: inside hits, far away does not. (Upstream
    // case 5, "resolved feature preserves instance ID", is the same hit's
    // id: the instance's, not the definition's.)
    let scene = scene_of(project(vec![rect("f0001", 0.0, 0.0, 10.0, 6.0)]));
    assert_eq!(hit_at(&scene, &vt, 5.0, 3.0), instance("f0001"), "case 1");
    assert_eq!(hit_at(&scene, &vt, 50.0, 50.0), Hit::Nothing, "case 1");

    // 2. Transformed feature: hit at its project-space position, not at
    // its definition-space one.
    let mut moved = rect("f0001", 0.0, 0.0, 10.0, 6.0);
    moved.transform = Transform::translate(100.0, 50.0);
    let scene = scene_of(project(vec![moved]));
    assert_eq!(
        hit_at(&scene, &vt, 105.0, 53.0),
        instance("f0001"),
        "case 2"
    );
    assert_eq!(ids_at(&scene, &vt, 105.0, 53.0), ["f0001"], "case 2");
    assert_eq!(hit_at(&scene, &vt, 5.0, 3.0), Hit::Nothing, "case 2");

    // 3. The marquee (upstream `featureFullyInsideRect`) reads project
    // space too: a box round (100, 50)–(110, 56) takes the shape, a box
    // round its definition-space position does not.
    let selected = pick::touching(
        &scene,
        Rect {
            min: Point::new(99.0, 49.0),
            max: Point::new(111.0, 57.0),
        },
    );
    assert_eq!(selected, [InstanceId::new("f0001")], "case 3");
    let selected = pick::touching(
        &scene,
        Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(10.0, 6.0),
        },
    );
    assert!(selected.is_empty(), "case 3: {selected:?}");

    // 4. An instance whose definition is missing is left out, without a
    // panic.
    let mut orphan = project(vec![rect("orphan", 10.0, 20.0, 30.0, 40.0)]);
    orphan.definitions.clear();
    let scene = scene_of(orphan);
    assert!(scene.shapes().is_empty(), "case 4");
    assert_eq!(hit_at(&scene, &vt, 25.0, 40.0), Hit::Nothing, "case 4");

    // 6. Overlaps: topmost first, the hidden one never; the outline's
    // tolerance (4 mm off the edge at 1 px/mm, inside 8 px) counts too.
    let mut hidden = rect("hidden", 0.0, 0.0, 10.0, 6.0);
    hidden.visible = false;
    let scene = scene_of(project(vec![
        rect("bottom", 0.0, 0.0, 10.0, 6.0),
        hidden,
        rect("top", 0.0, 0.0, 10.0, 6.0),
    ]));
    assert_eq!(ids_at(&scene, &vt, 5.0, 3.0), ["top", "bottom"], "case 6");
    assert_eq!(hit_at(&scene, &vt, 5.0, 3.0), instance("top"), "case 6");
    assert_eq!(ids_at(&scene, &vt, 14.0, 3.0), ["top", "bottom"], "case 6");

    // 7. A unique nearby outline wins over a shape that contains the
    // point, though that one is on top.
    let scene = scene_of(project(vec![
        rect("outlined", 0.0, 0.0, 10.0, 10.0),
        rect("containing-top", -5.0, -5.0, 20.0, 20.0),
    ]));
    assert_eq!(
        hit_at(&scene, &precise, 0.2, 5.0),
        instance("outlined"),
        "case 7"
    );
    assert_eq!(
        ids_at(&scene, &precise, 0.2, 5.0),
        ["outlined", "containing-top"],
        "case 7: every candidate, the outline first"
    );

    // 8. Coincident outlines: upstream is ambiguous; here the topmost
    // first and the next click the one below.
    let scene = scene_of(project(vec![
        rect("bottom", 0.0, 0.0, 20.0, 20.0),
        rect("top", 0.0, 0.0, 20.0, 20.0),
    ]));
    assert_eq!(
        ids_at(&scene, &precise, 0.2, 10.0),
        ["top", "bottom"],
        "case 8"
    );

    // 9. A shared interior with no outline near: the topmost.
    assert_eq!(
        hit_at(&scene, &precise, 10.0, 10.0),
        instance("top"),
        "case 9"
    );
    assert_eq!(
        ids_at(&scene, &precise, 10.0, 10.0),
        ["top", "bottom"],
        "case 9"
    );

    // 10. A hole inside a material: inside the hole picks the hole, the
    // material round it the material, the hole's outline the hole; and
    // the same with the material drawn on top.
    let scene = scene_of(project(vec![
        rect("material", 0.0, 0.0, 40.0, 40.0),
        rect("hole", 15.0, 15.0, 10.0, 10.0),
    ]));
    assert_eq!(
        hit_at(&scene, &precise, 20.0, 20.0),
        instance("hole"),
        "case 10"
    );
    assert_eq!(
        ids_at(&scene, &precise, 20.0, 20.0),
        ["hole", "material"],
        "case 10"
    );
    assert_eq!(
        hit_at(&scene, &precise, 5.0, 5.0),
        instance("material"),
        "case 10"
    );
    assert_eq!(
        hit_at(&scene, &precise, 15.0, 20.0),
        instance("hole"),
        "case 10"
    );
    let covered = scene_of(project(vec![
        rect("hole", 15.0, 15.0, 10.0, 10.0),
        rect("material", 0.0, 0.0, 40.0, 40.0),
    ]));
    assert_eq!(
        hit_at(&covered, &precise, 20.0, 20.0),
        instance("hole"),
        "case 10, covered"
    );

    // 11. A lone candidate.
    let scene = scene_of(project(vec![rect("only", 0.0, 0.0, 20.0, 20.0)]));
    assert_eq!(
        hit_at(&scene, &precise, 10.0, 10.0),
        instance("only"),
        "case 11"
    );

    // DreamCarve: of two outlines inside the tolerance, the nearer wins
    // though it is below. 0.8 mm is the tolerance at 10 px/mm; the point
    // is 0.4 mm from the top shape's edge and 0.1 mm from the bottom's.
    let scene = scene_of(project(vec![
        rect("wider", 0.0, 0.0, 10.5, 10.0),
        rect("narrower", 0.0, 0.0, 10.0, 10.0),
    ]));
    assert_eq!(
        hit_at(&scene, &precise, 10.4, 5.0),
        instance("wider"),
        "the nearer outline"
    );
    // An open path is picked on its line only: it has no inside.
    let open = Shape {
        id: "open",
        profile: Polyline::new(
            vec![
                dc_geometry::Vertex::line(0.0, 0.0),
                dc_geometry::Vertex::line(20.0, 0.0),
                dc_geometry::Vertex::line(20.0, 20.0),
            ],
            false,
        ),
        transform: Transform::IDENTITY,
        visible: true,
        locked: false,
    };
    let scene = scene_of(project(vec![open]));
    assert_eq!(
        hit_at(&scene, &precise, 20.0, 10.0),
        instance("open"),
        "on the line"
    );
    assert_eq!(
        hit_at(&scene, &precise, 15.0, 5.0),
        Hit::Nothing,
        "not inside"
    );
    // An arc is picked on the arc, not on its chord.
    let scene = scene_of(project(vec![Shape {
        id: "circle",
        profile: Polyline::circle(Point::new(50.0, 50.0), 20.0),
        transform: Transform::IDENTITY,
        visible: true,
        locked: false,
    }]));
    assert_eq!(
        hit_at(&scene, &precise, 70.0, 50.0),
        instance("circle"),
        "on the arc"
    );
    assert_eq!(
        hit_at(&scene, &precise, 50.0, 70.2),
        instance("circle"),
        "on the arc"
    );
    assert_eq!(
        hit_at(&scene, &precise, 85.0, 85.0),
        Hit::Nothing,
        "outside"
    );
}

/// Functional capability 75: "Click selects an instance (the topmost where
/// several overlap; a second click cycles)". A click where the last one
/// was, over the same candidates, takes the next one, wrapping round; a
/// click elsewhere starts again at the first. Shift-click adds, Ctrl-click
/// toggles, a click on nothing clears unless it extends.
#[test]
fn click_cycles_overlapping_instances() {
    let scene = scene_of(project(vec![
        rect("a", 0.0, 0.0, 20.0, 20.0),
        rect("b", 0.0, 0.0, 20.0, 20.0),
        rect("c", 0.0, 0.0, 20.0, 20.0),
        rect("apart", 50.0, 50.0, 10.0, 10.0),
    ]));
    let camera = view(SCALE_10);
    let at = camera.to_screen(Point::new(10.0, 10.0));
    let mut clicks = ClickCycle::default();
    let picked: Vec<Hit> = (0..4)
        .map(|_| clicks.click(&scene, &camera, at, PICK_TOLERANCE_PX))
        .collect();
    assert_eq!(
        picked,
        [instance("c"), instance("b"), instance("a"), instance("c")],
        "topmost first, then down the stack, then round again"
    );
    // A pixel of jitter is the same place.
    let jittered = Pos::new(at.x_px + 1.0, at.y_px - 1.0);
    assert_eq!(
        clicks.click(&scene, &camera, jittered, PICK_TOLERANCE_PX),
        instance("b")
    );
    // Elsewhere, then back: the cycle starts over.
    let apart = camera.to_screen(Point::new(55.0, 55.0));
    assert_eq!(
        clicks.click(&scene, &camera, apart, PICK_TOLERANCE_PX),
        instance("apart")
    );
    assert_eq!(
        clicks.click(&scene, &camera, at, PICK_TOLERANCE_PX),
        instance("c")
    );

    // The hit becomes the selection by the click's modifiers.
    let one = Selection::Instances(vec![InstanceId::new("c")]);
    assert_eq!(
        Selection::Nothing.clicked(&instance("c"), ClickMode::Replace),
        one
    );
    assert_eq!(
        one.clicked(&instance("a"), ClickMode::Replace),
        Selection::Instances(vec![InstanceId::new("a")])
    );
    let two = one.clicked(&instance("a"), ClickMode::Add);
    assert_eq!(
        two,
        Selection::Instances(vec![InstanceId::new("c"), InstanceId::new("a")])
    );
    assert_eq!(
        two.clicked(&instance("a"), ClickMode::Add),
        two,
        "adding twice"
    );
    assert_eq!(
        two.clicked(&instance("c"), ClickMode::Toggle),
        Selection::Instances(vec![InstanceId::new("a")])
    );
    assert_eq!(
        Selection::Instances(vec![InstanceId::new("a")]).clicked(&instance("a"), ClickMode::Toggle),
        Selection::Nothing
    );
    assert_eq!(
        two.clicked(&Hit::Nothing, ClickMode::Replace),
        Selection::Nothing
    );
    assert_eq!(two.clicked(&Hit::Nothing, ClickMode::Add), two);
    assert_eq!(two.clicked(&Hit::Nothing, ClickMode::Toggle), two);
}

/// Functional capability 75 ("Hidden instances cannot be selected; locked
/// ones can be selected but not transformed"), on the plaque through the
/// app: hiding a shape takes it out of the Scene, so a click on its
/// outline finds nothing there (or what is beneath); locking one leaves
/// it pickable, and the transform it would take is refused.
#[test]
fn hidden_is_not_picked_locked_is() {
    let mut harness = Harness::from_project(plaque());
    let camera = {
        let mut camera = Camera {
            viewport_px: Vec2::new(1200.0, 900.0),
            ..Camera::default()
        };
        camera.fit(dc_app::viewport::camera::fit_bounds(
            harness.app.project(),
            harness.app.scene(),
        ));
        camera
    };
    let first = harness.app.project().instances[0].id.clone();
    let second = harness.app.project().instances[1].id.clone();
    // A point on each shape's outline, from the Scene.
    let on_outline = |scene: &Scene, id: &InstanceId| -> Pos {
        let shape = scene.shape(id).expect("in the scene");
        let outline = shape.outlines.first().expect("an outline");
        let v = outline.vertices()[0];
        camera.to_screen(Point::new(v.x, v.y))
    };
    let at_first = on_outline(harness.app.scene(), &first);
    let at_second = on_outline(harness.app.scene(), &second);
    let candidates = |harness: &Harness, at: Pos| {
        pick::candidates(harness.app.scene(), &camera, at, PICK_TOLERANCE_PX)
    };
    assert!(candidates(&harness, at_first).contains(&first));

    harness
        .run(Command::SetInstanceFlags {
            ids: vec![first.clone()],
            visible: Some(false),
            locked: None,
        })
        .unwrap();
    assert!(
        harness.app.scene().shape(&first).is_none(),
        "hidden is not in the Scene"
    );
    assert!(
        !candidates(&harness, at_first).contains(&first),
        "a hidden shape is not picked"
    );
    assert!(
        pick::touching(
            harness.app.scene(),
            Rect {
                min: Point::new(-1e4, -1e4),
                max: Point::new(1e4, 1e4)
            }
        )
        .iter()
        .all(|id| *id != first),
        "nor taken by a marquee"
    );

    harness
        .run(Command::SetInstanceFlags {
            ids: vec![second.clone()],
            visible: None,
            locked: Some(true),
        })
        .unwrap();
    let shape = harness.app.scene().shape(&second).expect("locked is drawn");
    assert!(shape.locked, "the Scene marks it locked, for the padlock");
    let hit = pick::pick(
        harness.app.scene(),
        &camera,
        at_second,
        PICK_TOLERANCE_PX,
        0,
    );
    assert_eq!(
        hit,
        Hit::Instance(second.clone()),
        "a locked shape is picked"
    );
    harness
        .app
        .select(Selection::Nothing.clicked(&hit, ClickMode::Replace));
    assert_eq!(
        harness.app.selection().instances(),
        std::slice::from_ref(&second)
    );
    let refused = harness.run(Command::TransformInstances {
        ids: vec![second.clone()],
        delta: Transform::translate(5.0, 0.0),
    });
    assert!(
        matches!(refused, Err(dc_app::CommandError::Locked { .. })),
        "{refused:?}"
    );
}

/// Acceptance 75 with an Inlay (capability 72; slice 14, #543): the male
/// board's shapes are the female's instances seen from the other board,
/// so they are not instances to select. A click on one, on its outline or
/// inside it, is `Hit::MaleBoardShape` of its Inlay, which selects the
/// Inlay (and so opens it in the editor); no instance is a candidate
/// there, and a marquee over the male board takes no instance. The same
/// letter on the stock is still its instance. With the Inlay disabled
/// there is no male board and nothing to hit.
#[test]
fn male_board_shape_selects_the_inlay() {
    let project = common::load_dcv(&common::fixture("dc.dcv"));
    let mut harness = Harness::from_project(project.clone());
    let scene = harness.app.scene().clone();
    let mut camera = view(SCALE_1);
    camera.fit(dc_app::viewport::camera::fit_bounds(&project, &scene));
    let board = scene.male_board().expect("the male board");
    let op = dc_project::OperationId::new("op1");
    let outline = &board.shapes[0].outlines[0];
    let on = outline.vertices()[0].point();
    let at = camera.to_screen(on);
    assert!(
        pick::candidates(&scene, &camera, at, PICK_TOLERANCE_PX).is_empty(),
        "no instance under a male shape"
    );
    let hit = pick::pick(&scene, &camera, at, PICK_TOLERANCE_PX, 0);
    assert_eq!(hit, Hit::MaleBoardShape(op.clone()));
    // Inside a mirrored letter, away from its outline.
    let bounds = board.shapes[0].bounds.unwrap();
    let mut inside = None;
    'search: for i in 1..40 {
        for j in 1..40 {
            let p = Point::new(
                bounds.min.x + (bounds.max.x - bounds.min.x) * f64::from(i) / 40.0,
                bounds.min.y + (bounds.max.y - bounds.min.y) * f64::from(j) / 40.0,
            );
            let s = camera.to_screen(p);
            if pick::pick(&scene, &camera, s, 0.0, 0) == Hit::MaleBoardShape(op.clone()) {
                inside = Some(s);
                break 'search;
            }
        }
    }
    assert!(inside.is_some(), "a point inside the mirrored letter hits");

    // A click selects the Inlay, whatever the modifier, and opens it.
    let mut clicks = ClickCycle::default();
    let clicked = clicks.click(&scene, &camera, at, PICK_TOLERANCE_PX);
    assert_eq!(clicked, Hit::MaleBoardShape(op.clone()));
    for mode in [ClickMode::Replace, ClickMode::Add, ClickMode::Toggle] {
        assert_eq!(
            Selection::Nothing.clicked(&clicked, mode),
            Selection::Operation(op.clone()),
            "{mode:?}"
        );
    }
    harness
        .app
        .select(Selection::Nothing.clicked(&clicked, ClickMode::Replace));
    assert_eq!(harness.app.selection(), &Selection::Operation(op.clone()));
    assert_eq!(harness.app.editing(), Some(&op));
    assert!(harness.app.selection().instances().is_empty());

    // A marquee over the whole male board takes no instance.
    assert!(pick::touching(&scene, board.frame).is_empty());

    // The letter on the stock is its instance.
    let d = scene
        .shape(&InstanceId::new("i1"))
        .expect("letter D on the stock");
    let female = camera.to_screen(d.outlines[0].vertices()[0].point());
    assert_eq!(
        pick::pick(&scene, &camera, female, PICK_TOLERANCE_PX, 0),
        Hit::Instance(InstanceId::new("i1"))
    );

    // Disabled: no male board, nothing there.
    let mut disabled = project;
    if let dc_project::Operation::Inlay(inlay) = &mut disabled.operations[0] {
        inlay.enabled = false;
    }
    let scene = scene_of(disabled);
    assert!(scene.male_board().is_none());
    assert_eq!(
        pick::pick(&scene, &camera, at, PICK_TOLERANCE_PX, 0),
        Hit::Nothing
    );
}
