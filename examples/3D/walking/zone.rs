//! A trigger zone: a pad on the ground with a `Sensor` over it that detects
//! the player (its `Layers`) walking in and out - its behavior hears it in
//! `on_sensor_enter`/`on_sensor_exit`, and the scene's HUD asks
//! `app.physics.is_inside(..)`. Its area is drawn (`Sensor::debug`): blue,
//! orange while the player's in it.

use nalgebra::Vector3;
use pankarta_software::prelude::*;

const RADIUS: f32 = 2.0;
const HEIGHT: f32 = 2.0;

/// The zone `id` on the ground at `position`.
pub fn zone(id: &str, position: Vector3<f32>) -> Node {
    Node::new(id)
        .with_property(Transform3D { position, ..Default::default() })
        .with_property(
            Mesh::cylinder(RADIUS, 0.05)
                .transform(Transform3D { position: Vector3::y() * 0.025, ..Default::default() })
                .material(Material::standard().color(UiColor::Rgb(60, 180, 255))),
        )
        // A cylinder standing on the pad, as tall as the player.
        .with_property(
            Sensor::cylinder(HEIGHT / 2.0, RADIUS)
                .offset(Vector3::y() * HEIGHT / 2.0)
                .detects(["player"])
                .debug(ColliderDebug::new()
                    .edges(UiColor::Rgb(60, 180, 255))
                    .faces(UiColor::Rgba(60, 180, 255, 35))
                    .occupied_edges(UiColor::Rgb(255, 160, 40))
                    .occupied_faces(UiColor::Rgba(255, 160, 40, 50))),
        )
        .with_behavior(Zone { visits: 0 })
}

struct Zone {
    visits: u32,
}

impl Behavior for Zone {
    fn on_sensor_enter(&mut self, node: &mut Node, other: &str, _scene: &mut SceneAccess, _app: &mut App) {
        self.visits += 1;
        println!("'{other}' entered '{}' (visit {})", node.id, self.visits);
    }

    fn on_sensor_exit(&mut self, node: &mut Node, other: &str, _scene: &mut SceneAccess, _app: &mut App) {
        println!("'{other}' left '{}'", node.id);
    }
}
