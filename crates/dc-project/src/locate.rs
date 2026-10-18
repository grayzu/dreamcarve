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
