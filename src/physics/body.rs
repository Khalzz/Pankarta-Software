//! A node's physics: Rapier's `RigidBodyBuilder` as a property, and its
//! `Colliders` - Rapier's `ColliderBuilder`s, with the engine's extras (see
//! scene_manager::physics_bridge, docs/colliders.md).

use std::f32::consts::{FRAC_PI_2, TAU};

use nalgebra::Point3;
use rapier3d::prelude::{ColliderBuilder, RigidBodyBuilder, RigidBodyType};

use crate::asset_manager::Meshes;
use crate::physics::model_shape::{ModelPlaceholder, ModelRef, ModelShape};
use crate::scene_manager::node::properties::Model;
use crate::ui::color::UiColor;

/// One collider of a node's `Colliders`: Rapier's own `ColliderBuilder` -
/// its shape, where it sits, how it touches things - and what only the
/// engine knows: the layers a sensor `detects` and its `debug` look (set
/// with `Colliders::detects`/`debug`).
///
/// Its `Collider::..` functions make shapes Rapier has no constructor for -
/// from a model's meshes, a dome, a many-sided cylinder - as Rapier's own
/// `ColliderBuilder`, for `Colliders::add`.
#[derive(Debug, Clone)]
pub struct Collider {
    /// Rapier's collider, as it'll be built.
    pub builder: ColliderBuilder,
    /// For a sensor: the layers (see `Layers`) of the nodes it detects -
    /// empty for every node.
    pub detects: Vec<String>,
    /// Draws this collider, whatever debug views are on - see
    /// `ColliderDebug`.
    pub debug: Option<ColliderDebug>,
}

impl Collider {
    /// Rapier's collider shaped by `model`'s meshes - the ones `meshes`
    /// picks - made into `shape`. Every Rapier setting chains onto it:
    ///
    ///     let table = Model::file("models/table.glb").hide_meshes(Meshes::prefix("col_"));
    ///     Colliders::new()
    ///         .add(Collider::from_model(&table, Meshes::prefix("col_").each(), ModelShape::ConvexHull)
    ///             .friction(0.6)
    ///             .mass(3.0))
    ///
    /// The file is read once, with its scene's other assets (see
    /// `app.assets.collision_meshes`). When the node spawns, the meshes -
    /// placed and scaled as the node draws them: the node's scale, the
    /// model's `transform` - become the shape, merged into one, or one
    /// collider apiece with `Meshes::each`; the settings stay. One that
    /// can't be read or made is left out, saying why.
    pub fn from_model(model: &Model, meshes: Meshes, shape: ModelShape) -> ColliderBuilder {
        let model = match model.file_path() {
            Some(path) => ModelRef::File { path: path.to_owned(), transform: model.transform },
            None => ModelRef::NoFile,
        };
        ModelPlaceholder { model, meshes, shape }.into_builder()
    }

    /// `from_model` with its node's own `Model` file, whatever it is when
    /// the node spawns.
    pub fn from_own_model(meshes: Meshes, shape: ModelShape) -> ColliderBuilder {
        ModelPlaceholder { model: ModelRef::Own, meshes, shape }.into_builder()
    }

    /// Half a ball of `radius`, its flat side down on the center - a dome.
    /// A convex hull around points on it (6 rings of 16, and the top), so
    /// a little faceted. On the ground, a whole `ColliderBuilder::ball` -
    /// its lower half under the floor - works the same, and exactly.
    pub fn hemisphere(radius: f32) -> ColliderBuilder {
        const RINGS: usize = 6;
        const SEGMENTS: usize = 16;
        let mut points = vec![Point3::new(0.0, radius, 0.0)];
        for ring in 0..RINGS {
            // From the flat edge (0) up to just under the top.
            let (rise, out) = (ring as f32 / RINGS as f32 * FRAC_PI_2).sin_cos();
            for segment in 0..SEGMENTS {
                let (sin, cos) = (segment as f32 / SEGMENTS as f32 * TAU).sin_cos();
                points.push(Point3::new(cos * out, rise, sin * out) * radius);
            }
        }
        hull(&points)
    }

    /// A standing cylinder with `sides` flat sides (3 or more) instead of a
    /// perfectly round one - a prism, like Blender's cylinder (32 sides by
    /// default there, Radius 1, Depth 2: `cylinder_with_sides(1.0, 1.0, 32)`).
    /// Rapier's own `ColliderBuilder::cylinder` is exactly round.
    pub fn cylinder_with_sides(half_height: f32, radius: f32, sides: usize) -> ColliderBuilder {
        let sides = sides.max(3);
        let mut points = Vec::with_capacity(sides * 2);
        for side in 0..sides {
            let (sin, cos) = (side as f32 / sides as f32 * TAU).sin_cos();
            for y in [-half_height, half_height] {
                points.push(Point3::new(cos * radius, y, sin * radius));
            }
        }
        hull(&points)
    }

    /// Whether it's a trigger (Rapier's `sensor`).
    pub fn is_sensor(&self) -> bool {
        self.builder.is_sensor
    }
}

/// The convex hull around `points` - an empty ball for a size of 0, whose
/// points have no hull.
fn hull(points: &[Point3<f32>]) -> ColliderBuilder {
    ColliderBuilder::convex_hull(points).unwrap_or_else(|| ColliderBuilder::ball(0.0))
}

impl From<ColliderBuilder> for Collider {
    /// Rapier's collider as it is, with none of the engine's extras.
    fn from(builder: ColliderBuilder) -> Self {
        Self { builder, detects: Vec::new(), debug: None }
    }
}

/// How one collider is drawn for debugging (see `Colliders::debug`): its
/// edges and its faces, each in a color of its own or not at all - over
/// everything, so it shows through walls. A sensor with something inside it
/// uses the `occupied_` colors.
///
///     Colliders::new()
///         .add(ColliderBuilder::ball(2.0).sensor(true))
///             .debug(ColliderDebug::new()
///                 .edges(UiColor::Rgb(255, 200, 0))
///                 .faces(UiColor::Rgba(255, 200, 0, 40)))
#[derive(Debug, Clone, Copy)]
pub struct ColliderDebug {
    pub edges: Option<UiColor>,
    pub faces: Option<UiColor>,
    pub occupied_edges: Option<UiColor>,
    pub occupied_faces: Option<UiColor>,
}

impl Default for ColliderDebug {
    /// Cyan edges and see-through cyan faces - green ones while a sensor has
    /// something inside.
    fn default() -> Self {
        Self {
            edges: Some(UiColor::Rgb(50, 205, 255)),
            faces: Some(UiColor::Rgba(50, 205, 255, 40)),
            occupied_edges: Some(UiColor::Rgb(50, 255, 80)),
            occupied_faces: Some(UiColor::Rgba(50, 255, 80, 50)),
        }
    }
}

impl ColliderDebug {
    /// The default look - see `Default`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Its edges in `color` - the alpha is ignored. Also while occupied,
    /// unless `occupied_edges` says otherwise.
    pub fn edges(mut self, color: UiColor) -> Self {
        self.edges = Some(color);
        self.occupied_edges = Some(color);
        self
    }

    /// Its faces in `color` - give it some transparency to see through it.
    /// Also while occupied, unless `occupied_faces` says otherwise.
    pub fn faces(mut self, color: UiColor) -> Self {
        self.faces = Some(color);
        self.occupied_faces = Some(color);
        self
    }

    /// A sensor's edges while something's inside it.
    pub fn occupied_edges(mut self, color: UiColor) -> Self {
        self.occupied_edges = Some(color);
        self
    }

    /// A sensor's faces while something's inside it.
    pub fn occupied_faces(mut self, color: UiColor) -> Self {
        self.occupied_faces = Some(color);
        self
    }

    /// No edges.
    pub fn no_edges(mut self) -> Self {
        self.edges = None;
        self.occupied_edges = None;
        self
    }

    /// No faces.
    pub fn no_faces(mut self) -> Self {
        self.faces = None;
        self.occupied_faces = None;
        self
    }

    /// The edge and face colors for `occupied` or not.
    pub fn colors(&self, occupied: bool) -> (Option<UiColor>, Option<UiColor>) {
        if occupied { (self.occupied_edges, self.occupied_faces) } else { (self.edges, self.faces) }
    }
}

/// A node's colliders - its `Colliders` property: Rapier's
/// `ColliderBuilder`s, added one by one, each with the engine's extras
/// configured right after it. Its body is the node's `RigidBodyBuilder`
/// property (see docs/colliders.md):
///
///     Node::new("player")
///         .with_property(Transform3D::default())
///         .with_property(RigidBodyBuilder::dynamic().lock_rotations())
///         .with_property(Colliders::new()
///             .add(ColliderBuilder::capsule_y(0.5, 0.3).mass(80.0))
///             .add(ColliderBuilder::ball(2.0).sensor(true))
///                 .detects(["coin"]))
///
/// A node with colliders and no `RigidBodyBuilder` gets one of its own: a
/// kinematic one following the node when all its colliders are sensors (a
/// trigger zone), a fixed one otherwise (a wall, the ground).
#[derive(Debug, Clone, Default)]
pub struct Colliders {
    entries: Vec<Collider>,
}

impl Colliders {
    /// No colliders yet - add them with `add`.
    pub fn new() -> Self {
        Self::default()
    }

    /// One more collider: any Rapier `ColliderBuilder` - the `Collider::..`
    /// shapes give one too. `detects` and `debug` right after it configure
    /// this one.
    pub fn add(mut self, collider: ColliderBuilder) -> Self {
        self.entries.push(Collider::from(collider));
        self
    }

    /// The collider just added, if it's a sensor, detects only nodes in one
    /// of `layers` (see `Layers`) - every node with a body by default.
    pub fn detects<S: Into<String>>(mut self, layers: impl IntoIterator<Item = S>) -> Self {
        match self.entries.last_mut() {
            Some(collider) => collider.detects = layers.into_iter().map(Into::into).collect(),
            None => eprintln!("Colliders::detects before any collider was added - `add` one first"),
        }
        self
    }

    /// The collider just added is drawn as `debug` says
    /// (`ColliderDebug::new()` for the default look) - with no debug view on.
    pub fn debug(mut self, debug: ColliderDebug) -> Self {
        match self.entries.last_mut() {
            Some(collider) => collider.debug = Some(debug),
            None => eprintln!("Colliders::debug before any collider was added - `add` one first"),
        }
        self
    }

    /// Every collider, in the order they were added.
    pub fn iter(&self) -> impl Iterator<Item = &Collider> {
        self.entries.iter()
    }

    /// Every collider's Rapier builder, in the order they were added - for
    /// `compute_principal_inertia`.
    pub fn builders(&self) -> impl Iterator<Item = &ColliderBuilder> {
        self.entries.iter().map(|collider| &collider.builder)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn into_entries(self) -> Vec<Collider> {
        self.entries
    }
}

/// The body a node with `colliders` and no `RigidBodyBuilder` of its own
/// gets: kinematic, following the node, when every one is a sensor (a
/// trigger zone); fixed otherwise.
pub(crate) fn implied_body(colliders: &[Collider]) -> RigidBodyBuilder {
    if !colliders.is_empty() && colliders.iter().all(Collider::is_sensor) {
        RigidBodyBuilder::kinematic_position_based()
    } else {
        RigidBodyBuilder::fixed()
    }
}

/// Whether a body of `body`'s kind follows its node's `Transform3D` - a
/// kinematic position-based one.
pub(crate) fn follows_node(body: &RigidBodyBuilder) -> bool {
    body.body_type == RigidBodyType::KinematicPositionBased
}

/// What a node with a body is, for sensors - its `Layers` property:
/// `Layers::new(["player"])`, and a sensor collider that
/// `detects(["player"])` sees it. A node can be in several. Up to 31 different names a game.
#[derive(Debug, Clone, Default)]
pub struct Layers(pub Vec<String>);

impl Layers {
    pub fn new<S: Into<String>>(layers: impl IntoIterator<Item = S>) -> Self {
        Self(layers.into_iter().map(Into::into).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_shapes_are_their_size() {
        let dome = Collider::hemisphere(2.0).shape.compute_local_aabb();
        assert!((dome.maxs.y - 2.0).abs() < 1e-4 && dome.mins.y.abs() < 1e-4);
        let prism = Collider::cylinder_with_sides(1.0, 0.5, 6).shape;
        assert_eq!(prism.as_convex_polyhedron().expect("a convex hull").points().len(), 12);
    }

    #[test]
    fn kinematic_bodies_follow_their_node() {
        assert!(follows_node(&RigidBodyBuilder::kinematic_position_based()));
        assert!(!follows_node(&RigidBodyBuilder::dynamic()));
        assert!(!follows_node(&RigidBodyBuilder::kinematic_velocity_based()));
    }

    #[test]
    fn extras_configure_the_collider_just_added() {
        let colliders = Colliders::new()
            .add(ColliderBuilder::capsule_y(0.5, 0.3))
            .add(ColliderBuilder::ball(2.0).sensor(true))
                .detects(["coin"])
                .debug(ColliderDebug::new());
        let entries: Vec<&Collider> = colliders.iter().collect();
        assert!(entries[0].detects.is_empty() && entries[0].debug.is_none());
        assert_eq!(entries[1].detects, vec!["coin".to_owned()]);
        assert!(entries[1].debug.is_some());
        // Before any collider: nothing to configure, nothing breaks.
        assert!(Colliders::new().detects(["x"]).is_empty());
    }

    #[test]
    fn a_node_without_a_body_gets_one_from_its_colliders() {
        let zone = Colliders::new().add(ColliderBuilder::ball(1.0).sensor(true)).into_entries();
        assert!(follows_node(&implied_body(&zone)));
        let wall = Colliders::new().add(ColliderBuilder::cuboid(1.0, 1.0, 1.0)).add(ColliderBuilder::ball(1.0).sensor(true)).into_entries();
        assert_eq!(implied_body(&wall).body_type, RigidBodyType::Fixed);
    }
}
