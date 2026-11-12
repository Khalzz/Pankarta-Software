//! Lines - thin, one color at each end, in the world. Anyone can fill a
//! `Lines`: the game (`app.renderer.lines`, drawn this frame), a physics
//! behavior (its `debug_draw`, for the physics debug view), the engine's own
//! views. For thick or styled lines (a laser, a HUD) see the line sets
//! (rendering::render_sets).

use nalgebra::Vector3;

use crate::rendering::vertex::LineVertex;

/// Line segments, each from one point to another - built up with `line`,
/// `ray`, `cross` and `outline`.
#[derive(Default, Clone, Debug)]
pub struct Lines {
    segments: Vec<[LineVertex; 2]>,
}

impl Lines {
    /// A line from `from` to `to`, in `color` (RGB, 0..1).
    pub fn line(&mut self, from: Vector3<f32>, to: Vector3<f32>, color: [f32; 3]) {
        self.segments.push([LineVertex { position: from.into(), color }, LineVertex { position: to.into(), color }]);
    }

    /// A line starting at `origin`, pointing along `vector` - a force drawn
    /// from where it's applied, say.
    pub fn ray(&mut self, origin: Vector3<f32>, vector: Vector3<f32>, color: [f32; 3]) {
        self.line(origin, origin + vector, color);
    }

    /// A small 3-axis cross marking a point, `size` from end to end.
    pub fn cross(&mut self, center: Vector3<f32>, size: f32, color: [f32; 3]) {
        let half = size * 0.5;
        for axis in [Vector3::x(), Vector3::y(), Vector3::z()] {
            self.line(center - axis * half, center + axis * half, color);
        }
    }

    /// A closed outline through `corners`, in order.
    pub fn outline(&mut self, corners: &[Vector3<f32>], color: [f32; 3]) {
        for (index, corner) in corners.iter().enumerate() {
            self.line(*corner, corners[(index + 1) % corners.len()], color);
        }
    }

    /// Every segment - its two ends.
    pub fn segments(&self) -> &[[LineVertex; 2]] {
        &self.segments
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn clear(&mut self) {
        self.segments.clear();
    }
}

/// Draws `Lines` - the GPU's side: one pipeline, and a vertex buffer kept
/// from frame to frame, only made bigger when the lines outgrow it.
pub(crate) struct LineRenderer {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
}

impl LineRenderer {
    /// `light_layout`: the light buffer's (it carries the camera's position -
    /// lines are in the world, the shader makes them camera-relative).
    /// `camera_layout`: the camera's.
    pub(crate) fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat, light_layout: &wgpu::BindGroupLayout, camera_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Lines shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/lines/lines.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Lines pipeline layout"),
            bind_group_layouts: &[light_layout, camera_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Lines"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[LineVertex::desc()], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: Some(wgpu::BlendState::REPLACE), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::LineList, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Lines vertices"),
            size: 4096,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, vertices }
    }

    /// Draws every set of `lines` into `render_pass`, over what's there -
    /// copied to the GPU as they are, one after another.
    pub(crate) fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, render_pass: &mut wgpu::RenderPass<'_>, light: &wgpu::BindGroup, camera: &wgpu::BindGroup, lines: &[&Lines]) {
        let total: usize = lines.iter().map(|lines| lines.segments.len()).sum();
        if total == 0 {
            return;
        }
        let segment_size = std::mem::size_of::<[LineVertex; 2]>() as u64;
        crate::rendering::ui::ui::grow_buffer(&mut self.vertices, device, total as u64 * segment_size, wgpu::BufferUsages::VERTEX);
        let mut offset = 0;
        for lines in lines.iter().filter(|lines| !lines.is_empty()) {
            queue.write_buffer(&self.vertices, offset, bytemuck::cast_slice(&lines.segments));
            offset += lines.segments.len() as u64 * segment_size;
        }
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, light, &[]);
        render_pass.set_bind_group(1, camera, &[]);
        render_pass.set_vertex_buffer(0, self.vertices.slice(..offset));
        render_pass.draw(0..total as u32 * 2, 0..1);
    }
}

/// A corner of one of `Faces`' triangles - where it is, and its color with
/// how see-through it is.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct FaceVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
}

impl FaceVertex {
    const ATTRIBUTES: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x4];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<FaceVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBUTES,
        }
    }
}

/// Triangles in the world, one color each, blended over what's there - the
/// faces of a debugged collider (see `ColliderDebug`). Drawn like `Lines`:
/// over everything, under the lines.
#[derive(Default, Clone, Debug)]
pub struct Faces {
    triangles: Vec<[FaceVertex; 3]>,
}

impl Faces {
    /// A triangle through `a`, `b`, `c` in `color` (RGBA, 0..1) - seen from
    /// both sides.
    pub fn triangle(&mut self, a: Vector3<f32>, b: Vector3<f32>, c: Vector3<f32>, color: [f32; 4]) {
        let corner = |position: Vector3<f32>| FaceVertex { position: position.into(), color };
        self.triangles.push([corner(a), corner(b), corner(c)]);
    }

    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    pub fn clear(&mut self) {
        self.triangles.clear();
    }
}

/// Draws `Faces` - like `LineRenderer`, with alpha blending and no culling.
pub(crate) struct FaceRenderer {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
}

impl FaceRenderer {
    pub(crate) fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat, light_layout: &wgpu::BindGroupLayout, camera_layout: &wgpu::BindGroupLayout) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Faces shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/lines/faces.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Faces pipeline layout"),
            bind_group_layouts: &[light_layout, camera_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Faces"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), buffers: &[FaceVertex::desc()], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Faces vertices"),
            size: 4096,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, vertices }
    }

    /// Draws `faces` into `render_pass`, over what's there.
    pub(crate) fn draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, render_pass: &mut wgpu::RenderPass<'_>, light: &wgpu::BindGroup, camera: &wgpu::BindGroup, faces: &Faces) {
        if faces.is_empty() {
            return;
        }
        let size = std::mem::size_of_val(faces.triangles.as_slice()) as u64;
        crate::rendering::ui::ui::grow_buffer(&mut self.vertices, device, size, wgpu::BufferUsages::VERTEX);
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&faces.triangles));
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, light, &[]);
        render_pass.set_bind_group(1, camera, &[]);
        render_pass.set_vertex_buffer(0, self.vertices.slice(..size));
        render_pass.draw(0..faces.triangles.len() as u32 * 3, 0..1);
    }
}
