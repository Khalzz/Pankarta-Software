use nalgebra::Vector2;
use pankarta_software::app::App;
use pankarta_software::input::source::{InputSource, Key};
use pankarta_software::scene_manager::behavior::Behavior;
use pankarta_software::scene_manager::node::Node;
use pankarta_software::scene_manager::node::properties::{Shape2D, Transform2D, UiLayer};
use pankarta_software::scene_manager::scene::SceneBehaviour;
use pankarta_software::scene_manager::scene_access::SceneAccess;
use pankarta_software::ui::TextStyle;
use pankarta_software::ui::color::UiColor;
use pankarta_software::ui::ui_node::UiNode;
use pankarta_software::ui::ui_transform::SizeValue;
use pankarta_software::window::WindowMode;
use pankarta_software::ui::widgets::text;


// Scene and behaviour
struct Test2DScene;

impl SceneBehaviour for Test2DScene {
  fn on_spawn(&mut self, scene: &mut pankarta_software::scene_manager::scene::Scene, app: &mut App) {
    let node_2d = Node::new("player")
      .with_behavior(Player::new(500.0))
      .with_property(Transform2D { position: Vector2::new(500.0, 500.0), rotation: 0.0, scale: Vector2::new(1.0, 1.0), z_index: 0 })
      .with_property(Shape2D::square(100.0).color(UiColor::Hex("#f63b3b")));

    scene.spawn_node(app, node_2d).unwrap();

    // The HUD is a node: its UI is its UiLayer, kept up by its behavior.
    let framerate_label = text("600 fps", &TextStyle::default())
        .set_padding((10.0, 10.0))
        .set_size(SizeValue::Pixels(200.0), SizeValue::Fit);
    let hud = Node::new("hud")
      .with_property(UiLayer::new(UiNode::container().set_child("framerate", framerate_label)))
      .with_behavior(FpsCounter);

    scene.spawn_node(app, hud).unwrap();
  }
}

// Shows the frame rate in its node's UI.
struct FpsCounter;

impl Behavior for FpsCounter {
  fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
    let fps = app.time.fps().round();
    if let Some(label) = node.get_property_mut::<UiLayer>().and_then(|ui| ui.label_mut("framerate")) {
      label.set_text(format!("{fps} fps"));
    }
  }
}

// Player node and behaviour
struct Player {
  speed: f32
}

impl Player {
  fn new(speed: f32) -> Player {
    Player { speed }
  }
}

impl Behavior for Player {
  fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
    let direction = app.input.get_vector("Left", "Right", "Up", "Down").normalize();

    if let Some(transform) = node.get_property_mut::<Transform2D>() {
      transform.position += direction * self.speed * app.time.delta_time;
    }
  }
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

    // window setup
    app.window.set_window_mode(WindowMode::Fullscreen).unwrap();
    app.renderer.set_vsync(false);
    app.window.set_title("2D Example");

    // Input binding setup
    app.input.bind("Left", InputSource::key(Key::A));
    app.input.bind("Right", InputSource::key(Key::D));
    app.input.bind("Up", InputSource::key(Key::W));
    app.input.bind("Down", InputSource::key(Key::S));

    // creating and openning the scene
    app.scene_manager.create_scene("main", |scene, app| {      
      Test2DScene
    });

    app.scene_manager.open_scene("main");

    // running the game
    app.run();
}
