// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Placed tabs (#105): the tests, before the module.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::generate_profile;
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind};
    use crate::warning::WarningCode;
    use dc_geometry::{Point3, Transform, assert_approx_eq};
    use dc_project::test_support::{profile_mut, sample_pocket, sample_project, sample_tab};
    use dc_project::{
        CutDirection, DefinitionId, Entry, FeatureDefinition, InstanceId, Lead, Side, TabPlacement,
    };

    const TOL: f64 = 1e-9;
    /// The tab top of the tabs these tests place, mm above the stock
    /// bottom: below the last pass only (the passes are at 4, 2 and 0).
    const TAB_TOP_MM: f64 = 1.0;

    /// [`sample_tab`] with its top at [`TAB_TOP_MM`].
    fn tab(id: &str, x_mm: f64, y_mm: f64) -> PlacedTab {
        PlacedTab {
            height_mm: TAB_TOP_MM,
            ..sample_tab(id, x_mm, y_mm)
        }
    }

    /// The sample's Profile, generated and checked for safety.
    fn generate(project: &Project) -> (Toolpath, Vec<Warning>) {
        let op = project
            .operations
            .iter()
            .find(|o| matches!(o, Operation::Profile(_)))
            .expect("a Profile");
        generate_of(project, op)
    }

    fn generate_of(project: &Project, op: &Operation) -> (Toolpath, Vec<Warning>) {
        let Operation::Profile(profile) = op else {
            panic!("a Profile, not {op:?}");
        };
        let (toolpath, warnings) = generate_profile(project, profile).unwrap();
        assert_safe(&toolpath, project, op);
        (toolpath, warnings)
    }

    /// The sample with no tab request: an untabbed Profile.
    fn untabbed() -> Project {
        let mut p = sample_project();
        profile_mut(&mut p).tabs = None;
        p
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(Warning::code).collect()
    }

    /// `a` and `b` are the same motion: the same moves of the same kinds,
    /// every coordinate and feed within [`TOL`].
    fn assert_same_motion(a: &Toolpath, b: &Toolpath, case: &str) {
        assert_eq!(a.moves.len(), b.moves.len(), "{case}: move count");
        let near = |p: Point3, q: Point3| {
            (p.x - q.x).abs() <= TOL && (p.y - q.y).abs() <= TOL && (p.z - q.z).abs() <= TOL
        };
        for (i, (m, n)) in a.moves.iter().zip(&b.moves).enumerate() {
            let kinds_match = match (m.kind, n.kind) {
                (MoveKind::ArcCw { centre: c }, MoveKind::ArcCw { centre: d })
                | (MoveKind::ArcCcw { centre: c }, MoveKind::ArcCcw { centre: d }) => {
                    c.distance_to(d) <= TOL
                }
                (k, l) => std::mem::discriminant(&k) == std::mem::discriminant(&l),
            };
            let feeds_match = match (m.feed_mm_min, n.feed_mm_min) {
                (Some(f), Some(g)) => (f - g).abs() <= TOL,
                (None, None) => true,
                _ => false,
            };
            assert!(
                kinds_match && feeds_match && near(m.to, n.to),
                "{case}: move {i} differs: {m:?} vs {n:?}"
            );
        }
    }

    /// Every cutting move with the point it starts from.
    fn cuts(tp: &Toolpath) -> Vec<(Point3, Move)> {
        tp.moves
            .windows(2)
            .filter_map(|w| match w {
                [from, mv] if !mv.is_rapid() => Some((from.to, *mv)),
                _ => None,
            })
            .collect()
    }

    /// The runs of consecutive cutting moves held at `z_mm` while moving
    /// in XY: one per tab top crossed.
    fn held_runs(tp: &Toolpath, z_mm: f64) -> Vec<Vec<(Point3, Move)>> {
        let mut runs: Vec<Vec<(Point3, Move)>> = Vec::new();
        let mut open = false;
        for (from, mv) in cuts(tp) {
            let held = (from.z - z_mm).abs() <= TOL
                && (mv.to.z - z_mm).abs() <= TOL
                && from.xy().distance_to(mv.to.xy()) > EPS_MM;
            match (held, open) {
                (true, true) => {
                    if let Some(run) = runs.last_mut() {
                        run.push((from, mv));
                    }
                }
                (true, false) => runs.push(vec![(from, mv)]),
                _ => {}
            }
            open = held;
        }
        runs
    }

    /// A change to the sample's Profile.
    type Edit = Box<dyn Fn(&mut ProfileOp)>;

    /// #105, ADR-0005: placing a request writes tabs that cut exactly
    /// what the request cut: Outside both ways, an odd count whose middle
    /// tab sits on the arc round a corner, Inside at distances, On line,
    /// and with a ramp and a lead.
    #[test]
    fn placed_requests_cut_what_the_requests_cut() {
        let smooth = |placement: TabPlacement| TabSpec {
            placement,
            width_mm: 6.0,
            height_mm: TAB_TOP_MM,
            shape: TabShape::Smooth,
            on_holes: false,
        };
        let cases: Vec<(&str, Edit)> = vec![
            ("outside climb", Box::new(|_| {})),
            (
                "outside conventional, three smooth",
                Box::new(move |op| {
                    op.direction = CutDirection::Conventional;
                    op.tabs = Some(smooth(TabPlacement::Count { count: 3 }));
                }),
            ),
            (
                "inside at distances",
                Box::new(move |op| {
                    op.side = Side::Inside;
                    op.tabs = Some(smooth(TabPlacement::AtDistances {
                        distances_mm: vec![5.0, 40.0, 77.7, 300.0],
                    }));
                }),
            ),
            (
                "on line, five",
                Box::new(|op| {
                    op.side = Side::OnLine;
                    if let Some(tabs) = &mut op.tabs {
                        tabs.placement = TabPlacement::Count { count: 5 };
                    }
                }),
            ),
            (
                "ramp and lead",
                Box::new(move |op| {
                    op.entry = Entry::Ramp { angle_deg: 15.0 };
                    op.lead = Some(Lead {
                        radius_mm: 3.0,
                        sweep_deg: 90.0,
                        lead_out: true,
                        overcut_mm: 1.0,
                    });
                    op.tabs = Some(smooth(TabPlacement::Count { count: 4 }));
                }),
            ),
        ];
        for (case, edit) in cases {
            let mut p = sample_project();
            edit(profile_mut(&mut p));
            let (asked, _) = generate(&p);
            let (placed, _) = place_requests(&p).unwrap();
            assert_eq!(profile_mut(&mut placed.clone()).tabs, None, "{case}");
            assert!(!placed.tabs.is_empty(), "{case}: tabs placed");
            let (cut, _) = generate(&placed);
            assert_same_motion(&asked, &cut, case);
        }
    }

    /// #105, the issue's third test: a placed tab that no enabled Profile
    /// crosses warns `tab_not_crossed`: on an outline no Profile cuts, or
    /// cut only by a disabled Profile or by a Pocket. A tab the Profile
    /// cuts round does not warn.
    #[test]
    fn tab_no_enabled_operation_crosses_warns() {
        let mut p = untabbed();
        // A second rectangle at (70, 60), which the Profile does not cut.
        let mut other = p.instances[0].clone();
        other.id = InstanceId::new("i2");
        other.name = "Rectangle 2".into();
        other.transform = Transform::translate(70.0, 60.0);
        p.instances.push(other);
        // The middle of each rectangle's bottom edge.
        p.tabs = vec![
            tab("tab1", 25.0, 0.0),
            PlacedTab {
                instance: InstanceId::new("i2"),
                ..tab("tab2", 25.0, 0.0)
            },
        ];
        let op = p.operations[0].clone();
        let (tp, _) = generate(&p);
        let warnings = check(&p, &[(&op, &tp)]);
        assert_eq!(
            warnings,
            vec![Warning::TabNotCrossed {
                tab: TabId::new("tab2"),
                shape: ShapeRef {
                    instance: InstanceId::new("i2"),
                    instance_name: "Rectangle 2".into(),
                    profile: 0,
                },
            }]
        );
        assert!(warnings[0].op().is_none());
        // Disabled, it is not generated, so nothing crosses either tab.
        profile_mut(&mut p).enabled = false;
        assert_eq!(codes(&check(&p, &[])), vec![WarningCode::TabNotCrossed; 2]);
        // A Pocket of the outline does not cut round tabs.
        let pocket = Operation::Pocket(sample_pocket());
        let (pocket_tp, _) = crate::generate_pocket(&p, &sample_pocket()).unwrap();
        assert_eq!(
            codes(&check(&p, &[(&pocket, &pocket_tp)])),
            vec![WarningCode::TabNotCrossed; 2]
        );
    }

    /// #105: a tab is a point in its instance's definition space, so it
    /// moves with the instance. The sample turned a quarter turn and moved
    /// to (80, 20) puts the middle of its bottom edge at (80, 45), with
    /// the outward side towards +X: the last pass holds the tab top along
    /// X 83.175 for the tab width plus the bit, centred on Y 45.
    #[test]
    fn tab_follows_its_instance_when_it_moves() {
        let mut p = untabbed();
        p.instances[0].transform =
            Transform::rotate(std::f64::consts::FRAC_PI_2).then(Transform::translate(80.0, 20.0));
        p.tabs = vec![tab("tab1", 25.0, 0.0)];
        let (tp, warnings) = generate(&p);
        assert!(warnings.is_empty(), "{warnings:?}");
        let runs = held_runs(&tp, TAB_TOP_MM);
        assert_eq!(runs.len(), 1, "{runs:?}");
        let ys: Vec<f64> = runs[0]
            .iter()
            .flat_map(|(from, mv)| [from.y, mv.to.y])
            .collect();
        for (from, mv) in &runs[0] {
            assert_approx_eq!(from.x, 83.175, 1e-6);
            assert_approx_eq!(mv.to.x, 83.175, 1e-6);
        }
        let lo = ys.iter().copied().fold(f64::MAX, f64::min);
        let hi = ys.iter().copied().fold(f64::MIN, f64::max);
        assert_approx_eq!((lo + hi) / 2.0, 45.0, 1e-6);
        assert_approx_eq!(hi - lo, 6.0 + 6.35, 1e-6);
    }

    /// #105, ADR-0005: a tab is material on the part, so every Profile
    /// that cuts its outline rises over it, here an Outside and an Inside
    /// cut of the rectangle.
    #[test]
    fn two_profiles_on_one_outline_both_rise_over_its_tabs() {
        let mut p = untabbed();
        p.tabs = vec![tab("tab1", 25.0, 0.0)];
        let mut inside = profile_mut(&mut p).clone();
        inside.id = OperationId::new("o2");
        inside.side = Side::Inside;
        p.operations.push(Operation::Profile(inside));
        for op in p.operations.clone() {
            let (tp, _) = generate_of(&p, &op);
            assert_eq!(held_runs(&tp, TAB_TOP_MM).len(), 1, "{}", op.id());
            assert_eq!(
                crossed_by(
                    &p,
                    match &op {
                        Operation::Profile(profile) => profile,
                        _ => unreachable!(),
                    }
                )
                .unwrap(),
                vec![TabId::new("tab1")]
            );
        }
    }

    /// #105: each tab rises to its own top. With passes at Z 4, 2 and 0, a
    /// 3 mm tab on the top edge is crossed by the passes at 2 and 0, and a
    /// 1 mm tab on the bottom edge only by the last.
    #[test]
    fn tabs_of_different_heights_rise_each_to_its_own_top() {
        let mut p = untabbed();
        p.tabs = vec![
            tab("low", 25.0, 0.0),
            PlacedTab {
                height_mm: 3.0,
                ..sample_tab("high", 25.0, 30.0)
            },
        ];
        let (tp, warnings) = generate(&p);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(held_runs(&tp, 3.0).len(), 2);
        assert_eq!(held_runs(&tp, 1.0).len(), 1);
        for run in held_runs(&tp, 1.0) {
            assert!(run.iter().all(|(from, _)| from.y < 10.0), "{run:?}");
        }
    }

    /// The sample with its rectangle replaced by `profiles`, all cut by
    /// the one Profile.
    fn with_profiles(profiles: Vec<Polyline>) -> Project {
        let mut p = untabbed();
        p.definitions.insert(
            DefinitionId::new("d1"),
            FeatureDefinition {
                id: DefinitionId::new("d1"),
                name: "Frame".into(),
                profiles,
            },
        );
        p
    }

    fn four_tabs(on_holes: bool) -> TabSpec {
        TabSpec {
            placement: TabPlacement::Count { count: 4 },
            width_mm: 6.0,
            height_mm: TAB_TOP_MM,
            shape: TabShape::Rect,
            on_holes,
        }
    }

    /// #105 with #109's rules: `place` puts no tab on a hole unless
    /// `on_holes`, and on an outline too short for all its tabs, the most
    /// that cover at most half of it, with `tabs_cover_most_of_contour`.
    #[test]
    fn place_keeps_the_rules_for_holes_and_short_outlines() {
        let square = |side_mm: f64| {
            Polyline::rect(
                Point::new(25.0 - side_mm / 2.0, 15.0 - side_mm / 2.0),
                side_mm,
                side_mm,
            )
        };
        let p = with_profiles(vec![square(28.0), square(12.0)]);
        let op = OperationId::new("o1");
        let off = place(&p, &op, &four_tabs(false)).unwrap();
        assert_eq!(off.tabs.len(), 4, "{off:?}");
        assert!(off.tabs.iter().all(|t| t.profile == 0), "{off:?}");
        assert!(off.warnings.is_empty(), "{:?}", off.warnings);
        let on = place(&p, &op, &four_tabs(true)).unwrap();
        let on_hole = on.tabs.iter().filter(|t| t.profile == 1).count();
        // The hole's 22.6 mm path takes one 12.35 mm span at most... none
        // in half of it: a hole may lose them all.
        assert_eq!(on_hole, 0, "{on:?}");
        assert!(
            on.warnings.iter().any(|w| matches!(
                w,
                Warning::TabsCoverMostOfContour {
                    tab_count: 4,
                    cut_count: 0,
                    ..
                }
            )),
            "{:?}",
            on.warnings
        );
        // A 10 mm part keeps the two that fit in half its 59.95 mm path.
        let small = with_profiles(vec![square(10.0)]);
        let placed = place(&small, &op, &four_tabs(false)).unwrap();
        assert_eq!(placed.tabs.len(), 2, "{placed:?}");
        assert_eq!(
            codes(&placed.warnings),
            vec![WarningCode::TabsCoverMostOfContour]
        );
    }

    /// #105: a tab on a hole of the part is cut, since it was placed, and
    /// warns `tab_on_hole`.
    #[test]
    fn tab_on_a_hole_is_cut_and_warns() {
        let square = |side_mm: f64| {
            Polyline::rect(
                Point::new(25.0 - side_mm / 2.0, 15.0 - side_mm / 2.0),
                side_mm,
                side_mm,
            )
        };
        let mut p = with_profiles(vec![square(28.0), square(16.0)]);
        p.tabs = vec![PlacedTab {
            profile: 1,
            width_mm: 2.0,
            ..tab("tab1", 25.0, 7.0)
        }];
        let (tp, warnings) = generate(&p);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::TabOnHole],
            "{warnings:?}"
        );
        assert!(matches!(&warnings[0], Warning::TabOnHole { tab, .. } if tab.as_str() == "tab1"));
        assert_eq!(held_runs(&tp, TAB_TOP_MM).len(), 1);
    }

    /// #105: placing the same tabs again adds none, and new tabs take the
    /// next free ids.
    #[test]
    fn place_does_not_place_a_tab_twice() {
        let mut p = untabbed();
        let op = OperationId::new("o1");
        let first = place(&p, &op, &four_tabs(false)).unwrap();
        let ids: Vec<&str> = first.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["tab1", "tab2", "tab3", "tab4"]);
        p.tabs = first.tabs;
        assert!(place(&p, &op, &four_tabs(false)).unwrap().tabs.is_empty());
        let more = TabSpec {
            placement: TabPlacement::Count { count: 2 },
            ..four_tabs(false)
        };
        let second = place(&p, &op, &more).unwrap();
        let ids: Vec<&str> = second.tabs.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["tab5", "tab6"]);
    }

    /// #105: tabs are placed for a Profile; anything else is refused.
    #[test]
    fn place_needs_a_profile_of_the_project() {
        let mut p = untabbed();
        p.operations.push(Operation::Pocket(sample_pocket()));
        let err = place(&p, &OperationId::new("o9"), &four_tabs(false)).unwrap_err();
        assert!(matches!(err, CamError::UnknownOperation { .. }), "{err:?}");
        let err = place(&p, &OperationId::new("o2"), &four_tabs(false)).unwrap_err();
        assert!(
            matches!(err, CamError::TabsNeedProfile { kind: "pocket", .. }),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            "operation `o2` is a pocket operation; only a Profile cuts tabs"
        );
    }

    /// #105: `--at` places a tab at each distance along the outline from
    /// its first vertex as drawn: the rectangle's bottom edge runs 0 to
    /// 50 mm, its right edge 50 to 80 mm.
    #[test]
    fn place_on_outlines_puts_a_tab_at_each_distance() {
        let p = untabbed();
        let size = TabSize {
            width_mm: 6.0,
            height_mm: TAB_TOP_MM,
            shape: TabShape::Smooth,
        };
        let placed = place_on_outlines(
            &p,
            &OperationId::new("o1"),
            &[25.0, 65.0, 185.0],
            size,
            false,
        )
        .unwrap();
        // 185 mm wraps round the 160 mm outline to 25 mm, a tab already
        // placed, so it is not placed twice.
        let points: Vec<(f64, f64)> = placed.tabs.iter().map(|t| (t.x_mm, t.y_mm)).collect();
        assert_eq!(points.len(), 2);
        for ((x, y), (ex, ey)) in points.iter().zip([(25.0, 0.0), (50.0, 15.0)]) {
            assert_approx_eq!(*x, ex, 1e-9);
            assert_approx_eq!(*y, ey, 1e-9);
        }
        assert!(placed.tabs.iter().all(|t| t.shape == TabShape::Smooth));
        let err = place_on_outlines(&p, &OperationId::new("o1"), &[-1.0], size, false).unwrap_err();
        assert!(matches!(err, CamError::InvalidTab { .. }), "{err:?}");
    }
}
