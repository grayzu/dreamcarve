// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A snapshot of what every [`Problem`] variant tells the user: its code,
//! severity, field path and message, for one sample of each branch of
//! each. `problem_snapshot.txt` holds the text; a change to it is a
//! change to what `check` prints and must be meant.

use super::super::*;
use crate::tool::{TBN_FLUTES, TBN_TAPER, TBN_TIP};

/// The committed snapshot.
const SNAPSHOT: &str = include_str!("problem_snapshot.txt");

/// Where the snapshot lives, for [`UPDATE`].
const SNAPSHOT_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/src/validate/tests/problem_snapshot.txt"
);

/// Set this environment variable to rewrite the snapshot from the code
/// instead of comparing against it.
const UPDATE: &str = "DC_PROJECT_UPDATE_PROBLEM_SNAPSHOT";

fn op() -> OperationId {
    OperationId::new("o1")
}

fn tool() -> ToolId {
    ToolId::new("t1")
}

fn bitmap() -> BitmapId {
    BitmapId::new("b1")
}

fn instance() -> InstanceId {
    InstanceId::new("i1")
}

/// One [`Problem::VBitConeExceeded`] with the three parts its message
/// branches on.
fn cone(from_stock_top: bool, field: &'static str, limit: CuttingLimit) -> Problem {
    Problem::VBitConeExceeded {
        operation: op(),
        name: "Lettering".into(),
        tool_name: "60° V-bit".into(),
        engagement_mm: 7.123_456,
        cone_height_mm: 5.499_26,
        from_stock_top,
        field,
        limit,
    }
}

/// At least one sample of every variant, and one of every branch of a
/// variant whose message or field depends on its values.
#[allow(clippy::too_many_lines, reason = "one literal per variant and branch")]
fn samples() -> Vec<Problem> {
    let name = || String::from("Cut out");
    let tool_name = || String::from("6.35 mm upcut");
    let mut all = vec![
        Problem::DefinitionKeyMismatch {
            key: DefinitionId::new("d1"),
            id: DefinitionId::new("d2"),
        },
        Problem::DuplicateInstanceId { id: instance() },
        Problem::DuplicateToolId { id: tool() },
        Problem::DuplicateOperationId { id: op() },
        Problem::DanglingDefinition {
            instance: instance(),
            definition: DefinitionId::new("nope"),
        },
        Problem::DanglingTool {
            operation: op(),
            tool: ToolId::new("nope"),
        },
        Problem::DanglingInstance {
            operation: op(),
            instance: InstanceId::new("nope"),
        },
        Problem::DuplicateTabId {
            id: TabId::new("tab1"),
        },
        Problem::TabDanglingInstance {
            tab: TabId::new("tab2"),
            instance: InstanceId::new("nope"),
        },
        Problem::TabProfileMissing {
            tab: TabId::new("tab3"),
            instance: instance(),
            profile: 3,
            count: 1,
        },
        Problem::OutOfRange {
            field: "tools[t1].diameter_mm".into(),
            value: -6.35,
            expected: POSITIVE,
        },
        Problem::OutOfRange {
            field: "operations[o1].step_over_fraction".into(),
            value: 1.5,
            expected: FRACTION,
        },
        Problem::OutOfRange {
            field: "stock.width_mm".into(),
            value: 6000.5,
            expected: STOCK_SIDE,
        },
        Problem::NonFinite {
            field: "stock.height_mm".into(),
        },
        Problem::OperationDisabled { operation: op() },
        Problem::UnusedTool {
            tool: tool(),
            name: tool_name(),
        },
        Problem::StockThicknessUnset,
        Problem::OperationValueUnset {
            operation: op(),
            name: name(),
            field: "depth_mm",
        },
        Problem::ToolOutOfRange {
            tool: tool(),
            name: tool_name(),
            field: "feed_mm_min",
            value: 25000.0,
            expected: RATE,
        },
        Problem::OpenProfile {
            definition: DefinitionId::new("d1"),
            name: "Scroll".into(),
            profile: 2,
        },
        Problem::ToolChangeModeNeedsM6 {
            post: "grbl".into(),
        },
        Problem::StepDownAboveDiameter {
            operation: op(),
            name: name(),
            tool_name: tool_name(),
            field: "operations[o1].step_down_mm".into(),
            step_down_mm: 8.5,
            diameter_mm: 6.35,
        },
        Problem::StepDownAboveDiameter {
            operation: op(),
            name: name(),
            tool_name: tool_name(),
            field: "tools[t1].step_down_mm".into(),
            step_down_mm: 8.5,
            diameter_mm: 6.35,
        },
        Problem::OperationHasNoTargets {
            operation: op(),
            name: name(),
        },
        Problem::SpindleSpeedHigh {
            tool: tool(),
            name: tool_name(),
            rpm: 180_000,
        },
        Problem::PlungeAboveFeed {
            tool: tool(),
            name: tool_name(),
            plunge_mm_min: 1200.5,
            feed_mm_min: 300.0,
        },
        Problem::LargeDiameter {
            tool: tool(),
            name: tool_name(),
            diameter_mm: 63.5,
        },
        Problem::DuplicateToolNumber {
            tool: ToolId::new("t2"),
            name: "3 mm drill".into(),
            number: 1,
            other: tool(),
        },
        Problem::RestOfInvalid {
            operation: OperationId::new("o2"),
            name: "Rest".into(),
            rest_of: op(),
        },
        Problem::HelixNeedsWasteSide {
            operation: op(),
            name: name(),
        },
        Problem::CannedCyclesUnsupported {
            post: "grbl".into(),
        },
        Problem::DrillCutterUnusual {
            operation: op(),
            name: "Holes".into(),
            tool: tool(),
            tool_name: "90° V-bit".into(),
            cutter: "a V-bit",
        },
        Problem::ReliefNeedsStepDown {
            operation: op(),
            name: name(),
            tool: tool(),
            tool_name: tool_name(),
        },
        Problem::ClearanceBelowSafeZ {
            operation: op(),
            name: name(),
            clearance_mm: 3.5,
            safe_z_mm: 5.0,
        },
        Problem::MachineZNotBelowHome { machine_z_mm: 0.0 },
        Problem::MachineZNotBelowHome {
            machine_z_mm: 2.125,
        },
        Problem::MachineZNeedsG53 {
            post: "grbl".into(),
        },
        Problem::EngraveDeeperThanCone {
            operation: op(),
            name: "Lettering".into(),
            tool_name: "90° V-bit".into(),
            depth_mm: 4.5,
            cone_height_mm: 3.175_49,
        },
        Problem::LastPassStockToLeave {
            operation: op(),
            name: name(),
            tool_name: tool_name(),
            stock_to_leave_mm: 2.5,
            diameter_mm: 6.35,
            limit_mm: 6.35 / 3.0,
        },
        Problem::LastPassThickerThanStepDown {
            operation: op(),
            name: name(),
            thickness_mm: 4.0,
            step_down_mm: 2.5,
            step_down_field: "tools[t1].step_down_mm".into(),
        },
        Problem::BorePitchOverStepDown {
            operation: op(),
            name: "Bore".into(),
            pitch_mm: 3.25,
            step_down_mm: 2.0,
        },
        Problem::FlattenToleranceRange {
            flatten_tolerance_mm: 0.25,
        },
        Problem::TaperedBallNoseRange {
            tool: tool(),
            name: "Tapered ball nose".into(),
            field: TBN_TIP,
            value: 0.0,
            expected: "a value greater than 0".into(),
        },
        Problem::TaperedBallNoseRange {
            tool: tool(),
            name: "Tapered ball nose".into(),
            field: TBN_FLUTES,
            value: 0.125,
            expected: "a flute length of at least 0.221 mm, where the ball ends".into(),
        },
        Problem::TaperedBallNoseRange {
            tool: tool(),
            name: "Tapered ball nose".into(),
            field: "diameter_mm",
            value: 0.5,
            expected: "a cutting diameter above the 0.5 mm tip".into(),
        },
        Problem::VBitAngleRange {
            tool: tool(),
            name: "V-bit".into(),
            included_angle_deg: 180.0,
        },
        Problem::ClearingNeedsFlatDepth {
            operation: op(),
            name: "Lettering".into(),
        },
        Problem::ClearingNeedsEndMill {
            operation: op(),
            name: "Lettering".into(),
            pass: 1,
            tool: ToolId::new("t3"),
            tool_name: "Ball".into(),
            cutter: "a ball nose",
        },
        Problem::ClearingDanglingTool {
            operation: op(),
            pass: 0,
            tool: ToolId::new("nope"),
        },
        Problem::ClearingNotDescending {
            operation: op(),
            name: "Lettering".into(),
            pass: 1,
            tool_name: tool_name(),
            diameter_mm: 6.35,
            previous_tool_name: "3.175 mm end mill".into(),
            previous_diameter_mm: 3.175,
        },
        Problem::StartDepthNegative {
            operation: op(),
            name: "Lettering".into(),
            start_depth_mm: -0.5,
            field: "start_depth_mm",
        },
        Problem::StartDepthNegative {
            operation: op(),
            name: "Holes".into(),
            start_depth_mm: -0.5,
            field: COUNTERSINK_START_DEPTH,
        },
        Problem::StartDepthUncleared {
            operation: op(),
            name: "Lettering".into(),
            start_depth_mm: 2.5,
            field: "start_depth_mm",
            footprint: StartFootprint::Targets,
        },
        Problem::StartDepthUncleared {
            operation: op(),
            name: "Holes".into(),
            start_depth_mm: 2.5,
            field: COUNTERSINK_START_DEPTH,
            footprint: StartFootprint::BitAtEachHole,
        },
        Problem::StartDepthUncleared {
            operation: op(),
            name: "Photo".into(),
            start_depth_mm: 2.5,
            field: "start_depth_mm",
            footprint: StartFootprint::Picture,
        },
        Problem::CountersinkNeedsVBit {
            operation: op(),
            name: "Holes".into(),
            tool: tool(),
            tool_name: "3 mm drill".into(),
            cutter: "a drill",
        },
        Problem::CountersinkDiameterPositive {
            operation: op(),
            name: "Holes".into(),
            diameter_mm: 0.0,
        },
        Problem::CountersinkMouthExceedsTool {
            operation: op(),
            name: "Holes".into(),
            tool_name: "90° V-bit".into(),
            diameter_mm: 14.5,
            tool_diameter_mm: 12.7,
        },
        Problem::CountersinkIgnoresToolWidth {
            operation: op(),
            name: "Holes".into(),
        },
        Problem::InlayGlueGapNotBelowDepth {
            operation: op(),
            name: "Star".into(),
            glue_gap_mm: 4.5,
            depth_mm: 4.0,
        },
        Problem::InlayMarginPositive {
            operation: op(),
            name: "Star".into(),
            margin_mm: -1.5,
        },
        Problem::InlayOutlineRounding {
            operation: op(),
            name: "Star".into(),
            rounding_mm: 0.0,
        },
        Problem::InlayPlugBaseThin {
            operation: op(),
            name: "Star".into(),
            floor_depth_mm: 5.0,
            thickness_mm: 9.5,
            base_mm: 4.5,
        },
        Problem::InlayGlueGapBelowBall {
            operation: op(),
            name: "Star".into(),
            glue_gap_mm: 0.25,
            tangent_height_mm: 0.441_234_5,
            lift_mm: 0.123_456,
            wall_gap_mm: 0.045_678,
        },
        Problem::InlayPlugDepthBelowBall {
            operation: op(),
            name: "Star".into(),
            plug_depth_mm: 0.3,
            tangent_height_mm: 0.441_234_5,
            wall_gap_mm: 0.045_678,
        },
        Problem::InlayMaleBoardThickness {
            operation: op(),
            name: "Star".into(),
            thickness_mm: -12.0,
        },
        Problem::InlayMaleBoardsDiffer {
            operation: OperationId::new("o2"),
            name: "Moon".into(),
            first: op(),
        },
        Problem::InlayBoundaryUnknown {
            operation: op(),
            name: "Star".into(),
            index: 1,
            instance: InstanceId::new("nope"),
        },
        Problem::InlayBoundaryOpen {
            operation: op(),
            name: "Star".into(),
            index: 0,
            instance: instance(),
            profile: 2,
        },
        Problem::InlayBoundaryIsTarget {
            operation: op(),
            name: "Star".into(),
            index: 0,
            instance: instance(),
        },
        Problem::InlayBoundaryCrossesDesign {
            operation: op(),
            name: "Star".into(),
            x_mm: 12.345_678,
            y_mm: -7.000_49,
            distance_mm: 0.0,
        },
        Problem::InlayBoundaryCrossesDesign {
            operation: op(),
            name: "Star".into(),
            x_mm: 80.0,
            y_mm: 50.5,
            distance_mm: 1.234_56,
        },
        Problem::InlayBoundaryEmpty {
            operation: op(),
            name: "Star".into(),
            index: 2,
            instance: instance(),
            instance_name: "Frame".into(),
        },
        Problem::InlayBoundaryHoleIgnored {
            operation: op(),
            name: "Star".into(),
            index: 0,
            instance: instance(),
            instance_name: "Frame".into(),
            profile: 1,
        },
        Problem::PhotoLineAngleRange {
            operation: op(),
            name: "Photo".into(),
            line_angle_deg: 180.0,
        },
        Problem::PhotoLineSpacingPositive {
            operation: op(),
            name: "Photo".into(),
            line_spacing_fraction: 0.0,
        },
        Problem::PhotoLinesOverlap {
            operation: op(),
            name: "Photo".into(),
            line_spacing_fraction: 0.75,
        },
        Problem::PhotoSamplePositive {
            operation: op(),
            name: "Photo".into(),
            sample_mm: -0.1,
        },
        Problem::PhotoRetractPositive {
            operation: op(),
            name: "Photo".into(),
            stepover_retract_mm: 0.0,
        },
        Problem::PhotoUnknownBitmap {
            operation: op(),
            name: "Photo".into(),
            bitmap: BitmapId::new("nope"),
        },
        Problem::BitmapFootprintAspect {
            bitmap: bitmap(),
            name: "Portrait".into(),
            width_mm: 100.0,
            height_mm: 60.0,
            width_px: 640,
            height_px: 480,
        },
        Problem::BitmapPixelsMismatch {
            bitmap: bitmap(),
            name: "Portrait".into(),
            width_px: 640,
            height_px: 480,
            png_width_px: 320,
            png_height_px: 240,
        },
        Problem::DuplicateBitmapId { id: bitmap() },
    ];
    for extension in ["dcv", "../nc", "n c\n", "tär", ""] {
        all.push(Problem::UnsafeFileExtension {
            extension: extension.into(),
        });
    }
    for field in [
        "feed_mm_min",
        "plunge_mm_min",
        "rpm",
        "step_down_mm",
        TBN_TIP,
        TBN_TAPER,
        TBN_FLUTES,
    ] {
        all.push(Problem::ToolValueUnset {
            tool: tool(),
            name: tool_name(),
            field,
        });
    }
    for field in ["tool", "cut_out.tool"] {
        all.push(Problem::CutterNotFlat {
            operation: op(),
            name: name(),
            tool: tool(),
            tool_name: "90° V-bit".into(),
            cutter: "a V-bit",
            field,
        });
    }
    for side in [
        Side::OnLine,
        Side::Left,
        Side::Right,
        Side::Inside,
        Side::Outside,
    ] {
        all.push(Problem::LastPassNeedsSide {
            operation: op(),
            name: name(),
            side,
        });
    }
    for (cutter, photo) in [
        ("a tapered ball nose", true),
        ("an end mill", true),
        ("a tapered ball nose", false),
        ("an end mill", false),
    ] {
        all.push(Problem::VCarveNeedsVBit {
            operation: op(),
            name: "Lettering".into(),
            tool: tool(),
            tool_name: tool_name(),
            cutter,
            photo,
        });
    }
    for from_stock_top in [false, true] {
        for field in [
            "flat_depth_mm",
            "start_depth_mm",
            "depth_mm",
            "max_depth_mm",
            INLAY_PLUG_DEPTH,
            COUNTERSINK_DIAMETER,
        ] {
            all.push(cone(from_stock_top, field, CuttingLimit::Cone));
        }
        all.push(cone(from_stock_top, "depth_mm", CuttingLimit::Flutes));
        all.push(cone(
            from_stock_top,
            INLAY_PLUG_DEPTH,
            CuttingLimit::CuttingDiameter {
                diameter_mm: 6.123_456,
            },
        ));
    }
    for at_boundary in [false, true] {
        all.push(Problem::InlayCutOutNoRoom {
            operation: op(),
            name: "Star".into(),
            tool_name: tool_name(),
            diameter_mm: 6.35,
            margin_mm: 7.123_456,
            needed_mm: 8.659_87,
            at_boundary,
        });
    }
    for field in ["margin_mm", "outline_rounding_mm"] {
        all.push(Problem::InlayMarginIgnored {
            operation: op(),
            name: "Star".into(),
            field,
        });
    }
    for field in ["width_mm", "height_mm"] {
        all.push(Problem::InlayMaleBoardSize {
            operation: op(),
            name: "Star".into(),
            field,
            value_mm: -100.0,
        });
        all.push(Problem::BitmapFootprintPositive {
            bitmap: bitmap(),
            name: "Portrait".into(),
            field,
            value: 0.0,
        });
    }
    for colour in [
        BitmapColour::Grayscale,
        BitmapColour::GrayscaleAlpha,
        BitmapColour::Rgb,
        BitmapColour::Rgba,
        BitmapColour::Indexed,
    ] {
        all.push(Problem::BitmapNotGrayscale8 {
            bitmap: bitmap(),
            name: "Portrait".into(),
            colour,
            bit_depth: 16,
        });
    }
    for (width_px, height_px) in [(0, 480), (640, 0), (0, 0)] {
        all.push(Problem::BitmapEmpty {
            bitmap: bitmap(),
            name: "Portrait".into(),
            width_px,
            height_px,
        });
    }
    for (width_px, height_px) in [(9000, 480), (640, 9000), (9000, 9000)] {
        all.push(Problem::BitmapTooLarge {
            bitmap: bitmap(),
            name: "Portrait".into(),
            width_px,
            height_px,
            limit_px: MAX_BITMAP_SIDE_PX,
        });
    }
    for reason in [
        BitmapError::Base64 {
            message: "Invalid symbol 33, offset 0.".into(),
        },
        BitmapError::Png {
            message: "unexpected end of file".into(),
        },
        BitmapError::Empty {
            width_px: 0,
            height_px: 4,
        },
        BitmapError::TooLarge {
            width_px: 9000,
            height_px: 4,
            limit_px: MAX_BITMAP_SIDE_PX,
        },
        BitmapError::SizeMismatch {
            width_px: 4,
            height_px: 4,
            png_width_px: 8,
            png_height_px: 2,
        },
        BitmapError::NotGrayscale8 {
            colour: BitmapColour::Rgba,
            bit_depth: 8,
        },
    ] {
        all.push(Problem::BitmapUnreadable {
            bitmap: bitmap(),
            name: "Portrait".into(),
            reason,
        });
    }
    // Whose passes, and of what width: a tool's, a Pocket's own, a
    // carve's floor (its own fraction and its tool's) and a Photo's.
    for (field, passes, fraction, width_mm) in [
        (
            "tools[t1].step_over_fraction",
            FinePasses::Tool {
                tool: tool(),
                name: tool_name(),
            },
            0.0008,
            6.35,
        ),
        (
            "operations[o1].step_over_fraction",
            FinePasses::Pocket {
                operation: op(),
                name: "Tray".into(),
                tool_name: tool_name(),
            },
            0.0008,
            6.35,
        ),
        (
            "operations[o1].step_over_fraction",
            FinePasses::Floor {
                operation: op(),
                name: "Lettering".into(),
                depth_mm: 2.0,
                from_tool: None,
            },
            0.000_004,
            2.309_401,
        ),
        (
            "tools[t1].step_over_fraction",
            FinePasses::Floor {
                operation: op(),
                name: "Lettering".into(),
                depth_mm: 0.5,
                from_tool: Some("60° V-bit".into()),
            },
            0.0096,
            0.577_35,
        ),
        (
            "operations[o1].line_spacing_fraction",
            FinePasses::Grooves {
                operation: op(),
                name: "Portrait".into(),
                max_depth_mm: 1.5,
            },
            0.005,
            3.0,
        ),
    ] {
        all.push(Problem::StepOverTooFine {
            field: field.into(),
            passes,
            fraction,
            width_mm,
        });
    }
    // Format 29: construction geometry and groups.
    for boundary in [false, true] {
        all.push(Problem::ConstructionTargeted {
            operation: op(),
            name: name(),
            instance: instance(),
            instance_name: "Centre line".into(),
            boundary,
        });
    }
    all.push(Problem::ShapeOutlineDiffers {
        definition: DefinitionId::new("d2"),
        name: "Slot 1".into(),
        kind: "slot",
    });
    all.push(Problem::DuplicateGroupId {
        id: GroupId::new("g1"),
    });
    all.push(Problem::DanglingGroup {
        instance: instance(),
        group: GroupId::new("nope"),
    });
    all
}

/// The snapshot text of `problems`: four labelled lines each, a blank
/// line between.
fn render(problems: &[Problem]) -> String {
    let mut text = String::new();
    for problem in problems {
        let field = problem.field().unwrap_or_else(|| "(none)".to_owned());
        text.push_str(&format!(
            "code: {}\nseverity: {:?}\nfield: {field}\nmessage: {problem}\n\n",
            problem.code(),
            problem.severity(),
        ));
    }
    text
}

/// What `check` prints for each problem is fixed: the code, severity,
/// field path and message of every sample are the committed snapshot's,
/// line for line, and the samples hold every code [`Problem`] has.
/// Guards the `Problem` impls against a rewrite that changes a message; a
/// change that is meant regenerates the file (see [`UPDATE`]).
#[test]
fn every_problem_reads_as_the_snapshot_says() {
    let samples = samples();
    let codes: BTreeSet<&str> = samples.iter().map(Problem::code).collect();
    assert_eq!(codes.len(), 92, "a variant has no sample: {codes:?}");
    let actual = render(&samples);
    if std::env::var_os(UPDATE).is_some() {
        std::fs::write(SNAPSHOT_PATH, &actual).unwrap();
        return;
    }
    let (actual_lines, expected_lines): (Vec<_>, Vec<_>) =
        (actual.lines().collect(), SNAPSHOT.lines().collect());
    for (number, (actual_line, expected_line)) in
        actual_lines.iter().zip(&expected_lines).enumerate()
    {
        assert_eq!(
            actual_line,
            expected_line,
            "line {} of problem_snapshot.txt differs",
            number + 1
        );
    }
    assert_eq!(
        actual_lines.len(),
        expected_lines.len(),
        "problem_snapshot.txt has a different number of lines"
    );
}

/// A variant added to [`Problem`] needs a sample here: every code its
/// `code` method is generated for is one a sample has, so the snapshot
/// cannot fall behind the enum.
#[test]
fn every_variant_has_a_sample_in_the_snapshot() {
    let sampled: BTreeSet<&str> = samples().iter().map(Problem::code).collect();
    for code in Problem::CODES {
        assert!(sampled.contains(code), "no sample of `{code}`");
    }
}
