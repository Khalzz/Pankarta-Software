use wgpu::{util::DeviceExt, BindGroup, BindGroupLayout, Buffer, Device, RenderPipeline, SurfaceConfiguration};

use crate::rendering::render_pipeline::shadow_renderer::MAX_CASCADES;
use crate::rendering::{camera::handler::CameraResources, models::{model::{self, Vertex}, textures::Texture}, ui::rendering_utils};

// Byte layout has to match WGSL's own struct-member-offset rule exactly
// (roundUp(align(member), offset(end of previous member)) - see
// https://gpuweb.github.io/gpuweb/wgsl/#structure-member-layout): vec3<f32>
// aligns to 16, but a scalar right after one packs in immediately rather
// than waiting for the next 16-byte boundary (that's std140's rule, not
// WGSL's) - position(0..12)+_padding(12..16), color(16..28), time(28..32),
// camera_position(32..44)+_padding2(44..48), ambient(48..60)+ambient_ground(60..64).
// camera_position starts exactly on a 16-byte boundary (32) so it needs no
// extra padding before it either; _padding2 keeps ambient on the next one,
// and ambient_ground (a scalar, packed right after it) brings shadow_view_proj to 64, the next 16-byte boundary a
// mat4x4 aligns to - shadow_view_proj(64..320, 4 of them),
// shadow_params(320..336), shadow_texels(336..352), 352 a multiple of 16 as
// the struct's size has to be.
// Getting this wrong doesn't error - the shader just silently reads whatever
// raw bytes land at the offset it expects instead of the real value - burned
// once already (see git history), so any new field added here needs the
// same offset check.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightUniform {
    pub position: [f32; 3],
    pub _padding: u32,
    pub color: [f32; 3],
    // Seconds of elapsed animation time - piggybacks on this buffer purely
    // because it's already bound (group 3) to every model-drawing pipeline
    // and rewritten once a frame (see App::run's own "lighting update"
    // block); water.wgsl is the only shader that reads it today, but it's
    // not otherwise light-specific. Devices here cap bind groups at 4 (see
    // engine::rendering::shader_library's module doc for why a dedicated 5th group isn't
    // an option), so reusing an existing one was the only way to get a
    // per-frame value into a second pipeline without also touching every
    // other pipeline's layout.
    pub time: f32,
    // Camera's true world-space position, same piggyback trick as `time`.
    // water.wgsl's vertex model matrix already bakes in camera-relative
    // translation (see InstanceRaw::from_transform), so this is what lets the wave
    // function recover true world position instead of sampling relative to
    // wherever the camera happens to be.
    pub camera_position: [f32; 3],
    pub _padding2: u32,
    // The light from the whole sky - see `Renderer::set_ambient_light`.
    pub ambient: [f32; 3],
    // How much of `ambient` reaches a surface facing straight down (1: as
    // much as one facing up) - see `Renderer::set_ambient_ground`. Shaders
    // that don't declare it read it as padding.
    pub ambient_ground: f32,
    // Each cascade's view-projection - where a camera-relative position
    // falls in its shadow map (see render_pipeline::shadow_renderer).
    pub shadow_view_proj: [[[f32; 4]; 4]; MAX_CASCADES],
    // How many cascades there are this frame (0: no shadows), one texel in a
    // map's 0..1 coordinates, how soft the edges are, in texels
    // (Shadows::softness), and nothing.
    pub shadow_params: [f32; 4],
    // How big a texel of each cascade's map is in the world.
    pub shadow_texels: [f32; MAX_CASCADES],
}

pub struct LightRenderData {
    pub bind_group_layout: BindGroupLayout,
    pub bind_group: BindGroup,
    /// `bind_group` with a stand-in for the shadow maps - what the shadow
    /// pass binds, drawing into the real ones (see App::render_shadow_pass).
    pub unshadowed_bind_group: BindGroup,
    pub buffer: Buffer,
    /// The same light as `buffer`, but saying there are no shadow maps
    /// (`shadow_params.x` 0) - so what's drawn with it is never shadowed.
    /// Written next to `buffer` every frame (see App::render).
    pub no_shadows_buffer: Buffer,
    /// Binds `no_shadows_buffer` - the "transparent" pass draws with it when
    /// shadows don't fall on see-through meshes (`Shadows::on_transparent`).
    pub no_shadows_bind_group: BindGroup,
    pub render_pipeline: RenderPipeline
}

/// # Light
/// This struct is dedicated for light creation and rendering of them.
/// 
/// ## Values:
/// - uniform: The uniform data that will be given to the shader to render the light
pub struct Light {
    pub uniform: LightUniform,
    pub rendering_data: LightRenderData

}

impl Light {
    /// `shadow_map` is the sun's maps (see ShadowRender) - bound next to the
    /// uniform, so everything drawing with the light can read them.
    pub fn new(device: &Device, config: &SurfaceConfiguration, camera: &CameraResources, shadow_map: &Texture) -> Self {
        let uniform = LightUniform {
            position: [0.0, 5.0, 0.0],
            _padding: 0,
            color: [1.0, 1.0, 1.0],
            time: 0.0,
            camera_position: [0.0, 0.0, 0.0],
            _padding2: 0,
            ambient: [0.0, 0.0, 0.0],
            ambient_ground: 1.0,
            shadow_view_proj: [[[0.0; 4]; 4]; MAX_CASCADES],
            shadow_params: [0.0; 4],
            shadow_texels: [0.0; MAX_CASCADES],
        };

        let buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Light VB"),
                contents: bytemuck::cast_slice(&[uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, // remember that copydst let us change later elements like position of objects
            }
        );

        let bind_group_layout = Self::create_bind_group_layout(device);
        let bind_group = Self::create_bind_group(device, &bind_group_layout, &buffer, shadow_map);
        // The shadow pass binds the light without the shadow maps it's
        // drawing into - a stand-in, never read, in their place.
        let stand_in = Self::stand_in_shadow_map(device);
        let unshadowed_bind_group = Self::create_bind_group(device, &bind_group_layout, &buffer, &stand_in);
        // With no maps to read (its count is 0), the stand-in is never read.
        let no_shadows_buffer = device.create_buffer_init(
            &wgpu::util::BufferInitDescriptor {
                label: Some("Light (no shadows) VB"),
                contents: bytemuck::cast_slice(&[uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            }
        );
        let no_shadows_bind_group = Self::create_bind_group(device, &bind_group_layout, &no_shadows_buffer, &stand_in);

        let render_pipeline = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Light Pipeline Layout"),
                bind_group_layouts: &[&camera.bind_group_layout, &bind_group_layout],
                push_constant_ranges: &[],
            });

            let shader = wgpu::ShaderModuleDescriptor {
                label: Some("Light Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../../shaders/environment/light.wgsl").into()),
            };
            
            rendering_utils::create_render_pipeline(
                &device,
                &layout,
                config.format,
                Some(Texture::DEPTH_FORMAT),
                &[model::ModelVertex::desc()],
                shader,
                Some(wgpu::Face::Back),
                true,
            )
        };

        let rendering_data = LightRenderData {
            bind_group_layout,
            bind_group,
            unshadowed_bind_group,
            buffer,
            no_shadows_buffer,
            no_shadows_bind_group,
            render_pipeline,
        };

        Self {
            uniform,
            rendering_data,
        }
    }

    /// The light's bind group layout: the uniform, the shadow maps and their
    /// sampler - group 3 of everything drawn.
    pub fn create_bind_group_layout(device: &Device) -> BindGroupLayout {
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            // The shadow maps (a layer a cascade), and their comparison
            // sampler.
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            }],
            label: None,
        })
    }

    /// A one-layer depth array, never drawn into or read - see
    /// `unshadowed_bind_group`.
    fn stand_in_shadow_map(device: &Device) -> Texture {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shadow_map_stand_in"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Texture::DEPTH_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { compare: Some(wgpu::CompareFunction::LessEqual), ..Default::default() });
        Texture { texture, view, sampler }
    }

    fn create_bind_group(device: &Device, layout: &BindGroupLayout, buffer: &Buffer, shadow_map: &Texture) -> BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow_map.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&shadow_map.sampler) },
            ],
            label: None,
        })
    }

    /// The shadow maps were made again (see ShadowRender::resize) - bind the new ones
    /// (a bind group is tied to the texture it was made with).
    pub fn rebind_shadow_map(&mut self, device: &Device, shadow_map: &Texture) {
        let data = &mut self.rendering_data;
        data.bind_group = Self::create_bind_group(device, &data.bind_group_layout, &data.buffer, shadow_map);
    }
}