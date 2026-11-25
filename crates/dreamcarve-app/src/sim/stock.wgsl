// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to WGSL and modified from PureCutCNC:
//   src/engine/simulation/gpuMesh.ts
//   src/engine/simulation/instancedBoundary.ts
//   src/engine/simulation/heightfieldShader.ts (the light rig, depth darkening)

// The simulated stock (phase 6 design, "The renderer"). From upstream:
// one flat quad a cell with no vertex shared between cells, drawn a row
// at a time by instance; a wall on every grid edge, spanned in the
// vertex stage from the two heights either side, a cell off the grid
// reading as the bottom, which makes the stock's own sides; a cell cut
// through is not drawn; two fixed lights and a darkening with depth.
// Changed: the world is project space, Z up, so upstream's (x, z, y) is
// (x, y, z) here and its lights are turned to match; whether an edge is
// a step and each cell's gradient come from textures the app computes
// (`dc_app::sim::shade`), where upstream works them out here.

struct Globals {
    view_proj: mat4x4<f32>,
    // nx, ny, and two spare.
    cells: vec4<u32>,
    // The cell's size, the board's top, its width and its height, mm.
    size: vec4<f32>,
    // The deepest cut, mm below the top, which the depth ramp ends at;
    // three spare.
    depth: vec4<f32>,
    stock: vec4<f32>,
    bed: vec4<f32>,
    uncut: vec4<f32>,
    // 0 material, 1 by operation, 2 by depth; then whether uncut
    // material is shown; two spare.
    mode: vec4<u32>,
    // Colours here are sRGB-encoded, as the theme gives them, and the
    // light is worked in them, as upstream's is. The target is a plain
    // texture holding encoded colours, which is what egui samples.
}

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var heights: texture_2d<f32>;
@group(0) @binding(2) var owners: texture_2d<u32>;
@group(0) @binding(3) var classes: texture_2d<u32>;
@group(0) @binding(4) var gradients: texture_2d<f32>;
// A colour a run, by the run's place in the plan.
@group(0) @binding(5) var<storage, read> run_colours: array<vec4<f32>>;
// The depth ramp: a row of colours, shallow to deep.
@group(0) @binding(6) var ramp: texture_2d<f32>;

const EAST_STEP: u32 = 1u;
const NORTH_STEP: u32 = 2u;
const UNCUT: u32 = 4u;
const NO_OWNER: u32 = 65535u;
const THROUGH: f32 = 0.000001;

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) @interpolate(flat) cell: vec2<u32>,
    // The face's own normal for a wall that is a step; zero for a top
    // and for a riser inside a slope, which take the cell's gradient.
    @location(2) @interpolate(flat) face: vec3<f32>,
    // The cell whose cut made the face: a top's own, and for a wall the
    // lower cell beside it, whose cutter left the wall standing.
    @location(3) @interpolate(flat) cut_cell: vec2<u32>,
}

// The two triangles of a quad, as corners (0|1, 0|1).
fn corner(vertex: u32) -> vec2<f32> {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(0.0, 1.0),
    );
    return corners[vertex % 6u];
}

// A cell's height; the bottom for a cell off the grid.
fn height_at(i: i32, j: i32) -> f32 {
    if i < 0 || j < 0 || i >= i32(globals.cells.x) || j >= i32(globals.cells.y) {
        return 0.0;
    }
    return textureLoad(heights, vec2<i32>(i, j), 0).r;
}

fn class_at(i: i32, j: i32) -> u32 {
    if i < 0 || j < 0 || i >= i32(globals.cells.x) || j >= i32(globals.cells.y) {
        return EAST_STEP | NORTH_STEP;
    }
    return textureLoad(classes, vec2<i32>(i, j), 0).r;
}

// A point of the grid, trimmed to the board: the last column and row may
// hang past it.
fn on_board(x: f32, y: f32) -> vec2<f32> {
    return vec2(min(x, globals.size.z), min(y, globals.size.w));
}

// The top of cell (vertex / 6, instance).
@vertex
fn vs_top(@builtin(vertex_index) vertex: u32, @builtin(instance_index) row: u32) -> Varyings {
    let i = vertex / 6u;
    let c = corner(vertex);
    let cell = globals.size.x;
    let xy = on_board((f32(i) + c.x) * cell, (f32(row) + c.y) * cell);
    let h = height_at(i32(i), i32(row));
    var out: Varyings;
    out.world = vec3(xy, h);
    out.cell = vec2(i, row);
    out.cut_cell = out.cell;
    out.face = vec3(0.0);
    out.clip = globals.view_proj * vec4(out.world, 1.0);
    if h <= THROUGH {
        // Cut through: nothing to draw.
        out.clip = vec4(0.0, 0.0, 2.0, 1.0);
    }
    return out;
}

// The wall on the edge between cells (edge - 1, row) and (edge, row):
// edge runs from 0 to nx, the ends being the stock's own sides.
@vertex
fn vs_wall_x(@builtin(vertex_index) vertex: u32, @builtin(instance_index) row: u32) -> Varyings {
    let edge = i32(vertex / 6u);
    let c = corner(vertex);
    let cell = globals.size.x;
    let west = height_at(edge - 1, i32(row));
    let east = height_at(edge, i32(row));
    let xy0 = on_board(f32(edge) * cell, f32(row) * cell);
    let xy1 = on_board(f32(edge) * cell, f32(row + 1u) * cell);
    var out: Varyings;
    out.world = vec3(mix(xy0, xy1, c.x), mix(min(west, east), max(west, east), c.y));
    // The wall belongs to the higher cell, and faces the lower one.
    let higher = select(edge, edge - 1, west > east);
    let lower = select(edge - 1, edge, west > east);
    out.cell = vec2(u32(clamp(higher, 0, i32(globals.cells.x) - 1)), row);
    // Off the grid there is no cut: the stock's own side is its
    // material's.
    out.cut_cell = select(
        out.cell,
        vec2(u32(max(lower, 0)), row),
        lower >= 0 && lower < i32(globals.cells.x),
    );
    let is_step = (class_at(edge - 1, i32(row)) & EAST_STEP) != 0u;
    out.face = select(vec3(0.0), vec3(select(-1.0, 1.0, west > east), 0.0, 0.0), is_step);
    out.clip = globals.view_proj * vec4(out.world, 1.0);
    return out;
}

// The wall on the edge between cells (i, edge - 1) and (i, edge): the
// instance is the edge, from 0 to ny.
@vertex
fn vs_wall_y(@builtin(vertex_index) vertex: u32, @builtin(instance_index) edge_index: u32) -> Varyings {
    let i = vertex / 6u;
    let edge = i32(edge_index);
    let c = corner(vertex);
    let cell = globals.size.x;
    let south = height_at(i32(i), edge - 1);
    let north = height_at(i32(i), edge);
    let xy0 = on_board(f32(i) * cell, f32(edge) * cell);
    let xy1 = on_board(f32(i + 1u) * cell, f32(edge) * cell);
    var out: Varyings;
    out.world = vec3(mix(xy0, xy1, c.x), mix(min(south, north), max(south, north), c.y));
    let higher = select(edge, edge - 1, south > north);
    let lower = select(edge - 1, edge, south > north);
    out.cell = vec2(i, u32(clamp(higher, 0, i32(globals.cells.y) - 1)));
    out.cut_cell = select(
        out.cell,
        vec2(i, u32(max(lower, 0))),
        lower >= 0 && lower < i32(globals.cells.y),
    );
    let is_step = (class_at(i32(i), edge - 1) & NORTH_STEP) != 0u;
    out.face = select(vec3(0.0), vec3(0.0, select(-1.0, 1.0, south > north), 0.0), is_step);
    out.clip = globals.view_proj * vec4(out.world, 1.0);
    return out;
}

// Upstream's rig, turned to this world: ambient, a white key from the
// front right above, a blue-white fill from the back left.
fn lit(normal: vec3<f32>) -> vec3<f32> {
    let key = normalize(vec3(120.0, -120.0, 180.0));
    let fill = normalize(vec3(-120.0, 80.0, 80.0));
    let fill_colour = vec3(0.588, 0.714, 1.0);
    return vec3(0.7) + vec3(0.9) * max(dot(normal, key), 0.0)
        + fill_colour * 0.35 * max(dot(normal, fill), 0.0);
}

@fragment
fn fs_stock(in: Varyings) -> @location(0) vec4<f32> {
    let at = vec2<i32>(in.cell);
    var normal = in.face;
    if dot(normal, normal) < 0.5 {
        // A top, or a riser inside a slope: the surface's own normal.
        let g = textureLoad(gradients, at, 0).rg;
        normal = normalize(vec3(-g.x, -g.y, 1.0));
    }
    var colour = globals.stock.rgb;
    // The run whose cut made this face.
    let owner = textureLoad(owners, vec2<i32>(in.cut_cell), 0).r;
    let depth = clamp((globals.size.y - in.world.z) / max(globals.size.y, 0.001), 0.0, 1.0);
    if owner != NO_OWNER {
        if globals.mode.x == 1u {
            colour = run_colours[min(owner, arrayLength(&run_colours) - 1u)].rgb;
        } else if globals.mode.x == 2u {
            let steps = i32(textureDimensions(ramp).x);
            // Phase 4's ramp, the top to the deepest cut of the job, at
            // this point's own depth: a wall shades down its height.
            let cut = clamp((globals.size.y - in.world.z) / max(globals.depth.x, 0.001), 0.0, 1.0);
            colour = textureLoad(ramp, vec2<i32>(min(i32(cut * f32(steps)), steps - 1), 0), 0).rgb;
        }
    }
    if globals.mode.y != 0u && (textureLoad(classes, at, 0).r & UNCUT) != 0u {
        colour = mix(colour, globals.uncut.rgb, 0.6);
    }
    // A wall that is a step is not darkened, as upstream's is not.
    let darken = select(1.0 - 0.12 * depth, 1.0, dot(in.face, in.face) > 0.5);
    return vec4(clamp(colour * lit(normal) * darken * 0.62, vec3(0.0), vec3(1.0)), 1.0);
}

// The bed: a quad at the bottom, a margin wider than the board.
@vertex
fn vs_bed(@builtin(vertex_index) vertex: u32) -> @builtin(position) vec4<f32> {
    let c = corner(vertex);
    let margin = max(max(globals.size.z, globals.size.w) * 0.05, 10.0);
    let xy = mix(vec2(-margin), vec2(globals.size.z, globals.size.w) + margin, c);
    return globals.view_proj * vec4(xy, -0.02, 1.0);
}

@fragment
fn fs_bed() -> @location(0) vec4<f32> {
    return vec4(globals.bed.rgb, 1.0);
}
