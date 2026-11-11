// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 24 of phase 4 (`docs/specs/phase-4/design.md`, "Review redesign
//! slices"; #616): the status bar's cells as data (functional spec,
//! capabilities 50b and 71; acceptance 94). Each cell is shown only when
//! it has something to say; the generation cell is empty when idle and
//! offers *Regenerate all* only after Stop; the stock size is never a
//! cell.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::status::{self, GenerationAction, Inputs};
use dc_app::test_support::Harness;
use dc_geometry::Point;
use dc_job::EstimateConfig;
use dc_project::Units;

use common::{checked_projects, load_dcv, plaque, plaque_bytes, quick_start_projects};

/// The status bar of `harness`'s app off the Viewport, with nothing said.
fn idle_inputs(estimate: &EstimateConfig) -> Inputs<'_> {
    Inputs {
        cursor_mm: None,
        drag: None,
        hover: None,
        message: None,
        estimate,
    }
}

/// Capability 50b, table-driven over the states the generation cell has:
/// a project with shapes and no operation has no Toolpaths cell and no
/// generation text; the plaque mid-generation says `Generating 1 of 5…`
/// with Stop; after Stop, `Stopped` with Regenerate all; idle, nothing.
/// With no cursor position the cursor cell is empty, never `X — Y —`; over
/// the Viewport it reads the position from the origin.
#[test]
fn cells_follow_the_state() {
    let estimate = EstimateConfig::default();
    let inputs = idle_inputs(&estimate);

    // Shapes and no operation: no toolpath, nothing generating.
    let mut shapes_only = plaque();
    shapes_only.operations.clear();
    let mut harness = Harness::from_project(shapes_only);
    harness.generate();
    let bar = status::cells(&harness.app, &inputs);
    assert!(!bar.toolpaths, "no Toolpaths button without a toolpath");
    assert_eq!(bar.generation, None, "no generation text");

    // The plaque, opened and not yet generated: one of five.
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let bar = status::cells(&harness.app, &inputs);
    let generating = bar.generation.as_ref().expect("generating");
    assert_eq!(generating.text, "Generating 1 of 5…");
    assert!(generating.spinner);
    assert_eq!(
        generating
            .action
            .as_ref()
            .map(|a| (a.action, a.label.as_str())),
        Some((GenerationAction::Stop, "Stop"))
    );
    assert!(!bar.toolpaths, "nothing generated yet");

    // Stop: Stopped, and Regenerate all is offered.
    harness.app.stop();
    let bar = status::cells(&harness.app, &inputs);
    let stopped = bar.generation.as_ref().expect("stopped");
    assert_eq!(stopped.text, "Stopped");
    assert!(!stopped.spinner);
    assert_eq!(
        stopped
            .action
            .as_ref()
            .map(|a| (a.action, a.label.as_str())),
        Some((GenerationAction::RegenerateAll, "Regenerate all"))
    );

    // Regenerate all, then idle: the cell is empty and the toolpaths are
    // there.
    harness.app.regenerate_all();
    harness.generate();
    let bar = status::cells(&harness.app, &inputs);
    assert_eq!(bar.generation, None, "idle says nothing");
    assert!(bar.toolpaths, "the plaque has toolpaths");

    // The cursor: empty off the Viewport, never dashes.
    assert_eq!(bar.cursor, "");
    assert!(bar.texts().iter().all(|t| !t.contains('—')), "no dashes");
    let over = Inputs {
        cursor_mm: Some(Point::new(64.2, 51.8)),
        ..idle_inputs(&estimate)
    };
    assert_eq!(status::cells(&harness.app, &over).cursor, "X 64.2  Y 51.8");
    let origin = Inputs {
        cursor_mm: Some(Point::new(-0.01, 0.0)),
        ..idle_inputs(&estimate)
    };
    assert_eq!(
        status::cells(&harness.app, &origin).cursor,
        "X 0.0  Y 0.0",
        "no −0.0 at the origin"
    );

    // The units button names the display units.
    assert_eq!(bar.units, Units::Mm);
    assert_eq!(bar.units_label, "mm");
}

/// `n` as the stock size would print it: at most one decimal in mm and
/// three in inches, trailing zeros dropped.
fn dimension(mm: f64, units: Units) -> String {
    let (value, decimals) = match units {
        Units::Mm => (mm, 1),
        Units::Inch => (mm / 25.4, 3),
    };
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// Capability 50b, acceptance 94: for every fixture, generated or not, no
/// cell's text contains the stock's dimensions; Project Settings shows
/// them.
#[test]
fn stock_size_is_never_a_cell() {
    let estimate = EstimateConfig::default();
    let generated: Vec<String> = quick_start_projects().into_iter().map(|f| f.0).collect();
    for (name, path) in checked_projects() {
        let project = load_dcv(&path);
        let units = project.display_units;
        let stock = project.stock.clone();
        let mut harness = Harness::from_project(project);
        if generated.contains(&name) {
            harness.generate();
            let mut visibility = harness.app.view().visibility;
            visibility.extents = true;
            harness.app.set_visibility(visibility);
        }
        let bar = status::cells(&harness.app, &idle_inputs(&estimate));
        let mut sizes = vec![
            dimension(stock.width_mm, units),
            dimension(stock.height_mm, units),
        ];
        sizes.extend(stock.thickness_mm.map(|t| dimension(t, units)));
        for text in bar.texts() {
            assert!(!text.contains('×'), "{name}: {text:?} is a size");
            assert!(!text.contains("Stock"), "{name}: {text:?} names the stock");
            let tokens: Vec<&str> = text
                .split(|c: char| c.is_whitespace() || c == '×')
                .collect();
            for size in &sizes {
                assert!(
                    !tokens.contains(&size.as_str()),
                    "{name}: {text:?} holds the stock's {size}"
                );
            }
        }
    }
}
