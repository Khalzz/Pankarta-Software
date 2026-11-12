use nalgebra::Vector3;
use pankarta_software::prelude::*;
use pankarta_software::rendering::frame::FramePlan;
use pankarta_software::rendering::sdf_text::TextFrame;
use pankarta_software::rendering::thick_lines::LineInstance;
use pankarta_software::ui::components::label::Font;
use pankarta_software::ui::ui_node::UiNode;
use pankarta_software::ui::ui_transform::SizeValue;
use pankarta_software::ui::widgets::text;
use pankarta_software::ui::TextStyle;

use crate::player::Player;
use crate::sign::sign_glass;
use crate::zone::zone;

pub struct WalkingScene {
    /// Which of AMBIENT_GROUND_LEVELS is in use - H cycles them.
    ambient_ground: usize,
}

/// How much of the ambient light reaches a surface facing down (see
/// Renderer::set_ambient_ground) - H steps through these to compare: 1 is
/// flat (every way the same), lower darkens undersides and downward faces.
const AMBIENT_GROUND_LEVELS: [(f32, &str); 4] = [(1.0, "flat"), (0.7, "light"), (0.45, "default"), (0.25, "strong")];
const DEFAULT_AMBIENT_GROUND: usize = 2;

fn ambient_ground_text(level: usize) -> String {
    let (value, name) = AMBIENT_GROUND_LEVELS[level];
    format!("H: ambient from below ({name}, {value})")
}

const GROUND_TEXTURE: &str = "textures/grid/Dark/texture_01.png";
const CUBE_TEXTURES: [&str; 4] = [
    "textures/grid/Orange/texture_01.png",
    "textures/grid/Green/texture_01.png",
    "textures/grid/Purple/texture_01.png",
    "textures/grid/Red/texture_01.png",
];

const PLAYER_HALF_HEIGHT: f32 = 0.9;

const AIM_LINE: &str = "aim";
const PLANE_NAME: &str = "plane_name";
/// How far over the plane's middle its name floats (m).
const PLANE_NAME_HEIGHT: f32 = 4.0;

fn static_box(size: Vector3<f32>) -> Physics {
    Physics {
        rigidbody: RigidBodyData::fixed(),
        colliders: vec![Collider::new(ColliderShape::Cuboid {
            half_extents: (size.x / 2.0, size.y / 2.0, size.z / 2.0),
        })],
    }
}

impl WalkingScene {
    pub fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer
            .set_environment(Environment::ProceduralSky { sun: true });

        let ground = Node::new("ground")
            .with_property(Transform3D::default())
            .with_property(
                Mesh::plane(0)
                    .transform(Transform3D { scale: Vector3::new(200.0, 1.0, 200.0), ..Default::default() })
                    .material(Material::standard().texture(GROUND_TEXTURE).tiled(4.0, TextureSpace::World)),
            )
            .with_property(Physics {
                rigidbody: RigidBodyData {
                    is_static: true,
                    mass: 0.0,
                    ..Default::default()
                },
                colliders: vec![Collider::new(ColliderShape::HalfSpace {
                    normal: Vector3::y(),
                })],
            });

        let plane = Node::new("plane")
            .with_property(Transform3D {
                position: Vector3::new(0.0, 2.0, 0.0),
                scale: Vector3::new(14.0, 14.0, 14.0),
                ..Default::default()
            })
            .with_property(Physics {
                rigidbody: RigidBodyData {
                    is_static: true,
                    mass: 0.0,
                    ..Default::default()
                },
                colliders: vec![Collider::new(ColliderShape::TrimeshFromModel {
                    model_path: "models/f16.glb".to_owned(),
                })],
            })
            .with_property(Model::file("models/f16.glb"));

        let player = Node::new("player").with_behavior(Player::new(Vector3::new(
            0.0,
            PLAYER_HALF_HEIGHT + 0.5,
            -10.0,
        )));

        let sun = Node::new("sun")
            .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.2)))
            .with_property(Light::directional([1.0, 1.0, 1.0]).shadows(true));

        // Walk up behind it and look through it.
        let glass = sign_glass("sign_glass", Vector3::new(3.0, 0.0, -6.0), 0.0);

        // Walk onto it - see zone.rs.
        let pad = zone("zone", Vector3::new(-4.0, 0.0, -6.0));

        let mut nodes = vec![ground, player, plane, sun, glass, pad];

        for index in 0..12 {
            let angle = index as f32 / 12.0 * std::f32::consts::TAU;
            let height = 0.5 + (index % 4) as f32 * 0.75;
            let size = Vector3::new(2.0, height, 2.0);
            nodes.push(
                Node::new(format!("cube_{index}"))
                    .with_property(Transform3D {
                        position: Vector3::new(
                            angle.sin() * 12.0,
                            height / 2.0,
                            angle.cos() * 12.0,
                        ),
                        ..Default::default()
                    })
                    .with_property(
                        Mesh::cube()
                            .transform(Transform3D { scale: size, ..Default::default() })
                            .material(Material::standard().texture(CUBE_TEXTURES[index % 4]).tiled(1.0, TextureSpace::Object)),
                    )
                    .with_property(static_box(size)),
            );
        }
        for node in nodes {
            if let Err(error) = scene.spawn_node(app, node) {
                eprintln!("{error}");
            }
        }

        // The frame rate, top left.
        let framerate = text("- fps", &TextStyle::default())
            .set_padding((10.0, 10.0))
            .set_size(SizeValue::Pixels(200.0), SizeValue::Fit);
        let hud = Node::new("hud").with_property(UiLayer::new(
            UiNode::container()
                .set_child("framerate", framerate)
                .set_child("ambient_ground", text(&ambient_ground_text(DEFAULT_AMBIENT_GROUND), &TextStyle::default())
                    .set_padding((10.0, 4.0))
                    .set_size(SizeValue::Pixels(500.0), SizeValue::Fit))
                .set_child("zone", text("", &TextStyle::default())
                    .set_padding((10.0, 4.0))
                    .set_size(SizeValue::Pixels(500.0), SizeValue::Fit)),
        ));
        if let Err(error) = scene.spawn_node(app, hud) {
            eprintln!("{error}");
        }

        // A line from the player to the plane and the plane's name, both in
        // the world - filled every frame in `update`.
        app.add_line_set(AIM_LINE, false);
        app.add_text_set(PLANE_NAME, false);
        // The sign glass is drawn in its own pass, after the solid world it's
        // seen against (see sign_glass.ron).
        app.renderer.frame_plan = FramePlan::engine_default()
            .insert_after_scene("opaque", |steps| steps.scene("hud"))
            .lines(AIM_LINE)
            .text(PLANE_NAME);

        app.window.set_cursor_locked(true);
        app.renderer.set_ambient_ground(AMBIENT_GROUND_LEVELS[DEFAULT_AMBIENT_GROUND].0);
        Self { ambient_ground: DEFAULT_AMBIENT_GROUND }
    }
}

impl SceneBehaviour for WalkingScene {
    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        if app.input.is_just_pressed(InputSource::key(Key::H)) {
            self.ambient_ground = (self.ambient_ground + 1) % AMBIENT_GROUND_LEVELS.len();
            app.renderer.set_ambient_ground(AMBIENT_GROUND_LEVELS[self.ambient_ground].0);
        }
        if let Some(ui) = scene
            .get_node_mut("hud")
            .and_then(|node| node.get_property_mut::<UiLayer>())
        {
            if let Some(label) = ui.label_mut("framerate") {
                let fps = app.time.fps().round();
                label.set_text(format!("{fps} fps"));
            }
            if let Some(label) = ui.label_mut("ambient_ground") {
                label.set_text(ambient_ground_text(self.ambient_ground));
            }
            if let Some(label) = ui.label_mut("zone") {
                let inside = app.physics.is_inside("zone", "player");
                label.set_text(if inside { "In the blue zone" } else { "" });
            }
        }

        let transform_of = |id: &str| {
            scene
                .get_node(id)
                .and_then(|node| node.get_property::<Transform3D>())
                .copied()
        };
        let (Some(player), Some(plane), Some(camera)) = (
            transform_of("player"),
            transform_of("plane"),
            transform_of("camera"),
        ) else {
            return;
        };

        let lines = app.renderer.render_sets.lines(AIM_LINE);
        lines.dynamic.clear();
        lines.dynamic.push(LineInstance {
            start: player.position.into(),
            end: plane.position.into(),
            color: [1.0, 0.0, 0.0, 1.0],
            width: 2.0,
        });

        // Facing the camera, which looks along its +Z with +Y up - so its
        // right is -X.
        let name = app.renderer.render_sets.text(PLANE_NAME);
        name.clear();
        name.push(
            "F-16",
            Font::default(),
            TextFrame::world(
                plane.position + Vector3::y() * PLANE_NAME_HEIGHT,
                camera.rotation * -Vector3::x(),
                camera.rotation * Vector3::y(),
                1.0,
            ),
            [1.0, 1.0, 1.0, 1.0],
        );
    }
}
