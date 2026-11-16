//! A node's physics description - its `Physics` property: the body and its
//! colliders (see scene_manager::physics_bridge, docs/colliders.md).

use nalgebra::Vector3;
use rapier3d::prelude::ColliderBuilder;

use crate::asset_manager::Meshes;
use crate::physics::model_shape::{ModelPlaceholder, ModelRef, ModelShape};
use crate::scene_manager::node::properties::Model;
use crate::ui::color::UiColor;

/// One collider on a body: Rapier's own `ColliderBuilder` - its shape,
/// where it sits, how it touches things, anything Rapier lets a collider be
/// - plus what only the engine knows: the layers a sensor `detects` and its
/// `debug` look. `Physics::collider` and `Sensor::from` take a
/// `ColliderBuilder` as it is; this is for those two extras:
///
///     Physics::fixed().collider(Collider::from(ColliderBuilder::cuboid(1.0, 1.0, 1.0)).debug(ColliderDebug::new()))
///
/// Its node's `Layers` set the builder's collision groups when the body is
/// made - unless the builder already has groups of its own.
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
    ///     Physics::dynamic(15.0)
    ///         .collider(Collider::from_model(&table, Meshes::prefix("col_").each(), ModelShape::ConvexHull)
    ///             .friction(0.6)
    ///             .density(2.0))
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

    /// Whether it's a trigger (Rapier's `sensor`, see `Sensor`).
    pub fn is_sensor(&self) -> bool {
        self.builder.is_sensor
    }

    /// For a sensor: detects only nodes in one of `layers` (see `Layers`).
    pub fn detects<S: Into<String>>(mut self, layers: impl IntoIterator<Item = S>) -> Self {
        self.detects = layers.into_iter().map(Into::into).collect();
        self
    }

    /// Draws this collider in the world as `debug` says - just this one,
    /// with no debug view on.
    pub fn debug(mut self, debug: ColliderDebug) -> Self {
        self.debug = Some(debug);
        self
    }
}

impl From<ColliderBuilder> for Collider {
    /// Rapier's collider as it is.
    fn from(builder: ColliderBuilder) -> Self {
        Self { builder, detects: Vec::new(), debug: None }
    }
}

/// How one collider is drawn for debugging (see `Collider::debug`,
/// `Sensor::debug`): its edges and its faces, each in a color of its own or
/// not at all - over everything, so it shows through walls. A sensor with
/// something inside it uses the `occupied_` colors.
///
///     Sensor::from(ColliderBuilder::ball(2.0)).debug(ColliderDebug::new()
///         .edges(UiColor::Rgb(255, 200, 0))
///         .faces(UiColor::Rgba(255, 200, 0, 40)))
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

#[derive(Debug, Clone)]
pub struct RigidBodyData {
    pub is_static: bool,
    pub mass: f32,
    pub center_of_mass: Vector3<f32>,
    pub initial_velocity: Vector3<f32>,
    /// Never turns, whatever pushes it - a character standing upright.
    pub lock_rotations: bool,
    /// Continuous collision detection: checks the path it moves along
    /// between steps too, so moving fast it can't pass through something
    /// thin. Costs a little more each step.
    pub ccd: bool,
    /// How much of the world's gravity (`app.physics.gravity()`) it feels:
    /// 1 all of it, 2 twice, 0 none (it floats), negative pulls it up.
    /// Follows the world's gravity when that changes. Changed while it runs
    /// with `app.physics.set_gravity_scale`.
    pub gravity_scale: f32,
}

impl Default for RigidBodyData {
    /// A moving body of 1 kg, at rest, free to turn.
    fn default() -> Self {
        Self { is_static: false, mass: 1.0, center_of_mass: Vector3::zeros(), initial_velocity: Vector3::zeros(), lock_rotations: false, ccd: false, gravity_scale: 1.0 }
    }
}

impl RigidBodyData {
    /// A body that never moves - the ground, walls.
    pub fn fixed() -> Self {
        Self { is_static: true, mass: 0.0, ..Default::default() }
    }

    /// A moving body of `mass` kilograms.
    pub fn dynamic(mass: f32) -> Self {
        Self { mass, ..Default::default() }
    }
}

/// A node's body - its `Physics` property: a rigid body (`RigidBodyData`)
/// and its colliders, Rapier's own (see docs/colliders.md):
///
///     Node::new("crate")
///         .with_property(Transform3D::default())
///         .with_property(Physics::dynamic(20.0)
///             .collider(ColliderBuilder::cuboid(0.5, 0.5, 0.5).friction(0.7))
///             .collider(Collider::from_own_model(Meshes::named(["handle"]), ModelShape::ConvexHull)))
#[derive(Debug, Clone)]
pub struct Physics {
    pub(crate) rigidbody: RigidBodyData,
    pub(crate) colliders: Vec<Collider>,
}

impl Physics {
    /// A body as `rigidbody` describes it, with no colliders yet - for one
    /// with more settings than `dynamic`/`fixed` give:
    ///
    ///     Physics::new(RigidBodyData { lock_rotations: true, ..RigidBodyData::dynamic(70.0) })
    pub fn new(rigidbody: RigidBodyData) -> Self {
        Self { rigidbody, colliders: Vec::new() }
    }

    /// A moving body of `mass` kilograms.
    pub fn dynamic(mass: f32) -> Self {
        Self::new(RigidBodyData::dynamic(mass))
    }

    /// A body that never moves - the ground, walls.
    pub fn fixed() -> Self {
        Self::new(RigidBodyData::fixed())
    }

    /// One more collider: Rapier's `ColliderBuilder` (`Collider::from_model`
    /// gives one too), or a `Collider` with the engine's extras.
    pub fn collider(mut self, collider: impl Into<Collider>) -> Self {
        self.colliders.push(collider.into());
        self
    }

    /// Its rigid body's description.
    pub fn rigidbody(&self) -> &RigidBodyData {
        &self.rigidbody
    }

    /// Its colliders, in the order they were given.
    pub fn colliders(&self) -> &[Collider] {
        &self.colliders
    }
}

/// A node's trigger zone - its `Sensor` property: a shape that detects the
/// nodes with a body overlapping it, and stops nothing. Its node's behaviors
/// hear about them in `Behavior::on_sensor_enter`/`on_sensor_exit`, and
/// `app.physics.entered(..)`/`exited(..)`/`inside(..)` tell anyone else.
///
/// On a node without `Physics` it follows the node's `Transform3D` - a zone
/// that stays put, moves with its parent or is moved by hand. On a node with
/// `Physics` it's one more collider on that body - a pickup radius around a
/// player.
///
/// It detects moving bodies (and ones held still by an animation) - not
/// static ones (the ground, walls).
///
///     Node::new("coin")
///         .with_property(Transform3D { position, ..Default::default() })
///         .with_property(Sensor::from(ColliderBuilder::ball(1.0)).detects(["player"]))
///         .with_behavior(Coin);
#[derive(Debug, Clone)]
pub struct Sensor {
    pub collider: Collider,
}

impl From<ColliderBuilder> for Sensor {
    /// A zone of Rapier's collider - any shape, `Collider::from_model`'s too
    /// - made a sensor:
    ///
    ///     Sensor::from(ColliderBuilder::ball(1.5).translation(Vector3::y() * 1.5))
    ///     Sensor::from(Collider::from_own_model(Meshes::all(), ModelShape::ConvexHull))
    fn from(builder: ColliderBuilder) -> Self {
        Self { collider: Collider::from(builder.sensor(true)) }
    }
}

impl Sensor {
    /// Detects only nodes in one of `layers` (see `Layers`) - every node
    /// with a body by default.
    pub fn detects<S: Into<String>>(mut self, layers: impl IntoIterator<Item = S>) -> Self {
        self.collider = self.collider.detects(layers);
        self
    }

    /// Draws its area as `debug` says (`ColliderDebug::new()` for the
    /// default look) - with no debug view on.
    pub fn debug(mut self, debug: ColliderDebug) -> Self {
        self.collider = self.collider.debug(debug);
        self
    }
}

/// What a node with a body is, for sensors - its `Layers` property:
/// `Layers::new(["player"])`, and a `Sensor` that `detects(["player"])`
/// sees it. A node can be in several. Up to 31 different names a game.
#[derive(Debug, Clone, Default)]
pub struct Layers(pub Vec<String>);

impl Layers {
    pub fn new<S: Into<String>>(layers: impl IntoIterator<Item = S>) -> Self {
        Self(layers.into_iter().map(Into::into).collect())
    }
}
