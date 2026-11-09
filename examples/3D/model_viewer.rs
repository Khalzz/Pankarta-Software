use nalgebra::{UnitQuaternion, Vector3};
use pankarta_software::app::App;
use pankarta_software::input::source::{InputSource, Key};
use pankarta_software::scene_manager::behavior::Behavior;
use pankarta_software::scene_manager::node::Node;
use pankarta_software::scene_manager::node::properties::{rotation_from_degrees, CameraLens, Light, LookAt, Model, Transform3D, UiLayer};

// Moves the F-16's control surfaces - meshes inside its file - from the
// keyboard: A/D roll (ailerons), W/S pitch (elevators), Q/E yaw (rudder),
// Space hides/shows the afterburner. Only this node's copy changes.
struct ControlSurfaces {
  // Furthest a surface turns (radians), and how fast it gets there.
  max_deflection: f32,
  speed: f32,
  roll: f32,
  pitch: f32,
  yaw: f32,
  listed: bool,
}

impl ControlSurfaces {
  // Eases `current` toward `target` at `speed` per second.
  fn approach(current: f32, target: f32, step: f32) -> f32 {
    current + (target - current).clamp(-step, step)
  }
}

impl Behavior for ControlSurfaces {
  fn update(&mut self, node: &mut Node, _scene: &mut pankarta_software::scene_manager::scene_access::SceneAccess, app: &mut App) {
    let step = self.speed * app.time.delta_time;
    let axis = |negative: &str, positive: &str| app.input.is_action_pressed(positive) as i32 as f32 - app.input.is_action_pressed(negative) as i32 as f32;
    self.roll = Self::approach(self.roll, axis("Left", "Right") * self.max_deflection, step);
    self.pitch = Self::approach(self.pitch, axis("Down", "Up") * self.max_deflection, step);
    self.yaw = Self::approach(self.yaw, axis("YawLeft", "YawRight") * self.max_deflection, step);
    let toggle_afterburner = app.input.is_action_just_pressed("Afterburner");

    let Some(model) = node.get_property_mut::<Model>() else { return };

    // Reading: every mesh in the file, once it's loaded.
    if !self.listed && model.meshes().next().is_some() {
      for mesh in model.meshes() {
        println!("mesh \"{}\" at {:?}", mesh.name(), mesh.transform.position);
      }
      self.listed = true;
    }

    // Writing: turn a mesh away from where the file put it, about its own axes.
    let turn = |angle: f32, axis| UnitQuaternion::from_axis_angle(&axis, angle);
    if let Some(mesh) = model.mesh_mut("left_aleron") { mesh.rotate_from_original(turn(self.roll, Vector3::x_axis())); }
    if let Some(mesh) = model.mesh_mut("right_aleron") { mesh.rotate_from_original(turn(-self.roll, Vector3::x_axis())); }
    for name in ["left_elevator", "right_elevator"] {
      if let Some(mesh) = model.mesh_mut(name) { mesh.rotate_from_original(turn(self.pitch, Vector3::x_axis())); }
    }
    if let Some(mesh) = model.mesh_mut("rudder_0") { mesh.rotate_from_original(turn(self.yaw, Vector3::y_axis())); }

    // Hiding one.
    if toggle_afterburner {
      if let Some(mesh) = model.mesh_mut("Afterburner") { mesh.visible = !mesh.visible; }
    }
  }
}
use pankarta_software::scene_manager::scene::SceneBehaviour;
use pankarta_software::ui::TextStyle;
use pankarta_software::ui::ui_node::UiNode;
use pankarta_software::ui::ui_transform::SizeValue;
use pankarta_software::ui::widgets::text;

// Orbits the camera around a node: moving the mouse turns it around
// (left/right) and over (up/down) it, always `distance` away. Facing it is
// LookAt's job (see the camera node below).
struct CameraMovement {
  target: String,
  distance: f32,
  // Around the target (radians): 0 = behind it (-Z), turning to its right.
  yaw: f32,
  // Over the target (radians): 0 = level, up to straight above/below.
  pitch: f32,
  // Radians per pixel the mouse moves.
  sensitivity: f32,
}

impl Behavior for CameraMovement {
  fn update(&mut self, node: &mut Node, scene: &mut pankarta_software::scene_manager::scene_access::SceneAccess, app: &mut App) {
    self.yaw += app.input.mouse_rel_x() as f32 * self.sensitivity;
    self.pitch = (self.pitch + app.input.mouse_rel_y() as f32 * self.sensitivity).clamp(-1.5, 1.5);

    let center = scene.get_node("plane").unwrap().get_property::<Transform3D>().unwrap().position;

    // calculate offset from the object to the camera
    let offset = Vector3::new(
      self.yaw.sin() * self.pitch.cos(),
      self.pitch.sin(),
      -self.yaw.cos() * self.pitch.cos(),
    ) * self.distance;

    // apply transform to the camera
    if let Some(transform) = node.get_property_mut::<Transform3D>() {
      transform.position = center + offset;
    }
  }
}

// Scene and behaviour

#[derive(Default)]
struct ModelViewer {
    frames: u32,
    elapsed: f32,
}

impl SceneBehaviour for ModelViewer {
  fn on_spawn(&mut self, scene: &mut pankarta_software::scene_manager::scene::Scene, app: &mut App) {

    let framerate_label = text("600 fps", &TextStyle::default())
        .set_padding((10.0, 10.0))
        .set_size(SizeValue::Pixels(200.0), SizeValue::Fit);
    let hud = Node::new("hud").with_property(UiLayer::new(UiNode::container().set_child("framerate", framerate_label)));
    scene.spawn_node(app, hud).unwrap();

    app.window.set_cursor_locked(true);

    let model_node = Node::new("plane")
      .with_property(Model::file("models/f16.glb").transform(Transform3D { scale: Vector3::new(15.0, 15.0, 15.0), ..Default::default() })) // the file is ~1 unit long; an F-16 is ~15 m
      .with_property(Transform3D { position: Vector3::new(0.0,0.0,0.0), rotation: rotation_from_degrees(Vector3::new(0.0, 0.0, 0.0)), scale: Vector3::new(1.0,1.0,1.0), })
      .with_behavior(ControlSurfaces { max_deflection: 0.4, speed: 2.0, roll: 0.0, pitch: 0.0, yaw: 0.0, listed: false });

    scene.spawn_node(app, model_node).unwrap();

    let camera = Node::new("camera")
      .with_property(Transform3D { position: Vector3::new(0.0, 5.0, -25.0), ..Default::default() })
      .with_property(CameraLens::new(60.0))
      .with_property(LookAt { target: "plane".into() })
      .with_behavior(CameraMovement { target: "plane".into(), distance: 25.0, yaw: 0.0, pitch: 0.2, sensitivity: 0.005 });

    scene.spawn_node(app, camera).unwrap();

    let sun = Node::new("sun")
      .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.2)))
      .with_property(Light::directional([1.0, 1.0, 1.0]));
    scene.spawn_node(app, sun).unwrap();
  }

  fn update(&mut self, scene: &mut pankarta_software::prelude::Scene, app: &mut App, ctx: &mut pankarta_software::prelude::FrameContext) {
      if let Some(label) = scene.get_node_mut("hud").and_then(|node| node.get_property_mut::<UiLayer>()).and_then(|ui| ui.label_mut("framerate")) {
          let fps = app.time.fps().round();
          label.set_text(format!("{fps} fps"));
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
    app.window.set_title("2D Example");
    app.renderer.set_vsync(false);


    // Input binding setup
    app.input.bind("Left", InputSource::key(Key::A));
    app.input.bind("Right", InputSource::key(Key::D));
    app.input.bind("Up", InputSource::key(Key::W));
    app.input.bind("Down", InputSource::key(Key::S));
    app.input.bind("YawLeft", InputSource::key(Key::Q));
    app.input.bind("YawRight", InputSource::key(Key::E));
    app.input.bind("Afterburner", InputSource::key(Key::Space));

    // creating and openning the scene
    app.scene_manager.create_scene("main", |_scene, _app| {      
      ModelViewer::default()
    });

    app.scene_manager.open_scene("main");

    // running the game
    app.run();
}
