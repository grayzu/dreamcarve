// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The minimal cases the properties shrank their failures to, as named
//! tests (#231, #232, #234, #235, #265, #272, #273, #322, #353, #355,
//! #356, #358): each project
//! validates, generates, and exports through the full safety pass. Every
//! case is built from `dc_project::test_support::sample_project()` on
//! 240 × 160 mm stock, its instances placed as in the issue that reports
//! it.

use super::{DATE, generate};
use dc_cam::entry::EntryKind;
use dc_cam::{MoveKind, ReliefCornerReason, Warning, WarningCode, generate_profile};
use dc_gcode::emit;
use dc_gcode::parse::{MotionKind, parse_motion};
use dc_gcode::transform::MachineTransform;
use dc_geometry::{Point, Point3, Polyline, Transform, Vertex};
use dc_project::test_support::{sample_pocket, sample_project};
use dc_project::{
    ClearanceStrategy, CornerRelief, CutDirection, DefinitionId, Entry, FeatureDefinition,
    InstanceId, LastPass, MachineOrigin, MachiningOrder, Operation, OriginXy, OriginZ, PocketOp,
    PostProcessorRef, ProfileOp, Project, Side, StartAt, Stock, TabPlacement, TabShape, TabSpec,
    Units, WallPass, validate,
};

/// The sample project on 240 × 160 × `thickness_mm` stock, its one
/// definition drawn as `profiles`, placed by `transform`, cut by `op`
/// alone.
fn case(
    profiles: Vec<Polyline>,
    transform: Transform,
    thickness_mm: f64,
    op: Operation,
) -> Project {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: 240.0,
        height_mm: 160.0,
        thickness_mm: Some(thickness_mm),
    };
    if let Some(definition) = project.definitions.values_mut().next() {
        definition.profiles = profiles;
    }
    project.instances[0].transform = transform;
    project.instances[0].z_top_mm = Some(thickness_mm);
    project.operations = vec![op];
    project
}

/// `project` validates, generates and exports; the generator's warnings.
fn exports(project: &Project) -> Vec<Warning> {
    let errors: Vec<_> = validate(project)
        .into_iter()
        .filter(dc_project::Problem::is_error)
        .collect();
    assert!(errors.is_empty(), "the project is not valid: {errors:?}");
    let op = generate(project).unwrap_or_else(|e| panic!("generation fails: {e}\n{e:?}"));
    if let Err(e) = emit(project, &[op], DATE) {
        panic!("the safety pass rejects the toolpath: {e}\n{e:?}");
    }
    match &project.operations[0] {
        Operation::Profile(p) => generate_profile(project, p).map(|(_, w)| w),
        Operation::Pocket(p) => dc_cam::generate_pocket(project, p).map(|(_, w)| w),
        _ => Ok(Vec::new()),
    }
    .unwrap_or_default()
}

/// The sample's outside Profile, untabbed, `depth_mm` deep, with `relief`.
fn outside_profile(relief: CornerRelief, depth_mm: f64) -> Operation {
    let Operation::Profile(op) = sample_project().operations[0].clone() else {
        unreachable!("the sample's operation is a Profile");
    };
    Operation::Profile(ProfileOp {
        side: Side::Outside,
        corner_relief: relief,
        tabs: None,
        depth_mm: Some(depth_mm),
        ..op
    })
}

/// #231: a 60 × 40 mm frame with a 30 × 20 mm hole, cut outside with
/// T-bones by the 6.35 mm tool. Rotated 0.3 rad, the program's rounding
/// moves each notch's end, on the hole's wall, up to a quantum into the
/// part, and S9 refused the return leg that starts there. Rotated or
/// not, T-bone or dogbone, 2 or 6 mm deep, it exports.
#[test]
fn outside_t_bone_return_from_a_rounded_wall_point_passes() {
    let frame = vec![
        Polyline::rect(Point::new(-30.0, -20.0), 60.0, 40.0),
        Polyline::rect(Point::new(-15.0, -10.0), 30.0, 20.0),
    ];
    for angle_rad in [0.3, 0.0] {
        for relief in [CornerRelief::TBone, CornerRelief::Dogbone] {
            for depth_mm in [2.0, 6.0] {
                let transform =
                    Transform::rotate(angle_rad).then(Transform::translate(120.0, 80.0));
                let project = case(
                    frame.clone(),
                    transform,
                    6.0,
                    outside_profile(relief, depth_mm),
                );
                exports(&project);
            }
        }
    }
}

/// #231's second case: a 38.717 × 25.901 mm part with an 8.094 ×
/// 7.563 mm hole and a 3.318 × 3.146 mm island part in it, all centred,
/// cut outside with T-bones by a 4.412 mm tool, 1 mm deep. The bands
/// between hole and island (2.2 and 2.4 mm) are narrower than the tool:
/// the hole's lap passes 0.18 mm from the island's corners and gouges the
/// island part, which `toolpaths_overlap` warns about, as phase 1 decided
/// for two shapes closer than the bit. Each notch in the hole's corners
/// starts at the lap's corner and runs away from the island, so it comes
/// no nearer the island than the lap: S9 lets it through as it lets the
/// lap (the lap rule looks only at the cut's own region, #192), and the
/// program exports, rotated or not.
#[test]
fn outside_t_bone_beside_an_island_part_exports() {
    let centred = |w: f64, h: f64| Polyline::rect(Point::new(-w / 2.0, -h / 2.0), w, h);
    let profiles = vec![
        centred(38.717, 25.901),
        centred(8.094, 7.563),
        centred(3.318, 3.146),
    ];
    for angle_rad in [0.0, 0.3] {
        let transform = Transform::rotate(angle_rad).then(Transform::translate(120.0, 80.0));
        let mut project = case(
            profiles.clone(),
            transform,
            6.0,
            outside_profile(CornerRelief::TBone, 1.0),
        );
        project.tools[0].diameter_mm = 4.412;
        let warnings = exports(&project);
        let codes: Vec<WarningCode> = warnings.iter().map(Warning::code).collect();
        assert_eq!(
            codes,
            [WarningCode::ToolpathsOverlap],
            "{angle_rad}: {warnings:?}"
        );
    }
}

/// #322 (the Profile property, `PROPTEST_RNG_SEED=100`): a 20.47 ×
/// 14.90 mm part with a 9.23 × 8.39 mm hole, rotated and placed at
/// (40, 40), cut outside with T-bones by a 6.84 mm tool, 1.01 mm deep in
/// one pass, on grblHAL. Each of the hole's corners has its T-bone along
/// the hole's longer (9.23 mm) wall, so the notch reaches the tool's
/// radius, 3.42 mm, past the hole's top or bottom wall; the part is only
/// 3.26 mm thick there, so the notch would cut through it into the
/// outside, and S9 refused it. The generator skips each such corner with
/// [`Warning::ReliefCornerTooTight`]. The program exports; no cut brings
/// the cutter into the part; no Z is below the final depth or the
/// material bottom; and every feed is the tool's.
#[test]
fn outside_t_bone_in_a_hole_just_wider_than_the_bit_exports() {
    let project = t_bone_in_a_tight_hole();
    let warnings = exports(&project);
    let skipped: Vec<&Warning> = warnings
        .iter()
        .filter(|w| w.code() == WarningCode::ReliefCornerTooTight)
        .collect();
    assert_eq!(skipped.len(), 4, "the hole's four corners: {warnings:?}");
    assert!(
        skipped.iter().all(|w| matches!(
            w,
            Warning::ReliefCornerTooTight {
                reason: ReliefCornerReason::NearOtherWall,
                ..
            }
        )),
        "{warnings:?}"
    );

    let instance = &project.instances[0];
    let to_local = instance.transform.inverse().expect("a rotation inverts");
    let (outer_x, outer_y) = (10.236_917_615_093_802, 7.450_461_970_335_859);
    let (hole_x, hole_y) = (4.617_180_390_808_221, 4.195_238_437_099_146);
    // How far the cutter's centre at `p` (project XY) is from the part's
    // material: the outer rectangle less the hole, both centred on the
    // definition's origin.
    let clearance_mm = |p: Point| {
        let local = to_local.apply(p);
        let (x, y) = (local.x.abs(), local.y.abs());
        if x < hole_x && y < hole_y {
            (hole_x - x).min(hole_y - y)
        } else if x > outer_x || y > outer_y {
            (x - outer_x).max(0.0).hypot((y - outer_y).max(0.0))
        } else {
            0.0
        }
    };
    let radius_mm = project.tools[0].diameter_mm / 2.0;
    let Operation::Profile(profile) = &project.operations[0] else {
        unreachable!("the case is a Profile");
    };
    let thickness_mm = project.stock.thickness_mm.expect("the stock is sized");
    let final_z_mm = thickness_mm - profile.depth_mm.expect("the depth is set");
    let tool = &project.tools[0];
    let rates = [
        tool.feed_mm_min.expect("a feed"),
        tool.plunge_mm_min.expect("a plunge rate"),
    ];

    let post = project
        .post_processor
        .resolve()
        .expect("grblHAL is bundled");
    let transform =
        dc_gcode::transform::MachineTransform::new(&project, &post).expect("a valid mapping");
    let op = generate(&project).expect("generates");
    let (program, _) = emit(&project, &[op], DATE).expect("exports");
    // One unit in the last place and the flatten tolerance: what S9 lets
    // a written move depart by.
    let slack_mm = 0.001 + project.flatten_tolerance_mm;
    let mut cuts = 0;
    for mv in dc_gcode::parse_motion(&program.text).expect("the program parses") {
        let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
            (mv.from.x, mv.from.y, mv.from.z, mv.to.x, mv.to.y, mv.to.z)
        else {
            continue;
        };
        let from = transform.to_project(Point3::new(fx, fy, fz), mv.units);
        let to = transform.to_project(Point3::new(tx, ty, tz), mv.units);
        assert!(to.z >= 0.0, "line {}: below the material bottom", mv.line);
        assert!(
            to.z >= final_z_mm - 0.0005,
            "line {}: below the final depth",
            mv.line
        );
        if !mv.kind.is_cut() {
            continue;
        }
        cuts += 1;
        // Every feed is the tool's, or a ramp's feed the emitter derived
        // from them and rounded down (#325): never above the feed, and
        // never descending faster than the plunge rate.
        let feed = mv.feed.expect("a cut has a feed");
        let [feed_mm_min, plunge_mm_min] = rates;
        let derived = feed <= feed_mm_min + 0.0005 && {
            let length_mm = (to.x - from.x).hypot(to.y - from.y).hypot(to.z - from.z);
            let drop_mm = from.z - to.z;
            drop_mm <= 0.0 || feed * drop_mm / length_mm <= plunge_mm_min + 0.0005
        };
        assert!(
            rates.iter().any(|r| (feed - r).abs() <= 0.0005) || derived,
            "line {}: feed {feed} is not the tool's",
            mv.line
        );
        // The cut's path in XY: a line, or an arc about its centre words.
        let samples: Vec<Point> = match (mv.arc.i, mv.arc.j) {
            (Some(i), Some(j)) if !matches!(mv.kind, dc_gcode::parse::MotionKind::Linear) => {
                let centre = transform
                    .to_project(Point3::new(fx + i, fy + j, fz), mv.units)
                    .xy();
                let ccw = matches!(mv.kind, dc_gcode::parse::MotionKind::ArcCcw);
                let arc = dc_geometry::Arc::from_ends(from.xy(), to.xy(), centre, ccw);
                (0..=16)
                    .map(|k| {
                        let a = arc.start_angle_rad + arc.sweep_rad * f64::from(k) / 16.0;
                        centre + dc_geometry::Vec2::new(a.cos(), a.sin()) * arc.radius_mm
                    })
                    .collect()
            }
            _ => (0..=16)
                .map(|k| from.xy() + (to.xy() - from.xy()) * (f64::from(k) / 16.0))
                .collect(),
        };
        for p in samples {
            let c = clearance_mm(p);
            assert!(
                c >= radius_mm - slack_mm,
                "line {}: the cutter reaches {:.4} mm into the part",
                mv.line,
                radius_mm - c
            );
        }
    }
    assert!(cuts > 0, "the program cuts");
}

/// #322's project, as the property shrank it.
fn t_bone_in_a_tight_hole() -> Project {
    let rect = |x: f64, y: f64| {
        Polyline::new(
            vec![
                Vertex::new(-x, y, 0.0),
                Vertex::new(x, y, 0.0),
                Vertex::new(x, -y, 0.0),
                Vertex::new(-x, -y, 0.0),
            ],
            true,
        )
    };
    let profiles = vec![
        rect(10.236_917_615_093_802, 7.450_461_970_335_859),
        rect(4.617_180_390_808_221, 4.195_238_437_099_146),
    ];
    let transform = Transform::from([
        -0.671_372_193_632_167_6,
        -0.741_120_352_991_018_1,
        0.741_120_352_991_018_1,
        -0.671_372_193_632_167_6,
        40.0,
        40.0,
    ]);
    let Operation::Profile(outside) = outside_profile(CornerRelief::TBone, 1.012_347_095_014_033)
    else {
        unreachable!("outside_profile is a Profile");
    };
    let op = Operation::Profile(ProfileOp {
        direction: dc_project::CutDirection::Climb,
        step_down_mm: None,
        safe_z_mm: 1.0,
        entry: Entry::Plunge,
        lead: None,
        last_pass: None,
        machining_order: dc_project::MachiningOrder::FullDepthFirst,
        ..outside
    });
    let mut project = case(profiles, transform, 20.246_941_900_280_66, op);
    project.display_units = Units::Mm;
    project.arc_fit_tolerance_mm = None;
    project.origin = MachineOrigin {
        xy: OriginXy::Custom {
            x_mm: 138.033_995_045_598_8,
            y_mm: 122.911_474_822_395_33,
        },
        z: OriginZ::StockTop,
    };
    project.post_processor = PostProcessorRef::latest("grblhal").expect("grblHAL is bundled");
    let tool = &mut project.tools[0];
    tool.diameter_mm = 6.840_693_609_003_763;
    tool.feed_mm_min = Some(2_952.738_628_505_208);
    tool.plunge_mm_min = Some(2_760.977_947_122_163);
    tool.rpm = Some(19_265);
    tool.step_down_mm = Some(5.860_938_183_944_079);
    tool.step_over_fraction = Some(0.410_720_724_256_278_6);
    project
}

/// #232: a raster Pocket round a rectangular island, whose stay-down link
/// between two raster ends on the round tool-centre boundary about the
/// island's corner was 0.34 mm long, so `link_inside_cleared` sampled no
/// point inside it and the link cut 0.024 mm into the island (S9). Arc
/// fitting is off: with it on, this case still trips #233.
#[test]
fn raster_link_round_an_island_corner_exports() {
    let profiles = vec![
        Polyline::rect(
            Point::new(-14.917_410_529_058_088, -22.545_952_942_869_242),
            29.834_821_058_116_176,
            45.091_905_885_738_484,
        ),
        Polyline::rect(
            Point::new(-7.825_474_403_184_983, -7.242_710_582_657_877),
            15.650_948_806_369_966,
            14.485_421_165_315_754,
        ),
    ];
    let op = PocketOp {
        targets: sample_pocket().targets,
        strategy: ClearanceStrategy::Raster {
            angle_deg: 24.628_005_484_201_154,
        },
        depth_mm: Some(1.0),
        wall_pass: WallPass::Last,
        ..sample_pocket()
    };
    let mut project = case(
        profiles,
        Transform::translate(120.0, 80.0),
        6.0,
        Operation::Pocket(op),
    );
    project.arc_fit_tolerance_mm = None;
    project.tools[0].diameter_mm = 1.644_546_628_706_222_7;
    project.tools[0].step_over_fraction = Some(0.135_681_152_562_577_37);
    exports(&project);
}

/// #234: a ramp whose depth is a whole number of legs and a sliver ended
/// with a last leg dropping 0.0008 mm over 0.004 mm, written level at the
/// ramp's plunge-limited feed, which S3 refused. The issue's case ramped
/// from safe Z, 8.0018 legs; since #214 a ramp starts 1 mm above the
/// floor before it, so the step-down here is set to make each pass's
/// ramp 4.0018 legs of 0.44647 mm (2.3053 mm runs at 10.96°).
#[test]
fn ramp_with_a_sliver_of_a_last_leg_exports() {
    let (hw, hh) = (9.850_672_336_637_954, 6.689_536_207_079_782);
    let thickness_mm = 21.597_247_651_820_073;
    let op = PocketOp {
        strategy: ClearanceStrategy::Raster {
            angle_deg: 100.487_777_934_518_29,
        },
        wall_pass: WallPass::None,
        depth_mm: Some(5.211_308_473_146_035),
        step_down_mm: Some(0.786_675_376_765_386_9),
        entry: Entry::Ramp {
            angle_deg: 10.960_619_033_582_459,
        },
        safe_z_mm: 2.830_134_574_797_714,
        ..sample_pocket()
    };
    let mut project = case(
        vec![Polyline::rect(Point::new(-hw, -hh), 2.0 * hw, 2.0 * hh)],
        Transform::translate(120.0, 80.0),
        thickness_mm,
        Operation::Pocket(op),
    );
    project.origin = MachineOrigin {
        xy: OriginXy::BottomLeft,
        z: OriginZ::StockTop,
    };
    project.arc_fit_tolerance_mm = None;
    let tool = &mut project.tools[0];
    tool.diameter_mm = 0.768_445_642_847_113_7;
    tool.feed_mm_min = Some(1_942.285_697_480_094_8);
    tool.plunge_mm_min = Some(141.500_784_461_321_22);
    tool.step_down_mm = Some(1.661_242_934_006_000_2);
    tool.step_over_fraction = Some(0.333_786_017_227_830_1);
    exports(&project);
}

/// #235: an offset Pocket of a circle with a rectangular island holding a
/// nested circular pocket. With the 0.55 mm allowance the nested pocket's
/// tool-centre region is 1.4994 mm in radius, 5.0006 step-overs, so its
/// sixth ring was 0.00018 mm in radius: three arcs whose ends all round
/// to one point, which the program cannot reproduce (S6).
#[test]
fn offset_pocket_with_a_sub_tolerance_last_ring_exports() {
    let profiles = vec![
        Polyline::circle(Point::new(0.0, 0.0), 14.502_247_711_798_354),
        Polyline::rect(
            Point::new(-3.695_065_800_761_692, -3.602_490_119_677_72),
            7.390_131_601_523_384,
            7.204_980_239_355_44,
        ),
        Polyline::circle(Point::new(0.0, 0.0), 2.906_933_833_965_864_4),
    ];
    let op = PocketOp {
        wall_pass: WallPass::None,
        depth_mm: Some(1.0),
        step_over_fraction: Some(0.174_767_951_923_974_57),
        allowance_mm: 0.549_697_563_087_318_1,
        ..sample_pocket()
    };
    let mut project = case(
        profiles,
        Transform::translate(120.0, 80.0),
        6.0,
        Operation::Pocket(op),
    );
    let tool = &mut project.tools[0];
    tool.diameter_mm = 1.715_671_296_509_893_3;
    tool.feed_mm_min = Some(3_994.519);
    tool.plunge_mm_min = Some(805.055);
    exports(&project);
}

/// #265 (the Profile property, `PROPTEST_RNG_SEED=108`): an Outside
/// Profile of a 13.847 mm circle by a 0.5508 mm tool with a 0.05 helix at
/// 14.14°. The helix is 0.0275 mm across, and each of its chords falls
/// 0.00045 mm over 0.0018 mm, so in three places most were written level
/// at the plunge-limited 2643.095 mm/min, which S3 refused. The helix is
/// not cut; the pass ramps instead and says so (`entry_fallback`), in mm
/// and in inch.
#[test]
fn helix_too_small_to_write_falls_back_and_exports() {
    let Operation::Profile(outside) = outside_profile(CornerRelief::None, 0.436_449_925_824_323_2)
    else {
        unreachable!("outside_profile is a Profile");
    };
    let op = Operation::Profile(ProfileOp {
        entry: Entry::Helix {
            diameter_fraction: 0.05,
            angle_deg: 14.137_048_663_595_39,
        },
        ..outside
    });
    let circle = vec![Polyline::circle(
        Point::new(0.0, 0.0),
        13.847_180_826_619_223,
    )];
    let transform = Transform::from([
        0.119_284_082_705_176_67,
        0.992_860_165_186_006_7,
        -0.992_860_165_186_006_7,
        0.119_284_082_705_176_67,
        40.0,
        40.0,
    ]);
    for units in [Units::Mm, Units::Inch] {
        let mut project = case(circle.clone(), transform, 8.728_998_516_486_463, op.clone());
        project.display_units = units;
        project.origin = MachineOrigin {
            xy: OriginXy::BottomRight,
            z: OriginZ::StockTop,
        };
        project.post_processor = PostProcessorRef::latest("mach3").expect("Mach3 is bundled");
        let tool = &mut project.tools[0];
        tool.diameter_mm = 0.550_832_666_619_620_7;
        tool.feed_mm_min = Some(3_840.280_223_564_292_6);
        tool.plunge_mm_min = Some(645.988_501_885_48);
        let warnings = exports(&project);
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                Warning::EntryFallback {
                    requested: EntryKind::Helix,
                    used: EntryKind::Ramp,
                    ..
                }
            )),
            "{units:?}: {warnings:?}"
        );
    }
}

/// #272 (the Pocket property, `PROPTEST_RNG_SEED=108`): a raster Pocket
/// of a 23 × 26.9 mm rectangle and a ring (13.753 and 6.853 mm circles)
/// by a 3.235 mm tool with a 0.976 helix at 9.316°. The helix's straight
/// handoff to a raster start on the island's tool-centre circle passed
/// 0.0151 mm inside that circle, past the flatten tolerance S9 allows, and
/// S9 refused it, in mm and in inch. The handoff now keeps to the
/// flattened region (`entry::safe_endpoint`); the helix is still cut, and
/// the program exports in both.
#[test]
fn helix_handoff_in_a_ring_pocket_exports() {
    let clockwise = |r: f64| {
        Polyline::new(
            vec![Vertex::new(-r, 0.0, -1.0), Vertex::new(r, 0.0, -1.0)],
            true,
        )
    };
    let ring = vec![
        clockwise(13.752_766_298_916_859),
        clockwise(6.853_196_837_408_963),
    ];
    let (hw, hh) = (11.499_195_343_669_689, 13.456_119_896_218_437);
    let rect = FeatureDefinition {
        id: DefinitionId::new("d0"),
        name: "Cell 0".into(),
        profiles: vec![Polyline::rect(Point::new(-hw, -hh), 2.0 * hw, 2.0 * hh)],
    };
    let transform = Transform::from([
        0.995_419_341_093_451_9,
        -0.095_605_101_208_450_6,
        0.095_605_101_208_450_6,
        0.995_419_341_093_451_9,
        40.0,
        120.0,
    ]);
    let thickness_mm = 21.905_684_661_758_25;
    let op = PocketOp {
        strategy: ClearanceStrategy::Raster {
            angle_deg: 150.923_010_870_125_7,
        },
        wall_pass: WallPass::None,
        depth_mm: Some(thickness_mm),
        step_down_mm: Some(4.140_381_675_746_728),
        step_over_fraction: Some(0.518_261_200_233_289_6),
        entry: Entry::Helix {
            diameter_fraction: 0.976_100_214_112_066_5,
            angle_deg: 9.315_963_293_422_808,
        },
        safe_z_mm: 1.325_278_948_592_093_3,
        targets: vec![InstanceId::new("i0"), InstanceId::new("i1")],
        ..sample_pocket()
    };
    for units in [Units::Mm, Units::Inch] {
        let mut project = case(
            ring.clone(),
            transform,
            thickness_mm,
            Operation::Pocket(op.clone()),
        );
        let mut cell = project.instances[0].clone();
        cell.id = InstanceId::new("i0");
        cell.definition = rect.id.clone();
        cell.transform = Transform::from([
            -0.688_295_984_496_706_4,
            0.725_429_967_485_290_5,
            -0.725_429_967_485_290_5,
            -0.688_295_984_496_706_4,
            40.0,
            40.0,
        ]);
        project.instances.insert(0, cell);
        project.definitions.insert(rect.id.clone(), rect.clone());
        project.display_units = units;
        project.origin = MachineOrigin {
            xy: OriginXy::TopRight,
            z: OriginZ::StockTop,
        };
        project.arc_fit_tolerance_mm = None;
        project.post_processor = PostProcessorRef::latest("uccnc").expect("UCCNC is bundled");
        let tool = &mut project.tools[0];
        tool.diameter_mm = 3.234_923_320_453_882;
        tool.feed_mm_min = Some(1_965.292_560_643_942);
        tool.plunge_mm_min = Some(228.163_855_244_435_87);
        tool.step_down_mm = Some(3.864_272_970_794_913);
        tool.step_over_fraction = Some(0.482_127_398_535_474_37);
        let warnings = exports(&project);
        assert!(
            !warnings
                .iter()
                .any(|w| matches!(w, Warning::EntryFallback { .. })),
            "{units:?}: {warnings:?}"
        );
    }
}

/// #273 (the Profile property, `PROPTEST_RNG_SEED=500`): an On line
/// Profile of a 14.33 mm circle by a 1.614 mm tool, ramping at 14.3°. Each
/// pass's leg ended 0.0055 mm past a chord vertex, a piece falling
/// 0.0014 mm, under an inch program's last place, so it was written level
/// at the plunge-limited 3344.631 mm/min, which S3 refused. The piece is
/// merged into the chord before it (`entry::wall::along_path`); the ramp
/// is still cut, and the program exports in mm and in inch.
#[test]
fn on_line_ramp_on_a_circle_in_inch_exports() {
    let Operation::Profile(outside) = outside_profile(CornerRelief::None, 12.526_247_561_272_07)
    else {
        unreachable!("outside_profile is a Profile");
    };
    let op = Operation::Profile(ProfileOp {
        side: Side::OnLine,
        direction: dc_project::CutDirection::Climb,
        lead: None,
        step_down_mm: None,
        safe_z_mm: 1.0,
        entry: Entry::Ramp {
            angle_deg: 14.301_902_402_989_514,
        },
        ..outside
    });
    let circle = vec![Polyline::circle(
        Point::new(0.0, 0.0),
        14.332_509_386_700_824,
    )];
    let transform = Transform::from([
        -0.854_220_693_433_206,
        0.519_910_575_878_672_4,
        -0.519_910_575_878_672_4,
        -0.854_220_693_433_206,
        120.0,
        120.0,
    ]);
    let thickness_mm = 19.578_559_092_336_132;
    for units in [Units::Mm, Units::Inch] {
        let mut project = case(circle.clone(), transform, thickness_mm, op.clone());
        project.display_units = units;
        project.origin = MachineOrigin {
            xy: OriginXy::TopRight,
            z: OriginZ::StockTop,
        };
        project.arc_fit_tolerance_mm = None;
        project.post_processor = PostProcessorRef::latest("uccnc").expect("UCCNC is bundled");
        let tool = &mut project.tools[0];
        tool.diameter_mm = 1.614_062_019_946_190_8;
        tool.feed_mm_min = Some(4_484.108_985_070_408);
        tool.plunge_mm_min = Some(826.228_185_717_551_9);
        tool.step_down_mm = Some(0.802_067_899_102_111);
        tool.step_over_fraction = Some(0.282_757_627_478_426_64);
        let warnings = exports(&project);
        assert!(
            !warnings
                .iter()
                .any(|w| matches!(w, Warning::EntryFallback { .. })),
            "{units:?}: {warnings:?}"
        );
    }
}

/// #321 (the Profile property, `PROPTEST_RNG_SEED=72` on #284's
/// strategy): an On line Profile of three circles (27.55, 20.12 and
/// 12.42 mm) by a 3.455 mm tool, 3.45 mm deep in 0.845 mm steps, ramping
/// at 43.4°, its chords fitted to arcs at 0.01 mm on GRBL. On the small
/// circle the first pass's leg ended 0.00015 mm past a chord vertex, and
/// the way back went to that vertex first: a move the program writes as
/// nothing, which arc fitting counted as one of the three chords a fitted
/// arc must cover and S12 did not, so S12 refused the arc. The way back
/// now merges that move (`entry::wall::along_path`). The program exports
/// with the ramps cut; every linear move stays on its circle within the
/// flatten tolerance and a unit; no Z the program writes is below the
/// final depth or the material bottom; and every feed is the tool's.
#[test]
fn on_line_steep_ramp_on_circles_exports() {
    let project = steep_ramp_on_circles();
    let warnings = exports(&project);
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, Warning::EntryFallback { .. })),
        "{warnings:?}"
    );
    let circles: Vec<(Point, f64)> = project
        .instances
        .iter()
        .map(|i| {
            let r = project.definitions[&i.definition].profiles[0].vertices()[0].x;
            (i.transform.apply(Point::new(0.0, 0.0)), r)
        })
        .collect();
    let op = generate(&project).expect("generates");
    let mut from: Option<Point3> = None;
    for (k, mv) in op.toolpath.moves.iter().enumerate() {
        if let (Some(a), MoveKind::Linear) = (from, mv.kind) {
            let middle = a.xy() + (mv.to.xy() - a.xy()) * 0.5;
            for p in [middle, mv.to.xy()] {
                let off = circles
                    .iter()
                    .map(|(c, r)| (p.distance_to(*c) - r).abs())
                    .fold(f64::INFINITY, f64::min);
                assert!(off <= 0.01 + 0.001, "move {k} is {off} mm off its circle");
            }
        }
        from = Some(mv.to);
    }
    let Operation::Profile(profile) = &project.operations[0] else {
        unreachable!("the case is a Profile");
    };
    let thickness_mm = project.stock.thickness_mm.expect("the stock is sized");
    let final_z_mm = thickness_mm - profile.depth_mm.expect("the depth is set");
    let feed = format!("{:.3}", project.tools[0].feed_mm_min.expect("a feed"));
    let (program, _) = emit(&project, &[op], DATE).expect("exports");
    let word = |line: &str, letter: char| {
        line.split_whitespace()
            .find_map(|w| w.strip_prefix(letter))
            .map(str::to_owned)
    };
    let mut feeds = 0;
    for line in program.text.lines().filter(|l| !l.starts_with(';')) {
        if let Some(z) = word(line, 'Z') {
            let z: f64 = z.parse().expect("a number");
            // Z origin at the stock bottom: the program's Z is the
            // project's.
            assert!(z >= 0.0, "{line}: below the material bottom");
            assert!(z >= final_z_mm - 0.0005, "{line}: below the final depth");
        }
        if let Some(f) = word(line, 'F') {
            assert_eq!(f, feed, "{line}");
            feeds += 1;
        }
    }
    assert!(feeds > 0, "the program cuts");
    assert!(
        program
            .text
            .lines()
            .any(|l| l.starts_with("G2") || l.starts_with("G3")),
        "arcs are written"
    );
}

/// #321's project.
fn steep_ramp_on_circles() -> Project {
    let Operation::Profile(outside) = outside_profile(CornerRelief::None, 3.452_154_710_812_817_7)
    else {
        unreachable!("outside_profile is a Profile");
    };
    let op = Operation::Profile(ProfileOp {
        side: Side::OnLine,
        direction: dc_project::CutDirection::Climb,
        lead: None,
        step_down_mm: None,
        safe_z_mm: 1.0,
        entry: Entry::Ramp {
            angle_deg: 43.395_007_100_651_696,
        },
        targets: vec![
            InstanceId::new("i0"),
            InstanceId::new("i5"),
            InstanceId::new("i2"),
        ],
        ..outside
    });
    let thickness_mm = 9.999_327_355_218_911;
    let mut project = case(Vec::new(), Transform::translate(0.0, 0.0), thickness_mm, op);
    let template = project.instances[0].clone();
    project.definitions.clear();
    project.instances.clear();
    let circles = [
        (
            "0",
            27.545_650_047_230_467,
            [-0.450_274_119_556_700_27, -0.892_890_372_474_380_7],
            (40.0, 40.0),
        ),
        (
            "5",
            12.422_956_671_942_869,
            [-0.869_530_608_207_535_9, -0.493_879_055_427_776_55],
            (200.0, 120.0),
        ),
        (
            "2",
            20.119_655_775_889_72,
            [-0.767_977_879_914_663_9, -0.640_476_366_434_998_2],
            (200.0, 40.0),
        ),
    ];
    for (n, r, [a, b], (x, y)) in circles {
        let definition = FeatureDefinition {
            id: DefinitionId::new(format!("d{n}")),
            name: format!("Cell {n}"),
            profiles: vec![Polyline::new(
                vec![Vertex::new(r, 0.0, 1.0), Vertex::new(-r, 0.0, 1.0)],
                true,
            )],
        };
        let mut instance = template.clone();
        instance.id = InstanceId::new(format!("i{n}"));
        instance.definition = definition.id.clone();
        instance.transform = Transform::from([a, b, -b, a, x, y]);
        project.instances.push(instance);
        project
            .definitions
            .insert(definition.id.clone(), definition);
    }
    project.origin = MachineOrigin {
        xy: OriginXy::TopRight,
        z: OriginZ::StockBottom,
    };
    project.arc_fit_tolerance_mm = Some(0.01);
    project.post_processor = PostProcessorRef::latest("grbl").expect("GRBL is bundled");
    let tool = &mut project.tools[0];
    tool.diameter_mm = 3.454_973_032_185_739_3;
    tool.feed_mm_min = Some(2_327.326_110_062_505);
    tool.plunge_mm_min = Some(2_862.833_030_368_147);
    tool.step_down_mm = Some(0.844_691_259_235_000_5);
    tool.step_over_fraction = Some(0.248_493_065_736_951_43);
    project
}

/// #355 (the Pocket property, `PROPTEST_RNG_SEED=33` and 22 at 512
/// cases): an Offset Pocket of a 24.96 × 37.53 mm rectangle round a
/// circular island of 4.64 mm radius, by a 1.273 mm tool, through 3.88 mm
/// stock with a 1.706 mm allowance, rotated 44°, written in inch on
/// LinuxCNC with arcs fitted at 0.01 mm. S12 refused a fitted arc round
/// the island (`ArcFitViolation` at line 134). The program exports, and
/// every arc it writes stays within the arc-fit tolerance (plus the
/// program's rounding) of the toolpath it stands for.
#[test]
fn offset_pocket_round_a_circle_island_exports() {
    let project = pocket_round_a_circle_island();
    exports(&project);
    arcs_follow_the_toolpath(&project);
}

/// Every `G2`/`G3` `project`'s program writes lies, along its whole
/// length, within the arc-fit tolerance plus the program's rounding of the
/// toolpath at its Z; and there is at least one.
fn arcs_follow_the_toolpath(project: &Project) {
    let tolerance_mm = project.arc_fit_tolerance_mm.expect("arc fitting is on");
    let op = generate(project).expect("generates");
    let path = toolpath_segments(&op.toolpath.moves);
    let (program, _) = emit(project, &[op], DATE).expect("exports");
    let post = project.post_processor.resolve().expect("a bundled post");
    let transform = MachineTransform::new(project, &post).expect("a machine transform");
    // One unit in the last written place, mm, each side of a written word.
    let rounding_mm = match project.display_units {
        Units::Inch => 25.4 * 0.0001,
        Units::Mm => 0.001,
    };
    let mut arcs = 0;
    for mv in parse_motion(&program.text).expect("parses") {
        let clockwise = match mv.kind {
            MotionKind::ArcCw => true,
            MotionKind::ArcCcw => false,
            _ => continue,
        };
        arcs += 1;
        let at = |x: f64, y: f64, z: f64| transform.to_project(Point3::new(x, y, z), mv.units);
        let (Some(sx), Some(sy), Some(sz), Some(ex), Some(ey)) =
            (mv.from.x, mv.from.y, mv.from.z, mv.to.x, mv.to.y)
        else {
            panic!("line {}: an arc from an unknown position", mv.line);
        };
        let (i, j) = (mv.arc.i.expect("I"), mv.arc.j.expect("J"));
        let start = at(sx, sy, sz);
        let end = at(ex, ey, sz).xy();
        let centre = at(sx + i, sy + j, sz).xy();
        let clockwise = clockwise != transform.mirrors_xy();
        let radius_mm = start.xy().distance_to(centre);
        let a0 = (start.y - centre.y).atan2(start.x - centre.x);
        let a1 = (end.y - centre.y).atan2(end.x - centre.x);
        let sweep = if clockwise { a0 - a1 } else { a1 - a0 }.rem_euclid(std::f64::consts::TAU);
        for k in 0..=64 {
            let t = a0 + if clockwise { -sweep } else { sweep } * f64::from(k) / 64.0;
            let p = Point::new(
                centre.x + radius_mm * t.cos(),
                centre.y + radius_mm * t.sin(),
            );
            let off = path
                .iter()
                .filter(|(a, _)| (a.z - start.z).abs() <= rounding_mm)
                .map(|(a, b)| dc_geometry::point_segment_distance_mm(segment(a.xy(), b.xy()), p))
                .fold(f64::INFINITY, f64::min);
            assert!(
                off <= tolerance_mm + 2.0 * rounding_mm,
                "line {}: the arc is {off} mm off the toolpath at {p:?}",
                mv.line
            );
        }
    }
    assert!(arcs > 0, "arcs are written");
}

/// A straight segment from `a` to `b`.
fn segment(a: Point, b: Point) -> dc_geometry::Segment {
    dc_geometry::Segment {
        start: a,
        end: b,
        bulge: 0.0,
    }
}

/// The cutting moves of `moves` as straight segments; a toolpath arc as
/// 64 chords, close enough to measure a fitted arc against.
fn toolpath_segments(moves: &[dc_cam::Move]) -> Vec<(Point3, Point3)> {
    let mut out = Vec::new();
    let mut from: Option<Point3> = None;
    for mv in moves {
        if let Some(a) = from.filter(|_| !mv.is_rapid()) {
            match mv.kind {
                MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                    let cw = matches!(mv.kind, MoveKind::ArcCw { .. });
                    let r = a.xy().distance_to(centre);
                    let a0 = (a.y - centre.y).atan2(a.x - centre.x);
                    let a1 = (mv.to.y - centre.y).atan2(mv.to.x - centre.x);
                    let sweep =
                        if cw { a0 - a1 } else { a1 - a0 }.rem_euclid(std::f64::consts::TAU);
                    let mut p = a;
                    for k in 1..=64 {
                        let t = a0 + if cw { -sweep } else { sweep } * f64::from(k) / 64.0;
                        let q = Point3::new(centre.x + r * t.cos(), centre.y + r * t.sin(), a.z);
                        out.push((p, q));
                        p = q;
                    }
                }
                _ => out.push((a, mv.to)),
            }
        }
        from = Some(mv.to);
    }
    out
}

/// #355's project, as the property shrank it.
fn pocket_round_a_circle_island() -> Project {
    let (hx, hy) = (12.481_040_697_088_298, 18.762_814_826_848_278);
    let r = 4.637_682_794_054_545_5;
    let profiles = vec![
        Polyline::rect(Point::new(-hx, -hy), 2.0 * hx, 2.0 * hy),
        Polyline::new(
            vec![Vertex::new(-r, 0.0, -1.0), Vertex::new(r, 0.0, -1.0)],
            true,
        ),
    ];
    let thickness_mm = 3.880_412_583_570_487_2;
    let op = PocketOp {
        strategy: ClearanceStrategy::Offset,
        wall_pass: WallPass::None,
        depth_mm: Some(thickness_mm),
        step_over_fraction: Some(0.888_442_811_577_295_3),
        direction: dc_project::CutDirection::Conventional,
        allowance_mm: 1.705_889_174_804_463_3,
        safe_z_mm: 13.378_563_052_393_277,
        ..sample_pocket()
    };
    let transform = Transform::from([
        0.718_973_428_179_367_9,
        0.695_037_559_828_249_3,
        -0.695_037_559_828_249_3,
        0.718_973_428_179_367_9,
        200.0,
        40.0,
    ]);
    let mut project = case(profiles, transform, thickness_mm, Operation::Pocket(op));
    project.display_units = Units::Inch;
    project.origin = MachineOrigin {
        xy: OriginXy::Custom {
            x_mm: 192.539_444_272_137_16,
            y_mm: 119.461_040_582_034_09,
        },
        z: OriginZ::StockTop,
    };
    project.arc_fit_tolerance_mm = Some(0.01);
    project.post_processor = PostProcessorRef::latest("linuxcnc").expect("LinuxCNC is bundled");
    let tool = &mut project.tools[0];
    tool.diameter_mm = 1.272_837_131_008_848_2;
    tool.feed_mm_min = Some(2_704.671_379_278_641);
    tool.plunge_mm_min = Some(630.335_261_894_897_7);
    tool.rpm = Some(7014);
    tool.step_down_mm = Some(2.778_420_178_868_245_7);
    tool.step_over_fraction = Some(0.501_838_500_139_350_6);
    project
}

/// The Pocket property's `2773e846` regression (#355): an Offset Pocket of
/// a 32.47 × 36.44 mm rectangle round a 7.36 × 7.76 mm rectangular island,
/// T-bone relief, ramp entry, pass first, by a 1.435 mm tool, in inch on
/// GRBL with arcs fitted at 0.01 mm. S12 refused a fitted arc
/// (`ArcFitViolation` at line 95).
#[test]
fn t_bone_pocket_round_a_rect_island_exports() {
    let project = t_bone_pocket_round_a_rect_island();
    exports(&project);
    arcs_follow_the_toolpath(&project);
}

/// The Profile property's `e577fed0` regression (#356): a 1 mm deep
/// Outside Profile of a 33.77 mm circle with a 6.85 × 7.47 mm rectangular
/// hole, ramped at 1.33°, by a 7.27 mm tool, in inch on grblHAL with arcs
/// fitted at 0.01 mm. The second pass's ramp ran 0.69 mm past the trench
/// the first cut below the stock top, about twice the step-down into
/// uncut stock. S9 refused it (`OffMaterial`) with arcs fitted, and let it
/// through with arc fitting off. It exports either way, each later ramp
/// kept to the part of the ramp above it that ran below the stock top.
#[test]
fn shallow_ramp_round_a_circle_with_a_small_hole_exports() {
    for arc_fit_tolerance_mm in [Some(0.01), None] {
        let mut project = shallow_ramp_round_a_circle_with_a_small_hole();
        project.arc_fit_tolerance_mm = arc_fit_tolerance_mm;
        exports(&project);
    }
}

/// A closed polyline through `points`, straight edges.
fn straight(points: [(f64, f64); 4]) -> Polyline {
    Polyline::new(
        points
            .into_iter()
            .map(|(x, y)| Vertex::new(x, y, 0.0))
            .collect(),
        true,
    )
}

fn t_bone_pocket_round_a_rect_island() -> Project {
    let (hx, hy) = (16.236_790_976_329_89, 18.221_003_176_121_297);
    let (ix, iy) = (3.678_388_676_087_469_6, 3.878_425_362_102_490_2);
    let profiles = vec![
        straight([(-hx, hy), (hx, hy), (hx, -hy), (-hx, -hy)]),
        straight([(-ix, iy), (ix, iy), (ix, -iy), (-ix, -iy)]),
    ];
    let thickness_mm = 23.547_125_498_912_198;
    let op = PocketOp {
        strategy: ClearanceStrategy::Offset,
        wall_pass: WallPass::Last,
        depth_mm: Some(19.076_362_317_223_907),
        step_down_mm: Some(0.767_061_048_153_407_7),
        step_over_fraction: Some(0.892_292_758_843_146_3),
        direction: dc_project::CutDirection::Conventional,
        allowance_mm: 0.322_996_617_915_775_2,
        entry: Entry::Ramp {
            angle_deg: 2.064_141_769_483_755_6,
        },
        corner_relief: CornerRelief::TBone,
        machining_order: dc_project::MachiningOrder::PassFirst,
        through_cut_allowance_mm: 0.765_251_409_586_919_1,
        safe_z_mm: 13.828_972_857_860_856,
        ..sample_pocket()
    };
    let transform = Transform::from([
        0.005_516_090_348_034_621_5,
        0.999_984_786_257_907_2,
        -0.999_984_786_257_907_2,
        0.005_516_090_348_034_621_5,
        40.0,
        40.0,
    ]);
    let mut project = case(profiles, transform, thickness_mm, Operation::Pocket(op));
    project.display_units = Units::Inch;
    project.origin = MachineOrigin {
        xy: OriginXy::TopLeft,
        z: OriginZ::StockTop,
    };
    project.arc_fit_tolerance_mm = Some(0.01);
    project.post_processor = PostProcessorRef::latest("grbl").expect("GRBL is bundled");
    let tool = &mut project.tools[0];
    tool.diameter_mm = 1.434_988_949_287_780_3;
    tool.feed_mm_min = Some(4_623.749_764_726_896);
    tool.plunge_mm_min = Some(573.194_943_425_836_5);
    tool.rpm = Some(18_861);
    tool.step_down_mm = Some(2.560_319_329_700_137_6);
    tool.step_over_fraction = Some(0.832_159_140_181_261_5);
    project
}

fn shallow_ramp_round_a_circle_with_a_small_hole() -> Project {
    let r = 16.883_035_482_589_705;
    let (hx, hy) = (3.422_910_926_112_843, 3.733_229_249_787_881);
    let profiles = vec![
        Polyline::new(
            vec![Vertex::new(r, 0.0, 1.0), Vertex::new(-r, 0.0, 1.0)],
            true,
        ),
        straight([(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)]),
    ];
    let Operation::Profile(op) = outside_profile(CornerRelief::None, 1.009_782_134_478_237_6)
    else {
        unreachable!("outside_profile is a Profile");
    };
    let op = ProfileOp {
        safe_z_mm: 1.0,
        entry: Entry::Ramp {
            angle_deg: 1.334_494_957_255_164,
        },
        ..op
    };
    let transform = Transform::from([
        0.737_167_684_917_281_9,
        0.675_709_852_165_628_2,
        -0.675_709_852_165_628_2,
        0.737_167_684_917_281_9,
        40.0,
        40.0,
    ]);
    let mut project = case(
        profiles,
        transform,
        20.195_642_689_564_75,
        Operation::Profile(op),
    );
    project.display_units = Units::Inch;
    project.origin = MachineOrigin {
        xy: OriginXy::TopRight,
        z: OriginZ::StockBottom,
    };
    project.arc_fit_tolerance_mm = Some(0.01);
    project.post_processor = PostProcessorRef::latest("grblhal").expect("grblHAL is bundled");
    let tool = &mut project.tools[0];
    tool.diameter_mm = 7.269_778_836_713_369;
    tool.feed_mm_min = Some(3_192.045_316_616_497);
    tool.plunge_mm_min = Some(2_633.305_020_598_756_5);
    tool.rpm = Some(15_272);
    tool.step_down_mm = Some(0.415_478_759_066_539_8);
    tool.step_over_fraction = Some(0.326_634_913_148_346_75);
    project
}

/// #358 case 1: a 0.29 mm deep Left Profile of an open path with two arcs,
/// beside three closed outlines it skips, by a 5.31 mm tool, plunged,
/// with a 0.09 mm last pass allowance and a 0.015 mm last pass, in mm on
/// GRBL. The cut starts at the path's drawn end, where both offsets stop
/// on circles round the end vertex but not on one normal, so the last
/// pass's 0.11 mm step (line 26) from the allowance path's start to its
/// own ends 0.114 mm from the allowance path, past S9's allowance band
/// (`OffMaterial`).
#[test]
#[ignore = "#358"]
fn left_open_path_last_pass_plunge_exports() {
    exports(&left_open_path_last_pass_plunge());
}

/// #358 case 2: a 19.09 mm deep Inside Profile of a 13.6 × 20.3 mm
/// rectangle with one 1.38 × 12.96 mm rect tab, by a 7.78 mm tool in
/// 1.44 mm steps, helixed, pass first, with a 1.00 mm last pass
/// allowance, in inch on UCCNC. S9's tab rule refused each allowance
/// pass below the tab top coming down at the end of its raised stretch
/// (line 2576 first, `OffMaterial`), as #353's. Its last pass also comes
/// down the tab's 10 mm height in one move at the tab's far end: #348,
/// still open, which S9 does not catch, so this test passing does not
/// prove #348 fixed.
#[test]
fn inside_last_pass_helix_tabs_pass_first_exports() {
    exports(&inside_last_pass_helix_tabs_pass_first());
}

/// #358's `201280a2` regression: the rectangle of case 2 cut Inside
/// through its 22.03 mm stock with one 1.0 × 1.10 mm rect tab, plunged,
/// full depth first, with a 2.24 mm last pass allowance. S9's tab rule
/// refused the allowance pass at Z 0.49 coming down at the end of its
/// raised stretch (line 90, `OffMaterial`), as #353's.
#[test]
fn inside_last_pass_plunge_short_tab_exports() {
    exports(&inside_last_pass_plunge_short_tab());
}

/// #353's `a7bfa9cd` regression, rebuilt from the issue (the seed
/// replays its unshrunk project, which exports, and the shrunk one was
/// not kept): a 28.9 mm circle cut Outside through 10.1 mm stock with a
/// 8.4 × 10 mm rectangular hole, rotated 30°, by a 2.1 mm tool in
/// 1.21 mm steps, plunged, with a 0.60 mm last pass allowance and a
/// 3.64 × 0.51 mm rect tab at 55.18 mm on each outline. S9 refused the
/// hole's allowance pass at Z 0.39 coming down at the end of its raised
/// stretch over the tab (line 53, `OffMaterial`).
#[test]
fn outside_last_pass_small_tab_on_a_hole_exports() {
    exports(&outside_last_pass_small_tab_on_a_hole());
}

/// The sample's Profile with `side`, cut `depth_mm` deep by `entry`, with
/// `last_pass` and `tabs`, its other settings the issue's.
fn last_pass_profile(
    side: Side,
    depth_mm: f64,
    entry: Entry,
    last_pass: LastPass,
    tabs: Option<TabSpec>,
) -> ProfileOp {
    let Operation::Profile(op) = sample_project().operations[0].clone() else {
        unreachable!("the sample's operation is a Profile");
    };
    ProfileOp {
        side,
        depth_mm: Some(depth_mm),
        step_down_mm: None,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 1.0,
        tabs,
        entry,
        lead: None,
        corner_relief: CornerRelief::None,
        last_pass: Some(last_pass),
        start: StartAt::Auto,
        ..op
    }
}

/// Sets `project`'s one tool: its diameter, feed, plunge rate, spindle
/// speed, step-down and step-over.
fn set_tool(
    project: &mut Project,
    diameter_mm: f64,
    (feed_mm_min, plunge_mm_min, rpm): (f64, f64, u32),
    step_down_mm: f64,
    step_over_fraction: f64,
) {
    let tool = &mut project.tools[0];
    tool.diameter_mm = diameter_mm;
    tool.feed_mm_min = Some(feed_mm_min);
    tool.plunge_mm_min = Some(plunge_mm_min);
    tool.rpm = Some(rpm);
    tool.step_down_mm = Some(step_down_mm);
    tool.step_over_fraction = Some(step_over_fraction);
}

fn left_open_path_last_pass_plunge() -> Project {
    let (ax, ay) = (12.735_741_384_150_293, 10.463_103_598_006_539);
    let (bx, by) = (6.161_396_678_209_638, 3.546_270_859_301_415);
    let (cx, cy) = (1.723_663_449_543_754_8, 0.986_903_899_408_676_6);
    let profiles = vec![
        straight([(-ax, ay), (ax, ay), (ax, -ay), (-ax, -ay)]),
        straight([(-bx, -by), (bx, -by), (bx, by), (-bx, by)]),
        straight([(-cx, -cy), (cx, -cy), (cx, cy), (-cx, cy)]),
        Polyline::new(
            vec![
                Vertex::new(
                    -7.707_296_639_115_788,
                    0.964_187_341_962_776_1,
                    -0.389_311_580_111_253_6,
                ),
                Vertex::new(-16.993_313_986_140_127, 8.340_273_308_754_115, 0.0),
                Vertex::new(3.632_375_064_370_213_7, -3.256_045_238_042_021, 0.0),
                Vertex::new(
                    -5.890_870_604_198_071,
                    -1.253_510_400_986_085,
                    -0.251_098_236_982_057_13,
                ),
            ],
            false,
        ),
    ];
    let op = ProfileOp {
        direction: CutDirection::Climb,
        machining_order: MachiningOrder::FullDepthFirst,
        start: StartAt::Near {
            x_mm: 0.0,
            y_mm: 196.569_406_351_940_44,
        },
        ..last_pass_profile(
            Side::Left,
            0.294_704_283_231_769_4,
            Entry::Plunge,
            LastPass {
                allowance_mm: 0.088_504_229_190_802_82,
                thickness_mm: Some(0.014_735_214_161_588_47),
                reverse: false,
            },
            None,
        )
    };
    let transform = Transform::from([
        -0.438_645_465_077_644_5,
        0.898_660_200_502_290_5,
        -0.898_660_200_502_290_5,
        -0.438_645_465_077_644_5,
        40.0,
        40.0,
    ]);
    let mut project = case(
        profiles,
        transform,
        5.894_085_664_635_388,
        Operation::Profile(op),
    );
    project.display_units = Units::Mm;
    project.origin = MachineOrigin {
        xy: OriginXy::BottomRight,
        z: OriginZ::StockBottom,
    };
    project.arc_fit_tolerance_mm = None;
    project.post_processor = PostProcessorRef::latest("grbl").expect("GRBL is bundled");
    set_tool(
        &mut project,
        5.310_253_751_448_169,
        (1_812.667_973_601_802_7, 2_653.082_271_466_902_5, 19_417),
        2.876_197_522_786_930_5,
        0.372_401_770_699_803_57,
    );
    project
}

/// The 13.6 × 20.3 mm rectangle of #358's Inside cases, rotated 0.31 rad,
/// at (`x_mm`, 40), on 22.03 mm stock, in inch on UCCNC, cut by `op` with
/// their 7.78 mm tool.
fn uccnc_rectangle(x_mm: f64, op: ProfileOp) -> Project {
    let (hx, hy) = (6.812_943_106_136_772, 10.150_614_061_311_213);
    let transform = Transform::from([
        0.952_230_104_477_401_2,
        -0.305_381_446_926_557_1,
        0.305_381_446_926_557_1,
        0.952_230_104_477_401_2,
        x_mm,
        40.0,
    ]);
    let mut project = case(
        vec![straight([(-hx, -hy), (hx, -hy), (hx, hy), (-hx, hy)])],
        transform,
        22.031_217_735_812_504,
        Operation::Profile(op),
    );
    project.display_units = Units::Inch;
    project.origin = MachineOrigin {
        xy: OriginXy::TopLeft,
        z: OriginZ::StockBottom,
    };
    project.arc_fit_tolerance_mm = None;
    project.post_processor = PostProcessorRef::latest("uccnc").expect("UCCNC is bundled");
    set_tool(
        &mut project,
        7.780_855_084_777_605,
        (2_523.738_573_213_584_5, 1_678.407_747_757_199_3, 10_350),
        1.436_020_526_849_626_2,
        0.988_663_914_969_774_9,
    );
    project
}

fn inside_last_pass_helix_tabs_pass_first() -> Project {
    let op = ProfileOp {
        direction: CutDirection::Climb,
        machining_order: MachiningOrder::PassFirst,
        ..last_pass_profile(
            Side::Inside,
            19.094_398_963_361_22,
            Entry::Helix {
                diameter_fraction: 0.225_713_859_585_626_45,
                angle_deg: 2.152_324_848_967_854,
            },
            LastPass {
                allowance_mm: 1.002_757_156_741_800_5,
                thickness_mm: None,
                reverse: false,
            },
            Some(TabSpec {
                placement: TabPlacement::AtDistances {
                    distances_mm: vec![163.917_868_619_434],
                },
                width_mm: 1.382_330_403_110_679_3,
                height_mm: 12.958_115_719_223_605,
                shape: TabShape::Rect,
                on_holes: false,
                avoid_corners: true,
            }),
        )
    };
    uccnc_rectangle(200.0, op)
}

fn inside_last_pass_plunge_short_tab() -> Project {
    let op = ProfileOp {
        direction: CutDirection::Conventional,
        machining_order: MachiningOrder::FullDepthFirst,
        ..last_pass_profile(
            Side::Inside,
            22.031_217_735_812_504,
            Entry::Plunge,
            LastPass {
                allowance_mm: 2.240_394_252_311_478_6,
                thickness_mm: None,
                reverse: false,
            },
            Some(TabSpec {
                placement: TabPlacement::Count { count: 1 },
                width_mm: 1.0,
                height_mm: 1.101_560_886_790_625_2,
                shape: TabShape::Rect,
                on_holes: false,
                avoid_corners: false,
            }),
        )
    };
    uccnc_rectangle(40.0, op)
}

fn outside_last_pass_small_tab_on_a_hole() -> Project {
    let r = 14.453_060_993_366_394;
    let (hx, hy) = (4.212_446_961_402_92, 4.982_479_348_614_591);
    let profiles = vec![
        Polyline::new(
            vec![Vertex::new(-r, 0.0, -1.0), Vertex::new(r, 0.0, -1.0)],
            true,
        ),
        straight([(-hx, hy), (hx, hy), (hx, -hy), (-hx, -hy)]),
    ];
    let thickness_mm = 10.102_031_327_283_026;
    let op = ProfileOp {
        direction: CutDirection::Climb,
        machining_order: MachiningOrder::FullDepthFirst,
        ..last_pass_profile(
            Side::Outside,
            thickness_mm,
            Entry::Plunge,
            LastPass {
                allowance_mm: 0.60,
                thickness_mm: None,
                reverse: false,
            },
            Some(TabSpec {
                placement: TabPlacement::AtDistances {
                    distances_mm: vec![55.18],
                },
                width_mm: 3.64,
                height_mm: 0.51,
                shape: TabShape::Rect,
                on_holes: true,
                avoid_corners: false,
            }),
        )
    };
    let (sin, cos) = 30_f64.to_radians().sin_cos();
    let mut project = case(
        profiles,
        Transform::from([cos, sin, -sin, cos, 40.0, 40.0]),
        thickness_mm,
        Operation::Profile(op),
    );
    project.display_units = Units::Mm;
    project.arc_fit_tolerance_mm = None;
    project.post_processor = PostProcessorRef::latest("grbl").expect("GRBL is bundled");
    set_tool(
        &mut project,
        2.103_135_519_226_597_2,
        (249.818_149_582_006_92, 529.759_124_549_315_1, 16_789),
        1.213_709_039_413_399_1,
        0.894_619_790_919_957_6,
    );
    project
}
