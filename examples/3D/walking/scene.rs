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
use crate::sensor_gallery::{sensor_gallery, SENSORS};
use crate::sign::sign_glass;

pub struct WalkingScene {
    /// Which of AMBIENT_GROUND_LEVELS is in use - H cycles them.
    ambient_ground: usize,
}

/// How much of the ambient light reaches a surface facing down (see
/// Renderer::set_ambient_ground) - H steps through these to compare: 1 is
/// flat (every way the same), lower darkens undersides and downward faces.
const AMBIENT_GROUND_LEVELS: [(f32, &str); 4] = [
    (1.0, "flat"),
    (0.7, "light"),
    (0.45, "default"),
    (0.25, "strong"),
];
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

/// Colliders and no body: it stays put.
fn static_box(size: Vector3<f32>) -> Colliders {
    Colliders::new().add(ColliderBuilder::cuboid(size.x / 2.0, size.y / 2.0, size.z / 2.0))
}

impl WalkingScene {
    pub fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer
            .set_environment(Environment::ProceduralSky { sun: true });

        let ground = Node::new("ground")
            .with_property(Transform3D::default())
            .with_property(
                Mesh::plane(0)
                    .transform(Transform3D {
                        scale: Vector3::new(200.0, 1.0, 200.0),
                        ..Default::default()
                    })
                    .material(
                        Material::standard()
                            .texture(GROUND_TEXTURE)
                            .tiled(4.0, TextureSpace::World),
                    ),
            )
            .with_property(Colliders::new().add(ColliderBuilder::halfspace(Vector3::y_axis())));

        // A big static jet to walk on: its exact surface, every mesh of the
        // file merged into one triangle mesh - fine for a body that never
        // moves.
        let f16 = Model::file("models/f16.glb");
        let plane = Node::new("plane")
            .with_property(Transform3D {
                position: Vector3::new(0.0, 2.0, 0.0),
                scale: Vector3::new(14.0, 14.0, 14.0),
                ..Default::default()
            })
            .with_property(Colliders::new().add(Collider::from_model(&f16, Meshes::all(), ModelShape::Trimesh)))
            .with_property(f16);

        // A small jet that falls and can be pushed around: a moving body
        // can't use a triangle mesh, so each of its main parts is its own
        // convex hull (`.each()`) - wings, tail and wheels stay apart instead
        // of being wrapped into one blob. Its afterburner flame isn't drawn.
        let toy_model = Model::file("models/f16.glb").hide_meshes(Meshes::named(["Afterburner"]));
        let toy_parts = Meshes::named([
            "MainBody.001",
            "left_aleron",
            "right_aleron",
            "left_elevator",
            "right_elevator",
            "rudder_0",
            "wheel-f",
            "wheel-lb",
            "wheel-rb",
        ]);
        let toy_jet = Node::new("toy_jet")
            .with_property(Transform3D {
                position: Vector3::new(10.0, 6.0, -12.0),
                rotation: UnitQuaternion::from_axis_angle(&Vector3::y_axis(), 0.6),
                scale: Vector3::new(3.0, 3.0, 3.0),
            })
            // Each of its 9 parts weighs 5 kg: Rapier works out where its
            // center of mass is and how it spins from where they are.
            .with_property(RigidBodyBuilder::dynamic().angular_damping(2.0))
            .with_property(Colliders::new()
                .add(Collider::from_model(&toy_model, toy_parts.each(), ModelShape::ConvexHull).mass(5.0).friction(0.8)))
            .with_property(toy_model);

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


        // A barrel to push around: a 12-sided cylinder, drawn flat-sided like
        // Blender's, and colliding with exactly the mesh it's drawn with.
        let barrel = Node::new("barrel")
            .with_property(Transform3D { position: Vector3::new(-2.0, 0.6, -30.0), ..Default::default() })
            .with_property(Mesh::cylinder_shape(0.5, 1.2, 12).material(Material::standard().color(UiColor::Rgb(170, 90, 40))))
            .with_property(RigidBodyBuilder::dynamic().angular_damping(1.0))
            .with_property(Colliders::new().add(Collider::from_own_mesh(ModelShape::ConvexHull).mass(30.0)));

        // A stool built from parts - a seat and three legs - each part its own
        // convex collider (`.each()`), so you can see under it.
        let leg = |angle: f32| {
            let (sin, cos) = angle.to_radians().sin_cos();
            Mesh::cylinder_shape(0.05, 0.6, 6).transform(Transform3D { position: Vector3::new(cos * 0.3, 0.3, sin * 0.3), ..Default::default() })
        };
        let stool_model = Model::new()
            .part("seat", Mesh::cylinder_shape(0.4, 0.08, 16).transform(Transform3D { position: Vector3::new(0.0, 0.64, 0.0), ..Default::default() }))
            .part("leg_a", leg(90.0))
            .part("leg_b", leg(210.0))
            .part("leg_c", leg(330.0))
            .material(Material::standard().color(UiColor::Rgb(200, 170, 120)));
        let stool = Node::new("stool")
            .with_property(Transform3D { position: Vector3::new(2.0, 0.0, -30.0), ..Default::default() })
            .with_property(RigidBodyBuilder::dynamic().angular_damping(1.0))
            .with_property(Colliders::new()
                .add(Collider::from_model(&stool_model, Meshes::all().each(), ModelShape::ConvexHull).mass(1.5)))
            .with_property(stool_model);

        // Profile shapes: each turned around Y through its rings (height,
        // radius) - like a Blender cylinder with loop cuts moved and scaled.
        // Heights are from the node, so each stands on the ground at y 0.
        let profiled = |id: &str, x: f32, mesh: Mesh, shape: ModelShape, mass: f32| {
            Node::new(id)
                .with_property(Transform3D { position: Vector3::new(x, 0.0, -30.0), ..Default::default() })
                .with_property(mesh)
                .with_property(RigidBodyBuilder::dynamic().angular_damping(1.0))
                .with_property(Colliders::new().add(Collider::from_own_mesh(shape).mass(mass)))
        };
        // A thin base plate, a step in, then up to a point.
        let traffic_cone = profiled(
            "traffic_cone",
            -5.0,
            Mesh::profile_shape(16)
                .ring(0.0, 0.4)
                .ring(0.05, 0.4)
                .ring(0.05, 0.3)
                .ring(0.8, 0.0)
                .material(Material::standard().color(UiColor::Rgb(255, 120, 20))),
            ModelShape::ConvexHull,
            3.0,
        );
        // Wider on top, with a rim.
        let bucket = profiled(
            "bucket",
            5.0,
            Mesh::profile_shape(20)
                .ring(0.0, 0.25)
                .ring(0.55, 0.35)
                .ring(0.6, 0.37)
                .material(Material::standard().color(UiColor::Rgb(90, 140, 200))),
            ModelShape::ConvexHull,
            2.0,
        );
        // Dented in at the waist - a convex hull would fill the waist, so
        // it's cut into convex pieces that keep it.
        let vase = profiled(
            "vase",
            8.0,
            Mesh::profile_shape(24)
                .ring(0.0, 0.2)
                .ring(0.1, 0.3)
                .ring(0.45, 0.32)
                .ring(0.8, 0.12)
                .ring(1.0, 0.18)
                .material(Material::standard().color(UiColor::Rgb(170, 90, 160))),
            ModelShape::ConvexDecomposition,
            4.0,
        );
        // A jet's nose, laid forward (its Y toward -Z, its Z up): each
        // ring's offset droops it toward its tip (a negative z is down) and
        // its stretch makes it flatter than it's wide. It bulges everywhere,
        // so its convex hull fits it. Fixed in place.
        let nose = Node::new("nose")
            .with_property(Transform3D {
                position: Vector3::new(11.0, 0.7, -28.0),
                rotation: UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -std::f32::consts::FRAC_PI_2),
                ..Default::default()
            })
            .with_property(
                Mesh::profile_shape(24)
                    .ring(0.0, 0.6).stretch(1.0, 0.85)
                    .ring(0.8, 0.48).offset(0.0, -0.03).stretch(1.0, 0.85)
                    .ring(1.6, 0.3).offset(0.0, -0.12).stretch(1.0, 0.95)
                    .ring(2.2, 0.0).offset(0.0, -0.3)
                    .material(Material::standard().color(UiColor::Rgb(150, 155, 165))),
            )
            .with_property(Colliders::new().add(Collider::from_own_mesh(ModelShape::ConvexHull)));

        let mut nodes = vec![ground, player, plane, toy_jet, barrel, stool, traffic_cone, bucket, vase, nose, sun, glass];
        // Sensors of every shape, outside the cubes - see sensor_gallery.rs.
        nodes.extend(sensor_gallery());

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
                            .transform(Transform3D {
                                scale: size,
                                ..Default::default()
                            })
                            .material(
                                Material::standard()
                                    .texture(CUBE_TEXTURES[index % 4])
                                    .tiled(1.0, TextureSpace::Object),
                            ),
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
                .set_child(
                    "ambient_ground",
                    text(
                        &ambient_ground_text(DEFAULT_AMBIENT_GROUND),
                        &TextStyle::default(),
                    )
                    .set_padding((10.0, 4.0))
                    .set_size(SizeValue::Pixels(500.0), SizeValue::Fit),
                )
                .set_child(
                    "zone",
                    text("", &TextStyle::default())
                        .set_padding((10.0, 4.0))
                        .set_size(SizeValue::Pixels(500.0), SizeValue::Fit),
                ),
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
        app.renderer
            .set_ambient_ground(AMBIENT_GROUND_LEVELS[DEFAULT_AMBIENT_GROUND].0);
        Self {
            ambient_ground: DEFAULT_AMBIENT_GROUND,
        }
    }
}

impl SceneBehaviour for WalkingScene {
    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        if app.input.is_just_pressed(InputSource::key(Key::H)) {
            self.ambient_ground = (self.ambient_ground + 1) % AMBIENT_GROUND_LEVELS.len();
            app.renderer
                .set_ambient_ground(AMBIENT_GROUND_LEVELS[self.ambient_ground].0);
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
                let inside: Vec<&str> = SENSORS
                    .iter()
                    .filter(|(id, _)| app.physics.is_inside(id, "player"))
                    .map(|(_, name)| *name)
                    .collect();
                label.set_text(if inside.is_empty() {
                    String::new()
                } else {
                    format!("Inside: {}", inside.join(", "))
                });
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
