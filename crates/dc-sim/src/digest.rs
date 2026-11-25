// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A run's key (`docs/specs/phase-6/design.md`, "Runs in order, and what
//! is reused"): a number that changes when anything the simulation reads
//! of the run does, so a shell can tell which runs to cut again.
//!
//! It is a 64-bit FNV-1a hash, as the app's operation digests are: fixed
//! by its definition, the same on every build. Two runs with one key
//! are taken to cut alike; an unlisted input would be a bug, and the
//! test that changes each field in turn is the check.

use dc_geometry::Point3;

use crate::build::SimRun;
use crate::cut::{Motion, SimMove, Turn};
use crate::cutter::Cutter;

/// A running FNV-1a hash of what is fed to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Digest(u64);

impl Default for Digest {
    fn default() -> Self {
        Self::new()
    }
}

impl Digest {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0100_0000_01b3;

    /// A hash of nothing yet.
    #[must_use]
    pub const fn new() -> Self {
        Self(Self::OFFSET)
    }

    /// Feeds bytes.
    pub fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    /// Feeds a whole number.
    pub fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    /// Feeds a number by its bits, so that 0 and −0 differ and every
    /// value has one encoding.
    pub fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }

    /// The hash so far.
    #[must_use]
    pub const fn finish(&self) -> u64 {
        self.0
    }

    fn point(&mut self, p: Point3) {
        self.f64(p.x);
        self.f64(p.y);
        self.f64(p.z);
    }

    fn sim_move(&mut self, mv: &SimMove) {
        self.point(mv.from);
        self.point(mv.to);
        match mv.arc {
            None => self.u64(0),
            Some(arc) => {
                self.u64(match arc.turn {
                    Turn::Cw => 1,
                    Turn::Ccw => 2,
                });
                self.f64(arc.centre.x);
                self.f64(arc.centre.y);
            }
        }
        self.u64(match mv.motion {
            Motion::Cut => 1,
            Motion::Travel => 0,
        });
        self.f64(mv.seconds);
    }

    fn cutter(&mut self, cutter: &Cutter) {
        match *cutter {
            Cutter::Flat { radius_mm } => {
                self.u64(1);
                self.f64(radius_mm);
            }
            Cutter::Ball { radius_mm } => {
                self.u64(2);
                self.f64(radius_mm);
            }
            Cutter::Vee { slope, radius_mm } => {
                self.u64(3);
                self.f64(slope);
                self.f64(radius_mm);
            }
            Cutter::TaperedBall {
                tip_radius_mm,
                slope,
                radius_mm,
            } => {
                self.u64(4);
                self.f64(tip_radius_mm);
                self.f64(slope);
                self.f64(radius_mm);
            }
        }
    }
}

impl SimRun {
    /// The run's key: a hash of its cutter, whether it is shown plain,
    /// and every move.
    #[must_use]
    pub fn key(&self) -> u64 {
        let mut digest = Digest::new();
        digest.cutter(&self.cutter.cutter);
        digest.u64(u64::from(self.cutter.plain));
        digest.u64(self.moves.len() as u64);
        for mv in &self.moves {
            digest.sim_move(mv);
        }
        digest.finish()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "test code reports failure by panicking"
    )]

    use super::*;
    use crate::cutter::CutterShape;
    use dc_geometry::Point;

    fn run() -> SimRun {
        let (a, b) = (Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0));
        SimRun {
            cutter: CutterShape {
                cutter: Cutter::Vee {
                    slope: 1.0,
                    radius_mm: 3.0,
                },
                plain: false,
            },
            moves: vec![
                SimMove::travel(a, a, 0.0),
                SimMove::cut(a, b, 2.0),
                SimMove::cut(b, a, 2.0).along(Point::new(2.5, 3.5), Turn::Ccw),
            ],
        }
    }

    /// The key is fixed by its definition: FNV-1a of nothing is its
    /// offset basis, and of the byte `a` the published value.
    #[test]
    fn the_hash_is_fnv_1a() {
        assert_eq!(Digest::new().finish(), 0xcbf2_9ce4_8422_2325);
        let mut d = Digest::new();
        d.bytes(b"a");
        assert_eq!(d.finish(), 0xaf63_dc4c_8601_ec8c);
    }

    /// Every input the simulation reads of a run changes its key: the
    /// cutter's kind and each of its sizes, the plain mark, each move's
    /// ends, arc, motion and time, and the moves' count and order.
    #[test]
    fn every_input_changes_the_key() {
        let base = run();
        assert_eq!(base.key(), run().key());
        let mut changed: Vec<(&str, SimRun)> = Vec::new();
        let mut edit = |name: &'static str, f: &dyn Fn(&mut SimRun)| {
            let mut r = run();
            f(&mut r);
            changed.push((name, r));
        };
        edit("slope", &|r| {
            r.cutter.cutter = Cutter::Vee {
                slope: 1.1,
                radius_mm: 3.0,
            }
        });
        edit("radius", &|r| {
            r.cutter.cutter = Cutter::Vee {
                slope: 1.0,
                radius_mm: 3.1,
            }
        });
        edit("kind", &|r| {
            r.cutter.cutter = Cutter::Flat { radius_mm: 3.0 }
        });
        edit("ball", &|r| {
            r.cutter.cutter = Cutter::Ball { radius_mm: 3.0 }
        });
        edit("plain", &|r| r.cutter.plain = true);
        edit("from", &|r| r.moves[1].from.x = 1.5);
        edit("to", &|r| r.moves[1].to.z = 6.5);
        edit("seconds", &|r| r.moves[1].seconds = 3.0);
        edit("motion", &|r| r.moves[1].motion = Motion::Travel);
        edit("arc gone", &|r| r.moves[2].arc = None);
        edit("turn", &|r| {
            r.moves[2] = r.moves[2].along(Point::new(2.5, 3.5), Turn::Cw);
        });
        edit("centre", &|r| {
            r.moves[2] = r.moves[2].along(Point::new(2.5, 3.6), Turn::Ccw);
        });
        edit("one fewer", &|r| {
            r.moves.pop();
        });
        edit("order", &|r| r.moves.swap(0, 1));
        let mut keys = vec![base.key()];
        for (name, run) in &changed {
            let key = run.key();
            assert!(!keys.contains(&key), "{name} left the key as another's");
            keys.push(key);
        }
    }
}
