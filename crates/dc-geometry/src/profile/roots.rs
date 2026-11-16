// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The real roots of a polynomial of low degree on an interval, isolated by
//! its derivatives (`docs/specs/phase-3/design.md`, "Algorithms → Tapered
//! ball nose", step 7; the choice is recorded on #678).
//!
//! Between consecutive roots of `p′` the polynomial `p` is monotone, so it
//! has at most one root there, which it has exactly when its sign differs
//! at the two ends; bisection then finds it to the last bit. The roots of
//! `p′` come the same way from `p″`, down to a line, whose root is closed
//! form. Every root of `p′` is returned with the roots of `p`, so a double
//! root of `p` (where it touches 0 without changing sign) is returned as
//! the root of `p′` it also is. Nothing divides by a leading coefficient,
//! so a polynomial whose degree collapses (a coefficient of 0, or one so
//! small that Ferrari's resolvent would lose every digit) is handled the
//! same way, and one that vanishes identically has no roots.

/// Bisection steps at most: enough to narrow any interval of `f64`s to two
/// neighbours, subnormals included, after which it stops early.
const BISECTION_STEPS: usize = 1_100;

/// The product of two polynomials, coefficients constant term first.
pub(super) fn mul(a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; (a.len() + b.len()).saturating_sub(1)];
    for (i, &x) in a.iter().enumerate() {
        for (j, &y) in b.iter().enumerate() {
            if let Some(o) = out.get_mut(i + j) {
                *o = x.mul_add(y, *o);
            }
        }
    }
    out
}

/// `a − b`, coefficients constant term first.
pub(super) fn sub(a: &[f64], b: &[f64]) -> Vec<f64> {
    (0..a.len().max(b.len()))
        .map(|k| a.get(k).copied().unwrap_or(0.0) - b.get(k).copied().unwrap_or(0.0))
        .collect()
}

/// `p(t)` by Horner's rule.
fn value_at(p: &[f64], t: f64) -> f64 {
    p.iter().rev().fold(0.0, |acc, &c| acc.mul_add(t, c))
}

/// `p′`.
fn derivative(p: &[f64]) -> Vec<f64> {
    p.iter()
        .enumerate()
        .skip(1)
        .map(|(k, &c)| c * f64::from(u32::try_from(k).unwrap_or(u32::MAX)))
        .collect()
}

/// Every `t` strictly inside (`lo`, `hi`) where `p` may vanish, ascending:
/// its roots where it changes sign, and every root of its derivatives,
/// which bound the stretches where it is monotone and include its double
/// roots. `p` holds its coefficients, constant term first. A constant
/// polynomial, 0 included, gives none.
pub(super) fn roots_in(p: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let Some(degree) = p.iter().rposition(|&c| c != 0.0) else {
        return Vec::new();
    };
    let p = p.get(..=degree).unwrap_or(p);
    match p {
        [] | [_] => Vec::new(),
        &[c0, c1] => {
            let t = -c0 / c1;
            if lo < t && t < hi {
                vec![t]
            } else {
                Vec::new()
            }
        }
        _ => {
            let turns = roots_in(&derivative(p), lo, hi);
            let mut knots = Vec::with_capacity(turns.len() + 2);
            knots.push(lo);
            knots.extend_from_slice(&turns);
            knots.push(hi);
            let mut out = turns;
            for pair in knots.windows(2) {
                let &[a, b] = pair else { continue };
                let (pa, pb) = (value_at(p, a), value_at(p, b));
                if (pa < 0.0 && pb > 0.0) || (pa > 0.0 && pb < 0.0) {
                    out.push(bisect(p, a, b, pa));
                }
            }
            out.sort_by(f64::total_cmp);
            out.dedup();
            out
        }
    }
}

/// The root of `p` in (`a`, `b`), where `p` is monotone and changes sign
/// (`pa = p(a)`), to the last bit.
fn bisect(p: &[f64], mut a: f64, mut b: f64, pa: f64) -> f64 {
    let negative_at_a = pa < 0.0;
    for _ in 0..BISECTION_STEPS {
        let m = a + (b - a) * 0.5;
        if m <= a || m >= b {
            break;
        }
        let pm = value_at(p, m);
        if pm == 0.0 {
            return m;
        }
        if (pm < 0.0) == negative_at_a {
            a = m;
        } else {
            b = m;
        }
    }
    a + (b - a) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;

    /// `∏ (t − r)` over `roots`.
    fn with_roots(roots: &[f64]) -> Vec<f64> {
        roots.iter().fold(vec![1.0], |p, &r| mul(&p, &[-r, 1.0]))
    }

    /// Four simple roots, two outside the interval: the two inside, and
    /// the three turns of the quartic between them.
    #[test]
    fn simple_roots_are_found_to_the_last_bit() {
        let p = with_roots(&[-0.5, 0.2, 0.7, 1.5]);
        let found = roots_in(&p, 0.0, 1.0);
        for r in [0.2, 0.7] {
            let nearest = found
                .iter()
                .map(|&t| (t - r).abs())
                .fold(f64::INFINITY, f64::min);
            assert!(nearest <= 1e-15, "{r} not found: {found:?}");
        }
        assert!(found.len() <= 10, "{found:?}");
    }

    /// A double root does not change sign; it is found as the root of the
    /// derivative it also is.
    #[test]
    fn double_root_is_found_as_a_turn() {
        let p = with_roots(&[0.3, 0.3, 0.8, -2.0]);
        let found = roots_in(&p, 0.0, 1.0);
        let nearest = found
            .iter()
            .map(|&t| (t - 0.3).abs())
            .fold(f64::INFINITY, f64::min);
        assert!(nearest <= 1e-9, "the double root: {found:?}");
        assert!(found.iter().any(|&t| (t - 0.8).abs() <= 1e-15), "{found:?}");
    }

    /// A polynomial that vanishes identically, or is constant, has no
    /// roots; one whose leading coefficients are 0 is its lower degree.
    #[test]
    fn degenerate_polynomials() {
        assert!(roots_in(&[0.0; 5], 0.0, 1.0).is_empty());
        assert!(roots_in(&[2.0, 0.0, 0.0], 0.0, 1.0).is_empty());
        let found = roots_in(&[-0.25, 1.0, 0.0, 0.0, 0.0], 0.0, 1.0);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_approx_eq!(found.first().copied().unwrap_or(f64::NAN), 0.25, 0.0);
    }
}
