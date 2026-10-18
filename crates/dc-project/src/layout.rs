// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The layout of a saved `.dcv` file (#76): serde_json's pretty printing,
//! except that each vertex object is written on one line,
//! `{ "x": 137.684, "y": 88.535, "bulge": 0.0 }`, instead of five. A drawing
//! is mostly vertices, so this keeps the file short enough to edit by hand.
//!
//! Only whitespace differs from [`serde_json::to_string_pretty`]: the keys,
//! their order and every number's digits are the same, so any JSON reader
//! reads the same document, and the output is byte-for-byte deterministic.

use serde_json::ser::Formatter;
use std::io::{self, Write};

/// The indent of one nesting level, as serde_json's pretty printer.
const INDENT: &[u8] = b"  ";

/// The key whose array holds vertex objects (`Polyline::vertices`).
const VERTICES_KEY: &str = "vertices";

/// How one open object or array is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// One member or element per line, indented.
    Pretty {
        /// Whether the elements of this array are written on one line each.
        compact_elements: bool,
    },
    /// The whole value on one line.
    Compact {
        /// Whether a member or element has been written.
        has_value: bool,
    },
}

/// A [`Formatter`] for `.dcv` text; see the module docs.
#[derive(Debug, Default)]
pub(crate) struct DcvFormatter {
    /// Open objects and arrays, innermost last.
    frames: Vec<Frame>,
    /// Pretty frames open, which sets the indent.
    indent: usize,
    /// Whether the current pretty container has written a value yet.
    has_value: bool,
    /// The key being written, while inside it.
    key: Option<String>,
    /// The last complete key, until a container value takes it.
    last_key: Option<String>,
}

impl DcvFormatter {
    /// Opens a container: compact inside a compact one or as an element of
    /// a vertices array, pretty otherwise.
    fn open<W: ?Sized + Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        let key = self.last_key.take();
        let frame = match self.frames.last() {
            Some(Frame::Compact { .. })
            | Some(Frame::Pretty {
                compact_elements: true,
            }) => Frame::Compact { has_value: false },
            _ => Frame::Pretty {
                compact_elements: bracket == b"[" && key.as_deref() == Some(VERTICES_KEY),
            },
        };
        if let Frame::Pretty { .. } = frame {
            self.indent += 1;
            self.has_value = false;
        }
        self.frames.push(frame);
        writer.write_all(bracket)
    }

    /// Closes a container. An empty pretty one stays `[]` or `{}`.
    fn close<W: ?Sized + Write>(&mut self, writer: &mut W, bracket: &[u8]) -> io::Result<()> {
        match self.frames.pop() {
            Some(Frame::Compact { has_value }) => {
                if has_value && bracket == b"}" {
                    writer.write_all(b" ")?;
                }
            }
            _ => {
                self.indent = self.indent.saturating_sub(1);
                if self.has_value {
                    writer.write_all(b"\n")?;
                    self.write_indent(writer)?;
                }
            }
        }
        // The enclosing container now holds a value.
        self.has_value = true;
        writer.write_all(bracket)
    }

    /// Starts a member or element: a separator, then on a new line in a
    /// pretty container.
    fn separate<W: ?Sized + Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if let Some(Frame::Compact { has_value }) = self.frames.last_mut() {
            *has_value = true;
            return writer.write_all(if first { b" " } else { b", " });
        }
        writer.write_all(if first { b"\n" } else { b",\n" })?;
        self.write_indent(writer)
    }

    fn write_indent<W: ?Sized + Write>(&self, writer: &mut W) -> io::Result<()> {
        for _ in 0..self.indent {
            writer.write_all(INDENT)?;
        }
        Ok(())
    }
}

impl Formatter for DcvFormatter {
    fn begin_array<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"[")
    }

    fn end_array<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"]")
    }

    fn begin_array_value<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.last_key = None;
        if let Some(Frame::Compact { has_value }) = self.frames.last_mut() {
            // An array has no space after `[`, as `[1.0, 2.0]`.
            *has_value = true;
            return if first {
                Ok(())
            } else {
                writer.write_all(b", ")
            };
        }
        self.separate(writer, first)
    }

    fn end_array_value<W: ?Sized + Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    fn begin_object<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.open(writer, b"{")
    }

    fn end_object<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.close(writer, b"}")
    }

    fn begin_object_key<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.key = Some(String::new());
        self.separate(writer, first)
    }

    fn end_object_key<W: ?Sized + Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.last_key = self.key.take();
        Ok(())
    }

    fn begin_object_value<W: ?Sized + Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }

    fn end_object_value<W: ?Sized + Write>(&mut self, _writer: &mut W) -> io::Result<()> {
        self.has_value = true;
        Ok(())
    }

    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        if let Some(key) = &mut self.key {
            key.push_str(fragment);
        }
        writer.write_all(fragment.as_bytes())
    }

    fn write_char_escape<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        char_escape: serde_json::ser::CharEscape,
    ) -> io::Result<()> {
        // A key with an escape is never `vertices`; spoil it so it cannot
        // match by accident.
        if let Some(key) = &mut self.key {
            key.push('\\');
        }
        serde_json::ser::CompactFormatter.write_char_escape(writer, char_escape)
    }
}
