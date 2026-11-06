// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/feature-tree/mixedValue.ts

//! The value a multi-selection shows in a field (`docs/specs/phase-4/design.md`,
//! "PureCutCNC sources", `dc-app::tree`; functional capability 58): the
//! one every item shares, or `None` when they differ and the field shows
//! *Mixed*. Upstream's `commonNumber`, `commonBoolean`, `commonValue`,
//! `minValue` and `maxValue`.
//!
//! What changed: upstream compares numbers with `!==`; here numbers agree
//! within a tolerance the caller names (`docs/rust-guidelines.md`: no `==`
//! between floats), and the first item's value is returned. Upstream's
//! `undefined` and `null` are both `None`. The Z-range helpers
//! (`constrainZ` and its kin) are not ported: an instance has no Z range
//! to edit in this phase. Nothing here depends on the Y direction.

use dc_geometry::approx_eq;

/// The number every item of `items` has through `accessor`, within
/// `tolerance`; `None` when `items` is empty, when the first item has no
/// value, or when any item's value differs from the first's.
#[must_use]
pub fn common_number<T>(
    items: &[T],
    accessor: impl Fn(&T) -> Option<f64>,
    tolerance: f64,
) -> Option<f64> {
    let (first, rest) = items.split_first()?;
    let value = accessor(first)?;
    rest.iter()
        .all(|item| accessor(item).is_some_and(|v| approx_eq(v, value, tolerance)))
        .then_some(value)
}

/// The flag every item of `items` has through `accessor`; `None` when
/// `items` is empty, the first has none, or they differ.
#[must_use]
pub fn common_bool<T>(items: &[T], accessor: impl Fn(&T) -> Option<bool>) -> Option<bool> {
    common_value(items, accessor)
}

/// The value every item of `items` has through `accessor`; `None` when
/// `items` is empty, the first has none, or they differ.
#[must_use]
pub fn common_value<T, V: PartialEq>(items: &[T], accessor: impl Fn(&T) -> Option<V>) -> Option<V> {
    let (first, rest) = items.split_first()?;
    let value = accessor(first)?;
    rest.iter()
        .all(|item| accessor(item).is_some_and(|v| v == value))
        .then_some(value)
}

/// The least finite value of `items` through `accessor`; `None` when there
/// is none.
#[must_use]
pub fn min_value<T>(items: &[T], accessor: impl Fn(&T) -> Option<f64>) -> Option<f64> {
    items
        .iter()
        .filter_map(accessor)
        .filter(|v| v.is_finite())
        .reduce(f64::min)
}

/// The greatest finite value of `items` through `accessor`; `None` when
/// there is none.
#[must_use]
pub fn max_value<T>(items: &[T], accessor: impl Fn(&T) -> Option<f64>) -> Option<f64> {
    items
        .iter()
        .filter_map(accessor)
        .filter(|v| v.is_finite())
        .reduce(f64::max)
}
