// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

// The Viewport's picture shader (functional spec, capability 56;
// docs/specs/phase-4/design.md, "Architecture -> The Viewport renderer"):
// one textured quad per bitmap at its footprint in project millimetres,
// drawn after the stock and under the shapes. The texture is the bitmap's
// 8-bit gray, one channel, sampled with linear filtering (and through its
// mip chain once that is uploaded, so a picture shown small does not
// shimmer). The quad's uniform carries the footprint and how strongly the
// picture draws: 1, or 0.6 while a shape is selected (experience.md,
// section 5, `viewport.picture`).
//
// Project space is Y-up and so is clip space: the transform is the line
// shader's and never flips. A bitmap's row 0 is its top row, which lies
// along the footprint's top edge (its greatest y), so the texture's v
// runs down as y runs up: v = 1 - t, the one place the picture's row
// order meets the drawing's axis. u runs with x, so nothing mirrors.

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

struct Quad {
    // The footprint: its least corner, then its greatest, mm.
    rect_mm: vec4<f32>,
    // x: how strongly the picture draws, 0 to 1. y: 1 when the
    // framebuffer is sRGB, so the gray is written in linear light.
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> quad: Quad;
@group(1) @binding(1) var picture: texture_2d<f32>;
@group(1) @binding(2) var picture_sampler: sampler;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32) -> VertexOut {
    // Two triangles over the unit square, as the line shader's quad.
    var xs = array<f32, 6>(0.0, 1.0, 0.0, 0.0, 1.0, 1.0);
    var ys = array<f32, 6>(0.0, 0.0, 1.0, 1.0, 0.0, 1.0);
    let t = vec2<f32>(xs[vertex], ys[vertex]);
    let p_mm = mix(quad.rect_mm.xy, quad.rect_mm.zw, t);
    let p = (p_mm - camera.centre_mm) * camera.px_per_mm;

    var out: VertexOut;
    out.position = vec4<f32>(p * 2.0 / camera.viewport_px, 0.0, 1.0);
    out.uv = vec2<f32>(t.x, 1.0 - t.y);
    return out;
}

// An sRGB-encoded value in linear light.
fn to_linear(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var gray = textureSample(picture, picture_sampler, in.uv).r;
    if (quad.params.y > 0.5) {
        gray = to_linear(gray);
    }
    let alpha = quad.params.x;
    // Premultiplied, as the lines are.
    return vec4<f32>(gray * alpha, gray * alpha, gray * alpha, alpha);
}
