// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of [`Warning`]: its messages, its codes and how it rounds the
//! numbers it shows.

use super::*;

/// `Shape` (instance `i1`, profile 2).
fn shape() -> ShapeRef {
    ShapeRef {
        instance: "i1".into(),
        instance_name: "Shape".to_owned(),
        profile: 2,
    }
}

/// Operation `o1`.
fn op() -> OperationId {
    "o1".into()
}

/// One warning for every message form, each with the message it printed
/// before warnings were typed. The values carry float noise so
/// the rounding is checked too.
#[allow(clippy::too_many_lines)] // one entry per message form
fn message_fixtures() -> Vec<(Warning, &'static str)> {
    vec![
        (
            Warning::TabOnHole {
                op: op(),
                tab: TabId::new("tab3"),
                shape: shape(),
            },
            "tab `tab3` is on `Shape` (instance `i1`, profile 2), which this operation cuts as a hole, so the tab holds the hole's slug in place; the slug is usually waste, so remove the tab (`dreamcarve tabs clear <dcv> --tab tab3`) unless you mean to keep the slug",
        ),
        (
            Warning::TabNotCrossed {
                tab: TabId::new("tab3"),
                shape: shape(),
            },
            "tab `tab3` on `Shape` (instance `i1`, profile 2) is crossed by no enabled Profile operation, so it holds nothing; cut that outline with a Profile, or remove the tab (`dreamcarve tabs clear <dcv> --tab tab3`)",
        ),
        (
            Warning::TabOffOutline {
                tab: TabId::new("tab3"),
                shape: shape(),
                off_mm: 20.000_000_000_4,
                limit_mm: 12.350_000_000_1,
            },
            "tab `tab3` on `Shape` (instance `i1`, profile 2) is 20 mm from that outline, more than its width plus the widest bit that cuts round it (12.35 mm), so it is cut where the outline is nearest it; move it back onto the outline, or check where it lands with `dreamcarve tabs list <dcv>`",
        ),
        (
            Warning::TabOnCorner {
                op: op(),
                shape: shape(),
                distance_mm: 54.600_002_2,
                reason: TabOnCornerReason::NoRoom,
            },
            "the tab 54.6 mm along the bit's path round `Shape` (instance `i1`, profile 2) is on a corner, where the path turns 30° or more within one bit diameter, and cannot slide off it without coming more than half way to the next tab, onto another tab or past the path's end, so it is cut on the corner; use fewer or narrower tabs, or place this one by hand (`dreamcarve tabs place --at`)",
        ),
        (
            Warning::TabOnCorner {
                op: op(),
                shape: shape(),
                distance_mm: 9.999_999_9,
                reason: TabOnCornerReason::Everywhere,
            },
            "the tab 10 mm along the bit's path round `Shape` (instance `i1`, profile 2) is on a corner, and so is every place it could go: the path turns 30° or more within one bit diameter all along it, as round a part small for the bit, so it is cut where it is; use a smaller bit, or place it by hand (`dreamcarve tabs place --at`)",
        ),
        (
            Warning::TabsCoverMostOfContour {
                op: op(),
                contour: 0,
                shape: Some(shape()),
                path_length_mm: 60.0,
                tab_count: 3,
                tab_width_mm: 6.0,
                diameter_mm: 6.35,
                tab_span_mm: 12.35,
                tab_total_mm: 37.05,
                cut_count: 3,
                max_width_mm: Some(3.65),
                placed: true,
            },
            "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: each of the 3 tabs is 6 mm wide, and with the 6.35 mm bit each one keeps 12.35 mm of the tool path raised, 37.05 mm in total on a 60 mm path (61.7%), so the part or hole would not cut free. All of them are cut, as placed, so the part or hole may need cutting free by hand. To let it cut free, make them at most 3.65 mm wide.",
        ),
        (
            Warning::OpenPathSkipped {
                shape: shape(),
                reason: OpenPathReason::NoSide,
            },
            "`Shape` (instance `i1`, profile 2) is an open path, which has no inside or outside, so it is not cut; an On line, Left or Right Profile or an Engrave cuts along it",
        ),
        (
            Warning::SideNeedsOpenPath { shape: shape() },
            "`Shape` (instance `i1`, profile 2) is a closed outline, and Left and Right are sides of an open path, so it is not cut; cut it Inside, Outside or On line",
        ),
        (
            Warning::OpenPathSkipped {
                shape: shape(),
                reason: OpenPathReason::NoLength,
            },
            "`Shape` (instance `i1`, profile 2) is an open path with no length, so there is nothing to cut; it is skipped",
        ),
        (
            Warning::ToolTooLarge {
                shape: shape(),
                tool: "1/4\" upcut".to_owned(),
                tool_id: "t1".into(),
                diameter_mm: 6.35,
            },
            "tool `1/4\" upcut` (6.35 mm) does not fit `Shape` (instance `i1`, profile 2), so that shape is not cut",
        ),
        (
            Warning::DuplicateShapeDropped {
                shape: shape(),
                duplicate_of: ShapeRef {
                    instance: "i0".into(),
                    instance_name: "Kept".to_owned(),
                    profile: 1,
                },
            },
            "`Shape` (instance `i1`, profile 2) is the same outline in the same place as instance `i0` profile 1, so it is cut only once",
        ),
        (
            Warning::TabsOverlap {
                op: op(),
                contour: 3,
                shape: Some(shape()),
                path_length_mm: 99.996_000_1,
            },
            "tabs on `Shape` (instance `i1`, profile 2) touch or overlap on its 100 mm tool path, so they were joined into longer tabs; use fewer or narrower tabs",
        ),
        (
            Warning::TabsOverlap {
                op: op(),
                contour: 3,
                shape: None,
                path_length_mm: 12.345,
            },
            "tabs on contour 4 in cut order touch or overlap on its 12.35 mm tool path, so they were joined into longer tabs; use fewer or narrower tabs",
        ),
        (
            Warning::TabsOutsideCutRange {
                op: op(),
                reason: TabsRangeReason::AboveStockTop,
                tab_top_mm: 20.000_000_01,
                deepest_pass_z_mm: 0.0,
                stock_top_mm: 18.0,
            },
            "the tab top is 20 mm above the stock bottom, higher than the 18 mm stock top; over each tab every pass lifts clear of the stock",
        ),
        (
            Warning::TabsOutsideCutRange {
                op: op(),
                reason: TabsRangeReason::CutStopsAboveTab,
                tab_top_mm: 3.0,
                deepest_pass_z_mm: 4.499_999,
                stock_top_mm: 18.0,
            },
            "the tab top is 3 mm above the stock bottom, but the deepest pass stops at 4.5 mm, so the tabs change nothing; lower the tabs or cut deeper",
        ),
        (
            Warning::TabsCoverMostOfContour {
                op: op(),
                contour: 0,
                shape: Some(shape()),
                path_length_mm: 100.0,
                tab_count: 4,
                tab_width_mm: 10.0,
                diameter_mm: 6.35,
                tab_span_mm: 16.35,
                tab_total_mm: 65.4,
                cut_count: 3,
                max_width_mm: Some(6.15),
                placed: false,
            },
            "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: each of the 4 tabs is 10 mm wide, and with the 6.35 mm bit each one keeps 16.35 mm of the tool path raised, 65.4 mm in total on a 100 mm path (65.4%), so the part or hole would not cut free. Only 3 of them are cut there. To keep all 4, make them at most 6.15 mm wide.",
        ),
        (
            Warning::TabsCoverMostOfContour {
                op: op(),
                contour: 1,
                shape: None,
                path_length_mm: 30.0,
                tab_count: 2,
                tab_width_mm: 5.0,
                diameter_mm: 8.0,
                tab_span_mm: 13.0,
                tab_total_mm: 26.0,
                cut_count: 1,
                max_width_mm: None,
                placed: false,
            },
            "tabs on contour 2 in cut order would cover more than half of its tool path: each of the 2 tabs is 5 mm wide, and with the 8 mm bit each one keeps 13 mm of the tool path raised, 26 mm in total on a 30 mm path (86.7%), so the part or hole would not cut free. Only 1 of them is cut there.",
        ),
        (
            Warning::TabsCoverMostOfContour {
                op: op(),
                contour: 0,
                shape: Some(shape()),
                path_length_mm: 10.0,
                tab_count: 1,
                tab_width_mm: 3.0,
                diameter_mm: 3.175,
                tab_span_mm: 6.175,
                tab_total_mm: 6.175,
                cut_count: 0,
                max_width_mm: None,
                placed: false,
            },
            "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: its one tab is 3 mm wide, and with the 3.175 mm bit it keeps 6.17 mm of the tool path raised on a 10 mm path (61.8%), so the part or hole would not cut free. No tab is cut there: this outline is too short for tabs with this bit.",
        ),
        (
            Warning::TabsCoverMostOfContour {
                op: op(),
                contour: 0,
                shape: Some(shape()),
                path_length_mm: 6.5,
                tab_count: 1,
                tab_width_mm: 3.0,
                diameter_mm: 1.5,
                tab_span_mm: 4.5,
                tab_total_mm: 4.5,
                cut_count: 0,
                max_width_mm: Some(1.75),
                placed: false,
            },
            "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: its one tab is 3 mm wide, and with the 1.5 mm bit it keeps 4.5 mm of the tool path raised on a 6.5 mm path (69.2%), so the part or hole would not cut free. No tab is cut there. To keep it, make it at most 1.75 mm wide.",
        ),
        (
            Warning::TabsCoverMostOfContour {
                op: op(),
                contour: 0,
                shape: Some(shape()),
                path_length_mm: 20.0,
                tab_count: 1,
                tab_width_mm: 8.0,
                diameter_mm: 6.35,
                tab_span_mm: 14.35,
                tab_total_mm: 14.35,
                cut_count: 1,
                max_width_mm: Some(3.65),
                placed: false,
            },
            "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: its one tab is 8 mm wide, and with the 6.35 mm bit it keeps 14.35 mm of the tool path raised on a 20 mm path (71.8%), so the part or hole would not cut free. Its one tab is kept, as a part keeps at least one, so the part may need cutting free by hand. To let it cut free, make it at most 3.65 mm wide.",
        ),
        (
            Warning::TabsCoverMostOfContour {
                op: op(),
                contour: 0,
                shape: Some(shape()),
                path_length_mm: 20.0,
                tab_count: 4,
                tab_width_mm: 8.0,
                diameter_mm: 6.35,
                tab_span_mm: 14.35,
                tab_total_mm: 57.4,
                cut_count: 1,
                max_width_mm: None,
                placed: false,
            },
            "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: each of the 4 tabs is 8 mm wide, and with the 6.35 mm bit each one keeps 14.35 mm of the tool path raised, 57.4 mm in total on a 20 mm path (287%), so the part or hole would not cut free. Only 1 of them is cut there, as a part keeps at least one, so the part may need cutting free by hand.",
        ),
        (
            Warning::ToolpathsOverlap {
                shape: shape(),
                other: ShapeRef {
                    instance: "i9".into(),
                    instance_name: "Other".to_owned(),
                    profile: 0,
                },
                gap_mm: 1.004_999,
                tool: "3 mm".to_owned(),
                tool_id: "t1".into(),
                diameter_mm: 3.0,
                needed_mm: 1.5,
            },
            "the tool paths of `Shape` (instance `i1`, profile 2) and `Other` (instance `i9`, profile 0) cut into each other: the shapes are 1 mm apart, closer than the 1.5 mm tool `3 mm` (3 mm) needs between them, so cutting one gouges the other. Move them at least 1.5 mm apart or use a smaller bit.",
        ),
        (
            Warning::PocketNoRoom {
                op: op(),
                shape: shape(),
                tool: "1/8\"".to_owned(),
                tool_id: "t1".into(),
                diameter_mm: 3.175,
            },
            "tool `1/8\"` (3.175 mm) is wider than the pocket of `Shape` (instance `i1`, profile 2) somewhere it must pass, so that shape is not cut; use a smaller bit",
        ),
        (
            Warning::PocketIslandTooClose {
                op: op(),
                shape: shape(),
                island: 3,
                other: IslandNeighbour::Wall,
                gap_mm: 4.999_9,
                needed_mm: 6.35,
                stock_to_leave_mm: 0.0,
                tool: "t".to_owned(),
                tool_id: "t1".into(),
                diameter_mm: 6.35,
            },
            "in the pocket of `Shape` (instance `i1`, profile 2), island profile 3 is 5 mm from the pocket wall, closer than the 6.35 mm tool `t` is wide, so the material between them is left uncut",
        ),
        (
            Warning::PocketIslandTooClose {
                op: op(),
                shape: shape(),
                island: 3,
                other: IslandNeighbour::Island(4),
                gap_mm: 7.0,
                needed_mm: 7.350_000_1,
                stock_to_leave_mm: 0.5,
                tool: "t".to_owned(),
                tool_id: "t1".into(),
                diameter_mm: 6.35,
            },
            "in the pocket of `Shape` (instance `i1`, profile 2), island profile 3 is 7 mm from island profile 4, closer than tool `t` needs with 0.5 mm stock to leave (7.35 mm), so the material between them is left uncut",
        ),
        (
            Warning::EntryFallback {
                op: op(),
                shape: shape(),
                requested: EntryKind::Helix,
                used: EntryKind::Ramp,
                reason: FallbackReason::NoRoom,
            },
            "there is no room for a `helix` entry in `Shape` (instance `i1`, profile 2), so its passes start with a zigzag ramp instead",
        ),
        (
            Warning::EntryFallback {
                op: op(),
                shape: shape(),
                requested: EntryKind::Ramp,
                used: EntryKind::Plunge,
                reason: FallbackReason::TooManyMoves,
            },
            "a `ramp` entry at this angle would take too many moves to reach depth in `Shape` (instance `i1`, profile 2), so its passes start with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning",
        ),
        (
            Warning::EntryFallback {
                op: op(),
                shape: shape(),
                requested: EntryKind::Helix,
                used: EntryKind::Ramp,
                reason: FallbackReason::TooSmall,
            },
            "a `helix` entry this small has moves too short for the program's decimal places in `Shape` (instance `i1`, profile 2), so its passes start with a zigzag ramp instead",
        ),
        (
            Warning::HelixDiameterReduced {
                op: op(),
                shape: shape(),
                requested_mm: 5.08,
                used_mm: 2.539_999_9,
            },
            "the helix entry in `Shape` (instance `i1`, profile 2) is 2.54 mm across, smaller than the 5.08 mm asked for, because a larger one does not fit there",
        ),
        (
            Warning::LeadShrunk {
                op: op(),
                shape: shape(),
                requested_mm: 30.0,
                used_mm: 22.499_999,
                requested_sweep_deg: 90.0,
                used_sweep_deg: 44.6,
            },
            "the lead arc on `Shape` (instance `i1`, profile 2) is 22.5 mm in radius over 45°, smaller than the 30 mm over 90° asked for, because a larger one does not fit there",
        ),
        (
            Warning::LeadDropped {
                op: op(),
                shape: Some(shape()),
                reason: LeadDroppedReason::NoRoom,
            },
            "there is no room for a lead-in arc on `Shape` (instance `i1`, profile 2), even at a quarter of the radius, so its passes start on the outline without one",
        ),
        (
            Warning::LeadDropped {
                op: op(),
                shape: Some(shape()),
                reason: LeadDroppedReason::Exit,
            },
            "there is no room for a lead-out arc where the cut on `Shape` (instance `i1`, profile 2) ends, so it stops on the outline",
        ),
        (
            Warning::LeadDropped {
                op: op(),
                shape: None,
                reason: LeadDroppedReason::OnLine,
            },
            "operation `o1` cuts on the line, which has no waste side for a lead, so it cuts without one; remove the lead or cut Inside or Outside",
        ),
        (
            Warning::LeadDropped {
                op: op(),
                shape: None,
                reason: LeadDroppedReason::OpenPath,
            },
            "operation `o1` cuts beside open paths, which take no lead, so it cuts without one; remove the lead",
        ),
        (
            Warning::ReliefSkipped {
                op: op(),
                requested: CornerRelief::Dogbone,
                reason: ReliefSkippedReason::NoStepDown,
            },
            "corner relief `dogbone` is skipped because the tool has no step_down_mm, which the relief pass steps down at, so no corners were relieved; set the tool's step-down",
        ),
        (
            Warning::ReliefSkipped {
                op: op(),
                requested: CornerRelief::TBone,
                reason: ReliefSkippedReason::OnLine,
            },
            "corner relief `t_bone` is skipped: an On line Profile has no waste side to cut a notch into, so no corners were relieved",
        ),
        (
            Warning::ReliefSkipped {
                op: op(),
                requested: CornerRelief::Dogbone,
                reason: ReliefSkippedReason::OpenPath,
            },
            "corner relief `dogbone` is skipped: a Left or Right Profile cuts open paths, whose corners have no inside or outside to relieve, so no corners were relieved",
        ),
        (
            Warning::ReliefCornerTooTight {
                op: op(),
                shape: shape(),
                x_mm: 10.004,
                y_mm: -0.001,
                reason: ReliefCornerReason::ShortEdge,
            },
            "the corner of `Shape` (instance `i1`, profile 2) at (10, 0) mm is not relieved: an edge beside it is shorter than the bit is wide, so it keeps the bit's radius",
        ),
        (
            Warning::ReliefCornerTooTight {
                op: op(),
                shape: shape(),
                x_mm: 1.5,
                y_mm: 2.25,
                reason: ReliefCornerReason::NoWallPath,
            },
            "the corner of `Shape` (instance `i1`, profile 2) at (1.5, 2.25) mm is not relieved: the tool path does not turn it within reach, so it keeps the bit's radius",
        ),
        (
            Warning::ReliefCornerTooTight {
                op: op(),
                shape: shape(),
                x_mm: 4.617,
                y_mm: 4.195,
                reason: ReliefCornerReason::NearOtherWall,
            },
            "the corner of `Shape` (instance `i1`, profile 2) at (4.62, 4.2) mm is not relieved: its notch would come nearer another wall than the bit's radius and cut through the material there, so it keeps the bit's radius",
        ),
        (
            Warning::ReliefCornerNotCut {
                op: op(),
                shape: shape(),
                x_mm: -3.0,
                y_mm: 40.125,
            },
            "the corner of `Shape` (instance `i1`, profile 2) at (-3, 40.12) mm is not relieved because the main passes did not cut to full depth where its notch starts (a tab over it, or no wall pass), and a notch must not plunge into uncut material; it keeps the bit's radius",
        ),
        (
            Warning::LastPassUnused { op: op() },
            "it has a last_pass, but its cut is only one pass deep, so that pass is the last and no pass leaves stock; it cuts as with last_pass null",
        ),
        (
            Warning::LastPassEdgeFaster {
                op: op(),
                shape: shape(),
                radius_mm: 0.325,
                stock_to_leave_mm: 0.5,
                feed_mm_min: 136.054_421_768_707_5,
                edge_ratio: 1.465_201_465,
            },
            "its last pass round `Shape` (instance `i1`, profile 2) turns on an inside arc of 0.325 mm, tighter than the 0.5 mm stock to leave, where it runs at 136.05 mm/min, the least the safety pass allows, so its cutting edge there moves at 1.47 times the feed; use a smaller bit, larger inside radii or a smaller stock to leave",
        ),
        (
            Warning::LastPassFlattenedCurve {
                op: op(),
                shape: shape(),
                radius_mm: 1.234_5,
                x_mm: 25.5,
                y_mm: 20.0,
                edge_ratio: 3.571_9,
            },
            "its last pass round `Shape` (instance `i1`, profile 2) follows a curve drawn as straight segments, which it does not slow down for, and near (25.5, 20) mm its tool path bends on a radius of about 1.23 mm, so its cutting edge there moves at about 3.6 times the feed; lower the feed, use a smaller stock to leave or redraw the curve with arcs",
        ),
        (
            Warning::EngravePathDegenerate {
                op: op(),
                shape: shape(),
            },
            "`Shape` (instance `i1`, profile 2) has fewer than two distinct points, so there is nothing to engrave; it is skipped",
        ),
        (
            Warning::CutInsideFreedPart {
                op: op(),
                earlier_op: "o0".into(),
                shape: shape(),
                freed: ShapeRef {
                    instance: "i0".into(),
                    instance_name: "Outline".to_owned(),
                    profile: 0,
                },
            },
            "`Shape` (instance `i1`, profile 2) lies inside `Outline` (instance `i0`, profile 0), which operation `o0` cuts free of the stock first, so this cut would run in a loose part; move this operation before `o0`, or hold that part with tabs",
        ),
        (
            Warning::CutInsideTabHeldPart {
                op: op(),
                earlier_op: "o0".into(),
                shape: shape(),
                held: ShapeRef {
                    instance: "i0".into(),
                    instance_name: "Outline".to_owned(),
                    profile: 0,
                },
            },
            "`Shape` (instance `i1`, profile 2) lies inside `Outline` (instance `i0`, profile 0), which operation `o0` cuts through the stock first, leaving it held only by its tabs, so this cut would run in a part that can flex, chatter or break a tab; move this operation before `o0`, or, for a hole, cut it in the same operation as its part, which cuts holes first",
        ),
        (
            Warning::ToolChangeAvoidable {
                tool_id: "t1".into(),
                tool: "6.35 mm upcut".to_owned(),
                ops: vec!["o1".into(), "o3".into()],
            },
            "tool `t1` (`6.35 mm upcut`) is used by operations `o1`, `o3`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)",
        ),
        (
            Warning::DrillTargetNotCircle {
                op: op(),
                shape: shape(),
            },
            "`Shape` (instance `i1`, profile 2) is not a circle; Drill plunges only at the centre of circles, so it is not drilled",
        ),
        (
            Warning::DrillHoleTooSmall {
                op: op(),
                shape: shape(),
                hole_mm: 4.999_999_9,
                tool_mm: 6.35,
            },
            "the 5 mm hole `Shape` (instance `i1`, profile 2) is narrower than the 6.35 mm tool, so it is not drilled: a Drill never moves sideways, so the hole would come out the tool's size; cut it to size with a Bore and an end mill smaller than the hole (a Bore cuts holes bigger than its bit), drill it with a smaller drill, or set allow_tool_wider_than_hole to drill it at the tool's size",
        ),
        (
            Warning::DrillToolWiderThanHole {
                op: op(),
                shape: shape(),
                hole_mm: 4.999_999_9,
                tool_mm: 6.35,
            },
            "the 6.35 mm tool is wider than the 5 mm hole `Shape` (instance `i1`, profile 2), so the hole comes out the tool's size",
        ),
        (
            Warning::DrillPeckDeeperThanHole {
                op: op(),
                peck_mm: 20.0,
                depth_mm: 12.000_000_1,
            },
            "the 20 mm peck is at least the 12 mm depth of the holes, so each is drilled in one plunge",
        ),
        (
            Warning::DrillRetractBelowStockTop {
                op: op(),
                requested_mm: -2.5,
                used_mm: 0.0,
            },
            "retract_mm is -2.5 mm, below the stock top; the tool retracts to the stock top instead, because it must never rapid into material",
        ),
        (
            Warning::DrillNoHoles { op: op() },
            "none of its targets is a circle it can drill, so it drills nothing",
        ),
        (
            Warning::CountersinkMouthNotWiderThanHole {
                op: op(),
                shape: shape(),
                mouth_mm: 2.999_999_9,
                hole_mm: 3.175,
            },
            "the 3.175 mm hole `Shape` (instance `i1`, profile 2) is at least as wide as the 3 mm countersink, so there is no seat to cut and it is not countersunk; use a wider diameter_mm",
        ),
        (
            Warning::BoreTargetNotCircle {
                op: op(),
                shape: shape(),
            },
            "`Shape` (instance `i1`, profile 2) is not a circle; Bore cuts only circles, so it is not bored",
        ),
        (
            Warning::BoreNoRoom {
                op: op(),
                shape: shape(),
                hole_mm: 3.0,
                tool_mm: 3.175,
                stock_to_leave_mm: 0.0,
                drill_fits: false,
            },
            "the 3 mm hole `Shape` (instance `i1`, profile 2) leaves the 3.175 mm tool no room: the tool does not fit, so it is not bored; use a smaller end mill",
        ),
        (
            Warning::BoreNoRoom {
                op: op(),
                shape: shape(),
                hole_mm: 3.175,
                tool_mm: 3.175,
                stock_to_leave_mm: 0.02,
                drill_fits: true,
            },
            "the 3.175 mm hole `Shape` (instance `i1`, profile 2) leaves the 3.175 mm tool and its 0.02 mm stock to leave no room to spiral, so it is not bored; a hole the bit's size is a Drill's: drill it with a Drill",
        ),
        (
            Warning::BoreLeavesCore {
                op: op(),
                shape: shape(),
                hole_mm: 10.0,
                tool_mm: 3.175,
                stock_to_leave_mm: 0.0,
                core_mm: 3.65,
            },
            "the 10 mm hole `Shape` (instance `i1`, profile 2) is more than twice the 3.175 mm tool: a helix along its wall would leave a 3.65 mm core standing, which comes loose in a through hole, so it is not bored; clear it with a Pocket, or use a bigger end mill",
        ),
        (
            Warning::BoreNoHoles { op: op() },
            "none of its targets is a circle it can bore, so it cuts nothing",
        ),
        (
            Warning::RestOfClearsNothing {
                op: "o2".into(),
                rest_of: "o1".into(),
                tool: "12.7 mm end mill".to_owned(),
                tool_id: "t2".into(),
            },
            "operation `o1`, which this one rests on, already reaches everything tool `12.7 mm end mill` can, so this operation cuts nothing; use a smaller tool, or remove it",
        ),
        (
            Warning::VCarveTargetOpen {
                op: op(),
                shape: shape(),
            },
            "`Shape` (instance `i1`, profile 2) is an open path, which has no area to V-carve; it is skipped",
        ),
        (
            Warning::VCarveNoMedialAxis {
                op: op(),
                shape: shape(),
            },
            "`Shape` (instance `i1`, profile 2) is too thin for a medial line to be found, so it is not carved",
        ),
        (
            Warning::VCarveSamplingBudget {
                op: op(),
                resolution_mm: 0.125_000_000_1,
            },
            "a carved shape is so long for its width that its medial lines are found every 0.125 mm, coarser than its width alone would give; small details may be smoothed",
        ),
        (
            Warning::StartDepthMismatch {
                op: op(),
                cleared_by: "o2".into(),
                floor: FloorKind::Pocket,
                footprint: StartFootprint::Targets,
                start_depth_mm: 4.000_000_1,
                floor_depth_mm: Some(3.0),
            },
            "it starts 4 mm below the stock top, but operation `o2`, the Pocket before it whose area contains its targets, is 3 mm deep, so the start plane is not that Pocket's floor; set start_depth_mm to the Pocket's depth",
        ),
        (
            Warning::StartDepthMismatch {
                op: op(),
                cleared_by: "o2".into(),
                floor: FloorKind::Pocket,
                footprint: StartFootprint::BitAtEachHole,
                start_depth_mm: 0.0,
                floor_depth_mm: None,
            },
            "it starts 0 mm below the stock top, but operation `o2`, the Pocket before it whose area contains the whole of its V-bit at each hole, has no depth set, so the start plane is not that Pocket's floor; set start_depth_mm to the Pocket's depth",
        ),
        (
            Warning::StartDepthMismatch {
                op: op(),
                cleared_by: "o2".into(),
                floor: FloorKind::Pocket,
                footprint: StartFootprint::Picture,
                start_depth_mm: 2.0,
                floor_depth_mm: Some(1.5),
            },
            "it starts 2 mm below the stock top, but operation `o2`, the Pocket before it whose area contains its picture's footprint, is 1.5 mm deep, so the start plane is not that Pocket's floor; set start_depth_mm to the Pocket's depth",
        ),
        (
            Warning::StartDepthMismatch {
                op: op(),
                cleared_by: "o6".into(),
                floor: FloorKind::VCarve,
                footprint: StartFootprint::Targets,
                start_depth_mm: 2.0,
                floor_depth_mm: Some(2.5),
            },
            "it starts 2 mm below the stock top, but operation `o6`, the V-Carve before it whose flat floor contains its targets, is 2.5 mm deep, so the start plane is not that floor; set start_depth_mm to the floor's depth",
        ),
        (
            Warning::ClearingNoRoom {
                op: op(),
                tool: "6.35 mm end mill".to_owned(),
                tool_id: "t1".into(),
                diameter_mm: 6.35,
            },
            "its clearing pass with tool `6.35 mm end mill` (6.35 mm) can reach none of the floor the passes before it left, so it cuts nothing; remove it, or use a narrower end mill than the pass before",
        ),
        (
            Warning::ClearingNotDescending {
                op: op(),
                tool: "t3".into(),
                diameter_mm: 3.175,
                earlier_tool: "t4".into(),
                earlier_diameter_mm: 1.5,
            },
            "its clearing pass with tool `t3` (3.175 mm) is wider than the pass before it (`t4`, 1.5 mm); it still runs, but list the largest tool first so each pass clears only what the one before could not reach",
        ),
        (
            Warning::InlayMaleMirrored {
                op: op(),
                axis_x_mm: 50.000_000_000_01,
            },
            "it is a male inlay, so its design is cut mirrored about X = 50 mm, the middle of its targets, to seat face down in the female",
        ),
        (
            Warning::InlayPlugBaseThin {
                op: op(),
                base_mm: 5.000_000_001,
            },
            "it leaves 5 mm of board under its plug's surround, less than 6 mm; the plug may flex or crack when pressed into the glue",
        ),
        (
            Warning::InlayBandEdgeVWall {
                op: op(),
                margin_needed_mm: 4.330_127_018_922_193,
            },
            "none of its clearing passes can cut the band's outer edge upright (no end mill fits the band's flat floor, or the V-bit's flank would reach past the edge), so the V-bit cuts it as a sloped wall; raise margin_mm to at least 4.33 mm for the first clearing pass that fits to cut it",
        ),
        (
            Warning::InlayBandClipped {
                op: op(),
                edges: vec![StockEdge::MinX, StockEdge::MaxX, StockEdge::MaxY],
                open: true,
                narrowest_mm: 9.999_999_999,
            },
            "its band runs past the stock's -X (left), +X (right) and +Y (back) edges, so it is clipped to the board there and is 10 mm wide at its narrowest; the board's edge is the plug's rim there, and a cut-out stops at it",
        ),
        (
            Warning::InlayBandClipped {
                op: op(),
                edges: vec![StockEdge::MinY],
                open: false,
                narrowest_mm: 2.5,
            },
            "its band runs past the stock's -Y (front) edge, so its V wall is cut the V-bit's radius inside the board there, the band 2.5 mm wide at its narrowest",
        ),
        (
            Warning::InlayPartsMerged {
                op: op(),
                first: shape(),
                second: ShapeRef {
                    profile: 3,
                    ..shape()
                },
                x_mm: 50.000_4,
                y_mm: 55.0,
                gap_mm: 0.001,
                merge_mm: 0.05,
            },
            "`Shape` (instance `i1`, profile 2) and `Shape` (instance `i1`, profile 3) come 0.001 mm apart at (50, 55) mm, closer than the 0.05 mm an inlay keeps between two parts, so both sides cut them as one shape, with no wall between them; draw them at least 0.05 mm apart to keep them separate",
        ),
        (
            Warning::InlayPartsMerged {
                op: op(),
                first: shape(),
                second: shape(),
                x_mm: 50.0,
                y_mm: 55.0,
                gap_mm: 0.02,
                merge_mm: 0.05,
            },
            "`Shape` (instance `i1`, profile 2) comes within 0.02 mm of itself at (50, 55) mm, closer than the 0.05 mm an inlay keeps between two parts, so both sides close the gap there, with no wall in it; draw it at least 0.05 mm open to keep it",
        ),
        (
            Warning::InlayHoleFilled {
                op: op(),
                hole: shape(),
                x_mm: 50.0,
                y_mm: 55.000_4,
                width_mm: 0.004,
                merge_mm: 0.05,
            },
            "`Shape` (instance `i1`, profile 2) is a hole about 0.004 mm wide at (50, 55) mm, narrower everywhere than the 0.05 mm an inlay keeps between two parts, so both sides cut it as part of the shape round it, with no wall there; draw it at least 0.05 mm wide to keep it",
        ),
        (
            Warning::InlayMarginIgnored {
                op: op(),
                field: "margin_mm",
            },
            "it draws its male's edge with a boundary, so margin_mm does nothing; the boundary replaces both margin_mm and outline_rounding_mm. Set margin_mm to null, or empty the boundary list to use it",
        ),
        (
            Warning::InlayBoundaryEmpty {
                op: op(),
                index: 1,
                instance: "b2".into(),
                instance_name: "Oval".to_owned(),
            },
            "it lists boundary[1] `b2` (`Oval`), but no part of the design is inside it; it still widens the band, so remove it if you did not mean that",
        ),
        (
            Warning::InlayBoundaryHoleIgnored {
                op: op(),
                index: 0,
                instance: "b1".into(),
                instance_name: "Frame".to_owned(),
                profile: 2,
            },
            "it lists boundary[0] `b1` (`Frame`), whose profile 2 is a hole; the band ignores holes, since board left standing in it would hold the plug off the female, so the hole is cut as band too",
        ),
        (
            Warning::InlayMaleBoardUnset { op: op() },
            "its male board has no thickness set, so only the female is cut; measure the male board and set male_board.thickness_mm to cut the male too",
        ),
        (
            Warning::PhotoLinesOverlap {
                op: op(),
                fraction: 0.9,
            },
            "its line_spacing_fraction 0.9 is less than 1, so its grooves overlap where the picture is darkest and no ridge is left between them; use 1 or more to keep the ridges",
        ),
        (
            Warning::TaperedCornersSoftened {
                op: op(),
                count: 4,
                x_mm: 20.0,
                y_mm: 40.0,
                change_mm: 0.156_6,
                radius_mm: 0.378_4,
            },
            "its tapered ball nose cannot follow 4 corners sharper than its tip below the surface, so their walls meet in a round valley instead of a crease, up to 0.157 mm from the corner as drawn at (20, 40); round the corners by at least 0.378 mm to cut them as the taper would",
        ),
        (
            Warning::InlayRoundedForBit {
                op: op(),
                kind: CornerKind::Point,
                x_mm: 80.0,
                y_mm: 50.0,
                change_mm: 1.260_5,
                radius_mm: 0.730_8,
            },
            "its tapered ball nose cannot cut a point or a part narrower than its tip and taper allow, so both boards round the design's points to a radius of 0.731 mm; the outline moves up to 1.26 mm, at (80, 50). Round the drawing yourself, carve less deep, or use a finer tip to keep it",
        ),
        (
            Warning::InlayRoundedForBit {
                op: op(),
                kind: CornerKind::Notch,
                x_mm: 58.5,
                y_mm: 58.5,
                change_mm: 0.161,
                radius_mm: 0.378_4,
            },
            "its tapered ball nose cannot cut an inward corner or a gap tighter than its tip, so both boards round the design's notches to a radius of 0.378 mm; the outline moves up to 0.161 mm, at (58.5, 58.5). Round the drawing yourself, or use a finer tip to keep it",
        ),
        (
            Warning::InlayGlueGapBelowBall {
                op: op(),
                glue_gap_mm: 0.25,
                tangent_height_mm: 1.009_6,
                lift_mm: 0.432,
                wall_gap_mm: 0.050_8,
            },
            "its glue_gap_mm 0.25 mm is below its tapered ball nose's tangent height 1.01 mm, so the plug's top edge rides on the pocket's rounded floor: it stands about 0.432 mm proud with its walls 0.051 mm open at the surface; set glue_gap_mm to at least 1.01 mm and test-fit a coupon first",
        ),
        (
            Warning::InlayPlugDepthBelowBall {
                op: op(),
                plug_depth_mm: 0.5,
                tangent_height_mm: 1.009_6,
                wall_gap_mm: 0.130_3,
            },
            "its plug_depth_mm 0.5 mm is below its tapered ball nose's tangent height 1.01 mm, so the plug's wall just below the surface is cut by the ball, up to 0.13 mm narrower than the pocket; set plug_depth_mm to at least 1.01 mm",
        ),
    ]
}

#[test]
fn warning_messages_are_unchanged() {
    for (warning, message) in message_fixtures() {
        assert_eq!(warning.to_string(), message, "{warning:?}");
    }
}

/// The code after `code`, in declaration order. A match, so a new code
/// does not compile until it is listed here.
const fn next_code(code: WarningCode) -> Option<WarningCode> {
    use WarningCode as C;
    Some(match code {
        C::OpenPathSkipped => C::SideNeedsOpenPath,
        C::SideNeedsOpenPath => C::ToolTooLarge,
        C::ToolTooLarge => C::DuplicateShapeDropped,
        C::DuplicateShapeDropped => C::TabsOverlap,
        C::TabsOverlap => C::TabsOutsideCutRange,
        C::TabsOutsideCutRange => C::TabsCoverMostOfContour,
        C::TabsCoverMostOfContour => C::TabOnHole,
        C::TabOnHole => C::TabNotCrossed,
        C::TabNotCrossed => C::TabOffOutline,
        C::TabOffOutline => C::TabOnCorner,
        C::TabOnCorner => C::ToolpathsOverlap,
        C::ToolpathsOverlap => C::PocketNoRoom,
        C::PocketNoRoom => C::PocketIslandTooClose,
        C::PocketIslandTooClose => C::EntryFallback,
        C::EntryFallback => C::HelixDiameterReduced,
        C::HelixDiameterReduced => C::LeadShrunk,
        C::LeadShrunk => C::LeadDropped,
        C::LeadDropped => C::ReliefSkipped,
        C::ReliefSkipped => C::ReliefCornerTooTight,
        C::ReliefCornerTooTight => C::ReliefCornerNotCut,
        C::ReliefCornerNotCut => C::LastPassUnused,
        C::LastPassUnused => C::LastPassEdgeFaster,
        C::LastPassEdgeFaster => C::LastPassFlattenedCurve,
        C::LastPassFlattenedCurve => C::EngravePathDegenerate,
        C::EngravePathDegenerate => C::CutInsideFreedPart,
        C::CutInsideFreedPart => C::CutInsideTabHeldPart,
        C::CutInsideTabHeldPart => C::ToolChangeAvoidable,
        C::ToolChangeAvoidable => C::DrillTargetNotCircle,
        C::DrillTargetNotCircle => C::DrillHoleTooSmall,
        C::DrillHoleTooSmall => C::DrillToolWiderThanHole,
        C::DrillToolWiderThanHole => C::DrillPeckDeeperThanHole,
        C::DrillPeckDeeperThanHole => C::DrillRetractBelowStockTop,
        C::DrillRetractBelowStockTop => C::DrillNoHoles,
        C::DrillNoHoles => C::CountersinkMouthNotWiderThanHole,
        C::CountersinkMouthNotWiderThanHole => C::BoreTargetNotCircle,
        C::BoreTargetNotCircle => C::BoreNoRoom,
        C::BoreNoRoom => C::BoreLeavesCore,
        C::BoreLeavesCore => C::BoreNoHoles,
        C::BoreNoHoles => C::RestOfClearsNothing,
        C::RestOfClearsNothing => C::VCarveTargetOpen,
        C::VCarveTargetOpen => C::VCarveNoMedialAxis,
        C::VCarveNoMedialAxis => C::VCarveSamplingBudget,
        C::VCarveSamplingBudget => C::StartDepthMismatch,
        C::StartDepthMismatch => C::ClearingNoRoom,
        C::ClearingNoRoom => C::ClearingNotDescending,
        C::ClearingNotDescending => C::InlayMaleMirrored,
        C::InlayMaleMirrored => C::InlayPlugBaseThin,
        C::InlayPlugBaseThin => C::InlayBandEdgeVWall,
        C::InlayBandEdgeVWall => C::InlayBandClipped,
        C::InlayBandClipped => C::InlayPartsMerged,
        C::InlayPartsMerged => C::InlayHoleFilled,
        C::InlayHoleFilled => C::InlayMaleBoardUnset,
        C::InlayMaleBoardUnset => C::InlayMarginIgnored,
        C::InlayMarginIgnored => C::InlayBoundaryEmpty,
        C::InlayBoundaryEmpty => C::InlayBoundaryHoleIgnored,
        C::InlayBoundaryHoleIgnored => C::PhotoLinesOverlap,
        C::PhotoLinesOverlap => C::TaperedCornersSoftened,
        C::TaperedCornersSoftened => C::InlayRoundedForBit,
        C::InlayRoundedForBit => C::InlayGlueGapBelowBall,
        C::InlayGlueGapBelowBall => C::InlayPlugDepthBelowBall,
        C::InlayPlugDepthBelowBall => return None,
    })
}

#[test]
fn every_warning_code_has_a_variant() {
    let chapter = include_str!("../../../../docs/book/src/user-guide/warnings.md");
    let fixtures = message_fixtures();
    let mut code = Some(WarningCode::OpenPathSkipped);
    let mut count = 0;
    while let Some(c) = code {
        assert!(
            fixtures.iter().any(|(w, _)| w.code() == c),
            "no variant has code `{c}`"
        );
        // The warnings that name an operation return it from `op`.
        let names_op = !matches!(
            c,
            WarningCode::OpenPathSkipped
                | WarningCode::SideNeedsOpenPath
                | WarningCode::ToolTooLarge
                | WarningCode::DuplicateShapeDropped
                | WarningCode::TabNotCrossed
                | WarningCode::TabOffOutline
                | WarningCode::ToolpathsOverlap
                | WarningCode::ToolChangeAvoidable
        );
        for (w, _) in fixtures.iter().filter(|(w, _)| w.code() == c) {
            assert_eq!(w.op().is_some(), names_op, "{w:?}");
        }
        assert!(
            chapter.contains(&format!("| `{c}` |")),
            "the Warnings chapter does not list `{c}`"
        );
        count += 1;
        code = next_code(c);
    }
    assert_eq!(count, 61);
}

#[test]
fn rounded_drops_noise_and_trailing_zeros() {
    assert_eq!(rounded(54.600_002_225_240_075, 2), "54.6");
    assert_eq!(rounded(14.35, 2), "14.35");
    assert_eq!(rounded(12.0, 2), "12");
    assert_eq!(rounded(-0.001, 2), "0");
    assert_eq!(rounded(105.128, 1), "105.1");
}
