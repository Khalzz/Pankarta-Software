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
//! Sensors (see `Sensor`) are checked after every step: what started or
//! stopped overlapping each one is `sensor_events`, and is handed to the
//! sensor node's behaviors (`Behavior::on_sensor_enter`/`on_sensor_exit`).

use std::collections::{HashMap, HashSet};

use nalgebra::{Isometry3, Point3, UnitQuaternion, Vector3};
use rapier3d::prelude::{
    CCDSolver, Collider, ColliderHandle, ColliderSet, DefaultBroadPhase, ImpulseJointSet, IntegrationParameters, IslandManager, MultibodyJointSet, NarrowPhase,
    PhysicsPipeline, QueryFilter, QueryPipeline, Ray, RigidBody, RigidBodyHandle, RigidBodySet, RigidBodyType, Shape,
};

use crate::physics::body::ColliderDebug;
use crate::physics::physics_resources::{load_physics_from_definitions, LayerBits, PhysicsObjectDef};
use crate::rendering::lines::{Faces, Lines};

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
    /// Whether the physics debug view is on (`app.renderer.debug.physics`) -
    /// see `debug_lines`.
    debug_view: bool,
    /// Whether the sensors' debug view is on (`app.renderer.debug.sensors`).
    sensor_view: bool,
    lines: Lines,
    /// Debugged colliders' faces (see `ColliderDebug`), drawn this frame.
    faces: Faces,
    /// Every collider with its own `ColliderDebug`.
    debugged: HashMap<ColliderHandle, ColliderDebug>,
    /// The bit each layer name got (see `Layers`).
    layers: LayerBits,
    /// Every node with a sensor, and the nodes overlapping it as of the
    /// last step.
    sensors: HashMap<String, HashSet<String>>,
    /// What started or stopped overlapping a sensor in the last step.
    sensor_events: Vec<SensorEvent>,
    /// Nodes whose body follows their node (a `Sensor` without `Physics`).
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
            debug_view: false,
            sensor_view: false,
            lines: Lines::default(),
            faces: Faces::default(),
            debugged: HashMap::new(),
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
    /// own `RigidBodyData::gravity_scale`). (0, -9.81, 0) by default.
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
    /// see `RigidBodyData::gravity_scale`.
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

    /// Moves node `id`'s following body (a `Sensor` without `Physics`) to
    /// where its node is, by the next step.
    pub(crate) fn follow(&mut self, id: &str, position: Vector3<f32>, rotation: UnitQuaternion<f32>) {
        if let Some(body) = self.body_mut(id) {
            body.set_next_kinematic_position(Isometry3::from_parts(position.into(), rotation));
        }
    }

    /// Lines for the physics debug view, in world space - `None` while the
    /// view is off (`app.renderer.debug.physics`), so nothing's drawn for
    /// nobody. Emptied every frame: draw them again every frame.
    pub fn debug_lines(&mut self) -> Option<&mut Lines> {
        self.debug_view.then_some(&mut self.lines)
    }

    pub(crate) fn lines(&self) -> &Lines {
        &self.lines
    }

    /// A node's body - replacing any it had.
    pub(crate) fn add_body(&mut self, def: PhysicsObjectDef) {
        let id = def.id.clone();
        self.remove_body(&id);
        let mut loaded = HashMap::new();
        load_physics_from_definitions(std::slice::from_ref(&def), &mut self.colliders, &mut self.bodies, &mut loaded, &mut self.layers, &mut self.debugged);
        if let Some(Some(data)) = loaded.remove(&id) {
            if def.physics.colliders.iter().any(|collider| collider.sensor) {
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
    pub(crate) fn steps_due(&mut self, elapsed: f32, debug_view: bool, sensor_view: bool) -> u32 {
        self.debug_view = debug_view;
        self.sensor_view = sensor_view;
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

    /// Every moving body's colliders, into the debug lines where they're
    /// drawn this frame - static ones (the ground) skipped, they'd bury the
    /// screen - and every sensor, cyan, green while something overlaps it.
    /// While the physics debug view is on - or just the sensors, while
    /// theirs is. A collider with its own `ColliderDebug` is drawn its own
    /// way, views on or not.
    pub(crate) fn draw_colliders(&mut self) {
        if !self.debug_view && !self.sensor_view && self.debugged.is_empty() {
            return;
        }
        const COLLIDER_COLOR: [f32; 3] = [1.0, 0.55, 0.1];
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
                    None if collider.is_sensor() && (self.debug_view || self.sensor_view) => (default_look.colors(occupied).0.map(rgb), None),
                    None if !collider.is_sensor() && self.debug_view && !body.is_fixed() => (Some(COLLIDER_COLOR), None),
                    None => continue,
                };
                let pose = *pose.get_or_insert_with(|| self.render_pose(handle, body));
                let at = pose * collider.position_wrt_parent().copied().unwrap_or_else(Isometry3::identity);
                if let Some(color) = face_color {
                    draw_faces(&mut faces, &at, collider.shape(), color);
                }
                if let Some(color) = edges {
                    draw_shape(&mut lines, &at, collider.shape(), color);
                }
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
    } else {
        return;
    };
    let world: Vec<Vector3<f32>> = points.iter().map(|point| (pose * point).coords).collect();
    for [a, b, c] in triangles {
        faces.triangle(world[a as usize], world[b as usize], world[c as usize], color);
    }
}

/// `shape` at `pose` into `lines` - boxes as their 12 edges, balls as 3
/// circles, cylinders and capsules as their circles and 8 sides, convex
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
    use crate::physics::{Collider, ColliderShape, Physics, RigidBodyData, Sensor};

    fn def(id: &str, position: Vector3<f32>, physics: Physics, layers: &[&str], follows_node: bool) -> PhysicsObjectDef {
        PhysicsObjectDef {
            id: id.to_owned(),
            position,
            rotation: UnitQuaternion::identity(),
            physics,
            layers: layers.iter().map(|layer| layer.to_string()).collect(),
            follows_node,
        }
    }

    fn ball(id: &str, x: f32, layers: &[&str]) -> PhysicsObjectDef {
        let physics = Physics { rigidbody: RigidBodyData::dynamic(1.0), colliders: vec![Collider::new(ColliderShape::Ball { radius: 0.25 })] };
        def(id, Vector3::new(x, 5.0, 0.0), physics, layers, false)
    }

    fn zone(detects: &[&str]) -> PhysicsObjectDef {
        let sensor = Sensor::cuboid(Vector3::new(3.0, 1.0, 3.0)).detects(detects.iter().copied());
        def("zone", Vector3::zeros(), Physics { rigidbody: RigidBodyData::default(), colliders: vec![sensor.collider] }, &[], true)
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
    fn debugged_collider_draws_with_no_view_on() {
        let mut world = PhysicsWorld::new();
        let sensor = Sensor::sphere(1.0).debug(crate::physics::ColliderDebug::new());
        world.start(vec![
            def("zone", Vector3::zeros(), Physics { rigidbody: RigidBodyData::default(), colliders: vec![sensor.collider] }, &[], true),
            ball("rock", 10.0, &[]),
        ]);
        world.steps_due(0.0, false, false);
        world.draw_colliders();
        assert!(!world.lines().is_empty() && !world.faces().is_empty());
        // Only the debugged one: the rock's edges aren't there.
        let segments = world.lines().segments().len();
        world.remove_body("zone");
        world.steps_due(0.0, false, false);
        world.draw_colliders();
        assert!(world.lines().is_empty() && world.faces().is_empty() && segments > 0);
    }

    #[test]
    fn hemisphere_is_only_the_top_half() {
        let mut world = PhysicsWorld::new();
        let zone = Sensor::hemisphere(2.0);
        world.start(vec![
            def("dome", Vector3::zeros(), Physics { rigidbody: RigidBodyData::default(), colliders: vec![zone.collider] }, &[], true),
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
        let shape = rapier3d::prelude::SharedShape::convex_hull(&match ColliderShape::hemisphere(2.0) {
            ColliderShape::ConvexHull { points } => points.into_iter().map(Point3::from).collect::<Vec<_>>(),
            _ => unreachable!(),
        }).unwrap();
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
}
