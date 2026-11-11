// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/svg.ts
//   src/import/svg.test.ts

//! The walk (`docs/specs/phase-3/design.md`, "Image prep", "The walk"):
//! one pass over an SVG that says which paths are drawn. The import checks
//! and, later, `image-prep` both read it, so they agree on what is drawn.
//!
//! The paint-intent rule is PureCutCNC's `hasVisiblePaint` as the importer
//! already reads it: a fill or stroke paints when it is not `none`, is not
//! transparent, and neither its own opacity nor any ancestor group's is 0;
//! usvg has folded the colour's alpha, `fill-opacity`, `stroke-opacity` and
//! inherited presentation into [`usvg::Fill`] and [`usvg::Stroke`], so the
//! rule is applied to those. A path is, in this precedence, **hidden**
//! (`visibility` is not `visible`, or the product of its ancestors' opacity
//! is 0), **unpainted** (nothing it paints), or **painted**.
//!
//! usvg drops `display:none` subtrees, text and, from its tree, the
//! `clipPath` and `mask` uses, so those counts come from the raw document
//! (`svg::inspect_root`).

use crate::svg::{self, MM_PER_PX};
use crate::{ImportError, NotConvertible};
use dc_geometry::{FillRule, Point, Rect};
use kurbo::{Affine, Shape};

/// Why an object is not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenReason {
    /// In a `display:none` element.
    DisplayNone,
    /// `visibility` is not `visible`.
    Hidden,
    /// The object's or an ancestor's `opacity` multiplies to 0.
    ZeroOpacity,
    /// Neither filled nor stroked with anything that paints.
    Unpainted,
}

impl HiddenReason {
    /// The reason as the warning table writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DisplayNone => "display_none",
            Self::Hidden => "hidden",
            Self::ZeroOpacity => "zero_opacity",
            Self::Unpainted => "unpainted",
        }
    }

    /// The reason in words, for a message.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::DisplayNone => "display:none",
            Self::Hidden => "visibility is hidden",
            Self::ZeroOpacity => "its opacity is zero",
            Self::Unpainted => "neither filled nor stroked",
        }
    }
}

/// A fill that paints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillPaint {
    /// The path's `fill-rule`.
    pub rule: FillRule,
    /// The fill's opacity, above 0.
    pub opacity: f32,
}

/// A stroke that paints. Kept whole for stroke to area (slice 21d).
#[derive(Debug, Clone)]
pub struct StrokePaint {
    /// The stroke, in usvg's terms: width in user units, caps, joins,
    /// miter limit, dashes and opacity.
    pub stroke: usvg::Stroke,
    /// The stroke's width in millimetres: the user width times the mean of
    /// the transform's two axis scales (px to mm included).
    pub width_mm: f64,
}

/// A path that is drawn.
#[derive(Debug, Clone)]
pub struct PaintedPath {
    /// The id the importer names it by: its SVG id, or `path-N` where N
    /// counts every path of the document in order, from 1.
    pub name: String,
    /// Position among all the document's paths, from 1.
    pub index: usize,
    /// The fill, when it paints.
    pub fill: Option<FillPaint>,
    /// The stroke, when it paints.
    pub stroke: Option<StrokePaint>,
    /// The product of the ancestors' opacities.
    pub ancestor_opacity: f32,
    /// Bounds of the path's own curves (not its stroke), mm, Y-up; `None`
    /// for a path with no points.
    pub bounds_mm: Option<Rect>,
    /// The path in user space, for the stroker.
    pub user_path: usvg::tiny_skia_path::Path,
    /// User space to page millimetres, Y-up.
    pub to_mm: Affine,
    /// The path in millimetres, Y-up.
    pub(crate) geometry: svg::MmPath,
}

/// An object that is not drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenObject {
    /// Why.
    pub reason: HiddenReason,
    /// Its id, else its layer's, else `path-N`.
    pub name: String,
    /// How many shapes it stands for: those in a `display:none` element,
    /// else 1.
    pub shapes: usize,
}

/// A kind of content that cannot become shapes, counted in the raw
/// document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uncountable {
    /// Which kind.
    pub kind: NotConvertible,
    /// How many there are.
    pub count: usize,
    /// The first clip path or mask user's id (or its ancestor's); empty
    /// for text and images.
    pub group: String,
}

/// The result of [`walk`].
#[derive(Debug, Clone)]
pub struct Walk {
    /// Page width, mm.
    pub page_width_mm: f64,
    /// Page height, mm.
    pub page_height_mm: f64,
    /// The drawn paths, in document order.
    pub painted: Vec<PaintedPath>,
    /// Objects that are not drawn: the tree's, in document order, then the
    /// `display:none` elements'.
    pub hidden: Vec<HiddenObject>,
    /// Text, images, clip paths and masks, in that order, only those
    /// present.
    pub not_convertible: Vec<Uncountable>,
}

/// Walks the SVG in `bytes`.
///
/// # Errors
///
/// [`ImportError`] when the bytes are not a readable SVG or the page size
/// is not usable, as for the importer.
pub fn walk(bytes: &[u8]) -> Result<Walk, ImportError> {
    let tree = usvg::Tree::from_data(bytes, &svg::usvg_options())?;
    let root = svg::inspect_root(bytes)?;
    let (page_width_mm, page_height_mm) = svg::page_size_mm(&tree)?;
    let mut state = State {
        px_to_mm: Affine::new([MM_PER_PX, 0.0, 0.0, -MM_PER_PX, 0.0, page_height_mm]),
        index: 0,
        painted: Vec::new(),
        hidden: Vec::new(),
    };
    state.group(tree.root(), 1.0, "");

    let mut hidden = state.hidden;
    hidden.extend(root.display_none.into_iter().map(|d| HiddenObject {
        reason: HiddenReason::DisplayNone,
        name: d.name,
        shapes: d.shapes,
    }));
    let not_convertible = [
        (NotConvertible::Text, root.text_count, String::new()),
        (NotConvertible::Image, root.image_count, String::new()),
        (
            NotConvertible::ClipPath,
            root.clip_paths.count,
            root.clip_paths.first_group,
        ),
        (
            NotConvertible::Mask,
            root.masks.count,
            root.masks.first_group,
        ),
    ]
    .into_iter()
    .filter(|(_, count, _)| *count > 0)
    .map(|(kind, count, group)| Uncountable { kind, count, group })
    .collect();
    Ok(Walk {
        page_width_mm,
        page_height_mm,
        painted: state.painted,
        hidden,
        not_convertible,
    })
}

/// The walk in progress.
struct State {
    px_to_mm: Affine,
    /// Paths seen so far, drawn or not; numbers unnamed paths as the
    /// importer does.
    index: usize,
    painted: Vec<PaintedPath>,
    hidden: Vec<HiddenObject>,
}

impl State {
    /// `opacity` is the product of the opacities of `group`'s ancestors;
    /// `layer` the id of the nearest ancestor that has one.
    fn group(&mut self, group: &usvg::Group, opacity: f32, layer: &str) {
        let opacity = opacity * group.opacity().get();
        let layer = if group.id().is_empty() {
            layer
        } else {
            group.id()
        };
        for child in group.children() {
            match child {
                usvg::Node::Group(g) => self.group(g, opacity, layer),
                usvg::Node::Path(p) => self.path(p, opacity, layer),
                // Counted from the raw document.
                usvg::Node::Image(_) | usvg::Node::Text(_) => {}
            }
        }
    }

    fn path(&mut self, path: &usvg::Path, ancestor_opacity: f32, layer: &str) {
        self.index += 1;
        let name = if path.id().is_empty() {
            format!("path-{}", self.index)
        } else {
            path.id().to_owned()
        };

        let fill = path
            .fill()
            .filter(|fill| fill.opacity().get() > 0.0)
            .map(|fill| FillPaint {
                rule: match fill.rule() {
                    usvg::FillRule::NonZero => FillRule::NonZero,
                    usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                },
                opacity: fill.opacity().get(),
            });
        let to_mm = self.px_to_mm * svg::affine(path.abs_transform());
        let stroke = path
            .stroke()
            .filter(|stroke| stroke.opacity().get() > 0.0 && stroke.width().get() > 0.0)
            .map(|stroke| StrokePaint {
                stroke: stroke.clone(),
                width_mm: f64::from(stroke.width().get()) * mean_axis_scale(to_mm),
            });

        // usvg marks a path with neither fill nor stroke invisible, so the
        // paint is looked at before its visibility.
        let reason = if path.fill().is_none() && path.stroke().is_none() {
            Some(HiddenReason::Unpainted)
        } else if !path.is_visible() {
            Some(HiddenReason::Hidden)
        } else if ancestor_opacity <= 0.0 {
            Some(HiddenReason::ZeroOpacity)
        } else if fill.is_none() && stroke.is_none() {
            Some(HiddenReason::Unpainted)
        } else {
            None
        };
        if let Some(reason) = reason {
            let name = if path.id().is_empty() && !layer.is_empty() {
                layer.to_owned()
            } else {
                name
            };
            self.hidden.push(HiddenObject {
                reason,
                name,
                shapes: 1,
            });
            return;
        }

        let geometry = svg::to_mm_path(path.data(), to_mm);
        let bounds_mm = (!geometry.bez.elements().is_empty()).then(|| {
            let b = geometry.bez.bounding_box();
            Rect {
                min: Point::new(b.x0, b.y0),
                max: Point::new(b.x1, b.y1),
            }
        });
        self.painted.push(PaintedPath {
            name,
            index: self.index,
            fill,
            stroke,
            ancestor_opacity,
            bounds_mm,
            user_path: path.data().clone(),
            to_mm,
            geometry,
        });
    }
}

/// The mean of the two axis scales of `to_mm`, the factor a stroke's width
/// grows by along its normal.
pub(crate) fn mean_axis_scale(to_mm: Affine) -> f64 {
    let [a, b, c, d, _, _] = to_mm.as_coeffs();
    (a.hypot(b) + c.hypot(d)) / 2.0
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "tests"
    )]

    use super::*;

    fn walked(markup: &str) -> Walk {
        let svg = markup.replacen("<svg", r#"<svg xmlns="http://www.w3.org/2000/svg""#, 1);
        walk(svg.as_bytes()).expect("walks")
    }

    const PAGE: &str = r#"width="100mm" height="100mm" viewBox="0 0 100 100""#;

    fn hidden_reasons(w: &Walk) -> Vec<(HiddenReason, usize)> {
        w.hidden.iter().map(|h| (h.reason, h.shapes)).collect()
    }

    #[test]
    fn hidden_unpainted_and_zero_opacity_are_counted() {
        let w = walked(&format!(
            r#"<svg {PAGE}>
              <g id="layer" style="display:none"><rect id="a" width="5" height="5"/><rect id="b" x="9" width="5" height="5"/></g>
              <rect id="vis" width="5" height="5" visibility="hidden"/>
              <g id="faded" opacity="0"><rect id="c" width="5" height="5"/></g>
              <rect id="none" width="5" height="5" fill="none"/>
              <rect id="fill-clear" width="5" height="5" fill-opacity="0"/>
              <path id="stroke-clear" d="M0,0 L9,9" fill="none" stroke="red" stroke-opacity="0"/>
              <path id="no-width" d="M0,0 L9,9" fill="none" stroke="red" stroke-width="0"/>
              <rect id="shown" width="5" height="5"/>
            </svg>"#
        ));
        assert_eq!(w.painted.len(), 1, "{:?}", w.hidden);
        assert_eq!(w.painted[0].name, "shown");
        let by_reason = |reason| {
            w.hidden
                .iter()
                .filter(|h| h.reason == reason)
                .map(|h| (h.name.as_str(), h.shapes))
                .collect::<Vec<_>>()
        };
        // The layer is named, once, with the shapes it holds.
        assert_eq!(by_reason(HiddenReason::DisplayNone), [("layer", 2)]);
        assert_eq!(by_reason(HiddenReason::Hidden), [("vis", 1)]);
        assert_eq!(by_reason(HiddenReason::ZeroOpacity), [("c", 1)]);
        assert_eq!(
            by_reason(HiddenReason::Unpainted),
            [
                ("none", 1),
                ("fill-clear", 1),
                ("stroke-clear", 1),
                ("no-width", 1)
            ]
        );
    }

    #[test]
    fn painted_paths_keep_their_fill_rule() {
        let w = walked(&format!(
            r#"<svg {PAGE}>
              <path id="odd" fill-rule="evenodd" d="M0,0 H10 V10 H0 Z M2,2 H8 V8 H2 Z"/>
              <path id="non" d="M0,0 H10 V10 H0 Z"/>
              <path id="line" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" d="M0,0 L9,9"/>
            </svg>"#
        ));
        let rule = |name: &str| {
            w.painted
                .iter()
                .find(|p| p.name == name)
                .and_then(|p| p.fill)
                .map(|f| f.rule)
        };
        assert_eq!(rule("odd"), Some(FillRule::EvenOdd));
        assert_eq!(rule("non"), Some(FillRule::NonZero));
        assert_eq!(rule("line"), None);
        let line = w.painted.iter().find(|p| p.name == "line").unwrap();
        let stroke = line.stroke.as_ref().expect("stroked");
        assert!((stroke.width_mm - 2.0).abs() < 1e-4, "{}", stroke.width_mm);
        assert_eq!(stroke.stroke.linecap(), usvg::LineCap::Round);
    }

    #[test]
    fn text_images_clip_paths_and_masks_are_counted_with_their_fix() {
        let w = walked(&format!(
            r#"<svg {PAGE} xmlns:xlink="http://www.w3.org/1999/xlink">
              <defs><clipPath id="c"><rect width="5" height="5"/></clipPath><mask id="m"><rect width="5" height="5" fill="white"/></mask></defs>
              <text x="1" y="9">a</text><text x="1" y="19">b</text>
              <image width="5" height="5" xlink:href="p.png"/>
              <g id="outer"><g id="clipped" clip-path="url(#c)"><rect width="9" height="9"/></g></g>
              <g id="masked" mask="url(#m)"><rect x="20" width="9" height="9"/></g>
            </svg>"#
        ));
        let found: Vec<_> = w
            .not_convertible
            .iter()
            .map(|u| (u.kind, u.count, u.group.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (NotConvertible::Text, 2, ""),
                (NotConvertible::Image, 1, ""),
                (NotConvertible::ClipPath, 1, "clipped"),
                (NotConvertible::Mask, 1, "masked"),
            ]
        );
    }

    /// Every case of `svg::tests::every_paint_variant_imports_exactly_one_shape`
    /// (upstream `svg.test.ts`, paint intent): the walk reads each as the
    /// importer's paint intent does. The importer imports geometry whatever
    /// the paint; the walk says whether it is drawn.
    #[test]
    fn same_paint_rule_as_the_importer() {
        use HiddenReason::{Unpainted, ZeroOpacity};
        let drawn = None;
        let cases: [(&str, Option<HiddenReason>); 31] = [
            (r#"<svg><rect width="10" height="10"/></svg>"#, drawn),
            (
                r#"<svg><g fill="red"><rect width="10" height="10"/></g></svg>"#,
                drawn,
            ),
            (
                r#"<svg><g stroke="blue"><line x1="0" y1="0" x2="10" y2="10"/></g></svg>"#,
                drawn,
            ),
            (
                r#"<svg><circle cx="5" cy="5" r="3" fill="none" stroke="blue"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><path d="M0,0 L10,10" fill="none" stroke="red"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><rect x="0" y="0" width="10" height="10" fill="green" stroke="black"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><rect width="10" height="10" fill="blue" style="fill:red"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><rect width="10" height="10" fill="blue" style="fill:none"/></svg>"#,
                Some(Unpainted),
            ),
            (
                r#"<svg><rect width="10" height="10" stroke="blue" style="stroke:red"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><circle cx="5" cy="5" r="3" fill="transparent"/></svg>"#,
                Some(Unpainted),
            ),
            // usvg does not parse a few of upstream's spellings of a clear
            // colour (a mis-cased `None`, the CSS4 slash alpha, a percent
            // alpha): the paint is invalid and falls back to black, so those
            // are drawn here, and the importer parses with usvg too.
            // usvg reads the mis-cased keyword as an invalid paint, so the fill
            // falls back to black; PureCutCNC lower-cases it to `none`. The
            // walk follows usvg, which is what the importer parses with.
            (
                r#"<svg><rect width="10" height="10" fill="None"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><g opacity="0"><rect width="10" height="10" fill="red"/></g></svg>"#,
                Some(ZeroOpacity),
            ),
            (
                r#"<svg><g opacity="0"><g><rect width="10" height="10" fill="red" stroke="blue"/></g></g></svg>"#,
                Some(ZeroOpacity),
            ),
            (
                r#"<svg><rect width="10" height="10" fill="red" opacity="0"/></svg>"#,
                Some(ZeroOpacity),
            ),
            (
                r#"<svg><rect width="10" height="10" fill="red" fill-opacity="0"/></svg>"#,
                Some(Unpainted),
            ),
            (
                r#"<svg><rect width="10" height="10" fill="red" style="fill-opacity:0"/></svg>"#,
                Some(Unpainted),
            ),
            (
                r#"<svg><circle cx="5" cy="5" r="3" fill="none" stroke="blue" stroke-opacity="0"/></svg>"#,
                Some(Unpainted),
            ),
            (
                r#"<svg><g fill-opacity="0"><rect width="10" height="10" fill="red"/></g></svg>"#,
                Some(Unpainted),
            ),
            (
                r#"<svg><g stroke-opacity="0"><line x1="0" y1="0" x2="10" y2="10" stroke="blue"/></g></svg>"#,
                drawn,
            ),
            (
                r#"<svg><g fill-opacity="0"><rect width="10" height="10" fill="red" fill-opacity="1"/></g></svg>"#,
                drawn,
            ),
            (
                r#"<svg><rect width="10" height="10" fill="rgba(255, 0, 0, 0)"/></svg>"#,
                Some(Unpainted),
            ),
            (
                r#"<svg><rect width="10" height="10" fill="rgba(255 0 0 / 0)"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><rect width="10" height="10" fill="hsla(0, 100%, 50%, 0)"/></svg>"#,
                Some(Unpainted),
            ),
            (
                r#"<svg><rect width="10" height="10" fill="hsla(0 100% 50% / 0)"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><rect width="10" height="10" fill="rgba(255, 0, 0, 0%)"/></svg>"#,
                drawn,
            ),
            (
                r##"<svg><rect width="10" height="10" fill="#ff000000"/></svg>"##,
                Some(Unpainted),
            ),
            (
                r##"<svg><rect width="10" height="10" fill="#ff0000ff"/></svg>"##,
                drawn,
            ),
            (
                r#"<svg><rect width="10" height="10" fill="rgba(255, 0, 0, 0.5)"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><rect width="10" height="10" fill="hsla(0, 100%, 50%, 0.3)"/></svg>"#,
                drawn,
            ),
            (
                r#"<svg><g opacity="0.5"><g opacity="0.5"><rect width="10" height="10" fill="red"/></g></g></svg>"#,
                drawn,
            ),
            (
                r#"<svg><rect width="10" height="10" fill="red" fill-opacity="0" stroke="blue"/></svg>"#,
                drawn,
            ),
        ];
        let mut wrong = Vec::new();
        for (markup, expected) in cases {
            let w = walked(markup);
            let got = match (w.painted.len(), w.hidden.as_slice()) {
                (1, []) => None,
                (0, [h]) => Some(h.reason),
                _ => panic!(
                    "{markup}: {} painted, hidden {:?}",
                    w.painted.len(),
                    w.hidden
                ),
            };
            if got != expected {
                wrong.push(format!("{markup}: got {got:?}, expected {expected:?}"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn transforms_and_use_are_applied() {
        let w = walked(&format!(
            r##"<svg {PAGE} xmlns:xlink="http://www.w3.org/1999/xlink">
              <defs><rect id="sq" width="10" height="10"/></defs>
              <g transform="translate(30 20) scale(2)"><use xlink:href="#sq" x="1"/></g>
              <use xlink:href="#sq" transform="translate(50 60)"/>
            </svg>"##
        ));
        assert_eq!(w.painted.len(), 2);
        // translate(30,20) · scale(2): x 32..52, SVG y 20..40, flipped 60..80.
        let a = w.painted[0].bounds_mm.expect("bounds");
        assert!(
            (a.min.x - 32.0).abs() < 1e-3 && (a.max.x - 52.0).abs() < 1e-3,
            "{a:?}"
        );
        assert!(
            (a.min.y - 60.0).abs() < 1e-3 && (a.max.y - 80.0).abs() < 1e-3,
            "{a:?}"
        );
        let b = w.painted[1].bounds_mm.expect("bounds");
        assert!(
            (b.min.x - 50.0).abs() < 1e-3 && (b.max.y - 40.0).abs() < 1e-3,
            "{b:?}"
        );
        // Used twice, named by position as the importer names them.
        assert_eq!(w.painted[0].name, "path-1");
        assert_eq!(w.painted[1].name, "path-2");
        assert!((w.page_height_mm - 100.0).abs() < 1e-3);
        let _ = hidden_reasons(&w);
    }
}
