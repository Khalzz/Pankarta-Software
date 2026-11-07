//! Building scenes ahead (`preload`) and switching the active one
//! (`open_scene` / `open`). Run by `App::run` at the start of a frame.
//!
//! These take the whole `App` rather than `&mut self` - they reach the
//! renderer, UI, assets and physics too, and the scene manager is inside
//! `App`, so it can't be borrowed apart from it.

use std::time::Instant;

use crate::app::App;
use crate::asset_manager::models;
use crate::rendering::enviroment::environment::Environment;
use crate::scene_manager::manager::{Factory, OpenRequest, SceneManager, UNNAMED_SCENE};

use super::assets::{LoadedAssets, UsedAssets};
use super::settings::{GlobalSettings, SettingsChange};
use super::thread::{start_loading, LoadList, PendingSceneLoad, SceneFrame};

impl SceneManager {
    /// Builds a scene - without opening it - and starts loading what it
    /// still needs. The running scene isn't touched: what the factory changed
    /// in the engine-wide settings is put back, to be applied when it opens.
    pub(crate) fn prepare(app: &mut App, name: &str, factory: Factory) -> PendingSceneLoad {
        let before = GlobalSettings::capture(app);
        // The factory sets its frame plan and line/text sets up from scratch,
        // apart from the running scene's.
        let running_sets = std::mem::take(&mut app.renderer.render_sets);
        let running_plan = std::mem::replace(&mut app.renderer.frame_plan, app.renderer.default_frame_plan.clone());
        let scene = factory(app);
        let frame = SceneFrame {
            render_sets: std::mem::replace(&mut app.renderer.render_sets, running_sets),
            plan: std::mem::replace(&mut app.renderer.frame_plan, running_plan),
        };
        let after = GlobalSettings::capture(app);
        before.restore(app);
        let settings = SettingsChange::between(&before, &after);

        let assets = scene.assets_to_load(app);
        let image_paths = scene.ui_image_paths();
        let images = app.assets.images.missing_from(&image_paths);
        let textures = app.assets.textures.missing_from(&assets.textures);
        // A skybox reads its textures from disk, so it loads with the rest.
        // A color or the procedural sky is built on the spot when it opens.
        let environment = settings.environment()
            .filter(|environment| matches!(environment, Environment::Skybox(_)) && **environment != app.renderer.prepared_from)
            .cloned();
        let uses = UsedAssets { models: assets.keys.clone(), images: image_paths, textures: assets.textures.clone() };
        let list = LoadList { models: assets.models, trimeshes: assets.trimeshes, images, textures, environment };

        if list.len() == 0 {
            PendingSceneLoad::ready(scene, settings, uses, frame)
        } else {
            start_loading(app, name, scene, settings, uses, frame, list)
        }
    }

    /// The open `open_scene`/`open` asked for. A scene preloaded under that
    /// name is used as it is; otherwise it's built now. Ready: it starts
    /// right away. Not yet: the current scene stays on screen, frozen, until
    /// it is (see `update_loads`).
    pub(crate) fn apply_open(app: &mut App) {
        let Some(request) = app.scene_manager.requested.take() else { return };
        // A scene opened before this one, still loading, is dropped.
        app.scene_manager.switching = None;

        let (name, load) = match request {
            OpenRequest::Named(name) => {
                app.scene_manager.preload_requests.retain(|requested| *requested != name);
                match app.scene_manager.waiting.remove(&name) {
                    Some(load) => (name, load),
                    None => {
                        let Some(factory) = app.scene_manager.factory_for(&name) else {
                            eprintln!("No scene registered named '{name}'");
                            return;
                        };
                        let load = Self::prepare(app, &name, factory);
                        (name, load)
                    }
                }
            }
            OpenRequest::Factory(factory) => {
                let load = Self::prepare(app, UNNAMED_SCENE, factory);
                (UNNAMED_SCENE.to_owned(), load)
            }
        };

        if load.is_ready() {
            Self::switch_to(app, name, load);
        } else {
            app.scene_manager.switching = Some((name, load));
        }
    }

    /// Every frame: builds the scenes `preload` asked for, checks every
    /// loading thread, and switches to the scene being opened once it's ready.
    pub(crate) fn update_loads(app: &mut App) {
        for name in std::mem::take(&mut app.scene_manager.preload_requests) {
            if app.scene_manager.waiting.contains_key(&name) {
                continue;
            }
            let Some(factory) = app.scene_manager.factory_for(&name) else { continue };
            let load = Self::prepare(app, &name, factory);
            app.scene_manager.waiting.insert(name, load);
        }

        for load in app.scene_manager.waiting.values_mut() {
            load.poll();
        }
        if let Some((_, load)) = app.scene_manager.switching.as_mut() {
            load.poll();
        }

        if app.scene_manager.switching.as_ref().is_some_and(|(_, load)| load.is_ready()) {
            let (name, load) = app.scene_manager.switching.take().expect("checked just above");
            Self::switch_to(app, name, load);
        }
    }

    /// The definitive change: the old scene goes - its nodes, UI, physics,
    /// frame plan, and the assets neither the new scene nor a waiting one
    /// uses - and the new one starts.
    fn switch_to(app: &mut App, name: String, mut load: PendingSceneLoad) {
        let loaded = load.ready.take().unwrap_or_else(LoadedAssets::nothing);

        app.scene_manager.active_scene = None;
        app.ui.renderizable_elements.clear();
        app.ui.always_on_top.clear();
        app.ui.overlay.clear();
        app.ui.has_changed = true;
        app.physics.stop();
        app.origin.reset();
        app.renderer.render_sets = load.frame.render_sets;
        app.renderer.frame_plan = load.frame.plan;
        app.scene_manager.active = name;

        let mut keep = load.uses.clone();
        for waiting in app.scene_manager.waiting.values() {
            keep.extend(&waiting.uses);
        }
        keep.images.extend(app.ui.image_paths());
        models::unload_level_models(app, &keep.models);
        app.assets.images.keep_only(&keep.images);
        app.assets.textures.keep_only(&keep.textures);

        load.settings.apply(app);
        Self::start_scene(app, load.scene, loaded);
    }

    /// Starts a built scene with its assets loaded: its nodes go on the
    /// renderer and physics, the `on_spawn`s run, its physics starts, and it
    /// becomes the active scene.
    fn start_scene(app: &mut App, mut scene: crate::scene_manager::scene::Scene, loaded: LoadedAssets) {
        let started = Instant::now();
        models::adopt_level_models(app, loaded.models);
        app.assets.images.adopt(loaded.images);
        app.assets.textures.adopt(loaded.textures);
        if let Some((environment, prepared)) = loaded.environment {
            app.renderer.prepared_environment = prepared;
            app.renderer.prepared_from = environment;
        }
        scene.finish_loading(app, &loaded.trimeshes);
        scene.run_on_spawn(app, &loaded.trimeshes);
        println!("[load {}] starting the scene: {:.0} ms", app.scene_manager.active, started.elapsed().as_secs_f64() * 1000.0);

        scene.ensure_camera(app);
        // Re-baseline the frame clock, so the first delta_time the scene sees
        // doesn't include however long it took to set up.
        app.time.update();

        scene.start_physics(app);
        app.scene_manager.active_scene = Some(scene);
    }
}
