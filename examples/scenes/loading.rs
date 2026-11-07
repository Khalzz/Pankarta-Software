//! Splash, menu, loading screen and levels - all plain scenes.
//!
//! - "splash" preloads "menu" while its logo shows, and opens it once it's
//!   ready (and the logo has had its time).
//! - The menu's buttons open a loading scene with the level as a parameter
//!   (`app.scene_manager.open(..)`), which preloads that level, shows its
//!   progress, and opens it by itself once it's loaded.
//! - Esc in a level goes back to the menu with a plain `open_scene` - no
//!   loading screen: the level stays on screen, frozen, until the menu's ready.
//!
//! cargo run --example scene_loading

use nalgebra::Vector3;
use pankarta_software::app::App;
use pankarta_software::input::source::{InputSource, Key};
use pankarta_software::prelude::{FrameContext, Node, Scene, SceneBehaviour, UiLayer};
use pankarta_software::rendering::enviroment::environment::Environment;
use pankarta_software::scene_manager::node::properties::{CameraLens, Light, LookAt, Model, Transform3D};
use pankarta_software::ui::color::UiColor;
use pankarta_software::ui::ui_node::UiNode;
use pankarta_software::ui::ui_transform::{PositionValue, SizeValue};
use pankarta_software::ui::widgets::{button, card, text, ButtonStyle, CardStyle};
use pankarta_software::ui::{TextStyle, Weight};

/// Opens `level` through the loading scene.
fn go_to(app: &mut App, level: &'static str) {
    app.scene_manager.open(move |scene, app| Loading::new(scene, app, level));
}

fn centered_text(scene: &mut Scene, app: &mut App, id: &str, content: &str, style: &TextStyle) {
    let label = text(content, style).set_position(PositionValue::Center(0.0), PositionValue::Center(0.0));
    scene.spawn_node(app, Node::new(id).with_property(UiLayer::new(label))).unwrap();
}

// Splash

struct Splash {
    shown_for: f32,
}

impl Splash {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::Color(UiColor::Rgb(0, 0, 0)));
        centered_text(scene, app, "logo", "PANKARTA", &TextStyle::default().size(48.0).weight(Weight::BOLD));
        Self { shown_for: 0.0 }
    }
}

impl SceneBehaviour for Splash {
    fn on_spawn(&mut self, _scene: &mut Scene, app: &mut App) {
        app.scene_manager.preload("menu");
    }

    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        self.shown_for += app.time.unscaled_delta_time;
        let alpha = (self.shown_for / 0.6).min(1.0);
        if let Some(logo) = scene.get_node_mut("logo").and_then(|node| node.get_property_mut::<UiLayer>()).and_then(|ui| ui.get_mut("")) {
            logo.set_alpha(alpha);
        }
        if self.shown_for >= 2.0 && app.scene_manager.is_ready("menu") {
            app.scene_manager.open_scene("menu");
        }
    }
}

// Menu

struct Menu;

impl Menu {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::Color(UiColor::Rgb(0, 89, 149)));
        let style = ButtonStyle::default().width(220.0);
        let menu = card(&CardStyle::default().gap(10.0))
            .set_position(PositionValue::Center(0.0), PositionValue::Center(0.0))
            .set_child("title", text("Pick a level", &TextStyle::default().size(20.0).weight(Weight::BOLD)))
            .set_child("hangar", button("Hangar", &style).on_click(|_node, _scene, app| go_to(app, "hangar")))
            .set_child("empty", button("Empty room", &style).on_click(|_node, _scene, app| go_to(app, "empty")));
        scene.spawn_node(app, Node::new("menu").with_property(UiLayer::new(menu))).unwrap();
        Menu
    }
}

impl SceneBehaviour for Menu {}

// Loading - opened with the level it loads as a parameter.

struct Loading {
    target: &'static str,
    shown_for: f32,
}

impl Loading {
    fn new(scene: &mut Scene, app: &mut App, target: &'static str) -> Self {
        app.scene_manager.preload(target);
        app.renderer.set_environment(Environment::Color(UiColor::Rgb(10, 10, 14)));

        let root = UiNode::container()
            .set_size(SizeValue::Percent(100.0), SizeValue::Percent(100.0))
            .set_child("title", text(&format!("Loading {target}"), &TextStyle::default().size(24.0).weight(Weight::BOLD))
                .set_position(PositionValue::Center(0.0), PositionValue::Center(-20.0)))
            .set_child("bar", text(&progress_bar(0.0), &TextStyle::default().size(16.0))
                .set_position(PositionValue::Center(0.0), PositionValue::Center(20.0)));
        scene.spawn_node(app, Node::new("loading").with_property(UiLayer::new(root))).unwrap();
        Self { target, shown_for: 0.0 }
    }
}

impl SceneBehaviour for Loading {
    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        self.shown_for += app.time.unscaled_delta_time;
        let progress = app.scene_manager.load_progress(self.target);

        if let Some(ui) = scene.get_node_mut("loading").and_then(|node| node.get_property_mut::<UiLayer>()) {
            if let Some(label) = ui.label_mut("bar") {
                label.set_text(progress_bar(progress));
            }
            let pulse = 0.6 + 0.4 * (self.shown_for * 4.0).sin().abs();
            if let Some(title) = ui.get_mut("title") {
                title.set_alpha(pulse);
            }
        }

        // Shown at least a moment, so it doesn't just flash by.
        if app.scene_manager.is_ready(self.target) && self.shown_for >= 1.5 {
            app.scene_manager.open_scene(self.target);
        }
    }
}

fn progress_bar(progress: f32) -> String {
    const WIDTH: usize = 20;
    let filled = (progress * WIDTH as f32).round() as usize;
    format!("[{}{}] {:>3.0}%", "#".repeat(filled), "-".repeat(WIDTH - filled), progress * 100.0)
}

// Levels

struct Level;

impl SceneBehaviour for Level {
    fn update(&mut self, _scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        if app.input.is_just_pressed(InputSource::key(Key::Escape)) {
            app.scene_manager.open_scene("menu");
        }
    }
}

fn hangar(scene: &mut Scene, app: &mut App) -> Level {
    app.renderer.set_environment(Environment::Color(UiColor::Rgb(40, 44, 52)));
    scene.spawn_node(app, Node::new("plane")
        .with_property(Model::file("models/f16.glb").transform(Transform3D { scale: Vector3::new(15.0, 15.0, 15.0), ..Default::default() }))
        .with_property(Transform3D::default())).unwrap();
    scene.spawn_node(app, Node::new("camera")
        .with_property(Transform3D { position: Vector3::new(18.0, 8.0, -18.0), ..Default::default() })
        .with_property(CameraLens::new(60.0))
        .with_property(LookAt { target: "plane".into() })).unwrap();
    scene.spawn_node(app, Node::new("sun")
        .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.2)))
        .with_property(Light::directional([1.0, 1.0, 1.0]))).unwrap();
    centered_text(scene, app, "hint", "Esc: back to the menu", &TextStyle::default());
    Level
}

fn empty(scene: &mut Scene, app: &mut App) -> Level {
    app.renderer.set_environment(Environment::Color(UiColor::Rgb(90, 30, 60)));
    centered_text(scene, app, "hint", "An empty room. Esc: back to the menu", &TextStyle::default());
    Level
}

#[tokio::main]
async fn main() {
    let mut app = match App::new().await {
        Ok(app) => app,
        Err(err) => {
            eprintln!("Couldn't create the app: {err}");
            return;
        }
    };
    app.window.set_title("Scene loading");

    app.scene_manager.create_scene("splash", Splash::new);
    app.scene_manager.create_scene("menu", Menu::new);
    app.scene_manager.create_scene("hangar", hangar);
    app.scene_manager.create_scene("empty", empty);
    app.scene_manager.open_scene("splash");

    app.run();
}
