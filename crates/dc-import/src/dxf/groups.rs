// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/dxf.ts (parsePairs)

//! The DXF group-code reader: the bytes of an ASCII or binary DXF file as
//! a flat list of (group code, value) pairs (`docs/specs/phase-2/design.md`,
//! "Algorithms → DXF import", step 1). Changed from upstream, which reads
//! ASCII only and skips a pair whose code is not a number: binary DXF is
//! read too, and a malformed pair is an error, since the pairs after it can
//! no longer be framed.
//!
//! The format is Autodesk's published DXF Reference: "Group Code Value
//! Types" for how each code's value is typed, "ASCII DXF Files" (a code
//! line, then a value line) and "Binary DXF Files" (a 22-byte sentinel, then
//! each group as a little-endian code of 2 bytes, or of 1 byte before R13,
//! and its value: NUL-terminated strings, little-endian integers and IEEE
//! doubles, binary chunks as a length byte and the bytes).

use std::borrow::Cow;

use super::DxfReadError;

/// The first 22 bytes of a binary DXF file ("Binary DXF Files").
const BINARY_SENTINEL: &[u8] = b"AutoCAD Binary DXF\r\n\x1a\0";

/// A group's value, typed by its code.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Value {
    /// Strings, names and handles; in ASCII files also the value of any
    /// code the reference does not define.
    Text(String),
    /// Double-precision values: coordinates, distances, angles.
    Real(f64),
    /// 16-, 32- and 64-bit integers and booleans.
    Int(i64),
    /// A binary chunk (codes 310–319, 1004). DreamCarve reads none, so the
    /// bytes are not kept.
    Binary,
}

/// One (group code, value) pair.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Group {
    pub code: i32,
    pub value: Value,
}

impl Group {
    /// The value as a number, for real or integer codes.
    pub(super) fn real(&self) -> Option<f64> {
        match &self.value {
            Value::Real(v) => Some(*v),
            Value::Int(i) => i32::try_from(*i).ok().map(f64::from),
            Value::Text(_) | Value::Binary => None,
        }
    }

    /// The value as an integer, for integer codes.
    pub(super) fn int(&self) -> Option<i64> {
        match &self.value {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// The value as a string, for string codes.
    pub(super) fn text(&self) -> Option<&str> {
        match &self.value {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Whether this is the group `0` with the given value: the start of an
    /// entity, a section, or a section's end.
    pub(super) fn is_marker(&self, name: &str) -> bool {
        self.code == 0 && self.text().is_some_and(|t| t.eq_ignore_ascii_case(name))
    }
}

/// How a code's value is stored ("Group Code Value Types").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Text,
    Real,
    Int16,
    Int32,
    Int64,
    Bool,
    Binary,
}

impl Kind {
    /// The reference's value type for `code`; `None` for codes it does not
    /// define.
    const fn of(code: i32) -> Option<Self> {
        Some(match code {
            0..=9
            | 100..=102
            | 105
            | 300..=309
            | 320..=369
            | 390..=399
            | 410..=419
            | 430..=439
            | 470..=481
            | 999
            | 1000..=1003
            | 1005..=1009 => Self::Text,
            10..=59 | 110..=149 | 210..=239 | 460..=469 | 1010..=1059 => Self::Real,
            60..=79 | 170..=179 | 270..=289 | 370..=389 | 400..=409 | 1060..=1070 => Self::Int16,
            90..=99 | 420..=429 | 440..=459 | 1071 => Self::Int32,
            160..=169 => Self::Int64,
            290..=299 => Self::Bool,
            310..=319 | 1004 => Self::Binary,
            _ => return None,
        })
    }

    fn name(self) -> &'static str {
        match self {
            Self::Text => "string",
            Self::Real => "number",
            Self::Int16 | Self::Int32 | Self::Int64 => "integer",
            Self::Bool => "boolean (0 or 1)",
            Self::Binary => "hex string",
        }
    }
}

/// The groups of a DXF file, up to and including `0 EOF` (or the end of
/// the bytes when it is missing).
pub(super) fn read(bytes: &[u8]) -> Result<Vec<Group>, DxfReadError> {
    match bytes.strip_prefix(BINARY_SENTINEL) {
        Some(rest) => read_binary(rest),
        None => read_ascii(bytes),
    }
}

/// ASCII DXF: a code line and a value line per group, both trimmed. Blank
/// lines after the last group are allowed.
fn read_ascii(bytes: &[u8]) -> Result<Vec<Group>, DxfReadError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = decode(bytes);
    let mut lines = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l).trim());
    let mut groups = Vec::new();
    let mut line = 0;
    while let Some(code_text) = lines.next() {
        line += 1;
        if code_text.is_empty() && lines.clone().all(str::is_empty) {
            break;
        }
        let code: i32 = code_text.parse().map_err(|_| DxfReadError::BadCode {
            line,
            text: code_text.to_owned(),
        })?;
        let Some(value_text) = lines.next() else {
            return Err(DxfReadError::MissingValue { line, code });
        };
        line += 1;
        let bad = |kind: Kind| DxfReadError::BadValue {
            line,
            code,
            expected: kind.name(),
            text: value_text.to_owned(),
        };
        let value = match Kind::of(code) {
            Some(Kind::Real) => Value::Real(value_text.parse().map_err(|_| bad(Kind::Real))?),
            Some(kind @ (Kind::Int16 | Kind::Int32 | Kind::Int64 | Kind::Bool)) => {
                Value::Int(value_text.parse().map_err(|_| bad(kind))?)
            }
            Some(Kind::Binary) => Value::Binary,
            // Codes the reference does not define keep their text: the
            // pair is still framed by its two lines.
            Some(Kind::Text) | None => Value::Text(unescape(value_text).into_owned()),
        };
        let group = Group { code, value };
        let eof = group.is_marker("EOF");
        groups.push(group);
        if eof {
            break;
        }
    }
    Ok(groups)
}

/// Binary DXF after the sentinel. The first group is `0 SECTION` (or a
/// `999` comment): its code is `00 00` (`E7 03`) when codes take two bytes
/// (R13 and later), and `00` followed by the `S` of `SECTION` (`FF E7 03`)
/// when they take one.
fn read_binary(data: &[u8]) -> Result<Vec<Group>, DxfReadError> {
    let two_byte_codes = matches!(data, [_, 0, ..] | [0xE7, 0x03, ..]);
    let mut reader = Bytes { data, pos: 0 };
    let mut groups = Vec::new();
    while !reader.at_end() {
        let offset = reader.offset();
        let code = if two_byte_codes {
            i32::from(reader.u16()?)
        } else {
            match reader.u8()? {
                // A code of 255 or more: 255, then the code in two bytes.
                255 => i32::from(reader.u16()?),
                code => i32::from(code),
            }
        };
        let kind = Kind::of(code).ok_or(DxfReadError::UnknownBinaryCode { code, offset })?;
        let value = match kind {
            Kind::Text => Value::Text(reader.text()?),
            Kind::Real => Value::Real(f64::from_le_bytes(reader.array()?)),
            Kind::Int16 => Value::Int(i64::from(i16::from_le_bytes(reader.array()?))),
            Kind::Int32 => Value::Int(i64::from(i32::from_le_bytes(reader.array()?))),
            Kind::Int64 => Value::Int(i64::from_le_bytes(reader.array()?)),
            // One byte from R13 on, a 16-bit integer before.
            Kind::Bool if two_byte_codes => Value::Int(i64::from(reader.u8()?)),
            Kind::Bool => Value::Int(i64::from(i16::from_le_bytes(reader.array()?))),
            Kind::Binary => {
                let length = usize::from(reader.u8()?);
                reader.take(length)?;
                Value::Binary
            }
        };
        let group = Group { code, value };
        let eof = group.is_marker("EOF");
        groups.push(group);
        if eof {
            break;
        }
    }
    Ok(groups)
}

/// A cursor over the bytes after the sentinel.
struct Bytes<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bytes<'a> {
    fn at_end(&self) -> bool {
        self.pos >= self.data.len()
    }

    /// Position in the file, counting the sentinel.
    fn offset(&self) -> usize {
        self.pos + BINARY_SENTINEL.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DxfReadError> {
        let end = self.pos.checked_add(n);
        let bytes = end.and_then(|end| self.data.get(self.pos..end)).ok_or(
            DxfReadError::TruncatedBinary {
                offset: self.offset(),
            },
        )?;
        self.pos += n;
        Ok(bytes)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], DxfReadError> {
        let offset = self.offset();
        <[u8; N]>::try_from(self.take(N)?).map_err(|_| DxfReadError::TruncatedBinary { offset })
    }

    fn u8(&mut self) -> Result<u8, DxfReadError> {
        let [b] = self.array()?;
        Ok(b)
    }

    fn u16(&mut self) -> Result<u16, DxfReadError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// A NUL-terminated string.
    fn text(&mut self) -> Result<String, DxfReadError> {
        let rest = self.data.get(self.pos..).unwrap_or_default();
        let len = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or(DxfReadError::TruncatedBinary {
                offset: self.offset(),
            })?;
        let bytes = self.take(len)?;
        self.pos += 1;
        Ok(unescape(&decode(bytes)).into_owned())
    }
}

/// Text as UTF-8 (R2007 and later write UTF-8), else as Windows-1252, the
/// code page most earlier files are written in (`$DWGCODEPAGE`
/// `ANSI_1252`).
fn decode(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => Cow::Owned(bytes.iter().map(|&b| windows_1252(b)).collect()),
    }
}

/// One Windows-1252 byte as a character: Latin-1 except 0x80–0x9F, which
/// hold typographic characters (Microsoft, "Code Page 1252 Windows Latin
/// 1"); the five unassigned bytes map to the C1 control of the same value.
fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '\u{20AC}', '\u{81}', '\u{201A}', '\u{192}', '\u{201E}', '\u{2026}', '\u{2020}',
        '\u{2021}', '\u{2C6}', '\u{2030}', '\u{160}', '\u{2039}', '\u{152}', '\u{8D}', '\u{17D}',
        '\u{8F}', '\u{90}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}',
        '\u{2014}', '\u{2DC}', '\u{2122}', '\u{161}', '\u{203A}', '\u{153}', '\u{9D}', '\u{17E}',
        '\u{178}',
    ];
    match byte {
        0x80..=0x9F => HIGH
            .get(usize::from(byte - 0x80))
            .copied()
            .unwrap_or(char::REPLACEMENT_CHARACTER),
        _ => char::from(byte),
    }
}

/// Decodes `\U+XXXX` escapes, which files before R2007 use for characters
/// outside their code page.
fn unescape(text: &str) -> Cow<'_, str> {
    if !text.contains("\\U+") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\\U+") {
        out.push_str(rest.get(..at).unwrap_or_default());
        let after = rest.get(at + 3..).unwrap_or_default();
        let escaped = after
            .get(..4)
            .filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32);
        match escaped {
            Some(c) => {
                out.push(c);
                rest = after.get(4..).unwrap_or_default();
            }
            None => {
                out.push_str("\\U+");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]

    use super::*;

    fn text(code: i32, value: &str) -> Group {
        Group {
            code,
            value: Value::Text(value.to_owned()),
        }
    }

    #[test]
    fn ascii_values_are_typed_by_code() {
        let groups =
            read(b"0\r\nLINE\r\n  8\r\nwalls \r\n10\r\n1.5\r\n70\r\n   3\r\n310\r\nFF\r\n")
                .unwrap();
        assert_eq!(
            groups,
            [
                text(0, "LINE"),
                text(8, "walls"),
                Group {
                    code: 10,
                    value: Value::Real(1.5)
                },
                Group {
                    code: 70,
                    value: Value::Int(3)
                },
                Group {
                    code: 310,
                    value: Value::Binary
                },
            ]
        );
    }

    #[test]
    fn ascii_stops_at_eof_and_allows_trailing_blank_lines() {
        let groups = read(b"0\nEOF\nanything after\n").unwrap();
        assert_eq!(groups, [text(0, "EOF")]);
        let groups = read(b"0\nSECTION\n\n\n").unwrap();
        assert_eq!(groups, [text(0, "SECTION")]);
        assert!(read(b"").unwrap().is_empty());
    }

    #[test]
    fn ascii_errors_name_the_line() {
        assert_eq!(
            read(b"0\nLINE\n10\nabc\n").unwrap_err(),
            DxfReadError::BadValue {
                line: 4,
                code: 10,
                expected: "number",
                text: "abc".into()
            }
        );
        assert_eq!(
            read(b"0\nLINE\n\nx\n").unwrap_err(),
            DxfReadError::BadCode {
                line: 3,
                text: String::new()
            }
        );
        assert_eq!(
            read(b"0\nLINE\n8").unwrap_err(),
            DxfReadError::MissingValue { line: 3, code: 8 }
        );
    }

    #[test]
    fn undefined_ascii_code_keeps_its_text() {
        let groups = read(b"85\n1.0\n").unwrap();
        assert_eq!(groups, [text(85, "1.0")]);
    }

    #[test]
    fn binary_two_byte_and_one_byte_codes() {
        let mut r13 = BINARY_SENTINEL.to_vec();
        r13.extend_from_slice(&[0, 0]);
        r13.extend_from_slice(b"SECTION\0");
        r13.extend_from_slice(&[10, 0]);
        r13.extend_from_slice(&2.5_f64.to_le_bytes());
        r13.extend_from_slice(&[34, 1, 1]); // 290, one byte
        r13.extend_from_slice(&[0x36, 0x01, 2, 0xAB, 0xCD]); // 310, two bytes
        r13.extend_from_slice(&[0, 0]);
        r13.extend_from_slice(b"EOF\0");
        let groups = read(&r13).unwrap();
        assert_eq!(groups.len(), 5);
        assert_eq!(groups[1].real(), Some(2.5));
        assert_eq!(groups[2].code, 290);
        assert_eq!(groups[2].int(), Some(1));
        assert_eq!(groups[3].value, Value::Binary);
        assert!(groups[4].is_marker("EOF"));

        let mut r12 = BINARY_SENTINEL.to_vec();
        r12.push(0);
        r12.extend_from_slice(b"SECTION\0");
        r12.push(70);
        r12.extend_from_slice(&(-2_i16).to_le_bytes());
        r12.push(255);
        r12.extend_from_slice(&1000_u16.to_le_bytes());
        r12.extend_from_slice(b"x\0");
        let groups = read(&r12).unwrap();
        assert_eq!(groups[1].int(), Some(-2));
        assert_eq!(groups[2], text(1000, "x"));
    }

    #[test]
    fn windows_1252_and_unicode_escapes() {
        assert_eq!(decode(b"caf\xE9 \x80"), "café €");
        assert_eq!(decode("café".as_bytes()), "café");
        assert_eq!(unescape("d\\U+00E9cor"), "décor");
        assert_eq!(unescape("a\\U+12"), "a\\U+12");
        assert_eq!(unescape("a\\U++123b"), "a\\U++123b");
    }
}
