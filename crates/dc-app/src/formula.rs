// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Formulas in fields (the maintainer's request, 2026-11-25): a field
//! that takes a number takes a sum, and holds what it comes to. `1.5 +
//! .375 / 2` typed into a length is 1.6875, so nobody reaches for a
//! calculator to halve a board or add a kerf. The sum is worked out as it
//! is read and nothing of it is kept: no variables, no names, no formula
//! in the file, which holds numbers as it always has.
//!
//! PureCutCNC has nothing of the kind to port.
//!
//! [`resolved`] is run on the text every number field reads, before the
//! field's own parser. It leaves alone what is no sum, so a plain number,
//! a fraction (`1/4 in`) and a mixed number (`1 1/2`) mean exactly what
//! the file's grammar says they mean, and text no sum could be is refused
//! by the field in its own words. A sum is replaced by its value written
//! as a plain decimal, with the unit that followed it:
//!
//! - `+`, `-`, `*` and `/` (also `×` and `÷`), `*` and `/` first, left to
//!   right; brackets; a sign before a number or a bracket.
//! - Numbers are decimals, with or without a leading zero. In a sum a
//!   fraction is a division, and `1 1/2` (a whole number, a space, a
//!   fraction with no space in it) is one and a half.
//! - One unit, a percent sign or a degree sign, after the sum, is the
//!   result's: `(1 + 1/2) in`. A sum of two units (`1 in + 3 mm`) is
//!   refused rather than guessed at.
//!
//! The value is rounded to nine decimal places, so `0.1 + 0.2` is 0.3 and
//! not the nearest binary number to it.

use std::borrow::Cow;
use std::iter::Peekable;
use std::str::Chars;

/// How deep brackets may nest: far past any sum typed by hand, and well
/// short of the stack.
const MAX_DEPTH: usize = 32;
/// The decimal places a result is written to.
const PLACES: usize = 9;

/// `text` with the sum in it worked out, as a field that reads lengths
/// or rates then parses it: the text itself when it holds no sum.
///
/// # Errors
///
/// Why the sum cannot be worked out, as the field shows it: unfinished, a
/// bracket unmatched, a division by zero, a unit inside it, or not a sum
/// of numbers at all.
pub fn resolved(text: &str) -> Result<Cow<'_, str>, String> {
    resolve(text, true)
}

/// [`resolved`] for a field of plain decimals (a percentage, an angle, a
/// count, seconds), which has no fractions of its own: there `360/7` is a
/// sum too.
///
/// # Errors
///
/// As [`resolved`].
pub fn resolved_decimal(text: &str) -> Result<Cow<'_, str>, String> {
    resolve(text, false)
}

/// [`resolved`], refusing as a field's parser refuses.
pub(crate) fn typed(text: &str) -> Result<Cow<'_, str>, crate::CommandError> {
    resolved(text).map_err(|reason| crate::CommandError::NotAQuantity {
        text: text.to_owned(),
        reason,
    })
}

/// [`resolved_decimal`], refusing as a field's parser refuses.
pub(crate) fn typed_decimal(text: &str) -> Result<Cow<'_, str>, crate::CommandError> {
    resolved_decimal(text).map_err(|reason| crate::CommandError::NotAQuantity {
        text: text.to_owned(),
        reason,
    })
}

fn resolve(text: &str, fractions: bool) -> Result<Cow<'_, str>, String> {
    let trimmed = text.trim();
    // The unit starts at the first letter, as the file's grammar has it,
    // or at a percent or degree sign.
    let split = trimmed
        .find(|c: char| c.is_alphabetic() || matches!(c, '%' | '°'))
        .unwrap_or(trimmed.len());
    let (head, unit) = trimmed.split_at(split);
    let head = head.trim_end();
    let operators = ['+', '-', '*', '/', '(', ')', '×', '÷'];
    if is_plain(head, fractions) || !head.contains(operators) {
        // A sum of two units is no sum this can do, and says so.
        if !head.is_empty()
            && (unit.contains(['+', '*', '(', ')', '×', '÷']) || unit.contains(" - "))
        {
            return Err(format!(
                "`{trimmed}` has a unit inside it: write the unit once, at the end, as in `(1 + \
                 3) mm`"
            ));
        }
        return Ok(Cow::Borrowed(text));
    }
    let value = evaluate(head).map_err(|problem| problem.said_of(trimmed))?;
    if !value.is_finite() {
        return Err(format!("`{trimmed}` is too large"));
    }
    let number = written(value);
    Ok(Cow::Owned(if unit.is_empty() {
        number
    } else {
        format!("{number} {unit}")
    }))
}

/// Whether `head`, the text before any unit, is a number as a field's
/// own parser reads one: a decimal with an optional sign, and, with
/// `fractions`, the file's `a/b` and `w a/b` (no space beside the slash).
/// Empty text is plain: the field says nothing was typed.
fn is_plain(head: &str, fractions: bool) -> bool {
    let unsigned = head
        .strip_prefix('-')
        .or_else(|| head.strip_prefix('+'))
        .unwrap_or(head);
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let decimal = |s: &str| {
        let (whole, part) = s.split_once('.').unwrap_or((s, ""));
        let all = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
        !(whole.is_empty() && part.is_empty()) && all(whole) && all(part)
    };
    if head.is_empty() {
        return true;
    }
    match unsigned.split_once('/') {
        Some((before, denominator)) if fractions => {
            let numerator = match before.rsplit_once(char::is_whitespace) {
                Some((whole, numerator)) => {
                    if !digits(whole.trim_end()) {
                        return false;
                    }
                    numerator
                }
                None => before,
            };
            digits(numerator) && digits(denominator)
        }
        Some(_) => false,
        None => decimal(unsigned),
    }
}

/// `value` as a plain decimal: to [`PLACES`] places, without trailing
/// zeros, a bare point or a negative zero.
fn written(value: f64) -> String {
    let text = format!("{value:.PLACES$}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text.strip_prefix('-') {
        Some(digits) if digits.chars().all(|c| c == '0' || c == '.') || digits.is_empty() => {
            "0".to_owned()
        }
        _ if text.is_empty() => "0".to_owned(),
        _ => text.to_owned(),
    }
}

/// Why a sum cannot be worked out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Problem {
    /// It ends where a number should be.
    Unfinished,
    /// A bracket is opened and never closed.
    Unclosed,
    /// A bracket is closed that was never opened.
    Unopened,
    /// It divides by zero.
    DivisionByZero,
    /// Anything else: two numbers with nothing between them, a sign with
    /// nothing after it but another, brackets nested past [`MAX_DEPTH`].
    NotASum,
}

impl Problem {
    /// The problem as a field shows it, of the text typed.
    fn said_of(self, text: &str) -> String {
        match self {
            Self::Unfinished => {
                format!("`{text}` is not finished: a number is missing at its end")
            }
            Self::Unclosed => format!("`{text}` has a bracket that is never closed"),
            Self::Unopened => format!("`{text}` closes a bracket that was never opened"),
            Self::DivisionByZero => format!("`{text}` divides by zero"),
            Self::NotASum => format!(
                "`{text}` is not a sum this field can work out: use numbers with + - * / and \
                 brackets"
            ),
        }
    }
}

/// What `sum` comes to.
fn evaluate(sum: &str) -> Result<f64, Problem> {
    let mut reader = Reader {
        chars: sum.chars().peekable(),
        depth: 0,
    };
    let value = reader.sum()?;
    reader.skip_spaces();
    match reader.chars.peek() {
        None => Ok(value),
        Some(')') => Err(Problem::Unopened),
        Some(_) => Err(Problem::NotASum),
    }
}

/// A sum being read, left to right.
struct Reader<'a> {
    chars: Peekable<Chars<'a>>,
    /// How many brackets are open.
    depth: usize,
}

impl Reader<'_> {
    fn skip_spaces(&mut self) {
        while self.chars.next_if(|c| c.is_whitespace()).is_some() {}
    }

    /// Terms added and subtracted, left to right.
    fn sum(&mut self) -> Result<f64, Problem> {
        let mut value = self.product()?;
        loop {
            self.skip_spaces();
            if self.chars.next_if_eq(&'+').is_some() {
                value += self.product()?;
            } else if self.chars.next_if_eq(&'-').is_some() {
                value -= self.product()?;
            } else {
                return Ok(value);
            }
        }
    }

    /// Factors multiplied and divided, left to right.
    fn product(&mut self) -> Result<f64, Problem> {
        let mut value = self.factor()?;
        loop {
            self.skip_spaces();
            if self.chars.next_if(|c| matches!(c, '*' | '×')).is_some() {
                value *= self.factor()?;
            } else if self.chars.next_if(|c| matches!(c, '/' | '÷')).is_some() {
                let divisor = self.factor()?;
                if divisor == 0.0 {
                    return Err(Problem::DivisionByZero);
                }
                value /= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    /// A number or a bracketed sum, with a sign before it or not.
    fn factor(&mut self) -> Result<f64, Problem> {
        self.skip_spaces();
        if self.chars.next_if_eq(&'-').is_some() {
            return self.unsigned().map(|value| -value);
        }
        if self.chars.next_if_eq(&'+').is_some() {
            return self.unsigned();
        }
        self.unsigned()
    }

    /// A number or a bracketed sum.
    fn unsigned(&mut self) -> Result<f64, Problem> {
        self.skip_spaces();
        match self.chars.peek() {
            None => Err(Problem::Unfinished),
            Some('(') => {
                self.chars.next();
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(Problem::NotASum);
                }
                let value = self.sum()?;
                self.skip_spaces();
                if self.chars.next_if_eq(&')').is_none() {
                    return Err(if self.chars.peek().is_none() {
                        Problem::Unclosed
                    } else {
                        Problem::NotASum
                    });
                }
                self.depth -= 1;
                Ok(value)
            }
            Some(c) if c.is_ascii_digit() || *c == '.' => self.number(),
            Some(_) => Err(Problem::NotASum),
        }
    }

    /// A decimal; or a whole number, a space and a fraction, which is a
    /// mixed number as the file's grammar has it (`1 1/2`).
    fn number(&mut self) -> Result<f64, Problem> {
        let mut text = String::new();
        while let Some(c) = self.chars.next_if(char::is_ascii_digit) {
            text.push(c);
        }
        let whole = !text.is_empty();
        if self.chars.next_if_eq(&'.').is_some() {
            text.push('.');
            while let Some(c) = self.chars.next_if(char::is_ascii_digit) {
                text.push(c);
            }
            return text.parse().map_err(|_| Problem::NotASum);
        }
        let value: f64 = text.parse().map_err(|_| Problem::NotASum)?;
        if whole && let Some(fraction) = self.fraction_after_space()? {
            return Ok(value + fraction);
        }
        Ok(value)
    }

    /// The fraction of a mixed number, when one follows: spaces, digits, a
    /// slash and digits with no space beside the slash. Nothing is read
    /// when what follows is anything else.
    fn fraction_after_space(&mut self) -> Result<Option<f64>, Problem> {
        let mut ahead = self.chars.clone();
        if ahead.next_if(|c| c.is_whitespace()).is_none() {
            return Ok(None);
        }
        while ahead.next_if(|c| c.is_whitespace()).is_some() {}
        let digits = |ahead: &mut Peekable<Chars<'_>>| {
            let mut text = String::new();
            while let Some(c) = ahead.next_if(char::is_ascii_digit) {
                text.push(c);
            }
            text
        };
        let numerator = digits(&mut ahead);
        if numerator.is_empty() || ahead.next_if_eq(&'/').is_none() {
            return Ok(None);
        }
        let denominator = digits(&mut ahead);
        if denominator.is_empty() {
            return Ok(None);
        }
        let numerator: f64 = numerator.parse().map_err(|_| Problem::NotASum)?;
        let denominator: f64 = denominator.parse().map_err(|_| Problem::NotASum)?;
        if denominator == 0.0 {
            return Err(Problem::DivisionByZero);
        }
        self.chars = ahead;
        Ok(Some(numerator / denominator))
    }
}
