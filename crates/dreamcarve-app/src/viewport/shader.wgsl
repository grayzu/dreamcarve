// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

// The Viewport's line shader (docs/specs/phase-4/design.md, "Architecture
// -> The Viewport renderer"). Each instance is one segment in project
// millimetres; the vertex shader expands it into a quad of its style's
// width in screen pixels, plus a pixel of feather, so a pan or a zoom
// changes only the camera uniform. The fragment shader measures the
// distance to the segment (a capsule: overlapping round caps make the
// joins), anti-aliases its edge over one pixel, and cuts dashes by the
// distance along the outline in screen pixels, so neither widths nor
// dashes scale with zoom. A ring style draws a circle about the segment's
// start instead, its radius the segment's `along_mm` but never under the
// style's least radius in pixels (start points, drill holes, plunges).
//
// The style word (renderer.rs, `StyleWord`) carries, besides the style
// row, a toolpath's operation colour, its depth step and its stale flag:
// the colour is the operation's, or with `by_depth` set and a depth step
// present, the ramp texture's texel at that step; a stale segment draws
// at 35 % alpha. A shape behind the marked operation's targets sets the
// faded bit and draws at 40 % alpha.
//
// Project space is Y-up and so is clip space: the transform below scales
// about the camera's centre and never flips. The one Y flip of the app is
// `dc_app::viewport::camera::Camera::to_screen`, for the pointer and the
// painter; nothing here goes through Y-down.

struct Camera {
    // The project point at the pane's centre, mm.
    centre_mm: vec2<f32>,
    // The pane's size, physical pixels.
    viewport_px: vec2<f32>,
    // Physical pixels per millimetre.
    px_per_mm: f32,
    // Physical pixels per logical pixel (egui's point).
    pixels_per_point: f32,
    // How strongly the grid's minor lines draw, 0 to 1.
    minor_alpha: f32,
    // 1 to colour cutting moves by depth, 0 by operation.
    by_depth: f32,
};

struct Style {
    // Premultiplied, in the framebuffer's colour space.
    colour: vec4<f32>,
    width_pt: f32,
    // A dash's length, or a ring's least radius.
    dash_on_pt: f32,
    dash_off_pt: f32,
    flags: u32,
};

struct Segment {
    a_mm: vec2<f32>,
    b_mm: vec2<f32>,
    // How far along its outline the segment starts, mm, for the dashes;
    // a ring's radius, mm.
    along_mm: f32,
    style: u32,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<storage, read> styles: array<Style>;
@group(0) @binding(2) var ramp: texture_2d<f32>;
@group(1) @binding(0) var<storage, read> segments: array<Segment>;

const FEATHER_PX: f32 = 1.0;
const DASHED: u32 = 1u;
const FADES: u32 = 2u;
const RING: u32 = 4u;
const STALE_ALPHA: f32 = 0.35;
const FADED_ALPHA: f32 = 0.4;
// The first operation colour row; renderer.rs writes the style count here.
const OP_BASE: u32 = 0u;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    // Along the segment from its start, and across it, physical pixels;
    // for a ring, the offset from its centre.
    @location(0) local_px: vec2<f32>,
    // The segment's length, or a ring's radius, physical pixels.
    @location(1) @interpolate(flat) length_px: f32,
    @location(2) @interpolate(flat) along_px: f32,
    @location(3) @interpolate(flat) style: u32,
};

// A project point, mm, as physical pixels from the pane's centre (Y-up).
fn to_px(p: vec2<f32>) -> vec2<f32> {
    return (p - camera.centre_mm) * camera.px_per_mm;
}

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VertexOut {
    let seg = segments[instance];
    let style = styles[seg.style & 0xFFu];
    let ring = (style.flags & RING) != 0u;
    let a = to_px(seg.a_mm);
    var b = to_px(seg.b_mm);
    var radius = 0.0;
    if (ring) {
        b = a;
        radius = max(seg.along_mm * camera.px_per_mm, style.dash_on_pt * camera.pixels_per_point);
    }
    let d = b - a;
    let len = length(d);
    var dir = vec2<f32>(1.0, 0.0);
    if (len > 1e-6) {
        dir = d / len;
    }
    let normal = vec2<f32>(-dir.y, dir.x);
    let half = style.width_pt * camera.pixels_per_point * 0.5 + FEATHER_PX + radius;
    // Two triangles: (start, right) (end, right) (start, left), then
    // (start, left) (end, right) (end, left).
    var ends = array<f32, 6>(0.0, 1.0, 0.0, 0.0, 1.0, 1.0);
    var sides = array<f32, 6>(-1.0, -1.0, 1.0, 1.0, -1.0, 1.0);
    let along = mix(-half, len + half, ends[vertex]);
    let across = sides[vertex] * half;
    let p = a + dir * along + normal * across;

    var out: VertexOut;
    out.position = vec4<f32>(p * 2.0 / camera.viewport_px, 0.0, 1.0);
    out.local_px = vec2<f32>(along, across);
    out.length_px = select(len, radius, ring);
    out.along_px = seg.along_mm * camera.px_per_mm;
    out.style = seg.style;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let word = in.style;
    let style = styles[word & 0xFFu];
    let ppp = camera.pixels_per_point;
    let half = style.width_pt * ppp * 0.5;
    var coverage = 0.0;
    if ((style.flags & RING) != 0u) {
        let distance = abs(length(in.local_px) - in.length_px);
        coverage = clamp(half + 0.5 - distance, 0.0, 1.0);
    } else {
        let t = clamp(in.local_px.x, 0.0, in.length_px);
        let distance = length(vec2<f32>(in.local_px.x - t, in.local_px.y));
        coverage = clamp(half + 0.5 - distance, 0.0, 1.0);
        if ((style.flags & DASHED) != 0u) {
            let on = style.dash_on_pt * ppp;
            let period = on + style.dash_off_pt * ppp;
            let m = (in.along_px + t) % period;
            coverage = coverage * clamp(on - m + 0.5, 0.0, 1.0);
        }
    }
    if ((style.flags & FADES) != 0u) {
        coverage = coverage * camera.minor_alpha;
    }
    if (coverage <= 0.0) {
        discard;
    }
    var colour = style.colour;
    let op = (word >> 18u) & 0xFu;
    if (op > 0u) {
        colour = styles[OP_BASE + op - 1u].colour;
    }
    if (((word >> 16u) & 1u) == 1u && camera.by_depth > 0.5) {
        colour = textureLoad(ramp, vec2<i32>(i32((word >> 8u) & 0xFFu), 0), 0);
    }
    if (((word >> 17u) & 1u) == 1u) {
        colour = colour * STALE_ALPHA;
    }
    if (((word >> 22u) & 1u) == 1u) {
        colour = colour * FADED_ALPHA;
    }
    return colour * coverage;
}
