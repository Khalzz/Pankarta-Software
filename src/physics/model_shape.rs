//! Rapier colliders shaped by a model's meshes - see `Collider::from_model`.
//!
//! `from_model` gives back Rapier's own `ColliderBuilder`, so every Rapier
//! setting chains onto it. Its shape is a stand-in (`ModelPlaceholder`, one
//! of Rapier's custom shapes) saying which file, which meshes and which kind
//! of shape; when the node spawns, the engine reads the meshes (see
//! `app.assets.collision_meshes`) and swaps the real shape in, keeping every
//! setting. The stand-in never reaches the physics world.

use nalgebra::{Point3, Vector3};
use rapier3d::parry::bounding_volume::{Aabb, BoundingSphere};
use rapier3d::parry::mass_properties::MassProperties;
use rapier3d::parry::query::{PointProjection, PointQuery, Ray, RayCast, RayIntersection};
use rapier3d::parry::shape::{FeatureId, Shape, ShapeType, TypedShape};
use rapier3d::prelude::{ColliderBuilder, SharedShape};

use crate::asset_manager::{CollisionMesh, Meshes};
use crate::scene_manager::node::properties::Transform3D;

/// Which Rapier shape a model's meshes become (see `Collider::from_model`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelShape {
    /// The shrink-wrapped outline: solid, dents filled in. For moving
    /// bodies and sensors (Rapier's `convex_hull`).
    ConvexHull,
    /// The exact surface, hollow - for static bodies only: terrain, a level
    /// (Rapier's `trimesh`).
    Trimesh,
    /// Solid convex pieces Rapier works out from the surface - a concave
    /// moving object with no pieces modeled for it (Rapier's
    /// `convex_decomposition`).
    ConvexDecomposition,
}

impl ModelShape {
    /// Rapier's shape for `mesh` - or why it can't be made.
    pub(crate) fn build(self, mesh: CollisionMesh) -> Result<SharedShape, String> {
        match self {
            ModelShape::ConvexHull => SharedShape::convex_hull(&mesh.points).ok_or_else(|| "its points are flat or too few for a convex hull".to_owned()),
            ModelShape::Trimesh => SharedShape::trimesh(mesh.points, mesh.triangles).map_err(|error| error.to_string()),
            ModelShape::ConvexDecomposition => Ok(SharedShape::convex_decomposition(&mesh.points, &mesh.triangles)),
        }
    }
}

/// The model a collider is read from.
#[derive(Debug, Clone)]
pub(crate) enum ModelRef {
    /// A model file, placed in its node by `transform` (the `Model`'s own).
    File { path: String, transform: Transform3D },
    /// Its node's own `Model`, whatever it is when the node spawns.
    Own,
    /// A `Model` with no file - nothing to read.
    NoFile,
}

/// The stand-in shape of a collider read from a model (see the module):
/// which file, which of its meshes, and which Rapier shape they become.
#[derive(Debug, Clone)]
pub struct ModelPlaceholder {
    pub(crate) model: ModelRef,
    pub(crate) meshes: Meshes,
    pub(crate) shape: ModelShape,
}

impl ModelPlaceholder {
    /// Rapier's collider with this stand-in as its shape.
    pub(crate) fn into_builder(self) -> ColliderBuilder {
        ColliderBuilder::new(SharedShape::new(self))
    }

    /// The stand-in `builder` has for a shape, if it's one read from a model.
    pub(crate) fn of(builder: &ColliderBuilder) -> Option<&ModelPlaceholder> {
        builder.shape.as_shape::<ModelPlaceholder>()
    }
}

// Rapier's custom shape: a point with no size and no mass. Never simulated -
// it's swapped for the real shape before its body is made.
impl Shape for ModelPlaceholder {
    fn compute_local_aabb(&self) -> Aabb {
        Aabb::new(Point3::origin(), Point3::origin())
    }

    fn compute_local_bounding_sphere(&self) -> BoundingSphere {
        BoundingSphere::new(Point3::origin(), 0.0)
    }

    fn clone_dyn(&self) -> Box<dyn Shape> {
        Box::new(self.clone())
    }

    fn scale_dyn(&self, _scale: &Vector3<f32>, _num_subdivisions: u32) -> Option<Box<dyn Shape>> {
        Some(Box::new(self.clone()))
    }

    fn mass_properties(&self, _density: f32) -> MassProperties {
        MassProperties::new(Point3::origin(), 0.0, Vector3::zeros())
    }

    fn shape_type(&self) -> ShapeType {
        ShapeType::Custom
    }

    fn as_typed_shape(&self) -> TypedShape<'_> {
        TypedShape::Custom(self)
    }

    fn ccd_thickness(&self) -> f32 {
        0.0
    }

    fn ccd_angular_thickness(&self) -> f32 {
        0.0
    }
}

impl PointQuery for ModelPlaceholder {
    fn project_local_point(&self, _point: &Point3<f32>, _solid: bool) -> PointProjection {
        PointProjection::new(false, Point3::origin())
    }

    fn project_local_point_and_get_feature(&self, point: &Point3<f32>) -> (PointProjection, FeatureId) {
        (self.project_local_point(point, false), FeatureId::Unknown)
    }
}

impl RayCast for ModelPlaceholder {
    fn cast_local_ray_and_get_normal(&self, _ray: &Ray, _max_time_of_impact: f32, _solid: bool) -> Option<RayIntersection> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_chain_on_the_stand_in_and_it_is_found_again() {
        let placeholder = ModelPlaceholder { model: ModelRef::Own, meshes: Meshes::prefix("col_").each(), shape: ModelShape::ConvexHull };
        let builder = placeholder.into_builder().friction(0.25).density(3.0);
        assert_eq!(builder.friction, 0.25);
        let found = ModelPlaceholder::of(&builder).expect("still the stand-in");
        assert!(found.meshes.is_each());
        assert!(ModelPlaceholder::of(&ColliderBuilder::ball(1.0)).is_none());
    }

    #[test]
    fn each_kind_builds_its_rapier_shape() {
        let cube = CollisionMesh {
            name: "cube".to_owned(),
            points: [(-1.0, -1.0, -1.0), (1.0, -1.0, -1.0), (1.0, 1.0, -1.0), (-1.0, 1.0, -1.0), (-1.0, -1.0, 1.0), (1.0, -1.0, 1.0), (1.0, 1.0, 1.0), (-1.0, 1.0, 1.0)]
                .into_iter().map(|(x, y, z)| Point3::new(x, y, z)).collect(),
            triangles: vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [3, 6, 2], [3, 7, 6], [0, 4, 7], [0, 7, 3], [1, 2, 6], [1, 6, 5]],
        };
        assert!(ModelShape::ConvexHull.build(cube.clone()).unwrap().as_convex_polyhedron().is_some());
        assert!(ModelShape::Trimesh.build(cube.clone()).unwrap().as_trimesh().is_some());
        assert!(ModelShape::ConvexDecomposition.build(cube).unwrap().as_compound().is_some());
        let flat = CollisionMesh { points: vec![Point3::origin(), Point3::new(1.0, 0.0, 0.0)], ..Default::default() };
        assert!(ModelShape::ConvexHull.build(flat).is_err());
    }
}
