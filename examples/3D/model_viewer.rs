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
  // Fraction of `distance` each wheel notch moves the camera (up = closer).
  zoom_speed: f32,
  min_distance: f32,
  max_distance: f32,
}

impl Behavior for CameraMovement {
  fn update(&mut self, node: &mut Node, scene: &mut pankarta_software::scene_manager::scene_access::SceneAccess, app: &mut App) {
    // Only steered while it's the camera being looked through.
    if scene.cameras().active_name() != ORBIT_CAMERA {
      return;
    }
    self.yaw += app.input.mouse_rel_x() as f32 * self.sensitivity;
    self.pitch = (self.pitch + app.input.mouse_rel_y() as f32 * self.sensitivity).clamp(-1.5, 1.5);

    // Zoom: wheel up moves in, wheel down moves out. Scaling by the current
    // distance keeps each notch feeling the same up close and far away.
    let scroll = app.input.mouse_scroll_y();
    if scroll != 0.0 {
      self.distance = (self.distance * (1.0 - scroll * self.zoom_speed)).clamp(self.min_distance, self.max_distance);
    }

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

const ORBIT_CAMERA: &str = "camera";
const COCKPIT_CAMERA: &str = "cockpit_camera";

// A first-person camera sitting in the pilot's seat: it moves and turns with
// the plane, and the mouse turns the pilot's head - left/right (yaw) and
// up/down (pitch), from looking straight ahead along the nose.
struct CockpitCamera {
  plane: String,
  // Where the pilot's eyes are, in the plane node's frame (its +Z is the nose).
  eye: Vector3<f32>,
  yaw: f32,
  pitch: f32,
  // Radians per pixel the mouse moves.
  sensitivity: f32,
}

impl Behavior for CockpitCamera {
  fn update(&mut self, node: &mut Node, scene: &mut pankarta_software::scene_manager::scene_access::SceneAccess, app: &mut App) {
    let active = scene.cameras().active_name() == COCKPIT_CAMERA;
    if active {
      // Looking along +Z: turning right is a negative turn about Y, looking
      // down a positive one about X. A head turns only so far.
      self.yaw = (self.yaw - app.input.mouse_rel_x() as f32 * self.sensitivity).clamp(-2.6, 2.6);
      self.pitch = (self.pitch + app.input.mouse_rel_y() as f32 * self.sensitivity).clamp(-1.3, 1.4);
    }

    // Kept in the seat even while not looked through, so switching to it
    // doesn't show a frame from wherever it was.
    let Some(plane) = scene.get_node(&self.plane).and_then(|plane| plane.get_property::<Transform3D>()).copied() else { return };
    let head = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.yaw) * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), self.pitch);
    if let Some(transform) = node.get_property_mut::<Transform3D>() {
      transform.position = plane.position + plane.rotation * self.eye;
      transform.rotation = plane.rotation * head;
    }
  }
}

// Scene and behaviour

struct ModelViewer {
    // Whether the sun's shadows darken see-through parts (glass, flames).
    shadows_on_transparent: bool,
    // Whether there are shadows at all - off shows what's dark only because
    // it faces away from the sun.
    shadows_enabled: bool,
    // Which of SHADOW_PRESETS is in use - G cycles them.
    shadow_preset: usize,
    // Where the sun is (radians): around the plane from its nose (+Z),
    // turning toward its left (+X), and above the horizon. Arrow keys move it.
    sun_azimuth: f32,
    sun_elevation: f32,
    // Ambient light brighter from the sky than from the ground (the engine's
    // default), or the same whichever way a surface faces - H flips it.
    sky_ground_ambient: bool,
}

impl Default for ModelViewer {
    fn default() -> Self {
        // The sun the viewer always had: shining along (-0.3, -1, -0.2) -
        // ahead of the nose, a little to the left, 70 degrees up.
        let toward = Vector3::new(0.3_f32, 1.0, 0.2).normalize();
        Self {
            shadows_on_transparent: true,
            shadows_enabled: true,
            shadow_preset: 1,
            sun_azimuth: toward.x.atan2(toward.z),
            sun_elevation: toward.y.asin(),
            sky_ground_ambient: true,
        }
    }
}

// A shadow look to compare: how far from the camera shadows go, in how many
// maps (cascades), each how many texels across, and how wide their edges
// fade (`blur`, meters - the engine's default is 1.5 cm). Each map is
// CASCADE_GROWTH (3) times the one before, the last `distance` each way - so
// the nearest's texel is about 2.5 * distance / 3^(cascades - 1) / resolution.
struct ShadowPreset {
    name: &'static str,
    distance: f32,
    cascades: u32,
    resolution: u32,
    blur: f32,
}

const SHADOW_PRESETS: [ShadowPreset; 5] = [
    // The engine's old defaults: ~2 cm a texel nearest, shadows out to 150 m.
    ShadowPreset { name: "far: 150 m, ~2 cm texels", distance: 150.0, cascades: 3, resolution: 2048, blur: 0.015 },
    // The engine's defaults: the whole cockpit in the nearest map (~2 mm a
    // texel), out to 40 m - edges fading over 1.5 cm.
    ShadowPreset { name: "default, sharp, soft edges: 40 m, 1.5 cm fade", distance: 40.0, cascades: 4, resolution: 2048, blur: 0.015 },
    // The same, fading over 4 cm - like a hazier day.
    ShadowPreset { name: "sharp, softer edges: 40 m, 4 cm fade", distance: 40.0, cascades: 4, resolution: 2048, blur: 0.04 },
    // The same, edges as hard as the texels make them (~2 mm).
    ShadowPreset { name: "sharp, hard edges: 40 m, no fade", distance: 40.0, cascades: 4, resolution: 2048, blur: 0.0 },
    // Twice the texels each way: ~1 mm nearest out to 40 m - but 4x the
    // memory (about 512 MB for the 4 maps and their cached copies).
    ShadowPreset { name: "4K: 40 m, ~1 mm texels, 1.5 cm fade (512 MB)", distance: 40.0, cascades: 4, resolution: 4096, blur: 0.015 },
];

const HUD_LINES: [&str; 6] = ["camera", "sun", "shadows", "quality", "ambient", "transparent"];

// Radians a second the arrow keys move the sun.
const SUN_SPEED: f32 = 0.8;

impl ModelViewer {
    // One line per HUD label, in HUD_LINES' order.
    fn controls_text(&self, cockpit: bool) -> [String; 6] {
        let on_off = |on: bool| if on { "on" } else { "off" };
        let view = if cockpit { "cockpit" } else { "orbit" };
        [
            format!("C: camera ({view})"),
            format!("Arrows: sun ({:.0}° around, {:.0}° up)", self.sun_azimuth.to_degrees().rem_euclid(360.0), self.sun_elevation.to_degrees()),
            format!("F: shadows ({})", on_off(self.shadows_enabled)),
            format!("G: shadow quality ({})", SHADOW_PRESETS[self.shadow_preset].name),
            format!("H: ambient light ({})", if self.sky_ground_ambient { "brighter from the sky" } else { "flat, the same all ways" }),
            format!("T: shadows on glass/flames ({})", on_off(self.shadows_on_transparent)),
        ]
    }

    // The engine's shadow settings, from this viewer's toggles.
    fn apply_shadows(&self, app: &mut App) {
        let preset = &SHADOW_PRESETS[self.shadow_preset];
        let mut shadows = pankarta_software::prelude::Shadows::default();
        shadows.enabled = self.shadows_enabled;
        shadows.on_transparent = self.shadows_on_transparent;
        shadows.distance = preset.distance;
        shadows.cascades = preset.cascades;
        shadows.resolution = preset.resolution;
        shadows.blur = preset.blur;
        app.renderer.set_shadows(shadows);
        // 0.45 is the engine's default; 1 the old flat ambient.
        app.renderer.set_ambient_ground(if self.sky_ground_ambient { 0.45 } else { 1.0 });
    }

    // The way the sunlight travels - from the sun, toward the plane.
    fn sun_direction(&self) -> Vector3<f32> {
        let (elevation_sin, elevation_cos) = self.sun_elevation.sin_cos();
        -Vector3::new(self.sun_azimuth.sin() * elevation_cos, elevation_sin, self.sun_azimuth.cos() * elevation_cos)
    }
}

impl SceneBehaviour for ModelViewer {
  fn on_spawn(&mut self, scene: &mut pankarta_software::scene_manager::scene::Scene, app: &mut App) {

    let framerate_label = text("600 fps", &TextStyle::default())
        .set_padding((10.0, 10.0))
        .set_size(SizeValue::Pixels(200.0), SizeValue::Fit);
    let mut container = UiNode::container().set_child("framerate", framerate_label);
    for (id, line) in HUD_LINES.iter().zip(self.controls_text(false)) {
        container = container.set_child(*id, text(&line, &TextStyle::default())
            .set_padding((10.0, 4.0))
            .set_size(SizeValue::Pixels(700.0), SizeValue::Fit));
    }
    let hud = Node::new("hud").with_property(UiLayer::new(container));
    scene.spawn_node(app, hud).unwrap();
    self.apply_shadows(app);

    app.window.set_cursor_locked(true);

    let model_node = Node::new("plane")
      .with_property(Model::file("models/f16.glb").transform(Transform3D { scale: Vector3::new(15.0, 15.0, 15.0), ..Default::default() })) // the file is ~1 unit long; an F-16 is ~15 m
      .with_property(Transform3D { position: Vector3::new(0.0,0.0,0.0), rotation: rotation_from_degrees(Vector3::new(0.0, 0.0, 0.0)), scale: Vector3::new(1.0,1.0,1.0), })
      .with_behavior(ControlSurfaces { max_deflection: 0.4, speed: 2.0, roll: 0.0, pitch: 0.0, yaw: 0.0, listed: false });

    scene.spawn_node(app, model_node).unwrap();

    let camera = Node::new(ORBIT_CAMERA)
      .with_property(Transform3D { position: Vector3::new(0.0, 5.0, -25.0), ..Default::default() })
      .with_property(CameraLens::new(60.0))
      .with_property(LookAt { target: "plane".into() })
      .with_behavior(CameraMovement { target: "plane".into(), distance: 25.0, yaw: 0.0, pitch: 0.2, sensitivity: 0.005, zoom_speed: 0.1, min_distance: 5.0, max_distance: 200.0 });

    scene.spawn_node(app, camera).unwrap();

    // The pilot's eyes: in the file the seat runs z 0.42..0.51 (headrest at
    // the back) and the canopy tops out at y 0.09 - times the model's 15.
    // Not the first camera, so it doesn't start active - C switches to it.
    let cockpit_camera = Node::new(COCKPIT_CAMERA)
      .with_property(Transform3D::default())
      .with_property(CameraLens::new(80.0))
      .with_behavior(CockpitCamera { plane: "plane".into(), eye: Vector3::new(0.0, 1.08, 6.8), yaw: 0.0, pitch: 0.0, sensitivity: 0.003 });
    scene.spawn_node(app, cockpit_camera).unwrap();

    let sun = Node::new("sun")
      .with_property(Transform3D::facing(self.sun_direction()))
      .with_property(Light::directional([1.0, 1.0, 1.0]));
    scene.spawn_node(app, sun).unwrap();
  }

  fn update(&mut self, scene: &mut pankarta_software::prelude::Scene, app: &mut App, _ctx: &mut pankarta_software::prelude::FrameContext) {
      if app.input.is_action_just_pressed("ToggleCamera") {
          let next = if scene.cameras.active_name() == COCKPIT_CAMERA { ORBIT_CAMERA } else { COCKPIT_CAMERA };
          scene.select_camera(next);
      }
      let mut changed = false;
      for (action, toggle) in [
          ("ToggleTransparentShadows", &mut self.shadows_on_transparent),
          ("ToggleShadows", &mut self.shadows_enabled),
          ("ToggleAmbient", &mut self.sky_ground_ambient),
      ] {
          if app.input.is_action_just_pressed(action) {
              *toggle = !*toggle;
              changed = true;
          }
      }
      if app.input.is_action_just_pressed("ToggleShadowQuality") {
          self.shadow_preset = (self.shadow_preset + 1) % SHADOW_PRESETS.len();
          changed = true;
      }
      if changed {
          self.apply_shadows(app);
      }

      // The sun: held arrow keys turn it around the plane (left/right) and
      // raise or lower it (up/down) - from just over the horizon to overhead.
      let step = SUN_SPEED * app.time.delta_time;
      let around = app.input.is_action_pressed("SunLeft") as i32 - app.input.is_action_pressed("SunRight") as i32;
      let up = app.input.is_action_pressed("SunUp") as i32 - app.input.is_action_pressed("SunDown") as i32;
      if around != 0 || up != 0 {
          self.sun_azimuth += around as f32 * step;
          self.sun_elevation = (self.sun_elevation + up as f32 * step).clamp(0.02, 1.55);
          let direction = self.sun_direction();
          if let Some(transform) = scene.get_node_mut("sun").and_then(|sun| sun.get_property_mut::<Transform3D>()) {
              transform.rotation = Transform3D::facing(direction).rotation;
          }
      }

      let cockpit = scene.cameras.active_name() == COCKPIT_CAMERA;
      let lines = self.controls_text(cockpit);
      if let Some(ui) = scene.get_node_mut("hud").and_then(|node| node.get_property_mut::<UiLayer>()) {
          if let Some(label) = ui.label_mut("framerate") {
              let fps = app.time.fps().round();
              label.set_text(format!("{fps} fps"));
          }
          for (id, line) in HUD_LINES.iter().zip(lines) {
              if let Some(label) = ui.label_mut(id) {
                  label.set_text(line);
              }
          }
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
    app.input.bind("ToggleCamera", InputSource::key(Key::C));
    app.input.bind("ToggleTransparentShadows", InputSource::key(Key::T));
    app.input.bind("ToggleShadows", InputSource::key(Key::F));
    app.input.bind("ToggleShadowQuality", InputSource::key(Key::G));
    app.input.bind("ToggleAmbient", InputSource::key(Key::H));
    app.input.bind("SunLeft", InputSource::key(Key::Left));
    app.input.bind("SunRight", InputSource::key(Key::Right));
    app.input.bind("SunUp", InputSource::key(Key::Up));
    app.input.bind("SunDown", InputSource::key(Key::Down));

    // creating and openning the scene
    app.scene_manager.create_scene("main", |_scene, _app| {      
      ModelViewer::default()
    });

    app.scene_manager.open_scene("main");

    // running the game
    app.run();
}
