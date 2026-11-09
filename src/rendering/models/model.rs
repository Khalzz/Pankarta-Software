use std::{collections::HashMap, mem};


use gltf::material::AlphaMode;
use nalgebra::Vector3;
use wgpu::{BindGroupLayoutDescriptor, Device};

use crate::scene_manager::node::properties::Transform3D;

use super::textures::Texture;

pub trait Vertex {
    fn desc() -> wgpu::VertexBufferLayout<'static>;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub normal: [f32; 3],
}

impl Vertex for ModelVertex {
    fn desc() -> wgpu::VertexBufferLayout<'static> {
        // Vertex buffer layout with normalized coordinates
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<ModelVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // Position attribute
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                // Texture coordinates attribute
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                // Normal attribute
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 5]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

#[derive(Clone)]
pub struct Material {
    pub name: String,
    pub diffuse_texture: Texture,
    pub bind_group: wgpu::BindGroup,
}

pub struct Mesh {
    pub name: String,
    /// Where its indices start in its model's index buffer (see
    /// `Model::index_buffer`), and how many there are.
    pub first_index: u32,
    pub num_elements: u32,
    /// Where its vertices start in its model's vertex buffer - its indices
    /// count from its own first vertex, so this is added to them when it's
    /// drawn.
    pub base_vertex: i32,
    pub material: usize,
    pub transform_buffer: wgpu::Buffer,
    pub transform_bind_group: wgpu::BindGroup,
    pub transform: Transform3D,
    pub base_transform: Transform3D,
    pub parent_transform: Option<Transform3D>,
    pub alpha_mode: AlphaMode,
    /// Draw this mesh with backface culling OFF. Set at load from
    /// `asset_manager::gltf::DoubleSided` (declared per model at registration). The
    /// glTF `doubleSided` material flag is deliberately NOT used.
    pub double_sided: bool,
    /// The geometry's own box (min, max), in the mesh's local space - before
    /// its transform. Lets code that moves a mesh work from where its shape
    /// actually is, whether or not its origin sits on it (an exporter often
    /// leaves the origin at the model's, with the geometry baked off to the
    /// side - see `local_center`).
    pub local_bounds: ([f32; 3], [f32; 3]),
    /// `false` leaves it out of every pass - its shadow too. Set from a
    /// node's `ModelMesh::visible` on the node's own copy (see
    /// `properties::Model::mesh_mut`).
    pub visible: bool,
}

/// The object a mesh key belongs to: the key without the `#primN` an object
/// with several materials loads its later ones with (see
/// `Model::meshes_named_mut`).
pub(crate) fn object_name(key: &str) -> &str {
    key.split("#prim").next().unwrap_or(key)
}

/// # Model
/// A 3D model is defined by meshes, the "mesh_list" is for definition of different mesh types, for example separation of opaque and transparent ones.
pub struct Model {
    pub mesh_lists: HashMap<String, HashMap<String, Mesh>>,
    pub materials: Vec<Material>,
    /// Every mesh's vertices, one after another - one buffer for the whole
    /// model, so it's bound once to draw all of its meshes instead of once
    /// per mesh (see `Mesh::base_vertex`). Each mesh is still drawn on its
    /// own, with its own transform - only where its geometry lives is shared.
    pub vertex_buffer: wgpu::Buffer,
    /// Every mesh's indices, the same way (see `Mesh::first_index`).
    pub index_buffer: wgpu::Buffer,
}

impl Model {
    /// About how far from its origin its geometry reaches, at its own size -
    /// every mesh where its transform puts it.
    pub fn radius(&self) -> f32 {
        self.mesh_lists.values().flat_map(|meshes| meshes.values()).map(|mesh| {
            let (low, high) = mesh.local_bounds;
            let corner = Vector3::from(low).abs().sup(&Vector3::from(high).abs()).norm();
            // Under a parent, a mesh is turned about the parent's position -
            // at most that far again from where it'd be without.
            let (offset, scale) = match &mesh.parent_transform {
                Some(parent) => (2.0 * parent.position.norm() + mesh.transform.position.norm(), parent.scale),
                None => (mesh.transform.position.norm(), mesh.transform.scale),
            };
            offset + corner * scale.abs().max()
        }).fold(0.0, f32::max)
    }

    /// Every mesh of the object named `name` in list `list` ("opaque" /
    /// "transparent") - ALL of its materials. An object with several
    /// materials loads as one mesh per material: the first keyed by its
    /// plain name, the rest as "<name>#prim1", "#prim2"... (see
    /// asset_manager::gltf::traverse_node). Anything animating an object by name has
    /// to move all of them, or its materials drift apart.
    pub fn meshes_named_mut<'a>(&'a mut self, list: &str, name: &'a str) -> impl Iterator<Item = &'a mut Mesh> + 'a {
        let prefix = format!("{name}#prim");
        self.mesh_lists.get_mut(list).into_iter().flat_map(move |meshes| {
            let prefix = prefix.clone();
            meshes.iter_mut().filter(move |(key, _)| key.as_str() == name || key.starts_with(&prefix)).map(|(_, mesh)| mesh)
        })
    }

    /// A copy whose meshes move on their own - each with its own transform
    /// (buffer and bind group, 64 bytes) - but that shares everything else
    /// with this one: vertices, indices, textures, materials (wgpu's handles
    /// are reference-counted, so nothing's uploaded again). For giving each
    /// aircraft its own control surfaces, gear and afterburner without a
    /// whole second load of its model - see `Model::variant`.
    pub fn share(&self, device: &Device, queue: &wgpu::Queue) -> Model {
        let layout = Mesh::create_bind_group_layout(device);
        let mesh_lists = self.mesh_lists.iter()
            .map(|(list, meshes)| (list.clone(), meshes.iter().map(|(key, mesh)| (key.clone(), mesh.share(device, queue, &layout))).collect()))
            .collect();
        Model { mesh_lists, materials: self.materials.clone(), vertex_buffer: self.vertex_buffer.clone(), index_buffer: self.index_buffer.clone() }
    }
}

/// What a mesh's transform buffer holds (its bind group 2): where the mesh
/// is in its model, then the lowest corner of its geometry there - see
/// standard_template.wgsl's `Transform`.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MeshUniform {
    pub(crate) matrix: [[f32; 4]; 4],
    /// x, y, z - and 0, to fill the vector.
    pub(crate) bounds_min: [f32; 4],
}

impl MeshUniform {
    /// `transform` (the mesh's in its model), and the lowest corner of
    /// `local_bounds` (its geometry's, in its own space) once `transform` has
    /// put it there.
    pub(crate) fn new(transform: &Transform3D, local_bounds: ([f32; 3], [f32; 3])) -> Self {
        let matrix: [[f32; 4]; 4] = transform.matrix().into();
        let (lo, hi) = local_bounds;
        let mut bounds_min = [f32::MAX; 3];
        for corner in 0..8 {
            let p = [
                if corner & 1 == 0 { lo[0] } else { hi[0] },
                if corner & 2 == 0 { lo[1] } else { hi[1] },
                if corner & 4 == 0 { lo[2] } else { hi[2] },
            ];
            for axis in 0..3 {
                // Column-major: matrix * (p, 1).
                let moved = matrix[0][axis] * p[0] + matrix[1][axis] * p[1] + matrix[2][axis] * p[2] + matrix[3][axis];
                bounds_min[axis] = bounds_min[axis].min(moved);
            }
        }
        Self { matrix, bounds_min: [bounds_min[0], bounds_min[1], bounds_min[2], 0.0] }
    }
}

impl Mesh {
    /// This mesh with a transform of its own, starting where this one is -
    /// see `Model::share`.
    fn share(&self, device: &Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout) -> Mesh {
        let transform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shared mesh transform buffer"),
            size: mem::size_of::<MeshUniform>() as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&transform_buffer, 0, bytemuck::cast_slice(&[MeshUniform::new(&self.transform, self.local_bounds)]));
        let transform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shared mesh transform bind group"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: transform_buffer.as_entire_binding() }],
        });
        Mesh {
            name: self.name.clone(),
            first_index: self.first_index,
            num_elements: self.num_elements,
            base_vertex: self.base_vertex,
            material: self.material,
            transform_buffer,
            transform_bind_group,
            transform: self.transform,
            base_transform: self.base_transform,
            parent_transform: self.parent_transform,
            alpha_mode: self.alpha_mode,
            double_sided: self.double_sided,
            local_bounds: self.local_bounds,
            visible: self.visible,
        }
    }

    /// The middle of the geometry, in the mesh's local space.
    pub fn local_center(&self) -> Vector3<f32> {
        let (lo, hi) = self.local_bounds;
        Vector3::new((lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5)
    }

    /// Half the geometry's height (local Y) - e.g. a wheel's radius.
    pub fn local_half_height(&self) -> f32 {
        let (lo, hi) = self.local_bounds;
        (hi[1] - lo[1]) * 0.5
    }

    /// How far the geometry reaches below its middle, straight down in the
    /// model (not the mesh's own local Y) - under its authored rotation and
    /// scale. A wheel's radius, whichever way its mesh was modelled (e.g.
    /// turned 90° with its axle along local Y).
    pub fn depth_below_center(&self) -> f32 {
        let (lo, hi) = self.local_bounds;
        let base = &self.base_transform;
        let rotation = base.rotation;
        let lowest = (0..8).map(|corner| {
            let local = Vector3::new(
                if corner & 1 == 0 { lo[0] } else { hi[0] },
                if corner & 2 == 0 { lo[1] } else { hi[1] },
                if corner & 4 == 0 { lo[2] } else { hi[2] },
            );
            (rotation * base.scale.component_mul(&local)).y
        }).fold(f32::MAX, f32::min);
        self.center_offset().y - lowest
    }

    /// Where the geometry's middle ends up relative to the mesh's position,
    /// under its authored rotation/scale - add a position to it to get where
    /// the shape's middle is drawn.
    pub fn center_offset(&self) -> Vector3<f32> {
        let base = &self.base_transform;
        base.rotation * base.scale.component_mul(&self.local_center())
    }

    /// Writes `transform` - where it sits in its model - to the GPU, the same
    /// way loading it did (see `MeshUniform::new`): a mesh nobody moved is
    /// drawn exactly as loaded. (`transform` already has a nested mesh's
    /// parent in - see asset_manager::gltf::traverse_node.)
    pub fn update_transform(&self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.transform_buffer, 0, bytemuck::cast_slice(&[MeshUniform::new(&self.transform, self.local_bounds)]));
    }

    pub fn change_transform(&mut self, queue: &wgpu::Queue, transform: Transform3D) {
        if transform.position != self.transform.position || transform.rotation != self.transform.rotation || transform.scale != self.transform.scale {
            self.transform = transform;
            self.update_transform(queue);
        }
    }

     pub fn create_bind_group_layout(device: &Device) -> wgpu::BindGroupLayout {
        device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("transform_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        })
    }
}
