//! Perspective against orthographic cameras, on the same scene.
//!
//! Left click (or Tab) goes to the next view:
//! - perspective: the usual 3D view, where things shrink with distance;
//! - top (map): orthographic, straight down, with the same scale everywhere;
//! - side (blueprint): orthographic, from the side;
//! - isometric: orthographic, at an angle, the way strategy games look.
//!
//! The scroll wheel zooms: the field of view for perspective, and how many
//! meters fit on screen for orthographic. Right click drops a red marker on
//! the ground under the cursor (`CameraResources::screen_ray`). In the
//! orthographic views every pixel's ray runs the same way from a different
//! point, so the marker still lands exactly under the cursor.
//!
//! Each view is its own camera node. Switching between them is a
//! `select_camera` call: an orthographic one is just
//! `CameraLens::orthographic(half_height)` instead of `CameraLens::new(fovy)`.

use nalgebra::Vector3;
use pankarta_software::prelude::*;
use pankarta_software::ui::ui_node::UiNode;
use pankarta_software::ui::ui_transform::SizeValue;
use pankarta_software::ui::widgets::text;
use pankarta_software::ui::TextStyle;

const GROUND_TEXTURE: &str = "textures/grid/Dark/texture_01.png";
const CUBE_TEXTURES: [&str; 4] = [
    "textures/grid/Orange/texture_01.png",
    "textures/grid/Green/texture_01.png",
    "textures/grid/Purple/texture_01.png",
    "textures/grid/Red/texture_01.png",
];
const MARKER_TEXTURE: &str = "textures/grid/Red/texture_01.png";

/// One view: its camera node's name, what the HUD calls it, where it is and
/// which way it looks, and its lens.
struct View {
    name: &'static str,
    label: &'static str,
    position: Vector3<f32>,
    looking: Vector3<f32>,
    up: Vector3<f32>,
    lens: CameraLens,
}

fn views() -> [View; 4] {
    // Isometric: 45 degrees around, about 35 degrees down.
    let isometric = Vector3::new(1.0, -0.8165, 1.0).normalize();
    [
        View { name: "perspective", label: "perspective (fov)", position: Vector3::new(-32.0, 22.0, -32.0), looking: Vector3::new(1.0, -0.6, 1.0), up: Vector3::y(), lens: CameraLens::new(60.0) },
        // A camera close above the ground: an orthographic lens sees just as
        // much from 30 m up as from 3 km, and shadows are drawn around the
        // camera. The far plane is just past the ground (see CameraLens::zfar).
        View { name: "top", label: "top / map (orthographic)", position: Vector3::new(0.0, 30.0, 0.0), looking: -Vector3::y(), up: Vector3::z(), lens: CameraLens::orthographic(28.0).far(60.0) },
        View { name: "side", label: "side / blueprint (orthographic)", position: Vector3::new(0.0, 4.0, -40.0), looking: Vector3::z(), up: Vector3::y(), lens: CameraLens::orthographic(10.0).far(100.0) },
        View { name: "isometric", label: "isometric (orthographic)", position: -isometric * 50.0, looking: isometric, up: Vector3::y(), lens: CameraLens::orthographic(22.0).far(120.0) },
    ]
}

struct OrthographicScene {
    view: usize,
}

impl OrthographicScene {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::ProceduralSky { sun: true });

        let mut nodes = vec![
            Node::new("ground")
                .with_property(Transform3D::default())
                .with_property(Mesh::plane(0)
                    .transform(Transform3D { scale: Vector3::new(200.0, 1.0, 200.0), ..Default::default() })
                    .material(Material::standard().texture(GROUND_TEXTURE).tiled(4.0, TextureSpace::World))),
            Node::new("sun")
                .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.4)))
                .with_property(Light::directional([1.0, 1.0, 1.0])),
            // The F-16 in the middle: in the top view its true shape, as on a map.
            Node::new("f16")
                .with_property(Transform3D { position: Vector3::new(0.0, 1.8, 0.0), ..Default::default() })
                .with_property(Model::file("models/f16.glb").transform(Transform3D { scale: Vector3::new(15.0, 15.0, 15.0), ..Default::default() })),
            // Hidden under the ground until the first right click.
            Node::new("marker")
                .with_property(Transform3D { position: Vector3::new(0.0, -20.0, 0.0), ..Default::default() })
                .with_property(Mesh::cube()
                    .transform(Transform3D { scale: Vector3::new(0.8, 3.0, 0.8), ..Default::default() })
                    .material(Material::standard().texture(MARKER_TEXTURE))),
        ];
        // A grid of cubes, the same size near and far: in perspective the
        // far ones look smaller, in orthographic they're all the same.
        for x in -2i32..=2 {
            for z in -2i32..=2 {
                if x == 0 && z.abs() <= 1 {
                    continue; // where the plane is
                }
                let index = ((x + 2) * 5 + (z + 2)) as usize;
                let height = 1.0 + (index % 4) as f32;
                nodes.push(Node::new(format!("cube_{x}_{z}"))
                    .with_property(Transform3D { position: Vector3::new(x as f32 * 10.0, height / 2.0, z as f32 * 10.0), ..Default::default() })
                    .with_property(Mesh::cube()
                        .transform(Transform3D { scale: Vector3::new(2.0, height, 2.0), ..Default::default() })
                        .material(Material::standard().texture(CUBE_TEXTURES[index % 4]).tiled(1.0, TextureSpace::Object))));
            }
        }
        for view in views() {
            nodes.push(Node::new(view.name)
                .with_property(Transform3D { position: view.position, rotation: UnitQuaternion::face_towards(&view.looking, &view.up), ..Default::default() })
                .with_property(view.lens));
        }
        for node in nodes {
            if let Err(error) = scene.spawn_node(app, node) {
                eprintln!("{error}");
            }
        }

        let style = TextStyle::default();
        let hud = UiNode::container()
            .set_child("framerate", text("- fps", &style).set_padding((10.0, 10.0)).set_size(SizeValue::Pixels(200.0), SizeValue::Fit))
            .set_child("view", text("", &style).set_padding((10.0, 40.0)).set_size(SizeValue::Pixels(700.0), SizeValue::Fit))
            .set_child("controls", text("left click / Tab: next view   wheel: zoom   right click: drop a marker", &style).set_padding((10.0, 70.0)).set_size(SizeValue::Pixels(900.0), SizeValue::Fit));
        if let Err(error) = scene.spawn_node(app, Node::new("hud").with_property(UiLayer::new(hud))) {
            eprintln!("{error}");
        }

        let mut this = Self { view: 0 };
        this.select(scene, app, 0);
        this
    }

    fn select(&mut self, scene: &mut Scene, app: &mut App, view: usize) {
        self.view = view;
        let name = views()[view].name;
        scene.select_camera(name);
        // Shadows are drawn within `distance` of the camera. The orthographic
        // views see much more of the ground at once, so they draw farther.
        let mut shadows = app.renderer.shadows();
        shadows.distance = if view == 0 { Shadows::default().distance } else { 120.0 };
        app.renderer.set_shadows(shadows);
    }

    fn view_text(&self, scene: &mut Scene) -> String {
        let view = &views()[self.view];
        let zoom = scene.get_node_mut(view.name).and_then(|node| node.get_property::<CameraLens>().copied()).map(|lens| match lens.orthographic {
            Some(half_height) => format!("{:.0} m tall on screen", half_height * 2.0),
            None => format!("{:.0} degree field of view", lens.fovy),
        }).unwrap_or_default();
        format!("view {}/4: {} - {}", self.view + 1, view.label, zoom)
    }

    /// Moves the marker to where the cursor's ray meets the ground (y = 0).
    fn drop_marker(&self, scene: &mut Scene, app: &mut App) {
        let config = &app.renderer.config;
        let (x, y) = (app.input.mouse_x() as f32, app.input.mouse_y() as f32);
        let Some((start, direction)) = app.renderer.camera_resources.screen_ray(scene.cameras.active(), x, y, config.width, config.height) else { return };
        if direction.y.abs() < 1e-6 {
            return; // along the ground: never meets it
        }
        let along = -start.y / direction.y;
        if along < 0.0 {
            return;
        }
        let hit = start + direction * along;
        if let Some(transform) = scene.get_node_mut("marker").and_then(|node| node.get_property_mut::<Transform3D>()) {
            transform.position = Vector3::new(hit.x, 1.5, hit.z);
        }
    }
}

impl SceneBehaviour for OrthographicScene {
    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        if app.input.mouse_left_button_just_pressed() || app.input.is_just_pressed(InputSource::key(Key::Tab)) {
            let next = (self.view + 1) % views().len();
            self.select(scene, app, next);
        }
        if app.input.is_just_pressed(InputSource::mouse(MouseButton::Right)) {
            self.drop_marker(scene, app);
        }

        let scroll = app.input.mouse_scroll_y();
        if scroll != 0.0 {
            if let Some(lens) = scene.get_node_mut(views()[self.view].name).and_then(|node| node.get_property_mut::<CameraLens>()) {
                match &mut lens.orthographic {
                    // Each notch shows 10% less (or more) of the world.
                    Some(half_height) => *half_height = (*half_height * 0.9f32.powf(scroll)).clamp(2.0, 150.0),
                    None => lens.fovy = (lens.fovy - scroll * 3.0).clamp(15.0, 110.0),
                }
            }
        }

        let fps = app.time.fps().round();
        let view_text = self.view_text(scene);
        if let Some(ui) = scene.get_node_mut("hud").and_then(|node| node.get_property_mut::<UiLayer>()) {
            if let Some(label) = ui.label_mut("framerate") {
                label.set_text(format!("{fps} fps"));
            }
            if let Some(label) = ui.label_mut("view") {
                label.set_text(view_text);
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
    app.window.set_title("Orthographic camera");
    app.scene_manager.create_scene("main", OrthographicScene::new);
    app.scene_manager.open_scene("main");
    app.run();
}
