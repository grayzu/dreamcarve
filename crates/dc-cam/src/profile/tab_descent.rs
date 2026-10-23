// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #348: how a Profile's last pass comes down off a tab taller than the
//! step-down (`docs/specs/phase-2/design.md`, A8 step 4). The passes
//! before it run on the allowance path, `a` farther from the wall, so
//! wherever the last pass comes down, the allowance strip under its
//! cutter stands to the height the lap last cut it at: a straight drop
//! there goes its whole height into material no earlier pass cut. Two
//! places do that: a rect tab's far end, and a lap that starts low on a
//! smooth tab's slope where the allowance path is held higher.
//!
//! - **A rect tab's far end** ([`stage_tab_descents`]): the drop is cut in
//!   stages, each at most a step-down, as #339 holds every other plunge
//!   into uncut material: straight down to the floor of the pass before
//!   (the earlier passes' Z levels below the tab top), then down the rest
//!   one pass level at a time. Between stages the tool runs level along
//!   the lap's own path, forwards off the tab and back, over the stretch
//!   the lap is about to cut lower, so every stage plunges one step below
//!   a layer just cut and nothing touches the tab.
//! - **A lap that starts on a smooth tab's slope** ([`onto_slope`]): the
//!   tool enters the lap where the lap is as high as the step across the
//!   allowance ran, and follows the lap's own slope back down to its
//!   start. An open path's lap must start at its end, so moving the start
//!   off the tab (#176) is no option there.

use crate::block::CutBlock;
use crate::entry::plunge_limited_feed_mm_min;
use crate::tabs::LapRates;
use crate::toolpath::{Move, MoveKind};
use dc_geometry::{Arc, EPS_MM, Point, Point3};

/// One cutting move of a lap and where it starts.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Leg {
    from: Point3,
    mv: Move,
}

impl Leg {
    /// The arc this leg runs, if it is one.
    fn arc(&self) -> Option<Arc> {
        match self.mv.kind {
            MoveKind::ArcCw { centre } => Some(Arc::from_ends(
                self.from.xy(),
                self.mv.to.xy(),
                centre,
                false,
            )),
            MoveKind::ArcCcw { centre } => Some(Arc::from_ends(
                self.from.xy(),
                self.mv.to.xy(),
                centre,
                true,
            )),
            _ => None,
        }
    }

    /// Its length in XY, mm.
    fn length_xy_mm(&self) -> f64 {
        self.arc().map_or_else(
            || self.from.xy().distance_to(self.mv.to.xy()),
            |a| a.radius_mm * a.sweep_rad.abs(),
        )
    }

    /// A straight cut or an arc that moves in XY.
    fn moves_xy(&self) -> bool {
        matches!(
            self.mv.kind,
            MoveKind::Linear | MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. }
        ) && self.length_xy_mm() > EPS_MM
    }

    /// Straight down more than `depth_mm`.
    fn drops_more_than(&self, depth_mm: f64) -> bool {
        matches!(self.mv.kind, MoveKind::Linear)
            && self.from.xy().distance_to(self.mv.to.xy()) <= EPS_MM
            && self.from.z - self.mv.to.z > depth_mm + EPS_MM
    }

    /// Level at `z_mm` all along.
    fn level_at(&self, z_mm: f64) -> bool {
        self.moves_xy()
            && (self.from.z - z_mm).abs() <= EPS_MM
            && (self.mv.to.z - z_mm).abs() <= EPS_MM
    }

    /// The same leg run the other way, at the same feed.
    fn reversed(&self) -> Self {
        let kind = match self.mv.kind {
            MoveKind::ArcCw { centre } => MoveKind::ArcCcw { centre },
            MoveKind::ArcCcw { centre } => MoveKind::ArcCw { centre },
            kind => kind,
        };
        Self {
            from: self.mv.to,
            mv: Move {
                kind,
                to: self.from,
                feed_mm_min: self.mv.feed_mm_min,
            },
        }
    }

    /// Its first `length_mm` in XY, Z interpolated along it; an arc keeps
    /// its centre.
    fn first(&self, length_mm: f64) -> Self {
        let total_mm = self.length_xy_mm();
        if length_mm >= total_mm - EPS_MM || total_mm <= EPS_MM {
            return *self;
        }
        let f = length_mm / total_mm;
        let xy = match self.arc() {
            Some(a) => {
                let angle = a.start_angle_rad + a.sweep_rad.signum() * length_mm / a.radius_mm;
                Point::new(
                    a.centre.x + a.radius_mm * angle.cos(),
                    a.centre.y + a.radius_mm * angle.sin(),
                )
            }
            None => self.from.xy() + (self.mv.to.xy() - self.from.xy()) * f,
        };
        let z_mm = self.from.z + (self.mv.to.z - self.from.z) * f;
        Self {
            from: self.from,
            mv: Move {
                to: Point3::new(xy.x, xy.y, z_mm),
                ..self.mv
            },
        }
    }

    /// Its move with the end at `z_mm`: the leg run level there.
    fn at_z(&self, z_mm: f64) -> Move {
        Move {
            to: Point3::new(self.mv.to.x, self.mv.to.y, z_mm),
            ..self.mv
        }
    }
}

/// The legs of `cuts`, cut from `start`.
fn legs(start: Point3, cuts: &[Move]) -> Vec<Leg> {
    let mut at = start;
    cuts.iter()
        .map(|mv| {
            let leg = Leg { from: at, mv: *mv };
            at = mv.to;
            leg
        })
        .collect()
}

/// The first `length_mm` of the level run at `z_mm` that `legs` start
/// with (less if the run is shorter): where a stage of a descent runs.
fn level_run(legs: &[Leg], z_mm: f64, length_mm: f64) -> Vec<Leg> {
    let mut run = Vec::new();
    let mut left_mm = length_mm;
    for leg in legs.iter().take_while(|l| l.level_at(z_mm)) {
        if left_mm <= EPS_MM {
            break;
        }
        let part = leg.first(left_mm);
        left_mm -= part.length_xy_mm();
        run.push(part);
    }
    run
}

/// The Z levels, top down, a descent from `top_z_mm` to `floor_z_mm`
/// stops at: each pass level of `pass_z` between them, the floors the
/// earlier passes cut, then `floor_z_mm`; a gap wider than `step_mm`
/// (none, as the passes are laid) split evenly.
fn stage_levels(pass_z: &[f64], top_z_mm: f64, floor_z_mm: f64, step_mm: f64) -> Vec<f64> {
    let mut stops: Vec<f64> = pass_z
        .iter()
        .copied()
        .filter(|z| *z < top_z_mm - EPS_MM && *z > floor_z_mm + EPS_MM)
        .collect();
    stops.sort_by(|a, b| b.total_cmp(a));
    stops.push(floor_z_mm);
    let mut levels = Vec::with_capacity(stops.len());
    let mut above = top_z_mm;
    for stop in stops {
        let gap_mm = above - stop;
        let parts = (gap_mm / step_mm - EPS_MM).ceil().max(1.0);
        let mut k = 1.0;
        while k < parts {
            levels.push(above - gap_mm * k / parts);
            k += 1.0;
        }
        levels.push(stop);
        above = stop;
    }
    levels
}

/// #348: every straight drop of `block`'s cuts (the last pass's lap)
/// deeper than `step_mm`, a rect tab's far end, cut in stages instead
/// ([`stage_levels`]): at each level the tool goes straight down, then,
/// but after the last, runs level along the lap's next `run_mm` (the
/// stretch it cuts at the drop's foot after the tab) to its end, or back.
/// After the last stage it is back where the drop was, at its foot, and
/// the lap goes on from there as before.
///
/// Every stage plunges at most `step_mm` below a layer the stage before
/// cut, or the floor of the pass before, and every level move runs on the
/// lap's own path where the lap cuts lower, so it cuts no tab.
pub(super) fn stage_tab_descents(
    block: &mut CutBlock,
    pass_z: &[f64],
    step_mm: f64,
    run_mm: f64,
    plunge_mm_min: f64,
) {
    let start_z_mm = block.descent.last().map_or(block.z_mm, |m| m.to.z);
    let start = Point3::new(block.entry.x, block.entry.y, start_z_mm);
    let legs = legs(start, &block.cuts);
    if !legs.iter().any(|l| l.drops_more_than(step_mm)) {
        return;
    }
    let mut cuts = Vec::with_capacity(block.cuts.len() + 8);
    for (k, leg) in legs.iter().enumerate() {
        if !leg.drops_more_than(step_mm) {
            cuts.push(leg.mv);
            continue;
        }
        let floor_z_mm = leg.mv.to.z;
        let ahead = legs
            .get(k + 1..)
            .map_or_else(Vec::new, |rest| level_run(rest, floor_z_mm, run_mm));
        // Never empty: the lap cuts on at the drop's foot after the tab.
        let Some(end) = ahead.last().map(|l| l.mv.to) else {
            cuts.push(leg.mv);
            continue;
        };
        let back: Vec<Leg> = ahead.iter().rev().map(Leg::reversed).collect();
        let levels = stage_levels(pass_z, leg.from.z, floor_z_mm, step_mm);
        let mut at_drop = true;
        for (j, &z_mm) in levels.iter().enumerate() {
            let xy = if at_drop { leg.from.xy() } else { end.xy() };
            cuts.push(Move::linear(Point3::new(xy.x, xy.y, z_mm), plunge_mm_min));
            let last = j + 1 == levels.len();
            if !last || !at_drop {
                let run = if at_drop { &ahead } else { &back };
                cuts.extend(run.iter().map(|l| l.at_z(z_mm)));
                at_drop = !at_drop;
            }
        }
    }
    block.cuts = cuts;
}

/// #348: the moves that take the tool from `target`, the start of the
/// lap `cuts`, at `step_z_mm`, where the step across the allowance ran,
/// down to `start_z_mm`, where the lap starts on a smooth tab's slope,
/// without a straight drop into the allowance strip: level along the lap
/// at `step_z_mm` to where its slope rises to that height, then down the
/// lap's own slope back to `target`. The lap's slope rises forwards from
/// its start, or (a closed lap, `closed`, that starts on a tab's far
/// slope) backwards along its end. Above the slope and on it the tool
/// cuts no lower than the lap does, so into no tab; the slope down is cut
/// at the feed scaled to the plunge rate, as the lap cuts its own. `None`
/// when neither way rises to `step_z_mm` along the slope alone.
pub(super) fn onto_slope(
    cuts: &[Move],
    target: Point,
    (start_z_mm, step_z_mm): (f64, f64),
    closed: bool,
    rates: LapRates,
) -> Option<Vec<Move>> {
    let start = Point3::new(target.x, target.y, start_z_mm);
    let legs = legs(start, cuts);
    let forwards = rising_to(legs.iter().copied(), step_z_mm);
    let chain = forwards.or_else(|| {
        closed
            .then(|| rising_to(legs.iter().rev().map(Leg::reversed), step_z_mm))
            .flatten()
    })?;
    let mut moves: Vec<Move> = chain
        .iter()
        .map(|l| Move {
            feed_mm_min: Some(match l.mv.kind {
                MoveKind::Linear => rates.feed_mm_min,
                _ => l.mv.feed_mm_min.unwrap_or(rates.feed_mm_min),
            }),
            ..l.at_z(step_z_mm)
        })
        .collect();
    moves.extend(chain.iter().rev().map(|l| {
        let back = l.reversed();
        Move {
            feed_mm_min: Some(match back.mv.kind {
                MoveKind::Linear => plunge_limited_feed_mm_min(
                    rates.feed_mm_min,
                    rates.plunge_mm_min,
                    back.from,
                    back.mv.to,
                ),
                _ => back.mv.feed_mm_min.unwrap_or(rates.feed_mm_min),
            }),
            ..back.mv
        }
    }));
    Some(moves)
}

/// The legs of `legs` from the first to the point where they rise to
/// `z_mm`, the last cut short there; `None` if they fall, go straight up
/// or down, or end first.
fn rising_to(legs: impl Iterator<Item = Leg>, z_mm: f64) -> Option<Vec<Leg>> {
    let mut chain = Vec::new();
    for leg in legs {
        if !leg.moves_xy() || leg.mv.to.z < leg.from.z - EPS_MM {
            return None;
        }
        if leg.mv.to.z >= z_mm - EPS_MM {
            let rise_mm = leg.mv.to.z - leg.from.z;
            let part = if rise_mm > EPS_MM {
                leg.first(leg.length_xy_mm() * ((z_mm - leg.from.z) / rise_mm).clamp(0.0, 1.0))
            } else {
                leg
            };
            chain.push(part);
            return Some(chain);
        }
        chain.push(leg);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(x: f64, z: f64) -> Move {
        Move::linear(Point3::new(x, 0.0, z), 1000.0)
    }

    #[test]
    fn stage_levels_stop_at_each_pass_floor() {
        // Passes at 8, 4, 0 under a 6 mm tab top: down to 4, then 0.
        assert_eq!(
            stage_levels(&[8.0, 4.0, 0.0], 6.0, 0.0, 4.0),
            vec![4.0, 0.0]
        );
        // A gap wider than the step is split evenly.
        assert_eq!(stage_levels(&[0.0], 6.0, 0.0, 4.0), vec![3.0, 0.0]);
    }

    #[test]
    fn a_tall_rect_drop_is_cut_in_stages_along_the_lap() {
        // A lap at Z 0 that comes off a 6 mm rect tab at X 10.
        let mut block = CutBlock {
            shape: crate::error::ShapeRef {
                instance: dc_project::InstanceId::new("i"),
                instance_name: "i".to_owned(),
                profile: 0,
            },
            pass: 2,
            z_mm: 0.0,
            descent_from: Point::new(0.0, 0.0),
            descent_from_z_mm: 4.0,
            descent: vec![Move::linear(Point3::new(0.0, 0.0, 6.0), 300.0)],
            entry: Point::new(0.0, 0.0),
            cuts: vec![
                level(10.0, 6.0),
                Move::linear(Point3::new(10.0, 0.0, 0.0), 300.0),
                level(40.0, 0.0),
            ],
            exit: Point::new(40.0, 0.0),
            frees: false,
            cleared: None,
            group: 0,
        };
        stage_tab_descents(&mut block, &[8.0, 4.0, 0.0], 4.0, 3.0, 300.0);
        let ends: Vec<(f64, f64)> = block.cuts.iter().map(|m| (m.to.x, m.to.z)).collect();
        assert_eq!(
            ends,
            vec![
                (10.0, 6.0),
                (10.0, 4.0),
                (13.0, 4.0),
                (13.0, 0.0),
                (10.0, 0.0),
                (40.0, 0.0)
            ]
        );
    }

    #[test]
    fn a_lap_starting_on_a_slope_is_entered_where_it_is_as_high() {
        // A lap from X 0 at Z 0 rising to 6 at X 4, then level.
        let cuts = [
            Move::linear(Point3::new(4.0, 0.0, 6.0), 1000.0),
            level(20.0, 6.0),
        ];
        let rates = LapRates {
            feed_mm_min: 1000.0,
            plunge_mm_min: 300.0,
        };
        let moves = onto_slope(&cuts, Point::new(0.0, 0.0), (0.0, 3.0), false, rates)
            .expect("the slope rises to 3 mm");
        let ends: Vec<(f64, f64)> = moves.iter().map(|m| (m.to.x, m.to.z)).collect();
        assert_eq!(ends, vec![(2.0, 3.0), (0.0, 0.0)]);
        // Down the slope no faster than the plunge rate.
        let down = moves[1];
        let length = 2.0_f64.hypot(3.0);
        assert!(down.feed_mm_min.unwrap() * 3.0 / length <= 300.0 + 1e-9);
    }
}
