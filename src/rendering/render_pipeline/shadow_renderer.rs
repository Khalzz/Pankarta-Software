//! The sun's shadows - cascaded shadow maps: the scene's depth seen from the
//! sun, drawn before the frame plan (see App::render_shadow_pass), which
//! standard_template.wgsl compares every pixel against to know whether the
//! sun reaches it.
//!
//! One map can't be both sharp up close and reach far - so there are a few
//! (`Shadows::cascades`), each a box around the camera 3 times as big as the
//! one before, the last `Shadows::distance` each way. All are the same
//! resolution, so the small near one is sharp and the far ones coarser, where
//! a texel is smaller on screen anyway. A pixel reads the smallest one it's
//! in, blending into the next toward its edge.
//!
//! Casters that held still since the last frame are drawn into a map of their
//! own per cascade, kept from frame to frame - drawn again only when what's in
//! it would change: one of them moves, stops or goes, the sun turns, a
//! material changes, or the box moves. That's copied into the map read (when
//! it changed, or something moving was drawn over it last frame), and only
//! what moves is drawn over it. So the boxes don't move with
//! every step of the camera: each moves in steps of a quarter of its size (and
//! is that much bigger, so the camera's surroundings are always in it) - and
//! always by whole texels of its map, or shadow edges would shimmer.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use nalgebra::{Matrix4, Orthographic3, Point3, Vector3, Vector4};
use wgpu::util::DeviceExt;

use crate::rendering::instance_management::InstanceRaw;
use crate::rendering::models::model::{ModelVertex, Vertex};
use crate::rendering::models::textures::Texture;

/// How every shadow looks - for every scene, until it's set again (see
/// Renderer::set_shadows). Whether a light casts any is its own (see
/// `Light::shadows`), and whether a node casts and gets them its model's
/// (see `Model::casts_shadows`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadows {
    /// Off: no shadows anywhere, whatever the lights say - and nothing spent
    /// on them.
    pub enabled: bool,
    /// How far from the camera (world units) shadows are drawn - they fade
    /// out toward it.
    pub distance: f32,
    /// How many shadow maps share that distance (1 to `MAX_CASCADES`) - more,
    /// sharper up close for the same reach.
    pub cascades: u32,
    /// Each map's size, in texels each way - bigger, sharper (and more
    /// memory: 2048 is 32 MB a map - the one read, and the still casters').
    pub resolution: u32,
    /// How soft their edges are, in texels of the map - 0 for hard.
    pub softness: f32,
    /// The least a shadow's edge fades over, in meters, however fine the
    /// maps' texels - so close-up shadows (sharp maps) blend in smoothly
    /// instead of ending in a hard line. The wider of this and `softness`
    /// texels wins. 0: `softness` alone.
    pub blur: f32,
    /// Whether shadows fall on see-through meshes (glTF alpha mode Blend -
    /// glass, flames), drawn in the "transparent" pass. They never cast any;
    /// off, they don't get darkened by others' either.
    pub on_transparent: bool,
}

impl Default for Shadows {
    fn default() -> Self {
        // 4 maps over 40 m: the nearest ~1.5 m each way around the camera at
        // ~2 mm a texel, edges fading over at least 1.5 cm - sharp up close,
        // smooth edges; nothing past 40 m from the camera casts.
        Self { enabled: true, distance: 40.0, cascades: 4, resolution: 2048, softness: 1.0, blur: 0.015, on_transparent: true }
    }
}

/// The most shadow maps there can be - the light's uniform has room for
/// this many.
pub const MAX_CASCADES: usize = 4;

/// How much bigger each cascade's box is than the one before.
const CASCADE_GROWTH: f32 = 3.0;

/// How many steps a cascade's box moves in across its size - see the module
/// doc.
const CACHE_STEPS: f32 = 4.0;

/// How far toward the sun (world units) past a box its map covers - so a
/// tall thing outside it, between the sun and the box, still shades what's
/// in it.
const CASTER_REACH: f32 = 200.0;

/// Maps OpenGL's [-1, 1] NDC z-range to wgpu's [0, 1] - not reversed, unlike
/// the camera's (see projection::OPENGL_TO_WGPU_MATRIX): an orthographic
/// depth is spread evenly, so there's no precision to win back by reversing.
#[rustfmt::skip]
const OPENGL_TO_WGPU_DEPTH: Matrix4<f32> = Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 0.5, 0.5,
    0.0, 0.0, 0.0, 1.0,
);

/// A cascade's view from the sun, laid out like the camera's uniform
/// (CameraUniform) - the shadow pass binds it where the camera goes, so a
/// material's own vertex shader draws from the sun unchanged.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct SunView {
    view_proj: [[f32; 4]; 4],
    view_position: [f32; 4],
    near: f32,
    far: f32,
    _padding: [f32; 2],
}

/// One cascade's part of the shadow pass.
pub(crate) struct Cascade {
    /// Its layer of the maps read, drawn into.
    pub(crate) view: wgpu::TextureView,
    /// Its layer of the still casters' maps.
    pub(crate) still_view: wgpu::TextureView,
    /// Its view from the sun (a SunView), bound as the camera.
    buffer: wgpu::Buffer,
    pub(crate) bind_group: wgpu::BindGroup,
    /// This frame's view-projection, and half its box's size.
    view_proj: Matrix4<f32>,
    half: f32,
    /// Where its box is, which way the sun shines - see `update`.
    pub(crate) placed: u64,
    /// What its still casters' map was last drawn for - see
    /// App::render_shadow_pass. None: never.
    pub(crate) still_drawn: Option<u64>,
    /// What its layer of the maps read holds just the still casters' map
    /// for, as `still_drawn` - so it isn't copied again while nothing moves.
    /// None: moving casters were drawn over it (or never copied).
    pub(crate) copied: Option<u64>,
}

impl Cascade {
    /// Whether anything in the sphere at `center` (camera-relative) of
    /// `radius` can fall in its box - across the box; along the sun, it
    /// reaches far enough to take anything.
    pub(crate) fn sees(&self, center: Vector3<f32>, radius: f32) -> bool {
        let at = self.view_proj * Vector4::new(center.x, center.y, center.z, 1.0);
        let reach = 1.0 + radius / self.half;
        at.x.abs() <= reach && at.y.abs() <= reach
    }
}

/// This frame's cascades, for the light's uniform (see
/// App::update_lighting).
pub struct CascadeFit {
    /// Each one's view-projection - unused ones left as they are.
    pub view_proj: [[[f32; 4]; 4]; MAX_CASCADES],
    /// Each one's texel, in world units.
    pub texel: [f32; MAX_CASCADES],
}

pub struct ShadowRender {
    /// Every cascade's map, as layers of one texture - the whole of it is
    /// what the light binds (`map.view`, a 2D array).
    pub(crate) map: Texture,
    /// The still casters' maps, the same way - copied into `map` when it
    /// doesn't already hold just them (see `Cascade::copied`).
    pub(crate) still_map: wgpu::Texture,
    pub(crate) cascades: Vec<Cascade>,
    /// Each map's texels each way.
    resolution: u32,
    /// The camera's bind group layout - what each cascade's view is bound as.
    camera_layout: wgpu::BindGroupLayout,
    /// What most meshes cast with: depth only - and one whose texture cuts
    /// its shape out (alpha mode Mask) with what's see-through left out.
    pub(crate) depth_pipeline: wgpu::RenderPipeline,
    pub(crate) cutout_pipeline: wgpu::RenderPipeline,
    /// DebugViews::shadows - see `draw_debug`.
    debug_pipeline: wgpu::RenderPipeline,
    debug_layout: wgpu::BindGroupLayout,
    debug_bind_group: wgpu::BindGroup,
    /// Whether the shadow pass draws this frame - shadows on, a sun casting
    /// them, seen by a camera (see App::update_lighting).
    pub(crate) active: bool,
    /// The casters that held still this frame (which, where, what they're
    /// drawn with) - their maps are drawn again when it changes (see
    /// App::write_instance_buffers).
    pub(crate) still_casters: u64,
    /// Every drawn node's pose last frame (position, rotation, scale) - what
    /// tells a still one from a moving one.
    pub(crate) poses: HashMap<String, [f32; 10]>,
}

/// The most the slope bias pushes a face back - see create_shadow_pipeline.
const SHADOW_BIAS_CLAMP: f32 = 0.00005;

/// A shadow pass pipeline: drawn with `layout` (the model pipelines'), from
/// `module`'s `vs_main`, and `fragment` if any (none: depth only). Both
/// sides of every face: a double-sided mesh casts like it's drawn, and a
/// single-sided one seen from behind by the sun (a plane's underside) still
/// casts. The slope bias pushes faces the sun sees edge-on back a little, so
/// they don't shadow themselves in stripes (the shader's own offsets do the
/// rest) - but never more than SHADOW_BIAS_CLAMP: unclamped, a face the sun
/// sees nearly edge-on (a box's side under a high sun) goes back so far it
/// stops casting, and light leaks in a bright band around the shadows of
/// what's close to the surface it falls on.
pub(crate) fn create_shadow_pipeline(device: &wgpu::Device, label: &str, layout: &wgpu::PipelineLayout, module: &wgpu::ShaderModule, fragment: Option<&str>) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState { module, entry_point: Some("vs_main"), buffers: &[ModelVertex::desc(), InstanceRaw::desc()], compilation_options: Default::default() },
        fragment: fragment.map(|entry_point| wgpu::FragmentState { module, entry_point: Some(entry_point), targets: &[], compilation_options: Default::default() }),
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: Texture::DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            // The clamp is in depth (0..1 across the map's box, which runs
            // CASTER_REACH plus the box's own size along the sun - 200 to
            // ~600 m): about 1 to 3 cm.
            bias: wgpu::DepthBiasState { constant: 1, slope_scale: 2.0, clamp: SHADOW_BIAS_CLAMP },
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// Feeds `values`' exact bits to `hasher` - for telling whether a pose or a
/// direction changed at all. -0 as 0 (the same number, other bits).
pub(crate) fn hash_floats(values: &[f32], hasher: &mut DefaultHasher) {
    for value in values {
        (value + 0.0).to_bits().hash(hasher);
    }
}

impl ShadowRender {
    /// `model_layout` is the model pipelines' (the mesh's texture, the
    /// camera, the mesh's transform, the light) - the shadow pass is bound
    /// the same way.
    pub fn new(device: &wgpu::Device, settings: &Shadows, model_layout: &wgpu::PipelineLayout, camera_layout: &wgpu::BindGroupLayout, color_format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow_depth"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/environment/shadow_depth.wgsl").into()),
        });
        let depth_pipeline = create_shadow_pipeline(device, "shadow_depth", model_layout, &module, None);
        let cutout_pipeline = create_shadow_pipeline(device, "shadow_cutout", model_layout, &module, Some("fs_cutout"));

        let debug_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_debug_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Depth, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false },
                count: None,
            }],
        });
        let debug_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow_maps_debug"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/debug/shadow_maps.wgsl").into()),
        });
        let debug_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow_debug_pipeline_layout"),
            bind_group_layouts: &[&debug_layout],
            push_constant_ranges: &[],
        });
        let debug_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shadow_debug"),
            layout: Some(&debug_pipeline_layout),
            vertex: wgpu::VertexState { module: &debug_module, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &debug_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format: color_format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let (resolution, count) = Self::size_of(settings);
        let (map, still_map, cascades) = Self::create_maps(device, camera_layout, resolution, count);
        let debug_bind_group = Self::create_debug_bind_group(device, &debug_layout, &map);
        Self {
            map,
            still_map,
            cascades,
            resolution,
            camera_layout: camera_layout.clone(),
            depth_pipeline,
            cutout_pipeline,
            debug_pipeline,
            debug_layout,
            debug_bind_group,
            active: false,
            still_casters: 0,
            poses: HashMap::new(),
        }
    }

    /// `settings`' resolution and cascade count, kept to what can be made.
    fn size_of(settings: &Shadows) -> (u32, u32) {
        (settings.resolution.max(1), settings.cascades.clamp(1, MAX_CASCADES as u32))
    }

    /// The maps read and the still casters' - `count` layers each,
    /// `resolution` texels each way - the first with a comparison sampler
    /// (filtered, so every lookup already blends the 4 texels around it),
    /// and each cascade's layers and view from the sun.
    fn create_maps(device: &wgpu::Device, camera_layout: &wgpu::BindGroupLayout, resolution: u32, count: u32) -> (Texture, wgpu::Texture, Vec<Cascade>) {
        let maps = |label, usage| device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: resolution, height: resolution, depth_or_array_layers: count },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Texture::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | usage,
            view_formats: &[],
        });
        let texture = maps("shadow_maps", wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        let still_map = maps("still_shadow_maps", wgpu::TextureUsages::COPY_SRC);
        // An array even with one layer - what the shaders read.
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let layer = |texture: &wgpu::Texture, layer| texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: layer,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let cascades = (0..count).map(|index| {
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shadow_sun_view"),
                contents: bytemuck::cast_slice(&[SunView { view_proj: Matrix4::identity().into(), view_position: [0.0, 0.0, 0.0, 1.0], near: 0.0, far: 1.0, _padding: [0.0; 2] }]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shadow_sun_view"),
                layout: camera_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
            });
            Cascade { view: layer(&texture, index), still_view: layer(&still_map, index), buffer, bind_group, view_proj: Matrix4::identity(), half: 1.0, placed: 0, still_drawn: None, copied: None }
        }).collect();
        (Texture { texture, view, sampler }, still_map, cascades)
    }

    fn create_debug_bind_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, map: &Texture) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_debug"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&map.view) }],
        })
    }

    pub fn resolution(&self) -> u32 {
        self.resolution
    }

    /// Whether `settings` needs other maps than these (another resolution or
    /// cascade count) - see `resize`.
    pub fn needs_resize(&self, settings: &Shadows) -> bool {
        Self::size_of(settings) != (self.resolution, self.cascades.len() as u32)
    }

    /// New maps for `settings` - the light's bind group holds the old ones,
    /// so it has to be built again too (see Renderer::set_shadows).
    pub fn resize(&mut self, device: &wgpu::Device, settings: &Shadows) {
        let (resolution, count) = Self::size_of(settings);
        (self.map, self.still_map, self.cascades) = Self::create_maps(device, &self.camera_layout, resolution, count);
        self.debug_bind_group = Self::create_debug_bind_group(device, &self.debug_layout, &self.map);
        self.resolution = resolution;
    }

    /// Places every cascade's box for this frame - shining along `toward_sun`
    /// negated, around the camera at `camera_position` (the world's; what
    /// the maps hold is camera-relative, like everything drawn) - and writes
    /// its view from the sun for the shadow pass. Returned for the light's
    /// uniform.
    pub fn update(&mut self, queue: &wgpu::Queue, toward_sun: Vector3<f32>, camera_position: Vector3<f32>, settings: &Shadows) -> CascadeFit {
        let shining = -toward_sun;
        // Any up that isn't the way it shines.
        let up = if shining.y.abs() > 0.99 { Vector3::z() } else { Vector3::y() };
        let origin = Point3::origin();
        let view = Matrix4::look_at_rh(&origin, &(origin + shining), &up);
        // The camera in the sun's view - x, y across the map, z along the sun.
        let across = view.transform_vector(&camera_position);

        let mut fit = CascadeFit { view_proj: [[[0.0; 4]; 4]; MAX_CASCADES], texel: [0.0; MAX_CASCADES] };
        let last = self.cascades.len() - 1;
        let resolution = self.resolution;
        for (index, cascade) in self.cascades.iter_mut().enumerate() {
            // Each box CASCADE_GROWTH times the one before - the last
            // `distance` each way - and a step bigger, so the camera's
            // surroundings are in it wherever it is within a step.
            let reach = settings.distance.max(0.01) / CASCADE_GROWTH.powi((last - index) as i32);
            let half = reach * (1.0 + 1.0 / CACHE_STEPS);
            let texel = 2.0 * half / resolution as f32;
            // A step is a whole number of texels.
            let step = ((reach / CACHE_STEPS) / texel).round().max(1.0) * texel;
            // The box's centre: the step nearest the camera, in the sun's
            // view - less where the camera is, to be camera-relative.
            let grid = across.map(|along| (along / step).round());
            let centre = grid * step - across;
            // Looking down -z: from CASTER_REACH past the box toward the
            // sun to the far side of the box.
            let projection = Orthographic3::new(centre.x - half, centre.x + half, centre.y - half, centre.y + half, -(centre.z + half + CASTER_REACH), half - centre.z);
            let view_proj = OPENGL_TO_WGPU_DEPTH * projection.to_homogeneous() * view;
            let raw: [[f32; 4]; 4] = view_proj.into();
            queue.write_buffer(&cascade.buffer, 0, bytemuck::cast_slice(&[SunView { view_proj: raw, view_position: [0.0, 0.0, 0.0, 1.0], near: 0.0, far: 1.0, _padding: [0.0; 2] }]));

            let mut hasher = DefaultHasher::new();
            hash_floats(&[grid.x, grid.y, grid.z, step, half], &mut hasher);
            hash_floats(toward_sun.as_slice(), &mut hasher);
            cascade.placed = hasher.finish();
            cascade.view_proj = view_proj;
            cascade.half = half;
            fit.view_proj[index] = raw;
            fit.texel[index] = texel;
        }
        fit
    }

    /// Draws `DebugViews::shadows` over `view` (`size` pixels): every
    /// cascade's map, nearest first, in a row of squares along the bottom
    /// left.
    pub(crate) fn draw_debug(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, size: [u32; 2]) {
        let side = (size[1] / 4).min(size[0] / self.cascades.len() as u32).max(1) as f32;
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shadow maps debug"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        render_pass.set_pipeline(&self.debug_pipeline);
        render_pass.set_bind_group(0, &self.debug_bind_group, &[]);
        for index in 0..self.cascades.len() as u32 {
            render_pass.set_viewport(index as f32 * side, size[1] as f32 - side, side, side, 0.0, 1.0);
            render_pass.draw(0..3, index..index + 1);
        }
    }
}
