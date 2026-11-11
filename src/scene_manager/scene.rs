use std::collections::{HashMap, HashSet};

use sdl2::EventPump;

use crate::app::{App, AppState};
use crate::physics::physics_resources::PhysicsObjectDef;
use crate::physics::Physics;
use crate::rendering::camera::handler::{LookAtTarget, SceneCameras, DEFAULT_CAMERA};
use crate::rendering::instance_management::InstanceData;
use crate::scene_manager::node::properties::{
    CameraLens, Model as NodeModelProperty, Parent, Space, SpawnedLive, Transform2D,
    Transform3D,
};
use crate::scene_manager::node::Node;
use crate::scene_manager::loading::assets::SceneAssets;
use crate::scene_manager::physics_bridge::{self, TrimeshCache};
use crate::scene_manager::render_bridge;
use crate::scene_manager::scene_access::{SceneAccess, SceneChanges};
use crate::scene_manager::scene_nodes::{child_id, SceneNodes};

/// Everything a scene might need out of a single frame, bundled so `SceneBehaviour::update`
/// keeps one stable signature no matter which of these fields a given scene actually
/// uses. Input state isn't here - it's `app.input` (which also owns controller
/// connect/disconnect - see Input).
pub struct FrameContext<'a> {
    pub app_state: &'a mut AppState,
    pub event_pump: &'a mut EventPump,
}

/// Everything that belongs to a given scene - nodes (the code-first entity
/// system) and the renderable instances that scene put up (spawned via
/// `Scene::spawn_node`). Cleared as one unit on
/// every scene switch (see `App::run`'s reset handling), rather than each
/// piece needing its own separately-remembered clear call - a new
/// scene-scoped system, added later, gets this for free just by becoming a
/// field here, instead of needing a new line added to the reset block by
/// hand (which is exactly how `renderizable_instances` ended up never being
/// cleared for `create_scene`-registered scenes until this was noticed - see
/// the design discussion this came out of).
#[derive(Default)]
pub struct SceneContent {
    pub nodes: SceneNodes,
    pub(crate) renderizable_instances: HashMap<String, InstanceData>,
    // Every node spawned with a Physics property (see
    // engine::scene_manager::physics_bridge) while the scene is built - made
    // into bodies when its physics starts (see Scene::start_physics). One
    // spawned after that gets its body right away.
    pub(crate) physics_bodies: Vec<PhysicsObjectDef>,
    // Nodes something else is moving right now (an animation) - the
    // physics' pose isn't written onto them meanwhile (see App::run). Whoever
    // puts a node in here also holds its body still
    // (PhysicsWorld::set_kinematic) and lets it go again.
    pub(crate) kinematic: HashSet<String>,
}

impl SceneContent {
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.renderizable_instances.clear();
        self.physics_bodies.clear();
        self.kinematic.clear();
    }
}

/// The custom logic driving one `Scene` - what used to be a scene's own
/// `impl Scene for GameLogic` (e.g. `main_menu::scene::GameLogic`,
/// `play::scene::GameLogic`). Mirrors `Behavior` (see that trait's own doc
/// comment) - `on_spawn`/`update` both get `&mut Scene` instead
/// of `&mut Node`, so a scene's logic can reach its own `content` (spawn
/// nodes, read/write renderable instances) the same way a node's behavior
/// reaches its own properties.
pub trait SceneBehaviour {
    fn on_spawn(&mut self, scene: &mut Scene, app: &mut App) {
        let _ = (scene, app);
    }
    fn update(&mut self, scene: &mut Scene, app: &mut App, ctx: &mut FrameContext) {
        let _ = (scene, app, ctx);
    }
    /// Every physics step, after every node's `Behavior::fixed_update` and
    /// right before the world advances - see that hook.
    fn fixed_update(&mut self, scene: &mut Scene, app: &mut App) {
        let _ = (scene, app);
    }
    /// After every node's behaviors have run this frame, before the
    /// hierarchy is resolved and the cameras placed - where an animation
    /// goes, so it has the last word on what it moves.
    fn late_update(&mut self, scene: &mut Scene, app: &mut App, ctx: &mut FrameContext) {
        let _ = (scene, app, ctx);
    }
    /// After this frame's cameras and renderable transforms are the ones
    /// that will be drawn - for anything placed relative to the camera.
    fn after_cameras(&mut self, scene: &mut Scene, app: &mut App) {
        let _ = (scene, app);
    }
    /// The game's debug windows for this scene - egui, drawn over everything
    /// while the debug UI is up (see `DebugUi`). Runs every frame it's up,
    /// after the scene's update.
    fn debug_ui(&mut self, ctx: &egui::Context, scene: &mut Scene, app: &mut App) {
        let _ = (ctx, scene, app);
    }
    /// The floating origin moved the whole world (see `WorldOrigin`,
    /// `app.origin`): what was at local `p` is at `p + shift` now - after
    /// every node's own `Behavior::on_origin_shift`. Move the world
    /// positions this scene keeps in its own fields the same way.
    fn on_origin_shift(&mut self, scene: &mut Scene, app: &mut App, shift: nalgebra::Vector3<f32>) {
        let _ = (scene, app, shift);
    }
}

/// A "screen" the game can be in (main menu, a level, ...) - the generic
/// container every scene shares: whatever nodes/renderable instances it's
/// spawned (`content`), whatever cameras it's registered (`cameras`), what it
/// sets differently from the project (`settings`), plus the `SceneBehaviour`
/// actually driving it. `SceneManager::active_scene` holds whichever one is
/// currently active; `App::run`'s loop drives it.
///
/// A scene is built first - its constructor (see `SceneManager::create_scene`)
/// spawns its nodes, which only note which models and bodies they need -
/// then those load in the background, and only when it opens does it start:
/// its nodes' `on_spawn`, its own, then its updates.
pub struct Scene {
    pub content: SceneContent,
    pub cameras: SceneCameras,
    behaviour: Option<Box<dyn SceneBehaviour>>,
    // Set once the scene's been built (its behaviour's on_spawn has run) -
    // nodes spawned from then on are marked SpawnedLive, and put on the
    // renderer and physics right away instead of waiting in the lists below.
    started: bool,
    /// Nodes spawned while the scene was being built, with a model or a
    /// physics body still to set up - once the scene's assets have loaded
    /// (see `finish_loading`).
    pending_models: Vec<String>,
    pending_bodies: Vec<String>,
    /// Nodes spawned while it was being built, whose behaviors' `on_spawn`
    /// runs when the scene starts.
    pending_spawns: Vec<String>,
    /// Models named with `will_use` - loaded with the scene's own.
    extra_models: Vec<NodeModelProperty>,
}

impl Scene {
    // Takes &App (not width/height directly) so a fresh scene's cameras
    // start at the real current screen size (see CameraResources::width/
    // height) without every caller needing to know where that comes from.
    pub fn new(app: &App) -> Self {
        Self {
            content: SceneContent::default(),
            cameras: SceneCameras::new(
                app.renderer.camera_resources.width(),
                app.renderer.camera_resources.height(),
            ),
            behaviour: None,
            started: false,
            pending_models: Vec::new(),
            pending_bodies: Vec::new(),
            pending_spawns: Vec::new(),
            extra_models: Vec::new(),
        }
    }

    pub fn set_scene_behaviour(mut self, behaviour: impl SceneBehaviour + 'static) -> Self {
        self.behaviour = Some(Box::new(behaviour));
        self
    }

    pub fn spawn_node(&mut self, app: &mut App, node: Node) -> Result<(), String> {
        self.spawn_tree(app, node)
    }

    fn spawn_tree(&mut self, app: &mut App, mut node: Node) -> Result<(), String> {
        let children = node.take_children();
        let id = node.id.clone();
        let changes = self.spawn_one(app, node)?;
        for mut child in children {
            let (space, inherit_rotation) = child.placement();
            let parent_is_2d = self
                .content
                .nodes
                .get(&id)
                .is_some_and(|parent| parent.transform_2d().is_some());
            if parent_is_2d && child.transform_3d().is_none() && child.transform_2d().is_none() {
                child.set_property(Transform2D::default());
            }
            let given = child.transform_3d().copied().unwrap_or_default();
            let given_2d = child.transform_2d().copied().unwrap_or_default();
            let (local, local_2d) = match space {
                Space::Local => (given, given_2d),
                Space::Global => Default::default(),
            };
            child.set_property(Parent {
                id: id.clone(),
                space,
                local,
                local_2d,
                inherit_rotation,
            });
            child.id = child_id(&id, &child.id);
            self.spawn_tree(app, child)?;
        }
        self.apply_changes(app, changes);
        Ok(())
    }

    pub(crate) fn apply_changes(&mut self, app: &mut App, changes: SceneChanges) {
        for id in changes.despawns {
            self.despawn_node(app, &id);
        }
        for node in changes.spawns {
            let id = node.id.clone();
            if let Err(error) = self.spawn_tree(app, node) {
                eprintln!("couldn't spawn node '{id}': {error}");
            }
        }
    }

    fn spawn_one(&mut self, app: &mut App, mut node: Node) -> Result<SceneChanges, String> {
        let id = node.id.clone();
        if let Some(parent) = node
            .get_property::<Parent>()
            .filter(|parent| parent.space == Space::Local)
            .cloned()
        {
            if node.transform_2d().is_some() {
                if let Some(parent_2d) = self
                    .content
                    .nodes
                    .get(&parent.id)
                    .and_then(|parent| parent.transform_2d())
                {
                    node.set_property(parent_2d.compose(&parent.local_2d, parent.inherit_rotation));
                }
            } else if let Some(parent_world) = self.content.nodes.world_transform(&parent.id) {
                node.set_property(parent_world.compose(&parent.local, parent.inherit_rotation));
            }
        }
        if self.started {
            node.set_property(SpawnedLive);
        }
        let mut changes = SceneChanges::default();
        if self.started {
            self.content
                .nodes
                .spawn(node, &mut self.cameras, app, &mut changes)?;
        } else {
            self.content.nodes.insert(node)?;
            self.pending_spawns.push(id.clone());
        }

        let Some(node) = self.content.nodes.get(&id) else {
            return Ok(changes);
        };
        let has_model = render_bridge::has_drawable(node);
        let has_physics = node.get_property::<Physics>().is_some();
        let lens = node.get_property::<CameraLens>().copied();
        let transform = node.transform_3d().copied().unwrap_or_default();
        if node.get_property::<crate::scene_manager::node::properties::Material>().is_some() {
            eprintln!("node '{id}': a Material on the node itself isn't used - give it to its Mesh or Model (`.material(..)`)");
        }

        if has_model {
            if self.started {
                render_bridge::register_static_model(self, app, &id)?;
            } else {
                self.pending_models.push(id.clone());
            }
        }

        if has_physics {
            if self.started {
                physics_bridge::register_physics_body(self, &id, &TrimeshCache::new())?;
            } else {
                self.pending_bodies.push(id.clone());
            }
        }

        if let Some(lens) = lens {
            self.cameras.place(
                &id,
                transform.position,
                transform.rotation,
                &lens,
            );
        }

        if self.started && app.physics.is_running() {
            if let Some(index) = self
                .content
                .physics_bodies
                .iter()
                .rposition(|def| def.id == id)
            {
                app.physics
                    .add_body(self.content.physics_bodies.remove(index));
            }
        }
        Ok(changes)
    }

    pub fn despawn_node(&mut self, app: &mut App, id: &str) {
        if self.content.nodes.get(id).is_none() {
            return;
        }
        let mut ids = vec![id.to_owned()];
        ids.extend(self.content.nodes.descendants_of(id));
        for id in ids {
            if self.content.nodes.remove(&id).is_none() {
                continue;
            }
            render_bridge::unregister_model(self, app, &id);
            self.pending_models.retain(|pending| *pending != id);
            self.pending_bodies.retain(|pending| *pending != id);
            self.pending_spawns.retain(|pending| *pending != id);
            self.cameras.remove_camera(&id);
            self.content.kinematic.remove(&id);
            self.content.physics_bodies.retain(|def| def.id != id);
            app.physics.remove_body(&id);
        }
    }

    /// Has a model or mesh nothing in the scene uses yet loaded with it -
    /// for one a node will be given later.
    pub fn will_use(&mut self, model: impl Into<NodeModelProperty>) {
        self.extra_models.push(model.into());
    }

    pub(crate) fn assets_to_load(&self, app: &App) -> SceneAssets {
        let mut assets = SceneAssets {
            models: Vec::new(),
            keys: HashSet::new(),
            trimeshes: Vec::new(),
            textures: Vec::new(),
        };
        let mut loads = Vec::new();
        let mut materials = Vec::new();
        let mut add = |draws: Vec<render_bridge::Draw>, report: &str| {
            for draw in draws {
                assets.keys.insert(draw.model_ref());
                match app.assets.model_load(&draw.source, &draw.double_sided) {
                    Ok(load) => loads.push(load),
                    // A node's is reported when its node is set up.
                    Err(error) if !report.is_empty() => eprintln!("{report}: {error}"),
                    Err(_) => {}
                }
                if let Some(material) = draw.material {
                    materials.push(material.into_owned());
                }
            }
        };
        for id in &self.pending_models {
            if let Some(node) = self.content.nodes.get(id) {
                add(render_bridge::draws(node), "");
            }
        }
        for model in &self.extra_models {
            add(render_bridge::model_draws("will_use", model), "will_use");
        }
        let mut queued = HashSet::new();
        for load in loads {
            assets.keys.insert(load.key.clone());
            if !app.assets.models.is_loaded(&load.key) && queued.insert(load.key.clone()) {
                assets.models.push(load);
            }
        }
        for material in &materials {
            for texture in material.textures() {
                if !assets.textures.contains(texture) {
                    assets.textures.push(texture.clone());
                }
            }
        }
        for id in &self.pending_bodies {
            let Some(physics) = self
                .content
                .nodes
                .get(id)
                .and_then(|node| node.get_property::<Physics>())
            else {
                continue;
            };
            for path in physics_bridge::trimesh_paths(physics) {
                if !assets.trimeshes.iter().any(|known| known == path) {
                    assets.trimeshes.push(path.to_owned());
                }
            }
        }
        assets
    }

    pub(crate) fn finish_loading(&mut self, app: &mut App, trimeshes: &TrimeshCache) {
        for id in std::mem::take(&mut self.pending_models) {
            if let Err(error) = render_bridge::register_static_model(self, app, &id) {
                eprintln!("node '{id}': {error}");
            }
        }
        for id in std::mem::take(&mut self.pending_bodies) {
            if let Err(error) = physics_bridge::register_physics_body(self, &id, trimeshes) {
                eprintln!("node '{id}': {error}");
            }
        }
        self.extra_models.clear();
    }

    /// Every image path its nodes' `UiLayer`s draw - loaded with its other
    /// assets (see `SceneManager::apply_open`).
    pub(crate) fn ui_image_paths(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for layer in self.content.nodes.iter().filter_map(|node| node.get_property::<crate::scene_manager::node::properties::UiLayer>()) {
            layer.root.collect_image_paths(&mut paths);
        }
        paths
    }

    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.content.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.content.nodes.get_mut(id)
    }

    pub fn child(&self, parent: &str, name: &str) -> Option<&Node> {
        self.content.nodes.get(&child_id(parent, name))
    }

    pub fn child_mut(&mut self, parent: &str, name: &str) -> Option<&mut Node> {
        self.content.nodes.get_mut(&child_id(parent, name))
    }

    pub fn world_transform(&self, id: &str) -> Option<Transform3D> {
        self.content.nodes.world_transform(id)
    }

    pub fn set_world_transform(&mut self, id: &str, world: Transform3D) {
        let parent = self
            .content
            .nodes
            .get(id)
            .and_then(|node| node.get_property::<Parent>())
            .cloned();
        if let Some(mut parent) = parent.filter(|parent| parent.space == Space::Local) {
            if let Some(parent_world) = self.content.nodes.world_transform(&parent.id) {
                parent.local = parent_world.relative(&world, parent.inherit_rotation);
                if let Some(node) = self.content.nodes.get_mut(id) {
                    node.set_property(parent);
                }
            }
        }
        let Some(node) = self.content.nodes.get_mut(id) else {
            return;
        };
        node.set_property(world);
        for draw in render_bridge::draws(node) {
            if let Some(instance) = self.content.renderizable_instances.get_mut(&draw.id) {
                instance.transform = render_bridge::place(&world, &draw.local);
            }
        }
    }

    pub fn set_parent(&mut self, id: &str, parent: Option<(&str, Space, bool)>) -> bool {
        let Some(world) = self.content.nodes.world_transform(id) else {
            return false;
        };
        let new_parent = match parent {
            None => None,
            Some((parent_id, space, inherit_rotation)) => {
                if parent_id == id
                    || self
                        .content
                        .nodes
                        .descendants_of(id)
                        .iter()
                        .any(|child| child == parent_id)
                {
                    return false;
                }
                let Some(parent_world) = self.content.nodes.world_transform(parent_id) else {
                    return false;
                };
                let local = match space {
                    Space::Local => parent_world.relative(&world, inherit_rotation),
                    Space::Global => Transform3D::default(),
                };
                let on_plane = (
                    self.content
                        .nodes
                        .get(id)
                        .and_then(|node| node.transform_2d()),
                    self.content
                        .nodes
                        .get(parent_id)
                        .and_then(|node| node.transform_2d()),
                );
                let local_2d = match (space, on_plane) {
                    (Space::Local, (Some(world_2d), Some(parent_2d))) => {
                        parent_2d.relative(world_2d, inherit_rotation)
                    }
                    _ => Transform2D::default(),
                };
                Some(Parent {
                    id: parent_id.to_owned(),
                    space,
                    local,
                    local_2d,
                    inherit_rotation,
                })
            }
        };
        let Some(node) = self.content.nodes.get_mut(id) else {
            return false;
        };
        match new_parent {
            Some(parent) => node.set_property(parent),
            None => {
                node.remove_property::<Parent>();
            }
        }
        true
    }

    pub(crate) fn sync_render_instances(&mut self, app: &mut App) {
        render_bridge::sync_instances(self, app);
    }

    pub(crate) fn sync_cameras(&mut self) {
        for node in self.content.nodes.iter() {
            let Some(lens) = node.get_property::<CameraLens>() else {
                continue;
            };
            let transform = node.transform_3d().copied().unwrap_or_default();
            self.cameras.place(
                &node.id,
                transform.position,
                transform.rotation,
                &lens,
            );
        }
    }

    pub(crate) fn ensure_camera(&mut self, app: &mut App) {
        if self.cameras.has_active_camera() {
            return;
        }
        let camera = Node::new(DEFAULT_CAMERA)
            .with_property(Transform3D::default())
            .with_property(CameraLens::default());
        if let Err(error) = self.spawn_node(app, camera) {
            eprintln!("couldn't give the scene its default camera: {error}");
        }
    }

    pub fn select_camera(&mut self, name: &str) -> bool {
        self.cameras.select_camera(name)
    }

    pub fn set_camera_look_at(&mut self, name: &str, target: Option<LookAtTarget>) -> bool {
        self.cameras.set_look_at(name, target)
    }

    /// The scene starts: its nodes' `on_spawn`, then its own - each with
    /// what they set up (a model, a body) registered right after.
    pub(crate) fn run_on_spawn(&mut self, app: &mut App, trimeshes: &TrimeshCache) {
        self.run_node_spawns(app);
        self.finish_loading(app, trimeshes);

        if let Some(mut behaviour) = self.behaviour.take() {
            behaviour.on_spawn(self, app);
            self.behaviour = Some(behaviour);
        }

        self.run_node_spawns(app);
        self.finish_loading(app, trimeshes);
        self.started = true;
    }

    fn run_node_spawns(&mut self, app: &mut App) {
        while !self.pending_spawns.is_empty() {
            for id in std::mem::take(&mut self.pending_spawns) {
                let Some(mut node) = self.content.nodes.remove(&id) else { continue };
                let mut changes = SceneChanges::default();
                node.run_on_spawn(&mut SceneAccess::new(&mut self.content.nodes, &mut self.cameras, &mut changes), app);
                self.content.nodes.put_back(node);
                self.note_set_up_in_on_spawn(&id);
                self.apply_changes(app, changes);
            }
        }
    }

    /// A model, body or camera lens a behavior put on its node in `on_spawn`.
    fn note_set_up_in_on_spawn(&mut self, id: &str) {
        let Some(node) = self.content.nodes.get(id) else { return };
        let has_model = render_bridge::has_drawable(node);
        let has_physics = node.get_property::<Physics>().is_some();
        let lens = node.get_property::<CameraLens>().copied();
        let transform = node.transform_3d().copied().unwrap_or_default();
        if has_model && !self.content.renderizable_instances.values().any(|instance| instance.node == id) && !self.pending_models.iter().any(|pending| pending == id) {
            self.pending_models.push(id.to_owned());
        }
        if has_physics && !self.content.physics_bodies.iter().any(|body| body.id == id) && !self.pending_bodies.iter().any(|pending| pending == id) {
            self.pending_bodies.push(id.to_owned());
        }
        if let Some(lens) = lens {
            self.cameras.place(id, transform.position, transform.rotation, &lens);
        }
    }

    pub(crate) fn run_update(&mut self, app: &mut App, ctx: &mut FrameContext) {
        if let Some(mut behaviour) = self.behaviour.take() {
            behaviour.update(self, app, ctx);
            self.behaviour = Some(behaviour);
        }
    }

    pub(crate) fn run_late_update(&mut self, app: &mut App, ctx: &mut FrameContext) {
        if let Some(mut behaviour) = self.behaviour.take() {
            behaviour.late_update(self, app, ctx);
            self.behaviour = Some(behaviour);
        }
    }

    pub(crate) fn run_after_cameras(&mut self, app: &mut App) {
        if let Some(mut behaviour) = self.behaviour.take() {
            behaviour.after_cameras(self, app);
            self.behaviour = Some(behaviour);
        }
    }

    pub(crate) fn run_debug_ui(&mut self, app: &mut App, ctx: &egui::Context) {
        if let Some(mut behaviour) = self.behaviour.take() {
            behaviour.debug_ui(ctx, self, app);
            self.behaviour = Some(behaviour);
        }
    }

    pub(crate) fn start_physics(&mut self, app: &mut App) {
        app.physics
            .start(std::mem::take(&mut self.content.physics_bodies));
    }

    pub(crate) fn run_fixed_update(&mut self, app: &mut App) {
        if let Some(mut behaviour) = self.behaviour.take() {
            behaviour.fixed_update(self, app);
            self.behaviour = Some(behaviour);
        }
    }

    pub(crate) fn shift_origin(&mut self, shift: nalgebra::Vector3<f32>) {
        self.content.nodes.shift_origin(shift);
        for instance in self.content.renderizable_instances.values_mut() {
            instance.transform.position += shift;
        }
        for body in &mut self.content.physics_bodies {
            body.position += shift;
        }
        self.cameras.shift_origin(shift);
    }

    pub(crate) fn run_origin_shift(&mut self, app: &mut App, shift: nalgebra::Vector3<f32>) {
        let mut changes = SceneChanges::default();
        self.content
            .nodes
            .origin_shift(&mut self.cameras, app, &mut changes, shift);
        self.apply_changes(app, changes);
        if let Some(mut behaviour) = self.behaviour.take() {
            behaviour.on_origin_shift(self, app, shift);
            self.behaviour = Some(behaviour);
        }
    }
}
