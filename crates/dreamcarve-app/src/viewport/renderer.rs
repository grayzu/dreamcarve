// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport's wgpu line renderer (`docs/specs/phase-4/design.md`,
//! "Architecture → The Viewport renderer"): an `egui_wgpu` paint callback
//! drawing line segments kept in project millimetres, one storage buffer
//! per [`Layer`] and one per operation's toolpath, back to front.
//! `shader.wgsl` expands each segment into a screen-space quad of its
//! style's width, so a pan or zoom writes only the 32-byte camera
//! uniform; a buffer is written again only when its contents change (a
//! new Scene, a new zoom band for the arcs, a new hover or selection), and
//! the style table and the depth ramp only when the theme does.
//!
//! Each segment carries a style word ([`StyleWord`]): its row of the style
//! table (width, dashes, a ring), and for a toolpath its operation's
//! colour, its depth step for *By depth* and whether it is stale. The
//! shader picks the colour: the row's, else the operation's, else, when
//! the camera uniform says *By depth* and the segment cuts, the ramp
//! texture's texel at its step; a stale segment draws at 35 % alpha
//! (`experience.md`, section 13, item 18). Switching the colour mode so
//! writes only the camera.
//!
//! An operation's toolpath is one buffer, so its eye is one draw call. The
//! callback carries every buffer's data each frame as an `Arc`, with a
//! version; `prepare` uploads one only when its version differs from the
//! one on the GPU. A frame egui discards, or one a test harness never
//! renders, so loses nothing.
//!
//! Geometry is `f64` up to here; `f32` appears only in the bytes written to
//! the GPU (`docs/rust-guidelines.md`, "Types").

use std::collections::BTreeMap;
use std::sync::Arc;

use dc_app::view::DEPTH_STEPS;
use eframe::egui::{self, Color32};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};

use crate::theme::{gamma, linear, ramp_linear};

/// The fixed layers, drawn in this order (design, "The Viewport
/// renderer"; `experience.md` section 9: toolpaths over the shapes, hover
/// under selection). The operations' toolpaths draw after `Shapes`, in
/// the order the frame lists them. The male board (slice 14) goes after
/// `Stock`, bitmaps (slice 15) before `Shapes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// The grid's minor and major lines and the axes through the origin.
    Grid,
    /// The stock's dashed outline.
    Stock,
    // TODO(#544): `Bitmaps`, a textured quad per picture at its footprint,
    // under the shapes (functional spec, capability 56). It waits for
    // phase 3.4's `dc_project::Bitmap` (format 26); main is at format 25,
    // so slice 6 (#535) builds the drawing half only.
    /// Every visible shape.
    Shapes,
    /// The direction arrows of every drawn toolpath.
    Arrows,
    /// The program extents boxes.
    Extents,
    /// The shape under the pointer.
    Hover,
    /// The selected shapes.
    Selection,
}

impl Layer {
    /// Every layer, back to front.
    pub const ALL: [Self; 7] = [
        Self::Grid,
        Self::Stock,
        Self::Shapes,
        Self::Arrows,
        Self::Extents,
        Self::Hover,
        Self::Selection,
    ];

    /// The layer's slot in [`Layer::ALL`] and in [`Frame::layers`].
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// A stroke style: the row of the style table a segment names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum StyleId {
    /// The grid's minor lines, faded with the zoom.
    GridMinor,
    /// The grid's major lines.
    GridMajor,
    /// The axes through the origin.
    Axis,
    /// The stock's outline, dashed 6/4.
    Stock,
    /// A closed shape.
    Shape,
    /// An open shape, dashed 4/3.
    ShapeOpen,
    /// A locked closed shape.
    Locked,
    /// A locked open shape.
    LockedOpen,
    /// The closed shape under the pointer.
    Hover,
    /// The open shape under the pointer.
    HoverOpen,
    /// A selected closed shape.
    Selected,
    /// A selected open shape.
    SelectedOpen,
    /// A rapid: 1 px, dashed 4/3, grey.
    Rapid,
    /// A cutting move: 1.5 px solid, in its operation's colour.
    Cut,
    /// A plunge: a 6-px ring.
    Plunge,
    /// A tab mark: 4 px.
    Tab,
    /// A start point's or a drill hole's circle: the bit's size, never
    /// under 8 px across, in its operation's colour.
    Mark,
    /// A start point's tick, and a direction arrow's chevron.
    Tick,
    /// The extents' tool-centre box, 1 px solid.
    ExtentsCentre,
    /// The extents' bit-edge box, 1 px dashed 6/4.
    ExtentsEdge,
    /// The male inlay board's outline, 1.25 px dashed 6/4 (slice 14).
    MaleBoard,
    /// A shape mirrored on the male inlay board, 1.25 px solid.
    MaleShape,
}

/// The style table's rows, in [`StyleId`] order. The operation colours
/// follow them in the buffer, eight rows from here (`shader.wgsl`'s
/// `OP_BASE`).
pub const STYLE_COUNT: usize = 22;
/// The operation colour rows after the styles.
pub const OPERATION_COLOURS: usize = 8;

/// A segment's style word: its [`StyleId`] in bits 0–7; for a toolpath,
/// its depth step in bits 8–15 with bit 16 set when it cuts (so *By
/// depth* colours it), bit 17 when it is stale, and its operation's
/// colour as index + 1 in bits 18–21 (0: the row's own colour).
/// `shader.wgsl` decodes the same bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StyleWord(u32);

impl StyleWord {
    /// A toolpath segment of row `style` in operation colour `colour`
    /// (the preview's run index; the palette repeats after eight), at
    /// depth step `depth` when it cuts, faded when `stale`.
    #[must_use]
    pub fn toolpath(style: StyleId, colour: Option<usize>, depth: Option<u8>, stale: bool) -> Self {
        let mut word = style as u32;
        if let Some(step) = depth {
            word |= (u32::from(step) << 8) | (1 << 16);
        }
        if stale {
            word |= 1 << 17;
        }
        if let Some(k) = colour {
            let slot = u32::try_from(k % OPERATION_COLOURS).unwrap_or(0) + 1;
            word |= slot << 18;
        }
        Self(word)
    }

    /// The word as the shader reads it.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// The row it names.
    #[cfg(test)]
    #[must_use]
    pub const fn row(self) -> u32 {
        self.0 & 0xFF
    }

    /// Whether it is drawn faded.
    #[cfg(test)]
    #[must_use]
    pub const fn stale(self) -> bool {
        self.0 & (1 << 17) != 0
    }
}

impl From<StyleId> for StyleWord {
    fn from(style: StyleId) -> Self {
        Self(style as u32)
    }
}

/// One row of the style table (`experience.md`, section 9, "Line
/// weights"), in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The stroke's colour, from the theme; an operation colour in the
    /// style word replaces it.
    pub colour: Color32,
    /// Width, logical pixels.
    pub width_pt: f32,
    /// Dash and gap, logical pixels; `None` is solid.
    pub dash_pt: Option<(f32, f32)>,
    /// Multiplied by the grid's minor-line strength.
    pub fades: bool,
    /// A ring about the segment's start, not a line: its radius is the
    /// segment's `along_mm` in millimetres, but never under this many
    /// logical pixels.
    pub ring_min_radius_pt: Option<f32>,
}

/// The style table, the operation colours and the depth ramp of one
/// theme: what the shader reads besides the segments.
#[derive(Debug, Clone, PartialEq)]
pub struct StyleTable {
    /// The rows, in [`StyleId`] order.
    pub rows: [Style; STYLE_COUNT],
    /// The operation colours.
    pub operations: [Color32; OPERATION_COLOURS],
    /// The depth ramp's five stops, stock top first.
    pub ramp: [Color32; 5],
}

/// A start point's or drill hole's least radius, logical pixels: 8 px
/// across (`experience.md`, section 9).
const MARK_MIN_RADIUS_PT: f32 = 4.0;
/// A plunge mark's radius, logical pixels: a 6-px circle.
const PLUNGE_RADIUS_PT: f32 = 3.0;

/// The style table of `palette`, in [`StyleId`] order.
#[must_use]
pub fn styles(palette: &crate::theme::Palette) -> StyleTable {
    let solid = |colour, width_pt| Style {
        colour,
        width_pt,
        dash_pt: None,
        fades: false,
        ring_min_radius_pt: None,
    };
    let dashed = |colour, width_pt, on, off| Style {
        dash_pt: Some((on, off)),
        ..solid(colour, width_pt)
    };
    let ring = |colour, width_pt, radius| Style {
        ring_min_radius_pt: Some(radius),
        ..solid(colour, width_pt)
    };
    // Rows coloured per operation take the operation's colour from the
    // style word; their own is the shape colour, never seen.
    let per_op = palette.viewport_shape;
    StyleTable {
        rows: [
            Style {
                fades: true,
                ..solid(palette.viewport_grid_minor, 1.0)
            },
            solid(palette.viewport_grid_major, 1.0),
            solid(palette.viewport_axis, 1.0),
            dashed(palette.viewport_stock, 1.5, 6.0, 4.0),
            solid(palette.viewport_shape, 1.25),
            dashed(palette.viewport_shape, 1.25, 4.0, 3.0),
            solid(palette.viewport_shape_locked, 1.25),
            dashed(palette.viewport_shape_locked, 1.25, 4.0, 3.0),
            solid(palette.viewport_shape_hover, 1.25),
            dashed(palette.viewport_shape_hover, 1.25, 4.0, 3.0),
            solid(palette.viewport_shape_selected, 2.0),
            dashed(palette.viewport_shape_selected, 2.0, 4.0, 3.0),
            dashed(palette.viewport_rapid, 1.0, 4.0, 3.0),
            solid(per_op, 1.5),
            ring(palette.viewport_plunge, 1.0, PLUNGE_RADIUS_PT),
            solid(palette.viewport_tab, 4.0),
            ring(per_op, 1.25, MARK_MIN_RADIUS_PT),
            solid(per_op, 1.25),
            solid(palette.viewport_extents_centre, 1.0),
            dashed(palette.viewport_extents_edge, 1.0, 6.0, 4.0),
            dashed(palette.viewport_male_board, 1.25, 6.0, 4.0),
            solid(palette.viewport_male_board, 1.25),
        ],
        operations: std::array::from_fn(|k| palette.operation(k)),
        ramp: palette.depth_ramp,
    }
}

/// `x` as the GPU's `f32`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "project coordinates under 6 m and screen sizes: f32 keeps them to a micron"
)]
pub fn gpu_f32(x: f64) -> f32 {
    x as f32
}

/// One segment as `shader.wgsl` reads it, project millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Its start, mm; a ring's centre.
    pub a_mm: [f32; 2],
    /// Its end, mm; a ring's centre again.
    pub b_mm: [f32; 2],
    /// How far along its outline it starts, mm, for dashes; a ring's
    /// radius.
    pub along_mm: f32,
    /// Its style.
    pub style: StyleWord,
}

/// The bytes of one segment in the storage buffer.
const SEGMENT_BYTES: usize = 24;
/// The bytes of one style row.
const STYLE_BYTES: usize = 32;
/// The camera uniform's bytes.
const CAMERA_BYTES: usize = 32;
/// The ramp texture's format: four `f32`s a texel, read by `textureLoad`,
/// so dark steps keep their precision in linear light.
const RAMP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
/// The bytes of one ramp texel.
const RAMP_TEXEL_BYTES: u32 = 16;

/// A buffer's contents ready to upload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerData {
    bytes: Vec<u8>,
    count: u32,
}

impl LayerData {
    /// `segments` as the shader's storage buffer reads them.
    #[must_use]
    pub fn new(segments: &[Segment]) -> Self {
        let mut bytes = Vec::with_capacity(segments.len() * SEGMENT_BYTES);
        for s in segments {
            for v in [s.a_mm[0], s.a_mm[1], s.b_mm[0], s.b_mm[1], s.along_mm] {
                bytes.extend_from_slice(&v.to_ne_bytes());
            }
            bytes.extend_from_slice(&s.style.bits().to_ne_bytes());
        }
        Self {
            count: u32::try_from(segments.len()).unwrap_or(u32::MAX),
            bytes,
        }
    }
}

/// One operation's toolpath buffer in a frame.
#[derive(Debug, Clone)]
pub struct ToolpathData {
    /// Which operation's: stable while the operation exists.
    pub key: u64,
    /// The version of `data`.
    pub version: u64,
    /// The segments.
    pub data: Arc<LayerData>,
}

/// What the GPU needs for one frame of the Viewport.
#[derive(Debug, Clone)]
pub struct Frame {
    /// The project point at the pane's centre, mm.
    pub centre_mm: [f32; 2],
    /// Logical pixels per millimetre.
    pub px_per_mm: f32,
    /// The grid's minor-line strength, 0 to 1.
    pub minor_alpha: f32,
    /// Colour cutting moves by depth.
    pub by_depth: bool,
    /// The style table and its version (the theme's).
    pub styles: (u64, Arc<StyleTable>),
    /// Each fixed layer's contents and version, in [`Layer::ALL`] order.
    pub layers: [(u64, Arc<LayerData>); Layer::ALL.len()],
    /// The drawn operations' toolpaths, bottom first, drawn after
    /// [`Layer::Shapes`].
    pub toolpaths: Vec<ToolpathData>,
}

/// One buffer on the GPU.
struct GpuLayer {
    buffer: wgpu::Buffer,
    capacity: u64,
    bind_group: wgpu::BindGroup,
    count: u32,
    version: Option<u64>,
}

/// The pipeline and buffers, kept in egui's callback resources.
pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    layer_layout: wgpu::BindGroupLayout,
    camera: wgpu::Buffer,
    styles: wgpu::Buffer,
    ramp: wgpu::Texture,
    globals: wgpu::BindGroup,
    styles_version: Option<u64>,
    /// Whether the framebuffer is sRGB, so colours are written linear.
    linear_output: bool,
    layers: Vec<GpuLayer>,
    toolpaths: BTreeMap<u64, GpuLayer>,
    /// This frame's toolpath keys, bottom first.
    order: Vec<u64>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("linear_output", &self.linear_output)
            .finish_non_exhaustive()
    }
}

impl Renderer {
    /// The pipeline for a framebuffer of `format`, with empty layers.
    #[must_use]
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let source = include_str!("shader.wgsl").replace(
            "const OP_BASE: u32 = 0u;",
            &format!("const OP_BASE: u32 = {STYLE_COUNT}u;"),
        );
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport lines"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let storage = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let both = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport camera, styles and ramp"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: both,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1, both),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let layer_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport layer"),
            entries: &[storage(0, wgpu::ShaderStages::VERTEX)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport lines"),
            bind_group_layouts: &[Some(&globals_layout), Some(&layer_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport lines"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let buffer = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let camera = buffer(
            "viewport camera",
            CAMERA_BYTES as u64,
            wgpu::BufferUsages::UNIFORM,
        );
        let styles = buffer(
            "viewport styles",
            ((STYLE_COUNT + OPERATION_COLOURS) * STYLE_BYTES) as u64,
            wgpu::BufferUsages::STORAGE,
        );
        let ramp = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("viewport depth ramp"),
            size: wgpu::Extent3d {
                width: u32::try_from(DEPTH_STEPS).unwrap_or(256),
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: RAMP_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let ramp_view = ramp.create_view(&wgpu::TextureViewDescriptor::default());
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport camera, styles and ramp"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: styles.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&ramp_view),
                },
            ],
        });
        let mut renderer = Self {
            pipeline,
            layer_layout,
            camera,
            styles,
            ramp,
            globals,
            styles_version: None,
            linear_output: format.is_srgb(),
            layers: Vec::with_capacity(Layer::ALL.len()),
            toolpaths: BTreeMap::new(),
            order: Vec::new(),
        };
        for _ in Layer::ALL {
            let layer = renderer.layer(device, SEGMENT_BYTES as u64);
            renderer.layers.push(layer);
        }
        renderer
    }

    /// An empty layer buffer of `capacity` bytes.
    fn layer(&self, device: &wgpu::Device, capacity: u64) -> GpuLayer {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport layer"),
            size: capacity,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport layer"),
            layout: &self.layer_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        GpuLayer {
            buffer,
            capacity,
            bind_group,
            count: 0,
            version: None,
        }
    }

    /// `colour` as four floats in the framebuffer's colour space,
    /// premultiplied.
    fn output_colour(&self, colour: Color32) -> [f32; 4] {
        let [r, g, b, a] = colour.to_normalized_gamma_f32();
        if self.linear_output && a > 0.0 {
            // Unpremultiply, linearise, premultiply again.
            let lin = |c: f32| linear(c / a) * a;
            [lin(r), lin(g), lin(b), a]
        } else {
            [r, g, b, a]
        }
    }

    /// The style table's bytes, then the operation colours as rows.
    fn style_bytes(&self, table: &StyleTable) -> Vec<u8> {
        let mut bytes = Vec::with_capacity((STYLE_COUNT + OPERATION_COLOURS) * STYLE_BYTES);
        let rows = table
            .rows
            .iter()
            .copied()
            .chain(table.operations.iter().map(|c| Style {
                colour: *c,
                width_pt: 0.0,
                dash_pt: None,
                fades: false,
                ring_min_radius_pt: None,
            }));
        for style in rows {
            let rgba = self.output_colour(style.colour);
            let (on, off) = style
                .ring_min_radius_pt
                .map(|r| (r, 0.0))
                .or(style.dash_pt)
                .unwrap_or((0.0, 0.0));
            for v in rgba.into_iter().chain([style.width_pt, on, off]) {
                bytes.extend_from_slice(&v.to_ne_bytes());
            }
            let flags = u32::from(style.dash_pt.is_some())
                | (u32::from(style.fades) << 1)
                | (u32::from(style.ring_min_radius_pt.is_some()) << 2);
            bytes.extend_from_slice(&flags.to_ne_bytes());
        }
        bytes
    }

    /// The ramp texture's bytes: [`DEPTH_STEPS`] opaque texels from the
    /// stock top to the deepest cut, in the framebuffer's colour space.
    fn ramp_bytes(&self, stops: &[Color32; 5]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(DEPTH_STEPS * 16);
        for step in 0..DEPTH_STEPS {
            #[allow(clippy::cast_precision_loss, reason = "under 256 steps")]
            let t = step as f32 / (DEPTH_STEPS - 1) as f32;
            let [r, g, b] = ramp_linear(stops, t);
            let rgb = if self.linear_output {
                [r, g, b]
            } else {
                [gamma(r), gamma(g), gamma(b)]
            };
            for v in rgb.into_iter().chain([1.0]) {
                bytes.extend_from_slice(&v.to_ne_bytes());
            }
        }
        bytes
    }

    /// Writes `data` into `layer` at `version`, growing it if needed.
    fn write_layer(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &mut GpuLayer,
        version: u64,
        data: &LayerData,
    ) {
        if layer.version == Some(version) {
            return;
        }
        let needed = data.bytes.len() as u64;
        if needed > layer.capacity {
            *layer = self.layer(device, needed.next_power_of_two());
        }
        if !data.bytes.is_empty() {
            queue.write_buffer(&layer.buffer, 0, &data.bytes);
        }
        layer.count = data.count;
        layer.version = Some(version);
    }

    /// Writes `frame`'s camera, and its styles and buffers where their
    /// version changed, for a pane of `pane_pt` logical pixels drawn at
    /// `pixels_per_point`. Toolpath buffers the frame no longer lists are
    /// dropped.
    pub fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &Frame,
        pane_pt: [f32; 2],
        pixels_per_point: f32,
    ) {
        let camera = [
            frame.centre_mm[0],
            frame.centre_mm[1],
            pane_pt[0] * pixels_per_point,
            pane_pt[1] * pixels_per_point,
            frame.px_per_mm * pixels_per_point,
            pixels_per_point,
            frame.minor_alpha,
            if frame.by_depth { 1.0 } else { 0.0 },
        ];
        let bytes: Vec<u8> = camera.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&self.camera, 0, &bytes);

        let (version, table) = &frame.styles;
        if self.styles_version != Some(*version) {
            queue.write_buffer(&self.styles, 0, &self.style_bytes(table));
            let width = u32::try_from(DEPTH_STEPS).unwrap_or(256);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.ramp,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &self.ramp_bytes(&table.ramp),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * RAMP_TEXEL_BYTES),
                    rows_per_image: Some(1),
                },
                wgpu::Extent3d {
                    width,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            self.styles_version = Some(*version);
        }
        let mut layers = std::mem::take(&mut self.layers);
        for (layer, (version, data)) in layers.iter_mut().zip(&frame.layers) {
            self.write_layer(device, queue, layer, *version, data);
        }
        self.layers = layers;

        let mut toolpaths = std::mem::take(&mut self.toolpaths);
        toolpaths.retain(|key, _| frame.toolpaths.iter().any(|t| t.key == *key));
        for t in &frame.toolpaths {
            let layer = toolpaths
                .entry(t.key)
                .or_insert_with(|| self.layer(device, SEGMENT_BYTES as u64));
            self.write_layer(device, queue, layer, t.version, &t.data);
        }
        self.toolpaths = toolpaths;
        self.order = frame.toolpaths.iter().map(|t| t.key).collect();
    }

    /// Draws every non-empty buffer, back to front, into `pass`: the
    /// fixed layers through [`Layer::Shapes`], the toolpaths in the
    /// frame's order, then the rest.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.styles_version.is_none() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals, &[]);
        let draw = |pass: &mut wgpu::RenderPass<'_>, layer: &GpuLayer| {
            if layer.count > 0 {
                pass.set_bind_group(1, &layer.bind_group, &[]);
                pass.draw(0..6, 0..layer.count);
            }
        };
        for (slot, layer) in self.layers.iter().enumerate() {
            draw(pass, layer);
            if slot == Layer::Shapes.index() {
                for key in &self.order {
                    if let Some(layer) = self.toolpaths.get(key) {
                        draw(pass, layer);
                    }
                }
            }
        }
    }
}

/// Installs the renderer in eframe's wgpu state, so the Viewport's
/// callback finds it. Without a wgpu state (egui's CPU test harness) the
/// callback draws nothing.
pub fn install(render_state: &egui_wgpu::RenderState) {
    let renderer = Renderer::new(&render_state.device, render_state.target_format);
    render_state
        .renderer
        .write()
        .callback_resources
        .insert(renderer);
}

/// The paint callback of one frame.
struct Callback {
    frame: Frame,
    pane_pt: [f32; 2],
}

impl CallbackTrait for Callback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<Renderer>() {
            renderer.upload(
                device,
                queue,
                &self.frame,
                self.pane_pt,
                screen.pixels_per_point,
            );
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        if let Some(renderer) = resources.get::<Renderer>() {
            renderer.draw(pass);
        }
    }
}

/// The paint callback drawing `frame` over `pane`.
#[must_use]
pub fn callback(pane: egui::Rect, frame: Frame) -> egui::PaintCallback {
    egui_wgpu::Callback::new_paint_callback(
        pane,
        Callback {
            frame,
            pane_pt: [pane.width(), pane.height()],
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shader's operation colours start where the Rust table ends:
    /// `Renderer::new` writes `STYLE_COUNT` into the placeholder.
    #[test]
    fn shader_reads_the_operation_colours_after_the_styles() {
        let source = include_str!("shader.wgsl");
        assert!(source.contains("const OP_BASE: u32 = 0u;"));
        assert_eq!(StyleId::MaleShape as usize + 1, STYLE_COUNT);
    }

    /// A style word keeps its parts apart: the row, the depth step, the
    /// stale flag and the operation colour, which repeats after eight.
    #[test]
    fn style_words_pack_row_depth_stale_and_colour() {
        let word = StyleWord::toolpath(StyleId::Cut, Some(9), Some(200), true);
        assert_eq!(word.row(), StyleId::Cut as u32);
        assert_eq!((word.bits() >> 8) & 0xFF, 200);
        assert_eq!((word.bits() >> 16) & 1, 1);
        assert!(word.stale());
        assert_eq!((word.bits() >> 18) & 0xF, 2, "run 9 takes colour 2");
        let plain = StyleWord::from(StyleId::Shape);
        assert_eq!(plain.bits(), StyleId::Shape as u32);
        assert!(!plain.stale());
    }
}
