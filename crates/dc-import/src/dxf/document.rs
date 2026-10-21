// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/dxf.ts (parseDxfSourceUnits, extractEntities, extractBlocks,
//   pairNumber, pairString, repeatedPoints)

//! The parts of a DXF file the importer reads, from its groups
//! (`docs/specs/phase-2/design.md`, "Algorithms → DXF import", step 1): the
//! `$INSUNITS`, `$ACADVER` and `$DWGCODEPAGE` header variables, the block
//! definitions, and the entities of
//! the ENTITIES section, each entity being its type and its own groups,
//! with a POLYLINE's VERTEX rows attached to it. Every entity is kept,
//! whatever its type, so the importer sees (and counts) each one it does
//! not read. Changed from upstream: the reference's section, block and
//! sequence structure (DXF Reference, "DXF File Structure", "BLOCKS
//! Section", "POLYLINE", "SEQEND") is followed for both ASCII and binary
//! files, and SEQEND also ends an INSERT's attributes.

use dc_geometry::Point;

use super::groups::Group;

/// A DXF file's header units, blocks and model/paper space entities.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct Document {
    /// `$INSUNITS` (header variable, group 70), when present.
    pub insunits: Option<i64>,
    /// `$ACADVER` (group 1), the AutoCAD version the file is saved as:
    /// `AC1015` for R2000, `AC1021` for R2007.
    pub acadver: Option<String>,
    /// `$DWGCODEPAGE` (group 3), the code page text is written in before
    /// R2007: `ANSI_1252`, `ANSI_1251`, ...
    pub dwgcodepage: Option<String>,
    /// Block definitions, in file order.
    pub blocks: Vec<Block>,
    /// The ENTITIES section, in file order.
    pub entities: Vec<Entity>,
}

/// A block definition (BLOCKS section: `0 BLOCK` ... `0 ENDBLK`).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Block {
    /// Group 2 (or 3 when 2 is absent).
    pub name: String,
    /// The base point, groups 10/20.
    pub base: Point,
    pub entities: Vec<Entity>,
}

/// One entity: its type name (group 0, upper case) and the groups after it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Entity {
    pub kind: String,
    groups: Vec<Group>,
    /// A POLYLINE's VERTEX entities, in order; empty for any other type.
    pub vertices: Vec<Entity>,
}

impl Entity {
    /// The entity's own groups, in file order.
    pub(super) fn groups(&self) -> &[Group] {
        &self.groups
    }

    fn first(&self, code: i32) -> Option<&Group> {
        self.groups.iter().find(|g| g.code == code)
    }

    /// The first value of a real or integer code.
    pub(super) fn real(&self, code: i32) -> Option<f64> {
        self.first(code).and_then(Group::real)
    }

    /// The first value of a real code, or `default` when absent.
    pub(super) fn real_or(&self, code: i32, default: f64) -> f64 {
        self.real(code).unwrap_or(default)
    }

    /// The first value of an integer code, or 0 when absent.
    pub(super) fn int(&self, code: i32) -> i64 {
        self.first(code).and_then(Group::int).unwrap_or(0)
    }

    /// The first value of a string code.
    pub(super) fn text(&self, code: i32) -> Option<&str> {
        self.first(code).and_then(Group::text)
    }

    /// Every value of a repeated real code, in order (SPLINE knots 40,
    /// weights 41).
    pub(super) fn reals(&self, code: i32) -> Vec<f64> {
        self.groups
            .iter()
            .filter(|g| g.code == code)
            .filter_map(Group::real)
            .collect()
    }

    /// The point in groups `x_code` and `x_code + 10`; absent values are 0.
    pub(super) fn point(&self, x_code: i32) -> Point {
        Point::new(self.real_or(x_code, 0.0), self.real_or(x_code + 10, 0.0))
    }

    /// Every point of a repeated point code: each `x_code` group starts a
    /// point and the `x_code + 10` group after it is its Y (SPLINE control
    /// points 10/20, fit points 11/21).
    pub(super) fn points(&self, x_code: i32) -> Vec<Point> {
        let mut points: Vec<Point> = Vec::new();
        for g in &self.groups {
            if g.code == x_code {
                points.push(Point::new(g.real().unwrap_or(0.0), 0.0));
            } else if g.code == x_code + 10
                && let Some(p) = points.last_mut()
            {
                p.y = g.real().unwrap_or(0.0);
            }
        }
        points
    }

    /// The layer, group 8; `0` when absent or empty.
    pub(super) fn layer(&self) -> &str {
        match self.text(8) {
            Some(layer) if !layer.is_empty() => layer,
            _ => "0",
        }
    }

    /// Whether group 67 puts the entity in paper space.
    pub(super) fn in_paper_space(&self) -> bool {
        self.int(67) == 1
    }

    /// The extrusion direction, groups 210/220/230; (0, 0, 1) when absent.
    pub(super) fn extrusion(&self) -> [f64; 3] {
        [
            self.real_or(210, 0.0),
            self.real_or(220, 0.0),
            self.real_or(230, 1.0),
        ]
    }
}

/// The document in `groups`.
pub(super) fn read(groups: &[Group]) -> Document {
    let mut document = Document::default();
    let mut rest = groups;
    while let Some(start) = rest.iter().position(|g| g.is_marker("SECTION")) {
        let after = rest.get(start + 1..).unwrap_or_default();
        let end = after
            .iter()
            .position(|g| g.is_marker("ENDSEC") || g.is_marker("EOF"))
            .unwrap_or(after.len());
        let (section, tail) = after.split_at(end);
        rest = tail;
        let Some((name, body)) = section.split_first() else {
            continue;
        };
        if name.code != 2 {
            continue;
        }
        match name.text().map(str::to_ascii_uppercase).as_deref() {
            Some("HEADER") => {
                document.insunits = header(body, "$INSUNITS", 70).and_then(Group::int);
                document.acadver = header_text(body, "$ACADVER", 1);
                document.dwgcodepage = header_text(body, "$DWGCODEPAGE", 3);
            }
            Some("BLOCKS") => document.blocks = blocks(body),
            Some("ENTITIES") => document.entities = assemble(records(body)),
            _ => {}
        }
    }
    document
}

impl Document {
    /// Whether the file's text is UTF-8: `$ACADVER` is R2007 (`AC1021`) or
    /// later. Earlier files, and files that do not say, are written in
    /// `$DWGCODEPAGE`.
    pub(super) fn text_is_utf8(&self) -> bool {
        self.acadver
            .as_deref()
            .and_then(|v| v.trim().get(2..)?.parse::<u32>().ok())
            .is_some_and(|version| version >= 1021)
    }
}

/// The group `code` under the header variable `name` (group 9).
fn header<'a>(body: &'a [Group], name: &str, code: i32) -> Option<&'a Group> {
    let at = body
        .iter()
        .position(|g| g.code == 9 && g.text() == Some(name))?;
    body.get(at + 1..)?
        .iter()
        .take_while(|g| g.code != 9)
        .find(|g| g.code == code)
}

/// The text of `code` under the header variable `name`, when not empty.
fn header_text(body: &[Group], name: &str, code: i32) -> Option<String> {
    header(body, name, code)
        .and_then(Group::text)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

/// An entity's type name (group 0, upper case) and the groups after it.
type Record = (String, Vec<Group>);

/// The groups split at each group 0.
fn records(body: &[Group]) -> Vec<Record> {
    let mut out: Vec<Record> = Vec::new();
    for g in body {
        if g.code == 0 {
            out.push((
                g.text().unwrap_or_default().to_ascii_uppercase(),
                Vec::new(),
            ));
        } else if let Some((_, groups)) = out.last_mut() {
            groups.push(g.clone());
        }
    }
    out
}

/// Records to entities: VERTEX rows after a POLYLINE belong to it, and
/// SEQEND, which ends a POLYLINE's vertices or an INSERT's attributes, is
/// dropped. Attributes (ATTRIB) stay entities of their own, so they are
/// counted like other text.
fn assemble(records: Vec<Record>) -> Vec<Entity> {
    let mut out: Vec<Entity> = Vec::new();
    let mut in_polyline = false;
    for (kind, groups) in records {
        let entity = Entity {
            kind,
            groups,
            vertices: Vec::new(),
        };
        match entity.kind.as_str() {
            "VERTEX" if in_polyline => {
                if let Some(polyline) = out.last_mut() {
                    polyline.vertices.push(entity);
                }
            }
            "SEQEND" => in_polyline = false,
            _ => {
                in_polyline = entity.kind == "POLYLINE";
                out.push(entity);
            }
        }
    }
    out
}

/// The BLOCKS section: each `BLOCK` record opens a block, whose entities
/// run to its `ENDBLK`.
fn blocks(body: &[Group]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut current: Option<(Block, Vec<Record>)> = None;
    for (kind, groups) in records(body) {
        match kind.as_str() {
            "BLOCK" => {
                let header = Entity {
                    kind,
                    groups,
                    vertices: Vec::new(),
                };
                let name = header
                    .text(2)
                    .or_else(|| header.text(3))
                    .unwrap_or_default()
                    .to_owned();
                let block = Block {
                    name,
                    base: header.point(10),
                    entities: Vec::new(),
                };
                if let Some((block, records)) = current.replace((block, Vec::new())) {
                    out.push(finish(block, records));
                }
            }
            "ENDBLK" => {
                if let Some((block, records)) = current.take() {
                    out.push(finish(block, records));
                }
            }
            _ => {
                if let Some((_, records)) = current.as_mut() {
                    records.push((kind, groups));
                }
            }
        }
    }
    if let Some((block, records)) = current {
        out.push(finish(block, records));
    }
    out
}

fn finish(mut block: Block, records: Vec<Record>) -> Block {
    block.entities = assemble(records);
    block
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, reason = "tests")]

    use super::*;
    use crate::dxf::code_page::CodePage;
    use crate::dxf::groups;

    fn doc(text: &str) -> Document {
        read(
            &groups::read(text.as_bytes(), CodePage::WINDOWS_1252)
                .unwrap()
                .groups,
        )
    }

    #[test]
    fn sections_blocks_and_polyline_vertices() {
        let d = doc(
            "0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1015\n9\n$INSUNITS\n70\n6\n0\nENDSEC\n\
            0\nSECTION\n2\nTABLES\n0\nLINE\n0\nENDSEC\n\
            0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n1.0\n20\n2.0\n0\nCIRCLE\n40\n1.0\n0\nENDBLK\n0\nENDSEC\n\
            0\nSECTION\n2\nENTITIES\n0\nPOLYLINE\n66\n1\n0\nVERTEX\n10\n1.0\n0\nVERTEX\n10\n2.0\n0\nSEQEND\n\
            0\nVERTEX\n0\nhatch\n8\nh\n0\nENDSEC\n0\nEOF\n",
        );
        assert_eq!(d.insunits, Some(6));
        assert_eq!(d.acadver.as_deref(), Some("AC1015"));
        assert_eq!(d.dwgcodepage, None);
        assert!(!d.text_is_utf8());
        assert_eq!(d.blocks.len(), 1);
        assert_eq!(d.blocks[0].name, "B");
        assert_eq!(d.blocks[0].base, Point::new(1.0, 2.0));
        assert_eq!(d.blocks[0].entities[0].kind, "CIRCLE");
        let kinds: Vec<&str> = d.entities.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["POLYLINE", "VERTEX", "HATCH"]);
        assert_eq!(d.entities[0].vertices.len(), 2);
        assert_eq!(d.entities[0].vertices[1].point(10), Point::new(2.0, 0.0));
        assert_eq!(d.entities[2].layer(), "h");
        assert_eq!(d.entities[1].layer(), "0");
    }

    #[test]
    fn insunits_is_read_only_under_its_variable() {
        let d = doc("0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n9\n$OTHER\n70\n4\n0\nENDSEC\n");
        assert_eq!(d.insunits, None);
    }

    #[test]
    fn repeated_points_pair_x_with_the_following_y() {
        let d = doc(
            "0\nSECTION\n2\nENTITIES\n0\nSPLINE\n10\n1.0\n20\n2.0\n40\n0.0\n10\n3.0\n20\n4.0\n40\n1.0\n0\nENDSEC\n",
        );
        let spline = &d.entities[0];
        assert_eq!(
            spline.points(10),
            [Point::new(1.0, 2.0), Point::new(3.0, 4.0)]
        );
        assert_eq!(spline.reals(40), [0.0, 1.0]);
        assert!(
            spline
                .extrusion()
                .iter()
                .zip([0.0, 0.0, 1.0])
                .all(|(a, b)| (a - b).abs() < 1e-12)
        );
    }

    #[test]
    fn version_says_whether_text_is_utf8() {
        let header = |vars: &str| doc(&format!("0\nSECTION\n2\nHEADER\n{vars}0\nENDSEC\n"));
        let d = header("9\n$ACADVER\n1\nAC1021\n9\n$DWGCODEPAGE\n3\nANSI_1251\n");
        assert!(d.text_is_utf8());
        assert_eq!(d.dwgcodepage.as_deref(), Some("ANSI_1251"));
        assert!(header("9\n$ACADVER\n1\nAC1032\n").text_is_utf8());
        assert!(!header("9\n$ACADVER\n1\nAC1018\n").text_is_utf8());
        assert!(!header("9\n$ACADVER\n1\nAC1009\n").text_is_utf8());
        assert!(!header("9\n$DWGCODEPAGE\n3\n\n").text_is_utf8());
        assert_eq!(header("9\n$DWGCODEPAGE\n3\n\n").dwgcodepage, None);
    }
}
