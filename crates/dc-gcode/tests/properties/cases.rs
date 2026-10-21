// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The minimal cases the properties shrank their failures to, as named
//! tests (#231, #232, #234, #235): each project validates, generates, and
//! exports through the full safety pass. Every case is built from
//! `dc_project::test_support::sample_project()` on 240 × 160 mm stock,
//! its one instance placed about (120, 80), as in the issue that reports
//! it.

use super::{DATE, generate};
use dc_cam::{Warning, WarningCode, generate_profile};
use dc_gcode::emit;
use dc_geometry::{Point, Polyline, Transform};
use dc_project::test_support::{sample_pocket, sample_project};
use dc_project::{
    ClearanceStrategy, CornerRelief, Entry, MachineOrigin, Operation, OriginXy, OriginZ, PocketOp,
    ProfileOp, Project, Side, Stock, WallPass, validate,
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
        let codes: Vec<WarningCode> = warnings.iter().map(|w| w.code).collect();
        assert_eq!(
            codes,
            [WarningCode::ToolpathsOverlap],
            "{angle_rad}: {warnings:?}"
        );
    }
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
