use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use serde::Deserialize;
use wgpu::util::DeviceExt;

use crate::app::App;
use crate::asset_manager::gltf::{load_model_with, DoubleSided};
use crate::asset_manager::normalize;
use crate::asset_manager::primitives::{build_capsule_mesh, build_cube_mesh, build_cylinder_mesh, build_mesh_model, build_plane_mesh, build_sphere_mesh};
use crate::rendering::instance_management::ModelDataInstance;
use crate::rendering::models::model::{Mesh, Model, ModelVertex};

/// Every model in memory, by key (see ModelSource::key).
#[derive(Default)]
pub struct ModelCache {
    /// Drawn by at least one node - each with its GPU instance buffer.
    pub(crate) drawn: HashMap<String, ModelDataInstance>,
    /// Loaded, drawn by nothing yet - a model moves to `drawn` with its first
    /// node (see render_bridge::register_static_model): an instance buffer
    /// can't be made empty.
    pub(crate) idle: HashMap<String, Model>,
    /// The ones the open scene loaded - dropped with it (see
    /// `unload_level_models`).
    pub(crate) scene_owned: HashSet<String>,
    /// The game's own meshes, by name (see AssetManager::add_mesh) - kept
    /// while the game runs, built into a model by each scene using one.
    pub(crate) meshes: HashMap<String, Arc<MeshData>>,
}

impl ModelCache {
    pub(crate) fn is_loaded(&self, key: &str) -> bool {
        self.drawn.contains_key(key) || self.idle.contains_key(key)
    }
}

/// Which of a model's meshes skip backface culling - `DoubleSided`, owned,
/// so it can be stored with a model's source until it's loaded (see
/// `Model::double_sided`).
#[derive(Debug, Clone, Default, Deserialize)]
pub enum DoubleSidedMeshes {
    #[default]
    None,
    All,
    Meshes(Vec<String>),
}

/// The geometry a drawn instance uses - from a `Mesh`'s shape or a
/// `Model`'s file. Each is loaded once and shared by everything using it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ModelSource {
    /// A model file in the assets folder, by its path there.
    File(String),
    /// A mesh the game added (see `AssetManager::add_mesh`).
    Mesh(String),
    Cube,
    Plane { subdivisions: u32 },
    /// Diameter 1 - sized by scale.
    Sphere,
    /// Diameter 1, height 1 - sized by scale.
    Cylinder,
    /// Its own geometry for each size: scaling one would squash its ends.
    Capsule { radius: f32, height: f32 },
}

impl ModelSource {
    /// What its loaded copy is kept under (`ModelCache`).
    pub(crate) fn key(&self) -> String {
        match self {
            Self::File(path) => normalize(path),
            Self::Mesh(name) => format!("mesh:{name}"),
            Self::Cube => "engine:cube".to_owned(),
            Self::Plane { subdivisions } => format!("engine:plane:{subdivisions}"),
            Self::Sphere => "engine:sphere".to_owned(),
            Self::Cylinder => "engine:cylinder".to_owned(),
            Self::Capsule { radius, height } => format!("engine:capsule:{radius}:{height}"),
        }
    }
}

/// A mesh the game made - see `AssetManager::add_mesh`. Kept as it was given
/// while the game runs; each scene that uses it builds its own GPU copy.
#[derive(Debug)]
pub struct MeshData {
    pub vertices: Vec<ModelVertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    /// Whether its triangles make sense - `name` is for the message.
    pub(crate) fn check(&self, name: &str) -> Result<(), String> {
        if self.indices.len() % 3 != 0 {
            return Err(format!("mesh '{name}': {} indices isn't a whole number of triangles", self.indices.len()));
        }
        if let Some(index) = self.indices.iter().find(|index| **index as usize >= self.vertices.len()) {
            return Err(format!("mesh '{name}': index {index} is past its {} vertices", self.vertices.len()));
        }
        Ok(())
    }
}

/// One model to load - the shared copy of a `ModelSource`, with what it
/// needs to be built off the main thread (a scene's loading thread). Made by
/// `AssetManager::model_load`.
#[derive(Clone)]
pub(crate) struct ModelLoad {
    pub(crate) key: String,
    geometry: LoadGeometry,
    double_sided: DoubleSidedMeshes,
}

#[derive(Clone)]
enum LoadGeometry {
    /// Where the file is on disk - its buffers and textures are next to it.
    File(std::path::PathBuf),
    Mesh(std::sync::Arc<MeshData>),
    Cube,
    Plane(u32),
    Sphere,
    Cylinder,
    Capsule(f32, f32),
}

impl ModelLoad {
    pub(crate) fn file(key: String, path: std::path::PathBuf, double_sided: DoubleSidedMeshes) -> Self {
        Self { key, geometry: LoadGeometry::File(path), double_sided }
    }

    pub(crate) fn mesh(key: String, mesh: std::sync::Arc<MeshData>, double_sided: DoubleSidedMeshes) -> Self {
        Self { key, geometry: LoadGeometry::Mesh(mesh), double_sided }
    }

    pub(crate) fn cube(key: String, double_sided: DoubleSidedMeshes) -> Self {
        Self { key, geometry: LoadGeometry::Cube, double_sided }
    }

    pub(crate) fn plane(key: String, subdivisions: u32, double_sided: DoubleSidedMeshes) -> Self {
        Self { key, geometry: LoadGeometry::Plane(subdivisions), double_sided }
    }

    pub(crate) fn shape(key: String, source: &ModelSource, double_sided: DoubleSidedMeshes) -> Self {
        let geometry = match source {
            ModelSource::Sphere => LoadGeometry::Sphere,
            ModelSource::Cylinder => LoadGeometry::Cylinder,
            ModelSource::Capsule { radius, height } => LoadGeometry::Capsule(*radius, *height),
            ModelSource::Plane { subdivisions } => LoadGeometry::Plane(*subdivisions),
            _ => LoadGeometry::Cube,
        };
        Self { key, geometry, double_sided }
    }

    /// Reads or builds the model and uploads it.
    pub(crate) fn load(&self, device: &wgpu::Device, queue: &wgpu::Queue, transform_bind_group_layout: &wgpu::BindGroupLayout) -> anyhow::Result<Model> {
        match &self.geometry {
            LoadGeometry::File(path) => {
                let names: Vec<&str> = match &self.double_sided {
                    DoubleSidedMeshes::Meshes(names) => names.iter().map(String::as_str).collect(),
                    _ => Vec::new(),
                };
                let double_sided = match &self.double_sided {
                    DoubleSidedMeshes::None => DoubleSided::None,
                    DoubleSidedMeshes::All => DoubleSided::All,
                    DoubleSidedMeshes::Meshes(_) => DoubleSided::Meshes(&names),
                };
                let file_name = path.to_string_lossy().into_owned();
                load_model_with(&file_name, &|file: &Path| Ok(std::fs::read(file)?), device, queue, transform_bind_group_layout, double_sided)
            }
            LoadGeometry::Mesh(mesh) => self.build(&mesh.vertices, &mesh.indices, device, queue, transform_bind_group_layout),
            LoadGeometry::Cube => {
                let (vertices, indices) = build_cube_mesh();
                self.build(&vertices, &indices, device, queue, transform_bind_group_layout)
            }
            LoadGeometry::Plane(subdivisions) => {
                let (vertices, indices) = build_plane_mesh(*subdivisions);
                self.build(&vertices, &indices, device, queue, transform_bind_group_layout)
            }
            LoadGeometry::Sphere => {
                let (vertices, indices) = build_sphere_mesh();
                self.build(&vertices, &indices, device, queue, transform_bind_group_layout)
            }
            LoadGeometry::Cylinder => {
                let (vertices, indices) = build_cylinder_mesh();
                self.build(&vertices, &indices, device, queue, transform_bind_group_layout)
            }
            LoadGeometry::Capsule(radius, height) => {
                let (vertices, indices) = build_capsule_mesh(*radius, *height);
                self.build(&vertices, &indices, device, queue, transform_bind_group_layout)
            }
        }
    }

    fn build(&self, vertices: &[ModelVertex], indices: &[u32], device: &wgpu::Device, queue: &wgpu::Queue, transform_bind_group_layout: &wgpu::BindGroupLayout) -> anyhow::Result<Model> {
        let double_sided = match &self.double_sided {
            DoubleSidedMeshes::None => false,
            DoubleSidedMeshes::All => true,
            DoubleSidedMeshes::Meshes(names) => names.iter().any(|name| *name == self.key),
        };
        build_mesh_model(&self.key, vertices, indices, double_sided, device, queue, transform_bind_group_layout).map_err(anyhow::Error::msg)
    }
}

/// A model's meshes' geometry, gathered while it loads - uploaded as one
/// vertex and one index buffer for the whole model (see
/// `model::Model::vertex_buffer`).
#[derive(Default)]
pub(crate) struct ModelGeometry {
    vertices: Vec<ModelVertex>,
    indices: Vec<u32>,
}

impl ModelGeometry {
    /// Adds one mesh's vertices and indices - its indices kept as they are,
    /// counting from its own first vertex. Its `(first_index, base_vertex)`
    /// (see `model::Mesh`).
    pub(crate) fn add(&mut self, vertices: &[ModelVertex], indices: &[u32]) -> (u32, i32) {
        let placed = (self.indices.len() as u32, self.vertices.len() as i32);
        self.vertices.extend_from_slice(vertices);
        self.indices.extend_from_slice(indices);
        placed
    }

    /// The model's vertex and index buffers. One with no geometry still gets
    /// a (one-element) buffer each - wgpu won't make an empty one.
    pub(crate) fn upload(mut self, device: &wgpu::Device, label: &str) -> (wgpu::Buffer, wgpu::Buffer) {
        if self.vertices.is_empty() {
            self.vertices.push(ModelVertex { position: [0.0; 3], tex_coords: [0.0; 2], normal: [0.0; 3] });
        }
        if self.indices.is_empty() {
            self.indices.push(0);
        }
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label} vertex buffer")),
            contents: bytemuck::cast_slice(&self.vertices),
            // STORAGE: read as plain numbers by the wireframe debug view.
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label} index buffer")),
            contents: bytemuck::cast_slice(&self.indices),
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::STORAGE,
        });
        (vertex_buffer, index_buffer)
    }
}

/// The model `load` describes - `variant`'s copy of it if there's one (see
/// `Model::variant`) - taken from what the scene's loading step loaded, or
/// loaded now if nothing has. Remembered as the scene's, so
/// `unload_level_models` drops it when the scene ends.
///
/// A variant is a light copy of its base model (see `Model::share`): its own
/// mesh transforms, the base's geometry and textures - the base loaded first
/// if it isn't yet, and kept for whatever uses it plainly.
pub(crate) fn load_scene_model(app: &mut App, load: &ModelLoad, variant: Option<&str>) -> Result<Model, String> {
    let base_key = load.key.clone();
    let key = variant_key(&base_key, variant);
    if let Some(existing) = app.assets.models.idle.remove(&key) {
        return Ok(existing);
    }
    if !app.assets.models.is_loaded(&base_key) {
        let bind_group_layout = Mesh::create_bind_group_layout(&app.renderer.device);
        let model = load.load(&app.renderer.device, &app.renderer.queue, &bind_group_layout)
            .map_err(|e| format!("failed to load model '{base_key}': {e}"))?;
        app.assets.models.scene_owned.insert(base_key.clone());
        if variant.is_none() {
            return Ok(model);
        }
        app.assets.models.idle.insert(base_key.clone(), model);
    }
    let base_model = app.assets.models.drawn.get(&base_key).map(|instanced| &instanced.model)
        .or_else(|| app.assets.models.idle.get(&base_key))
        .ok_or_else(|| format!("model '{base_key}' vanished while sharing it"))?;
    let model = base_model.share(&app.renderer.device, &app.renderer.queue);
    app.assets.models.scene_owned.insert(key);
    Ok(model)
}

/// The key a `variant` of the model kept under `key` is kept under.
pub(crate) fn variant_key(key: &str, variant: Option<&str>) -> String {
    match variant {
        Some(variant) => format!("{key}#{variant}"),
        None => key.to_owned(),
    }
}

/// Takes models a scene's loading thread already built (see
/// `App::start_scene_load`) as this level's own - kept ready for
/// `load_scene_model` to hand out, and dropped with the level like any
/// other. One that failed is reported; spawning it then tries again itself.
pub fn adopt_level_models(app: &mut App, models: Vec<(String, Result<Model, String>)>) {
    for (key, model) in models {
        match model {
            Ok(model) => {
                app.assets.models.idle.insert(key.clone(), model);
                app.assets.models.scene_owned.insert(key);
            }
            Err(error) => eprintln!("{error}"),
        }
    }
}

/// Drops every model a scene loaded (see `load_scene_model`) - called as the
/// scene ends, so the next one starts from only what it uses. A game's mesh
/// goes too - its data stays in `AssetManager::add_mesh`'s, for the next
/// scene using it to build again.
///
/// Models in `keep` (keys the next scene uses too) stay loaded, handed over
/// to it: a restart of the same level reloads nothing.
pub fn unload_level_models(app: &mut App, keep: &HashSet<String>) {
    let level_keys = std::mem::take(&mut app.assets.models.scene_owned);
    for (key, instanced) in std::mem::take(&mut app.assets.models.drawn) {
        if !level_keys.contains(&key) || keep.contains(&key) {
            app.assets.models.idle.insert(key, instanced.model);
        }
    }
    app.assets.models.idle.retain(|key, _| !level_keys.contains(key) || keep.contains(key));
    // Kept ones still belong to a level - the next one - and go when it ends
    // (unless the one after keeps them too).
    app.assets.models.scene_owned = level_keys.into_iter().filter(|key| keep.contains(key)).collect();
}
