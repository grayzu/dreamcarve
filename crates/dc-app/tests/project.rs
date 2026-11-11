// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 7 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan →
//! Project panel"; #536): the Project panel's fields write the `.dcv`
//! fields `check` reads (functional spec, capability 60, acceptance 69),
//! offer the tool-change height, `M6` and canned cycles only where the
//! post-processor's definition has them, switch post-processors as
//! `export --post` does (capability 62, acceptance 70), and mark the
//! thickness and refuse export until it is set.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::project::{self, Choice, Field, Offer, OriginPreset};
use dc_app::test_support::Harness;
use dc_app::{Command, Strings};
use dc_geometry::{Point, assert_approx_eq};
use dc_job::{Drawing, Tier};
use dc_project::{
    OriginXy, OriginZ, ParkXy, PostProcessorRef, Problem, Project, ResetReason, SpindleControl,
    ToolChangeMode, Units,
};

use common::{plaque, plaque_bytes, read, repo_root};

/// Tolerance for a typed length read back, mm.
const EPS_MM: f64 = 1e-9;

/// Types `text` into `field` and leaves it, as the panel commits it: the
/// command the field builds, run on the app. Panics if the field refuses
/// the text or the text changes nothing.
fn type_into(harness: &mut Harness, field: Field, text: &str) {
    let command = field
        .set(harness.app.project(), text)
        .unwrap_or_else(|e| panic!("{field:?} refuses {text:?}: {e}"))
        .unwrap_or_else(|| panic!("{field:?} = {text:?} changes nothing"));
    harness
        .run(command)
        .unwrap_or_else(|e| panic!("{field:?} = {text:?} does not apply: {e}"));
}

/// Runs the command `make` builds from the current snapshot, which must
/// change something.
fn choose(harness: &mut Harness, make: impl FnOnce(&Project) -> Option<Command>, what: &str) {
    let command = make(harness.app.project()).unwrap_or_else(|| panic!("{what} changes nothing"));
    harness
        .run(command)
        .unwrap_or_else(|e| panic!("{what} does not apply: {e}"));
}

/// The project in the file the app saves, read back as `check` reads it.
fn saved(harness: &Harness) -> Project {
    let text = harness.save().expect("the project saves");
    dc_project::load(text.as_bytes())
        .expect("the saved file loads")
        .project
}

/// `project` on the latest version of post-processor `id`, with its
/// default settings.
fn on_post(mut project: Project, id: &str) -> Project {
    project.post_processor = PostProcessorRef::latest(id).expect("bundled post-processor");
    project
}

/// The coaster as `dreamcarve init coaster.svg` writes it.
fn coaster() -> Project {
    let bytes = read(&repo_root().join("fixtures/coaster.svg"));
    let (import, _) = dc_import::import_svg(&bytes, &dc_import::SvgImportOptions::default())
        .expect("coaster imports");
    dc_job::project_from::drawing(
        "coaster",
        Drawing::Svg(import),
        PostProcessorRef::latest("grblhal").expect("grblhal"),
        Units::Mm,
        true,
    )
    .expect("init builds the coaster")
}

/// Acceptance 69 (capability 60): typing the stock, choosing each origin
/// preset and a custom origin, Z zero, and the three heights writes the
/// fields `check` reads, one undo step each; the marks the Viewport draws
/// (the origin crosshair and the park point) follow; *Fit stock to
/// shapes* with `init`'s margin gives back `init`'s stock for the coaster.
/// The plaque is on grblHAL, which documents `G53`
/// (`definitions/grblhal`, `work_coordinates.machine_command`).
#[test]
fn stock_origin_heights_write_the_fields() {
    let mut h = Harness::open(&plaque_bytes()).expect("the plaque opens");

    // Stock: bare numbers in the project's units, unit strings as the file
    // reads them.
    type_into(&mut h, Field::StockWidth, "200");
    type_into(&mut h, Field::StockHeight, "150 mm");
    type_into(&mut h, Field::StockThickness, "1/2 in");
    let stock = &h.app.project().stock;
    assert_approx_eq!(stock.width_mm, 200.0, EPS_MM);
    assert_approx_eq!(stock.height_mm, 150.0, EPS_MM);
    assert_approx_eq!(stock.thickness_mm.expect("thickness set"), 12.7, EPS_MM);
    assert_eq!(Field::StockThickness.text(h.app.project()), "12.7");
    assert_eq!(h.app.undo_label(), Some("Set stock"));

    // Each preset moves the work zero, and the crosshair with it.
    let presets = [
        (
            OriginPreset::BottomRight,
            OriginXy::BottomRight,
            (200.0, 0.0),
        ),
        (OriginPreset::TopLeft, OriginXy::TopLeft, (0.0, 150.0)),
        (OriginPreset::TopRight, OriginXy::TopRight, (200.0, 150.0)),
        (OriginPreset::Centre, OriginXy::Centre, (100.0, 75.0)),
        (OriginPreset::BottomLeft, OriginXy::BottomLeft, (0.0, 0.0)),
    ];
    for (preset, xy, (x_mm, y_mm)) in presets {
        choose(
            &mut h,
            |p| project::set_origin_xy(p, preset),
            &format!("{preset:?}"),
        );
        assert_eq!(h.app.project().origin.xy, xy, "{preset:?}");
        assert_eq!(
            project::OriginPreset::of(&h.app.project().origin.xy),
            preset
        );
        let mark = project::marks(h.app.project()).origin_mm;
        assert_approx_eq!(mark.x, x_mm, EPS_MM);
        assert_approx_eq!(mark.y, y_mm, EPS_MM);
    }
    // Choosing the preset it already has is no undo step.
    assert_eq!(
        project::set_origin_xy(h.app.project(), OriginPreset::BottomLeft),
        None
    );

    // Custom starts where the work zero was, then takes typed X and Y.
    choose(
        &mut h,
        |p| project::set_origin_xy(p, OriginPreset::Centre),
        "centre",
    );
    choose(
        &mut h,
        |p| project::set_origin_xy(p, OriginPreset::Custom),
        "custom",
    );
    assert_eq!(
        h.app.project().origin.xy,
        OriginXy::Custom {
            x_mm: 100.0,
            y_mm: 75.0
        }
    );
    type_into(&mut h, Field::OriginX, "20");
    type_into(&mut h, Field::OriginY, "5");
    assert_eq!(
        h.app.project().origin.xy,
        OriginXy::Custom {
            x_mm: 20.0,
            y_mm: 5.0
        }
    );
    let mark = project::marks(h.app.project()).origin_mm;
    assert_approx_eq!(mark.x, 20.0, EPS_MM);
    assert_approx_eq!(mark.y, 5.0, EPS_MM);

    // Z zero.
    choose(
        &mut h,
        |p| project::set_origin_z(p, OriginZ::StockBottom),
        "stock bottom",
    );
    assert_eq!(h.app.project().origin.z, OriginZ::StockBottom);
    assert_eq!(h.app.undo_label(), Some("Set origin"));

    // The three heights; the park point is typed as X, Y.
    assert_eq!(project::marks(h.app.project()).park_mm, None);
    type_into(&mut h, Field::Clearance, "15");
    type_into(&mut h, Field::ToolChangeZ, "-5");
    type_into(&mut h, Field::Park, "150, 95");
    let heights = &h.app.project().heights;
    assert_approx_eq!(heights.clearance_mm.expect("clearance"), 15.0, EPS_MM);
    assert_approx_eq!(
        heights.tool_change_machine_z_mm.expect("machine Z"),
        -5.0,
        EPS_MM
    );
    assert_eq!(
        heights.park,
        Some(ParkXy {
            x_mm: 150.0,
            y_mm: 95.0
        })
    );
    assert_eq!(Field::Park.text(h.app.project()), "150, 95");
    assert_eq!(
        project::marks(h.app.project()).park_mm,
        Some(Point::new(150.0, 95.0))
    );
    assert_eq!(h.app.undo_label(), Some("Set heights"));
    // A park point that is not X, Y is refused before it reaches the file.
    assert!(Field::Park.set(h.app.project(), "150").is_err());

    // Every field is in the file `check` reads, and validates clean.
    let file = saved(&h);
    assert_eq!(file.stock, h.app.project().stock);
    assert_eq!(file.origin, h.app.project().origin);
    assert_eq!(file.heights, h.app.project().heights);
    let errors: Vec<String> = dc_job::validation(&file)
        .errors
        .into_iter()
        .map(|f| f.message)
        .collect();
    assert!(errors.is_empty(), "the edited plaque checks: {errors:?}");

    // Emptying an optional field writes `null`.
    type_into(&mut h, Field::Park, "");
    type_into(&mut h, Field::Clearance, "");
    assert_eq!(h.app.project().heights.park, None);
    assert_eq!(h.app.project().heights.clearance_mm, None);

    // Fit stock to shapes with init's margin is init's stock, the shapes
    // where init put them.
    let init = coaster();
    let mut h = Harness::from_project(init.clone());
    let ids: Vec<_> = init.instances.iter().map(|i| i.id.clone()).collect();
    h.run(Command::TransformInstances {
        ids,
        delta: dc_geometry::Transform::translate(37.0, -12.0),
    })
    .expect("the coaster moves");
    type_into(&mut h, Field::StockWidth, "300");
    choose(
        &mut h,
        |p| project::set_origin_xy(p, OriginPreset::Centre),
        "centre",
    );
    assert_eq!(project::fit_margin_text(Units::Mm), "10");
    let fit = project::fit_stock(h.app.project(), &project::fit_margin_text(Units::Mm))
        .expect("the default margin is a length");
    h.run(fit).expect("the stock fits");
    assert_eq!(h.app.project().stock, init.stock);
    assert_eq!(h.app.project().instances, init.instances);
    assert_eq!(h.app.project().origin.xy, OriginXy::Centre, "preset kept");
}

/// Capability 60 and acceptance 69: the tool-change machine Z is offered
/// exactly where the definition documents `G53`
/// (`work_coordinates.machine_command`, the rule `validate` applies), and
/// elsewhere is refused with the CLI's message for it. Per the bundled
/// definitions' READMEs, GRBL, grblHAL and LinuxCNC document `G53`;
/// generic RS274/NGC, Mach3 and UCCNC do not. A height set on a post that
/// cannot write it shows the file's error under the field and can be
/// cleared.
#[test]
fn tool_change_z_offered_only_with_g53() {
    let base = plaque();
    for definition in dc_project::latest_definitions().expect("bundled") {
        let project = on_post(base.clone(), definition.id);
        let documents = definition
            .post_processor()
            .work_coordinates
            .machine_command
            .is_some();
        match project::offers(&project).tool_change_z {
            Offer::Offered => assert!(documents, "{} offers it without G53", definition.id),
            Offer::Refused { reason } => {
                assert!(!documents, "{} refuses it with G53", definition.id);
                let cli = Problem::MachineZNeedsG53 {
                    post: definition.id.to_owned(),
                }
                .to_string();
                assert_eq!(reason, cli, "{}", definition.id);
            }
        }
    }
    for (id, offered) in [
        ("grbl", true),
        ("grblhal", true),
        ("linuxcnc", true),
        ("generic", false),
        ("mach3", false),
        ("uccnc", false),
    ] {
        let offer = project::offers(&on_post(base.clone(), id)).tool_change_z;
        assert_eq!(offer == Offer::Offered, offered, "{id}: {offer:?}");
    }

    // On generic, a height carried over from another post is the file's
    // error, under the field; emptying the field clears both.
    let mut generic = on_post(base, "generic");
    generic.heights.tool_change_machine_z_mm = Some(-5.0);
    let mut h = Harness::from_project(generic);
    let status = Field::ToolChangeZ
        .status(h.app.warnings())
        .expect("the height is located");
    assert_eq!(status.tier, Tier::Error);
    assert_eq!(
        status.message,
        Problem::MachineZNeedsG53 {
            post: "generic".to_owned()
        }
        .to_string()
    );
    assert!(project::export_refusal(&h.app).is_some());
    type_into(&mut h, Field::ToolChangeZ, "");
    assert_eq!(h.app.project().heights.tool_change_machine_z_mm, None);
    assert_eq!(Field::ToolChangeZ.status(h.app.warnings()), None);
}

/// Acceptance 70 (capability 62; acceptance 39's rule, through
/// `dc_project::switch_post`): the list is the six bundled definitions at
/// their latest version, the project's marked; switching the plaque from
/// grblHAL to LinuxCNC with a customised dwell and `M6` keeps the dwell,
/// resets the mode LinuxCNC cannot write (its tool change has no `M6`)
/// and says so, and takes LinuxCNC's defaults for what the user had not
/// changed. A project on an older version is told so, and picking its own
/// post-processor upgrades it.
#[test]
fn post_switch_keeps_settings() {
    let mut h = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let strings = Strings::english();

    let posts = project::posts().expect("bundled");
    let ids: Vec<&str> = posts.iter().map(|p| p.id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        ["generic", "grbl", "grblhal", "linuxcnc", "mach3", "uccnc"],
        "the six bundled definitions, once each"
    );
    assert_eq!(
        ids.first(),
        Some(&"grblhal"),
        "listed as `posts` lists them"
    );
    for post in &posts {
        let latest = dc_project::latest_definition(post.id).expect("bundled");
        assert_eq!(post.version, latest.version, "{}", post.id);
        assert_eq!(post.name, latest.post_processor().name, "{}", post.id);
    }
    assert_eq!(project::newer_version(h.app.project()), None);

    type_into(&mut h, Field::SpindleDwell, "6");
    choose(
        &mut h,
        |p| project::set_tool_change_mode(p, ToolChangeMode::M6),
        "M6",
    );
    let before = h.app.project().post_processor.clone();
    assert_approx_eq!(before.spindle_on_dwell_s, 6.0, EPS_MM);
    let (expected, expected_resets) = dc_project::switch_post(&before, "linuxcnc").expect("switch");

    choose(&mut h, |p| project::switch_post(p, "linuxcnc"), "linuxcnc");
    let post = &h.app.project().post_processor;
    assert_eq!(post, &expected);
    assert_eq!(post.id, "linuxcnc");
    assert_approx_eq!(post.spindle_on_dwell_s, 6.0, EPS_MM);
    assert_eq!(post.tool_change_mode, ToolChangeMode::SeparatePrograms);
    assert_eq!(post.file_extension, "ngc", "an unchanged default follows");
    assert_eq!(h.app.last_resets(), expected_resets.as_slice());
    assert_eq!(h.app.last_resets().len(), 1);
    assert_eq!(h.app.last_resets()[0].setting, "tool_change_mode");
    assert_eq!(h.app.last_resets()[0].reason, ResetReason::NoM6);
    let messages = project::reset_messages(&strings, h.app.project(), h.app.last_resets());
    assert_eq!(
        messages,
        [
            "Reset tool_change_mode \"m6\" → \"separate_programs\": post-processor linuxcnc has no M6 tool change."
        ]
    );
    assert_eq!(
        saved(&h).post_processor,
        expected,
        "the switch is in the file"
    );

    // The next edit clears the report; undo restores grblHAL's settings.
    type_into(&mut h, Field::DecimalsMm, "2");
    assert!(h.app.last_resets().is_empty());
    h.app.undo();
    h.app.undo();
    assert_eq!(h.app.project().post_processor, before);
    assert!(h.app.last_resets().is_empty());

    // An older version is noted; picking its own id upgrades it, keeping
    // the settings.
    let mut old = plaque();
    old.post_processor.definition_version = 1;
    old.post_processor.spindle_on_dwell_s = 6.0;
    let latest = dc_project::latest_definition("grblhal").expect("bundled");
    let mut h = Harness::from_project(old);
    assert_eq!(
        project::newer_version(h.app.project()),
        Some(latest.version)
    );
    choose(&mut h, |p| project::switch_post(p, "grblhal"), "upgrade");
    assert_eq!(
        h.app.project().post_processor.definition_version,
        latest.version
    );
    assert_approx_eq!(
        h.app.project().post_processor.spindle_on_dwell_s,
        6.0,
        EPS_MM
    );
    assert_eq!(project::newer_version(h.app.project()), None);
    assert_eq!(project::switch_post(h.app.project(), "grblhal"), None);

    // The rest of the settings write their fields.
    type_into(&mut h, Field::FileExtension, ".tap");
    type_into(&mut h, Field::DecimalsInch, "5");
    choose(
        &mut h,
        |p| project::set_spindle_control(p, SpindleControl::Manual),
        "manual",
    );
    let post = &saved(&h).post_processor;
    assert_eq!(post.file_extension, "tap");
    assert_eq!(post.decimal_places.inch, 5);
    assert_eq!(post.spindle_control, SpindleControl::Manual);
    assert!(Field::DecimalsMm.set(h.app.project(), "three").is_err());
}

/// Capability 62 and acceptance 70: one program with `M6` is offered
/// where the definition's tool change has `M6` (grblHAL; GRBL 1.1 has
/// none, #57), canned cycles where the definition has them (LinuxCNC, not
/// grblHAL or GRBL), each decided from the definition as `validate`
/// decides it.
#[test]
fn m6_and_canned_cycles_offered_per_definition() {
    let base = plaque();
    for definition in dc_project::latest_definitions().expect("bundled") {
        let project = on_post(base.clone(), definition.id);
        let offers = project::offers(&project);
        let post = definition.post_processor();
        assert_eq!(
            offers.m6 == Offer::Offered,
            post.tool_change.has_m6(),
            "{}",
            definition.id
        );
        assert_eq!(
            offers.canned_cycles == Offer::Offered,
            post.canned_cycles.is_some(),
            "{}",
            definition.id
        );
        let modes = project::tool_change_modes(&project);
        assert_eq!(
            modes.contains(&ToolChangeMode::M6),
            post.tool_change.has_m6(),
            "{}: {modes:?}",
            definition.id
        );
        assert_eq!(modes.first(), Some(&ToolChangeMode::SeparatePrograms));
    }
    let offers = |id: &str| project::offers(&on_post(base.clone(), id));
    assert_eq!(offers("grblhal").m6, Offer::Offered);
    assert!(matches!(offers("grbl").m6, Offer::Refused { .. }));
    assert_eq!(offers("linuxcnc").canned_cycles, Offer::Offered);
    assert!(matches!(
        offers("grblhal").canned_cycles,
        Offer::Refused { .. }
    ));
    assert!(matches!(
        offers("grbl").canned_cycles,
        Offer::Refused { .. }
    ));

    // Not offered means no command: the mode and the switch cannot be set.
    let grbl = on_post(base.clone(), "grbl");
    assert_eq!(
        project::set_tool_change_mode(&grbl, ToolChangeMode::M6),
        None
    );
    assert_eq!(project::set_canned_cycles(&grbl, true), None);

    // Where offered, they write the file's fields.
    let mut h = Harness::from_project(on_post(base, "linuxcnc"));
    choose(
        &mut h,
        |p| project::set_canned_cycles(p, true),
        "canned cycles",
    );
    assert!(saved(&h).post_processor.use_canned_cycles);
    assert_eq!(Choice::CannedCycles.status(h.app.warnings()), None);
}

/// Acceptance 69, last clause (capability 60; S20, export refuses on the
/// CLI's conditions): with the thickness unset the field carries the
/// file's `stock_thickness_unset` error and export is refused with the
/// same message; typing the thickness clears both.
#[test]
fn thickness_unset_disables_export() {
    let mut h = Harness::open(&plaque_bytes()).expect("the plaque opens");
    assert_eq!(project::export_refusal(&h.app), None, "the plaque exports");
    assert_eq!(Field::StockThickness.status(h.app.warnings()), None);

    type_into(&mut h, Field::StockThickness, "");
    assert_eq!(h.app.project().stock.thickness_mm, None);
    assert_eq!(Field::StockThickness.text(h.app.project()), "");
    let message = Problem::StockThicknessUnset.to_string();
    let status = Field::StockThickness
        .status(h.app.warnings())
        .expect("the thickness is marked");
    assert_eq!(status.tier, Tier::Error);
    assert_eq!(status.message, message);
    let refusal = project::export_refusal(&h.app).expect("export is refused");
    assert_eq!(refusal.message, message);
    assert_eq!(refusal.field.as_deref(), Some(Field::StockThickness.path()));

    type_into(&mut h, Field::StockThickness, "11.8");
    assert_eq!(Field::StockThickness.status(h.app.warnings()), None);
    assert_eq!(project::export_refusal(&h.app), None);
}

/// Slice 23 (#615), capability 60: Project Settings opens on an error in
/// one of its own fields (the thickness unset), not on an operation's or
/// a tool's, and not on a warning.
#[test]
fn settings_error_is_a_project_settings_field_only() {
    let mut h = Harness::open(&plaque_bytes()).expect("the plaque opens");
    assert!(!project::settings_error(h.app.warnings()), "the plaque");
    type_into(&mut h, Field::StockThickness, "");
    assert!(project::settings_error(h.app.warnings()), "thickness unset");

    let at = |field: &str, tier: Tier| dc_job::Finding {
        field: Some(field.to_owned()),
        code: None,
        message: String::new(),
        tier,
        op: None,
        shape: None,
    };
    for field in [
        "heights.clearance_mm",
        "post_processor",
        "post_processor.file_extension",
        "origin.xy",
        "arc_fit_tolerance_mm",
    ] {
        assert!(
            project::settings_error(&[at(field, Tier::Error)]),
            "{field}"
        );
    }
    for field in [
        "tools[t1].feed_mm_min",
        "operations[o1].depth_mm",
        "stockade",
    ] {
        assert!(
            !project::settings_error(&[at(field, Tier::Error)]),
            "{field}"
        );
    }
    assert!(!project::settings_error(&[at(
        "stock.thickness_mm",
        Tier::Warning
    )]));
}
