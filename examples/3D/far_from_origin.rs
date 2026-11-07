//! Floating-point precision far from the world origin - and the floating
//! origin that fixes it (see `WorldOrigin`, `app.origin`).
//!
//! The walking player, a floor and a cube flying in circles over it - built
//! several times, each copy further from (0, 0, 0) in the world. Number keys
//! 1-6 teleport the player to each copy; the top left shows where the player
//! is in the world and in the scene's own (local) numbers, and how big the
//! smallest step an `f32` can take is there.
//!
//! With the floating origin on (the default), the world is moved so the
//! player is always near the local (0, 0, 0): every area walks, moves and
//! looks the same. O turns it off (sending the player back to area 1, with
//! the origin back at the world's (0, 0, 0)) - then, the further out, the
//! coarser every coordinate gets: the floor's texture shimmers, the flying
//! cube stutters, and from area 5 on the player can't move.

use nalgebra::{UnitQuaternion, Vector3};
use pankarta_software::prelude::*;
use pankarta_software::ui::ui_node::UiNode;
use pankarta_software::ui::ui_transform::{Anchor, SizeValue};
use pankarta_software::ui::widgets::text;
use pankarta_software::ui::{Align, TextStyle};

#[path = "walking/player.rs"]
pub mod player;

use player::Player;

const PLAYER_HALF_HEIGHT: f32 = 0.9;
const GROUND_TEXTURE: &str = "textures/grid/Dark/texture_01.png";
const CUBE_TEXTURE: &str = "textures/grid/Orange/texture_01.png";

/// How far each test area is from the world's origin, on X and on Z - one
/// per number key (1 = the origin itself).
const DISTANCES: [f64; 6] = [0.0, 1_000.0, 10_000.0, 100_000.0, 1_000_000.0, 10_000_000.0];
const TELEPORT_KEYS: [Key; 6] = [Key::NUM_1, Key::NUM_2, Key::NUM_3, Key::NUM_4, Key::NUM_5, Key::NUM_6];
const TOGGLE_ORIGIN_KEY: Key = Key::O;

const FLOOR_SIZE: f32 = 80.0;
/// Where the player lands in an area, from its middle.
const SPAWN_OFFSET: Vector3<f64> = Vector3::new(0.0, PLAYER_HALF_HEIGHT as f64 + 0.5, -10.0);

const HUD: &str = "coordinates";
const HUD_LINES: usize = 7;

/// The middle of area `index`, in the world.
fn area_center(index: usize) -> Vector3<f64> {
    Vector3::new(DISTANCES[index], 0.0, DISTANCES[index])
}

/// The gap between `value` and the next `f32` up - the smallest move
/// anything can make at that coordinate.
fn f32_step(value: f32) -> f32 {
    let value = value.abs();
    f32::from_bits(value.to_bits() + 1) - value
}

/// The cube flying in circles over an area's floor. Its area's middle is
/// kept as a world position (`f64`) and turned into the scene's local
/// numbers each frame (`to_local`), so it stays right wherever the floating
/// origin is - no `on_origin_shift` needed.
struct FlyingCube {
    center: Vector3<f64>,
    time: f32,
}

impl Behavior for FlyingCube {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.time += app.time.delta_time;
        let t = self.time;
        let offset = Vector3::new(t.cos() * 6.0, 4.0 + (t * 2.0).sin() * 0.75, t.sin() * 6.0);
        let center = app.origin.to_local(self.center);
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            transform.position = center + offset;
            transform.rotation = UnitQuaternion::from_euler_angles(t * 0.7, t, 0.0);
        }
    }
}

struct FarFromOriginScene {
    area: usize,
}

impl FarFromOriginScene {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::ProceduralSky { sun: true });

        let sun = Node::new("sun")
            .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.2)))
            .with_property(Light::directional([1.0, 1.0, 1.0]).shadows(true));
        // The floating origin follows the player.
        let player = Node::new("player")
            .with_property(OriginAnchor)
            .with_behavior(Player::new(app.origin.to_local(area_center(0) + SPAWN_OFFSET)));

        let mut nodes = vec![sun, player];
        for index in 0..DISTANCES.len() {
            let center = area_center(index);
            nodes.push(
                Node::new(format!("floor_{index}"))
                    .with_property(Transform3D { position: app.origin.to_local(center), ..Default::default() })
                    .with_property(Mesh::plane(0)
                        .transform(Transform3D { scale: Vector3::new(FLOOR_SIZE, 1.0, FLOOR_SIZE), ..Default::default() })
                        .material(Material::standard().texture(GROUND_TEXTURE).tiled(4.0, TextureSpace::World)))
                    .with_property(Physics {
                        rigidbody: RigidBodyData::fixed(),
                        // A slab whose top is the floor's surface.
                        colliders: vec![Collider::new(ColliderShape::Cuboid { half_extents: (FLOOR_SIZE / 2.0, 0.5, FLOOR_SIZE / 2.0) })
                            .offset(Vector3::new(0.0, -0.5, 0.0))],
                    }),
            );
            nodes.push(
                Node::new(format!("flying_cube_{index}"))
                    .with_property(Transform3D { position: app.origin.to_local(center) + Vector3::y() * 4.0, ..Default::default() })
                    .with_property(Mesh::cube().material(Material::standard().texture(CUBE_TEXTURE).tiled(1.0, TextureSpace::Object)))
                    .with_behavior(FlyingCube { center, time: 0.0 }),
            );
        }
        for node in nodes {
            if let Err(error) = scene.spawn_node(app, node) {
                eprintln!("{error}");
            }
        }

        // A label is a single line, so the readout is a column of them -
        // filled every frame in `update`.
        let mut hud = UiNode::container()
            .set_padding((10.0, 10.0))
            .set_child_anchor(Anchor::Start, Anchor::Start)
            .set_gap(2.0);
        for line in 0..HUD_LINES {
            hud = hud.set_child(
                format!("line{line}"),
                text("-", &TextStyle::default().align(Align::Left)).set_size(SizeValue::Pixels(800.0), SizeValue::Fit),
            );
        }
        if let Err(error) = scene.spawn_node(app, Node::new(HUD).with_property(UiLayer::new(hud))) {
            eprintln!("{error}");
        }

        app.window.set_cursor_locked(true);
        Self { area: 0 }
    }

    /// Puts the player in area `index`. With the floating origin on, the
    /// origin moves there first, so the player lands precisely; off, the
    /// player is just put there, in whatever numbers that takes.
    fn teleport(&mut self, app: &mut App, index: usize) {
        let target = area_center(index) + SPAWN_OFFSET;
        if app.origin.enabled() {
            app.origin.teleport("player", target, None);
        } else {
            let local = app.origin.to_local(target);
            app.physics.teleport("player", local, UnitQuaternion::identity());
        }
        self.area = index;
    }
}

impl SceneBehaviour for FarFromOriginScene {
    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        if let Some(index) = TELEPORT_KEYS.iter().position(|key| app.input.is_just_pressed(InputSource::key(*key))) {
            self.teleport(app, index);
        }
        if app.input.is_just_pressed(InputSource::key(TOGGLE_ORIGIN_KEY)) {
            if app.origin.enabled() {
                // Back to area 1 first (moving the origin back to the
                // world's (0, 0, 0) on the way), then off - so what it's like
                // without one starts from the true origin.
                self.teleport(app, 0);
                app.origin.set_enabled(false);
            } else {
                app.origin.set_enabled(true);
            }
        }

        let Some(local) = scene.get_node("player").and_then(|node| node.get_property::<Transform3D>()).map(|transform| transform.position) else {
            return;
        };
        let world = app.origin.to_world(local);
        let origin = app.origin.origin();
        let step = local.map(f32_step);
        let lines: [String; HUD_LINES] = [
            format!("{} fps", app.time.fps().round()),
            format!("World    x {:.3}   y {:.3}   z {:.3}", world.x, world.y, world.z),
            format!("Local    x {:.4}   y {:.4}   z {:.4}", local.x, local.y, local.z),
            format!("f32 step here (m)   x {:.2e}   y {:.2e}   z {:.2e}", step.x, step.y, step.z),
            format!(
                "Floating origin {}   at x {:.0}   y {:.0}   z {:.0}",
                if app.origin.enabled() { "ON" } else { "OFF" },
                origin.x, origin.y, origin.z
            ),
            format!("Area {} of {}  -  {} m from the world's origin", self.area + 1, DISTANCES.len(), DISTANCES[self.area]),
            format!("Keys 1-{}: teleport to an area    O: floating origin on/off", DISTANCES.len()),
        ];
        let Some(hud) = scene.get_node_mut(HUD).and_then(|node| node.get_property_mut::<UiLayer>()) else {
            return;
        };
        for (index, line) in lines.iter().enumerate() {
            if let Some(label) = hud.label_mut(&format!("line{index}")) {
                label.set_text(line);
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
    app.window.set_title("Far from the origin");
    app.physics.set_gravity(Vector3::new(0.0, -20.0, 0.0));
    app.renderer.set_vsync(false);
    app.renderer.set_ambient_light([0.8, 0.8, 0.8]);

    // The walking example's controls.
    if let Err(error) = app.input.load_bindings("examples/3D/walking/input.ron") {
        eprintln!("{error}");
    }

    app.scene_manager.create_scene("main", FarFromOriginScene::new);
    app.scene_manager.open_scene("main");
    app.run();
}
