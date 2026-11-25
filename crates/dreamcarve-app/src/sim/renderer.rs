// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/gpuMesh.ts
//   src/engine/simulation/instancedBoundary.ts

//! The Simulate step's renderer (`docs/specs/phase-6/design.md`, "The
//! renderer"): the simulated stock, drawn off screen with a depth buffer
//! into a texture egui shows as the Viewport's picture.
//!
//! From upstream: the heights live in a texture and the geometry is made
//! in the vertex stage, a row of cells at a time by instance, so cutting
//! writes texels and never rebuilds a mesh; only the cells that changed
//! are written. Changed: there are no vertex or index buffers at all, a
//! cell's corner coming from the vertex's number; whether an edge is a
//! step, and each cell's slope, are textures the app computes
//! (`dc_app::sim::Shading`); the picture is drawn only when something it
//! shows has changed, and egui lays the same texture down on every other
//! frame.
//!
//! The 2D Viewport's pipelines and egui's pass have no depth buffer and
//! keep none: this renderer owns its target, 4× multisampled and
//! resolved, and submits its own commands. The target is `Rgba8Unorm`
//! holding sRGB-encoded colours, which is what egui expects of a texture
//! it samples (it reads one as "not sRGB-aware"); `stock.wgsl` lights in
//! the theme's encoded colours and writes them as they are.
//!
//! A wgpu object's debug label is an id (`sim.tops`), never words: the
//! string scan reads every literal as something a user might see.
//!
//! Geometry is `f64` up to here; `f32` appears only in the bytes written
//! to the GPU (`docs/rust-guidelines.md`, "Types").

use dc_app::sim::{Colouring, Shading, ToolMesh, ToolPart};
use dc_app::view::DEPTH_STEPS;
use dc_job::sim::{CellRect, Heightfield, UNCUT_FLAG};
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::{self, wgpu};

use crate::theme::{gamma, ramp_linear};

/// Samples a pixel of the target.
const SAMPLES: u32 = 4;
/// The target egui samples: plain, holding encoded colours, as its own
/// textures are.
const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// `Globals` of `stock.wgsl`: a matrix, then seven rows of four.
const GLOBALS_BYTES: usize = 64 + 7 * 16;
/// The classes texture's bit for a cell of uncut material; the two below
/// it are `dc_app::sim::shade`'s.
const UNCUT_BIT: u8 = 4;
/// The fewest run colours the table holds.
const MIN_RUN_COLOURS: usize = 64;
/// `Overlay` of `overlay.wgsl`: a matrix, then five rows of four.
const OVERLAY_BYTES: usize = 64 + 5 * 16;
/// A corner of the tool's mesh: a place, a normal and its part.
const TOOL_VERTEX_BYTES: usize = 28;
/// A line: its two ends and its colour.
const LINE_BYTES: usize = 40;
/// A dashed line's dash and its gap, logical px: the 2D Viewport's
/// rapids'.
const DASH_PT: (f32, f32) = (5.0, 4.0);

/// What the picture is drawn with: the theme's colours and the step's
/// display choices. A change to it is a reason to draw again.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// Behind everything: the Viewport card's own colour.
    pub background: Color32,
    /// Material no run has cut.
    pub stock: Color32,
    /// The bed under the stock; `None` while it is not shown.
    pub bed: Option<Color32>,
    /// What colours a cut cell.
    pub colouring: Colouring,
    /// A colour a run, by the run's place in the plan: its operation's.
    pub runs: Vec<Color32>,
    /// The depth ramp's five stops, shallow to deep.
    pub ramp: [Color32; 5],
    /// The tint of uncut material; `None` while it is not shown.
    pub uncut: Option<Color32>,
    /// The tool's cutting part and its shank.
    pub tool: (Color32, Color32),
}

/// One line in the world, drawn over the stock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    /// Its ends, mm.
    pub a: [f32; 3],
    /// Its other end, mm.
    pub b: [f32; 3],
    /// Its colour.
    pub colour: Color32,
    /// Whether it is dashed: a rapid.
    pub dashed: bool,
}

/// One picture: where the camera is and how large the card is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shot {
    /// World to clip space, column by column.
    pub view_projection: [[f32; 4]; 4],
    /// The card, physical pixels.
    pub size_px: [u32; 2],
    /// Where the tool's tip is, mm, while the tool is drawn.
    pub tool_tip: Option<[f32; 3]>,
    /// Physical pixels a logical one: a line's width and its dashes
    /// follow it.
    pub pixels_per_point: f32,
}

/// The board drawn, as the shader takes it: narrowed here, at the
/// renderer's edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extent {
    /// A cell's side, mm.
    pub cell_mm: f32,
    /// The board's top, mm above its bottom.
    pub top_mm: f32,
    /// The board's extent along X, mm.
    pub width_mm: f32,
    /// The board's extent along Y, mm.
    pub height_mm: f32,
    /// The job's deepest cut, mm below the top: where the depth ramp
    /// ends.
    pub deepest_mm: f32,
}

/// Why a stock cannot be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    /// The grid has more cells along a side than the graphics device's
    /// largest texture.
    GridTooLarge {
        /// The grid's longer count of cells.
        cells: u32,
        /// The device's largest texture side.
        most: u32,
    },
}

/// What each draw of a grid of `nx` by `ny` cells takes: its vertices,
/// then its instances. From upstream's `gpuMesh.ts` and
/// `instancedBoundary.ts`: a top a cell with no vertex shared between
/// cells, and a wall on every edge of the grid, the stock's own sides
/// among them. Six vertices make a quad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Draws {
    /// A row of tops, a row an instance.
    tops: (u32, u32),
    /// The edges between east–west neighbours in a row, a row an
    /// instance.
    walls_x: (u32, u32),
    /// The edges between north–south neighbours in a row of edges, a row
    /// of edges an instance.
    walls_y: (u32, u32),
}

impl Draws {
    const fn of(nx: u32, ny: u32) -> Self {
        Self {
            tops: (nx * 6, ny),
            walls_x: ((nx + 1) * 6, ny),
            walls_y: (nx * 6, ny + 1),
        }
    }
}

/// The bytes of one rectangle of cells, a texture each: what a change
/// to the stock sends to the GPU, and no more.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Patch {
    /// The rectangle's cells across and its rows.
    width: usize,
    rows: usize,
    /// `f32` heights, mm.
    heights: Vec<u8>,
    /// `u16` owners.
    owners: Vec<u8>,
    /// The step classes, with the check's flag.
    classes: Vec<u8>,
    /// Two `f32`s a cell.
    gradients: Vec<u8>,
}

impl Patch {
    /// The cells of `rect`, row by row. `None` for an empty rectangle,
    /// or when `shading` is not of `field`'s grid.
    fn of(
        field: &Heightfield,
        shading: &Shading,
        flags: Option<&[u8]>,
        rect: CellRect,
    ) -> Option<Self> {
        if rect.i0 >= rect.i1 || rect.j0 >= rect.j1 {
            return None;
        }
        let (width, rows) = ((rect.i1 - rect.i0) as usize, (rect.j1 - rect.j0) as usize);
        let cells = width * rows;
        let mut patch = Self {
            width,
            rows,
            heights: Vec::with_capacity(cells * 4),
            owners: Vec::with_capacity(cells * 2),
            classes: Vec::with_capacity(cells),
            gradients: Vec::with_capacity(cells * 8),
        };
        let stride = field.grid().nx() as usize;
        for j in rect.j0 as usize..rect.j1 as usize {
            let row = j * stride + rect.i0 as usize..j * stride + rect.i1 as usize;
            for z_mm in field.heights().get(row.clone()).unwrap_or(&[]) {
                // The renderer's edge: a height is narrowed here.
                #[expect(clippy::cast_possible_truncation, reason = "narrowed for the GPU")]
                patch
                    .heights
                    .extend_from_slice(&(*z_mm as f32).to_ne_bytes());
            }
            for owner in field.owners().get(row.clone()).unwrap_or(&[]) {
                patch.owners.extend_from_slice(&owner.to_ne_bytes());
            }
            let class = shading.classes.get(row.clone()).unwrap_or(&[]);
            let flag = flags.and_then(|f| f.get(row.clone()));
            for (k, class) in class.iter().enumerate() {
                let uncut = flag
                    .and_then(|f| f.get(k))
                    .is_some_and(|f| f & UNCUT_FLAG != 0);
                patch
                    .classes
                    .push(class | if uncut { UNCUT_BIT } else { 0 });
            }
            for [dx, dy] in shading.gradients.get(row).unwrap_or(&[]) {
                patch.gradients.extend_from_slice(&dx.to_ne_bytes());
                patch.gradients.extend_from_slice(&dy.to_ne_bytes());
            }
        }
        let whole = patch.heights.len() == cells * 4
            && patch.owners.len() == cells * 2
            && patch.classes.len() == cells
            && patch.gradients.len() == cells * 8;
        whole.then_some(patch)
    }
}

/// The textures of one grid and the bind group that reads them.
struct GpuGrid {
    nx: u32,
    ny: u32,
    heights: wgpu::Texture,
    owners: wgpu::Texture,
    classes: wgpu::Texture,
    gradients: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    /// The run colour table the bind group was made with.
    run_capacity: usize,
}

/// The off-screen target.
struct Target {
    size_px: [u32; 2],
    colour: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::TextureView,
}

/// The renderer: its pipelines, the grid on the GPU, and the target.
pub struct StockRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    bed: wgpu::RenderPipeline,
    tops: wgpu::RenderPipeline,
    walls_x: wgpu::RenderPipeline,
    walls_y: wgpu::RenderPipeline,
    globals: wgpu::Buffer,
    run_colours: wgpu::Buffer,
    run_capacity: usize,
    ramp: wgpu::Texture,
    max_side: u32,
    grid: Option<GpuGrid>,
    target: Option<Target>,
    texture: Option<egui::TextureId>,
    /// What the target shows, so the same picture is not drawn twice.
    drawn: Option<(Shot, Look, Extent)>,
    /// Whether a texture was written since the target was drawn.
    stale: bool,
    tool_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    overlay: wgpu::Buffer,
    overlay_bind: wgpu::BindGroup,
    /// The tool's mesh on the GPU, with what it was made of and its
    /// count of corners.
    tool: Option<(u64, wgpu::Buffer, u32)>,
    /// The lines on the GPU, likewise.
    lines: Option<(u64, wgpu::Buffer, u32)>,
}

impl std::fmt::Debug for StockRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StockRenderer")
            .field("grid", &self.grid.as_ref().map(|g| (g.nx, g.ny)))
            .field("target", &self.target.as_ref().map(|t| t.size_px))
            .finish_non_exhaustive()
    }
}

/// A `usize` count as the `u32` wgpu asks for; counts here are cells of
/// a grid, far under 2³².
fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// `colour` as the shader takes it: sRGB-encoded, alpha one.
fn encoded(colour: Color32) -> [f32; 4] {
    let [r, g, b, _] = colour.to_normalized_gamma_f32();
    [r, g, b, 1.0]
}

impl StockRenderer {
    /// The pipelines, on the window's own device; no grid and no target
    /// yet.
    #[must_use]
    pub fn new(state: &egui_wgpu::RenderState) -> Self {
        let device = state.device.clone();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sim.stock"),
            source: wgpu::ShaderSource::Wgsl(include_str!("stock.wgsl").into()),
        });
        let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: both,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let buffer = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: both,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let unfiltered = wgpu::TextureSampleType::Float { filterable: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sim.stock"),
            entries: &[
                buffer(0, wgpu::BufferBindingType::Uniform),
                texture(1, unfiltered),
                texture(2, wgpu::TextureSampleType::Uint),
                texture(3, wgpu::TextureSampleType::Uint),
                texture(4, unfiltered),
                buffer(5, wgpu::BufferBindingType::Storage { read_only: true }),
                texture(6, unfiltered),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sim.stock"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, vertex, fragment| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: TARGET_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                // A wall is seen from either side: nothing is culled.
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..wgpu::MultisampleState::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sim.globals"),
            size: GLOBALS_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ramp = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sim.ramp"),
            size: wgpu::Extent3d {
                width: count(DEPTH_STEPS),
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let (tool_pipeline, line_pipeline, overlay, overlay_bind) = Self::overlays(&device);
        Self {
            tool_pipeline,
            line_pipeline,
            overlay,
            overlay_bind,
            tool: None,
            lines: None,
            bed: pipeline("sim.bed", "vs_bed", "fs_bed"),
            tops: pipeline("sim.tops", "vs_top", "fs_stock"),
            walls_x: pipeline("sim.walls.x", "vs_wall_x", "fs_stock"),
            walls_y: pipeline("sim.walls.y", "vs_wall_y", "fs_stock"),
            run_colours: Self::run_buffer(&device, MIN_RUN_COLOURS),
            run_capacity: MIN_RUN_COLOURS,
            max_side: device.limits().max_texture_dimension_2d,
            queue: state.queue.clone(),
            device,
            layout,
            globals,
            ramp,
            grid: None,
            target: None,
            texture: None,
            drawn: None,
            stale: true,
        }
    }

    /// The pipelines of `overlay.wgsl`, the tool's and the lines', with
    /// the uniform they share.
    fn overlays(
        device: &wgpu::Device,
    ) -> (
        wgpu::RenderPipeline,
        wgpu::RenderPipeline,
        wgpu::Buffer,
        wgpu::BindGroup,
    ) {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sim.overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("overlay.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sim.overlay"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sim.overlay"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |label, vertex, fragment, buffer: wgpu::VertexBufferLayout<'_>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(vertex),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(buffer)],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: TARGET_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    ..wgpu::MultisampleState::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let tool = pipeline(
            "sim.tool",
            "vs_tool",
            "fs_tool",
            wgpu::VertexBufferLayout {
                array_stride: TOOL_VERTEX_BYTES as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32],
            },
        );
        let line = pipeline(
            "sim.lines",
            "vs_line",
            "fs_line",
            wgpu::VertexBufferLayout {
                array_stride: LINE_BYTES as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4],
            },
        );
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sim.overlay"),
            size: OVERLAY_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sim.overlay"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        (tool, line, uniform, bind)
    }

    /// A vertex buffer holding `bytes`.
    fn vertex_buffer(&self, label: &str, bytes: &[u8]) -> wgpu::Buffer {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes.len().max(4) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, bytes);
        buffer
    }

    /// The tool to draw: the mesh `mesh` gives, made and sent only when
    /// `key` is not the one on the GPU. The next [`Self::draw`] draws
    /// anew.
    pub fn set_tool(&mut self, key: u64, mesh: impl FnOnce() -> ToolMesh) {
        if self.tool.as_ref().is_some_and(|(have, _, _)| *have == key) {
            return;
        }
        let mesh = mesh();
        let mut bytes = Vec::with_capacity(mesh.vertices.len() * TOOL_VERTEX_BYTES);
        for vertex in &mesh.vertices {
            let (p, n) = (vertex.position, vertex.normal);
            let part = match vertex.part {
                ToolPart::Cutter => 0.0_f32,
                ToolPart::Shank => 1.0,
            };
            // The renderer's edge: the mesh is narrowed here.
            #[expect(clippy::cast_possible_truncation, reason = "narrowed for the GPU")]
            let corner = [
                p.x as f32, p.y as f32, p.z as f32, n.x as f32, n.y as f32, n.z as f32, part,
            ];
            for v in corner {
                bytes.extend_from_slice(&v.to_ne_bytes());
            }
        }
        let buffer = self.vertex_buffer("sim.tool", &bytes);
        self.tool = Some((key, buffer, count(mesh.vertices.len())));
        self.stale = true;
    }

    /// The lines to draw, made and sent only when `key` is not the one
    /// on the GPU.
    pub fn set_lines(&mut self, key: u64, lines: impl FnOnce() -> Vec<Line>) {
        if self.lines.as_ref().is_some_and(|(have, _, _)| *have == key) {
            return;
        }
        let lines = lines();
        let mut bytes = Vec::with_capacity(lines.len() * LINE_BYTES);
        for line in &lines {
            let [r, g, b, _] = line.colour.to_normalized_gamma_f32();
            // The shader reads the alpha as solid or dashed.
            let solid = if line.dashed { 0.0 } else { 1.0 };
            for v in line.a.into_iter().chain(line.b).chain([r, g, b, solid]) {
                bytes.extend_from_slice(&v.to_ne_bytes());
            }
        }
        let buffer = self.vertex_buffer("sim.lines", &bytes);
        self.lines = Some((key, buffer, count(lines.len())));
        self.stale = true;
    }

    /// The overlay's uniform for `shot` and `look`.
    fn overlay_bytes(shot: &Shot, look: &Look) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(OVERLAY_BYTES);
        for v in shot.view_projection.iter().flatten() {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        let [x, y, z] = shot.tool_tip.unwrap_or([0.0; 3]);
        // A card is a few thousand pixels at most: exact in an `f32`.
        #[expect(clippy::cast_precision_loss, reason = "a card's size in pixels")]
        let (width, height) = (shot.size_px[0] as f32, shot.size_px[1] as f32);
        let scale = shot.pixels_per_point;
        for v in [
            [x, y, z, 0.0],
            encoded(look.tool.0),
            encoded(look.tool.1),
            [width, height, 1.5 * scale, 0.0],
            [DASH_PT.0 * scale, DASH_PT.1 * scale, 0.0, 0.0],
        ]
        .iter()
        .flatten()
        {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        bytes
    }

    /// A run colour table of `capacity` colours.
    fn run_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sim.runs"),
            size: (capacity * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// A texture of the grid's size.
    fn cells_texture(
        &self,
        label: &str,
        nx: u32,
        ny: u32,
        format: wgpu::TextureFormat,
    ) -> wgpu::Texture {
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: nx,
                height: ny,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    /// The bind group reading `textures` and the run colour table.
    fn bind(
        &self,
        heights: &wgpu::Texture,
        owners: &wgpu::Texture,
        classes: &wgpu::Texture,
        gradients: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        let view = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor::default());
        let (heights, owners, classes, gradients, ramp) = (
            view(heights),
            view(owners),
            view(classes),
            view(gradients),
            view(&self.ramp),
        );
        let texture = |binding, view| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        };
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sim.stock"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.globals.as_entire_binding(),
                },
                texture(1, &heights),
                texture(2, &owners),
                texture(3, &classes),
                texture(4, &gradients),
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: self.run_colours.as_entire_binding(),
                },
                texture(6, &ramp),
            ],
        })
    }

    /// Sends the cells of `changed` to the GPU: `field`'s heights and
    /// owners, `shading`'s classes and gradients, and `flags`, the
    /// check's, where there are any. `None` sends every cell, which a
    /// grid of another size does whatever is asked. The next
    /// [`Self::draw`] draws anew.
    ///
    /// # Errors
    ///
    /// [`RenderError::GridTooLarge`] for a grid the device has no
    /// texture for; nothing is sent.
    pub fn upload(
        &mut self,
        field: &Heightfield,
        shading: &Shading,
        flags: Option<&[u8]>,
        changed: Option<CellRect>,
    ) -> Result<(), RenderError> {
        let grid = field.grid();
        let (nx, ny) = (grid.nx(), grid.ny());
        if nx.max(ny) > self.max_side {
            return Err(RenderError::GridTooLarge {
                cells: nx.max(ny),
                most: self.max_side,
            });
        }
        let whole = CellRect {
            i0: 0,
            i1: nx,
            j0: 0,
            j1: ny,
        };
        let fits = self.grid.as_ref().is_some_and(|g| (g.nx, g.ny) == (nx, ny));
        let rect = if fits {
            changed.map_or(whole, |c| CellRect {
                i0: c.i0.min(nx),
                i1: c.i1.min(nx),
                j0: c.j0.min(ny),
                j1: c.j1.min(ny),
            })
        } else {
            let heights = self.cells_texture("sim.heights", nx, ny, wgpu::TextureFormat::R32Float);
            let owners = self.cells_texture("sim.owners", nx, ny, wgpu::TextureFormat::R16Uint);
            let classes = self.cells_texture("sim.classes", nx, ny, wgpu::TextureFormat::R8Uint);
            let gradients =
                self.cells_texture("sim.gradients", nx, ny, wgpu::TextureFormat::Rg32Float);
            let bind_group = self.bind(&heights, &owners, &classes, &gradients);
            self.grid = Some(GpuGrid {
                nx,
                ny,
                heights,
                owners,
                classes,
                gradients,
                bind_group,
                run_capacity: self.run_capacity,
            });
            whole
        };
        let Some(gpu) = self.grid.as_ref() else {
            return Ok(());
        };
        // Nothing to send, or a field and a shading of different grids:
        // nothing is sent rather than a torn picture.
        let Some(patch) = Patch::of(field, shading, flags, rect) else {
            return Ok(());
        };
        let (width, rows) = (patch.width, patch.rows);
        let Patch {
            heights,
            owners,
            classes,
            gradients,
            ..
        } = &patch;
        let write = |texture: &wgpu::Texture, bytes: &[u8], texel: usize| {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: rect.i0,
                        y: rect.j0,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(count(width * texel)),
                    rows_per_image: Some(count(rows)),
                },
                wgpu::Extent3d {
                    width: count(width),
                    height: count(rows),
                    depth_or_array_layers: 1,
                },
            );
        };
        write(&gpu.heights, heights, 4);
        write(&gpu.owners, owners, 2);
        write(&gpu.classes, classes, 1);
        write(&gpu.gradients, gradients, 8);
        self.stale = true;
        Ok(())
    }

    /// The target of `size_px`, made anew when the card's size changed,
    /// and egui's handle to it.
    fn target(&mut self, state: &egui_wgpu::RenderState, size_px: [u32; 2]) -> egui::TextureId {
        if let (Some(target), Some(id)) = (&self.target, self.texture)
            && target.size_px == size_px
        {
            return id;
        }
        let texture = |label, format, sample_count, usage| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: size_px[0],
                        height: size_px[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let attachment = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let target = Target {
            size_px,
            colour: texture("sim.target.colour", TARGET_FORMAT, SAMPLES, attachment),
            depth: texture("sim.target.depth", DEPTH_FORMAT, SAMPLES, attachment),
            resolved: texture(
                "sim.target.picture",
                TARGET_FORMAT,
                1,
                attachment | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
        };
        let mut egui_renderer = state.renderer.write();
        let id = if let Some(id) = self.texture {
            egui_renderer.update_egui_texture_from_wgpu_texture(
                &self.device,
                &target.resolved,
                wgpu::FilterMode::Linear,
                id,
            );
            id
        } else {
            egui_renderer.register_native_texture(
                &self.device,
                &target.resolved,
                wgpu::FilterMode::Linear,
            )
        };
        drop(egui_renderer);
        self.target = Some(target);
        self.texture = Some(id);
        self.drawn = None;
        id
    }

    /// The globals' bytes for a grid of `cells` on the board `extent`.
    fn globals_bytes(shot: &Shot, look: &Look, cells: [u32; 2], extent: Extent) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(GLOBALS_BYTES);
        for v in shot.view_projection.iter().flatten() {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        for v in [cells[0], cells[1], 0, 0] {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        let uncut = look.uncut.unwrap_or(crate::theme::NONE);
        let bed = look.bed.unwrap_or(look.background);
        let size = [
            extent.cell_mm,
            extent.top_mm,
            extent.width_mm,
            extent.height_mm,
        ];
        let depth = [extent.deepest_mm, 0.0, 0.0, 0.0];
        for v in [
            size,
            depth,
            encoded(look.stock),
            encoded(bed),
            encoded(uncut),
        ]
        .iter()
        .flatten()
        {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        let colouring = match look.colouring {
            Colouring::Material => 0u32,
            Colouring::Operation => 1,
            Colouring::Depth => 2,
        };
        for v in [colouring, u32::from(look.uncut.is_some()), 0, 0] {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        bytes
    }

    /// Writes the run colours and the ramp of `look`, growing the run
    /// table when the plan has more runs than it holds.
    fn write_look(&mut self, look: &Look) {
        let wanted = look.runs.len().max(1);
        if wanted > self.run_capacity {
            self.run_capacity = wanted.next_power_of_two();
            self.run_colours = Self::run_buffer(&self.device, self.run_capacity);
        }
        if let Some(gpu) = &self.grid
            && gpu.run_capacity != self.run_capacity
        {
            let bind_group = self.bind(&gpu.heights, &gpu.owners, &gpu.classes, &gpu.gradients);
            if let Some(gpu) = &mut self.grid {
                gpu.bind_group = bind_group;
                gpu.run_capacity = self.run_capacity;
            }
        }
        let mut runs = Vec::with_capacity(self.run_capacity * 16);
        for colour in &look.runs {
            for v in encoded(*colour) {
                runs.extend_from_slice(&v.to_ne_bytes());
            }
        }
        if !runs.is_empty() {
            self.queue.write_buffer(&self.run_colours, 0, &runs);
        }
        let mut ramp = Vec::with_capacity(DEPTH_STEPS * 16);
        for step in 0..DEPTH_STEPS {
            #[allow(clippy::cast_precision_loss, reason = "under 256 steps")]
            let t = step as f32 / (DEPTH_STEPS - 1) as f32;
            let [r, g, b] = ramp_linear(&look.ramp, t);
            for v in [gamma(r), gamma(g), gamma(b), 1.0] {
                ramp.extend_from_slice(&v.to_ne_bytes());
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.ramp,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &ramp,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(count(DEPTH_STEPS * 16)),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: count(DEPTH_STEPS),
                height: 1,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Draws the stock as `shot` sees it, unless the target already
    /// shows exactly that, and returns egui's handle to the picture.
    /// `None` when there is no stock or no room to draw.
    pub fn draw(
        &mut self,
        state: &egui_wgpu::RenderState,
        shot: Shot,
        look: &Look,
        extent: Extent,
    ) -> Option<egui::TextureId> {
        let size_px = [
            shot.size_px[0].min(self.max_side),
            shot.size_px[1].min(self.max_side),
        ];
        if size_px[0] == 0 || size_px[1] == 0 || self.grid.is_none() {
            return None;
        }
        let id = self.target(state, size_px);
        let same = self
            .drawn
            .as_ref()
            .is_some_and(|(s, l, e)| *s == shot && l == look && *e == extent);
        if same && !self.stale {
            return Some(id);
        }
        if self.drawn.as_ref().is_none_or(|(_, l, _)| l != look) {
            self.write_look(look);
        }
        let (gpu, target) = (self.grid.as_ref()?, self.target.as_ref()?);
        self.queue.write_buffer(
            &self.globals,
            0,
            &Self::globals_bytes(&shot, look, [gpu.nx, gpu.ny], extent),
        );
        self.queue
            .write_buffer(&self.overlay, 0, &Self::overlay_bytes(&shot, look));
        let [r, g, b, _] = look.background.to_normalized_gamma_f32();
        // Encoded, as every colour in the target is.
        let clear = wgpu::Color {
            r: f64::from(r),
            g: f64::from(g),
            b: f64::from(b),
            a: 1.0,
        };
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("sim.stock"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sim.stock"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.colour,
                    depth_slice: None,
                    resolve_target: Some(&target.resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        // Only the resolved picture is read.
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &gpu.bind_group, &[]);
            if look.bed.is_some() {
                pass.set_pipeline(&self.bed);
                pass.draw(0..6, 0..1);
            }
            let draws = Draws::of(gpu.nx, gpu.ny);
            pass.set_pipeline(&self.tops);
            pass.draw(0..draws.tops.0, 0..draws.tops.1);
            pass.set_pipeline(&self.walls_x);
            pass.draw(0..draws.walls_x.0, 0..draws.walls_x.1);
            pass.set_pipeline(&self.walls_y);
            pass.draw(0..draws.walls_y.0, 0..draws.walls_y.1);
            // Over the stock, against the same depth: the lines, then
            // the tool.
            pass.set_bind_group(0, &self.overlay_bind, &[]);
            if let Some((_, buffer, lines)) = &self.lines
                && *lines > 0
            {
                pass.set_pipeline(&self.line_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..*lines);
            }
            if let (Some((_, buffer, corners)), Some(_)) = (&self.tool, shot.tool_tip)
                && *corners > 0
            {
                pass.set_pipeline(&self.tool_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*corners, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.drawn = Some((shot, look.clone(), extent));
        self.stale = false;
        Some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> Look {
        let palette = &crate::theme::DARK;
        Look {
            background: palette.viewport_bg,
            stock: palette.sim_stock,
            bed: Some(palette.sim_bed),
            colouring: Colouring::Operation,
            runs: vec![palette.operation(0)],
            ramp: palette.depth_ramp,
            uncut: Some(palette.status_warning.stroke),
            tool: (palette.sim_tool, palette.sim_tool_shank),
        }
    }

    /// The globals are laid out as `stock.wgsl` declares them: a matrix,
    /// the counts, the sizes, three colours, the mode.
    #[test]
    fn the_globals_match_the_shader() {
        let shot = Shot {
            view_projection: [[1.0, 0.0, 0.0, 0.0]; 4],
            size_px: [640, 480],
            tool_tip: Some([10.0, 20.0, 3.0]),
            pixels_per_point: 2.0,
        };
        // The overlay's uniform: the matrix, the tip, two colours, the
        // picture's size with the line's width, the dashes.
        let overlay = StockRenderer::overlay_bytes(&shot, &look());
        assert_eq!(overlay.len(), OVERLAY_BYTES);
        let float = |at: usize| {
            let mut four = [0u8; 4];
            four.copy_from_slice(&overlay[at..at + 4]);
            f32::from_ne_bytes(four)
        };
        assert!((float(64) - 10.0).abs() < 1e-6 && (float(72) - 3.0).abs() < 1e-6);
        assert!((float(112) - 640.0).abs() < 1e-6 && (float(120) - 3.0).abs() < 1e-6);
        assert!((float(128) - 10.0).abs() < 1e-6 && (float(132) - 8.0).abs() < 1e-6);
        let extent = Extent {
            cell_mm: 0.166,
            top_mm: 11.8,
            width_mm: 170.0,
            height_mm: 120.0,
            deepest_mm: 11.8,
        };
        let bytes = StockRenderer::globals_bytes(&shot, &look(), [1024, 723], extent);
        assert_eq!(bytes.len(), GLOBALS_BYTES);
        let word = |at: usize| {
            let mut four = [0u8; 4];
            four.copy_from_slice(&bytes[at..at + 4]);
            four
        };
        assert_eq!(u32::from_ne_bytes(word(64)), 1024);
        assert_eq!(u32::from_ne_bytes(word(68)), 723);
        assert!((f32::from_ne_bytes(word(84)) - 11.8).abs() < 1e-6);
        assert!((f32::from_ne_bytes(word(96)) - 11.8).abs() < 1e-6);
        // By operation, with uncut material shown.
        assert_eq!(u32::from_ne_bytes(word(160)), 1);
        assert_eq!(u32::from_ne_bytes(word(164)), 1);

        let source = include_str!("stock.wgsl");
        let fields: Vec<&str> = source
            .split("struct Globals {")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .map(|body| {
                body.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty() && !l.starts_with("//"))
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            fields,
            [
                "view_proj: mat4x4<f32>,",
                "cells: vec4<u32>,",
                "size: vec4<f32>,",
                "depth: vec4<f32>,",
                "stock: vec4<f32>,",
                "bed: vec4<f32>,",
                "uncut: vec4<f32>,",
                "mode: vec4<u32>,",
            ]
        );
    }

    /// Ported from upstream's `gpuMesh.test.ts` and
    /// `instancedBoundary.test.ts`, as the counts the draws ask for: a
    /// quad a cell, none shared; a wall on every edge of the grid, its
    /// outer edges among them.
    #[test]
    fn one_top_a_cell_and_a_wall_an_edge() {
        for (nx, ny) in [(1_u32, 1_u32), (3, 2), (1024, 723)] {
            let draws = Draws::of(nx, ny);
            let quads = |(vertices, instances): (u32, u32)| {
                assert_eq!(vertices % 6, 0);
                u64::from(vertices / 6) * u64::from(instances)
            };
            assert_eq!(quads(draws.tops), u64::from(nx) * u64::from(ny));
            // Between columns, and the two sides: nx + 1 a row.
            assert_eq!(quads(draws.walls_x), u64::from(nx + 1) * u64::from(ny));
            assert_eq!(quads(draws.walls_y), u64::from(nx) * u64::from(ny + 1));
        }
        // A 3 × 2 grid: 6 tops and its 17 edges.
        let draws = Draws::of(3, 2);
        assert_eq!(draws.tops, (18, 2));
        assert_eq!(draws.walls_x, (24, 2));
        assert_eq!(draws.walls_y, (18, 3));
    }

    /// A change sends its rectangle's cells and no others: each
    /// texture's bytes are that many texels, row by row from the
    /// rectangle's corner, the check's flag folded into the classes.
    #[test]
    fn only_the_changed_rectangle_is_sent() {
        use dc_geometry::Point3;
        use dc_job::sim::{CellSpec, Cutter, Grid, SimMove};

        let grid = Grid::of(8.0, 6.0, CellSpec::CellMm(1.0)).unwrap();
        let mut field = Heightfield::uncut(grid, 10.0).unwrap();
        // A slot along Y = 2.5 from X = 2.5 to 5.5, 3 mm deep: cells
        // (2..6, 2) at the least.
        field.cut(
            &SimMove::cut(Point3::new(2.5, 2.5, 7.0), Point3::new(5.5, 2.5, 7.0), 1.0),
            (0.0, 1.0),
            &Cutter::Flat { radius_mm: 0.4 },
            7,
        );
        let shading = Shading::of(&field);
        let mut flags = vec![0_u8; grid.len()];
        flags[2 * 8 + 3] = UNCUT_FLAG;
        let rect = CellRect {
            i0: 2,
            i1: 5,
            j0: 1,
            j1: 3,
        };
        let patch = Patch::of(&field, &shading, Some(&flags), rect).unwrap();
        assert_eq!((patch.width, patch.rows), (3, 2));
        assert_eq!(patch.heights.len(), 6 * 4);
        assert_eq!(patch.owners.len(), 6 * 2);
        assert_eq!(patch.classes.len(), 6);
        assert_eq!(patch.gradients.len(), 6 * 8);
        let height = |k: usize| {
            let mut four = [0u8; 4];
            four.copy_from_slice(&patch.heights[k * 4..k * 4 + 4]);
            f32::from_ne_bytes(four)
        };
        let owner = |k: usize| {
            let mut two = [0u8; 2];
            two.copy_from_slice(&patch.owners[k * 2..k * 2 + 2]);
            u16::from_ne_bytes(two)
        };
        // Row 1 is uncut; row 2 is the slot, cut by run 7.
        for k in 0..3 {
            assert!((height(k) - 10.0).abs() < 1e-6, "cell {k}");
            assert_eq!(owner(k), u16::MAX);
            assert!((height(3 + k) - 7.0).abs() < 1e-6, "cell {}", 3 + k);
            assert_eq!(owner(3 + k), 7);
        }
        // The flagged cell is (3, 2): the second of the patch's row 2.
        let flagged: Vec<bool> = patch.classes.iter().map(|c| c & UNCUT_BIT != 0).collect();
        assert_eq!(flagged, [false, false, false, false, true, false]);

        // Nothing for an empty rectangle, or a shading of another grid.
        let empty = CellRect {
            i0: 4,
            i1: 4,
            j0: 0,
            j1: 6,
        };
        assert_eq!(Patch::of(&field, &shading, None, empty), None);
        assert_eq!(Patch::of(&field, &Shading::default(), None, rect), None);
    }

    /// The shader's bits are the app's.
    #[test]
    fn the_class_bits_match_the_shader() {
        let source = include_str!("stock.wgsl");
        for (name, bit) in [
            ("EAST_STEP", dc_app::sim::EAST_STEP),
            ("NORTH_STEP", dc_app::sim::NORTH_STEP),
            ("UNCUT", UNCUT_BIT),
        ] {
            assert!(
                source.contains(&format!("const {name}: u32 = {bit}u;")),
                "{name}"
            );
        }
        assert!(source.contains("const NO_OWNER: u32 = 65535u;"));
    }
}
