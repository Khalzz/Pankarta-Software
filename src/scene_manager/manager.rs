//! The scene manager - `app.scene_manager`: registers scenes, prepares them
//! ahead, opens them, and holds the active one (the switching itself is in
//! `loading/switch.rs`).

use std::collections::HashMap;
use std::rc::Rc;

use crate::app::App;
use crate::rendering::camera::handler::SceneCameras;
use crate::scene_manager::loading::thread::PendingSceneLoad;
use crate::scene_manager::scene::{Scene, SceneBehaviour, SceneContent};

pub(crate) type Factory = Rc<dyn Fn(&mut App) -> Scene>;

/// The name `active` has for a scene opened with `open` (no registered name).
pub(crate) const UNNAMED_SCENE: &str = "(unnamed)";

pub(crate) enum OpenRequest {
    Named(String),
    Factory(Factory),
}

pub struct SceneManager {
    factories: HashMap<String, Factory>,
    pub(crate) active_scene: Option<Scene>,
    /// The name of the active scene.
    pub active: String,
    pub(crate) requested: Option<OpenRequest>,
    pub(crate) preload_requests: Vec<String>,
    /// Scenes built with `preload`, waiting to be opened.
    pub(crate) waiting: HashMap<String, PendingSceneLoad>,
    /// The scene being opened, still loading - the active one stays on
    /// screen, frozen, until it's ready.
    pub(crate) switching: Option<(String, PendingSceneLoad)>,
}

fn factory<B: SceneBehaviour + 'static>(build: impl Fn(&mut Scene, &mut App) -> B + 'static) -> Factory {
    Rc::new(move |app: &mut App| {
        let mut scene = Scene::new(app);
        let behaviour = build(&mut scene, app);
        scene.set_scene_behaviour(behaviour)
    })
}

impl SceneManager {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
            active_scene: None,
            active: String::new(),
            requested: None,
            preload_requests: Vec::new(),
            waiting: HashMap::new(),
            switching: None,
        }
    }

    /// Registers a scene under `name` - nothing is built until it's opened
    /// or preloaded.
    pub fn create_scene<B: SceneBehaviour + 'static>(
        &mut self,
        name: impl Into<String>,
        build: impl Fn(&mut Scene, &mut App) -> B + 'static,
    ) {
        self.factories.insert(name.into(), factory(build));
    }

    /// Makes `name` the active scene, at the start of the next frame. If it
    /// was preloaded and is ready, it starts instantly; otherwise the current
    /// scene stays on screen, frozen, until it's loaded.
    pub fn open_scene(&mut self, name: &str) {
        if self.factories.contains_key(name) {
            self.requested = Some(OpenRequest::Named(name.to_owned()));
        } else {
            eprintln!("SceneManager::open_scene: no scene registered named '{name}'");
        }
    }

    /// Opens a scene built by `build`, without registering it - how a scene
    /// gets parameters: `open(|scene, app| Loading::new(scene, app, "level2"))`.
    pub fn open<B: SceneBehaviour + 'static>(&mut self, build: impl Fn(&mut Scene, &mut App) -> B + 'static) {
        self.requested = Some(OpenRequest::Factory(factory(build)));
    }

    /// Builds `name` and loads its assets in the background, while the
    /// current scene keeps running. It waits, not active, until
    /// `open_scene(name)`. Asking again while it waits does nothing.
    pub fn preload(&mut self, name: &str) {
        if !self.factories.contains_key(name) {
            eprintln!("SceneManager::preload: no scene registered named '{name}'");
            return;
        }
        let already = self.waiting.contains_key(name)
            || self.preload_requests.iter().any(|requested| requested == name)
            || self.switching.as_ref().is_some_and(|(opening, _)| opening == name);
        if !already {
            self.preload_requests.push(name.to_owned());
        }
    }

    /// Drops a preloaded scene (its assets go when the next scene opens).
    pub fn cancel_preload(&mut self, name: &str) {
        self.waiting.remove(name);
        self.preload_requests.retain(|requested| requested != name);
    }

    /// How far `name`'s loading is, 0.0 to 1.0 - 1.0 once it's ready, or
    /// once it's the active scene.
    pub fn load_progress(&self, name: &str) -> f32 {
        if let Some(load) = self.waiting.get(name) {
            return load.progress();
        }
        if let Some((_, load)) = self.switching.as_ref().filter(|(opening, _)| opening == name) {
            return load.progress();
        }
        if self.active == name && self.active_scene.is_some() {
            return 1.0;
        }
        0.0
    }

    /// Whether `name` was preloaded and everything it needs is loaded -
    /// `open_scene(name)` then starts it instantly.
    pub fn is_ready(&self, name: &str) -> bool {
        self.waiting.get(name).is_some_and(|load| load.is_ready())
    }

    /// A scene is being opened and is still loading - the active scene is
    /// frozen meanwhile.
    pub(crate) fn is_switching(&self) -> bool {
        self.switching.is_some()
    }

    pub(crate) fn factory_for(&self, name: &str) -> Option<Factory> {
        self.factories.get(name).cloned()
    }

    pub(crate) fn content(&self) -> Option<&SceneContent> {
        self.active_scene.as_ref().map(|scene| &scene.content)
    }

    pub(crate) fn cameras(&self) -> Option<&SceneCameras> {
        self.active_scene.as_ref().map(|scene| &scene.cameras)
    }

    pub(crate) fn cameras_mut(&mut self) -> Option<&mut SceneCameras> {
        self.active_scene.as_mut().map(|scene| &mut scene.cameras)
    }

    /// Every built scene that isn't active - so the window's size reaches
    /// their cameras too.
    pub(crate) fn waiting_scenes_mut(&mut self) -> impl Iterator<Item = &mut Scene> {
        self.waiting.values_mut().chain(self.switching.as_mut().map(|(_, load)| load)).map(|load| &mut load.scene)
    }
}
