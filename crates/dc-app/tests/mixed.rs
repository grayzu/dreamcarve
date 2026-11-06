// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/feature-tree/mixedValue.test.ts

//! The common- and mixed-value helpers behind the Properties panel's
//! multi-selection display (phase 4 design, "PureCutCNC sources",
//! `dc-app::tree`; #534), with upstream's `commonNumber`,
//! `commonBoolean`, `minValue` and `maxValue` cases, named as upstream
//! describes them.
//!
//! What changed: upstream compares numbers with `!==`; DreamCarve compares
//! within a tolerance the caller names (`docs/rust-guidelines.md`, no `==`
//! between floats), so one more case shows two values a rounding apart
//! counting as common. Upstream's `undefined` and `null` are both `None`.
//! The Z-range helpers (`zDomainMax`, `clampDomainMax`, `constrainZ`,
//! `validateZEdits`, `zHandleAriaBounds`, `snapDragZ`) and their cases are
//! not ported: a DreamCarve instance has no Z range to edit in this phase
//! (its `z_top_mm` and `z_bottom_mm` are reserved, and nothing reads them).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::mixed::{common_bool, common_number, common_value, max_value, min_value};

/// Upstream's `TestItem`.
#[derive(Debug, Clone, Copy, Default)]
struct Item {
    a: f64,
    b: Option<f64>,
    visible: Option<bool>,
}

fn a(a: f64) -> Item {
    Item {
        a,
        ..Item::default()
    }
}

/// Values compared to a billionth.
const TOL: f64 = 1e-9;

fn near(got: Option<f64>, want: f64) -> bool {
    got.is_some_and(|v| (v - want).abs() < 1e-12)
}

// ── commonNumber ────────────────────────────────────────────────────

/// commonNumber: null for empty array
#[test]
fn common_number_none_for_empty() {
    assert_eq!(common_number(&[] as &[Item], |i| Some(i.a), TOL), None);
}

/// commonNumber: returns value when all items share it
#[test]
fn common_number_returns_shared_value() {
    let items = [a(5.0), a(5.0), a(5.0)];
    assert!(near(common_number(&items, |i| Some(i.a), TOL), 5.0));
}

/// commonNumber: null when values differ
#[test]
fn common_number_none_when_values_differ() {
    let items = [a(5.0), a(7.0), a(5.0)];
    assert_eq!(common_number(&items, |i| Some(i.a), TOL), None);
}

/// commonNumber: null when first value is undefined
#[test]
fn common_number_none_when_first_is_unset() {
    let items = [
        Item { b: None, ..a(0.0) },
        Item {
            b: Some(3.0),
            ..a(0.0)
        },
    ];
    assert_eq!(common_number(&items, |i| i.b, TOL), None);
}

/// commonNumber: null when accessor returns null
#[test]
fn common_number_none_when_accessor_returns_none() {
    let items = [a(0.0), a(0.0)];
    assert_eq!(common_number(&items, |_| None, TOL), None);
}

/// commonNumber: single-item array
#[test]
fn common_number_single_item() {
    assert!(near(common_number(&[a(42.0)], |i| Some(i.a), TOL), 42.0));
}

/// commonNumber: zero is a valid common value
#[test]
fn common_number_zero_is_a_value() {
    let items = [a(0.0), a(0.0)];
    assert!(near(common_number(&items, |i| Some(i.a), TOL), 0.0));
}

/// DreamCarve's addition: two values within the tolerance are common,
/// the first one's value returned; beyond it they are mixed.
#[test]
fn common_number_within_the_tolerance() {
    let items = [a(90.0), a(90.0 + 1e-12)];
    assert!(near(common_number(&items, |i| Some(i.a), TOL), 90.0));
    let items = [a(90.0), a(90.001)];
    assert_eq!(common_number(&items, |i| Some(i.a), TOL), None);
}

// ── commonBoolean ───────────────────────────────────────────────────

/// commonBoolean: null for empty array
#[test]
fn common_bool_none_for_empty() {
    assert_eq!(
        common_bool(&[] as &[Item], |i| Some(i.visible.unwrap_or(false))),
        None
    );
}

/// commonBoolean: true when all true
#[test]
fn common_bool_true_when_all_true() {
    let items = [
        Item {
            visible: Some(true),
            ..a(0.0)
        },
        Item {
            visible: Some(true),
            ..a(0.0)
        },
    ];
    assert_eq!(
        common_bool(&items, |i| Some(i.visible.unwrap_or(false))),
        Some(true)
    );
}

/// commonBoolean: false when all false
#[test]
fn common_bool_false_when_all_false() {
    let items = [
        Item {
            visible: Some(false),
            ..a(0.0)
        },
        Item {
            visible: Some(false),
            ..a(0.0)
        },
    ];
    assert_eq!(
        common_bool(&items, |i| Some(i.visible.unwrap_or(true))),
        Some(false)
    );
}

/// commonBoolean: null when mixed
#[test]
fn common_bool_none_when_mixed() {
    let items = [
        Item {
            visible: Some(true),
            ..a(0.0)
        },
        Item {
            visible: Some(false),
            ..a(0.0)
        },
    ];
    assert_eq!(
        common_bool(&items, |i| Some(i.visible.unwrap_or(false))),
        None
    );
}

/// commonValue, which upstream exports untested, over a value type.
#[test]
fn common_value_of_a_value_type() {
    let names = ["rect", "rect", "rect"];
    assert_eq!(common_value(&names, |n| Some(*n)), Some("rect"));
    let names = ["rect", "smooth"];
    assert_eq!(common_value(&names, |n| Some(*n)), None);
    assert_eq!(common_value(&[] as &[&str], |n| Some(*n)), None);
}

// ── minValue ────────────────────────────────────────────────────────

/// minValue: null for empty array
#[test]
fn min_value_none_for_empty() {
    assert_eq!(min_value(&[] as &[Item], |i| Some(i.a)), None);
}

/// minValue: returns minimum
#[test]
fn min_value_returns_minimum() {
    let items = [a(5.0), a(3.0), a(7.0)];
    assert!(near(min_value(&items, |i| Some(i.a)), 3.0));
}

/// minValue: skips undefined values
#[test]
fn min_value_skips_unset() {
    let items = [
        a(5.0),
        Item { b: None, ..a(0.0) },
        Item {
            b: Some(2.0),
            ..a(0.0)
        },
    ];
    assert!(near(min_value(&items, |i| i.b), 2.0));
}

/// minValue: null when all null/undefined
#[test]
fn min_value_none_when_all_unset() {
    let items = [a(5.0), a(3.0)];
    assert_eq!(min_value(&items, |_| None), None);
}

/// Upstream skips non-finite values (`Number.isFinite`).
#[test]
fn min_value_skips_non_finite() {
    let items = [a(f64::NAN), a(f64::NEG_INFINITY), a(4.0)];
    assert!(near(min_value(&items, |i| Some(i.a)), 4.0));
}

// ── maxValue ────────────────────────────────────────────────────────

/// maxValue: returns maximum
#[test]
fn max_value_returns_maximum() {
    let items = [a(5.0), a(3.0), a(7.0)];
    assert!(near(max_value(&items, |i| Some(i.a)), 7.0));
}
