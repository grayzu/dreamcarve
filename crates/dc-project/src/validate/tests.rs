// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of project validation: the problems [`validate`] reports, with
//! their codes, severities and field paths, and the projects it accepts.

mod problem_snapshot;

use super::*;
use crate::model::{
    ClearanceStrategy, CornerRelief, DrillCycle, Entry, LastPass, Lead, Operation, PeckRetract,
    ToolKind,
};
use crate::test_support::{
    photo_project, profile_mut, sample_bore, sample_drill, sample_engrave, sample_pocket,
    sample_project,
};
use dc_geometry::{MAX_COORDINATE_MM, Polyline, Transform, Vertex, approx_eq};

/// #105: a placed tab must name an instance and one of its
/// definition's profiles, and its id must be its own.
#[test]
fn placed_tab_on_a_missing_outline_is_an_error() {
    use crate::test_support::sample_tab;
    let mut p = sample_project();
    p.tabs = vec![
        sample_tab("tab1", 25.0, 0.0),
        PlacedTab {
            instance: InstanceId::new("nope"),
            ..sample_tab("tab2", 0.0, 0.0)
        },
        PlacedTab {
            profile: 3,
            ..sample_tab("tab3", 0.0, 0.0)
        },
        sample_tab("tab1", 50.0, 15.0),
    ];
    let problems = validate(&p);
    assert!(
        problems.contains(&Problem::TabDanglingInstance {
            tab: TabId::new("tab2"),
            instance: InstanceId::new("nope"),
        }),
        "{problems:?}"
    );
    assert!(
        problems.contains(&Problem::TabProfileMissing {
            tab: TabId::new("tab3"),
            instance: InstanceId::new("i1"),
            profile: 3,
            count: 1,
        }),
        "{problems:?}"
    );
    assert!(
        problems.contains(&Problem::DuplicateTabId {
            id: TabId::new("tab1")
        }),
        "{problems:?}"
    );
    let dangling = problems
        .iter()
        .find(|q| matches!(q, Problem::TabDanglingInstance { .. }))
        .unwrap();
    assert!(dangling.is_error());
    assert_eq!(dangling.field().as_deref(), Some("tabs[tab2].instance"));
    assert_eq!(
        dangling.to_string(),
        "tab `tab2` is on instance `nope`, which does not exist"
    );
}

/// #105: a placed tab's size must be positive and its point a finite
/// coordinate within the limit.
#[test]
fn placed_tab_size_and_point_are_checked() {
    use crate::test_support::sample_tab;
    let mut p = sample_project();
    p.tabs = vec![
        PlacedTab {
            width_mm: 0.0,
            height_mm: -1.0,
            ..sample_tab("tab1", 25.0, 0.0)
        },
        PlacedTab {
            x_mm: 1.0e7,
            ..sample_tab("tab2", 25.0, 0.0)
        },
    ];
    let fields: Vec<String> = errors(&validate(&p))
        .iter()
        .filter_map(|q| q.field())
        .collect();
    for field in [
        "tabs[tab1].width_mm",
        "tabs[tab1].height_mm",
        "tabs[tab2].x_mm",
    ] {
        assert!(fields.iter().any(|f| f == field), "{field}: {fields:?}");
    }
    p.tabs = vec![sample_tab("tab1", 25.0, 0.0)];
    assert_eq!(errors(&validate(&p)), Vec::<&Problem>::new());
}

fn errors(problems: &[Problem]) -> Vec<&Problem> {
    problems.iter().filter(|p| p.is_error()).collect()
}

#[test]
fn sample_project_has_no_problems() {
    assert_eq!(validate(&sample_project()), vec![]);
}

#[test]
fn dangling_tool_id_is_error() {
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.tool = ToolId::new("missing");
    let problems = validate(&p);
    // t1 is now used by nothing, which is a warning of its own.
    assert_eq!(
        problems,
        vec![
            Problem::UnusedTool {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
            },
            Problem::DanglingTool {
                operation: OperationId::new("o1"),
                tool: ToolId::new("missing"),
            },
        ]
    );
    assert_eq!(problems[1].severity(), Severity::Error);
}

/// #76: an unset feed is an error, so `check` lists it with the other
/// unset values in one run instead of failing later in export.
#[test]
fn unset_feed_is_an_error() {
    let mut p = sample_project();
    p.tools[0].feed_mm_min = None;
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ToolValueUnset {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
            field: "feed_mm_min",
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(
        problems[0].to_string(),
        "tools[t1].feed_mm_min is not set (mm/min) for tool `6.35 mm upcut`"
    );
}

#[test]
fn unset_plunge_and_rpm_are_errors() {
    let mut p = sample_project();
    p.tools[0].plunge_mm_min = None;
    p.tools[0].rpm = None;
    let problems = validate(&p);
    assert_eq!(problems.len(), 2);
    assert_eq!(errors(&problems).len(), 2);
    assert_eq!(
        problems[1].to_string(),
        "tools[t1].rpm is not set (RPM) for tool `6.35 mm upcut`"
    );
}

/// #34, S3 (manual): with the router switched by hand, the speed is
/// only a hint in the pause message, so an unset one is not reported.
/// Feed and plunge still are.
#[test]
fn unset_rpm_is_not_reported_with_manual_spindle_control() {
    let mut p = sample_project();
    p.post_processor.spindle_control = SpindleControl::Manual;
    p.tools[0].rpm = None;
    assert_eq!(validate(&p), vec![]);
    p.tools[0].plunge_mm_min = None;
    assert_eq!(
        validate(&p),
        vec![Problem::ToolValueUnset {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
            field: "plunge_mm_min",
        }]
    );
}

#[test]
fn dangling_definition_id_is_error() {
    let mut p = sample_project();
    p.instances[0].definition = DefinitionId::new("missing");
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::DanglingDefinition {
            instance: InstanceId::new("i1"),
            definition: DefinitionId::new("missing"),
        }]
    );
    assert!(problems[0].is_error());
}

#[test]
fn dangling_instance_id_is_error() {
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.targets.push(InstanceId::new("missing"));
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::DanglingInstance {
            operation: OperationId::new("o1"),
            instance: InstanceId::new("missing"),
        }]
    );
}

#[test]
fn non_positive_stock_dimension_is_error() {
    let mut p = sample_project();
    p.stock.thickness_mm = Some(0.0);
    let problems = validate(&p);
    assert_eq!(problems.len(), 1);
    assert!(
        matches!(&problems[0], Problem::OutOfRange { field, .. } if field == "stock.thickness_mm")
    );
    assert!(problems[0].is_error());
}

/// #64: thickness drives depth, tab Z and, with the origin at the stock
/// bottom, safe Z itself, so an unset one is an error like a missing
/// feed is at export, never a default. The message says export refuses.
#[test]
fn missing_thickness_is_an_error() {
    let mut p = sample_project();
    p.stock.thickness_mm = None;
    let problems = validate(&p);
    assert_eq!(problems, vec![Problem::StockThicknessUnset]);
    assert!(problems[0].is_error());
    assert_eq!(
        problems[0].to_string(),
        "stock.thickness_mm is not set (mm)"
    );
}

/// #64: an unset Profile depth is an error naming the operation.
#[test]
fn missing_depth_is_an_error() {
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.depth_mm = None;
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::OperationValueUnset {
            operation: OperationId::new("o1"),
            name: "Cut out".into(),
            field: "depth_mm",
        }]
    );
    assert!(problems[0].is_error());
    assert_eq!(
        problems[0].to_string(),
        "operations[o1].depth_mm is not set (mm) for operation `Cut out`"
    );
}

/// #64: one run reports the unset thickness, depth and tool rates
/// together, so the user fixes them in one edit.
#[test]
fn missing_thickness_depth_and_feeds_are_reported_together() {
    let mut p = sample_project();
    p.stock.thickness_mm = None;
    p.tools[0].feed_mm_min = None;
    let op = profile_mut(&mut p);
    op.depth_mm = None;
    let problems = validate(&p);
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(problems.contains(&Problem::StockThicknessUnset));
    assert!(
        problems.iter().any(
            |p| matches!(p, Problem::OperationValueUnset { field, .. } if *field == "depth_mm")
        )
    );
    assert!(
        problems
            .iter()
            .any(|p| matches!(p, Problem::ToolValueUnset { field, .. } if *field == "feed_mm_min"))
    );
}

/// #76: every unset value is listed, in the order the file holds them,
/// each with the field that locates it.
#[test]
fn unset_values_are_listed_in_file_order_with_fields() {
    let mut p = sample_project();
    p.stock.thickness_mm = None;
    p.tools[0].feed_mm_min = None;
    p.tools[0].plunge_mm_min = None;
    p.tools[0].rpm = None;
    let op = profile_mut(&mut p);
    op.depth_mm = None;
    let problems = validate(&p);
    assert!(problems.iter().all(Problem::is_unset_value), "{problems:?}");
    assert!(problems.iter().all(Problem::is_error), "{problems:?}");
    let fields: Vec<_> = problems.iter().filter_map(Problem::field).collect();
    assert_eq!(
        fields,
        [
            "stock.thickness_mm",
            "tools[t1].feed_mm_min",
            "tools[t1].plunge_mm_min",
            "tools[t1].rpm",
            "operations[o1].depth_mm",
        ]
    );
}

/// #76: a problem's field is where it sits in the saved file.
#[test]
fn problem_fields_locate_in_the_saved_file() {
    let mut p = sample_project();
    p.tools[0].diameter_mm = 0.0;
    p.tools[0].cutter = ToolKind::VBit {
        included_angle_deg: 200.0,
    };
    p.instances[0].definition = DefinitionId::new("missing");
    p.post_processor.file_extension = "dcv".into();
    let op = profile_mut(&mut p);
    op.safe_z_mm = 0.0;
    op.enabled = false;
    let problems = validate(&p);
    assert_eq!(problems.len(), 7, "{problems:?}");
    let text = crate::save(&p).unwrap();
    let map = crate::SourceMap::parse(&text).unwrap();
    for problem in &problems {
        let field = problem.field().unwrap();
        assert!(map.locate(&field).is_some(), "{field} in {text}");
    }
    assert!(
        Problem::DuplicateToolId {
            id: ToolId::new("t1")
        }
        .field()
        .is_none()
    );
    assert!(!problems.iter().any(Problem::is_unset_value));
}

/// A second tool with no rates set.
fn unset_tool(id: &str) -> Tool {
    Tool {
        id: ToolId::new(id),
        name: "3 mm downcut".into(),
        feed_mm_min: None,
        plunge_mm_min: None,
        rpm: None,
        ..crate::test_support::tool_6_35_upcut()
    }
}

/// PR #84: a tool nothing cuts with is one warning, located at the
/// tool; its unset rates are not errors, and nothing defaults them.
#[test]
fn unused_tool_with_unset_values_is_a_warning_not_an_error() {
    let mut p = sample_project();
    p.tools.push(unset_tool("t2"));
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::UnusedTool {
            tool: ToolId::new("t2"),
            name: "3 mm downcut".into(),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(
        problems[0].to_string(),
        "tool `t2` (`3 mm downcut`) is defined but not used by any enabled operation"
    );
    let text = crate::save(&p).unwrap();
    let map = crate::SourceMap::parse(&text).unwrap();
    let at = map.locate(&problems[0].field().unwrap()).unwrap();
    let line = text.lines().nth(at.line - 1).unwrap();
    assert_eq!(line.trim(), "{", "points at the tool object: {line}");
    let next = text.lines().nth(at.line).unwrap();
    assert_eq!(next.trim(), "\"id\": \"t2\",");
}

/// PR #84: a tool only disabled operations use is unused, like the
/// operations themselves; enabling one brings its errors back.
#[test]
fn tool_used_only_by_disabled_operation_is_unused() {
    let mut p = sample_project();
    p.tools[0].feed_mm_min = None;
    p.tools[0].rpm = None;
    let op = profile_mut(&mut p);
    op.enabled = false;
    let problems = validate(&p);
    assert!(errors(&problems).is_empty(), "{problems:?}");
    assert!(problems.contains(&Problem::UnusedTool {
        tool: ToolId::new("t1"),
        name: "6.35 mm upcut".into(),
    }));
    let op = profile_mut(&mut p);
    op.enabled = true;
    let problems = validate(&p);
    assert_eq!(errors(&problems).len(), 2, "{problems:?}");
    assert!(
        !problems
            .iter()
            .any(|p| matches!(p, Problem::UnusedTool { .. }))
    );
}

/// PR #84: a tool an enabled operation uses still reports every unset
/// rate as an error, beside an unused tool's warning.
#[test]
fn used_tool_with_unset_values_is_still_an_error() {
    let mut p = sample_project();
    p.tools[0].feed_mm_min = None;
    p.tools[0].plunge_mm_min = None;
    p.tools[0].rpm = None;
    p.tools.push(unset_tool("t2"));
    let problems = validate(&p);
    let fields: Vec<_> = errors(&problems)
        .into_iter()
        .filter_map(Problem::field)
        .collect();
    assert_eq!(
        fields,
        [
            "tools[t1].feed_mm_min",
            "tools[t1].plunge_mm_min",
            "tools[t1].rpm"
        ]
    );
    assert_eq!(problems.len(), 4, "one warning for t2: {problems:?}");
}

/// A zero feed is wrong even on an unused tool.
#[test]
fn unused_tool_still_rejects_bad_values() {
    let mut p = sample_project();
    let mut t2 = unset_tool("t2");
    t2.feed_mm_min = Some(0.0);
    p.tools.push(t2);
    let problems = validate(&p);
    assert!(
        problems.iter().any(|p| matches!(p, Problem::ToolOutOfRange { tool, field, .. } if tool.as_str() == "t2" && *field == "feed_mm_min")),
        "{problems:?}"
    );
}

#[test]
fn non_positive_tool_diameter_is_error() {
    let mut p = sample_project();
    p.tools[0].diameter_mm = -1.0;
    let problems = validate(&p);
    assert!(
        matches!(&problems[..], [Problem::ToolOutOfRange { tool, field, .. }] if tool.as_str() == "t1" && *field == "diameter_mm"),
        "{problems:?}"
    );
}

#[test]
fn non_positive_depth_is_error() {
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.depth_mm = Some(0.0);
    let problems = validate(&p);
    assert!(
        matches!(&problems[..], [Problem::OutOfRange { field, .. }] if field == "operations[o1].depth_mm"),
        "{problems:?}"
    );
}

#[test]
fn disabled_operation_is_warning() {
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.enabled = false;
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![
            Problem::UnusedTool {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
            },
            Problem::OperationDisabled {
                operation: OperationId::new("o1"),
            },
        ]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
}

#[test]
fn non_finite_value_is_error() {
    let mut p = sample_project();
    p.stock.width_mm = f64::NAN;
    p.tools[0].cutter = ToolKind::VBit {
        included_angle_deg: f64::INFINITY,
    };
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![
            Problem::NonFinite {
                field: "stock.width_mm".into()
            },
            Problem::NonFinite {
                field: "tools[t1].cutter.v_bit.included_angle_deg".into()
            },
            // #67: a V-bit of any angle cannot cut a Profile.
            Problem::CutterNotFlat {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
                tool: ToolId::new("t1"),
                tool_name: "6.35 mm upcut".into(),
                cutter: "a V-bit",
                field: "tool",
            },
        ]
    );
}

#[test]
fn duplicate_tool_id_is_error() {
    let mut p = sample_project();
    p.tools.push(p.tools[0].clone());
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::DuplicateToolId {
            id: ToolId::new("t1")
        }]
    );
}

#[test]
fn definition_key_mismatch_is_error() {
    let mut p = sample_project();
    if let Some(def) = p.definitions.values_mut().next() {
        def.id = DefinitionId::new("other");
    }
    let problems = validate(&p);
    assert_eq!(problems.len(), 1);
    assert!(matches!(problems[0], Problem::DefinitionKeyMismatch { .. }));
}

#[test]
fn problem_messages_name_the_subject() {
    let problem = Problem::ToolValueUnset {
        tool: ToolId::new("t1"),
        name: "Set me".into(),
        field: "feed_mm_min",
    };
    assert_eq!(
        problem.to_string(),
        "tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`"
    );
}

const DWELL: &str = "post_processor.spindle_on_dwell_s";

#[test]
fn negative_spindle_dwell_is_error() {
    let mut p = sample_project();
    p.post_processor.spindle_on_dwell_s = -1.0;
    let problems = validate(&p);
    let field = DWELL.to_owned();
    assert!(
        matches!(&problems[..], [Problem::OutOfRange { field: f, .. }] if *f == field),
        "{problems:?}"
    );
}

#[test]
fn non_finite_spindle_dwell_is_error() {
    let mut p = sample_project();
    p.post_processor.spindle_on_dwell_s = f64::NAN;
    let field = DWELL.to_owned();
    assert_eq!(validate(&p), vec![Problem::NonFinite { field }]);
}

#[test]
fn zero_spindle_dwell_is_allowed() {
    let mut p = sample_project();
    p.post_processor.spindle_on_dwell_s = 0.0;
    assert!(validate(&p).is_empty());
}

/// Functional spec acceptance test 2 (#40): a tool problem names the
/// tool as the user sees it, not only by id.
#[test]
fn tool_problems_name_the_tool() {
    let mut p = sample_project();
    p.tools[0].name = "Set me".into();
    p.tools[0].feed_mm_min = Some(0.0);
    p.tools[0].plunge_mm_min = None;
    let messages: Vec<String> = validate(&p).iter().map(ToString::to_string).collect();
    assert_eq!(messages.len(), 2, "{messages:?}");
    for (message, field) in messages.iter().zip(["feed_mm_min", "plunge_mm_min"]) {
        for part in ["Set me", "t1", field] {
            assert!(message.contains(part), "`{message}` names {part}");
        }
    }
}

/// Functional spec capability 8 (#40): open paths are imported but
/// Profile never cuts them, and `check` reports each one.
#[test]
fn open_profile_is_warning_naming_the_definition() {
    let mut p = sample_project();
    assert!(
        !validate(&p)
            .iter()
            .any(|q| matches!(q, Problem::OpenProfile { .. })),
        "the sample's closed rectangle is not reported"
    );
    let def = p
        .definitions
        .get_mut(&DefinitionId::new("d1"))
        .expect("the sample has d1");
    def.profiles.push(Polyline::new(
        vec![Vertex::new(0.0, 40.0, 0.0), Vertex::new(50.0, 40.0, 0.0)],
        false,
    ));
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::OpenProfile {
            definition: DefinitionId::new("d1"),
            name: "Rectangle".into(),
            profile: 1,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    let message = problems[0].to_string();
    assert!(
        message.contains("Rectangle") && message.contains("open"),
        "{message}"
    );
}

/// Capability 15 (#135): Engrave cuts open paths, so an open profile of
/// a definition an enabled Engrave targets is not reported as never
/// cut. A disabled Engrave cuts nothing, so the warning stays.
#[test]
fn open_profile_engraved_is_not_reported() {
    let mut p = sample_project();
    p.definitions
        .get_mut(&DefinitionId::new("d1"))
        .expect("the sample has d1")
        .profiles
        .push(Polyline::new(
            vec![Vertex::new(0.0, 40.0, 0.0), Vertex::new(50.0, 40.0, 0.0)],
            false,
        ));
    p.operations.push(Operation::Engrave(sample_engrave()));
    let open = |p: &Project| {
        validate(p)
            .iter()
            .any(|q| matches!(q, Problem::OpenProfile { .. }))
    };
    assert!(!open(&p), "{:?}", validate(&p));
    if let Some(Operation::Engrave(op)) = p.operations.last_mut() {
        op.enabled = false;
    }
    assert!(open(&p), "a disabled Engrave cuts nothing");
}

/// #175: an On line Profile cuts open paths too, so an open profile of
/// a definition an enabled On line Profile targets is not reported.
/// Inside and Outside skip open paths, so the warning stays for them,
/// and its message names what does cut an open path.
#[test]
fn open_profile_cut_on_line_is_not_reported() {
    let mut p = sample_project();
    p.definitions
        .get_mut(&DefinitionId::new("d1"))
        .expect("the sample has d1")
        .profiles
        .push(Polyline::new(
            vec![Vertex::new(0.0, 40.0, 0.0), Vertex::new(50.0, 40.0, 0.0)],
            false,
        ));
    let open = |p: &Project| {
        validate(p)
            .into_iter()
            .find(|q| matches!(q, Problem::OpenProfile { .. }))
    };
    let message = open(&p).expect("Outside skips it").to_string();
    assert!(
        message.contains("On line") && message.contains("Engrave"),
        "{message}"
    );
    profile_mut(&mut p).side = Side::OnLine;
    assert!(open(&p).is_none(), "{:?}", validate(&p));
    // #207: Left and Right cut open paths too.
    for side in [Side::Left, Side::Right] {
        profile_mut(&mut p).side = side;
        assert!(open(&p).is_none(), "{side:?}: {:?}", validate(&p));
    }
    profile_mut(&mut p).enabled = false;
    assert!(open(&p).is_some(), "a disabled Profile cuts nothing");
}

/// The fields of every error-class problem, in order.
fn error_fields(problems: &[Problem]) -> Vec<String> {
    problems
        .iter()
        .filter(|p| p.is_error())
        .map(|p| match p {
            Problem::OutOfRange { field, .. } | Problem::NonFinite { field } => field.clone(),
            other => panic!("unexpected problem {other:?}"),
        })
        .collect()
}

/// #59: a finite but huge transform passed validation and then
/// panicked `cavalier_contours`. Scale and translation are each
/// rejected, naming the instance's transform.
#[test]
fn huge_transform_is_rejected() {
    for transform in [
        Transform::scale(1e15, 1e15),
        Transform::translate(1e12, 0.0),
        Transform::from([1.0, 0.0, 0.0, 1.0, 0.0, -1e12]),
    ] {
        let mut p = sample_project();
        p.instances[0].transform = transform;
        let fields = error_fields(&validate(&p));
        assert!(!fields.is_empty(), "{transform:?} is rejected");
        assert!(
            fields
                .iter()
                .all(|f| f.starts_with("instances[i1].transform")),
            "{transform:?}: {fields:?}"
        );
    }
}

/// #59: an instance whose placed shape reaches just past the limit is
/// rejected even though each coefficient is in range; one that reaches
/// exactly the limit is accepted. The sample rectangle is 50 × 30 mm.
#[test]
fn placement_past_coordinate_limit_is_rejected() {
    let mut p = sample_project();
    p.stock.width_mm = MAX_COORDINATE_MM;
    p.instances[0].transform = Transform::translate(MAX_COORDINATE_MM - 50.0, 10.0);
    assert_eq!(validate(&p), vec![], "reaching the limit is accepted");
    p.instances[0].transform = Transform::translate(MAX_COORDINATE_MM - 49.5, 10.0);
    let problems = validate(&p);
    assert!(
        matches!(&problems[..], [Problem::OutOfRange { field, value, .. }]
            if field == "instances[i1].transform"
                && approx_eq(*value, MAX_COORDINATE_MM + 0.5, 1e-9)),
        "{problems:?}"
    );
}

/// #99: phase 1 does not use the instance depths, so leaving them unset
/// is neither an error nor a warning.
#[test]
fn unset_instance_depths_are_valid() {
    let mut p = sample_project();
    p.instances[0].z_top_mm = None;
    p.instances[0].z_bottom_mm = None;
    assert_eq!(validate(&p), vec![]);
}

/// #71: an instance's bottom may not lie above its top; equal is a
/// flat feature and allowed. Nothing reads them yet, so the order is
/// all there is to check.
#[test]
fn instance_z_bottom_above_z_top_is_error() {
    let mut p = sample_project();
    p.instances[0].z_top_mm = Some(2.0);
    p.instances[0].z_bottom_mm = Some(3.0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::OutOfRange {
            field: "instances[i1].z_bottom_mm".into(),
            value: 3.0,
            expected: BELOW_Z_TOP,
        }]
    );
    assert!(problems[0].is_error());
    p.instances[0].z_bottom_mm = Some(2.0);
    assert_eq!(validate(&p), vec![], "equal is allowed");
    p.instances[0].z_top_mm = None;
    p.instances[0].z_bottom_mm = Some(3.0);
    assert_eq!(validate(&p), vec![], "one unset is not compared");
}

/// #99: a set instance depth is still checked against the coordinate
/// limit.
#[test]
fn instance_z_top_out_of_range_is_error() {
    let mut p = sample_project();
    p.instances[0].z_top_mm = Some(7000.0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::OutOfRange {
            field: "instances[i1].z_top_mm".into(),
            value: 7000.0,
            expected: COORDINATE,
        }]
    );
    assert!(problems[0].is_error());
    p.instances[0].z_top_mm = Some(MAX_COORDINATE_MM);
    p.instances[0].z_bottom_mm = Some(-MAX_COORDINATE_MM);
    assert_eq!(validate(&p), vec![], "the limit is inclusive");
}

/// #59: vertex coordinates are bounded by the limit, inclusive.
#[test]
fn huge_vertex_is_rejected() {
    let mut p = sample_project();
    p.instances[0].transform = Transform::IDENTITY;
    let def = p
        .definitions
        .get_mut(&DefinitionId::new("d1"))
        .expect("the sample has d1");
    def.profiles = vec![Polyline::new(
        vec![
            Vertex::line(0.0, 0.0),
            Vertex::line(MAX_COORDINATE_MM, 0.0),
            Vertex::line(MAX_COORDINATE_MM, 10.0),
        ],
        true,
    )];
    assert!(errors(&validate(&p)).is_empty(), "the limit is accepted");
    let def = p
        .definitions
        .get_mut(&DefinitionId::new("d1"))
        .expect("the sample has d1");
    def.profiles = vec![Polyline::new(
        vec![
            Vertex::line(0.0, 0.0),
            Vertex::line(1e12, 0.0),
            Vertex::line(1e12, -10.0),
        ],
        true,
    )];
    assert_eq!(
        error_fields(&validate(&p)),
        vec![
            "definitions[d1].profiles[0].vertices[1].x",
            "definitions[d1].profiles[0].vertices[2].x",
        ]
    );
}

/// #59: an arc whose vertices are in range can still bow past the
/// limit; the profile is reported.
#[test]
fn arc_bulging_past_coordinate_limit_is_rejected() {
    let mut p = sample_project();
    p.instances[0].transform = Transform::IDENTITY;
    let def = p
        .definitions
        .get_mut(&DefinitionId::new("d1"))
        .expect("the sample has d1");
    def.profiles = vec![Polyline::circle(
        dc_geometry::Point::new(0.0, MAX_COORDINATE_MM - 0.5),
        1.0,
    )];
    let fields = error_fields(&validate(&p));
    assert!(
        fields.contains(&"definitions[d1].profiles[0]".to_owned()),
        "{fields:?}"
    );
}

/// #59: stock sides are bounded by the same limit, inclusive.
#[test]
fn huge_stock_is_rejected() {
    let mut p = sample_project();
    p.stock.width_mm = MAX_COORDINATE_MM;
    p.stock.height_mm = MAX_COORDINATE_MM;
    p.stock.thickness_mm = Some(MAX_COORDINATE_MM);
    assert_eq!(validate(&p), vec![], "the limit is accepted");
    p.stock.width_mm = MAX_COORDINATE_MM + 0.5;
    p.stock.height_mm = 1e15;
    p.stock.thickness_mm = Some(1e300);
    assert_eq!(
        error_fields(&validate(&p)),
        vec!["stock.width_mm", "stock.height_mm", "stock.thickness_mm"]
    );
}

/// The limit phrases spell out `MAX_COORDINATE_MM` by hand.
#[test]
fn limit_phrases_match_the_constant() {
    let limit = format!("{MAX_COORDINATE_MM}");
    for phrase in [STOCK_SIDE, COORDINATE, COEFFICIENT, REACH] {
        assert!(phrase.contains(&limit), "`{phrase}` states {limit}");
    }
}

/// #59: the message states the limit, so the user knows what to aim for.
#[test]
fn limit_messages_state_the_limit() {
    let mut p = sample_project();
    p.stock.width_mm = 1e15;
    p.instances[0].transform = Transform::translate(1e12, 0.0);
    let limit = MAX_COORDINATE_MM.to_string();
    let problems = validate(&p);
    assert_eq!(problems.len(), 2, "{problems:?}");
    for problem in problems {
        let message = problem.to_string();
        assert!(message.contains(&limit), "`{message}` states {limit}");
    }
}

/// #36 security review (#40): a shared `.dcv` chooses its output
/// extension, so it must not name the project file or a path.
#[test]
fn unsafe_file_extension_is_error() {
    for extension in ["dcv", "DCV", "", "/../x", "n c", "toolong12"] {
        let mut p = sample_project();
        p.post_processor.file_extension = extension.into();
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::UnsafeFileExtension {
                extension: extension.into()
            }],
            "`{extension}` is refused"
        );
        assert!(problems[0].is_error(), "`{extension}` is an error");
    }
    for extension in ["nc", "gcode", "tap", "NGC"] {
        let mut p = sample_project();
        p.post_processor.file_extension = extension.into();
        assert_eq!(validate(&p), vec![], "`{extension}` is allowed");
    }
}

/// #95: `"m6"` needs a definition whose tool change has `M6`. GRBL 1.1
/// has none (`definitions/README.md`, "Tool changes"), so the mode is an
/// error located at the field, whatever the operations. With an `M6`
/// line, as grblHAL has, it is fine.
#[test]
fn m6_mode_without_m6_command_is_an_error() {
    let mut p = sample_project();
    p.post_processor.tool_change_mode = crate::ToolChangeMode::M6;
    let problems = validate(&p);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].is_error(), "{problems:?}");
    let field = problems[0].field().unwrap();
    assert_eq!(field, "post_processor.tool_change_mode");
    let message = problems[0].to_string();
    assert!(
        message.contains("M6") && message.contains("separate_programs"),
        "{message}"
    );
    let text = crate::save(&p).unwrap();
    let map = crate::SourceMap::parse(&text).unwrap();
    assert!(map.locate(&field).is_some(), "{field} in {text}");

    p.post_processor = crate::PostProcessorRef {
        tool_change_mode: crate::ToolChangeMode::M6,
        ..crate::bundled_definition("grblhal", 1)
            .unwrap()
            .default_ref()
    };
    assert_eq!(validate(&p), vec![]);
}

// #67: sanity checks on dangerous but valid values.

fn op_mut(p: &mut Project) -> &mut crate::model::ProfileOp {
    profile_mut(p)
}

/// Asserts `problems` is exactly one problem with `severity`, whose
/// field locates in the saved `p`; returns its message.
fn only_located(p: &Project, problems: &[Problem], severity: Severity) -> String {
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].severity(), severity, "{problems:?}");
    let text = crate::save(p).unwrap();
    let map = crate::SourceMap::parse(&text).unwrap();
    let field = problems[0].field().unwrap();
    assert!(map.locate(&field).is_some(), "{field} in {text}");
    problems[0].to_string()
}

/// #67: a 50 mm step-down with a 6.35 mm end mill is one full-depth
/// pass; it is a warning naming the operation and the tool, located at
/// the step-down in effect.
#[test]
fn step_down_above_diameter_warns() {
    let mut p = sample_project();
    op_mut(&mut p).step_down_mm = Some(50.0);
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(problems[0].field().unwrap(), "operations[o1].step_down_mm");
    assert_eq!(
        message,
        "operation `Cut out` (`o1`) cuts 50 mm per pass, deeper than the 6.35 mm diameter of tool `6.35 mm upcut`; passes that deep can break the bit, so use a step-down of at most 6.35 mm"
    );

    // The tool's step-down, when the operation sets none.
    let mut p = sample_project();
    p.tools[0].step_down_mm = Some(7.0);
    let problems = validate(&p);
    only_located(&p, &problems, Severity::Warning);
    assert_eq!(problems[0].field().unwrap(), "tools[t1].step_down_mm");

    // Equal to the diameter is fine.
    let mut p = sample_project();
    op_mut(&mut p).step_down_mm = Some(6.35);
    assert_eq!(validate(&p), vec![]);
}

/// #67: Profile offsets by half the diameter, which is right only for
/// a flat end mill; any other cutter makes the part the wrong size.
#[test]
fn v_bit_in_profile_is_an_error() {
    let mut p = sample_project();
    p.tools[0].cutter = ToolKind::VBit {
        included_angle_deg: 60.0,
    };
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Error);
    assert_eq!(problems[0].field().unwrap(), "operations[o1].tool");
    assert_eq!(
        message,
        "operation `Cut out` (`o1`) cuts with tool `6.35 mm upcut` (`t1`), a V-bit; an Inside, Outside, Left or Right Profile and a Pocket offset the path by half the diameter, which only a flat end mill cuts at every depth, so the part would come out the wrong size. Use an end mill"
    );
}

#[test]
fn ball_nose_and_engraver_in_profile_are_errors() {
    for (cutter, words) in [
        (ToolKind::BallNose, "a ball-nose bit"),
        (ToolKind::Engraver, "an engraving bit"),
    ] {
        for side in [crate::model::Side::Outside, crate::model::Side::Inside] {
            let mut p = sample_project();
            p.tools[0].cutter = cutter.clone();
            op_mut(&mut p).side = side;
            let problems = validate(&p);
            let message = only_located(&p, &problems, Severity::Error);
            assert!(message.contains(words), "{message}");
        }
    }
}

/// #207: Left and Right offset the path by half the diameter, as
/// Inside and Outside do, so they need a flat end mill too.
#[test]
fn v_bit_left_or_right_is_an_error() {
    for side in [crate::model::Side::Left, crate::model::Side::Right] {
        let mut p = sample_project();
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        op_mut(&mut p).side = side;
        let problems = validate(&p);
        assert!(
            problems
                .iter()
                .any(|q| matches!(q, Problem::CutterNotFlat { .. })),
            "{side:?}: {problems:?}"
        );
    }
}

/// #67 (maintainer, PR #96): on the line the tool centre follows the
/// line itself, so the cut does not depend on the cutter's shape.
#[test]
fn v_bit_on_line_is_allowed() {
    let mut p = sample_project();
    p.tools[0].cutter = ToolKind::VBit {
        included_angle_deg: 60.0,
    };
    op_mut(&mut p).side = crate::model::Side::OnLine;
    assert_eq!(validate(&p), vec![]);
}

#[test]
fn ball_nose_and_engraver_on_line_are_allowed() {
    for cutter in [ToolKind::BallNose, ToolKind::Engraver] {
        let mut p = sample_project();
        p.tools[0].cutter = cutter;
        op_mut(&mut p).side = crate::model::Side::OnLine;
        assert_eq!(validate(&p), vec![]);
    }
}

/// #67 (maintainer comment): a feed over the limit fails export. The
/// error names the tool, the field and the limit.
#[test]
fn feed_above_limit_is_an_error() {
    let mut p = sample_project();
    p.tools[0].feed_mm_min = Some(1e9);
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Error);
    assert_eq!(problems[0].field().unwrap(), "tools[t1].feed_mm_min");
    assert_eq!(
        message,
        "tool `6.35 mm upcut` (`t1`): feed_mm_min is 1000000000, expected a value greater than 0 and at most 20000 mm/min"
    );
    p.tools[0].feed_mm_min = Some(MAX_RATE_MM_MIN);
    assert_eq!(validate(&p), vec![]);
}

#[test]
fn plunge_above_limit_is_an_error() {
    let mut p = sample_project();
    p.tools[0].feed_mm_min = Some(MAX_RATE_MM_MIN);
    p.tools[0].plunge_mm_min = Some(MAX_RATE_MM_MIN + 1.0);
    let problems = validate(&p);
    // Also faster than the feed, which is a warning of its own.
    let errors = errors(&problems);
    assert_eq!(errors.len(), 1, "{problems:?}");
    assert_eq!(errors[0].field().unwrap(), "tools[t1].plunge_mm_min");
}

/// The rate limit phrase spells out `MAX_RATE_MM_MIN` by hand.
#[test]
fn rate_limit_phrase_matches_the_constant() {
    assert!(
        RATE.contains(&format!("{MAX_RATE_MM_MIN} mm/min")),
        "{RATE}"
    );
}

/// #67: GRBL and grblHAL clamp `S` to `$30`, so a huge speed is a
/// warning, not an error.
#[test]
fn rpm_above_limit_warns() {
    let mut p = sample_project();
    p.tools[0].rpm = Some(1_000_000);
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(problems[0].field().unwrap(), "tools[t1].rpm");
    assert_eq!(
        message,
        "tool `6.35 mm upcut` (`t1`) is set to 1000000 RPM, faster than the 60000 RPM a hobby spindle or router turns; check it for a typo"
    );
    p.tools[0].rpm = Some(MAX_RPM);
    assert_eq!(validate(&p), vec![]);
}

#[test]
fn plunge_above_feed_warns() {
    let mut p = sample_project();
    p.tools[0].plunge_mm_min = Some(1200.0);
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(problems[0].field().unwrap(), "tools[t1].plunge_mm_min");
    assert_eq!(
        message,
        "tool `6.35 mm upcut` (`t1`) plunges at 1200 mm/min, faster than it cuts (1000 mm/min); a plunge rate is usually well below the feed rate, so check the two are not swapped"
    );
    p.tools[0].plunge_mm_min = Some(1000.0);
    assert_eq!(validate(&p), vec![]);
}

#[test]
fn diameter_above_limit_warns() {
    let mut p = sample_project();
    p.tools[0].diameter_mm = 200.0;
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(problems[0].field().unwrap(), "tools[t1].diameter_mm");
    assert_eq!(
        message,
        "tool `6.35 mm upcut` (`t1`) is 200 mm in diameter, larger than the 50 mm of a hobby router bit; check it for a typo"
    );
    p.tools[0].diameter_mm = MAX_TOOL_DIAMETER_MM;
    assert_eq!(validate(&p), vec![]);
}

/// #67: an enabled operation with no targets would write a program
/// that starts the spindle and stops it without moving.
#[test]
fn empty_operation_is_an_error() {
    let mut p = sample_project();
    op_mut(&mut p).targets.clear();
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Error);
    assert_eq!(problems[0].field().unwrap(), "operations[o1].targets");
    assert_eq!(
        message,
        "operation `Cut out` (`o1`) is enabled but has no targets, so it would cut nothing; add targets or disable it"
    );
    // Disabled, it is only the disabled warning (and its unused tool).
    op_mut(&mut p).enabled = false;
    assert!(errors(&validate(&p)).is_empty());
}

/// #67: only tools an enabled operation uses are checked; an unused
/// tool is already a warning of its own.
#[test]
fn unused_tool_gets_no_sanity_findings() {
    let mut p = sample_project();
    p.tools.push(Tool {
        id: ToolId::new("t2"),
        name: "Odd".into(),
        cutter: ToolKind::VBit {
            included_angle_deg: 60.0,
        },
        diameter_mm: 200.0,
        feed_mm_min: Some(1e9),
        plunge_mm_min: Some(2e9),
        rpm: Some(1_000_000),
        step_down_mm: Some(500.0),
        ..crate::test_support::tool_6_35_upcut()
    });
    assert_eq!(
        validate(&p),
        vec![Problem::UnusedTool {
            tool: ToolId::new("t2"),
            name: "Odd".into(),
        }]
    );
}

/// #67: every sanity problem is reported in one run, in file order.
#[test]
fn sanity_problems_are_reported_together() {
    let mut p = sample_project();
    p.tools[0].feed_mm_min = Some(1e9);
    p.tools[0].plunge_mm_min = Some(2e9);
    p.tools[0].rpm = Some(1_000_000);
    op_mut(&mut p).targets.clear();
    let problems = validate(&p);
    let fields: Vec<_> = problems.iter().filter_map(Problem::field).collect();
    assert_eq!(
        fields,
        [
            "tools[t1].feed_mm_min",
            "tools[t1].plunge_mm_min",
            "tools[t1].rpm",
            "tools[t1].plunge_mm_min",
            "operations[o1].targets",
        ],
        "{problems:?}"
    );
    assert_eq!(errors(&problems).len(), 3, "{problems:?}");
}

/// #98: with neither the operation nor its tool giving a step-down,
/// the passes are unknown, so it is an error located at the tool, the
/// one value every operation using it falls back on.
#[test]
fn step_down_unset_on_operation_and_tool_is_error() {
    let mut p = sample_project();
    p.tools[0].step_down_mm = None;
    op_mut(&mut p).step_down_mm = None;
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ToolValueUnset {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
            field: "step_down_mm",
        }]
    );
    assert!(problems[0].is_error());
    assert!(problems[0].is_unset_value());
    assert_eq!(problems[0].field().unwrap(), "tools[t1].step_down_mm");
    assert_eq!(
        problems[0].to_string(),
        "tools[t1].step_down_mm is not set (mm) for tool `6.35 mm upcut`"
    );
}

/// #98: the operation's own step-down is enough.
#[test]
fn step_down_on_operation_only_is_valid() {
    let mut p = sample_project();
    p.tools[0].step_down_mm = None;
    op_mut(&mut p).step_down_mm = Some(2.0);
    assert_eq!(validate(&p), vec![]);
}

/// #98: the tool's step-down applies when the operation sets none.
#[test]
fn step_down_on_tool_only_is_valid() {
    let mut p = sample_project();
    p.tools[0].step_down_mm = Some(2.0);
    op_mut(&mut p).step_down_mm = None;
    assert_eq!(validate(&p), vec![]);
}

/// #98: two operations falling back on the same unset tool value give
/// one error, not one per operation.
#[test]
fn step_down_unset_reported_once_per_tool() {
    let mut p = sample_project();
    p.tools[0].step_down_mm = None;
    op_mut(&mut p).step_down_mm = None;
    let mut second = op_mut(&mut p).clone();
    second.id = OperationId::new("o2");
    p.operations.push(Operation::Profile(second));
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ToolValueUnset {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
            field: "step_down_mm",
        }]
    );
}

/// #98: a disabled operation cuts nothing, so it needs no step-down.
#[test]
fn disabled_operation_needs_no_step_down() {
    let mut p = sample_project();
    p.tools[0].step_down_mm = None;
    let op = op_mut(&mut p);
    op.step_down_mm = None;
    op.enabled = false;
    let problems = validate(&p);
    assert!(errors(&problems).is_empty(), "{problems:?}");
    assert!(
        !problems
            .iter()
            .any(|p| matches!(p, Problem::ToolValueUnset { .. })),
        "{problems:?}"
    );
}

/// #67: the quick start's coaster, as the chapter fills it in (feed
/// 1500, plunge 500, 18000 RPM, 4 mm step-down, 6.35 mm end mill),
/// draws no sanity finding. `dreamcarve`'s CLI test
/// `quick_start_project_has_no_sanity_warnings` checks the real file.
#[test]
fn quick_start_values_have_no_sanity_findings() {
    let mut p = sample_project();
    let tool = &mut p.tools[0];
    tool.cutter = ToolKind::EndMill;
    tool.diameter_mm = 6.35;
    tool.feed_mm_min = Some(1500.0);
    tool.plunge_mm_min = Some(500.0);
    tool.rpm = Some(18000);
    // On the tool since #98; the operation's is left unset.
    tool.step_down_mm = Some(4.0);
    p.stock.thickness_mm = Some(11.8);
    let op = op_mut(&mut p);
    op.step_down_mm = None;
    op.depth_mm = Some(12.0);
    op.through_cut_allowance_mm = 0.2;
    assert_eq!(validate(&p), vec![]);
}

// Phase 2, #117: tool numbers and the new operations' rules
// (`docs/specs/phase-2/design.md`, "Tool numbers", "Validation").

/// The sample project with `ops` after its Profile.
fn with_ops(ops: Vec<Operation>) -> Project {
    let mut p = sample_project();
    p.operations.extend(ops);
    p
}

/// The sample tool copied as `id`, every value set.
fn tool(id: &str) -> Tool {
    Tool {
        id: ToolId::new(id),
        name: format!("tool {id}"),
        ..crate::test_support::tool_6_35_upcut()
    }
}

/// Design spec, "Tool numbers": two tools of enabled operations that
/// resolve to the same `T` number would make the program change to
/// the wrong bit. An error at the second tool's `number`.
#[test]
fn duplicate_tool_number_is_error() {
    let mut p = sample_project();
    p.tools[0].number = Some(2);
    let mut t2 = tool("t2");
    t2.number = Some(2);
    p.tools.push(t2);
    let mut second = sample_pocket();
    second.tool = ToolId::new("t2");
    p.operations.push(Operation::Pocket(second));
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::DuplicateToolNumber {
            tool: ToolId::new("t2"),
            name: "tool t2".into(),
            number: 2,
            other: ToolId::new("t1"),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].field().as_deref(), Some("tools[t2].number"));
    assert!(problems[0].to_string().contains("T2"), "{}", problems[0]);
}

/// An unset number is the tool's 1-based position, which a set number
/// on another tool may clash with.
#[test]
fn set_number_clashing_with_positional_is_error() {
    let mut p = sample_project();
    let mut t2 = tool("t2");
    t2.number = Some(1);
    p.tools.push(t2);
    let mut second = sample_pocket();
    second.tool = ToolId::new("t2");
    p.operations.push(Operation::Pocket(second));
    assert_eq!(
        validate(&p),
        vec![Problem::DuplicateToolNumber {
            tool: ToolId::new("t2"),
            name: "tool t2".into(),
            number: 1,
            other: ToolId::new("t1"),
        }]
    );
    // The other way round: the positional tool is the second one.
    let mut p = sample_project();
    p.tools[0].number = Some(2);
    p.tools.push(tool("t2"));
    let mut second = sample_pocket();
    second.tool = ToolId::new("t2");
    p.operations.push(Operation::Pocket(second));
    assert_eq!(
        validate(&p),
        vec![Problem::DuplicateToolNumber {
            tool: ToolId::new("t2"),
            name: "tool t2".into(),
            number: 2,
            other: ToolId::new("t1"),
        }]
    );
}

/// `T0` is no tool on most controllers.
#[test]
fn tool_number_zero_is_error() {
    let mut p = sample_project();
    p.tools[0].number = Some(0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ToolOutOfRange {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
            field: "number",
            value: 0.0,
            expected: POSITIVE,
        }]
    );
    assert_eq!(problems[0].field().as_deref(), Some("tools[t1].number"));
}

/// Only tools of enabled operations are written to a program, so only
/// they can clash.
#[test]
fn unused_tools_do_not_clash() {
    let mut p = sample_project();
    p.tools[0].number = Some(2);
    let mut t2 = tool("t2");
    t2.number = Some(2);
    p.tools.push(t2);
    assert_eq!(
        validate(&p),
        vec![Problem::UnusedTool {
            tool: ToolId::new("t2"),
            name: "tool t2".into(),
        }]
    );
}

/// Functional spec, capability 13: rest machining clears what an
/// earlier Pocket of the same shapes left, so `rest_of` must name one.
#[test]
fn rest_of_must_be_an_earlier_enabled_pocket_with_the_same_targets() {
    let rough = sample_pocket();
    let mut rest = sample_pocket();
    rest.id = OperationId::new("o5");
    rest.name = "Rest".into();
    rest.rest_of = Some(OperationId::new("o2"));
    let p = with_ops(vec![
        Operation::Pocket(rough.clone()),
        Operation::Pocket(rest.clone()),
    ]);
    assert_eq!(validate(&p), vec![], "an earlier enabled Pocket");

    let invalid = Problem::RestOfInvalid {
        operation: OperationId::new("o5"),
        name: "Rest".into(),
        rest_of: OperationId::new("o2"),
    };
    assert_eq!(invalid.severity(), Severity::Error);
    assert_eq!(invalid.field().as_deref(), Some("operations[o5].rest_of"));

    // Later, not earlier.
    let p = with_ops(vec![
        Operation::Pocket(rest.clone()),
        Operation::Pocket(rough.clone()),
    ]);
    assert_eq!(validate(&p), vec![invalid.clone()], "a later Pocket");

    // Disabled.
    let mut off = rough.clone();
    off.enabled = false;
    let p = with_ops(vec![
        Operation::Pocket(off),
        Operation::Pocket(rest.clone()),
    ]);
    let problems = validate(&p);
    assert!(
        problems.contains(&invalid),
        "a disabled Pocket: {problems:?}"
    );

    // A Profile, not a Pocket.
    let mut rest_of_profile = rest.clone();
    rest_of_profile.rest_of = Some(OperationId::new("o1"));
    let p = with_ops(vec![Operation::Pocket(rest_of_profile)]);
    assert_eq!(
        validate(&p),
        vec![Problem::RestOfInvalid {
            operation: OperationId::new("o5"),
            name: "Rest".into(),
            rest_of: OperationId::new("o1"),
        }],
        "a Profile"
    );

    // Other targets.
    let mut other_targets = rough.clone();
    other_targets.targets = vec![InstanceId::new("i2")];
    let mut p = with_ops(vec![
        Operation::Pocket(other_targets),
        Operation::Pocket(rest.clone()),
    ]);
    p.instances.push(crate::FeatureInstance {
        id: InstanceId::new("i2"),
        ..p.instances[0].clone()
    });
    assert_eq!(validate(&p), vec![invalid.clone()], "other targets");

    // Nothing of that id at all.
    let mut dangling = rest.clone();
    dangling.rest_of = Some(OperationId::new("nope"));
    let p = with_ops(vec![Operation::Pocket(dangling)]);
    assert_eq!(
        validate(&p),
        vec![Problem::RestOfInvalid {
            operation: OperationId::new("o5"),
            name: "Rest".into(),
            rest_of: OperationId::new("nope"),
        }]
    );
}

#[test]
fn step_over_fraction_outside_unit_interval_is_error() {
    for (fraction, ok) in [(0.0, false), (1.5, false), (1.0, true), (0.4, true)] {
        let mut pocket = sample_pocket();
        pocket.step_over_fraction = Some(fraction);
        let p = with_ops(vec![Operation::Pocket(pocket)]);
        let problems = validate(&p);
        if ok {
            assert_eq!(problems, vec![], "{fraction}");
        } else {
            assert_eq!(
                problems,
                vec![Problem::OutOfRange {
                    field: "operations[o2].step_over_fraction".into(),
                    value: fraction,
                    expected: FRACTION,
                }],
                "{fraction}"
            );
        }
    }
}

/// A Pocket or Engrave has no single-pass form: with no step-down on
/// the operation or its tool nothing says how deep each pass is, and
/// nothing defaults it (#98). Reported once, at the tool, as Profile
/// reports it.
#[test]
fn pocket_without_step_down_is_error() {
    for op in [
        Operation::Pocket(sample_pocket()),
        Operation::Engrave(sample_engrave()),
    ] {
        let mut p = with_ops(vec![op.clone()]);
        p.tools[0].step_down_mm = None;
        // The Profile falls back on the tool too; take it out of the way.
        profile_mut(&mut p).step_down_mm = Some(2.0);
        assert_eq!(
            validate(&p),
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "step_down_mm",
            }],
            "{op:?}"
        );
        // Set on the operation: fine.
        let mut p = with_ops(vec![op.clone()]);
        p.tools[0].step_down_mm = None;
        profile_mut(&mut p).step_down_mm = Some(2.0);
        match p.operations.get_mut(1) {
            Some(Operation::Pocket(pocket)) => pocket.step_down_mm = Some(1.0),
            Some(Operation::Engrave(engrave)) => engrave.step_down_mm = Some(1.0),
            _ => panic!("the added operation"),
        }
        assert_eq!(validate(&p), vec![], "{op:?}");
    }
}

/// Functional spec, capability 14: a peck's depth is the tool's
/// step-down by default, so one of the two must be set. A simple
/// cycle needs neither.
#[test]
fn peck_without_depth_or_tool_step_down_is_error() {
    let mut drill = sample_drill();
    drill.cycle = DrillCycle::Peck {
        peck_depth_mm: None,
        retract: PeckRetract::Full,
    };
    let mut p = with_ops(vec![Operation::Drill(drill.clone())]);
    p.tools[0].step_down_mm = None;
    profile_mut(&mut p).step_down_mm = Some(2.0);
    assert_eq!(
        validate(&p),
        vec![Problem::ToolValueUnset {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
            field: "step_down_mm",
        }]
    );
    drill.cycle = DrillCycle::Peck {
        peck_depth_mm: Some(1.5),
        retract: PeckRetract::Full,
    };
    let mut p = with_ops(vec![Operation::Drill(drill)]);
    p.tools[0].step_down_mm = None;
    profile_mut(&mut p).step_down_mm = Some(2.0);
    assert_eq!(validate(&p), vec![], "peck depth set");
    let mut p = with_ops(vec![Operation::Drill(sample_drill())]);
    p.tools[0].step_down_mm = None;
    profile_mut(&mut p).step_down_mm = Some(2.0);
    assert_eq!(validate(&p), vec![], "simple cycle");
}

/// Functional spec, capability 16: a helix sits in material the
/// operation removes, and an On line Profile removes only its kerf.
#[test]
fn helix_on_on_line_profile_is_error() {
    let helix = Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 5.0,
    };
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.side = Side::OnLine;
    op.entry = helix.clone();
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::HelixNeedsWasteSide {
            operation: OperationId::new("o1"),
            name: "Cut out".into(),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].field().as_deref(), Some("operations[o1].entry"));
    // #207: Left and Right ramp along the offset path and have no
    // waste area for a helix either.
    for side in [Side::Left, Side::Right] {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.side = side;
        op.entry = helix.clone();
        assert_eq!(
            validate(&p),
            vec![Problem::HelixNeedsWasteSide {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
            }],
            "{side:?}"
        );
    }
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.side = Side::Inside;
    op.entry = helix;
    assert_eq!(validate(&p), vec![]);
    // A Pocket always has waste inside.
    let mut pocket = sample_pocket();
    pocket.entry = Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 5.0,
    };
    assert_eq!(validate(&with_ops(vec![Operation::Pocket(pocket)])), vec![]);
}

/// Functional spec, capability 17: a lead is an arc of a positive
/// radius sweeping at most a half circle, with a non-negative overcut.
#[test]
fn lead_radius_must_be_positive() {
    let good = Lead {
        radius_mm: 2.0,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 0.0,
    };
    let mut p = sample_project();
    profile_mut(&mut p).lead = Some(good.clone());
    assert_eq!(validate(&p), vec![]);
    for (lead, field, value, expected) in [
        (
            Lead {
                radius_mm: 0.0,
                ..good.clone()
            },
            "radius_mm",
            0.0,
            POSITIVE,
        ),
        (
            Lead {
                sweep_deg: 0.0,
                ..good.clone()
            },
            "sweep_deg",
            0.0,
            SWEEP,
        ),
        (
            Lead {
                sweep_deg: 181.0,
                ..good.clone()
            },
            "sweep_deg",
            181.0,
            SWEEP,
        ),
        (
            Lead {
                overcut_mm: -1.0,
                ..good.clone()
            },
            "overcut_mm",
            -1.0,
            NON_NEGATIVE,
        ),
    ] {
        let mut p = sample_project();
        profile_mut(&mut p).lead = Some(lead);
        assert_eq!(
            validate(&p),
            vec![Problem::OutOfRange {
                field: format!("operations[o1].lead.{field}"),
                value,
                expected,
            }],
            "{field} {value}"
        );
    }
}

/// The last pass acceptance 46 asks for, on the sample's Outside
/// Profile (#284).
fn last_pass(stock_to_leave_mm: f64) -> LastPass {
    LastPass {
        stock_to_leave_mm,
        thickness_mm: None,
        reverse: false,
    }
}

/// #284 (capability 34, acceptance 46): the stock to leave is more than 0
/// and at most a third of the bit's diameter, as VCarve advises. 0, a
/// negative value and 2.2 mm with the 6.35 mm bit are errors naming the
/// diameter and the limit; 0.5 and exactly a third are valid.
#[test]
fn last_pass_stock_to_leave_limit() {
    let mut p = sample_project();
    for stock_to_leave_mm in [0.5, 6.35 / 3.0] {
        profile_mut(&mut p).last_pass = Some(last_pass(stock_to_leave_mm));
        assert_eq!(validate(&p), vec![], "{stock_to_leave_mm}");
    }
    for stock_to_leave_mm in [0.0, -0.5, 2.2] {
        profile_mut(&mut p).last_pass = Some(last_pass(stock_to_leave_mm));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::LastPassStockToLeave {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
                tool_name: "6.35 mm upcut".into(),
                stock_to_leave_mm,
                diameter_mm: 6.35,
                limit_mm: 6.35 / 3.0,
            }],
            "{stock_to_leave_mm}"
        );
        let problem = &problems[0];
        assert!(problem.is_error());
        assert_eq!(problem.code(), "last_pass_stock_to_leave");
        assert_eq!(
            problem.field().as_deref(),
            Some("operations[o1].last_pass.stock_to_leave_mm")
        );
        let message = problem.to_string();
        for part in ["6.35 mm", "2.117 mm", "a third"] {
            assert!(message.contains(part), "{message}");
        }
    }
    profile_mut(&mut p).last_pass = Some(last_pass(f64::NAN));
    assert_eq!(
        validate(&p),
        vec![Problem::NonFinite {
            field: "operations[o1].last_pass.stock_to_leave_mm".into()
        }]
    );
}

/// #284: On line has no wall to leave material on: it cuts with both
/// flanks. (changed in #311: Left and Right, which cut one flank
/// against the part, take a last pass too.)
#[test]
fn last_pass_on_line_is_error() {
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.side = Side::OnLine;
    op.last_pass = Some(last_pass(0.5));
    let problems = validate(&p);
    let expected = Problem::LastPassNeedsSide {
        operation: OperationId::new("o1"),
        name: "Cut out".into(),
        side: Side::OnLine,
    };
    assert!(problems.contains(&expected), "{problems:?}");
    assert!(expected.is_error());
    assert_eq!(expected.code(), "last_pass_needs_side");
    assert_eq!(
        expected.field().as_deref(),
        Some("operations[o1].last_pass")
    );
    let message = expected.to_string();
    assert!(
        message.contains("Inside, Outside, Left or Right"),
        "{message}"
    );
    for side in [Side::Inside, Side::Outside] {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.side = side;
        op.last_pass = Some(last_pass(0.5));
        assert_eq!(validate(&p), vec![], "{side:?}");
    }
}

/// #311: a Left or Right Profile cuts one flank of the bit against the
/// part, as Inside and Outside do, so it takes a last pass: no
/// `LastPassNeedsSide`, and the stock to leave and thickness are checked
/// as on Inside and Outside. On line still errors.
#[test]
fn last_pass_left_right_is_valid() {
    for side in [Side::Left, Side::Right] {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.side = side;
        op.tabs = None;
        op.last_pass = Some(last_pass(0.5));
        assert_eq!(validate(&p), vec![], "{side:?}");

        profile_mut(&mut p).last_pass = Some(last_pass(2.2));
        let problems = validate(&p);
        assert!(
            problems
                .iter()
                .any(|q| matches!(q, Problem::LastPassStockToLeave { .. })),
            "{side:?}: {problems:?}"
        );
        assert!(
            !problems
                .iter()
                .any(|q| matches!(q, Problem::LastPassNeedsSide { .. })),
            "{side:?}: {problems:?}"
        );
    }
    let mut p = sample_project();
    let op = profile_mut(&mut p);
    op.side = Side::OnLine;
    op.last_pass = Some(last_pass(0.5));
    assert!(
        validate(&p)
            .iter()
            .any(|q| matches!(q, Problem::LastPassNeedsSide { .. }))
    );
}

/// #284: `thickness_mm`, the last pass's depth, is more than 0 and at
/// most the operation's `depth_mm`.
#[test]
fn last_pass_thickness_range() {
    let mut p = sample_project();
    for thickness_mm in [0.5, 2.0] {
        profile_mut(&mut p).last_pass = Some(LastPass {
            thickness_mm: Some(thickness_mm),
            ..last_pass(0.5)
        });
        assert_eq!(validate(&p), vec![], "{thickness_mm}");
    }
    for thickness_mm in [0.0, -1.0, 6.5] {
        profile_mut(&mut p).last_pass = Some(LastPass {
            thickness_mm: Some(thickness_mm),
            ..last_pass(0.5)
        });
        assert_eq!(
            validate(&p),
            vec![Problem::OutOfRange {
                field: "operations[o1].last_pass.thickness_mm".into(),
                value: thickness_mm,
                expected: LAST_PASS_THICKNESS,
            }],
            "{thickness_mm}"
        );
    }
    // With the depth unset, only the depth is reported.
    let op = profile_mut(&mut p);
    op.depth_mm = None;
    op.last_pass = Some(LastPass {
        thickness_mm: Some(6.5),
        ..last_pass(0.5)
    });
    assert!(
        validate(&p)
            .iter()
            .all(|problem| !matches!(problem, Problem::OutOfRange { .. })),
        "{:?}",
        validate(&p)
    );
}

/// #310 review: the last pass is at most one step-down thick, the
/// operation's when it sets one, else the tool's: the sample's 2 mm
/// tool step-down allows 2 mm and refuses 2.5 and 6, at the thickness;
/// with the operation's 4 mm, 3 is fine.
#[test]
fn last_pass_thickness_at_most_step_down() {
    let mut p = sample_project();
    for (thickness_mm, step_down_mm) in [(2.5, None), (6.0, None), (4.5, Some(4.0))] {
        let op = profile_mut(&mut p);
        op.step_down_mm = step_down_mm;
        op.last_pass = Some(LastPass {
            thickness_mm: Some(thickness_mm),
            ..last_pass(0.5)
        });
        let problems = validate(&p);
        let (field, step) = match step_down_mm {
            Some(step) => ("operations[o1].step_down_mm", step),
            None => ("tools[t1].step_down_mm", 2.0),
        };
        let expected = Problem::LastPassThickerThanStepDown {
            operation: OperationId::new("o1"),
            name: "Cut out".into(),
            thickness_mm,
            step_down_mm: step,
            step_down_field: field.into(),
        };
        assert_eq!(problems, vec![expected.clone()], "{thickness_mm}");
        assert!(expected.is_error());
        assert_eq!(expected.code(), "last_pass_thicker_than_step_down");
        assert_eq!(
            expected.field().as_deref(),
            Some("operations[o1].last_pass.thickness_mm")
        );
        let message = expected.to_string();
        assert!(
            message.contains(field) && message.contains("step-down"),
            "{message}"
        );
    }
    let op = profile_mut(&mut p);
    op.step_down_mm = Some(4.0);
    op.last_pass = Some(LastPass {
        thickness_mm: Some(3.0),
        ..last_pass(0.5)
    });
    assert_eq!(validate(&p), vec![]);
}

/// Functional spec, capability 16: a ramp steeper than 45° is a
/// plunge in all but name; a helix is at most the tool's diameter.
#[test]
fn ramp_angle_range() {
    for (entry, field, value, expected) in [
        (
            Entry::Ramp { angle_deg: 0.0 },
            "entry.ramp.angle_deg",
            0.0,
            ENTRY_ANGLE,
        ),
        (
            Entry::Ramp { angle_deg: 46.0 },
            "entry.ramp.angle_deg",
            46.0,
            ENTRY_ANGLE,
        ),
        (
            Entry::Helix {
                diameter_fraction: 0.0,
                angle_deg: 5.0,
            },
            "entry.helix.diameter_fraction",
            0.0,
            FRACTION,
        ),
        (
            Entry::Helix {
                diameter_fraction: 1.1,
                angle_deg: 5.0,
            },
            "entry.helix.diameter_fraction",
            1.1,
            FRACTION,
        ),
        (
            Entry::Helix {
                diameter_fraction: 0.8,
                angle_deg: 50.0,
            },
            "entry.helix.angle_deg",
            50.0,
            ENTRY_ANGLE,
        ),
    ] {
        let mut pocket = sample_pocket();
        pocket.entry = entry.clone();
        let p = with_ops(vec![Operation::Pocket(pocket)]);
        assert_eq!(
            validate(&p),
            vec![Problem::OutOfRange {
                field: format!("operations[o2].{field}"),
                value,
                expected,
            }],
            "{entry:?}"
        );
    }
    for entry in [
        Entry::Ramp { angle_deg: 45.0 },
        Entry::Ramp { angle_deg: 15.0 },
        Entry::Helix {
            diameter_fraction: 1.0,
            angle_deg: 45.0,
        },
    ] {
        let mut p = sample_project();
        profile_mut(&mut p).entry = entry.clone();
        assert_eq!(validate(&p), vec![], "{entry:?}");
    }
}

/// Functional spec, capability 14: canned cycles are written only
/// where the definition has them; asking for them elsewhere would be
/// silently ignored, so it is an error.
#[test]
fn canned_cycles_on_definition_without_them_is_error() {
    let mut p = sample_project();
    p.post_processor.use_canned_cycles = true;
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::CannedCyclesUnsupported {
            post: "grbl".into()
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(
        problems[0].field().as_deref(),
        Some("post_processor.use_canned_cycles")
    );
}

/// Functional spec, capability 14: Drill plunges any cutter, but a
/// V-bit or ball nose makes a cone, not a hole, so it is a warning.
#[test]
fn drill_with_v_bit_warns() {
    let mut p = with_ops(vec![Operation::Drill(sample_drill())]);
    // The Profile needs an end mill; let the drill have its own tool.
    let mut v_bit = tool("t2");
    v_bit.cutter = ToolKind::VBit {
        included_angle_deg: 60.0,
    };
    p.tools.push(v_bit);
    if let Some(Operation::Drill(drill)) = p.operations.get_mut(1) {
        drill.tool = ToolId::new("t2");
    }
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::DrillCutterUnusual {
            operation: OperationId::new("o3"),
            name: "Drill".into(),
            tool: ToolId::new("t2"),
            tool_name: "tool t2".into(),
            cutter: "a V-bit",
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(problems[0].field().as_deref(), Some("operations[o3].tool"));
    for cutter in [ToolKind::EndMill, ToolKind::Drill] {
        if let Some(t2) = p.tools.get_mut(1) {
            t2.cutter = cutter;
        }
        assert_eq!(validate(&p), vec![]);
    }
}

/// Functional spec, capability 18: the relief pass steps down at the
/// tool's step-down; without one it is skipped, which the user should
/// know. The Profile's own step-down keeps the main passes valid.
#[test]
fn relief_without_step_down_warns() {
    let mut p = sample_project();
    p.tools[0].step_down_mm = None;
    let op = profile_mut(&mut p);
    op.step_down_mm = Some(2.0);
    op.corner_relief = CornerRelief::Dogbone;
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ReliefNeedsStepDown {
            operation: OperationId::new("o1"),
            name: "Cut out".into(),
            tool: ToolId::new("t1"),
            tool_name: "6.35 mm upcut".into(),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o1].corner_relief")
    );
    // A Pocket, likewise.
    let mut pocket = sample_pocket();
    pocket.step_down_mm = Some(2.0);
    pocket.corner_relief = CornerRelief::TBone;
    let mut p = with_ops(vec![Operation::Pocket(pocket)]);
    p.tools[0].step_down_mm = None;
    profile_mut(&mut p).step_down_mm = Some(2.0);
    let problems = validate(&p);
    assert!(
        matches!(problems.as_slice(), [Problem::ReliefNeedsStepDown { operation, .. }]
            if operation == &OperationId::new("o2")),
        "{problems:?}"
    );
    // With the tool's step-down set, nothing.
    p.tools[0].step_down_mm = Some(2.0);
    assert_eq!(validate(&p), vec![]);
}

/// Functional spec, capability 15: a V-bit engraving deeper than its
/// cone is tall cuts with the shank. A 60° bit 6 mm across has a
/// cone 3 / tan 30° = 5.196 mm tall.
#[test]
fn engrave_deeper_than_v_bit_cone_warns() {
    let mut p = with_ops(vec![Operation::Engrave(sample_engrave())]);
    let mut v_bit = tool("t2");
    v_bit.diameter_mm = 6.0;
    v_bit.cutter = ToolKind::VBit {
        included_angle_deg: 60.0,
    };
    p.tools.push(v_bit);
    if let Some(Operation::Engrave(engrave)) = p.operations.get_mut(1) {
        engrave.tool = ToolId::new("t2");
        engrave.depth_mm = Some(6.0);
    }
    let problems = validate(&p);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let Problem::EngraveDeeperThanCone {
        operation,
        name,
        tool_name,
        depth_mm,
        cone_height_mm,
    } = &problems[0]
    else {
        panic!("{problems:?}");
    };
    assert_eq!(operation, &OperationId::new("o4"));
    assert_eq!(name, "Engrave");
    assert_eq!(tool_name, "tool t2");
    assert!(approx_eq(*depth_mm, 6.0, 1e-9));
    assert!(approx_eq(*cone_height_mm, 5.196, 1e-3), "{cone_height_mm}");
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o4].depth_mm")
    );
    if let Some(Operation::Engrave(engrave)) = p.operations.get_mut(1) {
        engrave.depth_mm = Some(5.0);
    }
    assert_eq!(validate(&p), vec![]);
}

/// #67 applies to Pocket as to Profile: its rings are the wall offset
/// by half the diameter, which only a flat end mill cuts at every
/// depth.
#[test]
fn pocket_with_v_bit_is_error() {
    let mut p = with_ops(vec![Operation::Pocket(sample_pocket())]);
    let mut v_bit = tool("t2");
    v_bit.cutter = ToolKind::VBit {
        included_angle_deg: 60.0,
    };
    p.tools.push(v_bit);
    if let Some(Operation::Pocket(pocket)) = p.operations.get_mut(1) {
        pocket.tool = ToolId::new("t2");
    }
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::CutterNotFlat {
            operation: OperationId::new("o2"),
            name: "Pocket".into(),
            tool: ToolId::new("t2"),
            tool_name: "tool t2".into(),
            cutter: "a V-bit",
            field: "tool",
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
}

/// Format 18: a Bore's pitch is at most its tool's step-down, which
/// is an error; at the step-down it is fine.
#[test]
fn bore_pitch_over_step_down_is_error() {
    let mut bore = sample_bore();
    bore.pitch_mm = Some(2.5);
    let p = with_ops(vec![Operation::Bore(bore)]);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::BorePitchOverStepDown {
            operation: OperationId::new("o5"),
            name: "Bore".into(),
            pitch_mm: 2.5,
            step_down_mm: 2.0,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "bore_pitch_over_step_down");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o5].pitch_mm")
    );
    let mut bore = sample_bore();
    bore.pitch_mm = Some(2.0);
    assert_eq!(validate(&with_ops(vec![Operation::Bore(bore)])), vec![]);
    let mut bore = sample_bore();
    bore.pitch_mm = Some(0.0);
    assert_eq!(
        validate(&with_ops(vec![Operation::Bore(bore)])),
        vec![Problem::OutOfRange {
            field: "operations[o5].pitch_mm".into(),
            value: 0.0,
            expected: POSITIVE,
        }]
    );
}

/// Format 18: with no pitch a Bore takes the tool's step-down, so one
/// of the two must be set, as for a Pocket.
#[test]
fn bore_without_pitch_or_step_down_is_error() {
    let mut p = with_ops(vec![Operation::Bore(sample_bore())]);
    p.tools[0].step_down_mm = None;
    profile_mut(&mut p).step_down_mm = Some(2.0);
    assert_eq!(
        validate(&p),
        vec![Problem::ToolValueUnset {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
            field: "step_down_mm",
        }]
    );
    if let Some(Operation::Bore(bore)) = p.operations.get_mut(1) {
        bore.pitch_mm = Some(1.0);
    }
    assert_eq!(validate(&p), vec![]);
}

/// Format 18: the stock to leave is a distance of 0 or more.
#[test]
fn bore_negative_stock_to_leave_is_error() {
    let mut bore = sample_bore();
    bore.stock_to_leave_mm = -0.1;
    assert_eq!(
        validate(&with_ops(vec![Operation::Bore(bore)])),
        vec![Problem::OutOfRange {
            field: "operations[o5].stock_to_leave_mm".into(),
            value: -0.1,
            expected: NON_NEGATIVE,
        }]
    );
}

/// Format 18: a Bore cuts its wall with the side of the bit, so it
/// needs a flat end mill, as a Pocket does.
#[test]
fn bore_needs_flat_end_mill() {
    for cutter in [ToolKind::Drill, ToolKind::BallNose] {
        let mut p = with_ops(vec![Operation::Bore(sample_bore())]);
        let mut t2 = tool("t2");
        t2.cutter = cutter.clone();
        p.tools.push(t2);
        if let Some(Operation::Bore(bore)) = p.operations.get_mut(1) {
            bore.tool = ToolId::new("t2");
        }
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::CutterNotFlat {
                operation: OperationId::new("o5"),
                name: "Bore".into(),
                tool: ToolId::new("t2"),
                tool_name: "tool t2".into(),
                cutter: cutter.describe(),
                field: "tool",
            }],
            "{cutter:?}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
    }
}

/// Functional spec, capability 14: the dwell and retract are
/// distances and times of 0 or more; a chip-break gap of 0 would
/// never clear the chip.
#[test]
fn drill_ranges() {
    let mut dwell = sample_drill();
    dwell.dwell_s = -1.0;
    let mut retract = sample_drill();
    retract.retract_mm = -0.5;
    let mut gap = sample_drill();
    gap.cycle = DrillCycle::Peck {
        peck_depth_mm: Some(1.0),
        retract: PeckRetract::ChipBreak { gap_mm: 0.0 },
    };
    let mut peck = sample_drill();
    peck.cycle = DrillCycle::Peck {
        peck_depth_mm: Some(0.0),
        retract: PeckRetract::Full,
    };
    for (drill, field, value, expected) in [
        (dwell, "dwell_s", -1.0, NON_NEGATIVE),
        (retract, "retract_mm", -0.5, NON_NEGATIVE),
        (gap, "cycle.peck.retract.chip_break.gap_mm", 0.0, POSITIVE),
        (peck, "cycle.peck.peck_depth_mm", 0.0, POSITIVE),
    ] {
        let p = with_ops(vec![Operation::Drill(drill)]);
        assert_eq!(
            validate(&p),
            vec![Problem::OutOfRange {
                field: format!("operations[o3].{field}"),
                value,
                expected,
            }],
            "{field}"
        );
    }
    let mut stock = sample_pocket();
    stock.stock_to_leave_mm = -0.1;
    let p = with_ops(vec![Operation::Pocket(stock)]);
    assert_eq!(
        validate(&p),
        vec![Problem::OutOfRange {
            field: "operations[o2].stock_to_leave_mm".into(),
            value: -0.1,
            expected: NON_NEGATIVE,
        }]
    );
}

/// Functional spec, capability 12: the raster angle is from the X
/// axis, 0 to 180 exclusive.
#[test]
fn raster_angle_range() {
    for (angle_deg, ok) in [(-1.0, false), (180.0, false), (0.0, true), (179.0, true)] {
        let mut pocket = sample_pocket();
        pocket.strategy = ClearanceStrategy::Raster { angle_deg };
        let p = with_ops(vec![Operation::Pocket(pocket)]);
        let problems = validate(&p);
        if ok {
            assert_eq!(problems, vec![], "{angle_deg}");
        } else {
            assert_eq!(
                problems,
                vec![Problem::OutOfRange {
                    field: "operations[o2].strategy.raster.angle_deg".into(),
                    value: angle_deg,
                    expected: RASTER_ANGLE,
                }],
                "{angle_deg}"
            );
        }
    }
}

/// Every new problem names a field the saved file has, so `check` can
/// point at a line and column.
#[test]
fn phase_2_problem_fields_locate_in_the_saved_file() {
    let mut p = sample_project();
    p.tools[0].step_over_fraction = Some(2.0);
    p.post_processor.use_canned_cycles = true;
    let op = profile_mut(&mut p);
    op.side = Side::OnLine;
    op.entry = Entry::Helix {
        diameter_fraction: 1.5,
        angle_deg: 5.0,
    };
    op.lead = Some(Lead {
        radius_mm: -1.0,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 0.0,
    });
    let mut pocket = sample_pocket();
    pocket.rest_of = Some(OperationId::new("nope"));
    pocket.step_over_fraction = Some(2.0);
    pocket.strategy = ClearanceStrategy::Raster { angle_deg: 180.0 };
    pocket.corner_relief = CornerRelief::Dogbone;
    pocket.step_down_mm = Some(1.0);
    let mut t2 = tool("t2");
    t2.step_down_mm = None;
    t2.number = Some(1);
    p.tools.push(t2);
    pocket.tool = ToolId::new("t2");
    let mut drill = sample_drill();
    drill.dwell_s = -1.0;
    let mut engrave = sample_engrave();
    engrave.depth_mm = Some(0.0);
    p.operations.extend([
        Operation::Pocket(pocket),
        Operation::Drill(drill),
        Operation::Engrave(engrave),
    ]);
    let problems = validate(&p);
    let text = crate::save(&p).unwrap();
    let map = crate::SourceMap::parse(&text).unwrap();
    let mut kinds: Vec<String> = Vec::new();
    for problem in &problems {
        let field = problem.field().unwrap();
        assert!(map.locate(&field).is_some(), "{field} in {text}");
        kinds.push(
            format!("{problem:?}")
                .split([' ', '{'])
                .next()
                .unwrap()
                .to_owned(),
        );
    }
    for expected in [
        "ToolOutOfRange",
        "DuplicateToolNumber",
        "CannedCyclesUnsupported",
        "HelixNeedsWasteSide",
        "RestOfInvalid",
        "ReliefNeedsStepDown",
        "OutOfRange",
    ] {
        assert!(
            kinds.contains(&expected.to_owned()),
            "{expected} in {kinds:?}"
        );
    }
}

// #283: the clearance, tool-change and park heights.

/// Design addendum A7: the clearance height is for links between
/// parts, so it may not be below an enabled operation's retract
/// between passes. `clearance_mm: 3` with the sample Profile's
/// `safe_z_mm: 5` names the operation, at the clearance field; 5 is
/// valid, and a disabled operation does not count.
#[test]
fn clearance_below_safe_z_is_error() {
    let mut p = sample_project();
    p.heights.clearance_mm = Some(3.0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ClearanceBelowSafeZ {
            operation: OperationId::new("o1"),
            name: "Cut out".into(),
            clearance_mm: 3.0,
            safe_z_mm: 5.0,
        }]
    );
    assert!(problems[0].is_error());
    assert_eq!(problems[0].field().as_deref(), Some("heights.clearance_mm"));
    assert!(
        problems[0].to_string().contains("Cut out"),
        "{}",
        problems[0]
    );
    let text = crate::save(&p).unwrap();
    let map = crate::SourceMap::parse(&text).unwrap();
    assert!(map.locate("heights.clearance_mm").is_some(), "{text}");

    p.heights.clearance_mm = Some(5.0);
    assert_eq!(validate(&p), vec![]);
    p.heights.clearance_mm = Some(3.0);
    profile_mut(&mut p).enabled = false;
    assert!(
        !validate(&p)
            .iter()
            .any(|q| matches!(q, Problem::ClearanceBelowSafeZ { .. })),
        "a disabled operation is not cut"
    );
}

/// Design addendum A7: grblHAL and GRBL home Z at the top of its
/// travel, to a negative machine position (grblHAL `machine_limits.c`
/// 116-140, GRBL `limits.c` 382), so a lift to machine Z 0 or above
/// goes past the limit switch. `0` and `2` are errors; `-5` is valid.
#[test]
fn machine_z_must_be_below_zero() {
    for machine_z_mm in [0.0, 2.0] {
        let mut p = sample_project();
        p.heights.tool_change_machine_z_mm = Some(machine_z_mm);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::MachineZNotBelowHome { machine_z_mm }],
            "{machine_z_mm}"
        );
        assert!(problems[0].is_error());
        assert_eq!(
            problems[0].field().as_deref(),
            Some("heights.tool_change_machine_z_mm")
        );
    }
    let mut p = sample_project();
    p.heights.tool_change_machine_z_mm = Some(-5.0);
    assert_eq!(validate(&p), vec![]);
}

/// #304 review: the program writes the machine Z to its decimal places
/// (3 in mm, 4 in inches), so one that rounds to 0 there is the same
/// lift past the limit switch: −0.0004 mm is refused in a mm program,
/// −0.001 mm in an inch program (under half of 0.0001 in, 0.00127
/// mm), while −0.001 mm and −0.0013 mm write −0.001 and −0.0001.
#[test]
fn machine_z_rounding_to_zero_is_error() {
    for (units, machine_z_mm, refused) in [
        (crate::Units::Mm, -0.0004, true),
        (crate::Units::Mm, -0.001, false),
        (crate::Units::Inch, -0.001, true),
        (crate::Units::Inch, -0.0013, false),
    ] {
        let mut p = sample_project();
        p.display_units = units;
        p.heights.tool_change_machine_z_mm = Some(machine_z_mm);
        let problems = validate(&p);
        let expected = if refused {
            vec![Problem::MachineZNotBelowHome { machine_z_mm }]
        } else {
            vec![]
        };
        assert_eq!(problems, expected, "{units:?} {machine_z_mm}");
    }
}

/// Design addendum A7: the lift is `G53 G0 Z…`, which only a
/// definition that documents `G53` may write (`definitions/README.md`,
/// "Machine-coordinate moves"). On LinuxCNC, whose documents were not
/// checked for it, the setting is an error naming the post-processor;
/// on GRBL (the sample's) it is valid.
#[test]
fn machine_z_needs_g53_definition() {
    // #306: Mach3, UCCNC and generic document no `G53` that DreamCarve
    // could check, on any version.
    for definition in crate::bundled_definitions()
        .unwrap()
        .iter()
        .filter(|d| ["mach3", "uccnc", "generic"].contains(&d.id))
    {
        let mut p = sample_project();
        p.heights.tool_change_machine_z_mm = Some(-5.0);
        p.post_processor = definition.default_ref();
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::MachineZNeedsG53 {
                post: definition.id.into()
            }],
            "{}",
            definition.path
        );
        assert!(problems[0].is_error());
        assert!(
            problems[0].to_string().contains(definition.id),
            "{}",
            problems[0]
        );
        assert_eq!(
            problems[0].field().as_deref(),
            Some("heights.tool_change_machine_z_mm")
        );
    }
}

/// #306: LinuxCNC documents `G53` (definitions README,
/// "Machine-coordinate moves"), so both versions accept a machine Z,
/// with the same rule as grblHAL and GRBL: below 0 as written.
#[test]
fn machine_z_accepted_on_linuxcnc() {
    for version in [1, 2] {
        let mut p = sample_project();
        p.post_processor = crate::PostProcessorRef::latest("linuxcnc").unwrap();
        p.post_processor.definition_version = version;
        p.heights.tool_change_machine_z_mm = Some(-5.0);
        assert_eq!(validate(&p), vec![], "v{version}");
        for machine_z_mm in [0.0, 5.0, -0.0004] {
            p.heights.tool_change_machine_z_mm = Some(machine_z_mm);
            assert_eq!(
                validate(&p),
                vec![Problem::MachineZNotBelowHome { machine_z_mm }],
                "v{version} {machine_z_mm}"
            );
        }
    }
}

/// #283: the park point is a coordinate, within the 6000 mm limit
/// (#75); the clearance height is positive.
#[test]
fn park_and_clearance_are_in_range() {
    let mut p = sample_project();
    p.heights.park = Some(crate::ParkXy {
        x_mm: 7000.0,
        y_mm: 0.0,
    });
    let problems = validate(&p);
    assert!(
        matches!(problems.as_slice(), [Problem::OutOfRange { field, .. }] if field == "heights.park.x_mm"),
        "{problems:?}"
    );
    p.heights.park = Some(crate::ParkXy {
        x_mm: 0.0,
        y_mm: 0.0,
    });
    assert_eq!(validate(&p), vec![]);
    profile_mut(&mut p).enabled = false;
    p.heights.clearance_mm = Some(-1.0);
    let problems = validate(&p);
    assert!(
        problems.iter().any(
            |q| matches!(q, Problem::OutOfRange { field, .. } if field == "heights.clearance_mm")
        ),
        "{problems:?}"
    );
}

// Format 20: the V-Carve (`docs/specs/phase-3/design.md`, "Validation").

/// The sample project with the 90°, 6.35 mm V-bit `t2`, a 10 mm
/// square `i2` inside the sample rectangle, and a V-Carve `o6` of the
/// square, as `edit` leaves it.
fn carve_project(edit: impl FnOnce(&mut crate::model::VCarveOp)) -> Project {
    let mut p = sample_project();
    p.tools.push(crate::test_support::tool_v_bit_90());
    let square = DefinitionId::new("d2");
    p.definitions.insert(
        square.clone(),
        crate::model::FeatureDefinition {
            id: square.clone(),
            name: "Square".into(),
            profiles: vec![Polyline::rect(
                dc_geometry::Point { x: 0.0, y: 0.0 },
                10.0,
                10.0,
            )],
        },
    );
    p.instances.push(crate::model::FeatureInstance {
        id: InstanceId::new("i2"),
        definition: square,
        transform: Transform::translate(20.0, 20.0),
        name: "Square 1".into(),
        visible: true,
        locked: false,
        z_top_mm: Some(6.0),
        z_bottom_mm: Some(0.0),
    });
    let mut carve = crate::test_support::sample_v_carve();
    carve.targets = vec![InstanceId::new("i2")];
    edit(&mut carve);
    p.operations.push(Operation::VCarve(carve));
    p
}

/// The carve of [`carve_project`], to edit.
fn carve_mut(p: &mut Project) -> &mut crate::model::VCarveOp {
    p.operations
        .iter_mut()
        .find_map(|op| match op {
            Operation::VCarve(op) => Some(op),
            _ => None,
        })
        .unwrap()
}

/// An enabled Pocket `o2` of the sample rectangle, which contains the
/// square of [`carve_project`], `depth_mm` deep.
fn rectangle_pocket(depth_mm: f64) -> Operation {
    Operation::Pocket(PocketOp {
        depth_mm: Some(depth_mm),
        ..sample_pocket()
    })
}

/// A tool `id` as the sample's, `diameter_mm` wide and cutting with
/// `cutter`.
fn tool_of(id: &str, diameter_mm: f64, cutter: ToolKind) -> Tool {
    Tool {
        diameter_mm,
        cutter,
        ..tool(id)
    }
}

/// The one problem `problems` holds, which must be a
/// [`Problem::VBitConeExceeded`]: its engagement, cone height,
/// whether it counts from the stock top, and its field.
fn cone_exceeded(problems: &[Problem]) -> (f64, f64, bool, Option<String>) {
    match problems {
        [
            problem @ Problem::VBitConeExceeded {
                engagement_mm,
                cone_height_mm,
                from_stock_top,
                ..
            },
        ] => {
            assert_eq!(problem.severity(), Severity::Error);
            assert_eq!(problem.code(), "v_bit_cone_exceeded");
            (
                *engagement_mm,
                *cone_height_mm,
                *from_stock_top,
                problem.field(),
            )
        }
        _ => panic!("expected one v_bit_cone_exceeded: {problems:?}"),
    }
}

/// Capability 38: a V-Carve's depth follows a V-bit's angle, so any
/// other cutter is an error at the operation's tool.
#[test]
fn v_carve_with_end_mill_is_error() {
    assert_eq!(validate(&carve_project(|_| {})), vec![]);
    for cutter in [
        ToolKind::EndMill,
        ToolKind::BallNose,
        ToolKind::Engraver,
        ToolKind::Drill,
    ] {
        let mut p = carve_project(|_| {});
        p.tools[1].cutter = cutter.clone();
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::VCarveNeedsVBit {
                operation: OperationId::new("o6"),
                name: "V-Carve".into(),
                tool: ToolId::new("t2"),
                tool_name: "90° V-bit".into(),
                cutter: cutter.describe(),
                photo: false,
            }],
            "{cutter:?}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "v_carve_needs_v_bit");
        assert_eq!(problems[0].field().as_deref(), Some("operations[o6].tool"));

        // A Photo V-Carve's depth follows the angle too (format 27).
        let mut p = photo_project();
        p.tools[1].cutter = cutter.clone();
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::VCarveNeedsVBit {
                operation: OperationId::new("o8"),
                name: "Photo V-Carve".into(),
                tool: ToolId::new("t2"),
                tool_name: "90° V-bit".into(),
                cutter: cutter.describe(),
                photo: true,
            }],
            "{cutter:?}"
        );
        assert_eq!(problems[0].field().as_deref(), Some("operations[o8].tool"));
    }
    assert_eq!(validate(&photo_project()), vec![]);
}

/// A V-bit a V-Carve uses must have an angle above 0° and below 180°:
/// `v_bit_angle_range` at the tool's angle, instead of the
/// `tool_out_of_range` any other V-bit gets.
#[test]
fn v_bit_angle_out_of_range_is_error() {
    for angle in [0.0, 180.0, 200.0, -10.0] {
        let mut p = carve_project(|carve| carve.flat_depth_mm = Some(1.0));
        p.tools[1].cutter = ToolKind::VBit {
            included_angle_deg: angle,
        };
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::VBitAngleRange {
                tool: ToolId::new("t2"),
                name: "90° V-bit".into(),
                included_angle_deg: angle,
            }],
            "{angle}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "v_bit_angle_range");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("tools[t2].cutter.v_bit.included_angle_deg")
        );
    }
    // A V-bit no V-Carve uses keeps the general range check.
    let mut p = carve_project(|_| {});
    p.operations.pop();
    p.operations.push(Operation::Engrave(EngraveOp {
        tool: ToolId::new("t2"),
        step_down_mm: Some(1.0),
        ..sample_engrave()
    }));
    p.tools[1].cutter = ToolKind::VBit {
        included_angle_deg: 180.0,
    };
    assert_eq!(
        validate(&p),
        vec![Problem::ToolOutOfRange {
            tool: ToolId::new("t2"),
            name: "90° V-bit".into(),
            field: "cutter.v_bit.included_angle_deg",
            value: 180.0,
            expected: ANGLE,
        }]
    );
}

/// Capability 39: a flat depth deeper than the bit's cone is tall
/// would cut with the shank. The 90°, 6.35 mm bit's cone is 3.175 mm.
#[test]
fn flat_depth_beyond_cone_is_error() {
    let p = carve_project(|carve| carve.flat_depth_mm = Some(3.5));
    let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&validate(&p));
    assert!(approx_eq(engagement_mm, 3.5, 1e-9), "{engagement_mm}");
    assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
    assert!(!from_stock_top);
    assert_eq!(field.as_deref(), Some("operations[o6].flat_depth_mm"));
    let message = validate(&p)[0].to_string();
    assert!(message.contains("3.175 mm"), "{message}");

    for (flat_depth_mm, angle) in [(3.0, 90.0), (3.175, 90.0), (3.5, 60.0)] {
        let mut p = carve_project(|carve| carve.flat_depth_mm = Some(flat_depth_mm));
        p.tools[1].cutter = ToolKind::VBit {
            included_angle_deg: angle,
        };
        assert_eq!(validate(&p), vec![], "{flat_depth_mm} mm, {angle}°");
    }
    // The flat depth is a depth, more than 0.
    let p = carve_project(|carve| carve.flat_depth_mm = Some(0.0));
    assert_eq!(
        validate(&p),
        vec![Problem::OutOfRange {
            field: "operations[o6].flat_depth_mm".into(),
            value: 0.0,
            expected: POSITIVE,
        }]
    );
}

fn pass(tool: &str) -> crate::model::ClearingPass {
    crate::model::ClearingPass {
        tool: ToolId::new(tool),
        strategy: ClearanceStrategy::Offset,
        direction: crate::model::CutDirection::Climb,
        entry: Entry::Plunge,
    }
}

/// Capability 40: clearing passes clear the flat floor, so a carve with
/// none may not have any.
#[test]
fn clearing_needs_a_flat_depth() {
    let p = carve_project(|carve| carve.clearing = vec![pass("t1")]);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ClearingNeedsFlatDepth {
            operation: OperationId::new("o6"),
            name: "V-Carve".into(),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "clearing_needs_flat_depth");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o6].clearing")
    );
    let p = carve_project(|carve| {
        carve.clearing = vec![pass("t1")];
        carve.flat_depth_mm = Some(1.0);
    });
    assert_eq!(validate(&p), vec![]);
}

/// Capability 40: a clearing pass is cut as a Pocket, so its tool must
/// be a flat end mill; the carve's own V-bit is not one.
#[test]
fn clearing_pass_needs_a_flat_end_mill() {
    for cutter in [
        ToolKind::BallNose,
        ToolKind::Drill,
        ToolKind::VBit {
            included_angle_deg: 60.0,
        },
    ] {
        let mut p = carve_project(|carve| {
            carve.clearing = vec![pass("t1"), pass("t3")];
            carve.flat_depth_mm = Some(1.0);
        });
        p.tools.push(tool_of("t3", 3.175, cutter.clone()));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ClearingNeedsEndMill {
                operation: OperationId::new("o6"),
                name: "V-Carve".into(),
                pass: 1,
                tool: ToolId::new("t3"),
                tool_name: "tool t3".into(),
                cutter: cutter.describe(),
            }],
            "{cutter:?}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "clearing_needs_end_mill");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o6].clearing[1].tool")
        );
    }
    // The carve's own V-bit, given the step-down a pass steps at.
    let mut p = carve_project(|carve| {
        carve.clearing = vec![pass("t2")];
        carve.flat_depth_mm = Some(1.0);
    });
    p.tools[1].step_down_mm = Some(1.0);
    assert!(
        matches!(
            validate(&p).as_slice(),
            [Problem::ClearingNeedsEndMill {
                pass: 0,
                cutter: "a V-bit",
                ..
            }]
        ),
        "{:?}",
        validate(&p)
    );
}

/// Capability 40: a pass whose tool is wider than the one before still
/// runs, so it is a warning. A clearing tool is a used tool: its rates
/// and step-down, which the pass steps down at, are required.
#[test]
fn clearing_wider_than_the_pass_before_warns() {
    let mut p = carve_project(|carve| {
        carve.clearing = vec![pass("t3"), pass("t1")];
        carve.flat_depth_mm = Some(1.0);
    });
    p.tools.push(tool_of("t3", 3.175, ToolKind::EndMill));
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ClearingNotDescending {
            operation: OperationId::new("o6"),
            name: "V-Carve".into(),
            pass: 1,
            tool_name: "6.35 mm upcut".into(),
            diameter_mm: 6.35,
            previous_tool_name: "tool t3".into(),
            previous_diameter_mm: 3.175,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(problems[0].code(), "clearing_not_descending");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o6].clearing[1].tool")
    );
    // Largest first, or the same size twice, is quiet.
    carve_mut(&mut p).clearing = vec![pass("t1"), pass("t3"), pass("t3")];
    assert_eq!(validate(&p), vec![]);

    p.tools[2].step_down_mm = None;
    p.tools[2].feed_mm_min = None;
    assert_eq!(
        validate(&p),
        vec![
            Problem::ToolValueUnset {
                tool: ToolId::new("t3"),
                name: "tool t3".into(),
                field: "feed_mm_min",
            },
            Problem::ToolValueUnset {
                tool: ToolId::new("t3"),
                name: "tool t3".into(),
                field: "step_down_mm",
            },
        ]
    );
    // A clearing pass's strategy and entry follow the Pocket's rules.
    let p = carve_project(|carve| {
        carve.clearing = vec![crate::model::ClearingPass {
            strategy: ClearanceStrategy::Raster { angle_deg: 180.0 },
            entry: Entry::Ramp { angle_deg: 60.0 },
            ..pass("t1")
        }];
        carve.flat_depth_mm = Some(1.0);
    });
    assert_eq!(
        validate(&p),
        vec![
            Problem::OutOfRange {
                field: "operations[o6].clearing[0].strategy.raster.angle_deg".into(),
                value: 180.0,
                expected: RASTER_ANGLE,
            },
            Problem::OutOfRange {
                field: "operations[o6].clearing[0].entry.ramp.angle_deg".into(),
                value: 60.0,
                expected: ENTRY_ANGLE,
            },
        ]
    );
}

/// A clearing pass names a tool of the project, as an operation does.
#[test]
fn clearing_pass_naming_no_tool_is_error() {
    let p = carve_project(|carve| {
        carve.clearing = vec![pass("t9")];
        carve.flat_depth_mm = Some(1.0);
    });
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::ClearingDanglingTool {
            operation: OperationId::new("o6"),
            pass: 0,
            tool: ToolId::new("t9"),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "clearing_dangling_tool");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o6].clearing[0].tool")
    );
}

/// Capability 41: the start plane is a depth below the stock top.
#[test]
fn negative_start_depth_is_error() {
    let p = carve_project(|carve| carve.start_depth_mm = -1.0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::StartDepthNegative {
            operation: OperationId::new("o6"),
            name: "V-Carve".into(),
            start_depth_mm: -1.0,
            field: "start_depth_mm",
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "start_depth_negative");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o6].start_depth_mm")
    );
    let p = carve_project(|carve| carve.start_depth_mm = f64::NAN);
    assert_eq!(
        validate(&p),
        vec![Problem::NonFinite {
            field: "operations[o6].start_depth_mm".into(),
        }]
    );
}

/// Capability 41, security review M2: a carve below the stock top
/// needs an enabled earlier Pocket containing its targets, or the
/// user's word that another program cleared the start plane. A
/// disabled Pocket, or one after the carve, does not count.
#[test]
fn uncleared_start_depth_is_an_error_unless_acknowledged() {
    let uncleared = |p: &Project| {
        validate(p)
            .into_iter()
            .filter(|problem| matches!(problem, Problem::StartDepthUncleared { .. }))
            .collect::<Vec<_>>()
    };
    let p = carve_project(|carve| {
        carve.start_depth_mm = 1.0;
        carve.flat_depth_mm = Some(1.0);
    });
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::StartDepthUncleared {
            operation: OperationId::new("o6"),
            name: "V-Carve".into(),
            start_depth_mm: 1.0,
            field: "start_depth_mm",
            footprint: StartFootprint::Targets,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "start_depth_uncleared");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o6].start_depth_mm")
    );

    let mut acknowledged = p.clone();
    carve_mut(&mut acknowledged).start_plane_cleared_externally = true;
    assert_eq!(validate(&acknowledged), vec![]);

    // A containing Pocket at the depth, before the carve.
    let mut pocketed = p.clone();
    pocketed.operations.insert(1, rectangle_pocket(1.0));
    assert_eq!(validate(&pocketed), vec![]);

    let mut disabled = pocketed.clone();
    if let Some(Operation::Pocket(pocket)) = disabled.operations.get_mut(1) {
        pocket.enabled = false;
    }
    assert_eq!(uncleared(&disabled).len(), 1, "{:?}", validate(&disabled));

    let mut after = p.clone();
    after.operations.push(rectangle_pocket(1.0));
    assert_eq!(uncleared(&after).len(), 1, "{:?}", validate(&after));

    // A Pocket that does not contain the square: a 10 mm square
    // elsewhere.
    let mut elsewhere = pocketed.clone();
    elsewhere.instances[0].transform = Transform::translate(40.0, 50.0);
    assert_eq!(uncleared(&elsewhere).len(), 1, "{:?}", validate(&elsewhere));

    // From the stock top, nothing needs clearing.
    let p = carve_project(|carve| carve.flat_depth_mm = Some(1.0));
    assert_eq!(validate(&p), vec![]);

    // A Photo V-Carve (format 27): its footprint is the picture's
    // rectangle, 40 × 20 mm at (15, 15), inside the sample rectangle
    // the Pocket clears.
    let p = photo_with(|photo| photo.start_depth_mm = 1.0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::StartDepthUncleared {
            operation: OperationId::new("o8"),
            name: "Photo V-Carve".into(),
            start_depth_mm: 1.0,
            field: "start_depth_mm",
            footprint: StartFootprint::Picture,
        }]
    );
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o8].start_depth_mm")
    );

    let mut acknowledged = p.clone();
    photo_mut(&mut acknowledged).start_plane_cleared_externally = true;
    assert_eq!(validate(&acknowledged), vec![]);

    let mut pocketed = p.clone();
    pocketed.operations.insert(1, rectangle_pocket(1.0));
    assert_eq!(validate(&pocketed), vec![]);

    let mut disabled = pocketed.clone();
    if let Some(Operation::Pocket(pocket)) = disabled.operations.get_mut(1) {
        pocket.enabled = false;
    }
    assert_eq!(uncleared(&disabled).len(), 1, "{:?}", validate(&disabled));

    let mut after = p.clone();
    after.operations.push(rectangle_pocket(1.0));
    assert_eq!(uncleared(&after).len(), 1, "{:?}", validate(&after));

    // The picture moved so its right half is past the Pocket's wall.
    let mut outside = pocketed.clone();
    outside.bitmaps[0].x_mm = 40.0;
    assert_eq!(uncleared(&outside).len(), 1, "{:?}", validate(&outside));

    // A Photo V-Carve that names no bitmap has no footprint to hold.
    let mut unknown = pocketed.clone();
    photo_mut(&mut unknown).image = crate::ids::BitmapId::new("b9");
    assert_eq!(uncleared(&unknown).len(), 1, "{:?}", validate(&unknown));
}

/// Security review H4: with the start plane only vouched for, the bit's
/// flanks engage from the stock top, so the start depth counts toward
/// the cone: 4 + 1 mm with the 90° bit's 3.175 mm cone fails, while
/// a Pocket of the start depth makes it 1 mm and passes.
#[test]
fn acknowledged_start_depth_counts_toward_the_cone() {
    let p = carve_project(|carve| {
        carve.start_depth_mm = 4.0;
        carve.start_plane_cleared_externally = true;
        carve.flat_depth_mm = Some(1.0);
    });
    let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&validate(&p));
    assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
    assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
    assert!(from_stock_top);
    assert_eq!(field.as_deref(), Some("operations[o6].flat_depth_mm"));
    let message = validate(&p)[0].to_string();
    assert!(message.contains("stock top"), "{message}");

    // Cleared by a Pocket at the start depth: the flat depth alone.
    let mut pocketed = p.clone();
    carve_mut(&mut pocketed).start_plane_cleared_externally = false;
    pocketed.operations.insert(1, rectangle_pocket(4.0));
    assert_eq!(validate(&pocketed), vec![]);

    // A Pocket at another depth leaves the start plane in material the
    // project does not clear: from the stock top again.
    let mut shallower = pocketed.clone();
    shallower.operations[1] = rectangle_pocket(3.0);
    let (engagement_mm, _, from_stock_top, _) = cone_exceeded(&validate(&shallower));
    assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
    assert!(from_stock_top);

    // With no flat depth the start depth alone already exceeds it.
    let mut full = p.clone();
    carve_mut(&mut full).flat_depth_mm = None;
    let (engagement_mm, _, from_stock_top, field) = cone_exceeded(&validate(&full));
    assert!(approx_eq(engagement_mm, 4.0, 1e-9), "{engagement_mm}");
    assert!(from_stock_top);
    assert_eq!(field.as_deref(), Some("operations[o6].start_depth_mm"));
}

/// Security review L6: the flatten tolerance is at most 0.1 mm.
#[test]
fn flatten_tolerance_above_0_1_mm_is_error() {
    let mut p = sample_project();
    p.flatten_tolerance_mm = 0.2;
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::FlattenToleranceRange {
            flatten_tolerance_mm: 0.2,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "flatten_tolerance_range");
    assert_eq!(problems[0].field().as_deref(), Some("flatten_tolerance_mm"));
    assert!(approx_eq(MAX_FLATTEN_TOLERANCE_MM, 0.1, 1e-9));
    p.flatten_tolerance_mm = MAX_FLATTEN_TOLERANCE_MM;
    assert_eq!(validate(&p), vec![]);
    p.flatten_tolerance_mm = 0.0;
    assert_eq!(
        validate(&p),
        vec![Problem::OutOfRange {
            field: "flatten_tolerance_mm".into(),
            value: 0.0,
            expected: POSITIVE,
        }]
    );
}

// Format 21: the countersink Drill (functional spec, capability 44).

/// The sample project with the 90°, 6.35 mm V-bit `t2` (a 3.175 mm
/// cone) and a 3.175 mm circle `i3` at (35, 25), inside the sample
/// rectangle, countersunk 6 mm wide from the stock top by Drill `o3`
/// with no `depth_mm`, edited by `edit`.
fn countersink_project(edit: impl FnOnce(&mut crate::model::DrillOp)) -> Project {
    let mut p = sample_project();
    p.tools.push(crate::test_support::tool_v_bit_90());
    let circle = DefinitionId::new("d3");
    p.definitions.insert(
        circle.clone(),
        crate::model::FeatureDefinition {
            id: circle.clone(),
            name: "Hole".into(),
            profiles: vec![Polyline::circle(
                dc_geometry::Point { x: 0.0, y: 0.0 },
                3.175 / 2.0,
            )],
        },
    );
    p.instances.push(crate::model::FeatureInstance {
        id: InstanceId::new("i3"),
        definition: circle,
        transform: Transform::translate(35.0, 25.0),
        name: "Hole 1".into(),
        visible: true,
        locked: false,
        z_top_mm: Some(6.0),
        z_bottom_mm: Some(0.0),
    });
    let mut drill = sample_drill();
    drill.targets = vec![InstanceId::new("i3")];
    drill.tool = ToolId::new("t2");
    drill.depth_mm = None;
    drill.cycle = DrillCycle::Countersink {
        diameter_mm: 6.0,
        start_depth_mm: 0.0,
        start_plane_cleared_externally: false,
    };
    edit(&mut drill);
    p.operations.push(Operation::Drill(drill));
    p
}

/// The countersink's settings of `drill`, to edit.
fn countersink_of(drill: &mut crate::model::DrillOp) -> (&mut f64, &mut f64, &mut bool) {
    match &mut drill.cycle {
        DrillCycle::Countersink {
            diameter_mm,
            start_depth_mm,
            start_plane_cleared_externally,
        } => (diameter_mm, start_depth_mm, start_plane_cleared_externally),
        cycle => panic!("not a countersink: {cycle:?}"),
    }
}

/// Capability 44: the mouth can be no wider than the V-bit, past
/// whose diameter there is no cutting edge; a mouth the bit's own
/// diameter is the most it cuts. A mouth that is not above 0 is its
/// own error.
#[test]
fn countersink_mouth_wider_than_tool_is_error() {
    let p = countersink_project(|drill| *countersink_of(drill).0 = 7.0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::CountersinkMouthExceedsTool {
            operation: OperationId::new("o3"),
            name: "Drill".into(),
            tool_name: "90° V-bit".into(),
            diameter_mm: 7.0,
            tool_diameter_mm: 6.35,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "countersink_mouth_exceeds_tool");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o3].cycle.countersink.diameter_mm")
    );
    let p = countersink_project(|drill| *countersink_of(drill).0 = 6.35);
    assert_eq!(validate(&p), vec![]);

    for diameter_mm in [0.0, -6.0] {
        let p = countersink_project(|drill| *countersink_of(drill).0 = diameter_mm);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::CountersinkDiameterPositive {
                operation: OperationId::new("o3"),
                name: "Drill".into(),
                diameter_mm,
            }],
            "{diameter_mm}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "countersink_diameter_positive");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o3].cycle.countersink.diameter_mm")
        );
    }
    let p = countersink_project(|drill| *countersink_of(drill).0 = f64::NAN);
    assert_eq!(
        validate(&p),
        vec![Problem::NonFinite {
            field: "operations[o3].cycle.countersink.diameter_mm".into(),
        }]
    );
}

/// Capabilities 41 and 44, security review H4: the flanks engage
/// from the stock top unless an enabled earlier Pocket at the start
/// depth contains the bit at every hole, so a 6 mm mouth (3 mm deep
/// with the 90° bit) 2 mm down a start plane only vouched for engages
/// 5 mm of a 3.175 mm cone. The same countersink on the floor of a
/// 2 mm Pocket engages 3 mm, which fits; so does one from the top.
#[test]
fn countersink_depth_beyond_cone_is_error() {
    let vouched = countersink_project(|drill| {
        let (_, start_depth_mm, cleared_externally) = countersink_of(drill);
        *start_depth_mm = 2.0;
        *cleared_externally = true;
    });
    let problems = validate(&vouched);
    let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&problems);
    assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
    assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
    assert!(from_stock_top);
    assert_eq!(
        field.as_deref(),
        Some("operations[o3].cycle.countersink.diameter_mm")
    );
    let message = problems[0].to_string();
    assert!(
        message.contains("from the stock top") && message.contains("countersink's depth"),
        "{message}"
    );

    let mut pocketed = countersink_project(|drill| *countersink_of(drill).1 = 2.0);
    pocketed.operations.insert(1, rectangle_pocket(2.0));
    assert_eq!(validate(&pocketed), vec![]);

    // A Pocket at another depth does not clear the start plane: the
    // flanks engage from the stock top again.
    let mut shallower = countersink_project(|drill| *countersink_of(drill).1 = 2.0);
    shallower.operations.insert(1, rectangle_pocket(1.0));
    let (engagement_mm, _, from_stock_top, _) = cone_exceeded(&validate(&shallower));
    assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
    assert!(from_stock_top);

    assert_eq!(validate(&countersink_project(|_| {})), vec![]);
}

/// Capability 44, security review M2: a countersink below the stock
/// top needs an earlier Pocket that holds its whole bit at every
/// hole, or the user's word, as a V-Carve does; located at its own
/// start depth. A negative start depth is an error there too.
#[test]
fn uncleared_countersink_start_depth_is_an_error() {
    // 0.1 mm down, so the flanks engaged from the stock top (3.1 mm)
    // fit the 3.175 mm cone and the plane is the one problem.
    let p = countersink_project(|drill| *countersink_of(drill).1 = 0.1);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::StartDepthUncleared {
            operation: OperationId::new("o3"),
            name: "Drill".into(),
            start_depth_mm: 0.1,
            field: "cycle.countersink.start_depth_mm",
            footprint: StartFootprint::BitAtEachHole,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o3].cycle.countersink.start_depth_mm")
    );
    assert!(
        problems[0]
            .to_string()
            .contains("the whole of its V-bit at each hole"),
        "{}",
        problems[0]
    );
    let mut pocketed = p.clone();
    pocketed.operations.insert(1, rectangle_pocket(0.1));
    assert_eq!(validate(&pocketed), vec![]);
    // Deeper, the uncleared plane counts toward the cone as well.
    let deeper = countersink_project(|drill| *countersink_of(drill).1 = 1.0);
    let codes: Vec<&str> = validate(&deeper).iter().map(Problem::code).collect();
    assert_eq!(codes, ["start_depth_uncleared", "v_bit_cone_exceeded"]);
    let vouched = countersink_project(|drill| {
        let (_, start_depth_mm, cleared_externally) = countersink_of(drill);
        *start_depth_mm = 0.1;
        *cleared_externally = true;
    });
    assert_eq!(validate(&vouched), vec![]);

    let p = countersink_project(|drill| *countersink_of(drill).1 = -1.0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::StartDepthNegative {
            operation: OperationId::new("o3"),
            name: "Drill".into(),
            start_depth_mm: -1.0,
            field: "cycle.countersink.start_depth_mm",
        }]
    );
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o3].cycle.countersink.start_depth_mm")
    );
}

/// An enabled V-Carve `o6` of the sample rectangle with the 90° V-bit
/// `t2` and a 2.5 mm flat depth from the stock top: its floor is the
/// rectangle inset 2.5 mm, x 12.5..57.5, y 12.5..37.5.
fn rectangle_v_carve() -> Operation {
    let mut carve = crate::test_support::sample_v_carve();
    carve.flat_depth_mm = Some(2.5);
    Operation::VCarve(carve)
}

/// Issue #426: the countersink's hole at (35, 25) sits on the 2.5 mm
/// floor of a V-Carve before it, which holds the whole 6.35 mm bit,
/// so a countersink 2.5 mm down is neither uncleared nor past its
/// cone: the flanks engage its own 3 mm depth from the floor. At
/// 2 mm the floor is not its plane (the order check's mismatch), so
/// the flanks engage from the stock top, 5 mm of a 3.175 mm cone. A
/// hole on the carve's slope, outside the floor, is uncleared.
#[test]
fn countersink_on_a_v_carve_floor_is_not_uncleared() {
    let mut floored = countersink_project(|drill| *countersink_of(drill).1 = 2.5);
    floored.operations.insert(1, rectangle_v_carve());
    assert_eq!(validate(&floored), vec![]);

    let mut mismatched = countersink_project(|drill| *countersink_of(drill).1 = 2.0);
    mismatched.operations.insert(1, rectangle_v_carve());
    let (engagement_mm, _, from_stock_top, _) = cone_exceeded(&validate(&mismatched));
    assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
    assert!(from_stock_top);

    let mut on_the_slope = countersink_project(|drill| *countersink_of(drill).1 = 0.1);
    on_the_slope.operations.insert(1, rectangle_v_carve());
    on_the_slope.instances[1].transform = Transform::translate(11.5, 25.0);
    let codes: Vec<&str> = validate(&on_the_slope).iter().map(Problem::code).collect();
    assert_eq!(codes, ["start_depth_uncleared"]);
}

/// The design spec, "Validation": a countersink's V-bit is no
/// `drill_cutter_unusual`, and its unset `depth_mm` is not an unset
/// value (the depth is derived). Any cutter but a V-bit is
/// `countersink_needs_v_bit` at the tool, and a V-bit angle out of
/// range is `v_bit_angle_range`, as for a V-Carve.
#[test]
fn countersink_drill_with_v_bit_is_not_unusual() {
    let p = countersink_project(|_| {});
    assert_eq!(validate(&p), vec![]);
    let p = countersink_project(|drill| drill.depth_mm = Some(3.0));
    assert_eq!(validate(&p), vec![]);

    for cutter in [ToolKind::Drill, ToolKind::EndMill, ToolKind::BallNose] {
        let mut p = countersink_project(|_| {});
        p.tools[1].cutter = cutter.clone();
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::CountersinkNeedsVBit {
                operation: OperationId::new("o3"),
                name: "Drill".into(),
                tool: ToolId::new("t2"),
                tool_name: "90° V-bit".into(),
                cutter: cutter.describe(),
            }],
            "{cutter:?}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "countersink_needs_v_bit");
        assert_eq!(problems[0].field().as_deref(), Some("operations[o3].tool"));
    }

    for angle in [0.0, 180.0] {
        let mut p = countersink_project(|_| {});
        p.tools[1].cutter = ToolKind::VBit {
            included_angle_deg: angle,
        };
        assert_eq!(
            validate(&p),
            vec![Problem::VBitAngleRange {
                tool: ToolId::new("t2"),
                name: "90° V-bit".into(),
                included_angle_deg: angle,
            }],
            "{angle}"
        );
    }
}

/// Design spec, "Countersink" step 2: Drill's narrower-hole rule does
/// not apply to a countersink, so `allow_tool_wider_than_hole: true`
/// does nothing, and validation says so at the field. A warning.
#[test]
fn countersink_ignores_tool_width_warns() {
    let p = countersink_project(|drill| drill.allow_tool_wider_than_hole = true);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::CountersinkIgnoresToolWidth {
            operation: OperationId::new("o3"),
            name: "Drill".into(),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(problems[0].code(), "countersink_ignores_tool_width");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o3].allow_tool_wider_than_hole")
    );
    // A simple Drill uses the field.
    let mut p = with_ops(vec![Operation::Drill(sample_drill())]);
    if let Some(Operation::Drill(drill)) = p.operations.get_mut(1) {
        drill.allow_tool_wider_than_hole = true;
    }
    assert_eq!(validate(&p), vec![]);
}

// Format 22: the Inlay (`docs/specs/phase-3/design.md`, "Validation"
// and "Algorithms → Inlay", step 1).

/// The project of [`carve_project`] on a 19 mm board, with the
/// V-Carve replaced by the Inlay `o7` of the square `i2`
/// ([`crate::test_support::sample_inlay`] with a 0.5 mm plug depth:
/// female, `D 2.5, G 0.25, P 0.5`, margin 6, the 90° 6.35 mm V-bit
/// `t2`, so either side fits the cone), as `edit` leaves it.
fn inlay_project(edit: impl FnOnce(&mut crate::model::InlayOp)) -> Project {
    let mut p = carve_project(|_| {});
    p.stock.thickness_mm = Some(19.0);
    p.operations.pop();
    let mut inlay = crate::test_support::sample_inlay();
    inlay.targets = vec![InstanceId::new("i2")];
    inlay.plug_depth_mm = Some(0.5);
    edit(&mut inlay);
    p.operations.push(Operation::Inlay(inlay));
    p
}

/// Gives `inlay` a male board the size of [`inlay_project`]'s 19 mm
/// stock, so its male is cut and checked too (#498).
fn on_male_board(inlay: &mut crate::model::InlayOp) {
    inlay.male_board = crate::model::MaleBoard {
        width_mm: None,
        height_mm: None,
        thickness_mm: Some(19.0),
    };
}

/// Sets which boards `inlay` cuts in a test of one board (#498): on
/// [`Board::Female`] its male board is unset, so only the female is
/// cut and checked; on [`Board::Male`] it is measured
/// ([`on_male_board`]), so the male's rules apply too.
fn cuts(inlay: &mut crate::model::InlayOp, side: Board) {
    match side {
        Board::Female => inlay.male_board = crate::model::MaleBoard::default(),
        Board::Male => on_male_board(inlay),
    }
}

/// The star test part's values (functional spec, "Inlay pair"): `D 4,
/// G 1, P 2`, margin 8, cutting `side` ([`cuts`]).
fn star(inlay: &mut crate::model::InlayOp, side: Board) {
    cuts(inlay, side);
    inlay.depth_mm = Some(4.0);
    inlay.glue_gap_mm = Some(1.0);
    inlay.plug_depth_mm = Some(2.0);
    inlay.margin_mm = Some(8.0);
}

/// Sets the V-bit `t2` of `p` to `angle` degrees.
fn v_bit_angle(p: &mut Project, angle: f64) {
    p.tools[1].cutter = ToolKind::VBit {
        included_angle_deg: angle,
    };
}

const SIDES: [Board; 2] = [Board::Female, Board::Male];

/// Capability 45: the glue gap is left under the seated plug, so it
/// is 0 or more and below the depth, on either side; at or above it
/// the male's start plane would be at or above the stock top.
#[test]
fn inlay_glue_gap_must_be_below_depth() {
    for side in SIDES {
        assert_eq!(
            validate(&inlay_project(|inlay| cuts(inlay, side))),
            vec![],
            "{side:?}"
        );
        for glue_gap_mm in [2.5, 3.0] {
            let p = inlay_project(|inlay| {
                cuts(inlay, side);
                inlay.glue_gap_mm = Some(glue_gap_mm);
            });
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::InlayGlueGapNotBelowDepth {
                    operation: OperationId::new("o7"),
                    name: "Inlay".into(),
                    glue_gap_mm,
                    depth_mm: 2.5,
                }],
                "{side:?} {glue_gap_mm}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "inlay_glue_gap_not_below_depth");
            assert_eq!(
                problems[0].field().as_deref(),
                Some("operations[o7].glue_gap_mm")
            );
            let message = problems[0].to_string();
            assert!(
                message.contains(&format!("glue_gap_mm {glue_gap_mm} mm"))
                    && message.contains("depth_mm 2.5 mm"),
                "{message}"
            );
        }
        // 0 is a plug that bottoms exactly; just below the depth fits.
        for glue_gap_mm in [0.0, 2.4] {
            let p = inlay_project(|inlay| {
                cuts(inlay, side);
                inlay.glue_gap_mm = Some(glue_gap_mm);
            });
            assert_eq!(validate(&p), vec![], "{side:?} {glue_gap_mm}");
        }
        let p = inlay_project(|inlay| {
            cuts(inlay, side);
            inlay.glue_gap_mm = Some(-0.25);
        });
        assert_eq!(
            validate(&p),
            vec![Problem::OutOfRange {
                field: "operations[o7].glue_gap_mm".into(),
                value: -0.25,
                expected: NON_NEGATIVE,
            }],
            "{side:?}"
        );
    }
}

/// Capability 45: the depth, glue gap and plug depth describe both
/// sides, so each is required on either; unset, each is
/// `operation_value_unset` at its field, in file order, and set, the
/// depths are above 0.
#[test]
fn inlay_values_must_be_set() {
    for side in SIDES {
        let p = inlay_project(|inlay| {
            cuts(inlay, side);
            inlay.depth_mm = None;
            inlay.glue_gap_mm = None;
            inlay.plug_depth_mm = None;
        });
        let problems = validate(&p);
        let unset = |field| Problem::OperationValueUnset {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            field,
        };
        assert_eq!(
            problems,
            vec![
                unset("depth_mm"),
                unset("glue_gap_mm"),
                unset("plug_depth_mm")
            ],
            "{side:?}"
        );
        assert!(problems.iter().all(Problem::is_unset_value));
        assert_eq!(
            problems[2].field().as_deref(),
            Some("operations[o7].plug_depth_mm")
        );
        let p = inlay_project(|inlay| {
            cuts(inlay, side);
            inlay.depth_mm = Some(0.0);
            inlay.plug_depth_mm = Some(-1.0);
        });
        assert_eq!(
            validate(&p),
            vec![
                Problem::OutOfRange {
                    field: "operations[o7].depth_mm".into(),
                    value: 0.0,
                    expected: POSITIVE,
                },
                Problem::OutOfRange {
                    field: "operations[o7].plug_depth_mm".into(),
                    value: -1.0,
                    expected: POSITIVE,
                },
            ],
            "{side:?}"
        );
    }
}

/// Capability 45: the male's surround is the design grown by
/// `margin_mm`, so a male needs one above 0; the female ignores it.
#[test]
fn inlay_male_needs_margin() {
    let p = inlay_project(|inlay| {
        cuts(inlay, Board::Male);
        inlay.margin_mm = None;
    });
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::OperationValueUnset {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            field: "margin_mm",
        }]
    );
    assert!(problems[0].is_unset_value());
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].margin_mm")
    );
    for margin_mm in [0.0, -1.0] {
        let p = inlay_project(|inlay| {
            cuts(inlay, Board::Male);
            inlay.margin_mm = Some(margin_mm);
        });
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::InlayMarginPositive {
                operation: OperationId::new("o7"),
                name: "Inlay".into(),
                margin_mm,
            }],
            "{margin_mm}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "inlay_margin_positive");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o7].margin_mm")
        );
        // The female does not use it.
        let p = inlay_project(|inlay| inlay.margin_mm = Some(margin_mm));
        assert_eq!(validate(&p), vec![], "female {margin_mm}");
    }
    let p = inlay_project(|inlay| inlay.margin_mm = None);
    assert_eq!(validate(&p), vec![], "female, no margin");
}

/// Design spec, "Inlay" step 4: a male leaving less than 6 mm of
/// board under its surround's floor warns. The star's values put the
/// floor 4 − 1 + 2 = 5 mm down: 5 mm left on a 10 mm male board, 6 on
/// an 11 mm one, 7 on 12, whatever the 19 mm stock (#498: the male
/// board's thickness). With the male board unset only the female is
/// cut, which leaves no plug base.
#[test]
fn thin_plug_base_warns() {
    let male_board = |p: &mut Project, thickness_mm: f64| {
        if let Some(Operation::Inlay(inlay)) = p.operations.last_mut() {
            inlay.male_board.thickness_mm = Some(thickness_mm);
        }
    };
    let mut p = inlay_project(|inlay| star(inlay, Board::Male));
    v_bit_angle(&mut p, 60.0);
    male_board(&mut p, 10.0);
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::InlayPlugBaseThin {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            floor_depth_mm: 5.0,
            thickness_mm: 10.0,
            base_mm: 5.0,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(problems[0].code(), "inlay_plug_base_thin");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].plug_depth_mm")
    );
    assert!(approx_eq(INLAY_MIN_PLUG_BASE_MM, 6.0, 0.0));
    for thickness_mm in [11.0, 12.0] {
        male_board(&mut p, thickness_mm);
        assert_eq!(validate(&p), vec![], "{thickness_mm} mm");
    }
    let mut female = inlay_project(|inlay| star(inlay, Board::Female));
    v_bit_angle(&mut female, 60.0);
    female.stock.thickness_mm = Some(10.0);
    assert_eq!(validate(&female), vec![]);
}

/// Security review H4: the male's flank engages from the stock top,
/// `(D − G) + P`, since the plug above its start plane is never
/// cleared. Corbin's `D 2.5, G 0.25, P 1.5` engages 3.75 mm, past the
/// 90° 6.35 mm bit's 3.175 mm cone, though `P` alone fits; the star's
/// `4, 1, 2` engages 5 mm, within the 60° bit's 5.5 mm.
#[test]
fn male_inlay_engagement_beyond_the_cone_is_error() {
    let corbin = |inlay: &mut crate::model::InlayOp| {
        inlay.depth_mm = Some(2.5);
        inlay.glue_gap_mm = Some(0.25);
        inlay.plug_depth_mm = Some(1.5);
    };
    let p = inlay_project(|inlay| {
        corbin(inlay);
        cuts(inlay, Board::Male);
    });
    let problems = validate(&p);
    let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&problems);
    assert!(approx_eq(engagement_mm, 3.75, 1e-9), "{engagement_mm}");
    assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
    assert!(from_stock_top);
    assert_eq!(field.as_deref(), Some("operations[o7].plug_depth_mm"));
    let message = problems[0].to_string();
    assert!(
        message.contains("3.75 mm")
            && message.contains("3.175 mm")
            && message.contains("plug_depth_mm")
            && !message.contains("Pocket"),
        "{message}"
    );

    let mut p = inlay_project(|inlay| star(inlay, Board::Male));
    v_bit_angle(&mut p, 60.0);
    assert_eq!(validate(&p), vec![]);
    // The star on the 90° bit engages 5 mm on the male board, past its
    // 3.175 mm cone, and 4 mm on the female's (#498: each board is
    // checked, at its own field).
    let p = inlay_project(|inlay| star(inlay, Board::Male));
    let problems = validate(&p);
    let (female, male): (Vec<Problem>, Vec<Problem>) = problems
        .into_iter()
        .partition(|p| p.field().as_deref() == Some("operations[o7].depth_mm"));
    let (engagement_mm, ..) = cone_exceeded(&male);
    assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
    let (engagement_mm, _, from_stock_top, _) = cone_exceeded(&female);
    assert!(approx_eq(engagement_mm, 4.0, 1e-9), "{engagement_mm}");
    assert!(!from_stock_top);
}

/// Design spec, "Inlay" step 1: the female starts at the stock top,
/// so its engagement is its depth alone, whatever the glue gap and
/// plug depth its male uses; located at `depth_mm`.
#[test]
fn female_inlay_engagement_is_its_depth() {
    // Its male would engage 3.5 − 0.25 + 1.5 = 4.75 mm.
    let p = inlay_project(|inlay| {
        inlay.depth_mm = Some(3.5);
        inlay.plug_depth_mm = Some(1.5);
    });
    let problems = validate(&p);
    let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&problems);
    assert!(approx_eq(engagement_mm, 3.5, 1e-9), "{engagement_mm}");
    assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
    assert!(!from_stock_top);
    assert_eq!(field.as_deref(), Some("operations[o7].depth_mm"));
    // 3 mm fits, though its male would engage 3 − 0.25 + 3 mm.
    let p = inlay_project(|inlay| {
        inlay.depth_mm = Some(3.0);
        inlay.plug_depth_mm = Some(3.0);
    });
    assert_eq!(validate(&p), vec![]);
    let mut p = inlay_project(|inlay| star(inlay, Board::Female));
    v_bit_angle(&mut p, 60.0);
    assert_eq!(validate(&p), vec![]);
}

/// Capability 45: an Inlay carves with a V-bit, as a V-Carve does:
/// any other cutter is `v_carve_needs_v_bit` at its tool, and its
/// bit's angle out of range is `v_bit_angle_range`.
#[test]
fn inlay_with_end_mill_is_error() {
    for side in SIDES {
        for cutter in [
            ToolKind::EndMill,
            ToolKind::BallNose,
            ToolKind::Engraver,
            ToolKind::Drill,
        ] {
            let mut p = inlay_project(|inlay| cuts(inlay, side));
            p.tools[1].cutter = cutter.clone();
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::VCarveNeedsVBit {
                    operation: OperationId::new("o7"),
                    name: "Inlay".into(),
                    tool: ToolId::new("t2"),
                    tool_name: "90° V-bit".into(),
                    cutter: cutter.describe(),
                    photo: false,
                }],
                "{side:?} {cutter:?}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "v_carve_needs_v_bit");
            assert_eq!(problems[0].field().as_deref(), Some("operations[o7].tool"));
            assert!(
                !problems[0].to_string().contains("V-Carve"),
                "{}",
                problems[0]
            );
        }
        for angle in [0.0, 180.0] {
            let mut p = inlay_project(|inlay| cuts(inlay, side));
            v_bit_angle(&mut p, angle);
            assert_eq!(
                validate(&p),
                vec![Problem::VBitAngleRange {
                    tool: ToolId::new("t2"),
                    name: "90° V-bit".into(),
                    included_angle_deg: angle,
                }],
                "{side:?} {angle}"
            );
        }
    }
}

// Format 23 (#443): a male Inlay's cut-out.

/// [`inlay_project`] with the 3.175 mm end mill `t3` added and a
/// cut-out with it, four 6 mm tabs 3 mm tall, 0.2 mm through, as
/// `edit` leaves the Inlay.
fn cut_out_project(edit: impl FnOnce(&mut crate::model::InlayOp)) -> Project {
    use crate::model::{InlayCutOut, TabPlacement, TabShape, TabSpec};
    let mut p = inlay_project(|inlay| {
        on_male_board(inlay);
        inlay.cut_out = Some(InlayCutOut {
            tool: ToolId::new("t3"),
            tabs: Some(TabSpec {
                placement: TabPlacement::Count { count: 4 },
                width_mm: 6.0,
                height_mm: 3.0,
                shape: TabShape::Rect,
                on_holes: false,
                avoid_corners: true,
            }),
            through_cut_allowance_mm: 0.2,
        });
        edit(inlay);
    });
    p.tools.push(Tool {
        id: ToolId::new("t3"),
        name: "1/8 end mill".into(),
        diameter_mm: 3.175,
        ..crate::test_support::tool_6_35_upcut()
    });
    p
}

/// The male of [`cut_out_project`] is valid: its 6 mm band, 0.5 mm
/// deep on the 90° bit, has a floor ring 5 mm wide.
#[test]
fn cut_out_on_a_male_is_valid() {
    assert_eq!(validate(&cut_out_project(|_| {})), vec![]);
}

/// #498: every Inlay has a male, so a cut-out needs no side: an Inlay
/// with a cut-out whose male board is not measured yet is valid (its
/// male is not cut until it is, and the female ignores the cut-out),
/// and so is one with a measured board. `inlay_cut_out_needs_male` is
/// gone.
#[test]
fn cut_out_no_longer_needs_a_side() {
    let unmeasured = cut_out_project(|inlay| {
        inlay.male_board = crate::model::MaleBoard::default();
    });
    assert_eq!(validate(&unmeasured), vec![]);
    assert_eq!(validate(&cut_out_project(|_| {})), vec![]);
}

/// #498: a male board's width and height, when set, are its size,
/// above 0; `inlay_male_board_size`, an error, at the field. `null`
/// is the stock's.
#[test]
fn male_board_size_must_be_positive() {
    for field in ["width_mm", "height_mm"] {
        for value_mm in [0.0, -10.0] {
            let p = inlay_project(|inlay| {
                on_male_board(inlay);
                match field {
                    "width_mm" => inlay.male_board.width_mm = Some(value_mm),
                    _ => inlay.male_board.height_mm = Some(value_mm),
                }
            });
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::InlayMaleBoardSize {
                    operation: OperationId::new("o7"),
                    name: "Inlay".into(),
                    field,
                    value_mm,
                }],
                "{field} {value_mm}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "inlay_male_board_size");
            assert_eq!(
                problems[0].field(),
                Some(format!("operations[o7].male_board.{field}"))
            );
            let message = problems[0].to_string();
            assert!(message.contains(field), "{message}");
        }
        let p = inlay_project(|inlay| {
            on_male_board(inlay);
            match field {
                "width_mm" => inlay.male_board.width_mm = Some(60.0),
                _ => inlay.male_board.height_mm = Some(60.0),
            }
        });
        assert_eq!(validate(&p), vec![], "{field} 60");
    }
}

/// #498: a male board's thickness, when set, is above 0;
/// `inlay_male_board_thickness`, an error, at the field. Unset, the
/// male is not cut (the generator's `inlay_male_board_unset`), and the
/// project is valid.
#[test]
fn male_board_thickness_must_be_positive() {
    for thickness_mm in [0.0, -6.0] {
        let p = inlay_project(|inlay| {
            inlay.male_board.thickness_mm = Some(thickness_mm);
        });
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::InlayMaleBoardThickness {
                operation: OperationId::new("o7"),
                name: "Inlay".into(),
                thickness_mm,
            }],
            "{thickness_mm}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "inlay_male_board_thickness");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o7].male_board.thickness_mm")
        );
    }
    assert_eq!(
        validate(&inlay_project(|inlay| {
            inlay.male_board.thickness_mm = None;
        })),
        vec![]
    );
}

/// #498: two enabled Inlays of one project cut their males on the one
/// male board, so their boards must agree; a later one whose board
/// differs is `inlay_male_boards_differ`, an error, at its
/// `male_board`. A disabled Inlay cuts nothing and does not count.
#[test]
fn inlays_share_one_male_board() {
    let mut p = inlay_project(on_male_board);
    let Some(Operation::Inlay(first)) = p.operations.last().cloned() else {
        panic!("an inlay");
    };
    let mut second = crate::model::InlayOp {
        id: OperationId::new("o8"),
        name: "Second".into(),
        ..first
    };
    p.operations.push(Operation::Inlay(second.clone()));
    assert_eq!(validate(&p), vec![], "the same board");
    second.male_board.thickness_mm = Some(12.0);
    *p.operations.last_mut().unwrap() = Operation::Inlay(second.clone());
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::InlayMaleBoardsDiffer {
            operation: OperationId::new("o8"),
            name: "Second".into(),
            first: OperationId::new("o7"),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "inlay_male_boards_differ");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o8].male_board")
    );
    second.enabled = false;
    *p.operations.last_mut().unwrap() = Operation::Inlay(second);
    assert!(
        validate(&p)
            .iter()
            .all(|problem| problem.code() != "inlay_male_boards_differ"),
        "{:?}",
        validate(&p)
    );
}

/// #443: the cut-out's tool runs On line round the plug and through
/// the board; it must be a flat end mill, `cutter_not_flat` at
/// `cut_out.tool` otherwise.
#[test]
fn cut_out_needs_a_flat_end_mill() {
    for cutter in [
        ToolKind::BallNose,
        ToolKind::Engraver,
        ToolKind::VBit {
            included_angle_deg: 60.0,
        },
    ] {
        let mut p = cut_out_project(|_| {});
        p.tools[2].cutter = cutter.clone();
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::CutterNotFlat {
                operation: OperationId::new("o7"),
                name: "Inlay".into(),
                tool: ToolId::new("t3"),
                tool_name: "1/8 end mill".into(),
                cutter: cutter.describe(),
                field: "cut_out.tool",
            }],
            "{cutter:?}"
        );
        assert_eq!(problems[0].code(), "cutter_not_flat");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o7].cut_out.tool")
        );
        assert!(
            problems[0].to_string().contains("cut-out"),
            "{}",
            problems[0]
        );
    }
}

/// #443, #461: with a V wall the floor ring, `M − 2·P·tan(α/2)` wide,
/// must hold the tool's diameter. The star
/// with `margin_mm: 4` on the 60° bit (`P·s = 1.155`) leaves 1.69 mm
/// for the 3.175 mm end mill: `inlay_cut_out_no_room`, at
/// `margin_mm`, naming the margin needed, 2.309 + 3.175 = 5.484 mm.
#[test]
fn cut_out_with_no_room_in_the_floor_ring_is_an_error() {
    let mut p = cut_out_project(|inlay| {
        star(inlay, Board::Male);
        inlay.margin_mm = Some(4.0);
    });
    v_bit_angle(&mut p, 60.0);
    let problems = validate(&p);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let Problem::InlayCutOutNoRoom {
        operation,
        margin_mm,
        needed_mm,
        diameter_mm,
        ..
    } = &problems[0]
    else {
        panic!("{problems:?}");
    };
    assert_eq!(operation, &OperationId::new("o7"));
    dc_geometry::assert_approx_eq!(*margin_mm, 4.0, 1e-12);
    dc_geometry::assert_approx_eq!(*diameter_mm, 3.175, 1e-12);
    dc_geometry::assert_approx_eq!(
        *needed_mm,
        4.0 * (30.0_f64).to_radians().tan() + 3.175,
        1e-9
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "inlay_cut_out_no_room");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].margin_mm")
    );
    assert!(problems[0].to_string().contains("5.484"), "{}", problems[0]);

    // The star's own 8 mm margin has room.
    let mut p = cut_out_project(|inlay| star(inlay, Board::Male));
    v_bit_angle(&mut p, 60.0);
    assert_eq!(validate(&p), vec![]);
}

/// #448: with an open band edge the floor ring runs from the plug's
/// foot, `P·tan(α/2)` out, to the band's edge, `M` out, and the cut-out
/// runs along its outer edge (#461), so it needs `M ≥ P·tan(α/2) + d`.
/// The star's
/// values on the 60° bit (`P·s` = 1.155) with a 3.175 mm clearing pass
/// `t3`, which opens the edge, and a 6.35 mm cut-out `t1`: an 8 mm
/// margin has room (6.845 mm), which a V wall would not (it needs
/// 8.66 mm); a 6 mm margin has not, and names 1.155 + 6.35 = 7.505 mm.
#[test]
fn cut_out_room_follows_the_open_edge() {
    use crate::model::{ClearanceStrategy, ClearingPass, CutDirection, Entry};
    let open = |margin_mm: f64, clearing: bool| {
        let mut p = cut_out_project(|inlay| {
            star(inlay, Board::Male);
            inlay.margin_mm = Some(margin_mm);
            if let Some(cut_out) = inlay.cut_out.as_mut() {
                cut_out.tool = ToolId::new("t1");
            }
            if clearing {
                inlay.clearing = vec![ClearingPass {
                    tool: ToolId::new("t3"),
                    strategy: ClearanceStrategy::Offset,
                    direction: CutDirection::Climb,
                    entry: Entry::Plunge,
                }];
            }
        });
        v_bit_angle(&mut p, 60.0);
        validate(&p)
    };
    assert_eq!(open(8.0, true), vec![]);
    // Without the pass the end mill `t3` is unused, a warning.
    let needed = |problems: Vec<Problem>| {
        let errors: Vec<&Problem> = problems.iter().filter(|p| p.is_error()).collect();
        match &errors[..] {
            [Problem::InlayCutOutNoRoom { needed_mm, .. }] => *needed_mm,
            other => panic!("{other:?}"),
        }
    };
    let s = (30.0_f64).to_radians().tan();
    dc_geometry::assert_approx_eq!(needed(open(6.0, true)), 2.0 * s + 6.35, 1e-9);
    dc_geometry::assert_approx_eq!(needed(open(8.0, false)), 4.0 * s + 6.35, 1e-9);
}

/// #448: `outline_rounding_mm` is the radius the male's band is closed
/// by; set, it must be a number above 0. 0, −1 and NaN are each
/// `inlay_outline_rounding`, an error at the field, on either side
/// (the female ignores the value, but a pair shares it); `null` and 5
/// are valid.
#[test]
fn outline_rounding_must_be_positive() {
    for side in SIDES {
        for rounding_mm in [0.0, -1.0, f64::NAN] {
            let p = inlay_project(|inlay| {
                cuts(inlay, side);
                inlay.outline_rounding_mm = Some(rounding_mm);
            });
            let problems = validate(&p);
            assert_eq!(problems.len(), 1, "{side:?} {rounding_mm}: {problems:?}");
            let Problem::InlayOutlineRounding {
                operation,
                rounding_mm: got,
                ..
            } = &problems[0]
            else {
                panic!("{problems:?}");
            };
            assert_eq!(operation, &OperationId::new("o7"));
            assert!(
                got.is_nan() == rounding_mm.is_nan()
                    && (got.is_nan() || got.to_bits() == rounding_mm.to_bits())
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "inlay_outline_rounding");
            assert_eq!(
                problems[0].field().as_deref(),
                Some("operations[o7].outline_rounding_mm")
            );
            assert!(
                problems[0].to_string().contains("outline_rounding_mm"),
                "{}",
                problems[0]
            );
        }
        for rounding_mm in [None, Some(5.0)] {
            let p = inlay_project(|inlay| {
                cuts(inlay, side);
                inlay.outline_rounding_mm = rounding_mm;
            });
            assert_eq!(validate(&p), vec![], "{side:?} {rounding_mm:?}");
        }
    }
}

/// #443: the cut-out's tool is a used tool: its feed, plunge rate and
/// RPM are required, and so is its step-down, which sets the passes.
#[test]
fn cut_out_tool_needs_its_rates() {
    for field in ["feed_mm_min", "plunge_mm_min", "rpm", "step_down_mm"] {
        let mut p = cut_out_project(|_| {});
        let tool = &mut p.tools[2];
        match field {
            "feed_mm_min" => tool.feed_mm_min = None,
            "plunge_mm_min" => tool.plunge_mm_min = None,
            "rpm" => tool.rpm = None,
            _ => tool.step_down_mm = None,
        }
        assert_eq!(
            validate(&p),
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t3"),
                name: "1/8 end mill".into(),
                field,
            }],
            "{field}"
        );
    }
}

/// #443: the cut-out's fields follow their Profile counterparts': its
/// tool must exist, its allowance be 0 or more, and its tabs as a
/// Profile's tab request.
#[test]
fn cut_out_fields_are_checked() {
    let p = cut_out_project(|inlay| {
        let cut_out = inlay.cut_out.as_mut().unwrap();
        cut_out.through_cut_allowance_mm = -0.1;
        cut_out.tabs.as_mut().unwrap().width_mm = 0.0;
    });
    let fields: Vec<Option<String>> = validate(&p).iter().map(Problem::field).collect();
    assert_eq!(
        fields,
        [
            Some("operations[o7].cut_out.tabs.width_mm".to_owned()),
            Some("operations[o7].cut_out.through_cut_allowance_mm".to_owned()),
        ]
    );
    let p = cut_out_project(|inlay| inlay.cut_out.as_mut().unwrap().tool = ToolId::new("t9"));
    assert_eq!(
        validate(&p),
        vec![
            Problem::UnusedTool {
                tool: ToolId::new("t3"),
                name: "1/8 end mill".into(),
            },
            Problem::DanglingTool {
                operation: OperationId::new("o7"),
                tool: ToolId::new("t9"),
            },
        ]
    );
}

/// Capability 40: an Inlay's clearing passes follow the V-Carve's
/// rules, on either side, and an Inlay always has a flat depth, so it
/// never needs one for them. Each clearing list gives an Inlay the
/// problems it gives a floored V-Carve of the same id and name.
#[test]
fn inlay_clearing_rules_match_v_carve() {
    use crate::model::ClearingPass;
    let raster = ClearingPass {
        strategy: ClearanceStrategy::Raster { angle_deg: 180.0 },
        entry: Entry::Ramp { angle_deg: 60.0 },
        ..pass("t1")
    };
    let lists = [
        vec![pass("t1")],
        vec![pass("t1"), pass("t3")],
        vec![pass("t3"), pass("t1")],
        vec![pass("t1"), pass("t4")],
        vec![pass("t2")],
        vec![pass("t9")],
        vec![raster],
    ];
    // `t3` a 3.175 mm end mill with no rates or step-down: a clearing
    // tool is a used tool, so they are required. `t4` a V-bit.
    let tools = |p: &mut Project| {
        p.tools.push(Tool {
            feed_mm_min: None,
            step_down_mm: None,
            ..tool_of("t3", 3.175, ToolKind::EndMill)
        });
        p.tools.push(tool_of(
            "t4",
            6.35,
            ToolKind::VBit {
                included_angle_deg: 60.0,
            },
        ));
    };
    for side in SIDES {
        for clearing in &lists {
            let mut inlay = inlay_project(|inlay| {
                cuts(inlay, side);
                inlay.depth_mm = Some(2.0);
                inlay.glue_gap_mm = Some(0.5);
                inlay.plug_depth_mm = Some(1.0);
                inlay.clearing.clone_from(clearing);
            });
            tools(&mut inlay);
            let mut carve = carve_project(|carve| {
                carve.id = OperationId::new("o7");
                carve.name = "Inlay".into();
                carve.flat_depth_mm = Some(1.0);
                carve.clearing.clone_from(clearing);
            });
            carve.stock.thickness_mm = Some(19.0);
            tools(&mut carve);
            assert_eq!(validate(&inlay), validate(&carve), "{side:?} {clearing:?}");
        }
    }
    // The clearing passes' problems are the V-Carve's own.
    let mut p = inlay_project(|inlay| inlay.clearing = vec![pass("t1"), pass("t4")]);
    tools(&mut p);
    let codes: Vec<&str> = validate(&p).iter().map(Problem::code).collect();
    assert!(codes.contains(&"clearing_needs_end_mill"), "{codes:?}");
    assert!(!codes.contains(&"clearing_needs_flat_depth"), "{codes:?}");
}

// Format 26: the drawn boundary (#449, #602; `docs/specs/phase-3/
// design.md`, "Validation" and "Drawn boundary").

/// Adds instance `id` of a `w` × `h` mm rectangle with its corner at
/// (`x`, `y`) to `p`, with `holes` more rectangles (corner, size) in
/// the same definition after it.
fn add_shape(p: &mut Project, id: &str, x: f64, y: f64, w: f64, h: f64, holes: &[[f64; 4]]) {
    let definition = DefinitionId::new(format!("d_{id}"));
    let mut profiles = vec![Polyline::rect(dc_geometry::Point { x: 0.0, y: 0.0 }, w, h)];
    profiles.extend(
        holes
            .iter()
            .map(|&[hx, hy, hw, hh]| Polyline::rect(dc_geometry::Point { x: hx, y: hy }, hw, hh)),
    );
    p.definitions.insert(
        definition.clone(),
        crate::model::FeatureDefinition {
            id: definition.clone(),
            name: format!("Shape {id}"),
            profiles,
        },
    );
    p.instances.push(crate::model::FeatureInstance {
        id: InstanceId::new(id),
        definition,
        transform: Transform::translate(x, y),
        name: format!("Boundary {id}"),
        visible: true,
        locked: false,
        z_top_mm: Some(6.0),
        z_bottom_mm: Some(0.0),
    });
}

/// [`inlay_project`]'s male (its 10 mm square design `i2` at (20, 20)
/// to (30, 30), the sample rectangle `i1`, (10, 10) to (60, 40), round
/// it) with no margin and `boundary` listed, as `edit` leaves the
/// project. The sample rectangle is the Profile's target, not the
/// Inlay's, so it may be a boundary.
fn boundary_project(boundary: &[&str], edit: impl FnOnce(&mut Project)) -> Project {
    let mut p = inlay_project(|inlay| {
        on_male_board(inlay);
        inlay.margin_mm = None;
        inlay.boundary = boundary.iter().map(|b| InstanceId::new(*b)).collect();
    });
    edit(&mut p);
    p
}

/// The Inlay `o7` of `p`.
fn the_inlay(p: &Project) -> &crate::model::InlayOp {
    p.operations
        .iter()
        .find_map(|op| match op {
            Operation::Inlay(inlay) => Some(inlay),
            _ => None,
        })
        .unwrap()
}

/// The sample rectangle round the square leaves 10 mm on three
/// sides: a boundary with no margin is valid, whatever else is set.
#[test]
fn margin_is_not_required_with_a_boundary() {
    let p = boundary_project(&["i1"], |_| {});
    assert!(the_inlay(&p).has_boundary());
    assert_eq!(the_inlay(&p).margin_mm, None);
    assert_eq!(validate(&p), vec![]);
    // The same male without a boundary still needs its margin.
    let p = inlay_project(|inlay| {
        on_male_board(inlay);
        inlay.margin_mm = None;
    });
    assert_eq!(
        validate(&p),
        vec![Problem::OperationValueUnset {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            field: "margin_mm",
        }]
    );
}

/// `margin_mm` and `outline_rounding_mm` are replaced by the boundary,
/// so a value in either is a warning at that field, one per field set.
#[test]
fn margin_with_a_boundary_warns_ignored() {
    let p = boundary_project(&["i1"], |p| {
        if let Some(Operation::Inlay(inlay)) = p.operations.last_mut() {
            inlay.margin_mm = Some(6.0);
        }
    });
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::InlayMarginIgnored {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            field: "margin_mm",
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(problems[0].code(), "inlay_margin_ignored");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].margin_mm")
    );

    let p = boundary_project(&["i1"], |p| {
        if let Some(Operation::Inlay(inlay)) = p.operations.last_mut() {
            inlay.margin_mm = Some(6.0);
            inlay.outline_rounding_mm = Some(4.0);
        }
    });
    let problems = validate(&p);
    let fields: Vec<_> = problems
        .iter()
        .map(|p| (p.code(), p.field().unwrap_or_default()))
        .collect();
    assert_eq!(
        fields,
        vec![
            ("inlay_margin_ignored", "operations[o7].margin_mm".into()),
            (
                "inlay_margin_ignored",
                "operations[o7].outline_rounding_mm".into()
            ),
        ]
    );
    // A margin that is not above 0 is ignored too, not an error.
    let p = boundary_project(&["i1"], |p| {
        if let Some(Operation::Inlay(inlay)) = p.operations.last_mut() {
            inlay.margin_mm = Some(0.0);
        }
    });
    assert_eq!(
        validate(&p).iter().map(Problem::code).collect::<Vec<_>>(),
        vec!["inlay_margin_ignored"]
    );
}

/// The rounding's own range check still runs with a boundary, so a bad
/// value is not hidden by being ignored.
#[test]
fn bad_outline_rounding_is_still_an_error_with_a_boundary() {
    let p = boundary_project(&["i1"], |p| {
        if let Some(Operation::Inlay(inlay)) = p.operations.last_mut() {
            inlay.outline_rounding_mm = Some(-2.0);
        }
    });
    let problems = validate(&p);
    assert!(
        problems.contains(&Problem::InlayOutlineRounding {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            rounding_mm: -2.0,
        }),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.code() == "inlay_margin_ignored"),
        "{problems:?}"
    );
}

/// An id that is no instance is an error at its place in the list,
/// as `dangling_tool`.
#[test]
fn boundary_unknown_instance_is_error() {
    let p = boundary_project(&["i1", "nowhere"], |_| {});
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::InlayBoundaryUnknown {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            index: 1,
            instance: InstanceId::new("nowhere"),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "inlay_boundary_unknown");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].boundary[1]")
    );
    assert!(
        problems[0].to_string().contains("nowhere"),
        "{}",
        problems[0]
    );
}

/// An open profile is an error, not skipped: a boundary the user meant
/// and lost would fall back to nothing. The message names the
/// profile's index in the instance's definition.
#[test]
fn boundary_open_profile_is_error() {
    let p = boundary_project(&["i3"], |p| {
        add_shape(p, "i3", 0.0, 0.0, 80.0, 80.0, &[]);
        let definition = p.definitions.get_mut(&DefinitionId::new("d_i3")).unwrap();
        definition.profiles.push(Polyline::new(
            vec![
                dc_geometry::Vertex::line(0.0, 0.0),
                dc_geometry::Vertex::line(10.0, 0.0),
                dc_geometry::Vertex::line(10.0, 10.0),
            ],
            false,
        ));
    });
    // (The open profile is also the definition's own warning.)
    let problems: Vec<Problem> = validate(&p)
        .into_iter()
        .filter(|p| p.code().starts_with("inlay_boundary"))
        .collect();
    assert_eq!(
        problems,
        vec![Problem::InlayBoundaryOpen {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            index: 0,
            instance: InstanceId::new("i3"),
            profile: 1,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "inlay_boundary_open");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].boundary[0]")
    );
    assert!(
        problems[0].to_string().contains("profile 1"),
        "{}",
        problems[0]
    );

    // An instance with nothing closed is open too.
    let p = boundary_project(&["i3"], |p| {
        add_shape(p, "i3", 0.0, 0.0, 80.0, 80.0, &[]);
        p.definitions
            .get_mut(&DefinitionId::new("d_i3"))
            .unwrap()
            .profiles = vec![Polyline::new(
            vec![
                dc_geometry::Vertex::line(0.0, 0.0),
                dc_geometry::Vertex::line(10.0, 0.0),
            ],
            false,
        )];
    });
    let codes: Vec<&str> = validate(&p)
        .iter()
        .map(Problem::code)
        .filter(|c| c.starts_with("inlay_boundary"))
        .collect();
    assert_eq!(codes, vec!["inlay_boundary_open"]);
}

/// The design cannot be its own boundary.
#[test]
fn boundary_that_is_a_target_is_error() {
    let p = boundary_project(&["i2"], |_| {});
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::InlayBoundaryIsTarget {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            index: 0,
            instance: InstanceId::new("i2"),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "inlay_boundary_is_target");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].boundary[0]")
    );
}

/// A boundary whose edge cuts across the design (the square at (20,
/// 20) to (30, 30); the boundary's left edge at x = 25) is an error
/// at the list, naming the farthest point outside, (20, 20) or (20,
/// 30), 5 mm past the edge.
#[test]
fn boundary_crossing_the_design_is_error_with_the_point() {
    let p = boundary_project(&["i3"], |p| {
        add_shape(p, "i3", 25.0, 10.0, 40.0, 30.0, &[]);
    });
    let problems = validate(&p);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let Problem::InlayBoundaryCrossesDesign {
        operation,
        x_mm,
        y_mm,
        distance_mm,
        ..
    } = &problems[0]
    else {
        panic!("{problems:?}");
    };
    assert_eq!(operation, &OperationId::new("o7"));
    dc_geometry::assert_approx_eq!(*x_mm, 20.0, 1e-9);
    assert!(
        (*y_mm - 20.0).abs() < 1e-9 || (*y_mm - 30.0).abs() < 1e-9,
        "{y_mm}"
    );
    dc_geometry::assert_approx_eq!(*distance_mm, 5.0, 1e-9);
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "inlay_boundary_crosses_design");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].boundary")
    );
    assert!(problems[0].to_string().contains("5.000"), "{}", problems[0]);
}

/// A boundary that runs along the design's edge leaves the plug's
/// wall no band: touching is the same error, 0 mm past.
#[test]
fn boundary_touching_the_design_is_error() {
    let p = boundary_project(&["i3"], |p| {
        add_shape(p, "i3", 20.0, 10.0, 40.0, 30.0, &[]);
    });
    let problems = validate(&p);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        matches!(
            &problems[0],
            Problem::InlayBoundaryCrossesDesign { distance_mm, .. } if distance_mm.abs() < 1e-6
        ),
        "{problems:?}"
    );
    // A hair of room is enough.
    let p = boundary_project(&["i3"], |p| {
        add_shape(p, "i3", 19.9, 10.0, 40.0, 30.0, &[]);
    });
    assert_eq!(validate(&p), vec![]);
}

/// A listed instance none of whose shapes holds any part of the design
/// is a warning at its place; the design is still held by the rest.
#[test]
fn boundary_with_no_design_inside_warns() {
    let p = boundary_project(&["i1", "i3"], |p| {
        add_shape(p, "i3", 70.0, 70.0, 20.0, 20.0, &[]);
    });
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::InlayBoundaryEmpty {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            index: 1,
            instance: InstanceId::new("i3"),
            instance_name: "Boundary i3".into(),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(problems[0].code(), "inlay_boundary_empty");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].boundary[1]")
    );
}

/// A hole in a boundary shape is ignored (board standing in the band
/// would hold the plug off the female, #484): a warning naming the
/// instance and the hole's profile; the hole may be anywhere.
#[test]
fn boundary_hole_warns() {
    let p = boundary_project(&["i3"], |p| {
        add_shape(p, "i3", 5.0, 5.0, 80.0, 80.0, &[[40.0, 40.0, 20.0, 20.0]]);
    });
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::InlayBoundaryHoleIgnored {
            operation: OperationId::new("o7"),
            name: "Inlay".into(),
            index: 0,
            instance: InstanceId::new("i3"),
            instance_name: "Boundary i3".into(),
            profile: 1,
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Warning);
    assert_eq!(problems[0].code(), "inlay_boundary_hole_ignored");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].boundary[0]")
    );
    assert!(
        problems[0].to_string().contains("profile 1"),
        "{}",
        problems[0]
    );
}

/// Only the design must be strictly inside: a boundary may run past
/// the 100 × 100 board (open question 12).
#[test]
fn boundary_may_run_past_the_board() {
    let p = boundary_project(&["i3"], |p| {
        add_shape(p, "i3", -20.0, -20.0, 160.0, 160.0, &[]);
    });
    assert_eq!(validate(&p), vec![]);
}

/// Nothing about the boundary is checked until the male is cut: the
/// female ignores it, as it ignores the margin.
#[test]
fn female_only_inlay_ignores_the_boundary() {
    let p = inlay_project(|inlay| {
        inlay.margin_mm = Some(6.0);
        inlay.outline_rounding_mm = Some(3.0);
        inlay.boundary = vec![InstanceId::new("nowhere"), InstanceId::new("i2")];
    });
    assert!(!the_inlay(&p).cuts_male());
    assert_eq!(validate(&p), vec![]);
}

/// Several boundaries make one edge, their union (open question 13):
/// each design part must lie inside the union. Two bars that each
/// cross the square, 15 to 25 and 24 to 45 in x, cover it together
/// with 5 mm to spare; either alone crosses.
#[test]
fn several_boundaries_are_a_union() {
    let both = |p: &mut Project| {
        add_shape(p, "i3", 15.0, 10.0, 10.0, 30.0, &[]);
        add_shape(p, "i4", 24.0, 10.0, 21.0, 30.0, &[]);
    };
    assert_eq!(validate(&boundary_project(&["i3", "i4"], both)), vec![]);
    for only in ["i3", "i4"] {
        let problems = validate(&boundary_project(&[only], both));
        assert_eq!(problems.len(), 1, "{only}: {problems:?}");
        assert_eq!(
            problems[0].code(),
            "inlay_boundary_crosses_design",
            "{only}"
        );
    }
}

/// The cut-out's floor ring is read from the narrowest band: a
/// boundary 3 mm from the 10 mm square leaves the 3.175 mm end mill no
/// room on a V wall (it needs 2·P·tan(α/2) + d = 4.175 mm at P 0.5 on
/// the 90° bit), and the problem is located at `boundary`, naming the
/// 3 mm. With the sample rectangle, 10 mm from the square at its
/// nearest, there is room.
#[test]
fn cut_out_room_reads_the_narrowest_band() {
    let tight = {
        let mut p = cut_out_project(|inlay| {
            inlay.margin_mm = None;
            inlay.boundary = vec![InstanceId::new("i3")];
        });
        add_shape(&mut p, "i3", 17.0, 17.0, 16.0, 16.0, &[]);
        p
    };
    let problems = validate(&tight);
    assert_eq!(problems.len(), 1, "{problems:?}");
    let Problem::InlayCutOutNoRoom {
        margin_mm,
        needed_mm,
        ..
    } = &problems[0]
    else {
        panic!("{problems:?}");
    };
    dc_geometry::assert_approx_eq!(*margin_mm, 3.0, 1e-9);
    dc_geometry::assert_approx_eq!(*needed_mm, 1.0 + 3.175, 1e-9);
    assert_eq!(problems[0].code(), "inlay_cut_out_no_room");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].boundary")
    );

    let roomy = cut_out_project(|inlay| {
        inlay.margin_mm = None;
        inlay.boundary = vec![InstanceId::new("i1")];
    });
    assert_eq!(validate(&roomy), vec![]);
}

// Format 27: bitmaps and the Photo V-Carve (`docs/specs/phase-3/
// design.md`, "Validation"; security review M4).

/// The Photo V-Carve `o8` of [`photo_project`], to edit.
fn photo_mut(p: &mut Project) -> &mut crate::model::PhotoVCarveOp {
    p.operations
        .iter_mut()
        .find_map(|op| match op {
            Operation::PhotoVCarve(op) => Some(op),
            _ => None,
        })
        .expect("a Photo V-Carve")
}

/// [`photo_project`] with its Photo V-Carve as `edit` leaves it.
fn photo_with(edit: impl FnOnce(&mut crate::model::PhotoVCarveOp)) -> Project {
    let mut p = photo_project();
    edit(photo_mut(&mut p));
    p
}

/// [`photo_project`] with its bitmap `b1` as `edit` leaves it.
fn bitmap_with(edit: impl FnOnce(&mut crate::model::Bitmap)) -> Project {
    let mut p = photo_project();
    edit(&mut p.bitmaps[0]);
    p
}

/// The one problem of `p`, an error with `code`, whose field is
/// `field` and locates in the saved file.
fn only_error(p: &Project, code: &str, field: &str) -> Problem {
    let problems = validate(p);
    only_located(p, &problems, Severity::Error);
    assert_eq!(problems[0].code(), code, "{problems:?}");
    assert_eq!(problems[0].field().as_deref(), Some(field), "{problems:?}");
    problems[0].clone()
}

/// Capability 47: a `max_depth_mm` deeper than the bit's cone is tall
/// would cut with the shank. The 90°, 6.35 mm bit's cone is 3.175 mm.
/// The engagement sum applies as to a V-Carve: a start depth the user
/// only vouches for counts from the stock top, one a Pocket clears
/// does not.
#[test]
fn photo_max_depth_beyond_cone_is_error() {
    let p = photo_with(|photo| photo.max_depth_mm = Some(3.5));
    let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&validate(&p));
    assert!(approx_eq(engagement_mm, 3.5, 1e-9), "{engagement_mm}");
    assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
    assert!(!from_stock_top);
    assert_eq!(field.as_deref(), Some("operations[o8].max_depth_mm"));
    let message = validate(&p)[0].to_string();
    assert!(message.contains("3.175 mm"), "{message}");
    assert_eq!(validate(&p)[0].code(), "v_bit_cone_exceeded");

    for (max_depth_mm, angle) in [(3.0, 90.0), (3.175, 90.0), (3.5, 60.0)] {
        let mut p = photo_with(|photo| photo.max_depth_mm = Some(max_depth_mm));
        p.tools[1].cutter = ToolKind::VBit {
            included_angle_deg: angle,
        };
        assert_eq!(validate(&p), vec![], "{max_depth_mm} mm, {angle}°");
    }

    // 2 + 1.5 mm from the stock top, the start plane only vouched for.
    let vouched = photo_with(|photo| {
        photo.start_depth_mm = 2.0;
        photo.start_plane_cleared_externally = true;
    });
    let (engagement_mm, _, from_stock_top, field) = cone_exceeded(&validate(&vouched));
    assert!(approx_eq(engagement_mm, 3.5, 1e-9), "{engagement_mm}");
    assert!(from_stock_top);
    assert_eq!(field.as_deref(), Some("operations[o8].max_depth_mm"));
    // #648: a Photo has no flat depth; the message names its own field.
    let message = validate(&vouched)[0].to_string();
    assert!(
        message.contains("plus its max_depth_mm)") && !message.contains("flat depth"),
        "{message}"
    );
    // Cleared by a Pocket at the start depth: the maximum depth alone.
    let mut pocketed = vouched.clone();
    photo_mut(&mut pocketed).start_plane_cleared_externally = false;
    pocketed.operations.insert(1, rectangle_pocket(2.0));
    assert_eq!(validate(&pocketed), vec![]);

    // The maximum depth is a depth, more than 0.
    let p = photo_with(|photo| photo.max_depth_mm = Some(0.0));
    assert_eq!(
        validate(&p),
        vec![Problem::OutOfRange {
            field: "operations[o8].max_depth_mm".into(),
            value: 0.0,
            expected: POSITIVE,
        }]
    );
}

/// The maximum depth is never defaulted: `null` is
/// `operation_value_unset`, one of the values `init` lists.
#[test]
fn photo_max_depth_unset_is_error() {
    let p = photo_with(|photo| photo.max_depth_mm = None);
    let problem = only_error(&p, "operation_value_unset", "operations[o8].max_depth_mm");
    assert_eq!(
        problem,
        Problem::OperationValueUnset {
            operation: OperationId::new("o8"),
            name: "Photo V-Carve".into(),
            field: "max_depth_mm",
        }
    );
    assert!(problem.is_unset_value());
    assert_eq!(
        problem.to_string(),
        "operations[o8].max_depth_mm is not set (mm) for operation `Photo V-Carve`"
    );
}

/// Capability 47: the line angle is in [0, 180); the spacing, the
/// sample step and the step-over retract are above 0. Each has its own
/// code and its own field.
#[test]
fn photo_angle_and_spacing_ranges() {
    for angle in [180.0, -1.0, 360.0] {
        let p = photo_with(|photo| photo.line_angle_deg = angle);
        let problem = only_error(
            &p,
            "photo_line_angle_range",
            "operations[o8].line_angle_deg",
        );
        assert_eq!(
            problem,
            Problem::PhotoLineAngleRange {
                operation: OperationId::new("o8"),
                name: "Photo V-Carve".into(),
                line_angle_deg: angle,
            },
            "{angle}"
        );
    }
    for angle in [0.0, 90.0, 179.999] {
        let p = photo_with(|photo| photo.line_angle_deg = angle);
        assert_eq!(validate(&p), vec![], "{angle}");
    }

    for fraction in [0.0, -0.5] {
        let p = photo_with(|photo| photo.line_spacing_fraction = fraction);
        let problem = only_error(
            &p,
            "photo_line_spacing_positive",
            "operations[o8].line_spacing_fraction",
        );
        assert_eq!(
            problem,
            Problem::PhotoLineSpacingPositive {
                operation: OperationId::new("o8"),
                name: "Photo V-Carve".into(),
                line_spacing_fraction: fraction,
            },
            "{fraction}"
        );
    }

    for sample_mm in [0.0, -0.25] {
        let p = photo_with(|photo| photo.sample_mm = sample_mm);
        let problem = only_error(&p, "photo_sample_positive", "operations[o8].sample_mm");
        assert_eq!(
            problem,
            Problem::PhotoSamplePositive {
                operation: OperationId::new("o8"),
                name: "Photo V-Carve".into(),
                sample_mm,
            },
            "{sample_mm}"
        );
    }

    for retract_mm in [0.0, -1.0] {
        let p = photo_with(|photo| photo.stepover_retract_mm = retract_mm);
        let problem = only_error(
            &p,
            "photo_retract_positive",
            "operations[o8].stepover_retract_mm",
        );
        assert_eq!(
            problem,
            Problem::PhotoRetractPositive {
                operation: OperationId::new("o8"),
                name: "Photo V-Carve".into(),
                stepover_retract_mm: retract_mm,
            },
            "{retract_mm}"
        );
    }

    // A disabled Photo V-Carve's values are checked all the same.
    let p = photo_with(|photo| {
        photo.enabled = false;
        photo.sample_mm = 0.0;
    });
    let codes: Vec<&str> = validate(&p).iter().map(Problem::code).collect();
    assert!(codes.contains(&"photo_sample_positive"), "{codes:?}");
}

/// Capability 47: grooves closer than one groove width overlap, which
/// is allowed and worth saying: a warning, never an error.
#[test]
fn photo_spacing_below_one_warns() {
    let p = photo_with(|photo| photo.line_spacing_fraction = 0.9);
    let problems = validate(&p);
    let message = only_located(&p, &problems, Severity::Warning);
    assert_eq!(
        problems,
        vec![Problem::PhotoLinesOverlap {
            operation: OperationId::new("o8"),
            name: "Photo V-Carve".into(),
            line_spacing_fraction: 0.9,
        }]
    );
    assert_eq!(problems[0].code(), "photo_lines_overlap");
    assert!(!problems[0].is_error());
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o8].line_spacing_fraction")
    );
    assert!(message.contains("0.9"), "{message}");
    for fraction in [1.0, 1.1, 3.0] {
        let p = photo_with(|photo| photo.line_spacing_fraction = fraction);
        assert_eq!(validate(&p), vec![], "{fraction}");
    }
}

/// A Photo V-Carve carves one bitmap of the project, named by its id.
#[test]
fn photo_names_a_missing_bitmap() {
    use crate::ids::BitmapId;
    let p = photo_with(|photo| photo.image = BitmapId::new("b9"));
    let problem = only_error(&p, "photo_unknown_bitmap", "operations[o8].image");
    assert_eq!(
        problem,
        Problem::PhotoUnknownBitmap {
            operation: OperationId::new("o8"),
            name: "Photo V-Carve".into(),
            bitmap: BitmapId::new("b9"),
        }
    );
    assert_eq!(
        problem.to_string(),
        "operation `o8` carves bitmap `b9`, which does not exist"
    );
    // An instance's id is not a bitmap's.
    let p = photo_with(|photo| photo.image = BitmapId::new("i1"));
    only_error(&p, "photo_unknown_bitmap", "operations[o8].image");
    // A dangling reference is reported for a disabled operation too.
    let p = photo_with(|photo| {
        photo.enabled = false;
        photo.image = BitmapId::new("b9");
    });
    let codes: Vec<&str> = validate(&p).iter().map(Problem::code).collect();
    assert!(codes.contains(&"photo_unknown_bitmap"), "{codes:?}");
}

/// The rules every operation follows hold for a Photo V-Carve as for a
/// V-Carve: its tool exists and has its rates, its angle is a V-bit's,
/// its safe Z is above 0, its allowance and start depth 0 or more. It
/// has no targets, and that is not `operation_has_no_targets`.
#[test]
fn photo_follows_the_rules_of_a_used_tool() {
    let clean = photo_project();
    assert_eq!(validate(&clean), vec![]);
    assert!(clean.operations[1].targets().is_empty());

    let mut p = photo_project();
    p.tools[1].feed_mm_min = None;
    p.tools[1].plunge_mm_min = None;
    let fields: Vec<_> = validate(&p)
        .into_iter()
        .map(|problem| match problem {
            Problem::ToolValueUnset { tool, field, .. } => (tool, field),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        fields,
        [
            (ToolId::new("t2"), "feed_mm_min"),
            (ToolId::new("t2"), "plunge_mm_min"),
        ]
    );

    let p = photo_with(|photo| photo.tool = ToolId::new("t9"));
    let codes: Vec<&str> = validate(&p).iter().map(Problem::code).collect();
    assert_eq!(codes, ["unused_tool", "dangling_tool"]);

    for angle in [0.0, 180.0] {
        let mut p = photo_project();
        p.tools[1].cutter = ToolKind::VBit {
            included_angle_deg: angle,
        };
        assert_eq!(
            validate(&p),
            vec![Problem::VBitAngleRange {
                tool: ToolId::new("t2"),
                name: "90° V-bit".into(),
                included_angle_deg: angle,
            }],
            "{angle}"
        );
    }

    let p = photo_with(|photo| photo.safe_z_mm = 0.0);
    only_error(&p, "out_of_range", "operations[o8].safe_z_mm");
    let p = photo_with(|photo| photo.through_cut_allowance_mm = -1.0);
    only_error(
        &p,
        "out_of_range",
        "operations[o8].through_cut_allowance_mm",
    );
    let p = photo_with(|photo| photo.start_depth_mm = -1.0);
    let problem = only_error(&p, "start_depth_negative", "operations[o8].start_depth_mm");
    assert_eq!(
        problem,
        Problem::StartDepthNegative {
            operation: OperationId::new("o8"),
            name: "Photo V-Carve".into(),
            start_depth_mm: -1.0,
            field: "start_depth_mm",
        }
    );
}

/// The footprint keeps the picture's proportions: `height_mm /
/// width_mm` is `height_px / width_px` within 1e-6, so a picture is
/// never stretched. The sample is 4 × 2 pixels on 40 × 20 mm.
#[test]
fn bitmap_aspect_must_match_pixels() {
    for height_mm in [21.0, 19.0, 20.001, 40.0] {
        let p = bitmap_with(|bitmap| bitmap.height_mm = height_mm);
        let problem = only_error(&p, "bitmap_footprint_aspect", "bitmaps[b1].height_mm");
        assert_eq!(
            problem,
            Problem::BitmapFootprintAspect {
                bitmap: crate::ids::BitmapId::new("b1"),
                name: "ramp".into(),
                width_mm: 40.0,
                height_mm,
                width_px: 4,
                height_px: 2,
            },
            "{height_mm}"
        );
    }
    let p = bitmap_with(|bitmap| bitmap.height_mm = 21.0);
    assert_eq!(
        validate(&p)[0].to_string(),
        "bitmap `ramp` (`b1`) is 40 × 21 mm, which is not the shape of its 4 × 2 px picture; set height_mm to 20 mm, or width_mm to 42 mm, so the picture is not stretched"
    );
    // Within 1e-6 of the pixels' proportions.
    for height_mm in [20.0, 20.000_01, 19.999_99] {
        let p = bitmap_with(|bitmap| bitmap.height_mm = height_mm);
        assert_eq!(validate(&p), vec![], "{height_mm}");
    }
    // Any size, the same proportions.
    let p = bitmap_with(|bitmap| {
        bitmap.width_mm = 1.0;
        bitmap.height_mm = 0.5;
    });
    assert_eq!(validate(&p), vec![]);
}

/// Security review M4: a footprint has a width and a height above 0,
/// and both its corners are on the coordinate range, 6000 mm from the
/// origin, as every coordinate of a project is.
#[test]
fn bitmap_footprint_must_be_positive_and_on_the_coordinate_range() {
    use crate::ids::BitmapId;
    for (width_mm, height_mm, field, value) in [
        (0.0, 20.0, "width_mm", 0.0),
        (-40.0, 20.0, "width_mm", -40.0),
        (40.0, 0.0, "height_mm", 0.0),
        (40.0, -20.0, "height_mm", -20.0),
    ] {
        let p = bitmap_with(|bitmap| {
            bitmap.width_mm = width_mm;
            bitmap.height_mm = height_mm;
        });
        let problem = only_error(
            &p,
            "bitmap_footprint_positive",
            &format!("bitmaps[b1].{field}"),
        );
        assert_eq!(
            problem,
            Problem::BitmapFootprintPositive {
                bitmap: BitmapId::new("b1"),
                name: "ramp".into(),
                field,
                value,
            },
            "{width_mm} × {height_mm}"
        );
    }
    // Both at fault: both named, and no aspect on top.
    let p = bitmap_with(|bitmap| {
        bitmap.width_mm = 0.0;
        bitmap.height_mm = 0.0;
    });
    let codes: Vec<&str> = validate(&p).iter().map(Problem::code).collect();
    assert_eq!(
        codes,
        ["bitmap_footprint_positive", "bitmap_footprint_positive"]
    );

    // The near corner.
    for (field, x_mm, y_mm) in [
        ("x_mm", 6001.0, 15.0),
        ("x_mm", -6001.0, 15.0),
        ("y_mm", 15.0, 6001.0),
        ("y_mm", 15.0, -6001.0),
    ] {
        let p = bitmap_with(|bitmap| {
            bitmap.x_mm = x_mm;
            bitmap.y_mm = y_mm;
        });
        // Reported once, at the corner, not again at the far one.
        let problem = only_error(&p, "out_of_range", &format!("bitmaps[b1].{field}"));
        assert!(
            matches!(problem, Problem::OutOfRange { expected, .. } if expected == COORDINATE),
            "{problem:?}"
        );
    }
    // The far corner: `x_mm + width_mm`, `y_mm + height_mm`.
    let p = bitmap_with(|bitmap| bitmap.x_mm = 5990.0);
    assert_eq!(
        validate(&p),
        vec![Problem::OutOfRange {
            field: "bitmaps[b1].width_mm".into(),
            value: 6030.0,
            expected: BITMAP_REACH,
        }]
    );
    let p = bitmap_with(|bitmap| bitmap.y_mm = 5990.0);
    assert_eq!(
        validate(&p),
        vec![Problem::OutOfRange {
            field: "bitmaps[b1].height_mm".into(),
            value: 6010.0,
            expected: BITMAP_REACH,
        }]
    );
    assert!(BITMAP_REACH.contains("6000 mm"), "{BITMAP_REACH}");
    // On the range, though off the stock: where a picture lies on the
    // stock is the generator's and the safety pass's (S19).
    let p = bitmap_with(|bitmap| {
        bitmap.x_mm = -5990.0;
        bitmap.y_mm = 5980.0;
    });
    assert_eq!(validate(&p), vec![]);
}

/// A bitmap's id is its own: a Photo V-Carve names one picture by it.
#[test]
fn duplicate_bitmap_id_is_error() {
    let mut p = photo_project();
    p.bitmaps.push(crate::test_support::sample_bitmap());
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::DuplicateBitmapId {
            id: crate::ids::BitmapId::new("b1"),
        }]
    );
    assert_eq!(problems[0].severity(), Severity::Error);
    assert_eq!(problems[0].code(), "duplicate_bitmap_id");
    assert_eq!(problems[0].field(), None);
    assert_eq!(problems[0].to_string(), "bitmap id `b1` is used twice");
    // A second picture under its own id is fine, used or not.
    p.bitmaps[1].id = crate::ids::BitmapId::new("b2");
    assert_eq!(validate(&p), vec![]);
}

/// `width_px` and `height_px` are the stored PNG's size. The check
/// reads the PNG's header alone.
#[test]
fn bitmap_pixels_must_match_the_png() {
    // 8 × 4 keeps the footprint's proportions, so only the size is off.
    let p = bitmap_with(|bitmap| {
        bitmap.width_px = 8;
        bitmap.height_px = 4;
    });
    let problem = only_error(&p, "bitmap_pixels_mismatch", "bitmaps[b1].width_px");
    assert_eq!(
        problem,
        Problem::BitmapPixelsMismatch {
            bitmap: crate::ids::BitmapId::new("b1"),
            name: "ramp".into(),
            width_px: 8,
            height_px: 4,
            png_width_px: 4,
            png_height_px: 2,
        }
    );
    assert_eq!(
        problem.to_string(),
        "bitmap `ramp` (`b1`) says it is 8 × 4 px, but its PNG is 4 × 2 px; set width_px and height_px to the PNG's size"
    );
    // The limit is inclusive: 8192 × 4096 is a size a picture may
    // have, so here only the PNG disagrees.
    let p = bitmap_with(|bitmap| {
        bitmap.width_px = MAX_BITMAP_SIDE_PX;
        bitmap.height_px = MAX_BITMAP_SIDE_PX / 2;
    });
    only_error(&p, "bitmap_pixels_mismatch", "bitmaps[b1].width_px");
}

/// A project stores 8-bit grayscale: DreamCarve wrote the PNG, so a
/// hand edit is the only way to another kind. Read from the header.
#[test]
fn bitmap_png_must_be_8_bit_grayscale() {
    use crate::bitmap::BitmapColour;
    use crate::test_support::png_base64;
    let cases = [
        (
            png::ColorType::Rgb,
            png::BitDepth::Eight,
            3,
            BitmapColour::Rgb,
            8,
        ),
        (
            png::ColorType::Grayscale,
            png::BitDepth::Sixteen,
            2,
            BitmapColour::Grayscale,
            16,
        ),
    ];
    for (colour_type, depth, bytes_per_pixel, colour, bit_depth) in cases {
        let p = bitmap_with(|bitmap| {
            bitmap.png_base64 = png_base64(4, 2, colour_type, depth, &vec![7; 8 * bytes_per_pixel]);
        });
        let problem = only_error(&p, "bitmap_not_grayscale_8", "bitmaps[b1].png_base64");
        assert_eq!(
            problem,
            Problem::BitmapNotGrayscale8 {
                bitmap: crate::ids::BitmapId::new("b1"),
                name: "ramp".into(),
                colour,
                bit_depth,
            },
            "{colour_type:?}"
        );
    }
    let p = bitmap_with(|bitmap| {
        bitmap.png_base64 = png_base64(4, 2, png::ColorType::Rgb, png::BitDepth::Eight, &[7; 24]);
    });
    assert_eq!(
        validate(&p)[0].to_string(),
        "the PNG of bitmap `ramp` (`b1`) is 8-bit RGB, not 8-bit grayscale; import the picture again so that DreamCarve converts it"
    );
}

/// A picture has at least one pixel each way.
#[test]
fn bitmap_with_no_pixels_is_error() {
    for (width_px, height_px, field) in [(0, 2, "width_px"), (4, 0, "height_px")] {
        let p = bitmap_with(|bitmap| {
            bitmap.width_px = width_px;
            bitmap.height_px = height_px;
        });
        let problem = only_error(&p, "bitmap_empty", &format!("bitmaps[b1].{field}"));
        assert_eq!(
            problem,
            Problem::BitmapEmpty {
                bitmap: crate::ids::BitmapId::new("b1"),
                name: "ramp".into(),
                width_px,
                height_px,
            },
            "{width_px} × {height_px}"
        );
    }
    // Both 0: reported once, at the width.
    let p = bitmap_with(|bitmap| {
        bitmap.width_px = 0;
        bitmap.height_px = 0;
    });
    only_error(&p, "bitmap_empty", "bitmaps[b1].width_px");
}

/// Security review M4: a side above `MAX_BITMAP_SIDE_PX`, 8192, is an
/// error from the bitmap's own numbers, before anything is decoded.
#[test]
fn bitmap_above_the_side_limit_is_error() {
    assert_eq!(MAX_BITMAP_SIDE_PX, 8192);
    for (width_px, height_px, field) in [
        (9000, 4500, "width_px"),
        (8193, 8193, "width_px"),
        (4, 8193, "height_px"),
    ] {
        let p = bitmap_with(|bitmap| {
            bitmap.width_px = width_px;
            bitmap.height_px = height_px;
            // The footprint follows the pixels, so only the size is off.
            bitmap.width_mm = f64::from(width_px) / 100.0;
            bitmap.height_mm = f64::from(height_px) / 100.0;
        });
        let problem = only_error(&p, "bitmap_too_large", &format!("bitmaps[b1].{field}"));
        assert_eq!(
            problem,
            Problem::BitmapTooLarge {
                bitmap: crate::ids::BitmapId::new("b1"),
                name: "ramp".into(),
                width_px,
                height_px,
                limit_px: 8192,
            },
            "{width_px} × {height_px}"
        );
    }
    let p = bitmap_with(|bitmap| {
        bitmap.width_px = 9000;
        bitmap.height_px = 4500;
    });
    assert_eq!(
        validate(&p)[0].to_string(),
        "bitmap `ramp` (`b1`) is 9000 × 4500 px; a picture is at most 8192 px on a side, so scale it down before adding it"
    );
}

/// A project built in code can hold a `png_base64` that `load` would
/// have refused: validation names it instead of passing it on to the
/// generator. (Not in the design's list; #644 records it.)
#[test]
fn bitmap_that_cannot_be_read_is_error() {
    use crate::bitmap::BitmapError;
    for (text, base64) in [("QUJD", false), ("not base64!", true), ("", false)] {
        let p = bitmap_with(|bitmap| bitmap.png_base64 = text.into());
        let problems = validate(&p);
        assert_eq!(problems.len(), 1, "{text:?}: {problems:?}");
        let Problem::BitmapUnreadable {
            bitmap,
            name,
            reason,
        } = &problems[0]
        else {
            panic!("{text:?}: {problems:?}");
        };
        assert_eq!(bitmap.as_str(), "b1");
        assert_eq!(name, "ramp");
        assert_eq!(
            matches!(reason, BitmapError::Base64 { .. }),
            base64,
            "{reason:?}"
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "bitmap_unreadable");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("bitmaps[b1].png_base64")
        );
    }
}

/// Security review M4: validation reads a picture's header and never
/// its image data, so a shared file costs nothing per pixel to check.
/// A PNG that is a valid header followed by bytes that are no image
/// data validates clean; `pixels` is where it fails.
#[test]
fn validation_reads_only_the_png_header() {
    use base64::Engine as _;
    let engine = base64::engine::general_purpose::STANDARD;
    let mut png = engine
        .decode(photo_project().bitmaps[0].png_base64.clone())
        .unwrap();
    for byte in png.iter_mut().skip(33) {
        *byte = 0x55;
    }
    let p = bitmap_with(|bitmap| bitmap.png_base64 = engine.encode(&png));
    assert_eq!(validate(&p), vec![]);
    assert!(p.bitmaps[0].pixels().is_err());
}

/// Every bitmap and Photo V-Carve problem has its own stable code, and
/// only the overlap is a warning.
#[test]
fn bitmap_and_photo_problems_have_their_own_codes() {
    use crate::bitmap::{BitmapColour, BitmapError};
    use crate::ids::BitmapId;
    let operation = OperationId::new("o8");
    let bitmap = BitmapId::new("b1");
    let name = String::from("n");
    let problems = [
        Problem::PhotoLineAngleRange {
            operation: operation.clone(),
            name: name.clone(),
            line_angle_deg: 180.0,
        },
        Problem::PhotoLineSpacingPositive {
            operation: operation.clone(),
            name: name.clone(),
            line_spacing_fraction: 0.0,
        },
        Problem::PhotoLinesOverlap {
            operation: operation.clone(),
            name: name.clone(),
            line_spacing_fraction: 0.5,
        },
        Problem::PhotoSamplePositive {
            operation: operation.clone(),
            name: name.clone(),
            sample_mm: 0.0,
        },
        Problem::PhotoRetractPositive {
            operation: operation.clone(),
            name: name.clone(),
            stepover_retract_mm: 0.0,
        },
        Problem::PhotoUnknownBitmap {
            operation,
            name: name.clone(),
            bitmap: bitmap.clone(),
        },
        Problem::BitmapFootprintAspect {
            bitmap: bitmap.clone(),
            name: name.clone(),
            width_mm: 1.0,
            height_mm: 1.0,
            width_px: 2,
            height_px: 1,
        },
        Problem::BitmapPixelsMismatch {
            bitmap: bitmap.clone(),
            name: name.clone(),
            width_px: 1,
            height_px: 1,
            png_width_px: 2,
            png_height_px: 2,
        },
        Problem::BitmapNotGrayscale8 {
            bitmap: bitmap.clone(),
            name: name.clone(),
            colour: BitmapColour::Rgb,
            bit_depth: 8,
        },
        Problem::DuplicateBitmapId { id: bitmap.clone() },
        Problem::BitmapEmpty {
            bitmap: bitmap.clone(),
            name: name.clone(),
            width_px: 0,
            height_px: 0,
        },
        Problem::BitmapTooLarge {
            bitmap: bitmap.clone(),
            name: name.clone(),
            width_px: 9000,
            height_px: 1,
            limit_px: 8192,
        },
        Problem::BitmapFootprintPositive {
            bitmap: bitmap.clone(),
            name: name.clone(),
            field: "width_mm",
            value: 0.0,
        },
        Problem::BitmapUnreadable {
            bitmap,
            name,
            reason: BitmapError::Png {
                message: "x".into(),
            },
        },
    ];
    let codes: Vec<&str> = problems.iter().map(Problem::code).collect();
    assert_eq!(
        codes,
        [
            "photo_line_angle_range",
            "photo_line_spacing_positive",
            "photo_lines_overlap",
            "photo_sample_positive",
            "photo_retract_positive",
            "photo_unknown_bitmap",
            "bitmap_footprint_aspect",
            "bitmap_pixels_mismatch",
            "bitmap_not_grayscale_8",
            "duplicate_bitmap_id",
            "bitmap_empty",
            "bitmap_too_large",
            "bitmap_footprint_positive",
            "bitmap_unreadable",
        ]
    );
    for problem in &problems {
        assert_eq!(
            problem.is_error(),
            problem.code() != "photo_lines_overlap",
            "{problem:?}"
        );
        let message = problem.to_string();
        assert!(!message.is_empty() && !message.ends_with('.'), "{message}");
    }
}

// Format 28 (sub-phase 3.6, #677): the tapered ball nose.

/// The sample project with [`crate::test_support::tool_tbn_ultra`]
/// (`t2`) edited by `edit`, used by no operation.
fn unused_tbn(edit: impl FnOnce(&mut Tool)) -> Project {
    let mut p = sample_project();
    let mut tool = crate::test_support::tool_tbn_ultra();
    edit(&mut tool);
    p.tools.push(tool);
    p
}

/// The Ultra bit's cutter values, to edit.
fn tbn_of(tool: &mut Tool) -> (&mut Option<f64>, &mut Option<f64>, &mut Option<f64>) {
    match &mut tool.cutter {
        ToolKind::TaperedBallNose {
            tip_diameter_mm,
            taper_angle_deg,
            flute_length_mm,
        } => (tip_diameter_mm, taper_angle_deg, flute_length_mm),
        cutter => panic!("not a tapered ball nose: {cutter:?}"),
    }
}

/// The error codes of `problems`, each with its field.
fn coded(problems: &[Problem]) -> Vec<(&'static str, Option<String>)> {
    errors(problems)
        .into_iter()
        .map(|p| (p.code(), p.field()))
        .collect()
}

const TBN_TAPER: &str = "tools[t2].cutter.tapered_ball_nose.taper_angle_deg";
const TBN_TIP: &str = "tools[t2].cutter.tapered_ball_nose.tip_diameter_mm";
const TBN_FLUTES: &str = "tools[t2].cutter.tapered_ball_nose.flute_length_mm";

/// The taper per side is in (0, 45]: anything else, or not a number,
/// is `tapered_ball_nose_range` at the field, never the general
/// `tool_out_of_range` or `non_finite`, on a tool used or not.
#[test]
fn tapered_ball_nose_taper_out_of_range_is_error() {
    for taper in [0.0, -1.0, 45.1, f64::NAN, f64::INFINITY] {
        let p = unused_tbn(|t| *tbn_of(t).1 = Some(taper));
        let problems = validate(&p);
        assert_eq!(
            coded(&problems),
            [("tapered_ball_nose_range", Some(TBN_TAPER.to_owned()))],
            "{taper}"
        );
        let message = errors(&problems)[0].to_string();
        assert!(message.contains("per side"), "{message}");
    }
    let p = unused_tbn(|t| *tbn_of(t).1 = Some(45.0));
    assert_eq!(coded(&validate(&p)), []);
    // The tip, as any length, is above 0.
    for tip in [0.0, -0.5, f64::NAN] {
        let p = unused_tbn(|t| *tbn_of(t).0 = Some(tip));
        assert_eq!(
            coded(&validate(&p)),
            [("tapered_ball_nose_range", Some(TBN_TIP.to_owned()))],
            "{tip}"
        );
    }
}

/// The flutes must end above the tangent height, where the ball meets
/// the cone: 0.337 mm on the Ultra bit.
#[test]
fn tapered_ball_nose_flute_must_pass_the_ball() {
    for flutes in [0.3, 0.0, -1.0, f64::INFINITY] {
        let p = unused_tbn(|t| *tbn_of(t).2 = Some(flutes));
        assert_eq!(
            coded(&validate(&p)),
            [("tapered_ball_nose_range", Some(TBN_FLUTES.to_owned()))],
            "{flutes}"
        );
    }
    let p = unused_tbn(|t| *tbn_of(t).2 = Some(0.35));
    assert_eq!(coded(&validate(&p)), []);
    // With the taper unset there is no tangent height, but the flutes
    // still need a length.
    let p = unused_tbn(|t| {
        *tbn_of(t).1 = None;
        *tbn_of(t).2 = Some(0.0);
    });
    assert_eq!(
        coded(&validate(&p)),
        [("tapered_ball_nose_range", Some(TBN_FLUTES.to_owned()))]
    );
}

/// The cutting diameter must be wider than the ball, or there is no
/// cone: equal to the tip is the same error, and never an unset value.
#[test]
fn cut_diameter_must_exceed_the_tip() {
    for diameter in [0.7, 0.762] {
        let p = unused_tbn(|t| t.diameter_mm = diameter);
        let problems = validate(&p);
        assert_eq!(
            coded(&problems),
            [(
                "tapered_ball_nose_range",
                Some("tools[t2].diameter_mm".to_owned())
            )],
            "{diameter}"
        );
        assert!(
            !problems
                .iter()
                .any(|q| matches!(q, Problem::ToolValueUnset { .. })),
            "{problems:?}"
        );
    }
    // A diameter not above 0 is the general rule's, as on any tool.
    let p = unused_tbn(|t| t.diameter_mm = 0.0);
    let codes: Vec<&str> = errors(&validate(&p)).iter().map(|q| q.code()).collect();
    assert!(codes.contains(&"tool_out_of_range"), "{codes:?}");
}

/// Each unset cutter value of a tool an enabled operation uses is
/// `tool_value_unset` at its field; an unused tool's are not reported.
#[test]
fn unset_tapered_value_on_a_used_tool_is_tool_value_unset() {
    for (k, field) in [TBN_TIP, TBN_TAPER, TBN_FLUTES].into_iter().enumerate() {
        let mut p = crate::test_support::tapered_project();
        let (tip, taper, flutes) = tbn_of(&mut p.tools[1]);
        *[tip, taper, flutes].into_iter().nth(k).unwrap() = None;
        let problems = validate(&p);
        let unset: Vec<Option<String>> = problems
            .iter()
            .filter(|q| matches!(q, Problem::ToolValueUnset { .. }))
            .map(Problem::field)
            .collect();
        assert_eq!(unset, [Some(field.to_owned())], "{problems:?}");
        assert!(
            problems
                .iter()
                .all(|q| q.code() != "tapered_ball_nose_range"),
            "{problems:?}"
        );
        let message = problems
            .iter()
            .find(|q| q.is_unset_value())
            .unwrap()
            .to_string();
        assert!(message.contains(field), "{message}");
    }
    let p = unused_tbn(|t| {
        let (tip, taper, flutes) = tbn_of(t);
        *tip = None;
        *taper = None;
        *flutes = None;
    });
    let problems = validate(&p);
    assert_eq!(coded(&problems), []);
    assert_eq!(
        problems.iter().map(Problem::code).collect::<Vec<_>>(),
        ["unused_tool"]
    );
}

/// A Photo V-Carve needs a V-bit for good: a tapered ball nose is not
/// supported there.
#[test]
fn tapered_ball_nose_on_photo_is_v_carve_needs_v_bit() {
    let mut p = photo_project();
    p.tools[1] = crate::test_support::tool_tbn_ultra();
    let problems = validate(&p);
    assert_eq!(
        coded(&problems),
        [(
            "v_carve_needs_v_bit",
            Some("operations[o8].tool".to_owned())
        )]
    );
    let message = errors(&problems)[0].to_string();
    assert!(
        message
            .contains("a Photo V-Carve needs a V-bit; a tapered ball nose is not supported there"),
        "{message}"
    );
}

#[test]
fn tapered_ball_nose_countersink_is_error() {
    let mut p = countersink_project(|_| {});
    p.tools[1] = crate::test_support::tool_tbn_ultra();
    let problems = validate(&p);
    assert_eq!(
        problems,
        vec![Problem::CountersinkNeedsVBit {
            operation: OperationId::new("o3"),
            name: "Drill".into(),
            tool: ToolId::new("t2"),
            tool_name: "IDC Ultra-High Detail".into(),
            cutter: "a tapered ball nose",
        }]
    );
}

/// Slice 27c (#679): a V-Carve and an Inlay accept a tapered ball
/// nose, with no problem at all; any other cutter that is not a V-bit
/// is still `v_carve_needs_v_bit`, its message naming both bits that
/// carve.
#[test]
fn tapered_ball_nose_is_accepted_by_v_carve_and_inlay() {
    let mut carve = carve_project(|_| {});
    carve.tools[1] = crate::test_support::tool_tbn_ultra();
    // A glue gap at or above the Ultra bit's 0.337 mm tangent height
    // (#681: below it, `inlay_glue_gap_below_ball` warns).
    let mut inlay = inlay_project(|inlay| inlay.glue_gap_mm = Some(0.5));
    inlay.tools[1] = crate::test_support::tool_tbn_ultra();
    for p in [carve, inlay] {
        let problems = validate(&p);
        assert_eq!(problems, vec![], "{problems:?}");
    }
    let mut end_mill = carve_project(|_| {});
    end_mill.tools[1].cutter = ToolKind::EndMill;
    let problems = validate(&end_mill);
    let message = errors(&problems)[0].to_string();
    assert!(
        message.contains("needs a V-bit or a tapered ball nose"),
        "{message}"
    );
}

/// Slice 27c (#679): a tapered ball nose's limit is its cutting
/// height, `min(flute_length_mm, h_R)`, and `v_bit_cone_exceeded` says
/// which: the Ultra bit's 19.05 mm flutes, or the Fine bit's cone
/// reaching its 6.35 mm cutting diameter 18.374 mm up.
#[test]
fn tapered_ball_nose_cone_exceeded_names_its_limit() {
    let deep = |tool: Tool, flat_depth_mm: f64| {
        let mut p = carve_project(|c| c.flat_depth_mm = Some(flat_depth_mm));
        p.stock.thickness_mm = Some(30.0);
        for instance in &mut p.instances {
            instance.z_top_mm = Some(30.0);
        }
        p.tools[1] = Tool {
            id: ToolId::new("t2"),
            ..tool
        };
        validate(&p)
    };
    let problems = deep(crate::test_support::tool_tbn_ultra(), 21.2);
    let (engagement_mm, limit_mm, _, field) = cone_exceeded(&problems);
    assert!((engagement_mm - 21.2).abs() < 1e-9);
    assert!((limit_mm - 19.05).abs() < 1e-9);
    assert_eq!(field.as_deref(), Some("operations[o6].flat_depth_mm"));
    let message = problems[0].to_string();
    assert!(
        message.contains("engaged 21.200 mm, past the bit's 19.050 mm flutes"),
        "{message}"
    );

    let problems = deep(crate::test_support::tool_tbn_fine(), 18.5);
    let (_, limit_mm, _, _) = cone_exceeded(&problems);
    assert!((limit_mm - 18.374).abs() < 1e-3);
    let message = problems[0].to_string();
    assert!(
        message.contains("past 18.374 mm, where it reaches its 6.350 mm cutting diameter"),
        "{message}"
    );
    assert_eq!(deep(crate::test_support::tool_tbn_fine(), 18.3), vec![]);
}

/// Slice 27e (#681, question 32): a male Inlay's engagement from its
/// board's top, `D − G + P`, past a tapered ball nose's cutting height
/// is `v_bit_cone_exceeded` at `plug_depth_mm`, an error naming the
/// flutes (Ultra, 19.05 mm) or the cutting diameter (Fine, 18.374 mm).
#[test]
fn tapered_inlay_engagement_beyond_the_flute_is_error() {
    let deep = |tool: Tool, plug_depth_mm: f64| {
        let mut p = inlay_project(|inlay| {
            star(inlay, Board::Male);
            inlay.depth_mm = Some(12.0);
            // Above both bits' tangent heights.
            inlay.glue_gap_mm = Some(1.5);
            inlay.plug_depth_mm = Some(plug_depth_mm);
            inlay.male_board.thickness_mm = Some(30.0);
        });
        p.stock.thickness_mm = Some(30.0);
        p.tools[1] = Tool {
            id: ToolId::new("t2"),
            ..tool
        };
        validate(&p)
    };
    let problems = deep(crate::test_support::tool_tbn_ultra(), 9.0);
    let (engagement_mm, limit_mm, from_stock_top, field) = cone_exceeded(&problems);
    assert!((engagement_mm - 19.5).abs() < 1e-9, "{engagement_mm}");
    assert!((limit_mm - 19.05).abs() < 1e-9, "{limit_mm}");
    assert!(from_stock_top);
    assert_eq!(field.as_deref(), Some("operations[o7].plug_depth_mm"));
    assert!(
        problems[0].to_string().contains("19.050 mm flutes"),
        "{}",
        problems[0]
    );
    let problems = deep(crate::test_support::tool_tbn_fine(), 9.0);
    let (_, limit_mm, _, _) = cone_exceeded(&problems);
    assert!((limit_mm - 18.374).abs() < 1e-3, "{limit_mm}");
    assert!(
        problems[0]
            .to_string()
            .contains("6.350 mm cutting diameter"),
        "{}",
        problems[0]
    );
    // 10.5 + 7 = 17.5 mm is within both.
    assert_eq!(deep(crate::test_support::tool_tbn_ultra(), 7.0), vec![]);
    assert_eq!(deep(crate::test_support::tool_tbn_fine(), 7.0), vec![]);
}

/// Slice 27e (#681; functional 49k): an Inlay cut with a tapered ball
/// nose warns `inlay_glue_gap_below_ball` at `glue_gap_mm` and
/// `inlay_plug_depth_below_ball` at `plug_depth_mm` when either is
/// below the bit's tangent height (the Fine bit's 1.010 mm), naming
/// what each costs: with `G` 0.25 a 0.432 mm lift and a 0.051 mm wall
/// gap, with `P` 0.5 a 0.130 mm gap. Warnings, once; a V-bit's never.
#[test]
fn tapered_inlay_glue_gap_and_plug_depth_below_the_ball_warn() {
    let mut p = inlay_project(|inlay| {
        star(inlay, Board::Male);
        inlay.glue_gap_mm = Some(0.25);
        inlay.plug_depth_mm = Some(0.5);
    });
    p.tools[1] = Tool {
        id: ToolId::new("t2"),
        ..crate::test_support::tool_tbn_fine()
    };
    let problems = validate(&p);
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems.iter().all(|p| p.severity() == Severity::Warning));
    let Problem::InlayGlueGapBelowBall {
        tangent_height_mm,
        lift_mm,
        wall_gap_mm,
        ..
    } = &problems[0]
    else {
        panic!("{problems:?}");
    };
    assert_eq!(problems[0].code(), "inlay_glue_gap_below_ball");
    assert_eq!(
        problems[0].field().as_deref(),
        Some("operations[o7].glue_gap_mm")
    );
    assert!((tangent_height_mm - 1.010).abs() < 5e-4);
    assert!((lift_mm - 0.432).abs() < 5e-4, "{lift_mm}");
    assert!((wall_gap_mm - 0.051).abs() < 5e-4, "{wall_gap_mm}");
    let Problem::InlayPlugDepthBelowBall { wall_gap_mm, .. } = &problems[1] else {
        panic!("{problems:?}");
    };
    assert_eq!(problems[1].code(), "inlay_plug_depth_below_ball");
    assert_eq!(
        problems[1].field().as_deref(),
        Some("operations[o7].plug_depth_mm")
    );
    assert!((wall_gap_mm - 0.130).abs() < 5e-4, "{wall_gap_mm}");
    let message = problems[0].to_string();
    assert!(
        message.contains("below its tapered ball nose's tangent height 1.010 mm"),
        "{message}"
    );
    // A V-bit has no ball.
    let mut v_bit = p.clone();
    v_bit_angle(&mut v_bit, 60.0);
    assert_eq!(validate(&v_bit), vec![]);
}
