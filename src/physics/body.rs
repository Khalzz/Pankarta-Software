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
}

impl Collider {
    /// A collider of `shape` at the body's center, touching things the
    /// default way.
    pub fn new(shape: ColliderShape) -> Self {
        Self { shape, offset: Vector3::zeros(), friction: None, restitution: None, sensor: false }
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

