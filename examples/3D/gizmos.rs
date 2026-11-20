//! Gizmos: lines, icons and text drawn over the world for debugging, from
//! any behavior (see engine::rendering::gizmos and docs/gizmos.md).
//!
//! - A plane flies round a tower, drawing its forces in the "aero" channel:
//!   its center of mass, the wing's center of lift with the lift and drag,
//!   the engine's thrust, the wing's outline and its own axes. Behind the
//!   tower the lines are faint; the icons inside the plane, and its name,
//!   are on top.
//! - Every engine icon in a row, with its name, in the "icons" channel -
//!   and one loaded by the game ("plane"), drawn the same way.
//! - Hits dropped from `fixed_update` in the "hits" channel, kept up for two
//!   seconds each with `.duration`.
//!
//! 1, 2, 3: switch the channels on and off. G: every gizmo.

use pankarta_software::prelude::*;

const GROUND_TEXTURE: &str = "textures/grid/Dark/texture_01.png";
const TOWER_TEXTURE: &str = "textures/grid/Purple/texture_01.png";
const PLANE_TEXTURE: &str = "textures/grid/Light/texture_01.png";

/// An icon of the game's own - any SVG; a file would be loaded with
/// `app.gizmos.icons.load("plane", "icons/plane.svg")`.
const PLANE_ICON: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M12 1.5c.9 0 1.4.9 1.4 2.2v5.4l8.6 5v2.2l-8.6-2.6v4.8l2.5 1.9v1.7L12 21.2l-3.9 1v-1.7l2.5-1.9v-4.8L2 16.3v-2.2l8.6-5V3.7c0-1.3.5-2.2 1.4-2.2z"/></svg>"#;

/// The plane's wing, in its own frame: half its span and chord.
const HALF_SPAN: f32 = 3.0;
const HALF_CHORD: f32 = 0.6;

/// Takes the camera around the "focus" node (it looks at it - see `LookAt`).
struct Orbit {
    angle: f32,
}

impl Behavior for Orbit {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.angle += 0.1 * app.time.delta_time;
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            transform.position = Vector3::new(self.angle.sin() * 30.0, 12.0, self.angle.cos() * 30.0);
        }
    }
}

/// Flies the plane round the tower, banked into the turn - and draws what
/// acts on it.
struct Flight {
    angle: f32,
}

impl Behavior for Flight {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        const RADIUS: f32 = 12.0;
        const SPEED: f32 = 0.35;
        self.angle += SPEED * app.time.delta_time;
        let (sin, cos) = self.angle.sin_cos();
        let position = Vector3::new(cos * RADIUS, 6.0 + (self.angle * 2.0).sin(), sin * RADIUS);
        let forward = Vector3::new(-sin, 0.0, cos);
        let bank = UnitQuaternion::from_axis_angle(&Vector3::z_axis(), -0.5);
        let rotation = UnitQuaternion::face_towards(&forward, &Vector3::y()) * bank;
        let Some(transform) = node.get_property_mut::<Transform3D>() else { return };
        transform.position = position;
        transform.rotation = rotation;

        // Points on the plane, in the world.
        let at = |local: Vector3<f32>| position + rotation * local;
        let up = rotation * Vector3::y();
        let center_of_mass = at(Vector3::new(0.0, 0.0, 0.4));
        let center_of_lift = at(Vector3::new(0.0, 0.15, 0.1));
        let nozzle = at(Vector3::new(0.0, 0.0, -2.2));
        let wing = [
            at(Vector3::new(-HALF_SPAN, 0.15, HALF_CHORD)),
            at(Vector3::new(HALF_SPAN, 0.15, HALF_CHORD)),
            at(Vector3::new(HALF_SPAN, 0.15, -HALF_CHORD)),
            at(Vector3::new(-HALF_SPAN, 0.15, -HALF_CHORD)),
        ];
        let speed = RADIUS * SPEED;

        app.gizmos.channel("aero", |g| {
            // Points inside the plane: its own body would hide them - drawn
            // on top.
            g.icon(center_of_mass, Icon::Mass).on_top().label("CoM");
            g.icon(center_of_lift, Icon::Wind).color(GizmoColor::CYAN).on_top();
            g.ray(center_of_lift, up * 3.0).color(GizmoColor::CYAN).arrow().label("lift");
            g.ray(center_of_lift, -forward * 1.5).color(GizmoColor::RED).arrow().label("drag");
            g.icon(nozzle, Icon::Fire).color(GizmoColor::ORANGE).on_top();
            g.ray(nozzle, forward * speed * 0.5).colors(GizmoColor::ORANGE, GizmoColor::YELLOW).width(4.0).arrow();
            g.outline(&wing).color(GizmoColor::GRAY).dashed();
            g.axes((position, rotation), 1.5);
            // Its path, a little ahead and behind.
            let path: Vec<Vector3<f32>> = (-12..=12).map(|step| {
                let angle = self.angle + step as f32 * 0.05;
                Vector3::new(angle.cos() * RADIUS, 6.0 + (angle * 2.0).sin(), angle.sin() * RADIUS)
            }).collect();
            g.path(&path).colors(GizmoColor::WHITE.alpha(0.0), GizmoColor::WHITE.alpha(0.6)).width(1.5);
            g.icon(position + Vector3::y() * 2.0, Icon::named("plane")).size(26.0).color(GizmoColor::YELLOW).on_top().label("Plane");
        });
    }
}

fn cube(id: &str, position: Vector3<f32>, size: Vector3<f32>, texture: &str) -> Node {
    Node::new(id)
        .with_property(Transform3D { position, ..Default::default() })
        .with_property(Mesh::cube()
            .transform(Transform3D { scale: size, ..Default::default() })
            .material(Material::standard().texture(texture).tiled(1.0, TextureSpace::Object)))
}

fn nodes() -> Vec<Node> {
    let ground = Node::new("ground")
        .with_property(Transform3D::default())
        .with_property(Mesh::plane(0)
            .transform(Transform3D { scale: Vector3::new(200.0, 1.0, 200.0), ..Default::default() })
            .material(Material::standard().texture(GROUND_TEXTURE).tiled(4.0, TextureSpace::World)));
    let sun = Node::new("sun")
        .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.2)))
        .with_property(Light::directional([1.0, 1.0, 1.0]));
    let focus = Node::new("focus").with_property(Transform3D { position: Vector3::new(0.0, 4.0, 0.0), ..Default::default() });
    let camera = Node::new("camera")
        .with_property(Transform3D::default())
        .with_property(CameraLens::new(60.0))
        .with_property(LookAt { target: "focus".into() })
        .with_behavior(Orbit { angle: 0.4 });
    let tower = cube("tower", Vector3::new(0.0, 5.0, 0.0), Vector3::new(4.0, 10.0, 4.0), TOWER_TEXTURE);

    // The plane: a body, a wing across it and a tail - a cube each.
    let plane = Node::new("plane")
        .with_property(Transform3D::default())
        .with_property(Mesh::cube()
            .transform(Transform3D { scale: Vector3::new(0.7, 0.7, 4.4), ..Default::default() })
            .material(Material::standard().texture(PLANE_TEXTURE).tiled(1.0, TextureSpace::Object)))
        .with_child(cube("wing", Vector3::new(0.0, 0.15, 0.1), Vector3::new(HALF_SPAN * 2.0, 0.12, HALF_CHORD * 2.0), PLANE_TEXTURE))
        .with_child(cube("tail", Vector3::new(0.0, 0.5, -1.9), Vector3::new(0.12, 0.9, 0.6), PLANE_TEXTURE))
        .with_behavior(Flight { angle: 0.0 });

    vec![ground, sun, focus, camera, tower, plane]
}

struct GizmosScene {
    /// Physics steps since the last hit.
    steps: u32,
    hits: u32,
}

impl GizmosScene {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::ProceduralSky { sun: true });
        if let Err(error) = app.gizmos.icons.load_svg("plane", PLANE_ICON) {
            eprintln!("{error}");
        }
        // The icons' labels a little bigger than the default.
        app.gizmos.style("icons").text_size(13.0).icon_size(28.0);
        for node in nodes() {
            if let Err(error) = scene.spawn_node(app, node) {
                eprintln!("{error}");
            }
        }
        println!("1: aero gizmos, 2: icons, 3: hits, G: all gizmos");
        Self { steps: 0, hits: 0 }
    }
}

impl SceneBehaviour for GizmosScene {
    fn update(&mut self, _scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        for (key, channel) in [(Key::NUM_1, "aero"), (Key::NUM_2, "icons"), (Key::NUM_3, "hits")] {
            if app.input.is_just_pressed(InputSource::key(key)) {
                let on = app.gizmos.toggle(channel);
                println!("{channel}: {}", if on { "on" } else { "off" });
            }
        }
        if app.input.is_just_pressed(InputSource::key(Key::G)) {
            app.gizmos.visible = !app.gizmos.visible;
        }

        // Every engine icon in a row, by the tower, then the game's own.
        app.gizmos.channel("icons", |g| {
            let icons = Icon::BUILT_IN.iter().cloned().chain([Icon::named("plane")]);
            for (index, icon) in icons.enumerate() {
                let position = Vector3::new(-17.0 + index as f32 * 2.0, 1.5, 18.0);
                g.icon(position, icon.clone()).label(icon.name());
            }
        });
    }

    // A hit somewhere on the ground every second of physics steps. Drawn in
    // a step, so it's given a duration - without one it would only show on
    // the frames a step runs.
    fn fixed_update(&mut self, _scene: &mut Scene, app: &mut App) {
        self.steps += 1;
        if self.steps < 60 {
            return;
        }
        self.steps = 0;
        self.hits += 1;
        let angle = self.hits as f32 * 2.4;
        let point = Vector3::new(angle.cos() * 8.0, 0.05, angle.sin() * 8.0);
        app.gizmos.channel("hits", |g| {
            g.circle(point, Vector3::y(), 0.8).color(GizmoColor::RED).duration(2.0);
            g.cross(point, 0.6).color(GizmoColor::RED).duration(2.0);
            g.text(point + Vector3::y() * 0.8, format!("hit {}", self.hits)).color(GizmoColor::RED).duration(2.0);
        });
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
    app.window.set_title("Gizmos");

    app.scene_manager.create_scene("main", GizmosScene::new);
    app.scene_manager.open_scene("main");
    app.run();
}
