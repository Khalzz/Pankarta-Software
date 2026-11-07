//! A built scene waiting to open, and the background thread loading what it
//! still needs.

use std::cell::{Cell, RefCell};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

use crate::app::App;
use crate::asset_manager::models::ModelLoad;
use crate::rendering::enviroment::environment::{prepare_environment, Environment};
use crate::rendering::frame::FramePlan;
use crate::rendering::models::model::Mesh;
use crate::rendering::render_sets::RenderSets;
use crate::scene_manager::physics_bridge;
use crate::scene_manager::scene::Scene;

use super::assets::{LoadedAssets, UsedAssets};
use super::settings::SettingsChange;

/// A scene that's built but not open yet: not drawn, not updated, nothing on
/// the GPU or in physics. `open_scene` makes it the active scene.
pub(crate) struct PendingSceneLoad {
    pub(crate) scene: Scene,
    /// What its factory changed in the engine-wide settings - applied when it opens.
    pub(crate) settings: SettingsChange,
    /// Everything it uses - kept loaded while it waits.
    pub(crate) uses: UsedAssets,
    /// The frame plan and line/text sets its factory set up - the renderer
    /// takes them when it opens.
    pub(crate) frame: SceneFrame,
    receiver: Option<Receiver<LoadedAssets>>,
    progress: Option<Receiver<usize>>,
    pub(crate) ready: Option<LoadedAssets>,
    done: usize,
    total: usize,
}

pub(crate) struct SceneFrame {
    pub(crate) plan: FramePlan,
    pub(crate) render_sets: RenderSets,
}

impl PendingSceneLoad {
    /// One with nothing left to load.
    pub(crate) fn ready(scene: Scene, settings: SettingsChange, uses: UsedAssets, frame: SceneFrame) -> Self {
        Self { scene, settings, uses, frame, receiver: None, progress: None, ready: Some(LoadedAssets::nothing()), done: 0, total: 0 }
    }

    /// Checks the loading thread, without waiting on it.
    pub(crate) fn poll(&mut self) {
        if let Some(progress) = &self.progress {
            while let Ok(done) = progress.try_recv() {
                self.done = done;
            }
        }
        if self.ready.is_none() {
            if let Some(Ok(loaded)) = self.receiver.as_ref().map(|receiver| receiver.try_recv()) {
                self.ready = Some(loaded);
                self.receiver = None;
                self.progress = None;
            }
        }
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.ready.is_some()
    }

    pub(crate) fn progress(&self) -> f32 {
        if self.ready.is_some() || self.total == 0 {
            1.0
        } else {
            (self.done as f32 / self.total as f32).min(0.99)
        }
    }
}

/// What the loading thread has to load for one scene.
pub(crate) struct LoadList {
    pub models: Vec<ModelLoad>,
    pub trimeshes: Vec<String>,
    pub images: Vec<String>,
    pub textures: Vec<String>,
    pub environment: Option<Environment>,
}

impl LoadList {
    pub(crate) fn len(&self) -> usize {
        self.models.len() + self.trimeshes.len() + self.images.len() + self.textures.len() + self.environment.is_some() as usize
    }
}

/// Starts loading `list` on a background thread. Device/Queue/layouts are
/// cheap Arc-backed clones, so the thread builds the GPU resources itself.
pub(crate) fn start_loading(app: &App, name: &str, scene: Scene, settings: SettingsChange, uses: UsedAssets, frame: SceneFrame, list: LoadList) -> PendingSceneLoad {
    let device = app.renderer.device.clone();
    let queue = app.renderer.queue.clone();
    let camera_bind_group_layout = app.renderer.camera_resources.bind_group_layout.clone();
    let image_layout = app.assets.images.layout().clone();
    let texture_layout = app.assets.textures.layout().clone();
    let config = app.renderer.config.clone();
    let total = list.len();

    let (tx, rx) = channel();
    let (progress_tx, progress_rx) = channel();
    let scene_name = name.to_owned();
    std::thread::spawn(move || {
        let reporter = LoadReporter::new(&scene_name, progress_tx);
        let mesh_layout = Mesh::create_bind_group_layout(&device);
        let models = list.models.into_iter()
            .map(|load| {
                let key = load.key.clone();
                reporter.step(format!("Model {key}"));
                let model = load.load(&device, &queue, &mesh_layout).map_err(|error| format!("failed to load model '{key}': {error}"));
                (key, model)
            })
            .collect();
        let trimeshes = list.trimeshes.into_iter()
            .map(|path| {
                reporter.step(format!("Collider {path}"));
                let mesh = physics_bridge::load_trimesh(&path);
                (path, mesh)
            })
            .collect();
        let images = list.images.into_iter()
            .map(|path| {
                reporter.step(format!("Image {path}"));
                let image = crate::asset_manager::load_image_file(&path, &device, &queue, &image_layout);
                (path, image)
            })
            .collect();
        let textures = list.textures.into_iter()
            .map(|path| {
                reporter.step(format!("Texture {path}"));
                let texture = crate::asset_manager::load_texture_file(&path, &device, &queue, &texture_layout);
                (path, texture)
            })
            .collect();
        let environment = list.environment.map(|environment| {
            reporter.step("Environment");
            let prepared = prepare_environment(&device, &queue, &camera_bind_group_layout, &config, environment.clone());
            (environment, prepared)
        });
        reporter.finish();
        let _ = tx.send(LoadedAssets { models, trimeshes, images, textures, environment });
    });

    PendingSceneLoad { scene, settings, uses, frame, receiver: Some(rx), progress: Some(progress_rx), ready: None, done: 0, total }
}

/// Counts the loading thread's steps (for `load_progress`) and logs how long
/// each took, slowest first, at the end.
struct LoadReporter {
    scene: String,
    progress: Sender<usize>,
    started: Instant,
    done: Cell<usize>,
    current: RefCell<Option<(String, Instant)>>,
    timings: RefCell<Vec<(String, Duration)>>,
}

impl LoadReporter {
    fn new(scene: &str, progress: Sender<usize>) -> Self {
        Self {
            scene: scene.to_owned(),
            progress,
            started: Instant::now(),
            done: Cell::new(0),
            current: RefCell::new(None),
            timings: RefCell::new(Vec::new()),
        }
    }

    fn step(&self, what: impl Into<String>) {
        self.end_current();
        *self.current.borrow_mut() = Some((what.into(), Instant::now()));
    }

    fn end_current(&self) {
        if let Some((what, started)) = self.current.borrow_mut().take() {
            let took = started.elapsed();
            println!("[load {}] {what}: {:.0} ms", self.scene, took.as_secs_f64() * 1000.0);
            self.timings.borrow_mut().push((what, took));
            self.done.set(self.done.get() + 1);
            let _ = self.progress.send(self.done.get());
        }
    }

    fn finish(&self) {
        self.end_current();
        let mut timings = self.timings.borrow().clone();
        timings.sort_by(|a, b| b.1.cmp(&a.1));
        println!("[load {}] background load done in {:.0} ms - slowest:", self.scene, self.started.elapsed().as_secs_f64() * 1000.0);
        for (what, took) in timings.iter().take(5) {
            println!("[load {}]   {:>7.0} ms  {what}", self.scene, took.as_secs_f64() * 1000.0);
        }
    }
}
