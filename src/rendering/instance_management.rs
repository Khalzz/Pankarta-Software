use nalgebra::{Matrix3, Vector3};
use wgpu::util::DeviceExt;
use wgpu::{Buffer, Device};

use crate::{rendering::models::model::Model, scene_manager::node::properties::Transform3D};

/// A node drawn with a model (see scene_manager::render_bridge).
pub struct InstanceData {
    /// Where it's drawn - its node's Transform3D with its mesh's or part's
    /// own placed inside (see render_bridge::place).
    pub transform: Transform3D,
    pub model_ref: String,
    /// The node it's drawn for - a model's parts are several instances of one
    /// node.
    pub node: String,
    /// The shader it's drawn with (its mesh's or model's `Material`) - see
    /// engine::rendering::shader_library.
    pub material: String,
    /// Its mesh's or model's `casts_shadows` and `receives_shadows`.
    pub casts_shadows: bool,
    pub receives_shadows: bool,
}

pub struct ModelDataInstance {
    pub model: Model,
    pub instance_buffer: Buffer,
    pub instance_count: u32,
    /// Which of this frame's instances (in the buffer, in order) each
    /// material draws - they're written sorted by material, so each is one
    /// run (see App::run). Empty until the first frame's written.
    pub material_ranges: Vec<(String, std::ops::Range<u32>)>,
    /// The runs of them that cast shadows - each run one material, either
    /// all held still since the last frame or all moving (see
    /// App::write_instance_buffers).
    pub shadow_runs: Vec<ShadowRun>,
    /// Some of its meshes drawn with another material (by mesh name, the
    /// material instance - see Material::meshes), set every frame from the
    /// node that has them.
    pub mesh_materials: std::collections::HashMap<String, String>,
    /// Which entry this is - a new one for every one made (see `new_stamp`),
    /// so one put in place of another under the same name (a node spawned
    /// with a model already drawn) reads as a change (see
    /// App::place_materials).
    pub stamp: u64,
}

impl ModelDataInstance {
    /// A stamp no entry has had before - see `stamp`.
    pub fn new_stamp() -> u64 {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}

/// Instances of one model, one material, that cast shadows - all held still
/// or all moving - and the sphere they're all in (camera-relative), for
/// leaving them out of a cascade they can't fall in (see Cascade::sees).
pub struct ShadowRun {
    pub material: String,
    pub instances: std::ops::Range<u32>,
    pub still: bool,
    pub center: Vector3<f32>,
    pub radius: f32,
}

// Quaternions are not very usable in wgpu, so we save the raw instance here directly
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRaw {
    pub(crate) model: [[f32; 4]; 4],
    pub(crate) normal: [[f32; 3]; 3],
    /// 1 if shadows fall on it, 0 if not (`Model::receives_shadows`).
    pub(crate) receives_shadows: f32,
}

impl InstanceRaw {
    /// `transform`'s, drawn camera-relative: subtracting `camera_position`
    /// here, once, on the CPU keeps the numbers the GPU multiplies small no
    /// matter how far it is from the world origin, avoiding the precision
    /// loss (z-fighting, vertex jitter) that absolute world coordinates in
    /// the matrix cause.
    pub fn from_transform(transform: &Transform3D, camera_position: Vector3<f32>) -> Self {
        let relative = Transform3D { position: transform.position - camera_position, ..*transform };
        InstanceRaw {
            model: relative.matrix().into(),
            normal: Matrix3::from(transform.rotation).into(),
            receives_shadows: 1.0,
        }
    }

    // Create a vertex buffer layout for InstanceRaw
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<InstanceRaw>() as wgpu::BufferAddress, // Instance raw means shader will change per instance
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                // Define each vec4 in a mat4
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x4,
                },
                // Rotation for illumination normals
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 16]>() as wgpu::BufferAddress,
                    shader_location: 9,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 19]>() as wgpu::BufferAddress,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 22]>() as wgpu::BufferAddress,
                    shader_location: 11,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // Whether shadows fall on it.
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 25]>() as wgpu::BufferAddress,
                    shader_location: 12,
                    format: wgpu::VertexFormat::Float32,
                },
            ],
        }
    }
}

/// One instance buffer holding `instances`, drawn from `camera_position`
/// (see `InstanceRaw::from_transform`).
pub fn create_instance_buffer(instances: &[&Transform3D], device: &Device, camera_position: Vector3<f32>) -> Buffer {
    let raw_instances: Vec<InstanceRaw> = instances.iter()
    .map(|transform| InstanceRaw::from_transform(transform, camera_position))
    .collect();

    device.create_buffer_init(
        &wgpu::util::BufferInitDescriptor {
            label: Some("Instance Buffer"),
            contents: bytemuck::cast_slice(&raw_instances),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        }
    )
}
