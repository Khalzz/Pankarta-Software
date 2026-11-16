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
    /// Standing along Y - smooth, exactly round to look at.
    Cylinder { radius: f32, height: f32 },
    /// Standing along Y, with `sides` flat sides - like Blender's cylinder
    /// (its Vertices).
    CylinderShape { radius: f32, height: f32, sides: u32 },
    /// Turned around Y through `rings`, bottom to top, with `sides` flat
    /// sides - see `Mesh::profile_shape`.
    ProfileShape { rings: Vec<ProfileRing>, sides: u32 },
    /// Standing along Y, `height` from tip to tip.
    Capsule { radius: f32, height: f32 },
    /// One the game added with `app.assets.add_mesh(name, ..)`.
    Custom(String),
}

/// One ring of a `Mesh::profile_shape`: across Y at `height`, `radius` from
/// its middle - which is `offset` (x, z) off the Y axis, and `stretch`
/// (x, z) times as wide each way (an oval; 1, 1 is round).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProfileRing {
    pub height: f32,
    pub radius: f32,
    pub offset: (f32, f32),
    pub stretch: (f32, f32),
}

impl ProfileRing {
    /// Its point at `angle` (radians around Y, from +X toward +Z).
    pub(crate) fn point(&self, angle: f32) -> Vector3<f32> {
        let (sin, cos) = angle.sin_cos();
        Vector3::new(self.offset.0 + cos * self.radius * self.stretch.0, self.height, self.offset.1 + sin * self.radius * self.stretch.1)
    }

    /// Its middle.
    pub(crate) fn middle(&self) -> Vector3<f32> {
        Vector3::new(self.offset.0, self.height, self.offset.1)
    }

    /// Whether it's a single point (nothing around it to draw).
    pub(crate) fn is_point(&self) -> bool {
        self.radius <= 0.0 || self.stretch.0 <= 0.0 && self.stretch.1 <= 0.0
    }
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

    /// A cylinder with `sides` flat sides (3 or more), standing along Y -
    /// like Blender's: Radius `radius`, Depth `height`, Vertices `sides`
    /// (Blender's default is `Mesh::cylinder_shape(1.0, 2.0, 32)`). Drawn
    /// with flat faces; `Collider::from_mesh` collides with exactly it.
    pub fn cylinder_shape(radius: f32, height: f32, sides: u32) -> Self {
        Self::new(MeshShape::CylinderShape { radius, height, sides: sides.max(3) })
    }

    /// A shape turned around Y with `sides` flat sides (3 or more), drawn
    /// ring by ring with `.ring(height, radius)` from bottom to top - like
    /// a Blender cylinder with loop cuts, each loop moved and scaled:
    ///
    ///     // A vase: wide base, narrow waist, flared lip.
    ///     Mesh::profile_shape(24).ring(0.0, 0.3).ring(0.4, 0.35).ring(0.8, 0.15).ring(1.1, 0.25)
    ///     // A traffic cone: a thin base plate, then a point.
    ///     Mesh::profile_shape(16).ring(0.0, 0.4).ring(0.05, 0.4).ring(0.05, 0.3).ring(0.8, 0.0)
    ///
    /// Heights are from the node (0 is where it stands). A radius of 0 is a
    /// point; two rings at one height make a flat step. The first and last
    /// rings are capped flat unless they're points. Needs 2 rings or more.
    /// `.offset(x, z)` moves a ring off the axis (bending the shape - a nose
    /// drooping to its tip), `.stretch(x, z)` makes it an oval.
    /// Drawn with flat faces; `Collider::from_mesh` collides with exactly it.
    pub fn profile_shape(sides: u32) -> Self {
        Self::new(MeshShape::ProfileShape { rings: Vec::new(), sides: sides.max(3) })
    }

    /// The next ring of a `profile_shape`, above the one before it: at
    /// `height`, `radius` from the middle (0 or more).
    pub fn ring(mut self, height: f32, radius: f32) -> Self {
        match &mut self.shape {
            MeshShape::ProfileShape { rings, .. } => rings.push(ProfileRing { height, radius: radius.max(0.0), offset: (0.0, 0.0), stretch: (1.0, 1.0) }),
            other => eprintln!("Mesh::ring is for Mesh::profile_shape - a {other:?} has no rings; ignored"),
        }
        self
    }

    /// Moves the ring added just before it `x` and `z` off the Y axis -
    /// rings moved more and more one way bend the shape that way:
    ///
    ///     // A nose drooping to its tip.
    ///     Mesh::profile_shape(24).ring(0.0, 0.6).ring(1.2, 0.35).offset(0.0, -0.1).ring(2.2, 0.0).offset(0.0, -0.3)
    pub fn offset(self, x: f32, z: f32) -> Self {
        self.last_ring("offset", |ring| ring.offset = (x, z))
    }

    /// Makes the ring added just before it an oval: `x` times as wide along
    /// X and `z` times along Z (0 or more; 1, 1 is round) - a nose flatter
    /// underneath than it is wide.
    pub fn stretch(self, x: f32, z: f32) -> Self {
        self.last_ring("stretch", |ring| ring.stretch = (x.max(0.0), z.max(0.0)))
    }

    fn last_ring(mut self, call: &str, change: impl FnOnce(&mut ProfileRing)) -> Self {
        match &mut self.shape {
            MeshShape::ProfileShape { rings, .. } => match rings.last_mut() {
                Some(ring) => change(ring),
                None => eprintln!("Mesh::{call} changes the ring added just before it - add one with .ring(height, radius) first; ignored"),
            },
            other => eprintln!("Mesh::{call} is for Mesh::profile_shape's rings - a {other:?} has none; ignored"),
        }
        self
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
            MeshShape::CylinderShape { radius, height, sides } => (ModelSource::CylinderShape { sides: *sides }, Vector3::new(radius * 2.0, *height, radius * 2.0)),
            MeshShape::Capsule { radius, height } => (ModelSource::Capsule { radius: *radius, height: *height }, one),
            MeshShape::ProfileShape { rings, sides } => (ModelSource::ProfileShape { rings: rings.clone(), sides: *sides }, one),
            MeshShape::Custom(name) => (ModelSource::Mesh(name.clone()), one),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offset_and_stretch_change_the_ring_added_just_before_them() {
        let mesh = Mesh::profile_shape(8).ring(0.0, 1.0).ring(1.0, 0.5).offset(0.2, -0.1).stretch(1.0, 0.5).ring(2.0, 0.0);
        let MeshShape::ProfileShape { rings, .. } = &mesh.shape else { panic!("a profile") };
        assert_eq!(rings.len(), 3);
        assert_eq!((rings[0].offset, rings[0].stretch), ((0.0, 0.0), (1.0, 1.0)));
        assert_eq!((rings[1].offset, rings[1].stretch), ((0.2, -0.1), (1.0, 0.5)));
        assert_eq!((rings[2].offset, rings[2].stretch), ((0.0, 0.0), (1.0, 1.0)));
        // Nothing to change yet, or not a profile: left as it was.
        assert_eq!(Mesh::profile_shape(8).offset(1.0, 1.0).shape, MeshShape::ProfileShape { rings: Vec::new(), sides: 8 });
        assert_eq!(Mesh::cube().stretch(2.0, 2.0).shape, MeshShape::Cube);
    }
}
