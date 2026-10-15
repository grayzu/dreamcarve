// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Depth passes (`docs/specs/phase-1/design.md`, "Algorithms → Profile
//! contour", step 5): the Z levels an operation cuts at, top to bottom.

use crate::error::CamError;
use dc_geometry::EPS_MM;
use dc_project::OperationId;

/// Most depth passes one operation may have. Bounds the work a mistyped
/// step-down (0.0001 mm for 0.1 mm) can cause; a real job needs far fewer.
pub const MAX_PASSES: u32 = 10_000;

/// Absolute Z of each depth pass, mm, shallowest first, for a cut
/// `depth_mm` below the top of stock `thickness_mm` thick (stock bottom at
/// Z = 0) with steps of at most `step_down_mm`.
///
/// Pass k cuts to depth `d_k = min(k · step, depth)`; the last pass is the
/// first whose depth reaches `depth_mm` within [`dc_geometry::EPS_MM`], so
/// only the final pass can be shallower than a full step. Pass Z is
/// `thickness − d_k`.
///
/// Fails with [`CamError::BelowMaterialBottom`] when `depth_mm` exceeds
/// `thickness_mm + through_cut_allowance_mm` by more than
/// [`dc_geometry::EPS_MM`], and with [`CamError::InvalidDepth`] or
/// [`CamError::InvalidStepDown`] when either is not a positive finite
/// number or the depth would need more than [`MAX_PASSES`] passes. `op` names the operation in the error.
pub fn pass_z_mm(
    op: &OperationId,
    thickness_mm: f64,
    depth_mm: f64,
    step_down_mm: f64,
    through_cut_allowance_mm: f64,
) -> Result<Vec<f64>, CamError> {
    if !(depth_mm.is_finite() && depth_mm > 0.0) {
        return Err(CamError::InvalidDepth {
            op: op.clone(),
            depth_mm,
        });
    }
    let invalid_step = || CamError::InvalidStepDown {
        op: op.clone(),
        step_down_mm,
    };
    let max_passes = f64::from(MAX_PASSES);
    if !(step_down_mm.is_finite() && step_down_mm > 0.0)
        || (depth_mm - EPS_MM) / step_down_mm >= max_passes
    {
        return Err(invalid_step());
    }
    let limit_mm = thickness_mm + through_cut_allowance_mm;
    if depth_mm > limit_mm + EPS_MM {
        return Err(CamError::BelowMaterialBottom {
            op: op.clone(),
            depth_mm,
            limit_mm,
        });
    }
    let mut z_mm = Vec::new();
    // k · step rather than a running sum, so rounding does not accumulate.
    for k in 1..=MAX_PASSES {
        let d_mm = (f64::from(k) * step_down_mm).min(depth_mm);
        if depth_mm - d_mm <= EPS_MM {
            z_mm.push(thickness_mm - depth_mm);
            return Ok(z_mm);
        }
        z_mm.push(thickness_mm - d_mm);
    }
    // Unreachable: the pass count was checked against MAX_PASSES above.
    Err(invalid_step())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::assert_approx_eq;

    const TOL: f64 = 1e-9;

    fn op() -> OperationId {
        OperationId::new("o1")
    }

    fn assert_z(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert_approx_eq!(*a, *e, TOL);
        }
    }

    #[test]
    fn even_division() {
        let z = pass_z_mm(&op(), 12.0, 12.0, 4.0, 0.0).unwrap();
        assert_z(&z, &[8.0, 4.0, 0.0]);
    }

    #[test]
    fn uneven_final_pass_is_shallower() {
        let z = pass_z_mm(&op(), 12.0, 12.0, 5.0, 0.0).unwrap();
        assert_z(&z, &[7.0, 2.0, 0.0]);
    }

    #[test]
    fn depth_beyond_bottom_is_error() {
        let err = pass_z_mm(&op(), 12.0, 12.1, 4.0, 0.0).unwrap_err();
        assert!(
            matches!(err, CamError::BelowMaterialBottom { ref op, .. } if op.as_str() == "o1"),
            "{err:?}"
        );
    }

    #[test]
    fn through_cut_allowance_permits_exactly_that_much() {
        let z = pass_z_mm(&op(), 12.0, 12.2, 4.0, 0.2).unwrap();
        assert_z(&z, &[8.0, 4.0, 0.0, -0.2]);
        let err = pass_z_mm(&op(), 12.0, 12.3, 4.0, 0.2).unwrap_err();
        assert!(
            matches!(err, CamError::BelowMaterialBottom { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn non_positive_step_down_is_error() {
        for step in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let err = pass_z_mm(&op(), 12.0, 6.0, step, 0.0).unwrap_err();
            assert!(matches!(err, CamError::InvalidStepDown { .. }), "{err:?}");
        }
    }

    #[test]
    fn step_down_needing_too_many_passes_is_error() {
        let err = pass_z_mm(&op(), 12.0, 12.0, 1e-6, 0.0).unwrap_err();
        assert!(matches!(err, CamError::InvalidStepDown { .. }), "{err:?}");
    }

    #[test]
    fn non_positive_depth_is_error() {
        for depth in [0.0, -1.0, f64::NAN] {
            let err = pass_z_mm(&op(), 12.0, depth, 4.0, 0.0).unwrap_err();
            assert!(matches!(err, CamError::InvalidDepth { .. }), "{err:?}");
        }
    }

    #[test]
    fn step_larger_than_depth_is_one_pass() {
        let z = pass_z_mm(&op(), 12.0, 3.0, 4.0, 0.0).unwrap();
        assert_z(&z, &[9.0]);
    }

    #[test]
    fn rounding_does_not_add_a_hair_pass() {
        // 0.1 · 3 is 0.30000000000000004 in f64; no fourth pass may follow.
        let z = pass_z_mm(&op(), 12.0, 0.3, 0.1, 0.0).unwrap();
        assert_z(&z, &[11.9, 11.8, 11.7]);
    }
}
