// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A small deterministic random source (SplitMix64), so a design depends
//! on its seed alone, on every platform and with no dependency.

/// SplitMix64 (Steele, Lea and Flood, "Fast splittable pseudorandom
/// number generators", OOPSLA 2014): a 64-bit state stepped by a constant
/// and mixed.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// The source for `seed`.
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// The next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `[0, 1)`.
    #[allow(
        clippy::cast_precision_loss,
        reason = "53 bits fit an f64's mantissa exactly"
    )]
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A number in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }

    /// A number in `[lo, hi)` spread evenly over its logarithm, so small
    /// values (a 0.001 mm gap, a 0.1 mm ridge) come up as often as large.
    pub fn log_range(&mut self, lo: f64, hi: f64) -> f64 {
        (lo.ln() + (hi.ln() - lo.ln()) * self.unit()).exp()
    }

    /// An integer in `lo..=hi`.
    #[allow(clippy::cast_possible_truncation, reason = "the span is a small count")]
    pub fn int(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next_u64() % (hi - lo + 1) as u64) as usize
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }

    /// One of `items`.
    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.int(0, items.len() - 1)]
    }

    /// A source of its own for a part of a design, so adding a draw to one
    /// part leaves the others as they were.
    pub fn fork(&mut self) -> Self {
        Self(self.next_u64())
    }
}
