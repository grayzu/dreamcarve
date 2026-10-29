// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/drilling.ts (countersinkTipDepth)
//   src/engine/toolpaths/drillingCountersink.test.ts

//! A countersink's derived depth (format 21; `docs/specs/phase-3/
//! design.md`, "Algorithms → Countersink"). Validation, the generator and
//! the safety pass all need it, and validation may not depend on
//! `dc-cam`, so it lives here.

/// How far, mm, a countersink's mouth may pass its bit's diameter, and
/// its flank engagement the bit's cone height, and still be cut: upstream's
/// `1e-9`. Not rounding slack but floating point's: a 90° 12 mm bit's
/// full 12 mm mouth derives a depth one ulp above its 6 mm cone, the most
/// ordinary countersink there is. Validation and the generator compare
/// with the same value, so they agree.
pub const COUNTERSINK_TOLERANCE_MM: f64 = 1e-9;

/// How far below its start plane a V-bit of included angle
/// `included_angle_deg` must plunge to open a mouth `mouth_mm` wide: a
/// cone opens to radius `d · tan(α/2)` at depth `d`, so `d = mouth /
/// (2 · tan(α/2))` (upstream `countersinkTipDepth`). The tip is an ideal
/// point, as everywhere in the tool model.
///
/// `None` when the angle is not above 0° and below 180° (at 180° `tan`
/// is huge but finite, which would give a plausible sub-nanometre depth
/// for a flat cutter), or when the mouth or the depth is not a positive
/// finite number.
#[must_use]
pub fn countersink_depth_mm(mouth_mm: f64, included_angle_deg: f64) -> Option<f64> {
    if !(included_angle_deg > 0.0 && included_angle_deg < 180.0) {
        return None;
    }
    let slope = (included_angle_deg.to_radians() / 2.0).tan();
    if !(slope.is_finite() && slope > 1e-9) {
        return None;
    }
    let depth_mm = mouth_mm / (2.0 * slope);
    (depth_mm.is_finite() && depth_mm > 0.0).then_some(depth_mm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::assert_approx_eq;

    /// Port of `drillingCountersink.test.ts` "countersinkTipDepth rejects
    /// angles that cannot form a cone", with the 90° and 82° depths of
    /// acceptance 55.
    #[test]
    fn countersink_depth_rejects_angles_that_cannot_form_a_cone() {
        assert_eq!(countersink_depth_mm(6.0, 0.0), None, "0° has no cone");
        assert_eq!(countersink_depth_mm(6.0, 180.0), None, "180° is flat");
        assert_eq!(countersink_depth_mm(6.0, f64::NAN), None);
        assert_eq!(countersink_depth_mm(0.0, 90.0), None, "no mouth");
        assert_eq!(countersink_depth_mm(-6.0, 90.0), None);
        assert_eq!(countersink_depth_mm(f64::INFINITY, 90.0), None);
        let at = |mouth: f64, angle: f64| countersink_depth_mm(mouth, angle).unwrap_or(f64::NAN);
        assert_approx_eq!(
            at(10.0, 60.0),
            10.0 / (2.0 * 30f64.to_radians().tan()),
            1e-12
        );
        assert_approx_eq!(at(6.0, 90.0), 3.0, 1e-12);
        assert_approx_eq!(at(6.0, 82.0), 3.451, 5e-4);
    }
}
