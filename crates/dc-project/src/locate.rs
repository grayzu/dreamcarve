// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Where a value sits in a `.dcv` file's text (#76), so a message can say
//! `coaster.dcv:9:21:` and an editor can jump there.
//!
//! serde_json keeps no positions, so [`SourceMap::parse`] scans the text
//! itself and records the line and column of every value. It reads the text
//! the user has, whatever its layout or key order, so the positions stay
//! right in a file reformatted or reordered by hand.
//!
//! Two ways to ask for a position:
//!
//! - [`SourceMap::position`] takes a path of [`PathSegment`]s, keys and
//!   array indices, as a JSON pointer does. A schema error (#65) can be
//!   located this way.
//! - [`SourceMap::locate`] takes a field as [`crate::Problem::field`] names
//!   it, such as `tools[t1].feed_mm_min`: a bracket after an array picks the
//!   element whose `id` is the text in it, or else the element at that
//!   index; a bracket after an object picks that key.

use crate::serialize::LoadError;
use std::fmt;

/// Deepest nesting the scan follows, as serde_json's default limit. A
/// deeper document fails to parse rather than overflow the stack.
const MAX_DEPTH: usize = 128;

/// A position in a text: 1-based line and 1-based column, counted in
/// characters (a tab is one column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextPosition {
    /// 1-based line.
    pub line: usize,
    /// 1-based column, in characters.
    pub column: usize,
}

impl fmt::Display for TextPosition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// One step of a path into a JSON document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSegment {
    /// A member of an object.
    Key(String),
    /// An element of an array, 0-based.
    Index(usize),
}

/// The position of every value in a JSON document.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceMap {
    root: Node,
}

#[derive(Debug, Clone, PartialEq)]
struct Node {
    /// Where the value starts: its first character.
    at: TextPosition,
    kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq)]
enum NodeKind {
    /// An object's members in document order.
    Object(Vec<(String, Node)>),
    Array(Vec<Node>),
    /// A string, kept so an array element can be found by its `id`.
    String(String),
    /// A number, `true`, `false` or `null`.
    Other,
}

impl SourceMap {
    /// Scans `text`, a JSON document. Fails with [`LoadError::Json`] at the
    /// first place the text is not JSON.
    pub fn parse(text: &str) -> Result<Self, LoadError> {
        let mut scanner = Scanner::new(text);
        scanner.skip_whitespace();
        let root = scanner.value(0)?;
        scanner.skip_whitespace();
        if scanner.peek().is_some() {
            return Err(scanner.error("trailing characters after the document"));
        }
        Ok(Self { root })
    }

    /// The position of the value at `path`, or `None` when the document has
    /// no value there.
    #[must_use]
    pub fn position(&self, path: &[PathSegment]) -> Option<TextPosition> {
        let mut node = &self.root;
        for segment in path {
            node = match (segment, &node.kind) {
                (PathSegment::Key(key), NodeKind::Object(members)) => member(members, key)?,
                (PathSegment::Index(i), NodeKind::Array(items)) => items.get(*i)?,
                _ => return None,
            };
        }
        Some(node.at)
    }

    /// The position of `field`, written as validation names fields
    /// (`stock.thickness_mm`, `tools[t1].rpm`,
    /// `definitions[d1].profiles[0].vertices[2].x`), or `None` when the
    /// document has no value there.
    #[must_use]
    pub fn locate(&self, field: &str) -> Option<TextPosition> {
        let mut node = &self.root;
        for step in field_steps(field)? {
            node = match (step, &node.kind) {
                (FieldStep::Key(key), NodeKind::Object(members)) => member(members, key)?,
                (FieldStep::Bracket(key), NodeKind::Object(members)) => member(members, key)?,
                (FieldStep::Bracket(id), NodeKind::Array(items)) => items
                    .iter()
                    .find(|item| has_id(item, id))
                    .or_else(|| id.parse::<usize>().ok().and_then(|i| items.get(i)))?,
                _ => return None,
            };
        }
        Some(node.at)
    }
}

/// The member `key` of an object. If the key repeats, the last one, which
/// is the one serde_json keeps.
fn member<'a>(members: &'a [(String, Node)], key: &str) -> Option<&'a Node> {
    members.iter().rev().find(|(k, _)| k == key).map(|(_, n)| n)
}

/// Whether `node` is an object whose `id` is the string `id`.
fn has_id(node: &Node, id: &str) -> bool {
    match &node.kind {
        NodeKind::Object(members) => {
            matches!(member(members, "id"), Some(Node { kind: NodeKind::String(s), .. }) if s == id)
        }
        _ => false,
    }
}

/// One step of a field as validation writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldStep<'a> {
    /// `.name`, or the first name.
    Key(&'a str),
    /// `[text]`.
    Bracket(&'a str),
}

/// Splits `stock.thickness_mm` or `tools[t1].rpm` into steps; `None` when
/// the field is malformed (an empty name, an unclosed bracket).
fn field_steps(field: &str) -> Option<Vec<FieldStep<'_>>> {
    let mut steps = Vec::new();
    let mut rest = field;
    let mut first = true;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('[') {
            let end = after.find(']')?;
            steps.push(FieldStep::Bracket(after.get(..end)?));
            rest = after.get(end + 1..)?;
        } else {
            let name = if first { rest } else { rest.strip_prefix('.')? };
            let end = name.find(['.', '[']).unwrap_or(name.len());
            let key = name.get(..end)?;
            if key.is_empty() {
                return None;
            }
            steps.push(FieldStep::Key(key));
            rest = name.get(end..)?;
        }
        first = false;
    }
    Some(steps)
}

/// A character cursor that counts lines and columns.
struct Scanner<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
    column: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            chars: text.chars().peekable(),
            line: 1,
            column: 1,
        }
    }

    fn at(&self) -> TextPosition {
        TextPosition {
            line: self.line,
            column: self.column,
        }
    }

    fn error(&self, message: &str) -> LoadError {
        LoadError::Json {
            line: self.line,
            column: self.column,
            message: format!("{message} at line {} column {}", self.line, self.column),
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.chars.next()?;
        if c == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }
        Some(c)
    }

    fn expect(&mut self, wanted: char) -> Result<(), LoadError> {
        if self.peek() == Some(wanted) {
            self.next();
            Ok(())
        } else {
            Err(self.error(&format!("expected `{wanted}`")))
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.next();
        }
    }

    fn value(&mut self, depth: usize) -> Result<Node, LoadError> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        let at = self.at();
        let kind = match self.peek() {
            Some('{') => self.object(depth)?,
            Some('[') => self.array(depth)?,
            Some('"') => NodeKind::String(self.string()?),
            Some('t') => self.literal("true")?,
            Some('f') => self.literal("false")?,
            Some('n') => self.literal("null")?,
            Some('-' | '0'..='9') => self.number()?,
            _ => return Err(self.error("expected a value")),
        };
        Ok(Node { at, kind })
    }

    fn object(&mut self, depth: usize) -> Result<NodeKind, LoadError> {
        self.expect('{')?;
        let mut members = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some('}') {
            self.next();
            return Ok(NodeKind::Object(members));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some('"') {
                return Err(self.error("expected a key"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            self.expect(':')?;
            self.skip_whitespace();
            members.push((key, self.value(depth + 1)?));
            self.skip_whitespace();
            match self.next() {
                Some(',') => {}
                Some('}') => return Ok(NodeKind::Object(members)),
                _ => return Err(self.error("expected `,` or `}`")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<NodeKind, LoadError> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(']') {
            self.next();
            return Ok(NodeKind::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth + 1)?);
            self.skip_whitespace();
            match self.next() {
                Some(',') => {}
                Some(']') => return Ok(NodeKind::Array(items)),
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
    }

    fn string(&mut self) -> Result<String, LoadError> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.next() {
                None => return Err(self.error("unterminated string")),
                Some('"') => return Ok(out),
                Some('\\') => {
                    let c = match self.next() {
                        Some('"') => '"',
                        Some('\\') => '\\',
                        Some('/') => '/',
                        Some('b') => '\u{8}',
                        Some('f') => '\u{c}',
                        Some('n') => '\n',
                        Some('r') => '\r',
                        Some('t') => '\t',
                        Some('u') => self.unicode_escape()?,
                        _ => return Err(self.error("invalid escape")),
                    };
                    out.push(c);
                }
                Some(c) if c < ' ' => return Err(self.error("control character in string")),
                Some(c) => out.push(c),
            }
        }
    }

    /// The character of a `\uXXXX` escape, the `\u` already read, joining
    /// a surrogate pair. A lone surrogate becomes U+FFFD: only positions
    /// matter here, and serde_json rejects the file on load anyway.
    fn unicode_escape(&mut self) -> Result<char, LoadError> {
        let high = self.hex4()?;
        if !(0xD800..0xDC00).contains(&high) {
            return Ok(char::from_u32(high).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
        if self.peek() != Some('\\') {
            return Ok(char::REPLACEMENT_CHARACTER);
        }
        self.next();
        if self.next() != Some('u') {
            return Err(self.error("invalid escape"));
        }
        let low = self.hex4()?;
        let code = 0x10000 + ((high - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF);
        Ok(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER))
    }

    fn hex4(&mut self) -> Result<u32, LoadError> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self
                .next()
                .and_then(|c| c.to_digit(16))
                .ok_or_else(|| self.error("invalid \\u escape"))?;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn literal(&mut self, word: &str) -> Result<NodeKind, LoadError> {
        for wanted in word.chars() {
            if self.peek() != Some(wanted) {
                return Err(self.error(&format!("expected `{word}`")));
            }
            self.next();
        }
        Ok(NodeKind::Other)
    }

    /// Skips a number. Its exact syntax is serde_json's to check on load.
    fn number(&mut self) -> Result<NodeKind, LoadError> {
        let mut any = false;
        while matches!(self.peek(), Some('-' | '+' | '.' | 'e' | 'E' | '0'..='9')) {
            self.next();
            any = true;
        }
        if any {
            Ok(NodeKind::Other)
        } else {
            Err(self.error("expected a number"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hand-written project excerpt, laid out as no serializer would.
    const DOCUMENT: &str = r#"{
  "stock": { "width_mm": 100.0, "thickness_mm": null },
  "tools": [
    {
      "id": "t0",
      "feed_mm_min": 900.0
    },
    {
      "id": "t1",
        "feed_mm_min":   null,
      "rpm": 18000
    }
  ],
  "operations": [ { "kind": "profile", "id": "op1", "depth_mm": null } ],
  "definitions": {
    "d1": { "profiles": [ { "vertices": [ { "x": 1.0 }, { "x": 2.5 } ] } ] }
  }
}
"#;

    fn pos(line: usize, column: usize) -> Option<TextPosition> {
        Some(TextPosition { line, column })
    }

    /// The position of the first occurrence of `needle` in `text`.
    fn find(text: &str, needle: &str) -> Option<TextPosition> {
        let offset = text.find(needle)?;
        let before = &text[..offset];
        let line = before.matches('\n').count() + 1;
        let column = before.rsplit('\n').next()?.chars().count() + 1;
        pos(line, column)
    }

    #[test]
    fn paths_map_to_line_and_column() {
        let map = SourceMap::parse(DOCUMENT).unwrap();
        assert_eq!(map.locate("tools[t1].feed_mm_min"), pos(10, 26));
        assert_eq!(map.locate("operations[op1].depth_mm"), pos(14, 65));
        assert_eq!(map.locate("stock.thickness_mm"), pos(2, 49));
        assert_eq!(map.locate("tools[t1].rpm"), pos(11, 14));
        assert_eq!(
            map.locate("definitions[d1].profiles[0].vertices[1].x"),
            pos(16, 64)
        );
        // The same values by key and index, as a schema error would name them.
        let path = [
            PathSegment::Key("tools".into()),
            PathSegment::Index(1),
            PathSegment::Key("feed_mm_min".into()),
        ];
        assert_eq!(map.position(&path), pos(10, 26));
    }

    #[test]
    fn positions_survive_reformatting() {
        let value: serde_json::Value = serde_json::from_str(DOCUMENT).unwrap();
        let minified = serde_json::to_string(&value).unwrap();
        let tabbed = serde_json::to_string_pretty(&value)
            .unwrap()
            .lines()
            .map(|line| {
                let indent = line.len() - line.trim_start().len();
                format!("{}{}", "\t".repeat(indent / 2), line.trim_start())
            })
            .collect::<Vec<_>>()
            .join("\r\n");
        assert!(tabbed.contains("\t\t\t\"feed_mm_min\": null"), "{tabbed}");
        assert_eq!(minified.lines().count(), 1);
        for text in [DOCUMENT.to_owned(), minified, tabbed] {
            let map = SourceMap::parse(&text).unwrap();
            assert_points_at_null(&text, &map, "tools[t1].feed_mm_min", "feed_mm_min");
            assert_points_at_null(&text, &map, "operations[op1].depth_mm", "depth_mm");
            assert_points_at_null(&text, &map, "stock.thickness_mm", "thickness_mm");
        }
    }

    /// Asserts `field` resolves to the `null` right after `"key":`.
    fn assert_points_at_null(text: &str, map: &SourceMap, field: &str, key: &str) {
        let p = map
            .locate(field)
            .unwrap_or_else(|| panic!("{field} not found in {text}"));
        let line = text.split('\n').nth(p.line - 1).unwrap();
        let before: String = line.chars().take(p.column - 1).collect();
        let rest: String = line.chars().skip(p.column - 1).collect();
        assert!(rest.starts_with("null"), "{field}: {rest}");
        assert!(
            before.trim_end().ends_with(&format!("\"{key}\":")),
            "{field}: {before}"
        );
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        let map = SourceMap::parse("{ \"name\": \"Café ø\", \"x\": 1 }").unwrap();
        assert_eq!(map.locate("x"), pos(1, 26));
    }

    #[test]
    fn escaped_keys_and_ids_are_decoded() {
        let text = r#"{ "tools": [ { "id": "té", "rpm": 5 } ] }"#;
        let map = SourceMap::parse(text).unwrap();
        assert_eq!(map.locate("tools[té].rpm"), find(text, "5 }"));
    }

    #[test]
    fn missing_values_are_none() {
        let map = SourceMap::parse(DOCUMENT).unwrap();
        assert_eq!(map.locate("tools[t9].rpm"), None);
        assert_eq!(map.locate("stock.height_mm"), None);
        assert_eq!(map.locate("stock.width_mm.x"), None);
        assert_eq!(map.locate("tools[t1"), None);
        assert_eq!(map.locate("tools..rpm"), None);
        assert_eq!(map.position(&[PathSegment::Index(0)]), None);
    }

    #[test]
    fn invalid_json_is_an_error_with_position() {
        let err = SourceMap::parse("{\n  \"a\": tru\n}").unwrap_err();
        assert!(matches!(err, LoadError::Json { line: 2, .. }), "{err:?}");
        assert!(SourceMap::parse("{} x").is_err());
        assert!(SourceMap::parse("\"open").is_err());
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let text = "[".repeat(100_000);
        assert!(SourceMap::parse(&text).is_err());
    }
}
