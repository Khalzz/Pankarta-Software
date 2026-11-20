//! Draws the gizmos (see rendering::gizmos) - the GPU's side.
//!
//! Every frame `App::prepare_gizmos` turns what's up (`app.gizmos`) into
//! instances - each line shape into segments, each icon into a square on
//! the icon atlas, each text and label into screen-space SDF text - and
//! `App::render_gizmos` draws them, once, at the frame plan's first UI step
//! (under the UI, over everything else):
//! - the ones the world can hide: faint where it does (the depth test
//!   turned round), then plain where it doesn't;
//! - the ones on top, then the text, with no depth test.
//!
//! Drawing into one of the game's own images (a `Target` step) there's no
//! depth of the right size - all of them are on top there.

use nalgebra::{Matrix4, Vector2, Vector3, Vector4};

use crate::app::App;
use crate::rendering::gizmos::gizmos::{GizmoColor, GizmoParts, LineGizmo, Shape};
use crate::rendering::gizmos::icons::{ATLAS_SIZE, FIELD_SIZE, REF_SIZE};
use crate::rendering::models::textures::Texture;
use crate::rendering::render_pipeline::render_pass::{color_attachment, depth_attachment};
use crate::rendering::renderer::Renderer;
use crate::rendering::sdf_text::{srgb_to_linear, SdfText, TextFrame};
use crate::rendering::shader_library::Overlay;
use crate::ui::components::label::Font;

/// The material the gizmos' text is drawn with (see App::set_up_materials).
pub(crate) const GIZMO_TEXT: &str = "gizmo_text";

/// A dashed line's dashes (px) - the gaps are 3/4 of that.
const DASH_PIXELS: f32 = 8.0;
/// How many segments make a circle.
const CIRCLE_SEGMENTS: usize = 48;
/// An arrowhead's sides: how long (px, at least - wider lines get bigger
/// heads) and how far out from the line.
const ARROW_PIXELS: f32 = 12.0;
const ARROW_ANGLE: f32 = 0.45;

/// One segment - see gizmo_lines.wgsl's LineInstance.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct LineInstance {
    start: [f32; 3],
    end: [f32; 3],
    start_color: [f32; 4],
    end_color: [f32; 4],
    params: [f32; 4],
}

impl LineInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x4, 4 => Float32x4];
}

/// One icon - see gizmo_icons.wgsl's IconInstance.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct IconInstance {
    position: [f32; 3],
    size: f32,
    color: [f32; 4],
    uv: [f32; 4],
}

impl IconInstance {
    const ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32, 2 => Float32x4, 3 => Float32x4];
}

/// See gizmo_lines.wgsl's Gizmo.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GizmoUniform {
    camera_position: [f32; 4],
    screen: [f32; 4],
}

/// One kind's three ways of being drawn.
struct Pipelines {
    /// Where the world hides it - faint.
    hidden: wgpu::RenderPipeline,
    /// Where it doesn't.
    visible: wgpu::RenderPipeline,
    /// Over everything, in a pass with no depth.
    on_top: wgpu::RenderPipeline,
}

/// Instances of one kind for this frame: the ones the world can hide first
/// (`hideable` of them), then the ones on top - and the GPU buffer they go
/// in, kept from frame to frame and only made bigger.
struct Batch<T> {
    instances: Vec<T>,
    hideable: u32,
    buffer: Option<(wgpu::Buffer, usize)>,
}

impl<T: bytemuck::Pod> Batch<T> {
    fn new() -> Self {
        Self { instances: Vec::new(), hideable: 0, buffer: None }
    }

    fn fill(&mut self, hideable: Vec<T>, on_top: Vec<T>) {
        self.instances.clear();
        self.hideable = hideable.len() as u32;
        self.instances.extend(hideable);
        self.instances.extend(on_top);
    }

    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, label: &str) {
        if self.instances.is_empty() {
            return;
        }
        let needed = self.instances.len();
        if self.buffer.as_ref().is_none_or(|(_, capacity)| *capacity < needed) {
            let capacity = needed.next_power_of_two();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: (capacity * std::mem::size_of::<T>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.buffer = Some((buffer, capacity));
        }
        if let Some((buffer, _)) = &self.buffer {
            queue.write_buffer(buffer, 0, bytemuck::cast_slice(&self.instances));
        }
    }

    fn all(&self) -> std::ops::Range<u32> {
        0..self.instances.len() as u32
    }

    fn hideable(&self) -> std::ops::Range<u32> {
        0..self.hideable
    }

    fn on_top(&self) -> std::ops::Range<u32> {
        self.hideable..self.instances.len() as u32
    }

    fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline, range: std::ops::Range<u32>) {
        let Some((buffer, _)) = &self.buffer else { return };
        if range.is_empty() {
            return;
        }
        render_pass.set_pipeline(pipeline);
        render_pass.set_vertex_buffer(0, buffer.slice(..));
        render_pass.draw(0..6, range);
    }
}

pub(crate) struct GizmoRenderer {
    lines: Pipelines,
    icons: Pipelines,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    atlas: wgpu::Texture,
    line_batch: Batch<LineInstance>,
    icon_batch: Batch<IconInstance>,
    /// The text and labels, in screen pixels - made the first time there are
    /// any (it has its own glyph atlas).
    text: Option<SdfText>,
    /// Drawn already this frame - only the first UI step draws them.
    drawn: bool,
}

impl GizmoRenderer {
    pub(crate) fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat, camera_layout: &wgpu::BindGroupLayout) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Gizmo layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Gizmo uniform"),
            size: std::mem::size_of::<GizmoUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Gizmo icon atlas"),
            size: wgpu::Extent3d { width: ATLAS_SIZE, height: ATLAS_SIZE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Gizmo icon sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Gizmo bind group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&atlas.create_view(&wgpu::TextureViewDescriptor::default())) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Gizmo pipeline layout"),
            bind_group_layouts: &[camera_layout, &layout],
            push_constant_ranges: &[],
        });

        let lines_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Gizmo lines shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/debug/gizmo_lines.wgsl").into()),
        });
        let icons_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Gizmo icons shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/debug/gizmo_icons.wgsl").into()),
        });
        let line_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<LineInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &LineInstance::ATTRIBUTES,
        };
        let icon_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<IconInstance>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &IconInstance::ATTRIBUTES,
        };
        let lines = Pipelines::new(device, &pipeline_layout, &lines_shader, line_layout, color_format, "Gizmo lines");
        let icons = Pipelines::new(device, &pipeline_layout, &icons_shader, icon_layout, color_format, "Gizmo icons");

        Self {
            lines,
            icons,
            uniform_buffer,
            bind_group,
            atlas,
            line_batch: Batch::new(),
            icon_batch: Batch::new(),
            text: None,
            drawn: false,
        }
    }

    /// Whether gizmos have drawn text - its material is set up then.
    pub(crate) fn uses_text(&self) -> bool {
        self.text.is_some()
    }

    fn clear(&mut self) {
        self.line_batch.instances.clear();
        self.line_batch.hideable = 0;
        self.icon_batch.instances.clear();
        self.icon_batch.hideable = 0;
        if let Some(text) = &mut self.text {
            text.clear();
        }
        self.drawn = false;
    }

    fn is_empty(&self) -> bool {
        self.line_batch.instances.is_empty() && self.icon_batch.instances.is_empty() && self.text.as_ref().is_none_or(|text| text.is_empty())
    }
}

impl Pipelines {
    fn new(device: &wgpu::Device, layout: &wgpu::PipelineLayout, shader: &wgpu::ShaderModule, vertices: wgpu::VertexBufferLayout<'_>, color_format: wgpu::TextureFormat, label: &str) -> Self {
        let pipeline = |fragment: &str, depth_compare: Option<wgpu::CompareFunction>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState { module: shader, entry_point: Some("vs_main"), buffers: &[vertices.clone()], compilation_options: Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                // Tested against the world's depth, never written - gizmos
                // don't hide each other.
                depth_stencil: depth_compare.map(|depth_compare| wgpu::DepthStencilState {
                    format: Texture::DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        Self {
            // Reversed-Z: nearer is greater - behind the world is less.
            hidden: pipeline("fs_hidden", Some(wgpu::CompareFunction::Less)),
            visible: pipeline("fs_main", Some(wgpu::CompareFunction::GreaterEqual)),
            on_top: pipeline("fs_main", None),
        }
    }
}

/// The camera, for working out where things are on screen.
struct View {
    camera_position: Vector3<f32>,
    view_proj: Matrix4<f32>,
    /// The projection's [1][1] - how many screen heights (over 2) one unit
    /// of height is at a distance of one.
    focal: f32,
    forward: Vector3<f32>,
    orthographic: bool,
    screen: [f32; 2],
}

impl View {
    fn clip(&self, point: Vector3<f32>) -> Vector4<f32> {
        self.view_proj * (point - self.camera_position).push(1.0)
    }

    /// Where `point` is on screen (px from the top left) - None behind the
    /// camera.
    fn to_screen(&self, point: Vector3<f32>) -> Option<Vector2<f32>> {
        let clip = self.clip(point);
        if clip.w <= 1e-4 {
            return None;
        }
        Some(Vector2::new((clip.x / clip.w * 0.5 + 0.5) * self.screen[0], (0.5 - clip.y / clip.w * 0.5) * self.screen[1]))
    }

    /// How long one screen pixel is in the world at `point`.
    fn pixel_size(&self, point: Vector3<f32>) -> f32 {
        let w = self.clip(point).w.max(1e-4);
        2.0 * w / (self.screen[1] * self.focal.max(1e-6))
    }

    /// The way from `point` toward the camera.
    fn toward_camera(&self, point: Vector3<f32>) -> Vector3<f32> {
        if self.orthographic {
            -self.forward
        } else {
            (self.camera_position - point).try_normalize(1e-6).unwrap_or(-self.forward)
        }
    }
}

/// The lines `shape` is made of - each a run of points (closed: back to the
/// first) with its own color when it has one (the axes').
fn polylines(shape: &Shape) -> Vec<(Vec<Vector3<f32>>, bool, Option<GizmoColor>)> {
    let circle = |center: Vector3<f32>, normal: Vector3<f32>, radius: f32| {
        let normal = normal.try_normalize(1e-6).unwrap_or_else(Vector3::y);
        let helper = if normal.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
        let a = normal.cross(&helper).normalize() * radius;
        let b = normal.cross(&a);
        let points = (0..CIRCLE_SEGMENTS).map(|index| {
            let angle = index as f32 / CIRCLE_SEGMENTS as f32 * std::f32::consts::TAU;
            center + a * angle.cos() + b * angle.sin()
        }).collect();
        (points, true, None)
    };
    match shape {
        Shape::Line(start, end) => vec![(vec![*start, *end], false, None)],
        Shape::Path { points, closed } => vec![(points.clone(), *closed && points.len() > 2, None)],
        Shape::Cross { center, size } => [Vector3::x(), Vector3::y(), Vector3::z()].into_iter()
            .map(|axis: Vector3<f32>| (vec![center - axis * (*size * 0.5), center + axis * (*size * 0.5)], false, None))
            .collect(),
        Shape::Circle { center, normal, radius } => vec![circle(*center, *normal, *radius)],
        Shape::Sphere { center, radius } => [Vector3::x(), Vector3::y(), Vector3::z()].into_iter().map(|axis| circle(*center, axis, *radius)).collect(),
        Shape::Cube { pose, half_extents } => {
            let corner = |x: f32, y: f32, z: f32| pose.point(Vector3::new(x * half_extents.x, y * half_extents.y, z * half_extents.z));
            let face = |y: f32| vec![corner(-1.0, y, -1.0), corner(1.0, y, -1.0), corner(1.0, y, 1.0), corner(-1.0, y, 1.0)];
            let mut lines = vec![(face(-1.0), true, None), (face(1.0), true, None)];
            for (x, z) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                lines.push((vec![corner(x, -1.0, z), corner(x, 1.0, z)], false, None));
            }
            lines
        }
        Shape::Axes { pose, length } => [(Vector3::x(), GizmoColor::RED), (Vector3::y(), GizmoColor::GREEN), (Vector3::z(), GizmoColor::BLUE)].into_iter()
            .map(|(axis, color)| (vec![pose.position, pose.point(axis * *length)], false, Some(color)))
            .collect(),
    }
}

/// `line`'s segments, as instances, into `out`.
fn line_instances(line: &LineGizmo, view: &View, linear: impl Fn(GizmoColor) -> [f32; 4], out: &mut Vec<LineInstance>) {
    let width = line.resolved_width();
    let dash = if line.dashed { DASH_PIXELS } else { 0.0 };
    for (mut points, closed, own_color) in polylines(&line.shape) {
        if points.len() < 2 {
            continue;
        }
        if closed {
            points.push(points[0]);
        }
        let (start_color, end_color) = match (line.resolved_colors(), own_color) {
            (Some(colors), _) => colors,
            (None, Some(color)) => (color, color),
            (None, None) => (GizmoColor::WHITE, GizmoColor::WHITE),
        };
        // Along the run, by its length in the world, for the colors' fade.
        let total: f32 = points.windows(2).map(|pair| (pair[1] - pair[0]).norm()).sum::<f32>().max(1e-6);
        let color_at = |t: f32| {
            let [a, b] = [start_color.0, end_color.0];
            GizmoColor(std::array::from_fn(|index| a[index] + (b[index] - a[index]) * t))
        };
        let mut walked = 0.0;
        // How far into the dashes each segment starts - the run's length on
        // screen so far, so the dashes go on round corners and circles.
        let mut dash_offset = 0.0;
        for pair in points.windows(2) {
            let length = (pair[1] - pair[0]).norm();
            let (t0, t1) = (walked / total, (walked + length) / total);
            walked += length;
            out.push(LineInstance {
                start: pair[0].into(),
                end: pair[1].into(),
                start_color: linear(color_at(t0)),
                end_color: linear(color_at(t1)),
                params: [width, dash, dash_offset, 0.0],
            });
            if dash > 0.0 {
                if let (Some(a), Some(b)) = (view.to_screen(pair[0]), view.to_screen(pair[1])) {
                    dash_offset += (b - a).norm();
                }
            }
        }
        // The arrowhead: two short lines back from the end, square to the
        // way the camera sees it.
        if line.arrow && !closed {
            let tip = points[points.len() - 1];
            let from = points[points.len() - 2];
            let Some(direction) = (tip - from).try_normalize(1e-6) else { continue };
            let toward_camera = view.toward_camera(tip);
            let side = direction.cross(&toward_camera).try_normalize(1e-6)
                .or_else(|| direction.cross(&Vector3::y()).try_normalize(1e-6))
                .unwrap_or_else(Vector3::x);
            let length = (ARROW_PIXELS.max(width * 4.0) * view.pixel_size(tip)).min((tip - from).norm() * 0.6);
            let color = linear(end_color);
            for sign in [-1.0, 1.0] {
                let back = tip - direction * length * ARROW_ANGLE.cos() + side * sign * length * ARROW_ANGLE.sin();
                out.push(LineInstance { start: back.into(), end: tip.into(), start_color: color, end_color: color, params: [width, 0.0, 0.0, 0.0] });
            }
        }
    }
}

impl App {
    /// This frame's gizmos (`app.gizmos`) as the renderer's instances - see
    /// the module doc. Before the frame's drawn.
    pub(crate) fn prepare_gizmos(&mut self) {
        let Renderer { gizmo_renderer: renderer, device, queue, config, shaders, camera_resources, .. } = &mut self.renderer;
        renderer.clear();
        let view = self.scene_manager.cameras().filter(|cameras| cameras.has_active_camera()).map(|cameras| {
            let active = cameras.active();
            View {
                camera_position: active.camera.position().coords,
                view_proj: Matrix4::from(camera_resources.uniform.view_proj),
                focal: active.projection.calc_matrix()[(1, 1)].abs(),
                forward: active.camera.calc_forward_direction(),
                orthographic: active.projection.is_orthographic(),
                screen: [self.window.pixel_size.width as f32, self.window.pixel_size.height as f32],
            }
        });
        let Some(view) = view.filter(|_| !self.gizmos.is_empty()) else { return };
        let srgb = config.format.is_srgb();
        let linear = |color: GizmoColor| {
            let [r, g, b, a] = color.0;
            if srgb { [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a] } else { [r, g, b, a] }
        };
        let GizmoParts { list, icons, channels } = self.gizmos.parts();
        // Text and labels: where on screen, how big, what color.
        let mut texts: Vec<(&str, Vector2<f32>, f32, GizmoColor)> = Vec::new();

        let (mut hideable_lines, mut top_lines) = (Vec::new(), Vec::new());
        for line in list.lines.iter().filter(|line| channels.shows(&line.common.channel)) {
            line_instances(line, &view, linear, if line.common.on_top { &mut top_lines } else { &mut hideable_lines });
            if let (Some(label), Some(at)) = (&line.common.label, view.to_screen(line.shape.label_point())) {
                let size = channels.text_size(&line.common.channel);
                let color = line.resolved_colors().map_or(GizmoColor::WHITE, |(_, end)| end);
                texts.push((label, at - Vector2::new(0.0, size * 0.9 + line.resolved_width()), size, color));
            }
        }
        renderer.line_batch.fill(hideable_lines, top_lines);

        let (mut hideable_icons, mut top_icons) = (Vec::new(), Vec::new());
        for icon in list.icons.iter().filter(|icon| channels.shows(&icon.common.channel)) {
            let size = icon.resolved_size();
            if let Some(uv) = icons.uv(&icon.icon) {
                // The square holds the icon's margin too.
                let instance = IconInstance { position: icon.position.into(), size: size * FIELD_SIZE as f32 / REF_SIZE as f32, color: linear(icon.resolved_color()), uv };
                if icon.common.on_top { top_icons.push(instance) } else { hideable_icons.push(instance) }
            }
            if let (Some(label), Some(at)) = (&icon.common.label, view.to_screen(icon.position)) {
                let text_size = channels.text_size(&icon.common.channel);
                texts.push((label, at - Vector2::new(0.0, size * 0.5 + text_size * 0.75), text_size, icon.resolved_color()));
            }
        }
        renderer.icon_batch.fill(hideable_icons, top_icons);

        for text in list.texts.iter().filter(|text| channels.shows(&text.common.channel)) {
            if let Some(at) = view.to_screen(text.position) {
                texts.push((&text.text, at, text.resolved_size(), text.resolved_color()));
            }
        }
        if !texts.is_empty() {
            let renderer_text = renderer.text.get_or_insert_with(|| {
                let mut text = SdfText::new(device, config.format, shaders.overlay_layout(Overlay::Text));
                text.screen_space = true;
                text
            });
            for (text, at, size, color) in texts {
                // A dark shadow under it, so it reads on anything.
                renderer_text.push(text, Font::default(), TextFrame::screen(at.x + 1.0, at.y + 1.0, size), [0.0, 0.0, 0.0, 0.75 * color.0[3]]);
                renderer_text.push(text, Font::default(), TextFrame::screen(at.x, at.y, size), color.0);
            }
        }

        // The icons drawn for the first time, into the atlas.
        for (x, y, texels) in icons.uploads.drain(..) {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &renderer.atlas, mip_level: 0, origin: wgpu::Origin3d { x, y, z: 0 }, aspect: wgpu::TextureAspect::All },
                &texels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(FIELD_SIZE), rows_per_image: Some(FIELD_SIZE) },
                wgpu::Extent3d { width: FIELD_SIZE, height: FIELD_SIZE, depth_or_array_layers: 1 },
            );
        }
        let uniform = GizmoUniform {
            camera_position: [view.camera_position.x, view.camera_position.y, view.camera_position.z, 1.0],
            screen: [view.screen[0], view.screen[1], 0.0, 0.0],
        };
        queue.write_buffer(&renderer.uniform_buffer, 0, bytemuck::cast_slice(&[uniform]));
        renderer.line_batch.upload(device, queue, "Gizmo lines");
        renderer.icon_batch.upload(device, queue, "Gizmo icons");
    }

    /// Draws the gizmos prepared this frame into `view` - once a frame, at
    /// the first UI step. `on_screen`: `view` is the screen (not one of the
    /// game's own images), so the world's depth can hide them.
    pub(crate) fn render_gizmos(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, on_screen: bool) {
        let screen = [self.window.pixel_size.width as f32, self.window.pixel_size.height as f32];
        let Renderer { gizmo_renderer: renderer, device, queue, shaders, overlay_materials, camera_resources, light, depth_render, .. } = &mut self.renderer;
        if renderer.drawn || renderer.is_empty() {
            return;
        }
        renderer.drawn = true;
        let camera = &camera_resources.bind_group;
        let (lines, icons) = (&renderer.line_batch, &renderer.icon_batch);
        let hideable = on_screen && (lines.hideable > 0 || icons.hideable > 0);
        if hideable {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Gizmos pass"),
                color_attachments: &[color_attachment(view, wgpu::LoadOp::Load)],
                depth_stencil_attachment: depth_attachment(&depth_render.texture.view, wgpu::LoadOp::Load),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            render_pass.set_bind_group(0, camera, &[]);
            render_pass.set_bind_group(1, &renderer.bind_group, &[]);
            lines.draw(&mut render_pass, &renderer.lines.hidden, lines.hideable());
            lines.draw(&mut render_pass, &renderer.lines.visible, lines.hideable());
            icons.draw(&mut render_pass, &renderer.icons.hidden, icons.hideable());
            icons.draw(&mut render_pass, &renderer.icons.visible, icons.hideable());
        }

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Gizmos on top pass"),
            color_attachments: &[color_attachment(view, wgpu::LoadOp::Load)],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        render_pass.set_bind_group(0, camera, &[]);
        render_pass.set_bind_group(1, &renderer.bind_group, &[]);
        let (line_range, icon_range) = if hideable { (lines.on_top(), icons.on_top()) } else { (lines.all(), icons.all()) };
        lines.draw(&mut render_pass, &renderer.lines.on_top, line_range);
        icons.draw(&mut render_pass, &renderer.icons.on_top, icon_range);
        let material = overlay_materials.get(GIZMO_TEXT).and_then(|key| shaders.overlay(key, Overlay::Text));
        if let (Some(text), Some(material)) = (&mut renderer.text, material) {
            if !text.is_empty() {
                text.draw(device, queue, &mut render_pass, material, camera, &light.rendering_data.bind_group, [0.0; 3], screen);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shaders_are_valid() {
        for (name, source) in [("gizmo_lines", include_str!("../../shaders/debug/gizmo_lines.wgsl")), ("gizmo_icons", include_str!("../../shaders/debug/gizmo_icons.wgsl"))] {
            let module = wgpu::naga::front::wgsl::parse_str(source).unwrap_or_else(|error| panic!("{name}: {}", error.emit_to_string(source)));
            wgpu::naga::valid::Validator::new(wgpu::naga::valid::ValidationFlags::all(), wgpu::naga::valid::Capabilities::empty())
                .validate(&module)
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        }
    }

    #[test]
    fn the_instances_match_the_shaders() {
        // No padding between the attributes - the vertex layouts assume none.
        assert_eq!(std::mem::size_of::<LineInstance>(), (3 + 3 + 4 + 4 + 4) * 4);
        assert_eq!(std::mem::size_of::<IconInstance>(), (3 + 1 + 4 + 4) * 4);
    }

    fn view() -> View {
        // A camera at the origin looking down -Z, 90 degrees tall, 800 x 600.
        let projection = crate::rendering::camera::projection::Projection::new(800, 600, 90.0, 0.1, 100.0);
        let look = Matrix4::look_at_rh(&nalgebra::Point3::origin(), &nalgebra::Point3::new(0.0, 0.0, -1.0), &Vector3::y());
        let matrix = projection.calc_matrix();
        View { camera_position: Vector3::zeros(), view_proj: matrix * look, focal: matrix[(1, 1)].abs(), forward: -Vector3::z(), orthographic: false, screen: [800.0, 600.0] }
    }

    #[test]
    fn a_point_ahead_is_in_the_middle_of_the_screen() {
        let view = view();
        let middle = view.to_screen(Vector3::new(0.0, 0.0, -10.0)).unwrap();
        assert!((middle - Vector2::new(400.0, 300.0)).norm() < 0.01, "{middle}");
        assert_eq!(view.to_screen(Vector3::new(0.0, 0.0, 10.0)), None, "behind the camera");
        // 90 degrees tall: at 10 m, 20 m of height fill 600 px.
        assert!((view.pixel_size(Vector3::new(0.0, 0.0, -10.0)) - 20.0 / 600.0).abs() < 1e-4);
    }

    #[test]
    fn shapes_become_segments() {
        use crate::rendering::gizmos::gizmos::Gizmos;
        use crate::rendering::gizmos::icons::Icon;
        let mut gizmos = Gizmos::new();
        gizmos.channel("test", |g| {
            g.line([0.0, 0.0, -5.0], [1.0, 0.0, -5.0]).arrow();
            g.cube(Vector3::new(0.0, 0.0, -5.0), Vector3::new(1.0, 1.0, 1.0));
            g.circle([0.0, 0.0, -5.0], Vector3::z(), 1.0).dashed();
            g.axes(Vector3::new(0.0, 0.0, -5.0), 1.0);
            g.icon([0.0, 0.0, -5.0], Icon::Mass);
        });
        let view = view();
        let count = |index: usize| {
            let mut out = Vec::new();
            line_instances(&gizmos.list.lines[index], &view, |color| color.0, &mut out);
            out
        };
        assert_eq!(count(0).len(), 1 + 2, "a line and its arrowhead");
        assert_eq!(count(1).len(), 12, "a box's edges");
        let circle = count(2);
        assert_eq!(circle.len(), CIRCLE_SEGMENTS);
        assert!(circle.windows(2).all(|pair| pair[1].params[2] > pair[0].params[2]), "the dashes go on round it");
        let axes = count(3);
        assert_eq!(axes.len(), 3);
        assert_eq!(axes[0].start_color, GizmoColor::RED.0);
    }
}
