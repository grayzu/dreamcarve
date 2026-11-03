// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Lettering designs (#464), deterministic from a seed: two to five
//! letters of the repo's bundled Droid Serif Bold glyphs ("DreamCarve"
//! of `tests/fixtures/sign.dcv`, 14 mm caps, and the "DC" of
//! `tests/fixtures/dc.dcv`, 40 mm caps; the font notice is
//! `tests/fixtures/README.md`'s), 3 to 25 mm tall, each turned up to 15°
//! and lifted or dropped a little, spaced from 40 % of the cap height
//! apart to 40 % overlapped, sometimes in a second row over the first.
//! Letters that overlap are merged by union, as a user's welded text is.

use super::designs::{Design, bounds, outlines, transformed, valid};
use super::rng::Rng;
use dc_geometry::{
    Contour, FLATTEN_TOLERANCE_DEFAULT_MM, Polyline, Region, Transform, nest, union,
};
use std::sync::OnceLock;

/// A glyph: its outlines in its own frame, the box's lower left at the
/// origin, scaled to a 1 mm cap height.
struct Glyph {
    name: String,
    profiles: Vec<Polyline>,
}

/// The bundled glyphs.
fn glyphs() -> &'static [Glyph] {
    static GLYPHS: OnceLock<Vec<Glyph>> = OnceLock::new();
    GLYPHS.get_or_init(|| {
        let mut out = Vec::new();
        for (text, cap_mm) in [
            (include_str!("../../../../tests/fixtures/sign.dcv"), 14.0),
            (include_str!("../../../../tests/fixtures/dc.dcv"), 40.0),
        ] {
            let project = dc_project::load(text.as_bytes())
                .expect("the fixture loads")
                .project;
            for def in project.definitions.values() {
                if !def.name.starts_with("letter-") {
                    continue;
                }
                let b = bounds(&def.profiles);
                let to_unit = Transform::translate(-b.min.x, -b.min.y)
                    .then(Transform::scale(1.0 / cap_mm, 1.0 / cap_mm));
                out.push(Glyph {
                    name: def.name.trim_start_matches("letter-").to_owned(),
                    profiles: def
                        .profiles
                        .iter()
                        .map(|p| transformed(p, to_unit))
                        .collect(),
                });
            }
        }
        out
    })
}

/// The lettering design of `seed`, redrawn from the next draw in the rare
/// case the union leaves an outline touching itself ([`valid`]).
pub fn lettering(seed: u64) -> Design {
    lettering_of(seed, false)
}

/// [`lettering`] of two or three letters in one row, for the CI tests'
/// budget.
pub fn lettering_small(seed: u64) -> Design {
    lettering_of(seed, true)
}

/// [`lettering`], `small` or not.
fn lettering_of(seed: u64, small: bool) -> Design {
    let mut rng = Rng::new(seed);
    for _ in 0..32 {
        let mut fork = rng.fork();
        let design = word(&mut fork, small);
        if !design.profiles.is_empty() && valid(&design.profiles) {
            return design;
        }
    }
    panic!("seed {seed}: no valid lettering in 32 draws");
}

/// One or two rows of letters, merged; one row of two or three when
/// `small`.
fn word(rng: &mut Rng, small: bool) -> Design {
    let all = glyphs();
    let height_mm = rng.log_range(3.0, if small { 10.0 } else { 25.0 });
    let mut placed: Vec<Region> = Vec::new();
    let mut note = format!("lettering {height_mm:.2} mm:");
    let rows = if !small && rng.chance(0.3) { 2 } else { 1 };
    for row in 0..rows {
        #[allow(clippy::cast_precision_loss, reason = "a row index")]
        let base_mm = row as f64 * height_mm * rng.range(0.5, 1.1);
        let mut x_mm = rng.range(0.0, 0.5) * height_mm;
        if row > 0 {
            note += " / next row";
        }
        for _ in 0..rng.int(2, if small { 3 } else { 5 }) {
            let glyph = rng.pick(all);
            let turn = rng.range(-15.0, 15.0).to_radians();
            let lift = rng.range(-0.15, 0.15) * height_mm;
            let unit = bounds(&glyph.profiles);
            let (w, h) = (unit.max.x * height_mm, unit.max.y * height_mm);
            let t = Transform::scale(height_mm, height_mm)
                .then(Transform::translate(-w / 2.0, -h / 2.0))
                .then(Transform::rotate(turn))
                .then(Transform::translate(
                    x_mm + w / 2.0,
                    base_mm + lift + h / 2.0,
                ));
            let contours: Vec<Contour> = glyph
                .profiles
                .iter()
                .map(|p| Contour::new(transformed(p, t)).expect("a glyph outline is a contour"))
                .collect();
            let (regions, _) = nest(contours).expect("a glyph nests");
            placed = union(&placed, &regions, FLATTEN_TOLERANCE_DEFAULT_MM);
            let spacing = rng.range(-0.4, 0.4) * height_mm;
            note += &format!(
                " {} (turned {:.1}°, lifted {lift:.2}, then {spacing:.2} mm on);",
                glyph.name,
                turn.to_degrees()
            );
            x_mm += w + spacing;
        }
    }
    Design {
        profiles: outlines(&placed),
        notes: vec![note],
    }
}
