//! A ring of sensors of every shape, outside the ring of cubes - walk (and
//! jump) into each: its area turns white while you're in it, the console
//! says when you enter and leave, and the HUD lists the ones you're in.
//! Each is a collider with Rapier's `sensor` and no body of its own, so it
//! goes wherever its node is.
//!
//! - sphere, box (turned), capsule (lying down), cylinder,
//! - two domes: a whole ball half under the ground (exact - nothing goes
//!   under the floor), and `Collider::hemisphere` raised off it (a real
//!   dome shape, for when something can be under it),
//! - a convex hull from points (a pyramid),
//! - a small sphere just over head height (jump into it), and one that
//!   moves around.

use nalgebra::{Point3, UnitQuaternion, Vector3};
use pankarta_software::prelude::*;

/// How far out the ring is - the cubes are at 12.
const RING_RADIUS: f32 = 22.0;
/// The moving sensor's circle around its spot.
const PATROL_RADIUS: f32 = 3.0;
const PATROL_SPEED: f32 = 0.6;

/// Every sensor's node id, with what the HUD calls it.
pub const SENSORS: [(&str, &str); 9] = [
    ("sensor_sphere", "sphere"),
    ("sensor_dome", "dome (ball in the ground)"),
    ("sensor_raised_dome", "raised dome (hemisphere)"),
    ("sensor_box", "box"),
    ("sensor_capsule", "capsule"),
    ("sensor_cylinder", "cylinder"),
    ("sensor_pyramid", "pyramid (hull)"),
    ("sensor_high", "sphere in the air"),
    ("sensor_moving", "moving sphere"),
];

/// Every sensor of the gallery.
pub fn sensor_gallery() -> Vec<Node> {
    let pyramid = ColliderBuilder::convex_hull(&[
        Point3::new(-1.5, 0.0, -1.5),
        Point3::new(1.5, 0.0, -1.5),
        Point3::new(1.5, 0.0, 1.5),
        Point3::new(-1.5, 0.0, 1.5),
        Point3::new(0.0, 3.0, 0.0),
    ])
    .expect("a pyramid has a hull");
    let up = |height: f32| Vector3::y() * height;
    let sensors = [
        ColliderBuilder::ball(1.5).translation(up(1.5)),
        // Centered on the ground: its lower half is under the floor.
        ColliderBuilder::ball(2.2),
        // Its node is raised (below).
        Collider::hemisphere(1.8),
        ColliderBuilder::cuboid(2.5, 1.0, 1.0).translation(up(1.0)),
        // Standing, its node turned to lay it down.
        ColliderBuilder::capsule_y(1.5, 0.8),
        ColliderBuilder::cylinder(1.0, 1.8).translation(up(1.0)),
        pyramid,
        ColliderBuilder::ball(0.7),
        ColliderBuilder::ball(1.2).translation(up(1.2)),
    ];
    let colors = [
        UiColor::Rgb(255, 80, 80),
        UiColor::Rgb(240, 240, 120),
        UiColor::Rgb(200, 160, 255),
        UiColor::Rgb(255, 220, 60),
        UiColor::Rgb(255, 140, 40),
        UiColor::Rgb(80, 230, 110),
        UiColor::Rgb(40, 220, 200),
        UiColor::Rgb(255, 110, 200),
        UiColor::Rgb(120, 170, 255),
    ];

    let mut nodes = Vec::new();
    for (index, ((sensor, color), (id, name))) in sensors.into_iter().zip(colors).zip(SENSORS).enumerate() {
        let angle = (index as f32 + 0.5) / SENSORS.len() as f32 * std::f32::consts::TAU;
        let spot = Vector3::new(angle.sin(), 0.0, angle.cos()) * RING_RADIUS;
        let mut transform = Transform3D { position: spot, ..Default::default() };
        match id {
            "sensor_box" => transform.rotation = UnitQuaternion::from_euler_angles(0.0, 30f32.to_radians(), 0.0),
            "sensor_capsule" => {
                transform.position.y = 0.8;
                transform.rotation = UnitQuaternion::from_euler_angles(0.0, 0.0, 90f32.to_radians());
            }
            "sensor_raised_dome" => transform.position.y = 1.2,
            "sensor_high" => transform.position.y = 3.0,
            _ => {}
        }

        let look = ColliderDebug::new()
            .edges(color)
            .faces(color.with_alpha(0.15))
            .occupied_edges(UiColor::WHITE)
            .occupied_faces(UiColor::WHITE.with_alpha(0.3));
        let node = Node::new(id)
            .with_property(transform)
            .with_property(Colliders::new()
                .add(sensor.sensor(true))
                    .detects(["player"])
                    .debug(look))
            .with_behavior(Announce { name, patrol: (id == "sensor_moving").then_some((spot, 0.0)) });
        nodes.push(node);
    }
    nodes
}

/// Says when the player enters and leaves its sensor - and, with a
/// `patrol` (its center and how far round it is), moves it round in a circle.
struct Announce {
    name: &'static str,
    patrol: Option<(Vector3<f32>, f32)>,
}

impl Behavior for Announce {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        let Some((center, angle)) = &mut self.patrol else { return };
        *angle += PATROL_SPEED * app.time.delta_time;
        if let Some(transform) = node.transform_3d_mut() {
            transform.position = *center + Vector3::new(angle.cos(), 0.0, angle.sin()) * PATROL_RADIUS;
        }
    }

    fn on_sensor_enter(&mut self, _node: &mut Node, other: &str, _scene: &mut SceneAccess, _app: &mut App) {
        println!("'{other}' entered the {}", self.name);
    }

    fn on_sensor_exit(&mut self, _node: &mut Node, other: &str, _scene: &mut SceneAccess, _app: &mut App) {
        println!("'{other}' left the {}", self.name);
    }
}
