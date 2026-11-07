use std::borrow::Cow;

use nalgebra::Vector3;

use crate::asset_manager::models::{DoubleSidedMeshes, ModelSource};
use crate::asset_manager::normalize;

use super::material::Material;
use super::mesh::Mesh;
use super::transform::Transform3D;

/// A 3D object made of named parts: the meshes of a file, the meshes it was
/// given, or both.
///
///     Node::new("jet").with_property(Model::file("models/f16.glb"))
///
///     Node::new("tank").with_property(Model::new()
///         .part("body", Mesh::cube())
///         .part("turret", Mesh::cylinder(0.8, 0.6).transform(Transform3D { position: Vector3::y(), ..Default::default() })))
///
/// `transform` places the whole model relative to its node; a part's own
/// `transform` places it within the model. See docs/meshes_and_models.md.
#[derive(Debug, Clone)]
pub struct Model {
    file: Option<String>,
    parts: Vec<(String, Mesh)>,
    /// The file's meshes' materials, by the mesh's name in the file.
    file_materials: Vec<(String, Material)>,
    pub transform: Transform3D,
    /// Every part without its own material uses this.
    pub material: Option<Material>,
    pub casts_shadows: bool,
    pub receives_shadows: bool,
    double_sided: DoubleSidedMeshes,
    variant: Option<String>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new()
    }
}

impl Model {
    /// A model with no parts yet - give it some with `part`.
    pub fn new() -> Self {
        Self {
            file: None,
            parts: Vec::new(),
            file_materials: Vec::new(),
            transform: Transform3D::default(),
            material: None,
            casts_shadows: true,
            receives_shadows: true,
            double_sided: DoubleSidedMeshes::None,
            variant: None,
        }
    }

    /// The model file (`.glb`/`.gltf`) at `path` in the assets folder.
    pub fn file(path: impl Into<String>) -> Self {
        Self { file: Some(normalize(&path.into())), ..Self::new() }
    }

    /// Adds `mesh` as the part `name` (replacing one with that name).
    pub fn part(mut self, name: impl Into<String>, mesh: Mesh) -> Self {
        self.add_part(name, mesh);
        self
    }

    pub fn transform(mut self, transform: Transform3D) -> Self {
        self.transform = transform;
        self
    }

    /// The material of every part that doesn't have its own.
    pub fn material(mut self, material: Material) -> Self {
        self.material = Some(material);
        self
    }

    /// The material of one part - one it was given, or a mesh of its file.
    pub fn part_material(mut self, name: &str, material: Material) -> Self {
        self.set_part_material(name, material);
        self
    }

    pub fn casts_shadows(mut self, casts: bool) -> Self {
        self.casts_shadows = casts;
        self
    }

    pub fn receives_shadows(mut self, receives: bool) -> Self {
        self.receives_shadows = receives;
        self
    }

    /// Which of the file's meshes are drawn from both sides - thin surfaces
    /// like an afterburner's flame. Every node using the same file shares
    /// its loaded copy, so give them all the same.
    pub fn double_sided(mut self, double_sided: DoubleSidedMeshes) -> Self {
        self.double_sided = double_sided;
        self
    }

    /// A separate copy of the file, `name`d - for two uses of one file whose
    /// meshes are animated differently (mesh transforms belong to the loaded
    /// copy).
    pub fn variant(mut self, name: impl Into<String>) -> Self {
        self.variant = Some(name.into());
        self
    }

    pub fn add_part(&mut self, name: impl Into<String>, mesh: Mesh) {
        let name = name.into();
        match self.parts.iter_mut().find(|(part, _)| *part == name) {
            Some((_, existing)) => *existing = mesh,
            None => self.parts.push((name, mesh)),
        }
    }

    pub fn remove_part(&mut self, name: &str) -> Option<Mesh> {
        let index = self.parts.iter().position(|(part, _)| part == name)?;
        Some(self.parts.remove(index).1)
    }

    pub fn get_part(&self, name: &str) -> Option<&Mesh> {
        self.parts.iter().find(|(part, _)| part == name).map(|(_, mesh)| mesh)
    }

    pub fn part_mut(&mut self, name: &str) -> Option<&mut Mesh> {
        self.parts.iter_mut().find(|(part, _)| part == name).map(|(_, mesh)| mesh)
    }

    /// The parts it was given, in order (not the file's meshes).
    pub fn parts(&self) -> impl Iterator<Item = (&str, &Mesh)> {
        self.parts.iter().map(|(name, mesh)| (name.as_str(), mesh))
    }

    pub fn file_path(&self) -> Option<&str> {
        self.file.as_deref()
    }

    /// Sets a part's material - one it was given, or a mesh of its file.
    pub fn set_part_material(&mut self, name: &str, material: Material) {
        if let Some(mesh) = self.part_mut(name) {
            mesh.material = Some(material);
            return;
        }
        match self.file_materials.iter_mut().find(|(mesh, _)| mesh == name) {
            Some((_, existing)) => *existing = material,
            None => self.file_materials.push((name.to_owned(), material)),
        }
    }

    /// The file's drawn copy: its key, its double-sided meshes, its variant,
    /// and its material with the per-mesh ones in.
    pub(crate) fn file_draw(&self) -> Option<(ModelSource, &DoubleSidedMeshes, Option<&str>, Option<Cow<'_, Material>>)> {
        let path = self.file.as_ref()?;
        let material = match (&self.material, self.file_materials.is_empty()) {
            (Some(material), true) => Some(Cow::Borrowed(material)),
            (None, true) => None,
            (base, false) => {
                let mut material = base.clone().unwrap_or_else(Material::standard);
                for (mesh, mesh_material) in &self.file_materials {
                    material.meshes.insert(mesh.clone(), mesh_material.clone());
                }
                Some(Cow::Owned(material))
            }
        };
        Some((ModelSource::File(path.clone()), &self.double_sided, self.variant.as_deref(), material))
    }

    /// How much bigger than its node it's drawn - for colliders read from
    /// its file.
    pub(crate) fn scale(&self) -> Vector3<f32> {
        self.transform.scale
    }
}

impl From<Mesh> for Model {
    /// A model whose one part is `mesh`, named "mesh".
    fn from(mesh: Mesh) -> Self {
        Model::new().part("mesh", mesh)
    }
}
