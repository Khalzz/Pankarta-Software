use std::collections::HashMap;

use crate::app::App;
use crate::rendering::camera::handler::SceneCameras;

use nalgebra::{UnitQuaternion, Vector3};

use crate::physics::Physics;

use super::node::Node;
use super::scene_access::{SceneAccess, SceneChanges};
use super::node::properties::{LookAt, Parent, Space, Transform3D};

/// How deep a chain of parents is followed before it's taken for a loop.
const MAX_DEPTH: usize = 64;

/// Every spawned `Node` in a scene, kept flat by id - no node owns another.
/// The hierarchy is a `Parent` property on each child (see `Node::with_child`)
/// instead of literal nesting: a flat map lets any node be reached by id and
/// two nodes be mutated independently, which nested ownership would fight.
/// `resolve_hierarchy` is what makes children follow their parents.
///
/// `spawn`/`update`/`fixed_update` all take `&mut App` now, to hand down to
/// `Behavior`'s hooks (see that trait's own doc comment on why) - which means
/// this type can no longer be unit-tested on its own the way it originally
/// was: `App` needs a live window/GPU context to construct, not something a
/// unit test can reasonably spin up. The behavior-dispatch tests that used to
/// live in this file (duplicate-id guard, on_spawn/update/fixed_update call
/// counts) were removed for that reason, not because the behavior they
/// covered stopped mattering.
#[derive(Default)]
pub struct SceneNodes {
    nodes: HashMap<String, Node>,
}

impl SceneNodes {
    pub fn new() -> Self {
        SceneNodes { nodes: HashMap::new() }
    }

    /// Inserts `node`, refusing to silently overwrite an existing id - a
    /// duplicate id is almost always an authoring bug (every lookup here
    /// treats id as the unique address for that node), so this is loud about
    /// it instead of letting one node quietly clobber another.
    ///
    /// Its behaviors' `on_spawn` runs first; what they spawn or despawn is
    /// added to `changes`, for the scene to do afterwards.
    pub(crate) fn spawn(&mut self, mut node: Node, cameras: &mut SceneCameras, app: &mut App, changes: &mut SceneChanges) -> Result<(), String> {
        if self.nodes.contains_key(&node.id) {
            let message = format!("scene already has a node named '{}' - refusing to overwrite it", node.id);
            eprintln!("{message}");
            return Err(message);
        }

        node.run_on_spawn(&mut SceneAccess::new(self, cameras, changes), app);
        self.nodes.insert(node.id.clone(), node);
        Ok(())
    }

    /// Inserts `node` without running its behaviors' `on_spawn` - for a
    /// scene being built, which runs them when it starts.
    pub(crate) fn insert(&mut self, node: Node) -> Result<(), String> {
        if self.nodes.contains_key(&node.id) {
            let message = format!("scene already has a node named '{}' - refusing to overwrite it", node.id);
            eprintln!("{message}");
            return Err(message);
        }
        self.nodes.insert(node.id.clone(), node);
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    pub fn remove(&mut self, id: &str) -> Option<Node> {
        self.nodes.remove(id)
    }

    /// Puts back a node taken out with `remove` to be worked on (see
    /// `App::with_layer_node`) - unlike `spawn`, its behaviors' `on_spawn`
    /// doesn't run again.
    pub(crate) fn put_back(&mut self, node: Node) {
        self.nodes.insert(node.id.clone(), node);
    }

    /// Drops every spawned node - a scene-reset step (see `App::run`), so a
    /// node/behavior spawned by one scene (e.g. `sandbox`'s `Camera`) doesn't
    /// keep getting `update`/`fixed_update` calls after switching to a scene
    /// that never spawned it. No `on_despawn` hook exists on `Behavior` yet,
    /// so anything a behavior did outside its own node on spawn (`Camera`
    /// registering itself with `cameras`, hiding the cursor) isn't
    /// automatically undone here - only the node/behavior itself goes away.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    /// The ids of `id`'s direct children (nodes whose `Parent` is it).
    pub fn children_of(&self, id: &str) -> Vec<String> {
        self.nodes.values()
            .filter(|node| node.get_property::<Parent>().is_some_and(|parent| parent.id == id))
            .map(|node| node.id.clone())
            .collect()
    }

    /// `id`'s children, their children and so on - deepest last.
    pub fn descendants_of(&self, id: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut next = vec![id.to_owned()];
        while let Some(current) = next.pop() {
            for child in self.children_of(&current) {
                if found.len() < self.nodes.len() && !found.contains(&child) {
                    next.push(child.clone());
                    found.push(child);
                }
            }
        }
        found
    }

    /// Where `id` is in the world - its `Transform3D` (identity for a node
    /// without one, like a pure logic node).
    pub fn world_transform(&self, id: &str) -> Option<Transform3D> {
        let node = self.nodes.get(id)?;
        Some(node.transform_3d().copied().unwrap_or_default())
    }

    /// How many parents up `id` goes (0 for a node with none). A chain that
    /// loops back on itself stops at MAX_DEPTH.
    fn depth(&self, id: &str) -> usize {
        let mut depth = 0;
        let mut current = id;
        while let Some(parent) = self.nodes.get(current).and_then(|node| node.get_property::<Parent>()) {
            depth += 1;
            if depth >= MAX_DEPTH {
                break;
            }
            current = &parent.id;
        }
        depth
    }

    /// Places every `Local` child where its parent now is plus its offset,
    /// then turns every `LookAt` node to face its target - parents before
    /// their children, so a whole chain follows in the same frame. Called
    /// every frame by `App::run`, after everything that moves nodes
    /// (physics, behaviors, animation) and before the renderer and the
    /// cameras read them.
    pub fn resolve_hierarchy(&mut self) {
        let mut order: Vec<(usize, String)> = self.nodes.values()
            .filter(|node| {
                let follows = node.get_property::<Parent>().is_some_and(|parent| parent.space == Space::Local)
                    && node.get_property::<Physics>().is_none();
                follows || node.get_property::<LookAt>().is_some()
            })
            .map(|node| (self.depth(&node.id), node.id.clone()))
            .collect();
        if order.is_empty() {
            return;
        }
        order.sort();

        for (_, id) in order {
            let Some(node) = self.nodes.get(&id) else { continue };
            // A 2D node isn't placed in the 3D world - it follows a 2D parent
            // on the plane.
            if node.transform_2d().is_some() {
                let parent = node.get_property::<Parent>().filter(|parent| parent.space == Space::Local);
                let placed = parent.and_then(|parent| Some(self.nodes.get(&parent.id)?.transform_2d()?.compose(&parent.local_2d, parent.inherit_rotation)));
                if let (Some(placed), Some(transform)) = (placed, self.nodes.get_mut(&id).and_then(|node| node.transform_2d_mut())) {
                    *transform = placed;
                }
                continue;
            }
            let parent = node.get_property::<Parent>().filter(|parent| parent.space == Space::Local && node.get_property::<Physics>().is_none()).cloned();
            let look_at = node.get_property::<LookAt>().map(|look_at| look_at.target.clone());

            let mut world = node.transform_3d().copied().unwrap_or_default();
            if let Some(parent) = parent {
                // A parent that's gone leaves the child where it last was.
                if let Some(parent_world) = self.world_transform(&parent.id) {
                    world = parent_world.compose(&parent.local, parent.inherit_rotation);
                }
            }
            if let Some(target) = look_at.and_then(|target| self.world_transform(&target)) {
                if let Some(rotation) = facing(target.position - world.position) {
                    world.rotation = rotation;
                }
            }
            // Written over the node's own transform - not a new one put on
            // in its place every frame.
            if let Some(node) = self.nodes.get_mut(&id) {
                match node.transform_3d_mut() {
                    Some(transform) => *transform = world,
                    None => node.set_property(world),
                }
            }
        }
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Node> {
        self.nodes.values_mut()
    }

    /// Runs every node's attached behaviors' `update` - called once per
    /// rendered frame from `App::run`. What they spawn or despawn is added
    /// to `changes`, for the scene to do once they've all run.
    pub(crate) fn update(&mut self, cameras: &mut SceneCameras, app: &mut App, changes: &mut SceneChanges) {
        self.run_behaviors(cameras, app, changes, |node, scene, app| node.run_update(scene, app));
    }

    /// Runs `run` on every node that has behaviors - each taken out of the
    /// map while it runs, so its behaviors can be handed both the node and
    /// everything else in the scene (see `SceneAccess`), then put back.
    fn run_behaviors(&mut self, cameras: &mut SceneCameras, app: &mut App, changes: &mut SceneChanges, mut run: impl FnMut(&mut Node, &mut SceneAccess, &mut App)) {
        let ids: Vec<String> = self.nodes.values().filter(|node| node.has_behaviors()).map(|node| node.id.clone()).collect();
        for id in ids {
            let Some(mut node) = self.nodes.remove(&id) else { continue };
            run(&mut node, &mut SceneAccess::new(self, cameras, changes), app);
            self.nodes.insert(id, node);
        }
    }

    /// Runs every node's attached behaviors' `fixed_update` - once per
    /// physics step (see `Behavior::fixed_update`).
    pub(crate) fn fixed_update(&mut self, cameras: &mut SceneCameras, app: &mut App, changes: &mut SceneChanges) {
        self.run_behaviors(cameras, app, changes, |node, scene, app| node.run_fixed_update(scene, app));
    }

    /// Hands each of `events` to its sensor node's behaviors
    /// (`on_sensor_enter`/`on_sensor_exit`) - right after the physics step
    /// they came from.
    pub(crate) fn sensor_events(&mut self, events: &[crate::physics::SensorEvent], cameras: &mut SceneCameras, app: &mut App, changes: &mut SceneChanges) {
        for event in events {
            if !self.nodes.get(&event.sensor).is_some_and(Node::has_behaviors) {
                continue;
            }
            let Some(mut node) = self.nodes.remove(&event.sensor) else { continue };
            node.run_sensor_event(event, &mut SceneAccess::new(self, cameras, changes), app);
            self.nodes.insert(event.sensor.clone(), node);
        }
    }

    /// Runs every node's attached behaviors' `on_origin_shift` - right after
    /// the floating origin moved the world by `shift` (see `WorldOrigin`).
    pub(crate) fn origin_shift(&mut self, cameras: &mut SceneCameras, app: &mut App, changes: &mut SceneChanges, shift: Vector3<f32>) {
        self.run_behaviors(cameras, app, changes, |node, scene, app| node.run_origin_shift(scene, app, shift));
    }

    /// Moves every node in the 3D world by `shift` - its `Transform3D`, and
    /// the fixed world points its `Line`s end at. Nodes on the 2D plane stay.
    /// A `Local` child moves with the rest (its offset from its parent
    /// stays), so it's right before the hierarchy is resolved again too.
    pub(crate) fn shift_origin(&mut self, shift: Vector3<f32>) {
        for node in self.nodes.values_mut() {
            if node.transform_2d().is_some() {
                continue;
            }
            let Some(transform) = node.transform_3d_mut() else { continue };
            transform.position += shift;
            if let Some(line) = node.get_property_mut::<crate::scene_manager::node::properties::Line>() {
                line.shift_world_points(shift);
            }
        }
    }
}

/// The id a child given to `parent` with `Node::with_child` is spawned
/// under: "left_wing" under "plane" is "plane/left_wing" (and a child of
/// that, "plane/left_wing/light"). Set once, when it spawns - a node given
/// a new parent later keeps its id.
pub fn child_id(parent: &str, name: &str) -> String {
    format!("{parent}/{name}")
}

/// The rotation that points +Z along `direction` with +Y as near up as it
/// gets - `None` for no direction at all.
pub fn facing(direction: Vector3<f32>) -> Option<UnitQuaternion<f32>> {
    let direction = direction.try_normalize(1e-6)?;
    // Straight up or down: any heading will do, Z is as good as any.
    let up = if direction.y.abs() > 0.999 { Vector3::z() } else { Vector3::y() };
    Some(UnitQuaternion::face_towards(&direction, &up))
}
