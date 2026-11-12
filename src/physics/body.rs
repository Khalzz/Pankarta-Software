//! A node's physics description - its `Physics` property: the body and its
//! colliders (see scene_manager::physics_bridge).

use nalgebra::Vector3;

use crate::ui::color::UiColor;

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
    /// Draws this collider, whatever debug views are on - see
    /// `ColliderDebug`.
    pub debug: Option<ColliderDebug>,
}

impl Collider {
    /// A collider of `shape` at the body's center, touching things the
    /// default way.
    pub fn new(shape: ColliderShape) -> Self {
        Self { shape, offset: Vector3::zeros(), friction: None, restitution: None, sensor: false, detects: Vec::new(), debug: None }
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

    /// Draws this collider in the world as `debug` says - just this one,
    /// with no debug view on.
    pub fn debug(mut self, debug: ColliderDebug) -> Self {
        self.debug = Some(debug);
        self
    }
}

/// How one collider is drawn for debugging (see `Collider::debug`,
/// `Sensor::debug`): its edges and its faces, each in a color of its own or
/// not at all - over everything, so it shows through walls. A sensor with
/// something inside it uses the `occupied_` colors.
///
///     Sensor::sphere(2.0).debug(ColliderDebug::new()
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

impl ColliderShape {
    /// Half a ball of `radius`, its flat side down on the center - a dome. A
    /// `ConvexHull` around points on it.
    pub fn hemisphere(radius: f32) -> Self {
        const RINGS: usize = 6;
        const SEGMENTS: usize = 16;
        let mut points = vec![Vector3::y() * radius];
        for ring in 0..RINGS {
            // From the flat edge (0) up to just under the top.
            let (rise, out) = (ring as f32 / RINGS as f32 * std::f32::consts::FRAC_PI_2).sin_cos();
            for segment in 0..SEGMENTS {
                let (sin, cos) = (segment as f32 / SEGMENTS as f32 * std::f32::consts::TAU).sin_cos();
                points.push(Vector3::new(cos * out, rise, sin * out) * radius);
            }
        }
        ColliderShape::ConvexHull { points }
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

    /// Half a ball of `radius`, its flat side down on the node - a dome.
    pub fn hemisphere(radius: f32) -> Self {
        Self::shape(ColliderShape::hemisphere(radius))
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
