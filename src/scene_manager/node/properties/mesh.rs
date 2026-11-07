use nalgebra::Vector3;

use crate::asset_manager::models::ModelSource;

use super::material::Material;
use super::transform::Transform3D;

/// What a `Mesh`'s geometry is.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshShape {
    /// 1 x 1 x 1.
    Cube,
    /// 1 x 1 across X and Z, facing up - `subdivisions` extra cuts along
    /// each side, for a shader that moves its vertices (waves).
    Plane { subdivisions: u32 },
    Sphere { radius: f32 },
    /// Standing along Y.
    Cylinder { radius: f32, height: f32 },
    /// Standing along Y, `height` from tip to tip.
    Capsule { radius: f32, height: f32 },
    /// One the game added with `app.assets.add_mesh(name, ..)`.
    Custom(String),
}

/// One piece of geometry with one material - a node property on its own, or
/// a part of a `Model`:
///
///     Node::new("crate")
///         .with_property(Transform3D::default())
///         .with_property(Mesh::cube().material(Material::standard().texture("textures/crate.png")))
///
/// `transform` places it relative to its node (or its model). See
/// docs/meshes_and_models.md.
#[derive(Debug, Clone)]
pub struct Mesh {
    pub shape: MeshShape,
    pub transform: Transform3D,
    /// None: engine/standard - or, as a model's part, the model's material.
    pub material: Option<Material>,
    pub visible: bool,
    pub casts_shadows: bool,
    pub receives_shadows: bool,
}

impl Mesh {
    pub fn cube() -> Self {
        Self::new(MeshShape::Cube)
    }

    pub fn plane(subdivisions: u32) -> Self {
        Self::new(MeshShape::Plane { subdivisions })
    }

    pub fn sphere(radius: f32) -> Self {
        Self::new(MeshShape::Sphere { radius })
    }

    pub fn cylinder(radius: f32, height: f32) -> Self {
        Self::new(MeshShape::Cylinder { radius, height })
    }

    pub fn capsule(radius: f32, height: f32) -> Self {
        Self::new(MeshShape::Capsule { radius, height })
    }

    /// The mesh the game added as `name` (see `AssetManager::add_mesh`).
    pub fn custom(name: impl Into<String>) -> Self {
        Self::new(MeshShape::Custom(name.into()))
    }

    fn new(shape: MeshShape) -> Self {
        Self { shape, transform: Transform3D::default(), material: None, visible: true, casts_shadows: true, receives_shadows: true }
    }

    pub fn transform(mut self, transform: Transform3D) -> Self {
        self.transform = transform;
        self
    }

    pub fn material(mut self, material: Material) -> Self {
        self.material = Some(material);
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

    /// The loaded geometry it's drawn with, and the scale that geometry
    /// needs to be this size - a sphere is one shared unit sphere, scaled.
    pub(crate) fn geometry(&self) -> (ModelSource, Vector3<f32>) {
        let one = Vector3::new(1.0, 1.0, 1.0);
        match &self.shape {
            MeshShape::Cube => (ModelSource::Cube, one),
            MeshShape::Plane { subdivisions } => (ModelSource::Plane { subdivisions: *subdivisions }, one),
            MeshShape::Sphere { radius } => (ModelSource::Sphere, one * *radius * 2.0),
            MeshShape::Cylinder { radius, height } => (ModelSource::Cylinder, Vector3::new(radius * 2.0, *height, radius * 2.0)),
            MeshShape::Capsule { radius, height } => (ModelSource::Capsule { radius: *radius, height: *height }, one),
            MeshShape::Custom(name) => (ModelSource::Mesh(name.clone()), one),
        }
    }
}
