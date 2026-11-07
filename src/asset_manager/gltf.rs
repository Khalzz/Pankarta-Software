//! Reading `.gltf`/`.glb` model files - into a `Model` to draw (see
//! `ModelLoad::load`), or just their triangles, for a physics collider.

use std::collections::HashMap;
use std::path::Path;

use gltf::{image, Gltf};
use nalgebra::{Quaternion, UnitQuaternion, Vector3};
use wgpu::util::DeviceExt;

use crate::asset_manager::load_asset_binary;
use crate::asset_manager::models::ModelGeometry;
use crate::rendering::models::model::{self, Mesh, Model, ModelVertex};
use crate::rendering::models::textures::Texture;
use crate::scene_manager::node::properties::Transform3D;

/// Flip to `true` to dump every model's materials/textures/primitives to
/// stdout as it loads (see `model_log!` uses in `load_model_with` /
/// `traverse_node`).
const DEBUG_MODEL_LOAD: bool = false;

macro_rules! model_log {
    ($($arg:tt)*) => { if DEBUG_MODEL_LOAD { println!($($arg)*); } };
}

/// A linear color channel (0..1) encoded to sRGB (0..1) - the standard
/// transfer curve.
fn linear_to_srgb(linear: f32) -> f32 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// Loads a `.gltf`/`.glb` through `read` - which turns a path (the model's
/// own `file_name`, or one of its buffers/textures, resolved next to it)
/// into bytes.
pub(crate) fn load_model_with(file_name: &str, read: &dyn Fn(&Path) -> anyhow::Result<Vec<u8>>, device: &wgpu::Device, queue: &wgpu::Queue, transform_bind_group_layout: &wgpu::BindGroupLayout, double_sided: DoubleSided) -> anyhow::Result<Model> {
    // Gltf::from_slice auto-detects the format from the header, so this handles
    // both text .gltf (JSON) and binary .glb files.
    let gltf_data = read(Path::new(file_name))?;
    let gltf = Gltf::from_slice(&gltf_data).unwrap();

    // Load buffers
    let mut buffer_data = Vec::new();
    for buffer in gltf.buffers() {
        match buffer.source() {
            gltf::buffer::Source::Bin => {
                if let Some(blob) = gltf.blob.as_deref() {
                    buffer_data.push(blob.to_vec());
                }
            }
            gltf::buffer::Source::Uri(uri) => {
                let file_dir = Path::new(file_name).parent().unwrap_or(Path::new(""));
                let full_path = file_dir.join(uri);
                let bin = read(&full_path)?;
                buffer_data.push(bin);
            }
        }
    }

    // Load materials
    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
                label: Some("texture_bind_group_layout"),
            });
    
    model_log!(
        "[model {}] {} material(s), {} mesh(es), {} node(s)",
        file_name,
        gltf.materials().count(),
        gltf.meshes().count(),
        gltf.nodes().count(),
    );

    let mut materials = Vec::new();
    // Every image decoded once, by its index in the file - many materials
    // share one (the F-16's 64 materials use 10), and decoding a PNG is by
    // far the slowest part of loading a model. Textures are GPU handles,
    // so sharing one is just a clone.
    let mut decoded: HashMap<usize, Texture> = HashMap::new();
    for (mat_index, material) in gltf.materials().enumerate() {
        let pbr = material.pbr_metallic_roughness();

        let texture_source = pbr
            .base_color_texture()
            .map(|tex| (tex.texture().source().index(), tex.texture().source().source()));

        let Some((image_index, texture_source)) = texture_source else {
            // No base color texture assigned (e.g. a flat-color/procedural material) -
            // fall back to a solid-color texture built from the material's base_color_factor.
            let [r, g, b, a] = pbr.base_color_factor();
            model_log!(
                "[model {}]   material {} \"{}\": no base-color texture -> solid color rgba({:.2}, {:.2}, {:.2}, {:.2})",
                file_name, mat_index, material.name().unwrap_or("<unnamed>"), r, g, b, a,
            );
            // glTF's factor is linear, but the texture is sRGB (the GPU
            // decodes it to linear when sampling) - so it's encoded to sRGB
            // first, or the curve would be applied twice (darker, more
            // saturated than the color picked). Alpha is linear either way.
            let srgb = |channel: f32| (linear_to_srgb(channel) * 255.0).round() as u8;
            let solid_texture = Texture::from_image(
                &::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(
                    1,
                    1,
                    ::image::Rgba([
                        srgb(r),
                        srgb(g),
                        srgb(b),
                        (a.clamp(0.0, 1.0) * 255.0).round() as u8,
                    ]),
                )),
                device,
                queue,
                Some("solid_color_texture"),
            ).expect("Couldn't create solid color texture");

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&solid_texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&solid_texture.sampler),
                    },
                ],
                label: None,
            });

            materials.push(model::Material {
                name: material.name().unwrap_or("Default Material").to_string(),
                diffuse_texture: solid_texture,
                bind_group,
            });

            continue;
        };

        match texture_source {
            gltf::image::Source::View { view, .. } => {
                    model_log!(
                        "[model {}]   material {} \"{}\": embedded base-color texture ({} bytes in buffer {})",
                        file_name, mat_index, material.name().unwrap_or("<unnamed>"),
                        view.length(), view.buffer().index(),
                    );
                    let buffer = &buffer_data[view.buffer().index()];
                    let start = view.offset();
                    let end = start + view.length();
                    let diffuse_texture = match decoded.get(&image_index) {
                        Some(texture) => texture.clone(),
                        None => {
                            let texture = Texture::from_bytes(&buffer[start..end], device, queue, file_name).expect("Couldn't load diffuse");
                            decoded.insert(image_index, texture.clone());
                            texture
                        }
                    };
                    
                    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        layout: &bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                            },
                        ],
                        label: None,
                    });

                    materials.push(model::Material {
                        name: material.name().unwrap_or("Default Material").to_string(),
                        diffuse_texture,
                        bind_group,
                    });
                }
            image::Source::Uri { uri, mime_type: _ } => {
                let file_dir = Path::new(file_name).parent().unwrap_or(Path::new(""));

                // Join the GLTF directory with the URI to get the correct path.
                let full_path = file_dir.join(uri);
                model_log!(
                    "[model {}]   material {} \"{}\": external base-color texture \"{}\"",
                    file_name, mat_index, material.name().unwrap_or("<unnamed>"),
                    full_path.display(),
                );
                let diffuse_texture = match decoded.get(&image_index) {
                    Some(texture) => texture.clone(),
                    None => {
                        let texture = Texture::from_bytes(&read(&full_path)?, device, queue, &full_path.to_string_lossy())?;
                        decoded.insert(image_index, texture.clone());
                        texture
                    }
                };

                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: &bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                        },
                    ],
                    label: None,
                });

                materials.push(model::Material {
                    name: material.name().unwrap_or("Default Material").to_string(),
                    diffuse_texture,
                    bind_group,
                });
            },
        };
    }

    // Primitives with no material assigned (valid glTF - it means "use the default material")
    // fall back to this plain white material so they still render instead of indexing out of bounds.
    let default_material_index = materials.len();
    let default_texture = Texture::from_image(
        &::image::DynamicImage::ImageRgba8(::image::RgbaImage::from_pixel(1, 1, ::image::Rgba([255, 255, 255, 255]))),
        device,
        queue,
        Some("default_texture"),
    ).expect("Couldn't create default texture");
    let default_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&default_texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&default_texture.sampler),
            },
        ],
        label: Some("default_material_bind_group"),
    });
    materials.push(model::Material {
        name: "Default Material".to_string(),
        diffuse_texture: default_texture,
        bind_group: default_bind_group,
    });

    let mut mesh_lists = HashMap::new();
    let mut geometry = ModelGeometry::default();

    for scene in gltf.scenes() {
        for node in scene.nodes() {
            traverse_node(node, &buffer_data, device, queue, transform_bind_group_layout, &mut mesh_lists, &mut geometry, file_name, None, default_material_index, double_sided)?;
        }
    }
    let (vertex_buffer, index_buffer) = geometry.upload(device, file_name);

    Ok(model::Model {
        mesh_lists,
        materials,
        vertex_buffer,
        index_buffer,
    })
}

/// Extracts a mesh's raw triangle geometry (world-space vertex positions +
/// triangle indices, no GPU upload, no materials) from a `.gltf`/`.glb` file -
/// for building a `ColliderShape::Trimesh` from a model's own
/// shape instead of a hand-authored primitive (see that variant's own doc
/// comment). Reads the file independently of `load_model_with`'s own
/// GPU-facing load rather than reusing its result - a `Mesh` only keeps
/// `wgpu::Buffer`s once loaded (see that struct's own fields), nothing
/// CPU-readable survives to reuse here, so rendering + physics both wanting
/// the same model means a small amount of duplicate file I/O, paid once at
/// scene-construction time.
pub fn load_trimesh_geometry(file_name: &str) -> anyhow::Result<(Vec<Vector3<f32>>, Vec<[u32; 3]>)> {
    let gltf_data = load_asset_binary(file_name)?;
    let gltf = Gltf::from_slice(&gltf_data).unwrap();

    let mut buffer_data = Vec::new();
    for buffer in gltf.buffers() {
        match buffer.source() {
            gltf::buffer::Source::Bin => {
                if let Some(blob) = gltf.blob.as_deref() {
                    buffer_data.push(blob.to_vec());
                }
            }
            gltf::buffer::Source::Uri(uri) => {
                let file_dir = Path::new(file_name).parent().unwrap_or(Path::new(""));
                let full_path = file_dir.join(uri);
                let bin = load_asset_binary(full_path.to_str().unwrap())?;
                buffer_data.push(bin);
            }
        }
    }

    let mut vertices: Vec<Vector3<f32>> = Vec::new();
    let mut indices: Vec<[u32; 3]> = Vec::new();

    for scene in gltf.scenes() {
        for node in scene.nodes() {
            collect_trimesh_geometry(node, &buffer_data, None, &mut vertices, &mut indices);
        }
    }

    Ok((vertices, indices))
}

// Recurses the same way traverse_node does (child transforms compose with
// their parent's), but only ever touches positions/indices - no GPU device,
// no materials, since this is only ever building collision geometry, not
// anything rendered.
fn collect_trimesh_geometry(
    node: gltf::Node<'_>,
    buffer_data: &[Vec<u8>],
    parent_transform: Option<(Vector3<f32>, UnitQuaternion<f32>, Vector3<f32>)>,
    vertices: &mut Vec<Vector3<f32>>,
    indices: &mut Vec<[u32; 3]>,
) {
    let (translation, rotation, scale) = node.transform().decomposed();
    let local_translation = Vector3::from(translation);
    let local_rotation = UnitQuaternion::from_quaternion(Quaternion::from(rotation));
    let local_scale = Vector3::from(scale);

    // Compose parent * local (scale then rotate then translate at each level).
    // Non-uniform scale through a rotated hierarchy isn't perfectly
    // representable this way, but it's exact for the common single-level case
    // (a root mesh node with its own scale, e.g. Runway.glb's "Island") and a
    // good approximation otherwise - and it matches what the collider needs to
    // line up with the rendered mesh, which had the same scale-dropping bug
    // (see traverse_node).
    let (world_translation, world_rotation, world_scale) = match parent_transform {
        Some((parent_translation, parent_rotation, parent_scale)) => (
            parent_translation + parent_rotation * parent_scale.component_mul(&local_translation),
            parent_rotation * local_rotation,
            parent_scale.component_mul(&local_scale),
        ),
        None => (local_translation, local_rotation, local_scale),
    };

    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            let reader = primitive.reader(|buffer| Some(&buffer_data[buffer.index()]));

            let base_index = vertices.len() as u32;
            if let Some(positions) = reader.read_positions() {
                for position in positions {
                    vertices.push(world_translation + world_rotation * world_scale.component_mul(&Vector3::from(position)));
                }
            }
            if let Some(indices_raw) = reader.read_indices() {
                let flat: Vec<u32> = indices_raw.into_u32().collect();
                for triangle in flat.chunks_exact(3) {
                    indices.push([base_index + triangle[0], base_index + triangle[1], base_index + triangle[2]]);
                }
            }
        }
    }

    for child in node.children() {
        collect_trimesh_geometry(child, buffer_data, Some((world_translation, world_rotation, world_scale)), vertices, indices);
    }
}

fn traverse_node(node: gltf::Node<'_>, buffer_data: &[Vec<u8>], device: &wgpu::Device, queue: &wgpu::Queue, transform_bind_group_layout: &wgpu::BindGroupLayout, mesh_lists: &mut HashMap<String, HashMap<String, Mesh>>, geometry: &mut ModelGeometry, file_name: &str, parent_transform: Option<([f32; 3], [f32; 4], [f32; 3])>, default_material_index: usize, double_sided: DoubleSided) -> anyhow::Result<()> {
        // Nodes without a mesh (Blender Empties, armatures, pure parent/group
        // nodes) are valid glTF - just recurse past them into their children.
        let Some(mesh) = node.mesh() else {
            for child in node.children() {
                traverse_node(child, buffer_data, device, queue, transform_bind_group_layout, mesh_lists, geometry, file_name, Some(node.transform().decomposed()), default_material_index, double_sided)?;
            }
            return Ok(());
        };
        let primitives = mesh.primitives();
        primitives.for_each(|primitive| {
            let reader = primitive.reader(|buffer| Some(&buffer_data[buffer.index()]));

            let mut vertices = Vec::new();
                if let Some(vertex_attribute) = reader.read_positions() {
                    vertex_attribute.for_each(|vertex| {
                        vertices.push(ModelVertex {
                            position: vertex,
                            tex_coords: Default::default(),
                            normal: Default::default(),
                        })
                    });
                }
                if let Some(normal_attribute) = reader.read_normals() {
                    let mut normal_index = 0;
                    normal_attribute.for_each(|normal| {
                        vertices[normal_index].normal = normal;
                        normal_index += 1;
                    });
                }
                if let Some(tex_coord_attribute) = reader.read_tex_coords(0).map(|v| v.into_f32()) {
                    let mut tex_coord_index = 0;
                    tex_coord_attribute.for_each(|tex_coord| {
                        vertices[tex_coord_index].tex_coords = tex_coord;
                        tex_coord_index += 1;
                    });
                }

            let mut indices = Vec::new();
            if let Some(indices_raw) = reader.read_indices() {
                indices.append(&mut indices_raw.into_u32().collect::<Vec<u32>>());
            }

            // Into the model's shared buffers (see ModelGeometry).
            let (first_index, base_vertex) = geometry.add(&vertices, &indices);

            let transform: Transform3D;
            let mut parent_values = None;

            match parent_transform {
                Some(parent_data) => {
                    let (parent_translation, parent_rotation, parent_scale) = parent_data;
                    let (translation, rotation, scale) = node.transform().decomposed();

                    let position = Vector3::from(parent_translation) + Vector3::from(translation);
                    let rotation = UnitQuaternion::from_quaternion(Quaternion::from(parent_rotation) * Quaternion::from(rotation));
                    // Keep this node's own scale - parent scale is carried
                    // separately in `parent_values` below. Forcing (1,1,1) here
                    // silently dropped any per-node scale a glTF exporter left
                    // on a mesh node (Blender routinely does, e.g. Runway.glb's
                    // "Island" node), rendering it ~1/scale too small.
                    transform = Transform3D { position, rotation, scale: Vector3::from(scale) };
                    parent_values = Some(Transform3D { position: parent_translation.into(), rotation: UnitQuaternion::from_quaternion(Quaternion::from(parent_rotation)), scale: parent_scale.into() });
                },
                None => {
                    let (translation, rotation, scale) = node.transform().decomposed();
                    transform = Transform3D { position: translation.into(), rotation: UnitQuaternion::from_quaternion(Quaternion::from(rotation)), scale: Vector3::from(scale) };
                },
            }

            // The corner is filled in once the geometry's bounds are known,
            // below.
            let uniform = model::MeshUniform { matrix: transform.matrix().into(), bounds_min: [0.0; 4] };
            let transform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("Transform Buffer")),
                contents: bytemuck::cast_slice(&[uniform]),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });

            let transform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("transform bind group"),
                layout: &transform_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: transform_buffer.as_entire_binding(),
                    },
                ],
            });

            // A mesh split across several materials in the DCC tool exports as
            // ONE node -> ONE mesh -> multiple primitives. The mesh map is
            // keyed by node name, so without this the primitives all land on
            // the same key and clobber each other (only the last material
            // would render). First primitive keeps the plain node name (so
            // the `wheel-f` / `left_elevator` / ... name lookups elsewhere
            // still resolve); extras get a "#primN" suffix. Single-primitive
            // meshes - the common case - are completely unchanged.
            // Local-space AABB of this primitive (kept on the mesh - see
            // Mesh::local_bounds - and logged below).
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &vertices {
                for a in 0..3 {
                    lo[a] = lo[a].min(v.position[a]);
                    hi[a] = hi[a].max(v.position[a]);
                }
            }
            if vertices.is_empty() {
                (lo, hi) = ([0.0; 3], [0.0; 3]);
            }
            queue.write_buffer(&transform_buffer, 0, bytemuck::cast_slice(&[model::MeshUniform::new(&transform, (lo, hi))]));

            let node_name = node.name().map(str::to_owned).unwrap_or_else(|| format!("node{}", node.index()));
            let key = if primitive.index() == 0 {
                node_name.clone()
            } else {
                format!("{}#prim{}", node_name, primitive.index())
            };

            let mesh = model::Mesh {
                name: file_name.to_string(),
                first_index,
                num_elements: indices.len() as u32,
                base_vertex,
                material: primitive.material().index().unwrap_or(default_material_index),
                transform_buffer,
                transform_bind_group,
                transform,
                base_transform: transform,
                parent_transform: parent_values,
                alpha_mode: primitive.material().alpha_mode(),
                // Node name matches all of a node's primitives; the "#primN"
                // key matches just one.
                double_sided: double_sided.covers(&node_name) || double_sided.covers(&key),
                local_bounds: (lo, hi),
            };

            let list = if primitive.material().alpha_mode() == gltf::material::AlphaMode::Blend || primitive.material().alpha_mode() == gltf::material::AlphaMode::Mask {
                "transparent"
            } else {
                "opaque"
            };

            // Where the node's own transform (already baked into `transform`
            // above) puts the primitive's AABB.
            let corner_after = |p: [f32; 3]| {
                let m: [[f32; 4]; 4] = transform.matrix().into();
                // column-major mat * (p, 1)
                [
                    m[0][0] * p[0] + m[1][0] * p[1] + m[2][0] * p[2] + m[3][0],
                    m[0][1] * p[0] + m[1][1] * p[1] + m[2][1] * p[2] + m[3][1],
                    m[0][2] * p[0] + m[1][2] * p[1] + m[2][2] * p[2] + m[3][2],
                ]
            };
            let a_lo = corner_after(lo);
            let a_hi = corner_after(hi);
            model_log!(
                "[model {}]   node \"{}\" / mesh \"{}\" prim {}: {} verts, {} indices, material {} ({:?}) -> \"{}\" list as key \"{}\"",
                file_name,
                node_name,
                node.mesh().and_then(|m| m.name().map(str::to_owned)).unwrap_or_else(|| "<unnamed>".to_owned()),
                primitive.index(),
                vertices.len(),
                indices.len(),
                primitive.material().index().map(|i| i.to_string()).unwrap_or_else(|| format!("default({default_material_index})")),
                primitive.material().alpha_mode(),
                list,
                key,
            );
            {
                let (t, r, s) = node.transform().decomposed();
                model_log!(
                    "[model {}]       node transform T={:?} R={:?} S={:?} | local AABB [{:.2},{:.2},{:.2}]..[{:.2},{:.2},{:.2}] -> after node xf [{:.1},{:.1},{:.1}]..[{:.1},{:.1},{:.1}]",
                    file_name, t, r, s,
                    lo[0], lo[1], lo[2], hi[0], hi[1], hi[2],
                    a_lo[0], a_lo[1], a_lo[2], a_hi[0], a_hi[1], a_hi[2],
                );
            }
            add_or_init_mesh_list(mesh_lists, &list.to_string(), key, mesh);

        });
    for child in node.children() {
        traverse_node(child, buffer_data, device, queue, transform_bind_group_layout, mesh_lists, geometry, file_name, Some(node.transform().decomposed()), default_material_index, double_sided)?;
    }

    Ok(())
}

/// # Add or init mesh list
/// This function is used to create a mesh_list, here we define a list and if it exists we add data, else we create it and add data later
fn add_or_init_mesh_list(mesh_lists: &mut HashMap<String, HashMap<String, Mesh>>, list_name: &String, key: String, mesh_to_add: Mesh) {
    match mesh_lists.get_mut(list_name) {
        Some(inner_mesh_list) => {
            inner_mesh_list.insert(key, mesh_to_add);
        },
        None => {
            mesh_lists.insert(list_name.to_string(), HashMap::new());
            add_or_init_mesh_list(mesh_lists, list_name, key, mesh_to_add);
        },
    }
}

/// Which of a model's meshes render with backface culling OFF (two-sided).
/// Culling is ON by default (`None`). Declared here at registration rather
/// than poked in later - it's a property of the asset. Mesh names are glTF
/// node names (the `Model::mesh_lists` inner key - e.g. "Afterburner",
/// "canopy_glass", "Foo#prim1" for a multi-primitive mesh's extra prims).
#[derive(Clone, Copy)]
pub enum DoubleSided<'a> {
    None,
    All,
    Meshes(&'a [&'a str]),
}

impl DoubleSided<'_> {
    pub(crate) fn covers(&self, mesh_name: &str) -> bool {
        match self {
            DoubleSided::None => false,
            DoubleSided::All => true,
            DoubleSided::Meshes(names) => names.contains(&mesh_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::linear_to_srgb;

    #[test]
    fn a_flat_color_is_encoded_to_srgb_once() {
        // Black, white, and linear 0.214 - what Blender shows as 0.5 grey.
        assert_eq!(linear_to_srgb(0.0), 0.0);
        assert!((linear_to_srgb(1.0) - 1.0).abs() < 1e-5);
        assert!((linear_to_srgb(0.214) - 0.5).abs() < 0.005);
    }
}
