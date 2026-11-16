//! Models the engine builds instead of reading a file - `Mesh::cube`,
//! `plane`, `sphere`, `cylinder`, `cylinder_shape`, `capsule`, and a game's
//! own mesh (see `AssetManager::add_mesh`).

use std::collections::HashMap;
use std::sync::Arc;

use nalgebra::Vector3;

use wgpu::util::DeviceExt;

use crate::asset_manager::models::{MeshData, ModelGeometry, ModelSource};
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

/// A cylinder 1 across and 1 tall, centered, standing along Y, with `sides`
/// flat sides - each its own two triangles with its own flat normal (as
/// Blender draws its cylinder), and a flat cap top and bottom. Its corners
/// are on the circle, at angles 0, 1/sides, 2/sides... of a turn.
pub(crate) fn build_cylinder_shape_mesh(sides: u32) -> (Vec<ModelVertex>, Vec<u32>) {
    let sides = sides.max(3);
    let corner = |side: u32| {
        let (sin, cos) = (side as f32 / sides as f32 * std::f32::consts::TAU).sin_cos();
        (cos * 0.5, sin * 0.5)
    };
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    for side in 0..sides {
        let ((x0, z0), (x1, z1)) = (corner(side), corner(side + 1));
        // Facing out of the middle of the side.
        let (sin, cos) = ((side as f32 + 0.5) / sides as f32 * std::f32::consts::TAU).sin_cos();
        let normal = [cos, 0.0, sin];
        let (u0, u1) = (side as f32 / sides as f32, (side + 1) as f32 / sides as f32);
        let base = vertices.len() as u32;
        vertices.push(ModelVertex { position: [x0, 0.5, z0], tex_coords: [u0, 0.0], normal });
        vertices.push(ModelVertex { position: [x0, -0.5, z0], tex_coords: [u0, 1.0], normal });
        vertices.push(ModelVertex { position: [x1, -0.5, z1], tex_coords: [u1, 1.0], normal });
        vertices.push(ModelVertex { position: [x1, 0.5, z1], tex_coords: [u1, 0.0], normal });
        // Counter-clockwise seen from outside, as build_rings winds them.
        indices.extend_from_slice(&[base, base + 3, base + 2, base, base + 2, base + 1]);
    }
    for (y, up) in [(0.5, true), (-0.5, false)] {
        let normal = [0.0, if up { 1.0 } else { -1.0 }, 0.0];
        let center = vertices.len() as u32;
        vertices.push(ModelVertex { position: [0.0, y, 0.0], tex_coords: [0.5, 0.5], normal });
        for side in 0..sides {
            let (x, z) = corner(side);
            vertices.push(ModelVertex { position: [x, y, z], tex_coords: [0.5 + x, 0.5 + z], normal });
        }
        for side in 0..sides {
            let (here, next) = (center + 1 + side, center + 1 + (side + 1) % sides);
            if up {
                indices.extend_from_slice(&[center, next, here]);
            } else {
                indices.extend_from_slice(&[center, here, next]);
            }
        }
    }
    (vertices, indices)
}

/// A shape turned around Y through `rings` - (height, radius), bottom to
/// top - with `sides` flat sides. Each band between two rings is its own
/// flat faces, lit leaning as the band does (out, up where it narrows, down
/// where it widens, straight up on a step); the first ring is capped facing
/// down and the last facing up, unless they're points. Corners are at angles
/// 0, 1/sides, 2/sides... of a turn; heights are as given, not centered.
pub(crate) fn build_profile_shape_mesh(rings: &[(f32, f32)], sides: u32) -> (Vec<ModelVertex>, Vec<u32>) {
    let sides = sides.max(3);
    let at = |side: u32| (side as f32 / sides as f32 * std::f32::consts::TAU).sin_cos();
    // How far along the outline each ring is, for the texture's v.
    let mut along = vec![0.0];
    for pair in rings.windows(2) {
        let ((y0, r0), (y1, r1)) = (pair[0], pair[1]);
        along.push(along.last().unwrap() + ((y1 - y0).powi(2) + (r1 - r0).powi(2)).sqrt());
    }
    let length = along.last().copied().unwrap_or(0.0).max(1e-6);
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    for (band, pair) in rings.windows(2).enumerate() {
        let ((y0, r0), (y1, r1)) = (pair[0], pair[1]);
        if r0 <= 0.0 && r1 <= 0.0 || (y0 == y1 && r0 == r1) {
            continue; // No area: along the middle, or the same ring twice.
        }
        let (v0, v1) = (1.0 - along[band] / length, 1.0 - along[band + 1] / length);
        for side in 0..sides {
            let ((s0, c0), (s1, c1)) = (at(side), at(side + 1));
            // Out of the face's middle (r cos(pi/sides) from the axis),
            // turned as the band runs from ring to ring.
            let (sin, cos) = ((side as f32 + 0.5) / sides as f32 * std::f32::consts::TAU).sin_cos();
            let lean = (r0 - r1) * (std::f32::consts::PI / sides as f32).cos();
            let n = Vector3::new(cos * (y1 - y0), lean, sin * (y1 - y0)).try_normalize(1e-9).unwrap_or_else(Vector3::y);
            let normal = [n.x, n.y, n.z];
            let (u0, u1) = (side as f32 / sides as f32, (side + 1) as f32 / sides as f32);
            let base = vertices.len() as u32;
            vertices.push(ModelVertex { position: [c0 * r1, y1, s0 * r1], tex_coords: [u0, v1], normal });
            vertices.push(ModelVertex { position: [c0 * r0, y0, s0 * r0], tex_coords: [u0, v0], normal });
            vertices.push(ModelVertex { position: [c1 * r0, y0, s1 * r0], tex_coords: [u1, v0], normal });
            vertices.push(ModelVertex { position: [c1 * r1, y1, s1 * r1], tex_coords: [u1, v1], normal });
            // Counter-clockwise seen from outside; a pointed ring's
            // triangle has no area, so it's left out.
            if r1 > 0.0 {
                indices.extend_from_slice(&[base, base + 3, base + 2]);
            }
            if r0 > 0.0 {
                indices.extend_from_slice(&[base, base + 2, base + 1]);
            }
        }
    }
    let ends = [rings.first().map(|&(y, r)| (y, r, false)), rings.last().map(|&(y, r)| (y, r, true))];
    for (y, radius, up) in ends.into_iter().flatten() {
        if radius <= 0.0 || rings.len() < 2 {
            continue;
        }
        let normal = [0.0, if up { 1.0 } else { -1.0 }, 0.0];
        let center = vertices.len() as u32;
        vertices.push(ModelVertex { position: [0.0, y, 0.0], tex_coords: [0.5, 0.5], normal });
        for side in 0..sides {
            let (sin, cos) = at(side);
            vertices.push(ModelVertex { position: [cos * radius, y, sin * radius], tex_coords: [0.5 + cos * 0.5, 0.5 + sin * 0.5], normal });
        }
        for side in 0..sides {
            let (here, next) = (center + 1 + side, center + 1 + (side + 1) % sides);
            if up {
                indices.extend_from_slice(&[center, next, here]);
            } else {
                indices.extend_from_slice(&[center, here, next]);
            }
        }
    }
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

/// The geometry a `Mesh` of `source` is drawn with, for a collider: its
/// points (before the mesh's size and `transform`, see `Mesh::geometry`)
/// and triangles. `custom` is the meshes the game added
/// (`AssetManager::add_mesh`); None for a file, or one never added.
pub(crate) fn collision_geometry(source: &ModelSource, custom: &HashMap<String, Arc<MeshData>>) -> Option<(Vec<Vector3<f32>>, Vec<[u32; 3]>)> {
    let (vertices, indices) = match source {
        ModelSource::Cube => build_cube_mesh(),
        ModelSource::Plane { subdivisions } => build_plane_mesh(*subdivisions),
        ModelSource::Sphere => build_sphere_mesh(),
        ModelSource::Cylinder => build_cylinder_mesh(),
        ModelSource::CylinderShape { sides } => build_cylinder_shape_mesh(*sides),
        ModelSource::Capsule { radius, height } => build_capsule_mesh(*radius, *height),
        ModelSource::ProfileShape { rings, sides } => build_profile_shape_mesh(rings, *sides),
        ModelSource::Mesh(name) => {
            let mesh = custom.get(name)?;
            (mesh.vertices.clone(), mesh.indices.clone())
        }
        ModelSource::File(_) => return None,
    };
    let points = vertices.iter().map(|vertex| Vector3::from(vertex.position)).collect();
    let triangles = indices.chunks_exact(3).map(|triangle| [triangle[0], triangle[1], triangle[2]]).collect();
    Some((points, triangles))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cylinder_shape_has_its_sides_on_the_circle() {
        let (vertices, indices) = build_cylinder_shape_mesh(6);
        // 4 a side, plus the middle and 6 corners for each cap.
        assert_eq!(vertices.len(), 6 * 4 + 2 * 7);
        assert_eq!(indices.len(), 6 * 6 + 2 * 6 * 3);
        for vertex in vertices.iter().filter(|vertex| vertex.position[0] != 0.0 || vertex.position[2] != 0.0) {
            let out = (vertex.position[0].powi(2) + vertex.position[2].powi(2)).sqrt();
            assert!((out - 0.5).abs() < 1e-5);
        }
        // Each side's normal points out of its middle, flat.
        let side = &vertices[0..4];
        assert!(side.iter().all(|vertex| vertex.normal == side[0].normal));
        assert!(build_cylinder_shape_mesh(1).0.len() == build_cylinder_shape_mesh(3).0.len());
    }

    #[test]
    fn a_meshs_collision_geometry_is_what_it_draws() {
        let custom = HashMap::new();
        let (points, triangles) = collision_geometry(&ModelSource::CylinderShape { sides: 8 }, &custom).unwrap();
        let hull = rapier3d::prelude::SharedShape::convex_hull(&points.iter().map(|point| nalgebra::Point3::from(*point)).collect::<Vec<_>>()).unwrap();
        // A prism's corners: 8 around the top, 8 around the bottom.
        assert_eq!(hull.as_convex_polyhedron().unwrap().points().len(), 16);
        assert!(!triangles.is_empty());
        assert!(collision_geometry(&ModelSource::Mesh("never added".to_owned()), &custom).is_none());
        // A pointed cone: its corners around the bottom, and the tip.
        let (points, _) = collision_geometry(&ModelSource::ProfileShape { rings: vec![(0.0, 1.0), (2.0, 0.0)], sides: 10 }, &custom).unwrap();
        let hull = rapier3d::prelude::SharedShape::convex_hull(&points.iter().map(|point| nalgebra::Point3::from(*point)).collect::<Vec<_>>()).unwrap();
        assert_eq!(hull.as_convex_polyhedron().unwrap().points().len(), 11);
        assert!(collision_geometry(&ModelSource::File("a.glb".to_owned()), &custom).is_none());
    }

    #[test]
    fn a_profile_shape_follows_its_rings_and_lights_each_band_leaning() {
        // A tapered cup: two rings, both capped.
        let (vertices, indices) = build_profile_shape_mesh(&[(0.0, 1.0), (2.0, 0.5)], 8);
        // 4 a side, plus a middle and 8 corners for each cap.
        assert_eq!(vertices.len(), 8 * 4 + 2 * 9);
        assert_eq!(indices.len(), 8 * 6 + 2 * 8 * 3);
        let radius = |v: &ModelVertex| (v.position[0].powi(2) + v.position[2].powi(2)).sqrt();
        assert!(vertices.iter().filter(|v| v.position[1] > 1.0 && radius(v) > 0.0).all(|v| (radius(v) - 0.5).abs() < 1e-5));
        assert!(vertices.iter().filter(|v| v.position[1] < 1.0 && radius(v) > 0.0).all(|v| (radius(v) - 1.0).abs() < 1e-5));
        // Narrower on top: the sides face a little up.
        assert!(vertices[0].normal[1] > 0.0);

        // Pointed at the top: no top cap, one triangle a side.
        let (vertices, indices) = build_profile_shape_mesh(&[(0.0, 1.0), (2.0, 0.0)], 8);
        assert_eq!(vertices.len(), 8 * 4 + 9);
        assert_eq!(indices.len(), 8 * 3 + 8 * 3);

        // A waist: the lower band narrows, so it faces up and out;
        // the upper band widens - down and out; a step faces straight up.
        let (vertices, _) = build_profile_shape_mesh(&[(0.0, 1.0), (1.0, 0.5), (2.0, 1.0), (2.0, 0.6), (3.0, 0.6)], 8);
        let band = |index: usize| vertices[index * 8 * 4].normal;
        assert!(band(0)[1] > 0.0 && band(0)[0] > 0.0);
        assert!(band(1)[1] < 0.0 && band(1)[0] > 0.0);
        assert!((band(2)[1] - 1.0).abs() < 1e-5);
        assert!(band(3)[1].abs() < 1e-5);
        // Its middle ring really is narrower.
        assert!(vertices.iter().filter(|v| (v.position[1] - 1.0).abs() < 1e-5).all(|v| (radius(v) - 0.5).abs() < 1e-5));

        // Nothing to turn with fewer than two rings.
        assert!(build_profile_shape_mesh(&[(0.0, 1.0)], 8).0.is_empty());
    }
}
