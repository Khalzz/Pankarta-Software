//! A node's physics description - its `Physics` property: the body and its
//! colliders (see scene_manager::physics_bridge).

use nalgebra::Vector3;

/// One collider on a body: its `shape`, where it sits on the body, and how
/// it touches things - see `Collider::new` and its builder methods.
#[derive(Debug, Clone)]
pub struct Collider {
    pub shape: ColliderShape,
    /// Where its center is, from the body's - in the body's frame.
    pub offset: Vector3<f32>,
    /// How much it grips what it slides on: 0 none, 1 a lot. None: Rapier's
    /// default (0.5).
    pub friction: Option<f32>,
    /// How much it bounces back: 0 not at all, 1 everything. None: Rapier's
    /// default (0).
    pub restitution: Option<f32>,
    /// Detects what overlaps it without stopping anything - a trigger zone.
    pub sensor: bool,
    /// For a sensor: the layers (see `Layers`) of the nodes it detects -
    /// empty for every node.
    pub detects: Vec<String>,
}

impl Collider {
    /// A collider of `shape` at the body's center, touching things the
    /// default way.
    pub fn new(shape: ColliderShape) -> Self {
        Self { shape, offset: Vector3::zeros(), friction: None, restitution: None, sensor: false, detects: Vec::new() }
    }

    /// Placed `offset` from the body's center (in the body's frame).
    pub fn offset(mut self, offset: Vector3<f32>) -> Self {
        self.offset = offset;
        self
    }

    /// Grips `friction` - it wins over what it touches when it's the lower
    /// of the two, so 0 always slides.
    pub fn friction(mut self, friction: f32) -> Self {
        self.friction = Some(friction);
        self
    }

    /// Bounces back `restitution` of its speed - it wins over what it
    /// touches when it's the higher of the two.
    pub fn restitution(mut self, restitution: f32) -> Self {
        self.restitution = Some(restitution);
        self
    }

    /// A trigger: detects overlaps, stops nothing.
    pub fn sensor(mut self) -> Self {
        self.sensor = true;
        self
    }

    /// For a sensor: detects only nodes in one of `layers` (see `Layers`).
    pub fn detects<S: Into<String>>(mut self, layers: impl IntoIterator<Item = S>) -> Self {
        self.detects = layers.into_iter().map(Into::into).collect();
        self
    }
}

impl From<ColliderShape> for Collider {
    fn from(shape: ColliderShape) -> Self {
        Self::new(shape)
    }
}

/// A collider's shape - centered on where its `Collider` is.
#[derive(Debug, Clone)]
pub enum ColliderShape {
    Cuboid { half_extents: (f32, f32, f32) },
    Ball { radius: f32 },
    // A standing capsule, centered on the body: a cylinder `half_height`
    // above and below the center, capped by half-balls of `radius` - so
    // `half_height + radius` from the center to each end. The usual shape for
    // a character: its round bottom rides over small steps and edges instead
    // of catching on them.
    Capsule { half_height: f32, radius: f32 },
    Cylinder { half_height: f32, radius: f32 },
    HeightField { heights: Vec<Vec<f32>>, scale_x: f32, scale_y: f32 },
    HalfSpace { normal: Vector3<f32> },
    // Its shape taken from a model file's own triangles (its path in the
    // assets folder) instead of a hand-authored primitive - just a
    // reference. Resolved into a real `Trimesh` (below) by
    // scene_manager::physics_bridge at spawn time, scaled by the node's own
    // `Transform3D::scale` - a body is never made from this variant, only
    // from the resolved one.
    TrimeshFromModel { model_path: String },
    // The resolved form of `TrimeshFromModel` - what a collider is
    // actually built from.
    // Meant for static bodies only (Rapier's own trimesh shape isn't
    // reliable for anything dynamic sliding/rolling across sharp internal
    // edges) - a good fit for irregular, concave static terrain (mountains,
    // ...) that no primitive shape approximates well.
    Trimesh { vertices: Vec<Vector3<f32>>, indices: Vec<[u32; 3]> },
    // The smallest convex shape around a model file's points (its path in
    // the assets folder) - resolved into a `ConvexHull` at spawn time, scaled
    // like `TrimeshFromModel`. Solid inside, unlike a trimesh: the shape for
    // a sensor read from a model, and fine on a moving body too.
    ConvexHullFromModel { model_path: String },
    // `ConvexHullFromModel` from the node's own `Model` file.
    ConvexHullFromOwnModel,
    // The smallest convex shape around `points` (in the body's frame) - a
    // shape that follows an outline more closely than a box, still cheap
    // and stable for a moving body.
    ConvexHull { points: Vec<Vector3<f32>> },
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

#[derive(Debug, Clone)]
pub struct Physics {
    pub rigidbody: RigidBodyData,
    pub colliders: Vec<Collider>,
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
///         .with_property(Sensor::sphere(1.0).detects(["player"]))
///         .with_behavior(Coin);
#[derive(Debug, Clone)]
pub struct Sensor {
    pub collider: Collider,
}

impl Sensor {
    /// A zone of `shape` - any collider shape.
    pub fn shape(shape: ColliderShape) -> Self {
        Self { collider: Collider::new(shape).sensor() }
    }

    /// A ball of `radius` around the node.
    pub fn sphere(radius: f32) -> Self {
        Self::shape(ColliderShape::Ball { radius })
    }

    /// A box reaching `half_extents` from the node each way.
    pub fn cuboid(half_extents: Vector3<f32>) -> Self {
        Self::shape(ColliderShape::Cuboid { half_extents: (half_extents.x, half_extents.y, half_extents.z) })
    }

    /// A standing capsule (see `ColliderShape::Capsule`).
    pub fn capsule(half_height: f32, radius: f32) -> Self {
        Self::shape(ColliderShape::Capsule { half_height, radius })
    }

    /// A standing cylinder.
    pub fn cylinder(half_height: f32, radius: f32) -> Self {
        Self::shape(ColliderShape::Cylinder { half_height, radius })
    }

    /// The convex shape around the model at `model_path` (in the assets
    /// folder), scaled like the node - a model's dents are filled in (a zone
    /// shaped like an L is better made of boxes on child nodes).
    pub fn from_model(model_path: impl Into<String>) -> Self {
        Self::shape(ColliderShape::ConvexHullFromModel { model_path: model_path.into() })
    }

    /// `from_model` with the node's own `Model` file.
    pub fn from_own_model() -> Self {
        Self::shape(ColliderShape::ConvexHullFromOwnModel)
    }

    /// Placed `offset` from the node (in its frame).
    pub fn offset(mut self, offset: Vector3<f32>) -> Self {
        self.collider = self.collider.offset(offset);
        self
    }

    /// Detects only nodes in one of `layers` (see `Layers`) - every node
    /// with a body by default.
    pub fn detects<S: Into<String>>(mut self, layers: impl IntoIterator<Item = S>) -> Self {
        self.collider = self.collider.detects(layers);
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
