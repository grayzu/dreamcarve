// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to WGSL and modified from PureCutCNC:
//   src/engine/simulation/toolMesh.ts (the tool's two parts and their colours)
//   src/engine/simulation/heightfieldShader.ts (the light rig)

// What is drawn over the simulated stock, in the same pass and against
// the same depth buffer (phase 6 design, "The renderer", draws 4 and 5):
// the tool where the playback has it, and lines in the world (the
// toolpath of the operation being shown, its rapids, the origin's axes).
//
// From upstream: the tool is a cutting part and a plain shank, each its
// own colour, lit by the stock's two lights. Changed: the mesh is one
// turned profile (`dc_app::sim::tool_mesh`), drawn from a vertex buffer
// and moved by a uniform, where upstream moves a scene node. The lines
// are DreamCarve's: upstream draws no toolpath over its simulation. A
// line is a quad a segment, widened on the screen so it is as wide at
// the back of the stock as at the front, as the 2D Viewport's are.

struct Overlay {
    view_proj: mat4x4<f32>,
    // The tool's tip, mm; w spare.
    tip: vec4<f32>,
    cutter: vec4<f32>,
    shank: vec4<f32>,
    // The picture's width and height, px; a line's width, px; one spare.
    screen: vec4<f32>,
    // A dashed line's dash and its gap, px; two spare.
    gap: vec4<f32>,
}

@group(0) @binding(0) var<uniform> overlay: Overlay;

// The stock's rig (`stock.wgsl`): ambient, a white key, a blue-white
// fill.
fn lit(normal: vec3<f32>) -> vec3<f32> {
    let key = normalize(vec3(120.0, -120.0, 180.0));
    let fill = normalize(vec3(-120.0, 80.0, 80.0));
    let fill_colour = vec3(0.588, 0.714, 1.0);
    return vec3(0.7) + vec3(0.9) * max(dot(normal, key), 0.0)
        + fill_colour * 0.35 * max(dot(normal, fill), 0.0);
}

struct ToolVertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // 0 for the cutting part, 1 for the shank.
    @location(2) part: f32,
}

struct ToolVaryings {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) part: f32,
}

@vertex
fn vs_tool(in: ToolVertex) -> ToolVaryings {
    var out: ToolVaryings;
    out.clip = overlay.view_proj * vec4(in.position + overlay.tip.xyz, 1.0);
    out.normal = in.normal;
    out.part = in.part;
    return out;
}

@fragment
fn fs_tool(in: ToolVaryings) -> @location(0) vec4<f32> {
    let colour = mix(overlay.cutter.rgb, overlay.shank.rgb, step(0.5, in.part));
    return vec4(clamp(colour * lit(normalize(in.normal)) * 0.62, vec3(0.0), vec3(1.0)), 1.0);
}

// One segment, an instance: its ends, mm, and its colour. The alpha is
// 1 for a solid line and 0 for a dashed one.
struct Segment {
    @location(0) a: vec3<f32>,
    @location(1) b: vec3<f32>,
    @location(2) colour: vec4<f32>,
}

struct LineVaryings {
    @builtin(position) clip: vec4<f32>,
    @location(0) colour: vec3<f32>,
    // How far along the segment, px, for the dashes; negative for a
    // solid line. Even on the screen, not in the world.
    @location(1) @interpolate(linear) along: f32,
}

// The least w a line's end is drawn at: an end behind the eye is
// brought to the plane just in front of it.
const NEAR_W: f32 = 0.001;
// How far a line stands off the surface it lies on, in depth, so a cut's
// path is not lost in its own floor.
const LIFT: f32 = 0.0004;

@vertex
fn vs_line(@builtin(vertex_index) vertex: u32, segment: Segment) -> LineVaryings {
    // The quad's corners: which end, and which side of the line.
    let ends = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    let sides = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    let end = ends[vertex % 6u];
    let side = sides[vertex % 6u];

    var a = overlay.view_proj * vec4(segment.a, 1.0);
    var b = overlay.view_proj * vec4(segment.b, 1.0);
    // Clipped to the plane in front of the eye.
    if a.w < NEAR_W && b.w > NEAR_W {
        a = mix(a, b, (NEAR_W - a.w) / (b.w - a.w));
    } else if b.w < NEAR_W && a.w > NEAR_W {
        b = mix(b, a, (NEAR_W - b.w) / (a.w - b.w));
    }
    var out: LineVaryings;
    out.colour = segment.colour.rgb;
    if a.w < NEAR_W || b.w < NEAR_W {
        // Wholly behind the eye: nothing to draw.
        out.clip = vec4(0.0, 0.0, 2.0, 1.0);
        out.along = -1.0;
        return out;
    }
    let half = overlay.screen.xy / 2.0;
    let a_px = a.xy / a.w * half;
    let b_px = b.xy / b.w * half;
    let run = b_px - a_px;
    let length = max(length(run), 0.0001);
    let across = vec2(-run.y, run.x) / length;
    let here = mix(a, b, end);
    let offset_px = across * side * overlay.screen.z / 2.0;
    out.clip = vec4(
        here.xy + offset_px / half * here.w,
        here.z - LIFT * here.w,
        here.w,
    );
    out.along = select(end * length, -1.0, segment.colour.a > 0.5);
    return out;
}

@fragment
fn fs_line(in: LineVaryings) -> @location(0) vec4<f32> {
    if in.along >= 0.0 {
        let period = overlay.gap.x + overlay.gap.y;
        if in.along % period > overlay.gap.x {
            discard;
        }
    }
    return vec4(in.colour, 1.0);
}
