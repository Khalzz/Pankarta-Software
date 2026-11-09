use std::borrow::Cow;

use nalgebra::{UnitQuaternion, Vector3};

use crate::asset_manager::models::{self, DoubleSidedMeshes, ModelSource};
use crate::asset_manager::normalize;
use crate::rendering::models::model as loaded;

use super::material::Material;
use super::mesh::Mesh;
use super::transform::Transform3D;

/// One mesh of a model's file - an object as the file names it (all of its
/// materials, when it has several). Where it sits and whether it's drawn
/// belong to its node alone: changing them changes nothing on other nodes
/// using the same file. See `Model::mesh_mut`.
#[derive(Debug, Clone)]
pub struct ModelMesh {
    name: String,
    original: Transform3D,
    bounds: (Vector3<f32>, Vector3<f32>),
    /// Where it sits in the model (before the model's own `transform`) -
    /// where the file put it, to begin with. It turns and scales about its
    /// own origin.
    pub transform: Transform3D,
    /// `false` hides it, and its shadow.
    pub visible: bool,
}

impl ModelMesh {
    /// Its name in the file.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the file put it.
    pub fn original_transform(&self) -> Transform3D {
        self.original
    }

    /// Its geometry's box - lowest and highest corner - in its own space,
    /// before `transform`.
    pub fn local_bounds(&self) -> (Vector3<f32>, Vector3<f32>) {
        self.bounds
    }

    /// Turns it `rotation` (about its own axes) away from how the file
    /// turned it - a control surface's deflection, a wheel's spin.
    pub fn rotate_from_original(&mut self, rotation: UnitQuaternion<f32>) {
        self.transform.rotation = self.original.rotation * rotation;
    }

    /// Back where the file put it, and shown.
    pub fn reset(&mut self) {
        self.transform = self.original;
        self.visible = true;
    }

    /// Moved, turned, scaled or hidden since the file put it there.
    fn changed(&self) -> bool {
        !self.visible
            || self.transform.position != self.original.position
            || self.transform.rotation != self.original.rotation
            || self.transform.scale != self.original.scale
    }
}

/// The variant a node's own copy of its file is kept as (see
/// `Model::own_copy`).
fn own_variant(node: &str) -> String {
    format!("node:{node}")
}

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
    /// The file's meshes, by name - read once the file is loaded (see
    /// `mesh_mut`).
    meshes: Vec<ModelMesh>,
    meshes_read: bool,
    /// Drawn with a copy of the file of its own - from when one of `meshes`
    /// was first changed. Until then it shares the one loaded copy (and its
    /// instance buffer) with every other node using the file.
    own_copy: bool,
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
            meshes: Vec::new(),
            meshes_read: false,
            own_copy: false,
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

    /// A separate copy of the file, `name`d, shared by the nodes giving the
    /// same name. (A node changing its meshes - see `mesh_mut` - gets a copy
    /// of its own anyway.)
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

    /// One of its file's meshes, by its name in the file - to move, turn,
    /// scale or hide on this node alone:
    ///
    ///     let model = node.get_property_mut::<Model>().unwrap();
    ///     model.mesh_mut("left_aleron").unwrap().rotate_from_original(deflection);
    ///     model.mesh_mut("Afterburner").unwrap().visible = false;
    ///
    /// None until the file is loaded - from when its node is spawned (for a
    /// node spawned in `on_spawn`, once the scene has loaded).
    pub fn mesh_mut(&mut self, name: &str) -> Option<&mut ModelMesh> {
        let index = self.meshes.binary_search_by(|mesh| mesh.name.as_str().cmp(name)).ok()?;
        Some(&mut self.meshes[index])
    }

    pub fn mesh(&self, name: &str) -> Option<&ModelMesh> {
        let index = self.meshes.binary_search_by(|mesh| mesh.name.as_str().cmp(name)).ok()?;
        Some(&self.meshes[index])
    }

    /// All of its file's meshes, by name - see `mesh_mut`. Empty until the
    /// file is loaded.
    pub fn meshes_mut(&mut self) -> impl Iterator<Item = &mut ModelMesh> {
        self.meshes.iter_mut()
    }

    pub fn meshes(&self) -> impl Iterator<Item = &ModelMesh> {
        self.meshes.iter()
    }

    /// The key its file's shared copy is kept under.
    pub(crate) fn file_key(&self) -> Option<String> {
        self.file.as_ref().map(|path| ModelSource::File(path.clone()).key())
    }

    /// Its file's meshes not read yet.
    pub(crate) fn needs_meshes(&self) -> bool {
        self.file.is_some() && !self.meshes_read
    }

    /// Reads its file's meshes from `model`, its loaded copy.
    pub(crate) fn read_meshes(&mut self, model: &loaded::Model) {
        let mut meshes: Vec<ModelMesh> = Vec::new();
        for (key, mesh) in model.mesh_lists.values().flatten() {
            let name = loaded::object_name(key);
            let (low, high) = (Vector3::from(mesh.local_bounds.0), Vector3::from(mesh.local_bounds.1));
            match meshes.iter_mut().find(|known| known.name == name) {
                Some(known) => known.bounds = (known.bounds.0.inf(&low), known.bounds.1.sup(&high)),
                None => meshes.push(ModelMesh {
                    name: name.to_owned(),
                    original: mesh.base_transform,
                    bounds: (low, high),
                    transform: mesh.base_transform,
                    visible: true,
                }),
            }
        }
        meshes.sort_by(|a, b| a.name.cmp(&b.name));
        self.meshes = meshes;
        self.meshes_read = true;
    }

    /// Takes a copy of the file of its own once one of its meshes changed.
    /// Kept from then on, so it isn't put back on and off the shared one.
    pub(crate) fn check_own_copy(&mut self) {
        if !self.own_copy && self.meshes.iter().any(ModelMesh::changed) {
            self.own_copy = true;
        }
    }

    /// The key its own copy of the file is kept under, on node `node` -
    /// None while it shares the loaded one.
    pub(crate) fn own_copy_key(&self, node: &str) -> Option<String> {
        if !self.own_copy {
            return None;
        }
        Some(models::variant_key(&self.file_key()?, Some(&own_variant(node))))
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

    /// The file's drawn copy, on node `node`: its key, its double-sided
    /// meshes, its variant (the node's own, once it has its own copy), and
    /// its material with the per-mesh ones in.
    pub(crate) fn file_draw(&self, node: &str) -> Option<(ModelSource, &DoubleSidedMeshes, Option<Cow<'_, str>>, Option<Cow<'_, Material>>)> {
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
        let variant = match self.own_copy {
            true => Some(Cow::Owned(own_variant(node))),
            false => self.variant.as_deref().map(Cow::Borrowed),
        };
        Some((ModelSource::File(path.clone()), &self.double_sided, variant, material))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `Model::file(path)` as if its file had loaded with meshes `names`.
    fn loaded_file(path: &str, names: &[&str]) -> Model {
        let mut model = Model::file(path);
        model.meshes = names.iter().map(|name| ModelMesh {
            name: (*name).to_owned(),
            original: Transform3D::default(),
            bounds: (Vector3::zeros(), Vector3::zeros()),
            transform: Transform3D::default(),
            visible: true,
        }).collect();
        model.meshes.sort_by(|a, b| a.name.cmp(&b.name));
        model.meshes_read = true;
        model
    }

    #[test]
    fn a_model_shares_its_file_until_one_of_its_meshes_changes() {
        let mut model = loaded_file("models/f16.glb", &["rudder", "left_aleron", "Afterburner"]);
        model.check_own_copy();
        assert_eq!(model.own_copy_key("jet"), None);
        assert!(model.file_draw("jet").unwrap().2.is_none());

        model.mesh_mut("left_aleron").unwrap().rotate_from_original(UnitQuaternion::from_axis_angle(&Vector3::x_axis(), 0.3));
        model.check_own_copy();
        assert_eq!(model.own_copy_key("jet").as_deref(), Some("models/f16.glb#node:jet"));
        assert_eq!(model.file_draw("jet").unwrap().2.as_deref(), Some("node:jet"));

        // Kept, even once it's back where it was.
        model.mesh_mut("left_aleron").unwrap().reset();
        model.check_own_copy();
        assert!(model.own_copy_key("jet").is_some());
    }

    #[test]
    fn hiding_a_mesh_is_a_change_too() {
        let mut model = loaded_file("models/f16.glb", &["Afterburner"]);
        model.mesh_mut("Afterburner").unwrap().visible = false;
        model.check_own_copy();
        assert!(model.own_copy_key("jet").is_some());
        assert!(model.mesh("nope").is_none());
    }
}
