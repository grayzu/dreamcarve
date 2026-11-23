// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Clauses of phase 3's acceptance tests asserted through the binary
//! (`docs/specs/phase-3/functional.md`, "Acceptance tests", 55, 61, 61j,
//! 61m and 61n; `docs/specs/phase-3/audit.md` lists which clause each test
//! holds).

use super::*;

/// Acceptance 55's second half in `dir`: the coaster with its operations
/// replaced by a Pocket `o2` of a 40 × 40 mm square, 4 mm deep with the
/// coaster's end mill, and after it a countersink `o3` of four 3.175 mm
/// circles inside it with a 90° 6.35 mm V-bit, a 6 mm mouth, starting
/// `start_depth_mm` below the stock top. Returns the `.dcv` path.
fn countersinks_in_a_pocket(dir: &Path, start_depth_mm: f64) -> PathBuf {
    let dcv = init_coaster(dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    let end_mill = project
        .operations
        .first()
        .expect("init writes a Profile")
        .tool()
        .clone();
    let mut bit = dc_project::test_support::tool_v_bit_90();
    bit.id = ToolId::new("tv");
    bit.feed_mm_min = Some(1500.0);
    bit.plunge_mm_min = Some(500.0);
    bit.rpm = Some(18000);
    project.tools.push(bit);
    let mut place = |id: &str, profile: Polyline, x_mm: f64, y_mm: f64| {
        let definition = DefinitionId::new(format!("d-{id}"));
        project.definitions.insert(
            definition.clone(),
            FeatureDefinition {
                id: definition.clone(),
                name: id.into(),
                shape: None,
                profiles: vec![profile],
            },
        );
        project.instances.push(FeatureInstance {
            id: InstanceId::new(id),
            definition,
            transform: Transform::translate(x_mm, y_mm),
            name: id.into(),
            visible: true,
            locked: false,
            construction: false,
            folder: None,
            z_top_mm: None,
            z_bottom_mm: None,
        });
        InstanceId::new(id)
    };
    let square = place(
        "square",
        Polyline::rect(Point::new(0.0, 0.0), 40.0, 40.0),
        35.0,
        35.0,
    );
    let holes: Vec<InstanceId> = [(45.0, 45.0), (65.0, 45.0), (65.0, 65.0), (45.0, 65.0)]
        .into_iter()
        .enumerate()
        .map(|(k, (x, y))| {
            place(
                &format!("hole{}", k + 1),
                Polyline::circle(Point::new(0.0, 0.0), 3.175 / 2.0),
                x,
                y,
            )
        })
        .collect();
    let mut pocket = sample_pocket();
    pocket.targets = vec![square];
    pocket.tool = end_mill;
    pocket.step_over_fraction = Some(0.4);
    pocket.depth_mm = Some(4.0);
    let mut sink = sample_drill();
    sink.name = "Countersinks".into();
    sink.targets = holes;
    sink.tool = ToolId::new("tv");
    sink.depth_mm = None;
    sink.cycle = DrillCycle::Countersink {
        diameter_mm: 6.0,
        start_depth_mm,
        start_plane_cleared_externally: false,
    };
    project.operations = vec![Operation::Pocket(pocket), Operation::Drill(sink)];
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    dcv
}

/// The Z each plunge of the countersinks' program in `dir` ends at: the
/// straight cuts of the program that holds the `Countersinks` operation
/// which move in Z alone.
fn countersink_plunges(dir: &Path) -> Vec<f64> {
    let program = programs_in(dir)
        .into_iter()
        .map(|name| std::fs::read_to_string(dir.join(name)).expect("program readable"))
        .find(|text| text.contains("Countersinks )"))
        .expect("a program holds the countersinks");
    dc_gcode::parse_motion(&program)
        .expect("the program parses")
        .iter()
        .filter(|m| m.kind.is_cut() && m.to.x == m.from.x && m.to.y == m.from.y)
        .filter_map(|m| m.to.z)
        .collect()
}

/// Acceptance 55: four countersinks in the floor of a Pocket 4 mm deep
/// that runs before them. With `start_depth_mm: 4` `check` is quiet and
/// each plunge ends at Z −7, 3 mm below the floor: the 6 mm mouth opens
/// at the floor. With `start_depth_mm: 0` `check` warns
/// `start_depth_mismatch` once, at the countersink's start depth, naming
/// the Pocket and its 4 mm, and still exports: each plunge then ends at
/// Z −3, so the mouth would open at the stock top, 4 mm above the floor,
/// and the tip stops 1 mm short of it. The bit cuts nothing but air in
/// the Pocket, which is why the chapter shows this case.
#[test]
fn countersink_from_the_stock_top_in_a_pocket_warns_and_cuts_above_its_floor() {
    let dir = work_dir("countersink_in_a_pocket_at_its_floor");
    countersinks_in_a_pocket(&dir, 4.0);
    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert!(!report.contains("start_depth_"), "{report}");
    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "export: {report}");
    let plunges = countersink_plunges(&dir);
    assert_eq!(plunges.len(), 4, "{plunges:?}");
    for z in plunges {
        assert_approx_eq!(z, -7.0, 1e-9);
    }

    let dir = work_dir("countersink_in_a_pocket_from_the_stock_top");
    let dcv = countersinks_in_a_pocket(&dir, 0.0);
    let out = run(&dir, &["check", "coaster.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "a warning exits 0: {report}");
    let mismatches: Vec<&str> = warning_lines(&report)
        .into_iter()
        .filter(|l| warning_code(l) == Some("start_depth_mismatch"))
        .collect();
    assert_eq!(mismatches.len(), 1, "{report}");
    let at = position_of(&dcv, "operations[o3].cycle.countersink.start_depth_mm");
    assert!(
        mismatches[0].starts_with(&format!(
            "coaster.dcv:{at}: warning: start_depth_mismatch: operation `o3`: "
        )),
        "located at the start depth: {}",
        mismatches[0]
    );
    assert!(
        mismatches[0].contains("starts 0 mm below the stock top")
            && mismatches[0].contains("operation `o2`")
            && mismatches[0].contains("is 4 mm deep"),
        "names the Pocket and both depths: {}",
        mismatches[0]
    );
    assert!(!report.contains("start_depth_uncleared"), "{report}");

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "export: {report}");
    let plunges = countersink_plunges(&dir);
    assert_eq!(plunges.len(), 4, "{plunges:?}");
    let floor_z_mm = -4.0;
    for z in plunges {
        assert_approx_eq!(z, -3.0, 1e-9);
        assert_approx_eq!(z - floor_z_mm, 1.0, 1e-9);
    }
}

/// The `<g id="…" class="operation">` groups of a preview, in the order
/// drawn: each run's operation id and what it holds.
fn preview_runs(svg: &str) -> Vec<(&str, &str)> {
    svg.split("<g id=\"")
        .skip(1)
        .map(|chunk| {
            let (id, rest) = chunk.split_once('"').expect("the id ends");
            let (body, _) = rest.split_once("</g>").expect("the group ends");
            (id, body)
        })
        .collect()
}

/// The points the `cut` paths of a preview group pass through, in project
/// space: Y flipped back about a stock `height_mm` tall.
fn drawn_cuts(group: &str, height_mm: f64) -> Vec<Point> {
    svg_elements(group, "cut")
        .iter()
        .flat_map(|cut| svg_path_points(svg_attr(cut, "d")))
        .map(|(x, y)| Point::new(x, height_mm - y))
        .collect()
}

/// The closed outlines of instance `id` of `project`, placed.
fn placed_outlines(project: &Project, id: &str) -> Vec<Polyline> {
    let instance = project
        .instances
        .iter()
        .find(|i| i.id.as_str() == id)
        .unwrap_or_else(|| panic!("the project has instance `{id}`"));
    project
        .definitions
        .get(&instance.definition)
        .expect("the instance's definition")
        .profiles
        .iter()
        .filter(|p| p.is_closed())
        .map(|p| instance.transform.apply_polyline(p).expect("placed"))
        .collect()
}

/// The direction a segment of a polyline leaves `from` in and the one it
/// reaches `to` in, radians, for a segment of `bulge`: the chord's, turned
/// by half the arc's angle either way.
fn tangents(from: Point, to: Point, bulge: f64) -> (f64, f64) {
    let chord = (to.y - from.y).atan2(to.x - from.x);
    let half = 2.0 * bulge.atan();
    (chord - half, chord + half)
}

/// The corners of the shapes `outlines` bound that point outward and turn
/// by more than `least_turn_deg`: serif tips and stroke ends, where a
/// medial line ends at the surface. The outlines are nested first, so the
/// shape is on the left of each contour and such a corner is a left turn.
fn outward_corners(outlines: &[Polyline], least_turn_deg: f64) -> Vec<Point> {
    let contours = outlines
        .iter()
        .map(|o| dc_geometry::Contour::new(o.clone()).expect("a closed outline"))
        .collect();
    let (regions, _) = dc_geometry::nest(contours).expect("the outlines nest");
    let mut corners = Vec::new();
    for contour in regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
    {
        let vertices = contour.polyline().vertices();
        // Each vertex with the one before it and the one after, round
        // the closed outline.
        let before = vertices.iter().cycle().skip(vertices.len() - 1);
        let after = vertices.iter().cycle().skip(1);
        for ((before, at), after) in before.zip(vertices).zip(after) {
            let (_, arriving) = tangents(before.point(), at.point(), before.bulge);
            let (leaving, _) = tangents(at.point(), after.point(), at.bulge);
            let turn = (leaving - arriving).sin().atan2((leaving - arriving).cos());
            if turn.to_degrees() > least_turn_deg {
                corners.push(at.point());
            }
        }
    }
    corners
}

/// Acceptance 61: `check --preview` on the sign draws what each run cuts.
/// The end mill's floor rings run round the oval: its clearing run stays
/// on the background's floor, at least the slope's 2.5 mm and its own
/// radius from the oval, and its innermost ring hugs the oval all the way
/// round. The V-bit's lines run into every serif of the lettering: each
/// outward corner of the letters is the end of a drawn line. The two
/// countersinks are drawn as the V-bit's circle at each hanging hole, and
/// the cut-out as the outline grown by the end mill's radius with its
/// four tabs marked on it.
#[test]
fn sign_preview_draws_the_rings_the_serifs_the_countersinks_and_the_tabs() {
    let dir = work_dir("sign_preview_draws_the_rings_the_serifs_the_countersinks_and_the_tabs");
    copy_fixture(&dir, "sign.dcv", "sign.dcv");
    let out = run(&dir, &["check", "sign.dcv", "--preview", "preview.svg"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "check: {report}");
    let svg = std::fs::read_to_string(dir.join("preview.svg")).expect("preview written");
    let project = sign_project();
    let height_mm = project.stock.height_mm;

    let runs = preview_runs(&svg);
    let ids: Vec<&str> = runs.iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, ["op1", "op2", "op2", "op3", "op5", "op4"], "{report}");
    let legend: Vec<&str> = svg_elements(&svg, "legend-entry")
        .iter()
        .map(|e| {
            let (_, text) = e.split_once('>').expect("the entry's text");
            text.trim_end()
        })
        .collect();
    assert_eq!(
        legend,
        [
            "op1 Hanging holes",
            "op2 (clearing t2) Background",
            "op2 Background",
            "op3 Lettering",
            "op5 Countersinks",
            "op4 Cut out",
        ]
    );

    // The end mill's floor rings round the oval: 6.35 mm across, on the
    // floor a 90° bit's 2.5 mm slope leaves.
    let oval = placed_outlines(&project, "i3");
    let oval_box = oval[0].bounds().expect("the oval's box");
    let centre = Point::new(
        (oval_box.min.x + oval_box.max.x) / 2.0,
        (oval_box.min.y + oval_box.max.y) / 2.0,
    );
    let nearest_ring_mm = 2.5 + 6.35 / 2.0;
    let rings = drawn_cuts(runs[1].1, height_mm);
    assert!(rings.len() > 100, "the clearing run is drawn");
    let mut sectors = [false; 12];
    for p in &rings {
        let away = distance_to_outlines_mm(*p, &oval);
        assert!(
            away >= nearest_ring_mm - 0.01,
            "the end mill is drawn at {p:?}, {away} mm from the oval"
        );
        if away <= nearest_ring_mm + 0.01 {
            let angle = (p.y - centre.y).atan2(p.x - centre.x) + std::f64::consts::PI;
            let sector = (angle / std::f64::consts::TAU * 12.0)
                .floor()
                .clamp(0.0, 11.0);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a whole number from 0 to 11"
            )]
            let sector = sector as usize;
            sectors[sector] = true;
        }
    }
    assert!(
        sectors.iter().all(|&drawn| drawn),
        "the innermost ring runs all the way round the oval: {sectors:?}"
    );

    // The V-bit's lines into every serif.
    let lines = drawn_cuts(runs[3].1, height_mm);
    let lettering = v_carve_op(&project, "op3");
    let mut serifs = 0;
    for target in &lettering.targets {
        let letter = placed_outlines(&project, target.as_str());
        let corners = outward_corners(&letter, 30.0);
        assert!(
            !corners.is_empty(),
            "`{target}` has a serif or a stroke end"
        );
        for corner in corners {
            let nearest = lines
                .iter()
                .map(|p| (*p - corner).length())
                .fold(f64::INFINITY, f64::min);
            assert!(
                nearest <= 0.02,
                "no line is drawn into the corner of `{target}` at {corner:?}: the nearest ends {nearest} mm away"
            );
            serifs += 1;
        }
    }
    // "DreamCarve" in Droid Serif Bold has 57 of them.
    assert!(serifs >= 50, "the lettering has its serifs: {serifs}");

    // The two countersinks: the V-bit's circle at each hanging hole.
    let sinks = svg_elements(runs[4].1, "hole");
    let mut drawn: Vec<(&str, &str, &str)> = sinks
        .iter()
        .map(|e| (svg_attr(e, "cx"), svg_attr(e, "cy"), svg_attr(e, "r")))
        .collect();
    drawn.sort_unstable();
    assert_eq!(
        drawn,
        [
            ("195.000", "25.000", "3.175"),
            ("25.000", "25.000", "3.175")
        ]
    );

    // The tabbed outline: the 200 × 80 mm rectangle at (10, 10) grown by
    // the end mill's radius, each of its four sides drawn, a tab on each.
    let (low_x, high_x, low_y, high_y) = (6.825, 213.175, 6.825, 93.175);
    let on_path = |p: Point| {
        let on_x = (p.x - low_x).abs() < 0.002 || (p.x - high_x).abs() < 0.002;
        let on_y = (p.y - low_y).abs() < 0.002 || (p.y - high_y).abs() < 0.002;
        (on_x && (low_y..=high_y).contains(&p.y)) || (on_y && (low_x..=high_x).contains(&p.x))
    };
    let outline = drawn_cuts(runs[5].1, height_mm);
    for side in [
        |p: &Point| (p.x - 6.825).abs() < 0.002,
        |p: &Point| (p.x - 213.175).abs() < 0.002,
        |p: &Point| (p.y - 6.825).abs() < 0.002,
        |p: &Point| (p.y - 93.175).abs() < 0.002,
    ] {
        assert!(
            outline.iter().filter(|p| side(p)).count() >= 2,
            "a side of the outline is not drawn"
        );
    }
    let tabs = svg_elements(runs[5].1, "tab");
    assert_eq!(tabs.len(), 4, "the cut-out's four tabs");
    for tab in tabs {
        for (x, y) in svg_path_points(svg_attr(tab, "d")) {
            let end = Point::new(x, height_mm - y);
            assert!(on_path(end), "a tab mark ends off the outline at {end:?}");
        }
    }
}

/// Acceptance 61j: "the output of 61g is the same whatever the project it
/// is later used in". `image-prep` takes no project and reads none: the
/// same drawing prepared in an empty folder, and in a folder that holds a
/// project of the drawing's own name (the sign's, with its stock, tools
/// and depths) under a user config that changes what `init` writes, gives
/// the same bytes, and the project beside it is not touched.
#[test]
fn image_prep_output_is_the_same_whatever_project_is_beside_it() {
    let alone = work_dir("image_prep_output_alone");
    copy_fixture(&alone, "prep/ring-cross.svg", "ring-cross.svg");
    let out = run(&alone, &["image-prep", "ring-cross.svg", "--regions"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let want = std::fs::read(alone.join("ring-cross-inlay.svg")).expect("written");

    let beside = work_dir("image_prep_output_beside_a_project");
    copy_fixture(&beside, "prep/ring-cross.svg", "ring-cross.svg");
    let project = copy_fixture(&beside, "sign.dcv", "ring-cross.dcv");
    let before = std::fs::read(&project).expect("project readable");
    write_config(&beside, "[export]\ntool_change_mode = \"m6\"\n");
    let out = run(&beside, &["image-prep", "ring-cross.svg", "--regions"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let got = std::fs::read(beside.join("ring-cross-inlay.svg")).expect("written");
    assert!(got == want, "the prepared drawing differs beside a project");
    assert!(
        std::fs::read(&project).expect("project readable") == before,
        "image-prep changed the project beside the drawing"
    );
}

/// Acceptance 61m: with the "DC" project's frame, moved 3 mm right, as
/// the boundary, `check` prints `inlay_male_mirrored` once, at
/// X = 55 mm: the mirror line is the letters', not the boundary's, which
/// is mirrored with them.
#[test]
fn moved_boundary_check_names_the_mirror_at_x_55() {
    let dir = work_dir("moved_boundary_check_names_the_mirror_at_x_55");
    dc_with_boundary(&dir, 3.0, false);
    let out = run(&dir, &["check", "dc.dcv"]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{report}");
    let mirrored: Vec<&str> = warning_lines(&report)
        .into_iter()
        .filter(|l| warning_code(l) == Some("inlay_male_mirrored"))
        .collect();
    assert_eq!(mirrored.len(), 1, "{report}");
    assert!(
        mirrored[0].contains("mirrored about X = 55 mm"),
        "{}",
        mirrored[0]
    );
    assert_approx_eq!(mirror_x_mm(&report), 55.0, 1e-9);
}

/// Acceptance 61n: `inlay_boundary_empty` is "a warning that does not
/// stop export". The "DC" pair on stock 205 mm wide with its frame as the
/// boundary and the frame drawn again 95 mm to the right, where it holds
/// no part of the design: `check` and `export` warn once, at the second
/// entry of the list, exit 0, and `export` writes the pair's five
/// programs.
#[test]
fn empty_boundary_warns_and_still_exports() {
    let dir = work_dir("empty_boundary_warns_and_still_exports");
    let dcv = dc_with_boundary(&dir, 0.0, false);
    let mut project = load_file(&dcv).project;
    project.stock.width_mm = 205.0;
    let frame = project
        .instances
        .iter()
        .find(|i| i.id.as_str() == "i3")
        .expect("the frame")
        .clone();
    project.instances.push(FeatureInstance {
        id: InstanceId::new("i4"),
        name: "frame-2".into(),
        transform: frame.transform.then(Transform::translate(95.0, 0.0)),
        ..frame
    });
    for op in &mut project.operations {
        if let Operation::Inlay(inlay) = op {
            inlay.boundary.push(InstanceId::new("i4"));
        }
    }
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let at = position_of(&dcv, "operations[op1].boundary[1]");

    for command in ["check", "export"] {
        let mut args = vec![command, "dc.dcv"];
        if command == "export" {
            args.extend(["--date", FIXED_DATE]);
        }
        let out = run(&dir, &args);
        let report = format!("{}{}", stdout(&out), stderr(&out));
        assert_eq!(out.status.code(), Some(0), "{command}: {report}");
        let empty: Vec<&str> = warning_lines(&report)
            .into_iter()
            .filter(|l| warning_code(l) == Some("inlay_boundary_empty"))
            .collect();
        assert_eq!(empty.len(), 1, "{command}: {report}");
        assert!(
            empty[0].starts_with(&format!("dc.dcv:{at}: warning: inlay_boundary_empty: ")),
            "{command}: located at the entry: {}",
            empty[0]
        );
        assert!(empty[0].contains("`i4`"), "{command}: {}", empty[0]);
    }
    assert_eq!(
        programs_in(&dir),
        [
            "dc-female-1-T2.nc",
            "dc-female-2-T1.nc",
            "dc-male-1-T2.nc",
            "dc-male-2-T1.nc",
            "dc-male-3-T2.nc",
        ]
    );
}
