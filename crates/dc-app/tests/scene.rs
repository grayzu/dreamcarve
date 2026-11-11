// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 4 of phase 4 (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app", `Scene`; #533): the read model the Viewport draws and `pick`
//! reads. It holds every visible instance's outlines in project space,
//! placed by the instance's transform as the generators place them, and
//! is rebuilt when the snapshot or a generation result changes, never
//! for the selection, the camera or any other view state.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::test_support::Harness;
use dc_app::viewport::camera::Pos;
use dc_app::{Command, Selection};
use dc_geometry::{EPS_MM, Point, Transform, approx_eq};

use common::plaque;

/// The Scene follows the snapshot: built once with the app, rebuilt by
/// each command, undo and redo, and by each generation result the app
/// keeps; untouched by selecting, by moving the camera and by the other
/// view settings, which change every frame.
#[test]
fn rebuilds_only_when_the_snapshot_changes() {
    let mut harness = Harness::from_project(plaque());
    let project = harness.app.project().clone();
    let scene = harness.app.scene();
    let built = scene.revision();

    // Every visible instance, in file order (the last drawn on top), its
    // outlines the definition's profiles placed by its transform.
    let ids: Vec<_> = scene.shapes().iter().map(|s| s.instance.clone()).collect();
    let file_order: Vec<_> = project.instances.iter().map(|i| i.id.clone()).collect();
    assert_eq!(ids, file_order);
    for (shape, instance) in scene.shapes().iter().zip(&project.instances) {
        let definition = &project.definitions[&instance.definition];
        assert_eq!(shape.outlines.len(), definition.profiles.len());
        assert_eq!(shape.name, instance.name);
        for (outline, profile) in shape.outlines.iter().zip(&definition.profiles) {
            let placed = instance.transform.apply_polyline(profile).unwrap();
            assert_eq!(
                outline, &placed,
                "{} placed as the generators place it",
                shape.instance
            );
        }
    }
    // The placed tabs, at their project-space points.
    assert_eq!(scene.tabs().len(), project.tabs.len());
    for (mark, tab) in scene.tabs().iter().zip(&project.tabs) {
        let instance = project
            .instances
            .iter()
            .find(|i| i.id == tab.instance)
            .unwrap();
        let want = instance.transform.apply(Point::new(tab.x_mm, tab.y_mm));
        assert_eq!(mark.id, tab.id);
        assert!(approx_eq(mark.at_mm.x, want.x, EPS_MM) && approx_eq(mark.at_mm.y, want.y, EPS_MM));
    }

    // View state does not rebuild it.
    let first = project.instances[0].id.clone();
    harness
        .app
        .select(Selection::Instances(vec![first.clone()]));
    harness.app.select(Selection::Nothing);
    let view = harness.app.view_mut();
    view.camera.zoom_about(Pos::new(10.0, 10.0), 2.0);
    view.camera.pan(40.0, -3.0);
    view.visibility.only_selected = true;
    view.visibility.rapids = false;
    assert_eq!(
        harness.app.scene().revision(),
        built,
        "selection and view keep the Scene"
    );

    // A command does, and the Scene reads the new snapshot.
    harness
        .run(Command::TransformInstances {
            ids: vec![first.clone()],
            delta: Transform::translate(5.0, 0.0),
        })
        .unwrap();
    let moved = harness.app.scene().revision();
    assert_ne!(moved, built, "a command rebuilds the Scene");
    let now = harness.app.project().instances[0].transform;
    assert_eq!(
        harness.app.scene().shapes()[0].outlines[0],
        now.apply_polyline(&project.definitions[&project.instances[0].definition].profiles[0])
            .unwrap()
    );
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![first.clone()],
            visible: Some(false),
            locked: None,
        })
        .unwrap();
    assert!(
        harness.app.scene().shape(&first).is_none(),
        "hidden is absent"
    );
    let hidden = harness.app.scene().revision();
    assert_ne!(hidden, moved);

    // Undo and redo are snapshot changes too.
    harness.app.undo();
    assert!(harness.app.scene().shape(&first).is_some());
    let undone = harness.app.scene().revision();
    assert_ne!(undone, hidden);
    harness.app.redo();
    assert_ne!(harness.app.scene().revision(), undone);
    harness.app.undo();

    // A generation result the app keeps rebuilds it (its operations'
    // stale flags change); a frame with nothing new does not.
    let before_results = harness.app.scene().revision();
    assert!(
        harness.app.scene().operations().iter().all(|o| o.stale),
        "nothing is generated yet"
    );
    harness.generate();
    let generated = harness.app.scene().revision();
    assert_ne!(generated, before_results, "results rebuild the Scene");
    assert!(
        harness.app.scene().operations().iter().any(|o| !o.stale),
        "the results are in"
    );
    harness.generate();
    assert_eq!(
        harness.app.scene().revision(),
        generated,
        "nothing new, no rebuild"
    );
}

/// Acceptance 79 (capability 72): for each golden case and the quick
/// start's plaque, what the Viewport draws of an operation is the
/// generator's toolpath for the saved file, and its marks are the
/// preview's. The Scene's runs, in list order, are the runs `dc_job::build`
/// generates for the file (what `export` and `check` emit and preview),
/// generated again here independently of the app's worker; each run's
/// strokes (`scene::strokes`, what the renderer reads) are its moves, one
/// for one but the first (which only places the tool) and the dwells
/// (which do not move it), same count, same end points to 1 µm; its marks
/// are `dc_job::preview::marks` of that run, the tab marks, start points
/// and holes `--preview` draws. The Viewport's SVG dump equals the file
/// `--preview` writes for the project: `render_generated` of what
/// `dc_job::build` generated, which is what the CLI writes.
#[test]
fn marks_equal_the_svg_marks() {
    let mut cases = common::golden_projects();
    // The quick start's plaque: a Profile with placed tabs, so tab marks.
    cases.push(("plaque".to_owned(), common::fixture("plaque.dcv")));
    // One thread a case: the V-Carve cases generate for seconds each.
    let marks: usize = std::thread::scope(|scope| {
        let checks: Vec<_> = cases
            .iter()
            .map(|(name, path)| scope.spawn(move || marks_of_one_case(name, path)))
            .collect();
        checks.into_iter().map(|c| c.join().unwrap()).sum()
    });
    assert!(marks > 100, "{marks} marks compared");
}

/// [`marks_equal_the_svg_marks`] on the project at `path`; the number of
/// marks compared.
fn marks_of_one_case(name: &str, path: &std::path::Path) -> usize {
    use dc_app::scene::{StrokeKind, strokes};
    use dc_job::preview;

    let bytes = common::read(path);
    let mut harness = Harness::open(&bytes).expect("the project opens");
    harness.generate();
    let project = harness.app.project().clone();
    let scene = harness.app.scene();
    assert!(
        scene.operations().iter().all(|o| !o.stale),
        "{name}: every operation is current"
    );
    let built = dc_job::build(&project, dc_job::ProgramSplit::ProjectDefault, "2026-10-15");
    let generated = built.generated.expect("every operation generates");
    let drawn: Vec<_> = scene
        .operations()
        .iter()
        .flat_map(|o| o.runs.iter().map(move |r| (&o.id, r)))
        .collect();
    assert_eq!(
        drawn.len(),
        generated.len(),
        "{name}: one drawn run per run"
    );
    let mut checked = 0;
    for (k, ((op, drawn), run)) in drawn.iter().zip(&generated).enumerate() {
        assert_eq!(*op, run.op.id(), "{name} run {k}: in list order");
        let moves: Vec<_> = run
            .toolpath
            .moves
            .iter()
            .filter(|m| !m.is_dwell())
            .skip(1)
            .collect();
        let strokes = strokes(&drawn.run.toolpath);
        assert_eq!(
            strokes.len(),
            moves.len(),
            "{name} {op} run {k}: one stroke per move"
        );
        for (i, (stroke, m)) in strokes.iter().zip(&moves).enumerate() {
            let d = (stroke.to.x - m.to.x)
                .abs()
                .max((stroke.to.y - m.to.y).abs())
                .max((stroke.to.z - m.to.z).abs());
            assert!(
                d <= 1e-3,
                "{name} {op} run {k} move {i}: {stroke:?} vs {m:?}"
            );
            assert_eq!(
                stroke.kind == StrokeKind::Rapid,
                m.is_rapid(),
                "{name} {op} run {k} move {i}: a rapid is drawn as one"
            );
        }
        let want = preview::marks(&preview::Run::from(run));
        assert_eq!(drawn.marks, want, "{name} {op} run {k}: marks");
        checked += want.tab_marks.len() + want.start_points.len() + want.holes.len();
    }
    assert_eq!(
        scene.preview_svg(&project),
        preview::render_generated(&project, &generated),
        "{name}: the Viewport's SVG dump is --preview's"
    );
    checked
}

/// Acceptance 79 and 83 (capability 77): the export sheet no longer
/// writes the toolpath preview; the Viewport is the preview. Its SVG dump
/// for the coaster, the plaque, the sign and the inlay fixture, once every
/// operation is generated, is the file `export --preview` writes for the
/// project: `render_generated` of what `dc_job::build` generated. The
/// byte comparison with the `dreamcarve` binary is
/// `gui_equality::app_export_equals_cli_export_on_six_posts`.
#[test]
fn svg_dump_equals_preview() {
    use dc_job::preview;

    // The coaster as `init` writes it, then made ready to cut as
    // `gui_equality`'s `ready_coaster` makes it.
    let mut coaster = common::coaster();
    for tool in &mut coaster.tools {
        tool.feed_mm_min = Some(1500.0);
        tool.plunge_mm_min = Some(500.0);
        tool.rpm = Some(18000);
        tool.step_down_mm = Some(2.0);
    }
    coaster.stock.thickness_mm = Some(11.8);
    for op in &mut coaster.operations {
        if let dc_project::Operation::Profile(p) = op {
            p.depth_mm = Some(12.0);
            p.through_cut_allowance_mm = 0.2;
        }
    }
    let mut cases: Vec<(String, dc_project::Project)> = vec![("coaster".to_owned(), coaster)];
    for name in ["plaque", "sign", "dc"] {
        cases.push((
            name.to_owned(),
            common::load_dcv(&common::fixture(&format!("{name}.dcv"))),
        ));
    }
    for (name, project) in cases {
        let mut harness = Harness::from_project(project);
        harness.generate();
        let project = harness.app.project().clone();
        let built = dc_job::build(
            &project,
            dc_job::ProgramSplit::ProjectDefault,
            dc_app::test_support::DATE,
        );
        assert!(!built.programs.is_empty(), "{name}: the CLI exports");
        let generated = built.generated.expect("every operation generates");
        let dump = harness.app.scene().preview_svg(&project);
        assert!(dump.contains("<svg"), "{name}: an SVG");
        assert!(
            dump == preview::render_generated(&project, &generated),
            "{name}: the Viewport's SVG dump is --preview's"
        );
    }
}

/// Acceptance 87 (capability 71; design, "Digest and staleness"): an
/// edit that changes an operation's digest leaves its last runs in the
/// Scene, flagged stale for the Viewport to draw faded, while the other
/// operations stay current and keep their runs, shared rather than
/// copied; when the new result lands it replaces them and the flag
/// clears.
#[test]
fn stale_results_are_flagged_until_replaced() {
    use std::sync::Arc;

    let mut harness = Harness::from_project(plaque());
    harness.generate();
    let before = harness.app.scene().clone();
    assert!(
        before
            .operations()
            .iter()
            .all(|o| !o.stale && !o.runs.is_empty())
    );
    let pocket = dc_project::OperationId::new("op1");

    // Deepen the Panel pocket: only it is stale.
    let mut op = harness
        .app
        .project()
        .operations
        .iter()
        .find(|o| *o.id() == pocket)
        .unwrap()
        .clone();
    let dc_project::Operation::Pocket(p) = &mut op else {
        panic!("op1 is the Panel pocket");
    };
    p.depth_mm = Some(5.0);
    harness.run(Command::UpdateOperation(op)).unwrap();
    let scene = harness.app.scene();
    let edited = scene.operation(&pocket).unwrap();
    let old = before.operation(&pocket).unwrap();
    assert!(edited.stale, "the edited operation is stale");
    assert!(
        Arc::ptr_eq(&edited.runs, &old.runs),
        "its last runs stay until the new result lands"
    );
    for (now, was) in scene.operations().iter().zip(before.operations()) {
        if now.id != pocket {
            assert!(!now.stale, "{} is untouched", now.id);
            assert!(
                Arc::ptr_eq(&now.runs, &was.runs),
                "{} keeps its runs",
                now.id
            );
        }
    }

    // The new result replaces them.
    harness.generate();
    let replaced = harness.app.scene().operation(&pocket).unwrap();
    assert!(!replaced.stale, "the new result is current");
    assert!(!Arc::ptr_eq(&replaced.runs, &old.runs));
    let deepest = replaced
        .runs
        .iter()
        .flat_map(|r| r.run.toolpath.moves.iter().map(|m| m.to.z))
        .fold(f64::INFINITY, f64::min);
    let thickness = harness.app.project().stock.thickness_mm.unwrap();
    assert!(
        approx_eq(deepest, thickness - 5.0, 1e-6),
        "the new runs cut to the new depth: {deepest}"
    );
}

/// Slice 14 (#543; design spec, "Data model → dc-app", `Scene`; functional
/// spec, capability 72, acceptance 76): with an Inlay the male board is
/// drawn to the right of the stock, offset by one stock width plus 20 mm,
/// for display only. On the lettered inlay fixture (a 110 × 80 mm stock,
/// the male board the stock's size) its frame is (130, 0)–(240, 80), its
/// origin mark is the origin preset's point on it, offset, and its shapes
/// are the Inlay's targets mirrored as the generator mirrors them
/// (`dc_job::preview::male_shapes`, the preview's), offset. Nothing of the
/// offset reaches the project or the runs: the male runs stay in their
/// board's own space, the Scene's SVG equals the CLI's preview, and the
/// file is unchanged. A male board of its own size keeps the same offset;
/// Fit takes it in; without an enabled Inlay there is none.
#[test]
fn male_board_sits_beside_the_stock_in_display_only() {
    let fixture = common::fixture("dc.dcv");
    let bytes = common::read(&fixture);
    let mut harness = Harness::open(&bytes).expect("the fixture opens");
    harness.generate();
    let project = harness.app.project().clone();
    let scene = harness.app.scene();
    let board = scene.male_board().expect("an Inlay's male board");
    assert_eq!(board.op, dc_project::OperationId::new("op1"));
    assert_eq!(board.name, "DC");
    assert!(approx_eq(board.offset.x, 110.0 + 20.0, EPS_MM));
    assert!(approx_eq(board.offset.y, 0.0, EPS_MM));
    assert!(approx_eq(board.frame.min.x, 130.0, EPS_MM));
    assert!(approx_eq(board.frame.min.y, 0.0, EPS_MM));
    assert!(approx_eq(board.frame.max.x, 240.0, EPS_MM));
    assert!(approx_eq(board.frame.max.y, 80.0, EPS_MM));
    assert!(approx_eq(board.origin_mm.x, 130.0, EPS_MM));
    assert!(approx_eq(board.origin_mm.y, 0.0, EPS_MM));

    // The design mirrored as the generator mirrors it, then offset.
    let shift = Transform::translate(130.0, 0.0);
    let want: Vec<_> = dc_job::preview::male_shapes(&project)
        .into_iter()
        .flat_map(|m| {
            assert_eq!(m.op, board.op);
            m.outlines
        })
        .map(|o| shift.apply_polyline(&o).unwrap())
        .collect();
    let got: Vec<_> = board
        .shapes
        .iter()
        .flat_map(|s| s.outlines.iter().cloned())
        .collect();
    assert!(!want.is_empty());
    assert_eq!(got, want);
    for shape in &board.shapes {
        assert_eq!(shape.op, board.op);
        assert_eq!(shape.flat.len(), shape.outlines.len());
        let bounds = shape.bounds.expect("a mirrored shape has a box");
        assert!(bounds.min.x >= 130.0 && bounds.max.x <= 240.0, "{bounds:?}");
    }

    // Display only: the male runs are in their board's space, the SVG is
    // the CLI's, and the file is the fixture's.
    let males: Vec<_> = scene.runs_on(dc_project::Board::Male).collect();
    assert!(!males.is_empty());
    for run in &males {
        for m in &run.toolpath.moves {
            assert!(
                m.to.x <= 110.0 + 10.0,
                "{:?} is on the board, not offset",
                m.to
            );
        }
    }
    let built = dc_job::build(
        &project,
        dc_job::ProgramSplit::ProjectDefault,
        dc_app::test_support::DATE,
    );
    let generated = built.generated.expect("the fixture generates");
    assert_eq!(
        scene.preview_svg(&project),
        dc_job::preview::render_generated(&project, &generated)
    );
    assert_eq!(
        harness.save().expect("it saves"),
        dc_project::save(&common::load_dcv(&fixture)).expect("the fixture saves")
    );
    assert!(!harness.app.is_dirty());

    // Fit frames both boards.
    let fit = dc_app::viewport::camera::fit_bounds(&project, scene);
    assert!(approx_eq(fit.max.x, 240.0, EPS_MM), "{fit:?}");

    // A male board of its own size: the same offset, its own frame.
    let mut sized = project.clone();
    if let dc_project::Operation::Inlay(inlay) = &mut sized.operations[0] {
        inlay.male_board.width_mm = Some(150.0);
        inlay.male_board.height_mm = Some(60.0);
    }
    let sized_scene = Harness::from_project(sized).app.scene().clone();
    let board = sized_scene.male_board().expect("still one");
    assert!(approx_eq(board.frame.min.x, 130.0, EPS_MM));
    assert!(approx_eq(board.frame.max.x, 280.0, EPS_MM));
    assert!(approx_eq(board.frame.max.y, 60.0, EPS_MM));

    // No enabled Inlay, no male board.
    let mut disabled = project.clone();
    if let dc_project::Operation::Inlay(inlay) = &mut disabled.operations[0] {
        inlay.enabled = false;
    }
    assert!(
        Harness::from_project(disabled)
            .app
            .scene()
            .male_board()
            .is_none()
    );
    assert!(
        Harness::from_project(plaque())
            .app
            .scene()
            .male_board()
            .is_none()
    );
}
