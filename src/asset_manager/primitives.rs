//! Models the engine builds instead of reading a file - `Mesh::cube`,
//! `plane`, `sphere`, `cylinder`, `capsule`, and a game's own mesh (see
//! `AssetManager::add_mesh`).

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use crate::asset_manager::models::ModelGeometry;
use crate::rendering::models::model::{self, Model, ModelVertex};
use crate::rendering::models::textures::Texture;
use crate::scene_manager::node::properties::Transform3D;

/// Builds a `Cube`'s raw geometry - 4 vertices per face (24 total) rather
/// than 8 shared ones, since each face needs its own flat normal/UV, which a
/// shared-corner vertex can't hold. Winding is CCW as viewed from outside
/// each face, matching the engine's `FrontFace::Ccw` + back-face culling
/// convention (see e.g. `light.rs`'s pipeline).
pub(crate) fn build_cube_mesh() -> (Vec<ModelVertex>, Vec<u32>) {
    // (normal, corners) per face - corners already in CCW-from-outside order.
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, 1.0], [[-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]]),
        ([0.0, 0.0, -1.0], [[0.5, -0.5, -0.5], [-0.5, -0.5, -0.5], [-0.5, 0.5, -0.5], [0.5, 0.5, -0.5]]),
        ([1.0, 0.0, 0.0], [[0.5, -0.5, 0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [0.5, 0.5, 0.5]]),
        ([-1.0, 0.0, 0.0], [[-0.5, -0.5, -0.5], [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5], [-0.5, 0.5, -0.5]]),
        ([0.0, 1.0, 0.0], [[-0.5, 0.5, 0.5], [0.5, 0.5, 0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5]]),
        ([0.0, -1.0, 0.0], [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, -0.5, 0.5], [-0.5, -0.5, 0.5]]),
    ];
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, corners) in faces {
        let base = vertices.len() as u32;
        for (corner, uv) in corners.iter().zip(uvs.iter()) {
            vertices.push(ModelVertex { position: *corner, tex_coords: *uv, normal });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

/// Builds a `Plane { subdivisions }`'s raw geometry - a `(subdivisions + 2)`-
/// per-side grid of shared vertices (a flat plane has no per-face normal/UV
/// seams to worry about, unlike `build_cube_mesh`), UVs spanning 0..1 across
/// the whole grid. Winding is CCW as viewed from +Y, matching `normal`.
pub(crate) fn build_plane_mesh(subdivisions: u32) -> (Vec<ModelVertex>, Vec<u32>) {
    let segments = subdivisions + 1;
    let verts_per_side = segments + 1;

    let mut vertices = Vec::with_capacity((verts_per_side * verts_per_side) as usize);
    for row in 0..verts_per_side {
        for col in 0..verts_per_side {
            let u = col as f32 / segments as f32;
            let v = row as f32 / segments as f32;
            vertices.push(ModelVertex { position: [u - 0.5, 0.0, v - 0.5], tex_coords: [u, v], normal: [0.0, 1.0, 0.0] });
        }
    }

    let mut indices = Vec::with_capacity((segments * segments * 6) as usize);
    for row in 0..segments {
        for col in 0..segments {
            let top_left = row * verts_per_side + col;
            let top_right = top_left + 1;
            let bottom_left = top_left + verts_per_side;
            let bottom_right = bottom_left + 1;
            indices.extend_from_slice(&[top_left, bottom_left, bottom_right, top_left, bottom_right, top_right]);
        }
    }
    (vertices, indices)
}

const ROUND_SEGMENTS: u32 = 32;
const HALF_RINGS: u32 = 8;

/// Rings of vertices around Y, stacked top to bottom - each ring
/// `(y offset, angle down from +Y)`, every point `radius` from its ring's
/// axis point. Consecutive rings are joined by quads, counter-clockwise
/// seen from outside.
fn build_rings(rings: &[(f32, f32)], radius: f32, vertices: &mut Vec<ModelVertex>, indices: &mut Vec<u32>) {
    let base = vertices.len() as u32;
    let per_ring = ROUND_SEGMENTS + 1;
    for (ring, (offset, down)) in rings.iter().enumerate() {
        let (sin_down, cos_down) = down.sin_cos();
        for segment in 0..per_ring {
            let around = segment as f32 / ROUND_SEGMENTS as f32 * std::f32::consts::TAU;
            let normal = [sin_down * around.cos(), cos_down, sin_down * around.sin()];
            vertices.push(ModelVertex {
                position: [normal[0] * radius, normal[1] * radius + offset, normal[2] * radius],
                tex_coords: [segment as f32 / ROUND_SEGMENTS as f32, ring as f32 / (rings.len() - 1) as f32],
                normal,
            });
        }
    }
    for ring in 0..rings.len() as u32 - 1 {
        for segment in 0..ROUND_SEGMENTS {
            let a = base + ring * per_ring + segment;
            let (b, c, d) = (a + per_ring, a + per_ring + 1, a + 1);
            indices.extend_from_slice(&[a, d, c, a, c, b]);
        }
    }
}

/// A flat disc at `y`, facing up or down.
fn build_cap(y: f32, radius: f32, up: bool, vertices: &mut Vec<ModelVertex>, indices: &mut Vec<u32>) {
    let normal = [0.0, if up { 1.0 } else { -1.0 }, 0.0];
    let center = vertices.len() as u32;
    vertices.push(ModelVertex { position: [0.0, y, 0.0], tex_coords: [0.5, 0.5], normal });
    for segment in 0..=ROUND_SEGMENTS {
        let around = segment as f32 / ROUND_SEGMENTS as f32 * std::f32::consts::TAU;
        let (sin, cos) = around.sin_cos();
        vertices.push(ModelVertex { position: [cos * radius, y, sin * radius], tex_coords: [0.5 + cos * 0.5, 0.5 + sin * 0.5], normal });
    }
    for segment in 0..ROUND_SEGMENTS {
        let (here, next) = (center + 1 + segment, center + 2 + segment);
        if up {
            indices.extend_from_slice(&[center, next, here]);
        } else {
            indices.extend_from_slice(&[center, here, next]);
        }
    }
}

/// A sphere 1 across, centered.
pub(crate) fn build_sphere_mesh() -> (Vec<ModelVertex>, Vec<u32>) {
    let rings: Vec<(f32, f32)> = (0..=HALF_RINGS * 2)
        .map(|ring| (0.0, ring as f32 / (HALF_RINGS * 2) as f32 * std::f32::consts::PI))
        .collect();
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    build_rings(&rings, 0.5, &mut vertices, &mut indices);
    (vertices, indices)
}

/// A cylinder 1 across and 1 tall, centered, standing along Y.
pub(crate) fn build_cylinder_mesh() -> (Vec<ModelVertex>, Vec<u32>) {
    let side = std::f32::consts::FRAC_PI_2;
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    build_rings(&[(0.5, side), (-0.5, side)], 0.5, &mut vertices, &mut indices);
    build_cap(0.5, 0.5, true, &mut vertices, &mut indices);
    build_cap(-0.5, 0.5, false, &mut vertices, &mut indices);
    (vertices, indices)
}

/// A capsule `radius` around and `height` from tip to tip, centered,
/// standing along Y - two half spheres with a cylinder between them (none,
/// when `height` is no more than its width).
pub(crate) fn build_capsule_mesh(radius: f32, height: f32) -> (Vec<ModelVertex>, Vec<u32>) {
    let half_middle = (height / 2.0 - radius).max(0.0);
    let quarter = std::f32::consts::FRAC_PI_2;
    let mut rings: Vec<(f32, f32)> = (0..=HALF_RINGS).map(|ring| (half_middle, ring as f32 / HALF_RINGS as f32 * quarter)).collect();
    rings.extend((0..=HALF_RINGS).map(|ring| (-half_middle, quarter + ring as f32 / HALF_RINGS as f32 * quarter)));
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    build_rings(&rings, radius, &mut vertices, &mut indices);
    (vertices, indices)
}

/// A model of one mesh, `vertices` and `indices` (three a triangle,
/// counter-clockwise seen from the front), plain white until a `Material`
/// says otherwise - one of `Mesh`'s shapes, or a game's mesh (see
/// `AssetManager::add_mesh`). `name` is its mesh's name.
pub(crate) fn build_mesh_model(name: &str, vertices: &[ModelVertex], indices: &[u32], double_sided: bool, device: &wgpu::Device, queue: &wgpu::Queue, transform_bind_group_layout: &wgpu::BindGroupLayout) -> Result<Model, String> {

    let texture_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
        label: Some("primitive_texture_bind_group_layout"),
    });

    let default_texture = Texture::from_image(
        &::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(1, 1, ::image::Rgba([255, 255, 255, 255]))),
        device,
        queue,
        Some("primitive_default_texture"),
    ).map_err(|e| format!("failed to build a default texture for primitive '{name}': {e}"))?;

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &texture_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&default_texture.view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&default_texture.sampler) },
        ],
        label: Some("primitive_material_bind_group"),
    });

    let materials = vec![model::Material { name: "Default Material".to_owned(), diffuse_texture: default_texture, bind_group }];

    let mut geometry = ModelGeometry::default();
    let (first_index, base_vertex) = geometry.add(vertices, indices);

    // Identity - a primitive's actual placement/size comes from its node's
    // own Transform3D, same as every mesh loaded via load_model_with.
    let transform = Transform3D::default();
    let mut local_bounds = ([f32::MAX; 3], [f32::MIN; 3]);
    for vertex in vertices {
        for axis in 0..3 {
            local_bounds.0[axis] = local_bounds.0[axis].min(vertex.position[axis]);
            local_bounds.1[axis] = local_bounds.1[axis].max(vertex.position[axis]);
        }
    }
    let transform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("primitive transform buffer"),
        contents: bytemuck::cast_slice(&[model::MeshUniform::new(&transform, local_bounds)]),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let transform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("primitive transform bind group"),
        layout: transform_bind_group_layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: transform_buffer.as_entire_binding() }],
    });

    let mesh = model::Mesh {
        name: name.to_owned(),
        first_index,
        num_elements: indices.len() as u32,
        base_vertex,
        material: 0,
        transform_buffer,
        transform_bind_group,
        transform,
        base_transform: transform,
        parent_transform: None,
        alpha_mode: gltf::material::AlphaMode::Opaque,
        double_sided,
        local_bounds,
        visible: true,
    };
    let mut opaque = HashMap::new();
    opaque.insert(name.to_owned(), mesh);

    let mut mesh_lists = HashMap::new();
    mesh_lists.insert("opaque".to_owned(), opaque);

    let (vertex_buffer, index_buffer) = geometry.upload(device, name);
    Ok(model::Model { mesh_lists, materials, vertex_buffer, index_buffer })
}
