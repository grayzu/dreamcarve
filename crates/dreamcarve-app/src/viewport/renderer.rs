// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Viewport's wgpu line renderer (`docs/specs/phase-4/design.md`,
//! "Architecture → The Viewport renderer"): an `egui_wgpu` paint callback
//! drawing line segments kept in project millimetres, one storage buffer
//! per [`Layer`], back to front. `shader.wgsl` expands each segment into a
//! screen-space quad of its style's width, so a pan or zoom writes only
//! the 32-byte camera uniform; a layer's buffer is written again only
//! when its contents change (a new Scene, a new zoom band for the arcs, a
//! new hover or selection), and the style table only when the theme does.
//!
//! The callback carries every layer's data each frame as an `Arc`, with a
//! version; `prepare` uploads a layer only when its version differs from
//! the one on the GPU. A frame egui discards, or one a test harness never
//! renders, so loses nothing.
//!
//! Geometry is `f64` up to here; `f32` appears only in the bytes written to
//! the GPU (`docs/rust-guidelines.md`, "Types").

use std::sync::Arc;

use eframe::egui::{self, Color32};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait, ScreenDescriptor, wgpu};

/// The layers, drawn in this order (design, "The Viewport renderer";
/// `experience.md` section 9: hover under selection). The male board
/// (slice 14) goes after `Stock`, bitmaps (slice 15) before `Shapes`,
/// toolpaths and their marks (slice 16) after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// The grid's minor and major lines and the axes through the origin.
    Grid,
    /// The stock's dashed outline.
    Stock,
    /// Every visible shape.
    Shapes,
    /// The shape under the pointer.
    Hover,
    /// The selected shapes.
    Selection,
}

impl Layer {
    /// Every layer, back to front.
    pub const ALL: [Self; 5] = [
        Self::Grid,
        Self::Stock,
        Self::Shapes,
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
}

/// The style table's rows, in [`StyleId`] order.
pub const STYLE_COUNT: usize = 12;

/// One row of the style table (`experience.md`, section 9, "Line
/// weights"), in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// The stroke's colour, from the theme.
    pub colour: Color32,
    /// Width, logical pixels.
    pub width_pt: f32,
    /// Dash and gap, logical pixels; `None` is solid.
    pub dash_pt: Option<(f32, f32)>,
    /// Multiplied by the grid's minor-line strength.
    pub fades: bool,
}

/// The style table of `palette`, in [`StyleId`] order.
#[must_use]
pub fn styles(palette: &crate::theme::Palette) -> [Style; STYLE_COUNT] {
    let solid = |colour, width_pt| Style {
        colour,
        width_pt,
        dash_pt: None,
        fades: false,
    };
    let dashed = |colour, width_pt, on, off| Style {
        colour,
        width_pt,
        dash_pt: Some((on, off)),
        fades: false,
    };
    [
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
    ]
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
    /// Its start, mm.
    pub a_mm: [f32; 2],
    /// Its end, mm.
    pub b_mm: [f32; 2],
    /// How far along its outline it starts, mm, for dashes.
    pub along_mm: f32,
    /// Its style.
    pub style: StyleId,
}

/// The bytes of one segment in the storage buffer.
const SEGMENT_BYTES: usize = 24;
/// The bytes of one style row.
const STYLE_BYTES: usize = 32;
/// The camera uniform's bytes.
const CAMERA_BYTES: usize = 32;

/// A layer's contents ready to upload.
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
            bytes.extend_from_slice(&(s.style as u32).to_ne_bytes());
        }
        Self {
            count: u32::try_from(segments.len()).unwrap_or(u32::MAX),
            bytes,
        }
    }
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
    /// The style table and its version (the theme's).
    pub styles: (u64, Arc<[Style; STYLE_COUNT]>),
    /// Each layer's contents and version, in [`Layer::ALL`] order.
    pub layers: [(u64, Arc<LayerData>); Layer::ALL.len()],
}

/// One layer on the GPU.
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
    globals: wgpu::BindGroup,
    styles_version: Option<u64>,
    /// Whether the framebuffer is sRGB, so colours are written linear.
    linear_output: bool,
    layers: Vec<GpuLayer>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renderer")
            .field("linear_output", &self.linear_output)
            .finish_non_exhaustive()
    }
}

/// One sRGB channel, 0 to 1, as linear light.
fn linear(gamma: f32) -> f32 {
    if gamma <= 0.04045 {
        gamma / 12.92
    } else {
        ((gamma + 0.055) / 1.055).powf(2.4)
    }
}

impl Renderer {
    /// The pipeline for a framebuffer of `format`, with empty layers.
    #[must_use]
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewport lines"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
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
            label: Some("viewport camera and styles"),
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
            (STYLE_COUNT * STYLE_BYTES) as u64,
            wgpu::BufferUsages::STORAGE,
        );
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport camera and styles"),
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
            ],
        });
        let mut renderer = Self {
            pipeline,
            layer_layout,
            camera,
            styles,
            globals,
            styles_version: None,
            linear_output: format.is_srgb(),
            layers: Vec::with_capacity(Layer::ALL.len()),
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

    /// The style table's bytes, in the framebuffer's colour space.
    fn style_bytes(&self, styles: &[Style; STYLE_COUNT]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(STYLE_COUNT * STYLE_BYTES);
        for style in styles {
            let [r, g, b, a] = style.colour.to_normalized_gamma_f32();
            let rgba = if self.linear_output && a > 0.0 {
                // Unpremultiply, linearise, premultiply again.
                let lin = |c: f32| linear(c / a) * a;
                [lin(r), lin(g), lin(b), a]
            } else {
                [r, g, b, a]
            };
            let (on, off) = style.dash_pt.unwrap_or((0.0, 0.0));
            for v in rgba.into_iter().chain([style.width_pt, on, off]) {
                bytes.extend_from_slice(&v.to_ne_bytes());
            }
            let flags = u32::from(style.dash_pt.is_some()) | (u32::from(style.fades) << 1);
            bytes.extend_from_slice(&flags.to_ne_bytes());
        }
        bytes
    }

    /// Writes `frame`'s camera, and its styles and layers where their
    /// version changed, for a pane of `pane_pt` logical pixels drawn at
    /// `pixels_per_point`.
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
            0.0,
        ];
        let bytes: Vec<u8> = camera.iter().flat_map(|v| v.to_ne_bytes()).collect();
        queue.write_buffer(&self.camera, 0, &bytes);

        let (version, styles) = &frame.styles;
        if self.styles_version != Some(*version) {
            queue.write_buffer(&self.styles, 0, &self.style_bytes(styles));
            self.styles_version = Some(*version);
        }
        for (slot, (version, data)) in frame.layers.iter().enumerate() {
            let Some(current) = self.layers.get(slot).map(|l| l.version) else {
                continue;
            };
            if current == Some(*version) {
                continue;
            }
            let needed = data.bytes.len() as u64;
            let capacity = self.layers.get(slot).map_or(0, |l| l.capacity);
            if needed > capacity {
                let grown = self.layer(device, needed.next_power_of_two());
                if let Some(layer) = self.layers.get_mut(slot) {
                    *layer = grown;
                }
            }
            if let Some(layer) = self.layers.get_mut(slot) {
                if !data.bytes.is_empty() {
                    queue.write_buffer(&layer.buffer, 0, &data.bytes);
                }
                layer.count = data.count;
                layer.version = Some(*version);
            }
        }
    }

    /// Draws every non-empty layer, back to front, into `pass`.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.styles_version.is_none() {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.globals, &[]);
        for layer in self.layers.iter().filter(|l| l.count > 0) {
            pass.set_bind_group(1, &layer.bind_group, &[]);
            pass.draw(0..6, 0..layer.count);
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
