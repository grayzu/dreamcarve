// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 4 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan → Tool
//! model and snapping"): the Viewport's tool in use (functional
//! capability 81, acceptance 129), with no window.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::test_support::Harness;
use dc_app::tool::{Event, Key};
use dc_app::{Drawing, Pointer, SnapKind, Tool};
use dc_geometry::Point;

use common::plaque_bytes;

fn plaque() -> Harness {
    Harness::open(&plaque_bytes()).unwrap()
}

/// The app starts on Select; arming a tool makes it the tool in use, and
/// a drawing tool becomes the one the Draw button holds, kept after
/// another tool is armed (capability 81).
#[test]
fn arming_a_tool_makes_it_the_tool_in_use() {
    let mut harness = plaque();
    let app = &mut harness.app;
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(
        app.prefs().drawing_tool,
        Drawing::Rectangle,
        "the first offered"
    );
    for drawing in Drawing::ALL {
        app.arm(Tool::Draw(drawing));
        assert_eq!(app.tool(), Tool::Draw(drawing));
        assert_eq!(app.prefs().drawing_tool, drawing);
        assert_eq!(Drawing::from_key(drawing.key()), Some(drawing));
    }
    for tool in [
        Tool::Nodes,
        Tool::Tab,
        Tool::Trim,
        Tool::Measure,
        Tool::Select,
    ] {
        app.arm(tool);
        assert_eq!(app.tool(), tool);
        assert_eq!(
            app.prefs().drawing_tool,
            Drawing::Text,
            "the last drawing tool"
        );
    }
    assert_eq!(Drawing::from_key("pen"), None);
}

/// `Escape` with nothing begun returns to Select; arming a tool, or
/// leaving the Viewport, drops what the tool before was showing.
#[test]
fn escape_returns_to_select_and_arming_drops_the_draft() {
    let mut harness = plaque();
    let app = &mut harness.app;
    app.arm(Tool::Draw(Drawing::Rectangle));
    let reply = app.tool_event(Event::Move(Pointer::at(Point::new(31.0, 32.0))));
    assert_eq!(reply.ran, None);
    assert!(
        app.draft().snap.is_some(),
        "where the first corner would go"
    );
    app.arm(Tool::Draw(Drawing::Circle));
    assert_eq!(app.draft().snap, None, "a new tool starts with nothing");
    app.tool_event(Event::Move(Pointer::at(Point::new(31.0, 32.0))));
    app.tool_event(Event::Leave);
    assert_eq!(app.draft().snap, None, "the pointer is off the Viewport");
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
    // Select does not read the tool's events: phase 4's Viewport has them.
    app.tool_event(Event::Move(Pointer::at(Point::new(31.0, 32.0))));
    assert_eq!(app.draft().snap, None);
    app.tool_event(Event::Key(Key::Escape));
    assert_eq!(app.tool(), Tool::Select);
}

/// While an operation's targets are being edited the drawing is theirs:
/// no tool but Select can be armed (phase 4, capability 67).
#[test]
fn target_editing_keeps_the_drawing_to_itself() {
    let mut harness = plaque();
    let app = &mut harness.app;
    let op = app.project().operations[0].id().clone();
    assert!(app.edit_targets(&op));
    app.arm(Tool::Draw(Drawing::Line));
    assert_eq!(app.tool(), Tool::Select);
    assert_eq!(
        app.prefs().drawing_tool,
        Drawing::Rectangle,
        "nothing was armed"
    );
    app.cancel_targets();
    app.arm(Tool::Draw(Drawing::Line));
    assert_eq!(app.tool(), Tool::Draw(Drawing::Line));
}

/// The app snaps a tool's point to its own project: the plaque's shapes,
/// its stock and its origin, at the Viewport's zoom, with the grid the
/// View shows (capabilities 82 and 82a).
#[test]
fn the_app_snaps_to_its_project_at_its_zoom() {
    let mut harness = plaque();
    let app = &mut harness.app;
    // The stock's corner at the origin: an endpoint.
    let corner = app.snap_at(&Pointer::at(Point::new(0.4, -0.3)), None, &[]);
    assert_eq!(corner.kind, Some(SnapKind::Endpoint));
    assert_eq!(corner.at_mm, Point::new(0.0, 0.0));
    // The default camera is 5 px to the millimetre, so the grid steps by
    // 2 mm (10 px), and a point clear of everything lands on it.
    assert_eq!(app.view().camera.px_per_mm.to_bits(), 5.0f64.to_bits());
    assert_eq!(app.grid_step_mm().to_bits(), 2.0f64.to_bits());
    let clear = Point::new(-23.3, -40.9);
    let on_grid = app.snap_at(&Pointer::at(clear), None, &[]);
    assert_eq!(on_grid.kind, Some(SnapKind::Grid));
    assert_eq!(on_grid.at_mm, Point::new(-24.0, -40.0));
    // A pinned spacing is the spacing.
    app.prefs_mut().grid_spacing_mm = Some(5.0);
    assert_eq!(app.grid_step_mm().to_bits(), 5.0f64.to_bits());
    assert_eq!(
        app.snap_at(&Pointer::at(clear), None, &[]).at_mm,
        Point::new(-25.0, -40.0)
    );
    // A hidden grid does not snap.
    let mut visibility = app.view().visibility;
    visibility.grid = false;
    app.set_visibility(visibility);
    let free = app.snap_at(&Pointer::at(clear), None, &[]);
    assert_eq!(free.kind, None);
    assert_eq!(free.at_mm, clear);
    // Snapping off: the pointer, everywhere.
    app.prefs_mut().snap.enabled = false;
    let off = app.snap_at(&Pointer::at(Point::new(0.4, -0.3)), None, &[]);
    assert_eq!(off.kind, None);
}

/// Review feedback, 2026-11-23: the drawing tools are in three groups,
/// each a button of the Viewport's tool strip that holds the tool of the
/// group last used: the lines (Line, Arc, Spline), the closed shapes
/// (Rectangle, Circle, Ellipse, Polygon) and the special ones (Slot,
/// Gear). Text stands alone.
#[test]
fn the_drawing_tools_are_in_three_groups() {
    use dc_app::tool::ToolGroup;
    let grouped: Vec<Drawing> = ToolGroup::ALL
        .into_iter()
        .flat_map(|group| group.tools().iter().copied())
        .collect();
    let mut expected: Vec<Drawing> = Drawing::ALL
        .into_iter()
        .filter(|drawing| *drawing != Drawing::Text)
        .collect();
    assert_eq!(grouped, expected, "every tool but Text, once, in order");
    expected.clear();
    assert_eq!(
        ToolGroup::Lines.tools(),
        [Drawing::Line, Drawing::Arc, Drawing::Spline]
    );
    assert_eq!(
        ToolGroup::Shapes.tools(),
        [
            Drawing::Rectangle,
            Drawing::Circle,
            Drawing::Ellipse,
            Drawing::Polygon
        ]
    );
    assert_eq!(ToolGroup::Special.tools(), [Drawing::Slot, Drawing::Gear]);
    assert_eq!(Drawing::Text.group(), None);
    assert_eq!(Drawing::Ellipse.group(), Some(ToolGroup::Shapes));
    let strings = dc_app::Strings::english();
    assert_eq!(strings.get(ToolGroup::Lines.name()), "Line tools");
    assert_eq!(strings.get(ToolGroup::Shapes.name()), "Shape tools");
    assert_eq!(
        strings.get(ToolGroup::Special.name()),
        "Slot and gear tools"
    );

    // Each group holds its first tool until another of it is used.
    let mut app = plaque().app;
    assert_eq!(app.held(ToolGroup::Lines), Drawing::Line);
    assert_eq!(app.held(ToolGroup::Shapes), Drawing::Rectangle);
    assert_eq!(app.held(ToolGroup::Special), Drawing::Slot);
    app.arm(Tool::Draw(Drawing::Arc));
    app.arm(Tool::Draw(Drawing::Ellipse));
    app.arm(Tool::Draw(Drawing::Text));
    app.arm(Tool::Select);
    assert_eq!(app.held(ToolGroup::Lines), Drawing::Arc);
    assert_eq!(app.held(ToolGroup::Shapes), Drawing::Ellipse);
    assert_eq!(app.held(ToolGroup::Special), Drawing::Slot, "untouched");
}
