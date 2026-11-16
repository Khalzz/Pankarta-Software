//! Rapier colliders shaped by a model's or a mesh's geometry - see
//! `Collider::from_model` and `Collider::from_mesh`.
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
use crate::scene_manager::node::properties::{Mesh, ProfileRing, Transform3D};

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

/// Where a collider's meshes come from.
#[derive(Debug, Clone)]
pub(crate) enum ModelRef {
    /// A `Model`: its file's meshes and its parts, placed in its node by
    /// `transform` (the `Model`'s own).
    Model { file: Option<String>, parts: Vec<(String, Mesh)>, transform: Transform3D },
    /// Its node's own `Model`, whatever it is when the node spawns.
    OwnModel,
    /// Its node's own `Mesh`, whatever it is when the node spawns.
    OwnMesh,
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

/// A `Mesh::profile_shape`'s rings, placed as its collider's points are,
/// for drawing that collider: its rings and the lines along its length, as
/// written - but traced onto the collider's real surface (see `trace`). A
/// convex hull fills dents and merges near-flat bands, so its own edges
/// don't show the rings; these do, wherever the collider really is.
#[derive(Debug, Clone, Default)]
pub(crate) struct ProfileOutline {
    /// Each profile's rings (several, when merged into one collider).
    profiles: Vec<Vec<OutlineRing>>,
}

#[derive(Debug, Clone)]
struct OutlineRing {
    middle: Point3<f32>,
    corners: Vec<Point3<f32>>,
    /// A radius of 0: its corners are all its middle.
    point: bool,
}

impl ProfileOutline {
    /// One profile, `rings` around with `sides` corners each, every point
    /// placed by `place`.
    pub(crate) fn new(rings: &[ProfileRing], sides: u32, place: impl Fn(Vector3<f32>) -> Point3<f32>) -> Self {
        let sides = sides.max(3);
        let rings = rings.iter().map(|ring| OutlineRing {
            middle: place(ring.middle()),
            corners: (0..sides).map(|side| place(ring.point(side as f32 / sides as f32 * std::f32::consts::TAU))).collect(),
            point: ring.is_point(),
        });
        Self { profiles: vec![rings.collect()] }
    }

    /// Its profiles and `other`'s, for one collider made of both.
    pub(crate) fn merge(mut self, other: ProfileOutline) -> Self {
        self.profiles.extend(other.profiles);
        self
    }

    /// The lines on `shape`: each ring's corners joined around and to the
    /// next ring's. A corner is moved to where `shape`'s surface really is
    /// going straight out from its ring's middle - found by a ray coming in
    /// from outside, so it lands on the outer surface: out to a hull where
    /// it fills a dent, exactly on the corner where the collider is the
    /// mesh. Where a ray finds nothing, the corner stays as written.
    pub(crate) fn trace(&self, shape: &dyn Shape) -> Vec<[Point3<f32>; 2]> {
        let bounds = shape.compute_local_bounding_sphere();
        let traced = |ring: &OutlineRing| -> Vec<Point3<f32>> {
            ring.corners.iter().map(|corner| {
                let Some(out) = (corner - ring.middle).try_normalize(1e-9).filter(|_| !ring.point) else { return *corner };
                let far = 2.0 * (bounds.radius() + (ring.middle - bounds.center()).norm()) + 1.0;
                let ray = Ray::new(ring.middle + out * far, -out);
                shape.cast_local_ray(&ray, far, true).map_or(*corner, |time| ray.point_at(time))
            }).collect()
        };
        let mut lines = Vec::new();
        for profile in &self.profiles {
            let rings: Vec<(Vec<Point3<f32>>, bool)> = profile.iter().map(|ring| (traced(ring), ring.point)).collect();
            for (index, (corners, point)) in rings.iter().enumerate() {
                if !point {
                    for (here, next) in corners.iter().zip(corners.iter().cycle().skip(1)) {
                        lines.push([*here, *next]);
                    }
                }
                if let Some((above, above_point)) = rings.get(index + 1) {
                    if !(*point && *above_point) {
                        lines.extend(corners.iter().zip(above).map(|(low, high)| [*low, *high]));
                    }
                }
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_chain_on_the_stand_in_and_it_is_found_again() {
        let placeholder = ModelPlaceholder { model: ModelRef::OwnModel, meshes: Meshes::prefix("col_").each(), shape: ModelShape::ConvexHull };
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

    fn rings(rings: &[(f32, f32)]) -> Vec<ProfileRing> {
        rings.iter().map(|&(height, radius)| ProfileRing { height, radius, offset: (0.0, 0.0), stretch: (1.0, 1.0) }).collect()
    }

    /// A profile's points (corners only), as its collider is made from them.
    fn points(rings: &[ProfileRing], sides: u32) -> Vec<Point3<f32>> {
        rings.iter().flat_map(|ring| (0..sides).map(move |side| Point3::from(ring.point(side as f32 / sides as f32 * std::f32::consts::TAU)))).collect()
    }

    #[test]
    fn an_outline_keeps_every_ring_where_the_hull_is_the_mesh() {
        // Nearly straight: a hull merges these bands into one face, but the
        // outline still has all 4 rings, each exactly where it was written.
        let profile = rings(&[(0.0, 0.75), (1.0, 0.80), (2.0, 0.83), (3.0, 0.82)]);
        let hull = SharedShape::convex_hull(&points(&profile, 12)).unwrap();
        let lines = ProfileOutline::new(&profile, 12, Point3::from).trace(hull.as_ref());
        // 4 rings of 12, and 3 bands of 12 lines along.
        assert_eq!(lines.len(), 4 * 12 + 3 * 12);
        let corner = |height: f32, radius: f32| Point3::new(radius, height, 0.0);
        for (height, radius) in [(0.0, 0.75), (1.0, 0.80), (2.0, 0.83), (3.0, 0.82)] {
            assert!(lines.iter().any(|[a, _]| (a - corner(height, radius)).norm() < 1e-3), "ring at {height} not on its corner");
        }
    }

    #[test]
    fn an_outline_shows_a_dent_filled_where_the_hull_fills_it() {
        // A waist the hull fills: its ring is drawn out on the hull, half
        // way between the rings around it - not in at 0.2.
        let profile = rings(&[(0.0, 1.0), (1.0, 0.2), (2.0, 1.0)]);
        let hull = SharedShape::convex_hull(&points(&profile, 8)).unwrap();
        let lines = ProfileOutline::new(&profile, 8, Point3::from).trace(hull.as_ref());
        let waist: Vec<_> = lines.iter().flatten().filter(|point| (point.y - 1.0).abs() < 1e-4).collect();
        assert!(!waist.is_empty());
        assert!(waist.iter().all(|point| point.coords.xz().norm() > 0.9));
        // An exact collider keeps it in.
        let (vertices, indices) = crate::asset_manager::primitives::build_profile_shape_mesh(&profile, 8);
        let trimesh = SharedShape::trimesh(
            vertices.iter().map(|vertex| Point3::from(vertex.position)).collect(),
            indices.chunks_exact(3).map(|triangle| [triangle[0], triangle[1], triangle[2]]).collect(),
        ).unwrap();
        let lines = ProfileOutline::new(&profile, 8, Point3::from).trace(trimesh.as_ref());
        let waist: Vec<_> = lines.iter().flatten().filter(|point| (point.y - 1.0).abs() < 1e-4).collect();
        assert!(waist.iter().all(|point| (point.coords.xz().norm() - 0.2).abs() < 1e-3));
        // A pointed end draws no ring - only the lines up to it.
        let cone = rings(&[(0.0, 1.0), (2.0, 0.0)]);
        let hull = SharedShape::convex_hull(&points(&cone, 8)).unwrap();
        assert_eq!(ProfileOutline::new(&cone, 8, Point3::from).trace(hull.as_ref()).len(), 8 + 8);
    }
}
