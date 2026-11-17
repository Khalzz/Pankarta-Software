//! The open scene's physics - `app.physics`. It lives on the main thread
//! with everything else, and steps at a fixed rate from `App::run`: each
//! frame runs as many steps as the time that passed calls for (none, one,
//! several), each one right after every node's `Behavior::fixed_update`. The
//! solver inside a step spreads its work over the CPU's cores (Rapier's
//! `parallel` feature).
//!
//! Behaviors reach it directly - `app.physics.body_mut(&node.id)` to push a
//! body, `app.physics.cast_ray(..)` to ask what's where - from any hook. A
//! body's forces and velocities belong in `fixed_update` (they then act the
//! same at any frame rate); reading where things are, or asking the world,
//! works from `update` too.
//!
//! Nodes are drawn where their body is right now: between the last two
//! steps, by how far the frame is toward the next one (see `render_pose`) -
//! so a body moves smoothly at any frame rate, without ever being drawn
//! somewhere it hasn't really been.
//!
//! Sensors (colliders with Rapier's `sensor`) are checked after every step: what started or
//! stopped overlapping each one is `sensor_events`, and is handed to the
//! sensor node's behaviors (`Behavior::on_sensor_enter`/`on_sensor_exit`).

use std::collections::{HashMap, HashSet};

use nalgebra::{Isometry3, Point3, UnitQuaternion, Vector3};
use rapier3d::prelude::{
    CCDSolver, Collider, ColliderHandle, ColliderSet, DefaultBroadPhase, ImpulseJointSet, IntegrationParameters, IslandManager, MultibodyJointSet, NarrowPhase,
    PhysicsPipeline, QueryFilter, QueryPipeline, Ray, RigidBody, RigidBodyHandle, RigidBodySet, RigidBodyType, Shape,
};

use crate::physics::body::{ColliderDebug, Outline};
use crate::physics::physics_resources::{load_physics_from_definitions, LayerBits, PhysicsObjectDef};
use crate::rendering::lines::{Faces, Lines};

/// Which physics gizmos are drawn - `app.renderer.debug.physics`, all off
/// by default:
///
///     app.renderer.debug.physics.colliders = true;       // one gizmo
///     app.renderer.debug.physics = PhysicsGizmos::all(); // every one
///     app.renderer.debug.physics.toggle_all();           // all on, or all off if any is on
///
/// A collider with its own `ColliderDebug` is drawn whatever's on here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhysicsGizmos {
    /// Every collider that isn't a sensor: moving bodies' in orange, fixed
    /// ones' (the ground, the level) in a dim blue-gray.
    pub colliders: bool,
    /// Every sensor's area - cyan, green while something overlaps it.
    pub sensors: bool,
    /// Every dynamic body's center of mass, its axes along the body's
    /// principal axes of inertia (gray while it's asleep), and its velocity
    /// in yellow - plus what the game draws into `PhysicsWorld::debug_lines`.
    pub rigidbodies: bool,
}

impl PhysicsGizmos {
    /// Every gizmo on.
    pub fn all() -> Self {
        Self { colliders: true, sensors: true, rigidbodies: true }
    }

    /// Whether any gizmo is on.
    pub fn any(&self) -> bool {
        self.colliders || self.sensors || self.rigidbodies
    }

    /// Every gizmo off if any is on, every one on if none is - for a key
    /// that switches the physics debug view.
    pub fn toggle_all(&mut self) {
        *self = if self.any() { Self::default() } else { Self::all() };
    }
}

/// What a ray hit - see `PhysicsWorld::cast_ray`.
#[derive(Debug, Clone)]
pub struct RayHit {
    /// The node whose body was hit - None for a collider without one.
    pub node: Option<String>,
    /// How far along the ray, in meters (the ray's direction is normalized).
    pub distance: f32,
    pub point: Vector3<f32>,
    /// The way the surface faces where it was hit.
    pub normal: Vector3<f32>,
}

/// Whether a node started or stopped overlapping a sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorChange {
    Entered,
    Exited,
}

/// A node that started or stopped overlapping a sensor in the last step -
/// see `PhysicsWorld::sensor_events`.
#[derive(Debug, Clone)]
pub struct SensorEvent {
    /// The node with the sensor.
    pub sensor: String,
    /// The node it detected.
    pub other: String,
    pub change: SensorChange,
}

/// See the module doc.
pub struct PhysicsWorld {
    /// Every body, and every collider - Rapier's own, for anything the
    /// helpers here don't cover.
    pub bodies: RigidBodySet,
    pub colliders: ColliderSet,
    /// Joints between bodies.
    pub impulse_joints: ImpulseJointSet,
    pub multibody_joints: MultibodyJointSet,
    /// Contacts from the last step.
    pub narrow_phase: NarrowPhase,
    /// What's where, as of the last step - for rays and shape casts.
    pub query: QueryPipeline,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad_phase: DefaultBroadPhase,
    ccd: CCDSolver,
    integration: IntegrationParameters,
    gravity: Vector3<f32>,
    /// Each node's body, by node id - and the other way round.
    handles: HashMap<String, RigidBodyHandle>,
    owners: HashMap<RigidBodyHandle, String>,
    /// Where each moving body was before the last step - `render_pose`
    /// draws it between there and where it is now.
    previous: HashMap<RigidBodyHandle, Isometry3<f32>>,
    /// Scaled time passed that no step has used up yet.
    accumulator: f32,
    running: bool,
    /// The physics gizmos that are on (`app.renderer.debug.physics`).
    gizmos: PhysicsGizmos,
    lines: Lines,
    /// Debugged colliders' faces (see `ColliderDebug`), drawn this frame.
    faces: Faces,
    /// Every collider with its own `ColliderDebug`.
    debugged: HashMap<ColliderHandle, ColliderDebug>,
    /// Colliders drawn by lines of their own instead of their shape's
    /// edges (see `Collider::outline`).
    outlines: HashMap<ColliderHandle, Outline>,
    /// The bit each layer name got (see `Layers`).
    layers: LayerBits,
    /// Every node with a sensor, and the nodes overlapping it as of the
    /// last step.
    sensors: HashMap<String, HashSet<String>>,
    /// What started or stopped overlapping a sensor in the last step.
    sensor_events: Vec<SensorEvent>,
    /// Nodes whose body follows their node (a kinematic position-based one).
    followers: HashSet<String>,
}

impl PhysicsWorld {
    /// The most steps one frame runs - after a stall (a debugger break, the
    /// PC freezing a moment, steps slower than real time), time beyond this
    /// is dropped instead of made up: catching it all up would take longer
    /// still, and fall further behind every frame.
    const MAX_STEPS_PER_FRAME: u32 = 5;

    pub(crate) fn new() -> Self {
        Self {
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            impulse_joints: ImpulseJointSet::new(),
            multibody_joints: MultibodyJointSet::new(),
            narrow_phase: NarrowPhase::new(),
            query: QueryPipeline::new(),
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad_phase: DefaultBroadPhase::new(),
            ccd: CCDSolver::new(),
            integration: IntegrationParameters { dt: 1.0 / 120.0, ..IntegrationParameters::default() },
            gravity: Vector3::new(0.0, -9.81, 0.0),
            handles: HashMap::new(),
            owners: HashMap::new(),
            previous: HashMap::new(),
            accumulator: 0.0,
            running: false,
            gizmos: PhysicsGizmos::default(),
            lines: Lines::default(),
            faces: Faces::default(),
            debugged: HashMap::new(),
            outlines: HashMap::new(),
            layers: LayerBits::default(),
            sensors: HashMap::new(),
            sensor_events: Vec::new(),
            followers: HashSet::new(),
        }
    }

    /// Starts the open scene's physics from nothing: its `bodies`, with the
    /// gravity and timestep set now.
    pub(crate) fn start(&mut self, bodies: Vec<PhysicsObjectDef>) {
        self.stop();
        self.running = true;
        for def in bodies {
            self.add_body(def);
        }
    }

    /// Drops every body - the scene's closing. The gravity and timestep stay.
    pub(crate) fn stop(&mut self) {
        let (gravity, timestep) = (self.gravity, self.integration.dt);
        *self = Self::new();
        self.gravity = gravity;
        self.integration.dt = timestep;
    }

    /// Whether the open scene's physics has started - from when the scene
    /// starts until it closes.
    pub fn is_running(&self) -> bool {
        self.running
    }

    /// How long one step is, in seconds - what a `fixed_update` moves
    /// things by. 1/120 by default.
    pub fn timestep(&self) -> f32 {
        self.integration.dt
    }

    /// Sets how long one step is, in seconds (1/1000 at the least) - for
    /// every scene, from the next step on. Shorter steps are steadier for
    /// fast things, and cost more.
    pub fn set_timestep(&mut self, timestep: f32) {
        self.integration.dt = timestep.max(1.0 / 1000.0);
    }

    /// The world's gravity, in m/s² - what every body falls by (times its
    /// own `gravity_scale`, Rapier's `RigidBodyBuilder::gravity_scale`). (0, -9.81, 0) by default.
    pub fn gravity(&self) -> Vector3<f32> {
        self.gravity
    }

    /// Sets the world's gravity - for every scene, from the next step on,
    /// until it's set again. Particles fall by it too.
    pub fn set_gravity(&mut self, gravity: Vector3<f32>) {
        self.gravity = gravity;
    }

    /// The body of node `id`.
    pub fn body(&self, id: &str) -> Option<&RigidBody> {
        self.handles.get(id).and_then(|handle| self.bodies.get(*handle))
    }

    pub fn body_mut(&mut self, id: &str) -> Option<&mut RigidBody> {
        self.handles.get(id).and_then(|handle| self.bodies.get_mut(*handle))
    }

    /// Node `id`'s collider number `index`, in the order its `Colliders`
    /// added them (a `Meshes::each` collider counts each of its pieces) -
    /// Rapier's own, to change while it runs: its shape (`set_shape`), where
    /// it sits (`set_translation_wrt_parent`), friction, whether it's on.
    pub fn collider_mut(&mut self, id: &str, index: usize) -> Option<&mut Collider> {
        let body = self.bodies.get(*self.handles.get(id)?)?;
        let handle = *body.colliders().get(index)?;
        self.colliders.get_mut(handle)
    }

    /// The handle of node `id`'s body - for Rapier's own calls (joints,
    /// query filters).
    pub fn handle(&self, id: &str) -> Option<RigidBodyHandle> {
        self.handles.get(id).copied()
    }

    /// The node whose body `handle` is.
    pub fn owner(&self, handle: RigidBodyHandle) -> Option<&str> {
        self.owners.get(&handle).map(String::as_str)
    }

    /// The first thing a ray from `origin` along `direction` hits within
    /// `max_distance` - skipping node `ignore`'s body (the one casting it,
    /// usually). As of the last step.
    pub fn cast_ray(&self, origin: Vector3<f32>, direction: Vector3<f32>, max_distance: f32, ignore: Option<&str>) -> Option<RayHit> {
        let direction = direction.try_normalize(1e-6)?;
        let mut filter = QueryFilter::default().exclude_sensors();
        if let Some(handle) = ignore.and_then(|id| self.handle(id)) {
            filter = filter.exclude_rigid_body(handle);
        }
        let ray = Ray::new(Point3::from(origin), direction);
        let (collider, hit) = self.query.cast_ray_and_get_normal(&self.bodies, &self.colliders, &ray, max_distance, true, filter)?;
        let node = self.colliders.get(collider).and_then(Collider::parent).and_then(|body| self.owner(body)).map(str::to_owned);
        Some(RayHit { node, distance: hit.time_of_impact, point: origin + direction * hit.time_of_impact, normal: hit.normal })
    }

    /// Puts node `id`'s body at `position`, turned `rotation`, stopped -
    /// drawn there right away, not slid there from where it was.
    pub fn teleport(&mut self, id: &str, position: Vector3<f32>, rotation: UnitQuaternion<f32>) {
        let Some(handle) = self.handle(id) else { return };
        let Some(body) = self.bodies.get_mut(handle) else { return };
        body.set_position(Isometry3::from_parts(position.into(), rotation), true);
        body.set_linvel(Vector3::zeros(), true);
        body.set_angvel(Vector3::zeros(), true);
        self.previous.insert(handle, *body.position());
    }

    /// Holds node `id`'s body still - nothing pushes it, it doesn't fall,
    /// whatever moves its node moves it (`RigidBody::set_next_kinematic_position`)
    /// - or lets it go again. A static body stays static.
    pub fn set_kinematic(&mut self, id: &str, kinematic: bool) {
        if self.followers.contains(id) {
            return;
        }
        let Some(body) = self.body_mut(id).filter(|body| !body.is_fixed()) else { return };
        let body_type = if kinematic { RigidBodyType::KinematicPositionBased } else { RigidBodyType::Dynamic };
        body.set_body_type(body_type, true);
        if kinematic {
            body.set_linvel(Vector3::zeros(), true);
            body.set_angvel(Vector3::zeros(), true);
        }
    }

    /// How much of the world's gravity node `id`'s body feels from now on -
    /// see Rapier's `RigidBodyBuilder::gravity_scale`.
    pub fn set_gravity_scale(&mut self, id: &str, scale: f32) {
        if let Some(body) = self.body_mut(id) {
            body.set_gravity_scale(scale, true);
        }
    }

    /// What started or stopped overlapping a sensor in the last step - the
    /// same the sensors' behaviors hear about. Seen by the next step's
    /// `fixed_update`s; an `update` may see a step's twice or not at all.
    pub fn sensor_events(&self) -> &[SensorEvent] {
        &self.sensor_events
    }

    /// The nodes that started overlapping node `sensor`'s sensor in the
    /// last step.
    pub fn entered<'a>(&'a self, sensor: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.changes(sensor, SensorChange::Entered)
    }

    /// The nodes that stopped overlapping node `sensor`'s sensor in the
    /// last step - despawned ones too.
    pub fn exited<'a>(&'a self, sensor: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.changes(sensor, SensorChange::Exited)
    }

    fn changes<'a>(&'a self, sensor: &'a str, change: SensorChange) -> impl Iterator<Item = &'a str> + 'a {
        self.sensor_events.iter().filter(move |event| event.sensor == sensor && event.change == change).map(|event| event.other.as_str())
    }

    /// The nodes overlapping node `sensor`'s sensor, as of the last step.
    pub fn inside<'a>(&'a self, sensor: &str) -> impl Iterator<Item = &'a str> + 'a {
        self.sensors.get(sensor).into_iter().flatten().map(String::as_str)
    }

    /// Whether node `other` overlaps node `sensor`'s sensor, as of the last
    /// step.
    pub fn is_inside(&self, sensor: &str, other: &str) -> bool {
        self.sensors.get(sensor).is_some_and(|inside| inside.contains(other))
    }

    /// The nodes whose body follows their node - see `follow`.
    pub(crate) fn followers(&self) -> impl Iterator<Item = &str> {
        self.followers.iter().map(String::as_str)
    }

    /// Moves node `id`'s following body (a kinematic position-based one) to
    /// where its node is, by the next step.
    pub(crate) fn follow(&mut self, id: &str, position: Vector3<f32>, rotation: UnitQuaternion<f32>) {
        if let Some(body) = self.body_mut(id) {
            body.set_next_kinematic_position(Isometry3::from_parts(position.into(), rotation));
        }
    }

    /// Lines a game draws for its bodies - forces, pushes - in world space,
    /// shown with the bodies' gizmos: `None` while
    /// `app.renderer.debug.physics.rigidbodies` is off, so nothing's drawn
    /// for nobody. Emptied every frame: draw them again every frame.
    pub fn debug_lines(&mut self) -> Option<&mut Lines> {
        self.gizmos.rigidbodies.then_some(&mut self.lines)
    }

    pub(crate) fn lines(&self) -> &Lines {
        &self.lines
    }

    /// A node's body - replacing any it had.
    pub(crate) fn add_body(&mut self, def: PhysicsObjectDef) {
        let id = def.id.clone();
        self.remove_body(&id);
        let mut loaded = HashMap::new();
        load_physics_from_definitions(std::slice::from_ref(&def), &mut self.colliders, &mut self.bodies, &mut loaded, &mut self.layers, &mut self.debugged, &mut self.outlines);
        if let Some(Some(data)) = loaded.remove(&id) {
            if def.colliders.iter().any(|collider| collider.is_sensor()) {
                self.sensors.insert(id.clone(), HashSet::new());
            }
            if def.follows_node {
                self.followers.insert(id.clone());
            }
            self.owners.insert(data.rigidbody_handle, id.clone());
            self.handles.insert(id, data.rigidbody_handle);
        }
    }

    /// Drops node `id`'s body, with its colliders - nothing if it has none.
    pub(crate) fn remove_body(&mut self, id: &str) {
        let Some(handle) = self.handles.remove(id) else { return };
        // What it overlapped hears it leave after the next step.
        self.sensors.remove(id);
        self.followers.remove(id);
        if let Some(body) = self.bodies.get(handle) {
            for collider in body.colliders() {
                self.debugged.remove(collider);
                self.outlines.remove(collider);
            }
        }
        self.owners.remove(&handle);
        self.previous.remove(&handle);
        self.bodies.remove(handle, &mut self.islands, &mut self.colliders, &mut self.impulse_joints, &mut self.multibody_joints, true);
    }

    /// Moves every body and collider by `shift` - static ones too - for the
    /// floating origin (see `WorldOrigin`). Velocities, contacts and joints
    /// are relative, so they carry on as they were; where a moving body was
    /// before the last step moves too, so it isn't drawn sliding across the
    /// shift. Rays cast right after already see the moved world.
    ///
    /// The next step works out which colliders are near which again, for
    /// all of them - about what the scene's first step costs.
    pub(crate) fn shift_origin(&mut self, shift: Vector3<f32>) {
        for (_, body) in self.bodies.iter_mut() {
            let translation = *body.translation() + shift;
            body.set_translation(translation, false);
        }
        for (_, collider) in self.colliders.iter_mut().filter(|(_, collider)| collider.parent().is_none()) {
            let translation = collider.translation() + shift;
            collider.set_translation(translation);
        }
        for previous in self.previous.values_mut() {
            previous.translation.vector += shift;
        }
        self.bodies.propagate_modified_body_positions_to_colliders(&mut self.colliders);
        self.query.update(&self.colliders);
    }

    /// Adds a frame's `elapsed` (scaled) seconds, and says how many steps
    /// are due now.
    pub(crate) fn steps_due(&mut self, elapsed: f32, gizmos: PhysicsGizmos) -> u32 {
        self.gizmos = gizmos;
        self.lines.clear();
        self.faces.clear();
        if !self.running {
            return 0;
        }
        let dt = self.integration.dt;
        self.accumulator = (self.accumulator + elapsed.max(0.0)).min(dt * Self::MAX_STEPS_PER_FRAME as f32);
        let steps = (self.accumulator / dt) as u32;
        self.accumulator -= steps as f32 * dt;
        steps
    }

    /// Advances the world by one step.
    pub(crate) fn step(&mut self) {
        // Where the moving bodies are before it - drawn between there and
        // after (see render_pose).
        self.previous.clear();
        for (handle, body) in self.bodies.iter().filter(|(_, body)| !body.is_fixed()) {
            self.previous.insert(handle, *body.position());
        }
        self.pipeline.step(
            &self.gravity,
            &self.integration,
            &mut self.islands,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.impulse_joints,
            &mut self.multibody_joints,
            &mut self.ccd,
            Some(&mut self.query),
            &(),
            &(),
        );
        self.update_sensors();
    }

    /// What overlaps each sensor now, against what did before the step -
    /// the differences into `sensor_events`.
    fn update_sensors(&mut self) {
        self.sensor_events.clear();
        for (id, inside) in self.sensors.iter_mut() {
            let mut now = HashSet::new();
            let body = self.handles.get(id).and_then(|handle| self.bodies.get(*handle));
            for &collider in body.map(|body| body.colliders()).unwrap_or_default() {
                if !self.colliders.get(collider).is_some_and(|collider| collider.is_sensor() && collider.is_enabled()) {
                    continue;
                }
                for (a, b, overlapping) in self.narrow_phase.intersection_pairs_with(collider) {
                    let other = if a == collider { b } else { a };
                    let owner = self.colliders.get(other).and_then(Collider::parent).and_then(|body| self.owners.get(&body));
                    if let Some(owner) = owner.filter(|owner| overlapping && *owner != id) {
                        now.insert(owner.clone());
                    }
                }
            }
            let mut exited: Vec<&String> = inside.difference(&now).collect();
            let mut entered: Vec<&String> = now.difference(inside).collect();
            exited.sort();
            entered.sort();
            let event = |other: &String, change| SensorEvent { sensor: id.clone(), other: other.clone(), change };
            self.sensor_events.extend(exited.into_iter().map(|other| event(other, SensorChange::Exited)));
            self.sensor_events.extend(entered.into_iter().map(|other| event(other, SensorChange::Entered)));
            *inside = now;
        }
    }

    /// Where `handle`'s body is drawn this frame: between where it was before
    /// the last step and where it is now, by how much of the next step's time
    /// has passed already.
    fn render_pose(&self, handle: RigidBodyHandle, body: &RigidBody) -> Isometry3<f32> {
        let current = *body.position();
        match self.previous.get(&handle) {
            Some(previous) => previous.lerp_slerp(&current, (self.accumulator / self.integration.dt).clamp(0.0, 1.0)),
            None => current,
        }
    }

    /// Every moving body's node id and where it's drawn this frame (see
    /// `render_pose`).
    pub(crate) fn render_poses(&self) -> impl Iterator<Item = (&str, Isometry3<f32>)> {
        self.handles.iter().filter_map(|(id, handle)| {
            let body = self.bodies.get(*handle).filter(|body| !body.is_fixed() && !self.followers.contains(id))?;
            Some((id.as_str(), self.render_pose(*handle, body)))
        })
    }

    /// The physics gizmos that are on (see `PhysicsGizmos`), into the debug
    /// lines where things are drawn this frame - and every collider with
    /// its own `ColliderDebug`, its own way, gizmos on or not.
    pub(crate) fn draw_gizmos(&mut self) {
        if !self.gizmos.any() && self.debugged.is_empty() {
            return;
        }
        const MOVING_COLOR: [f32; 3] = [1.0, 0.55, 0.1];
        const FIXED_COLOR: [f32; 3] = [0.35, 0.45, 0.6];
        let gizmos = self.gizmos;
        let default_look = ColliderDebug::default().no_faces();
        let mut lines = std::mem::take(&mut self.lines);
        let mut faces = std::mem::take(&mut self.faces);
        for (handle, body) in self.bodies.iter() {
            let occupied = self.owners.get(&handle).and_then(|id| self.sensors.get(id)).is_some_and(|inside| !inside.is_empty());
            let mut pose = None;
            for &collider_handle in body.colliders() {
                let Some(collider) = self.colliders.get(collider_handle).filter(|collider| collider.is_enabled()) else { continue };
                let (edges, face_color) = match self.debugged.get(&collider_handle) {
                    Some(look) => {
                        let (edges, faces) = look.colors(occupied);
                        (edges.map(rgb), faces.map(|color| color.to_rgba_f32()))
                    }
                    None if collider.is_sensor() && gizmos.sensors => (default_look.colors(occupied).0.map(rgb), None),
                    None if !collider.is_sensor() && gizmos.colliders => (Some(if body.is_fixed() { FIXED_COLOR } else { MOVING_COLOR }), None),
                    None => continue,
                };
                let pose = *pose.get_or_insert_with(|| self.render_pose(handle, body));
                let at = pose * collider.position_wrt_parent().copied().unwrap_or_else(Isometry3::identity);
                if let Some(color) = face_color {
                    draw_faces(&mut faces, &at, collider.shape(), color);
                }
                match (edges, self.outlines.get(&collider_handle)) {
                    (Some(color), Some(outline)) => {
                        for [a, b] in outline.iter() {
                            lines.line((at * a).coords, (at * b).coords, color);
                        }
                    }
                    (Some(color), None) => draw_shape(&mut lines, &at, collider.shape(), color),
                    (None, _) => {}
                }
            }
            if gizmos.rigidbodies && body.is_dynamic() {
                let pose = pose.unwrap_or_else(|| self.render_pose(handle, body));
                draw_body(&mut lines, &pose, body, &self.colliders);
            }
        }
        self.lines = lines;
        self.faces = faces;
    }

    /// The faces debugged colliders drew this frame (see `ColliderDebug`).
    pub(crate) fn faces(&self) -> &Faces {
        &self.faces
    }
}

fn rgb(color: crate::ui::color::UiColor) -> [f32; 3] {
    let [r, g, b, _] = color.to_rgba_f32();
    [r, g, b]
}

/// A dynamic body's gizmo at `pose`: its center of mass as three axes
/// along its principal axes of inertia (red, green, blue - all gray while
/// it's asleep), sized to its colliders, and its velocity - a yellow line
/// as far as it'll go in a second.
fn draw_body(lines: &mut Lines, pose: &Isometry3<f32>, body: &RigidBody, colliders: &ColliderSet) {
    let mass = &body.mass_properties().local_mprops;
    let center = pose * mass.local_com;
    let reach = body.colliders().iter()
        .filter_map(|handle| colliders.get(*handle))
        .map(|collider| collider.shape().compute_local_bounding_sphere().radius())
        .fold(0.0, f32::max);
    let size = (reach * 0.4).clamp(0.1, 3.0);
    let axes = pose.rotation * mass.principal_inertia_local_frame;
    const GRAY: [f32; 3] = [0.55, 0.55, 0.55];
    for (axis, color) in [(Vector3::x(), [1.0, 0.2, 0.2]), (Vector3::y(), [0.2, 1.0, 0.2]), (Vector3::z(), [0.3, 0.5, 1.0])] {
        let color = if body.is_sleeping() { GRAY } else { color };
        let arm = axes * axis * size;
        lines.line(center.coords - arm * 0.25, center.coords + arm, color);
    }
    let velocity = *body.linvel();
    if velocity.norm() > 1e-3 {
        lines.line(center.coords, center.coords + velocity, [1.0, 0.9, 0.2]);
    }
}

/// `shape` at `pose` as triangles into `faces` - the shapes `draw_shape`
/// draws, and convex hulls and triangle meshes.
fn draw_faces(faces: &mut Faces, pose: &Isometry3<f32>, shape: &dyn Shape, color: [f32; 4]) {
    const ROUND: u32 = 24;
    let (points, triangles) = if let Some(cuboid) = shape.as_cuboid() {
        cuboid.to_trimesh()
    } else if let Some(ball) = shape.as_ball() {
        ball.to_trimesh(ROUND, ROUND / 2)
    } else if let Some(cylinder) = shape.as_cylinder() {
        cylinder.to_trimesh(ROUND)
    } else if let Some(cone) = shape.as_cone() {
        cone.to_trimesh(ROUND)
    } else if let Some(capsule) = shape.as_capsule() {
        capsule.to_trimesh(ROUND, ROUND / 2)
    } else if let Some(hull) = shape.as_convex_polyhedron() {
        // Each flat face as triangles from its middle.
        let mut points = hull.points().to_vec();
        let mut triangles = Vec::new();
        let around = hull.vertices_adj_to_face();
        for face in hull.faces() {
            let corners = &around[face.first_vertex_or_edge as usize..(face.first_vertex_or_edge + face.num_vertices_or_edges) as usize];
            let middle = corners.iter().map(|corner| points[*corner as usize].coords).sum::<Vector3<f32>>() / corners.len() as f32;
            let center = points.len() as u32;
            points.push(Point3::from(middle));
            for (index, corner) in corners.iter().enumerate() {
                triangles.push([center, *corner, corners[(index + 1) % corners.len()]]);
            }
        }
        (points, triangles)
    } else if let Some(mesh) = shape.as_trimesh() {
        (mesh.vertices().to_vec(), mesh.indices().to_vec())
    } else if let Some(compound) = shape.as_compound() {
        for (part_pose, part) in compound.shapes() {
            draw_faces(faces, &(pose * part_pose), part.as_ref(), color);
        }
        return;
    } else {
        return;
    };
    let world: Vec<Vector3<f32>> = points.iter().map(|point| (pose * point).coords).collect();
    for [a, b, c] in triangles {
        faces.triangle(world[a as usize], world[b as usize], world[c as usize], color);
    }
}

/// `shape` at `pose` into `lines` - boxes as their 12 edges, balls as 3
/// circles, cylinders, cones and capsules as their circles and 8 sides, compounds
/// as their parts, convex
/// hulls and triangle meshes as their edges. Other shapes aren't drawn.
pub fn draw_shape(lines: &mut Lines, pose: &Isometry3<f32>, shape: &dyn Shape, color: [f32; 3]) {
    let world = |local: Vector3<f32>| (pose * Point3::from(local)).coords;

    if let Some(cuboid) = shape.as_cuboid() {
        let h = cuboid.half_extents;
        let corner = |x: f32, y: f32, z: f32| world(Vector3::new(h.x * x, h.y * y, h.z * z));
        for (sy, sz) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
            lines.line(corner(-1.0, sy, sz), corner(1.0, sy, sz), color);
        }
        for (sx, sz) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
            lines.line(corner(sx, -1.0, sz), corner(sx, 1.0, sz), color);
        }
        for (sx, sy) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
            lines.line(corner(sx, sy, -1.0), corner(sx, sy, 1.0), color);
        }
    } else if let Some(ball) = shape.as_ball() {
        let r = ball.radius;
        circle(lines, &world, Vector3::zeros(), Vector3::x() * r, Vector3::y() * r, color);
        circle(lines, &world, Vector3::zeros(), Vector3::y() * r, Vector3::z() * r, color);
        circle(lines, &world, Vector3::zeros(), Vector3::z() * r, Vector3::x() * r, color);
    } else if let Some(cylinder) = shape.as_cylinder() {
        let (r, h) = (cylinder.radius, cylinder.half_height);
        for y in [-h, h] {
            circle(lines, &world, Vector3::y() * y, Vector3::x() * r, Vector3::z() * r, color);
        }
        for side in 0..8 {
            let (sin, cos) = (side as f32 / 8.0 * std::f32::consts::TAU).sin_cos();
            let side = Vector3::new(cos, 0.0, sin) * r;
            lines.line(world(side - Vector3::y() * h), world(side + Vector3::y() * h), color);
        }
    } else if let Some(cone) = shape.as_cone() {
        // Its base circle and 8 lines up to the tip.
        let (r, h) = (cone.radius, cone.half_height);
        circle(lines, &world, -Vector3::y() * h, Vector3::x() * r, Vector3::z() * r, color);
        for side in 0..8 {
            let (sin, cos) = (side as f32 / 8.0 * std::f32::consts::TAU).sin_cos();
            lines.line(world(Vector3::new(cos * r, -h, sin * r)), world(Vector3::y() * h), color);
        }
    } else if let Some(compound) = shape.as_compound() {
        for (part_pose, part) in compound.shapes() {
            draw_shape(lines, &(pose * part_pose), part.as_ref(), color);
        }
    } else if let Some(capsule) = shape.as_capsule() {
        // Its two ball centers, wherever along its axis they are.
        let (a, b, r) = (capsule.segment.a.coords, capsule.segment.b.coords, capsule.radius);
        let axis = (b - a).try_normalize(1e-6).unwrap_or_else(Vector3::y);
        let side_a = axis.cross(&Vector3::x()).try_normalize(1e-6).unwrap_or_else(|| axis.cross(&Vector3::z()).normalize());
        let side_b = axis.cross(&side_a);
        for center in [a, b] {
            circle(lines, &world, center, side_a * r, side_b * r, color);
            circle(lines, &world, center, side_a * r, axis * r, color);
            circle(lines, &world, center, side_b * r, axis * r, color);
        }
        for side in 0..8 {
            let (sin, cos) = (side as f32 / 8.0 * std::f32::consts::TAU).sin_cos();
            let side = side_a * cos + side_b * sin;
            lines.line(world(a + side * r), world(b + side * r), color);
        }
    } else if let Some(hull) = shape.as_convex_polyhedron() {
        // Its faces' outlines - not the lines splitting a flat face into
        // triangles (both sides of one of those are the same face).
        let points = hull.points();
        for edge in hull.edges().iter().filter(|edge| edge.faces.x != edge.faces.y) {
            lines.line(world(points[edge.vertices.x as usize].coords), world(points[edge.vertices.y as usize].coords), color);
        }
    } else if let Some(mesh) = shape.as_trimesh() {
        let points = mesh.vertices();
        for [a, b, c] in mesh.indices() {
            let [a, b, c] = [a, b, c].map(|index| world(points[*index as usize].coords));
            lines.line(a, b, color);
            lines.line(b, c, color);
            lines.line(c, a, color);
        }
    }
}

/// A circle in local space (center + cos*a + sin*b), mapped by `world`.
fn circle(lines: &mut Lines, world: &dyn Fn(Vector3<f32>) -> Vector3<f32>, center: Vector3<f32>, a: Vector3<f32>, b: Vector3<f32>, color: [f32; 3]) {
    const SEGMENTS: usize = 24;
    let point = |i: usize| {
        let angle = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        world(center + a * angle.cos() + b * angle.sin())
    };
    for i in 0..SEGMENTS {
        lines.line(point(i), point(i + 1), color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::body::{follows_node, implied_body};
    use crate::physics::{Collider, Colliders};
    use rapier3d::prelude::RigidBodyBuilder;
    use rapier3d::prelude::ColliderBuilder;

    /// A node's body as physics_bridge makes it: its `body`, or the one its
    /// `colliders` imply.
    fn def(id: &str, position: Vector3<f32>, body: Option<RigidBodyBuilder>, colliders: Colliders, layers: &[&str]) -> PhysicsObjectDef {
        let colliders = colliders.into_entries();
        let body = body.unwrap_or_else(|| implied_body(&colliders));
        PhysicsObjectDef {
            id: id.to_owned(),
            position,
            rotation: UnitQuaternion::identity(),
            follows_node: follows_node(&body),
            body,
            colliders,
            layers: layers.iter().map(|layer| layer.to_string()).collect(),
        }
    }

    fn ball(id: &str, x: f32, layers: &[&str]) -> PhysicsObjectDef {
        let colliders = Colliders::new().add(ColliderBuilder::ball(0.25).mass(1.0));
        def(id, Vector3::new(x, 5.0, 0.0), Some(RigidBodyBuilder::dynamic()), colliders, layers)
    }

    /// A trigger zone: sensor colliders and no body of its own.
    fn zone(detects: &[&str]) -> PhysicsObjectDef {
        let colliders = Colliders::new().add(ColliderBuilder::cuboid(3.0, 1.0, 3.0).sensor(true)).detects(detects.iter().copied());
        def("zone", Vector3::zeros(), None, colliders, &[])
    }

    /// Steps until the balls fell through the zone and out the bottom,
    /// collecting every event.
    fn run(world: &mut PhysicsWorld) -> Vec<(String, SensorChange)> {
        let mut seen = Vec::new();
        for _ in 0..240 {
            world.step();
            seen.extend(world.sensor_events().iter().map(|event| (event.other.clone(), event.change)));
        }
        seen
    }

    #[test]
    fn sensor_sees_layer_enter_and_exit() {
        let mut world = PhysicsWorld::new();
        world.start(vec![zone(&["player"]), ball("player", 0.0, &["player"]), ball("rock", 1.0, &[])]);
        let seen = run(&mut world);
        assert_eq!(seen, vec![("player".to_owned(), SensorChange::Entered), ("player".to_owned(), SensorChange::Exited)]);
    }

    #[test]
    fn sensor_without_detects_sees_everything() {
        let mut world = PhysicsWorld::new();
        world.start(vec![zone(&[]), ball("player", 0.0, &["player"]), ball("rock", 1.0, &[])]);
        let seen = run(&mut world);
        assert_eq!(seen.iter().filter(|(_, change)| *change == SensorChange::Entered).count(), 2);
    }

    #[test]
    fn despawned_body_exits() {
        let mut world = PhysicsWorld::new();
        world.start(vec![zone(&[]), ball("rock", 0.0, &[])]);
        world.teleport("rock", Vector3::zeros(), UnitQuaternion::identity());
        world.set_gravity(Vector3::zeros());
        world.step();
        assert!(world.is_inside("zone", "rock"));
        world.remove_body("rock");
        world.step();
        assert_eq!(world.exited("zone").collect::<Vec<_>>(), vec!["rock"]);
        assert_eq!(world.inside("zone").count(), 0);
    }

    #[test]
    fn each_gizmo_draws_its_own_things() {
        let mut world = PhysicsWorld::new();
        let ground = def("ground", Vector3::zeros(), None, Colliders::new().add(ColliderBuilder::cuboid(10.0, 0.1, 10.0)), &[]);
        world.start(vec![ground, ball("rock", 0.0, &[]), zone(&[])]);
        let drawn = |world: &mut PhysicsWorld, gizmos: PhysicsGizmos| {
            world.steps_due(0.0, gizmos);
            world.draw_gizmos();
            let count = world.lines().segments().len();
            world.lines.clear();
            count
        };
        assert_eq!(drawn(&mut world, PhysicsGizmos::default()), 0);
        // The ground's 12 edges (fixed ones too) and the ball's 3 circles.
        let colliders = drawn(&mut world, PhysicsGizmos { colliders: true, ..Default::default() });
        assert_eq!(colliders, 12 + 3 * 24);
        // Just the zone's box.
        assert_eq!(drawn(&mut world, PhysicsGizmos { sensors: true, ..Default::default() }), 12);
        // The ball's center of mass, 3 axes - not moving yet, so no velocity.
        assert_eq!(drawn(&mut world, PhysicsGizmos { rigidbodies: true, ..Default::default() }), 3);
        assert_eq!(drawn(&mut world, PhysicsGizmos::all()), 12 + 3 * 24 + 12 + 3);
        // The game's own lines go with the bodies' gizmo.
        world.steps_due(0.0, PhysicsGizmos { colliders: true, sensors: true, rigidbodies: false });
        assert!(world.debug_lines().is_none());
        world.steps_due(0.0, PhysicsGizmos { rigidbodies: true, ..Default::default() });
        assert!(world.debug_lines().is_some());
    }

    #[test]
    fn toggling_all_gizmos_turns_them_off_if_any_is_on() {
        let mut gizmos = PhysicsGizmos { sensors: true, ..Default::default() };
        gizmos.toggle_all();
        assert_eq!(gizmos, PhysicsGizmos::default());
        gizmos.toggle_all();
        assert_eq!(gizmos, PhysicsGizmos::all());
    }

    #[test]
    fn a_collider_with_an_outline_draws_it_where_the_collider_is() {
        let mut world = PhysicsWorld::new();
        let mut colliders = Colliders::new().add(ColliderBuilder::cuboid(1.0, 1.0, 1.0)).debug(crate::physics::ColliderDebug::new()).into_entries();
        let outline: Outline = vec![[Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)]].into();
        colliders[0].outline = Some(outline);
        let mut def = def("traced", Vector3::new(5.0, 0.0, 0.0), None, Colliders::new(), &[]);
        def.colliders = colliders;
        world.start(vec![def]);
        world.steps_due(0.0, PhysicsGizmos::default());
        world.draw_gizmos();
        // Its one line, moved with its body - not the box's 12 edges.
        let segments = world.lines().segments();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0][0].position[0], 5.0);
    }

    #[test]
    fn debugged_collider_draws_with_no_view_on() {
        let mut world = PhysicsWorld::new();
        let sensor = Colliders::new().add(ColliderBuilder::ball(1.0).sensor(true)).debug(crate::physics::ColliderDebug::new());
        world.start(vec![
            def("zone", Vector3::zeros(), None, sensor, &[]),
            ball("rock", 10.0, &[]),
        ]);
        world.steps_due(0.0, PhysicsGizmos::default());
        world.draw_gizmos();
        assert!(!world.lines().is_empty() && !world.faces().is_empty());
        // Only the debugged one: the rock's edges aren't there.
        let segments = world.lines().segments().len();
        world.remove_body("zone");
        world.steps_due(0.0, PhysicsGizmos::default());
        world.draw_gizmos();
        assert!(world.lines().is_empty() && world.faces().is_empty() && segments > 0);
    }

    #[test]
    fn hemisphere_is_only_the_top_half() {
        let mut world = PhysicsWorld::new();
        let zone = Colliders::new().add(Collider::hemisphere(2.0).sensor(true));
        world.start(vec![
            def("dome", Vector3::zeros(), None, zone, &[]),
            ball("above", 0.0, &[]),
            ball("below", 0.0, &[]),
        ]);
        world.set_gravity(Vector3::zeros());
        world.teleport("above", Vector3::new(0.0, 1.0, 0.0), UnitQuaternion::identity());
        world.teleport("below", Vector3::new(0.0, -1.0, 0.0), UnitQuaternion::identity());
        world.step();
        assert!(world.is_inside("dome", "above"));
        assert!(!world.is_inside("dome", "below"));
    }

    #[test]
    fn hemisphere_outline_has_no_lines_across_its_base() {
        let shape = Collider::hemisphere(2.0).shape;
        let mut lines = Lines::default();
        draw_shape(&mut lines, &Isometry3::identity(), &*shape, [1.0; 3]);
        // On the ground, only the rim - no line through the inside of the base.
        for [a, b] in lines.segments() {
            if a.position[1].abs() < 1e-4 && b.position[1].abs() < 1e-4 {
                let middle = Vector3::from(a.position).lerp(&Vector3::from(b.position), 0.5);
                assert!(middle.norm() > 1.9, "a line across the base: {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn follower_moves_with_its_node() {
        let mut world = PhysicsWorld::new();
        world.start(vec![zone(&[]), ball("rock", 10.0, &[])]);
        world.set_gravity(Vector3::zeros());
        world.teleport("rock", Vector3::new(10.0, 0.0, 0.0), UnitQuaternion::identity());
        world.step();
        assert!(!world.is_inside("zone", "rock"));
        world.follow("zone", Vector3::new(10.0, 0.0, 0.0), UnitQuaternion::identity());
        world.step();
        assert!(world.is_inside("zone", "rock"));
        assert!(world.render_poses().all(|(id, _)| id != "zone"));
    }

    #[test]
    fn a_body_is_placed_at_its_node_and_weighs_what_its_colliders_say() {
        let mut world = PhysicsWorld::new();
        // The builder's own position is replaced by the node's.
        let body = RigidBodyBuilder::dynamic().translation(Vector3::new(99.0, 99.0, 99.0));
        let colliders = Colliders::new()
            .add(ColliderBuilder::cuboid(0.5, 0.5, 0.5).mass(20.0))
            .add(ColliderBuilder::ball(0.5).translation(Vector3::new(0.0, 2.0, 0.0)).mass(5.0));
        world.start(vec![def("crate", Vector3::new(1.0, 2.0, 3.0), Some(body), colliders, &[])]);
        let body = world.body("crate").unwrap();
        assert!((body.translation() - Vector3::new(1.0, 2.0, 3.0)).norm() < 1e-5);
        // Exactly the colliders' masses - nothing hidden added.
        assert!((body.mass() - 25.0).abs() < 1e-3, "mass {}", body.mass());
        // The center of mass is pulled toward the heavier part: 5 kg at y=2.
        assert!((body.center_of_mass().y - (2.0 + 5.0 * 2.0 / 25.0)).abs() < 1e-3);
    }

    #[test]
    fn body_level_mass_is_final_with_weightless_colliders() {
        // What a flight model does: its own mass, center and inertia, the
        // colliders only shaping it.
        let mut world = PhysicsWorld::new();
        let colliders = Colliders::new().add(ColliderBuilder::cuboid(1.0, 0.5, 4.0).density(0.0));
        let center = Vector3::new(0.0, 0.8, 0.0);
        let inertia = crate::physics::compute_principal_inertia(3000.0, center, colliders.builders());
        let body = RigidBodyBuilder::dynamic().additional_mass_properties(rapier3d::prelude::MassProperties::new(center.into(), 3000.0, inertia));
        world.start(vec![def("jet", Vector3::zeros(), Some(body), colliders, &[])]);
        world.set_gravity(Vector3::zeros());
        // Rapier adds the body's own mass when it next updates it.
        world.step();
        let body = world.body("jet").unwrap();
        assert!((body.mass() - 3000.0).abs() < 1e-2, "mass {}", body.mass());
        assert!((body.center_of_mass().coords - center).norm() < 1e-4);
    }

    #[test]
    fn colliders_without_a_body_stay_put_or_follow_as_zones() {
        let mut world = PhysicsWorld::new();
        world.start(vec![
            def("wall", Vector3::zeros(), None, Colliders::new().add(ColliderBuilder::cuboid(1.0, 1.0, 1.0)), &[]),
            def("zone", Vector3::zeros(), None, Colliders::new().add(ColliderBuilder::ball(1.0).sensor(true)), &[]),
        ]);
        assert!(world.body("wall").unwrap().is_fixed());
        assert!(world.body("zone").unwrap().is_kinematic());
        assert!(world.followers().any(|id| id == "zone") && !world.followers().any(|id| id == "wall"));
    }

    #[test]
    fn a_nodes_colliders_are_reached_in_the_order_they_were_added() {
        let mut world = PhysicsWorld::new();
        let colliders = Colliders::new().add(ColliderBuilder::cuboid(1.0, 1.0, 1.0)).add(ColliderBuilder::ball(0.5));
        world.start(vec![def("thing", Vector3::zeros(), Some(RigidBodyBuilder::dynamic()), colliders, &[])]);
        assert!(world.collider_mut("thing", 0).unwrap().shape().as_cuboid().is_some());
        let ball = world.collider_mut("thing", 1).unwrap();
        assert!(ball.shape().as_ball().is_some());
        ball.set_shape(rapier3d::prelude::SharedShape::ball(2.0));
        assert_eq!(world.collider_mut("thing", 1).unwrap().shape().as_ball().unwrap().radius, 2.0);
        assert!(world.collider_mut("thing", 2).is_none() && world.collider_mut("nobody", 0).is_none());
    }
}
