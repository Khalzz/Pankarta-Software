//! The smallest engine project: a window with one empty scene.
//!
//! cargo run --example basic_window

use pankarta_software::app::App;
use pankarta_software::prelude::*;
use pankarta_software::ui::ui_node::UiNode;
use pankarta_software::ui::ui_transform::SizeValue;
use pankarta_software::ui::widgets::text;
use pankarta_software::ui::TextStyle;

/// A scene with nothing in it but a frame rate counter.
struct EmptyScene;

impl SceneBehaviour for EmptyScene {
    fn on_spawn(&mut self, scene: &mut Scene, app: &mut App) {
        // Wide enough for any frame rate - it's sized to its first text otherwise.
        let framerate_label = text("600 fps", &TextStyle::default())
            .set_padding((10.0, 10.0))
            .set_size(SizeValue::Pixels(200.0), SizeValue::Fit);

        // The counter is a node: its UI is its UiLayer, its behavior keeps
        // the text up to date.
        let counter = Node::new("framerate")
            .with_property(UiLayer::new(UiNode::container().set_child("label", framerate_label)))
            .with_behavior(FrameRate::default());
        if let Err(error) = scene.spawn_node(app, counter) {
            eprintln!("{error}");
        }
    }
}

/// Shows the frame rate in its node's UI.
#[derive(Default)]
struct FrameRate {
    // Frames and seconds since the counter last changed - it shows their
    // average, twice a second, instead of a number too jumpy to read.
    frames: u32,
    elapsed: f32,
}

impl Behavior for FrameRate {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.frames += 1;
        self.elapsed += app.time.unscaled_delta_time;
        if self.elapsed < 0.5 {
            return;
        }
        let fps = self.frames as f32 / self.elapsed;
        self.frames = 0;
        self.elapsed = 0.0;

        if let Some(label) = node.get_property_mut::<UiLayer>().and_then(|ui| ui.label_mut("label")) {
            label.set_text(format!("{fps:.0} fps"));
        }
    }
}

#[tokio::main]
async fn main() {
    // 800x600, windowed, resizable, "Pankarta Software" - hidden until run().
    let mut app = match App::new().await {
        Ok(app) => app,
        Err(err) => {
            eprintln!("Couldn't create the app: {err}");
            return;
        }
    };

    app.scene_manager.create_scene("main", |_scene, _app| EmptyScene);
    app.scene_manager.open_scene("main");

    app.run();
}
