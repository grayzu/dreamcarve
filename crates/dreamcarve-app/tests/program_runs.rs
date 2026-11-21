// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Review pictures of one program for all tools (functional spec,
//! capabilities 77 and 77a; the maintainer's follow-up to the smoke test,
//! 2026-11-20: the plaque's one file was listed with its first tool
//! alone). The Export step with *Program Details* open and the export's
//! result sheet, each listing both tool runs under the one file, rendered
//! through wgpu in the dark and the light appearance at 1×, and committed
//! in `docs/specs/phase-4/review/field-problems/`. Ignored, since it
//! writes files and needs an adapter; run it by hand:
//!
//! ```sh
//! cargo test -p dreamcarve-app --test program_runs -- --ignored
//! ```
//!
//! The behaviour is held by `smoke_fixes::one_program_lists_every_tool_under_its_file`;
//! these are pictures for a person. The helpers are `review.rs`'s.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::{Path, PathBuf};

use common::{QueuedDialogs, copy_fixture, go_to, options};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use egui_kittest::{Harness, SnapshotOptions};

type H = Harness<'static, dreamcarve_app::Shell>;

/// Where the pictures are committed.
fn out_dir() -> PathBuf {
    std::env::var_os("DC_REVIEW_DIR").map_or_else(
        || common::repo_root().join("docs/specs/phase-4/review/field-problems"),
        PathBuf::from,
    )
}

/// Writes `scene` into `dir` in the dark and the light appearance at 1×,
/// as `<scene>-<dark|light>-1x.png`, and leaves the window in the
/// appearance it had. The appearance is set as the preference, which wins
/// over the OS's.
fn shoot(h: &mut H, dir: &Path, scene: &str) {
    let before = h.state().app().prefs().theme;
    for (theme, look) in [
        (dc_app::Theme::Dark, "dark"),
        (dc_app::Theme::Light, "light"),
    ] {
        h.state_mut().app_mut().prefs_mut().theme = theme;
        h.run();
        let name = format!("{scene}-{look}-1x");
        let options = SnapshotOptions::new().output_path(dir);
        let _ = std::fs::remove_file(dir.join(format!("{name}.png")));
        // No reference exists, so this fails having written `.new.png`.
        let _ = h.try_snapshot_options(&name, &options);
        let new = dir.join(format!("{name}.new.png"));
        std::fs::rename(&new, dir.join(format!("{name}.png"))).expect("the picture was written");
    }
    h.state_mut().app_mut().prefs_mut().theme = before;
    h.run();
}

/// Takes the pointer off the window, so no control shows its hover.
fn park_pointer(h: &mut H) {
    h.event(egui::Event::PointerGone);
    h.run();
}

/// The plaque written as one program for all tools: the Export step with
/// *Program Details* open (`export-single-program-…`), then the result
/// sheet of its export (`export-result-single-program-…`).
#[test]
#[ignore = "writes the committed review pictures; run by hand"]
fn single_program_review_pictures() {
    if !common::wgpu_adapter_available() {
        println!("program_runs: no wgpu adapter on this machine, so nothing is rendered");
        return;
    }
    let out = out_dir();
    std::fs::create_dir_all(&out).unwrap();
    // A folder with the same name every run, since the pictures show it.
    let dir = std::env::temp_dir()
        .join("dreamcarve-review")
        .join("plaque");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let plaque = copy_fixture("plaque.dcv", &dir);
    let options = options(&dir.join("config"), Some(plaque), &QueuedDialogs::default());
    let mut h: H = Harness::builder()
        .with_size(egui::vec2(1280.0, 860.0))
        .with_pixels_per_point(1.0)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::new())
        .build_eframe(move |cc| dreamcarve_app::Shell::new(cc, options).expect("the shell starts"));
    common::settle(&mut h);
    go_to(&mut h, "Export");
    common::settle(&mut h);

    // Programs: one program for all tools, the project's own setting.
    h.get_by_role_and_label(Role::ComboBox, "Programs").click();
    h.run();
    h.query_all_by_label("One program for all tools")
        .last()
        .expect("the choice")
        .click();
    h.run();
    common::settle(&mut h);
    h.get_by_label("Program Details").click();
    h.run();
    assert!(
        h.query_by_label("2 tools, 1 tool change").is_some(),
        "Program Details lists the one program's tools"
    );
    park_pointer(&mut h);
    shoot(&mut h, &out, "export-single-program");

    h.get_by_role_and_label(Role::Button, "Export G-code")
        .click();
    h.run();
    assert!(
        h.get_by_role(Role::Dialog)
            .query_by_label("2 tools, 1 tool change")
            .is_some(),
        "the result's table lists them too"
    );
    park_pointer(&mut h);
    shoot(&mut h, &out, "export-result-single-program");
    drop(h);
    std::fs::remove_dir_all(&dir).unwrap();
}
