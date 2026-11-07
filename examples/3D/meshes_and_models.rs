//! `Mesh` and `Model`: one piece of geometry, and an object made of parts.
//!
//! - a row of every shape `Mesh` has, each a plain color (`Material::color`);
//! - a tank built from parts - its turret turns (`part_mut`), and an antenna
//!   is added and taken off every second (`add_part` / `remove_part`);
//! - the F-16 file, with a part of its own added: a sphere under it, and
//!   the whole model scaled by its `transform`.
//!
//! See docs/meshes_and_models.md.

use pankarta_software::prelude::*;

const GROUND_TEXTURE: &str = "textures/grid/Dark/texture_01.png";

fn at(position: Vector3<f32>) -> Transform3D {
    Transform3D { position, ..Default::default() }
}

/// Takes the camera around the "focus" node.
struct Orbit {
    angle: f32,
}

impl Behavior for Orbit {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.angle += 0.15 * app.time.delta_time;
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            transform.position = Vector3::new(self.angle.sin() * 22.0, 9.0, self.angle.cos() * 22.0);
        }
    }
}

/// Turns the tank's turret, and puts its antenna on and off.
struct Tank {
    time: f32,
}

impl Behavior for Tank {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.time += app.time.delta_time;
        let Some(model) = node.get_property_mut::<Model>() else { return };
        if let Some(turret) = model.part_mut("turret") {
            turret.transform.rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.time * 0.8);
        }
        let antenna_up = (self.time as u32) % 2 == 0;
        match (antenna_up, model.get_part("antenna").is_some()) {
            (true, false) => model.add_part("antenna", Mesh::cylinder(0.05, 2.0)
                .transform(at(Vector3::new(0.6, 2.2, -0.8)))
                .material(Material::standard().color(UiColor::Hex("#FFD166")))),
            (false, true) => {
                model.remove_part("antenna");
            }
            _ => {}
        }
    }
}

fn shapes() -> Vec<Node> {
    let shapes = [
        ("cube", Mesh::cube(), "#F63B3B"),
        ("sphere", Mesh::sphere(0.6), "#3B82F6"),
        ("cylinder", Mesh::cylinder(0.5, 1.4), "#7CFFB2"),
        ("capsule", Mesh::capsule(0.4, 1.6), "#FFD166"),
        ("plane", Mesh::plane(0), "#C084FC"),
    ];
    shapes.into_iter().enumerate().map(|(index, (name, mesh, color))| {
        Node::new(name)
            .with_property(at(Vector3::new(-6.0 + index as f32 * 3.0, 1.0, 6.0)))
            .with_property(mesh.material(Material::standard().color(UiColor::Hex(color))))
    }).collect()
}

fn tank() -> Node {
    let green = |hex| Material::standard().color(UiColor::Hex(hex));
    Node::new("tank")
        .with_property(at(Vector3::new(-5.0, 0.0, -3.0)))
        .with_property(Model::new()
            .material(green("#4D7C4A"))
            .part("body", Mesh::cube().transform(Transform3D { position: Vector3::y() * 0.6, scale: Vector3::new(3.0, 1.0, 4.5), ..Default::default() }))
            .part("turret", Mesh::cylinder(1.0, 0.7).transform(at(Vector3::y() * 1.45)))
            .part("barrel", Mesh::cylinder(0.12, 2.5)
                .transform(Transform3D {
                    position: Vector3::new(0.0, 1.45, 2.0),
                    rotation: UnitQuaternion::from_axis_angle(&Vector3::x_axis(), std::f32::consts::FRAC_PI_2),
                    ..Default::default()
                })
                .material(green("#2F4F2D"))))
        .with_behavior(Tank { time: 0.0 })
}

fn jet() -> Node {
    Node::new("jet")
        .with_property(at(Vector3::new(5.0, 3.0, -3.0)))
        .with_property(Model::file("models/f16.glb")
            .transform(Transform3D { scale: Vector3::new(8.0, 8.0, 8.0), ..Default::default() })
            .part("pod", Mesh::sphere(0.05).transform(at(Vector3::new(0.0, -0.08, 0.0))).material(Material::standard().color(UiColor::Hex("#FF3B3B")))))
}

struct MeshesAndModels;

impl MeshesAndModels {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::ProceduralSky { sun: true });
        let ground = Node::new("ground")
            .with_property(Transform3D::default())
            .with_property(Mesh::plane(0)
                .transform(Transform3D { scale: Vector3::new(200.0, 1.0, 200.0), ..Default::default() })
                .material(Material::standard().texture(GROUND_TEXTURE).tiled(4.0, TextureSpace::World)));
        let sun = Node::new("sun")
            .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.2)))
            .with_property(Light::directional([1.0, 1.0, 1.0]));
        let focus = Node::new("focus").with_property(at(Vector3::new(0.0, 1.0, 1.0)));
        let camera = Node::new("camera")
            .with_property(Transform3D::default())
            .with_property(CameraLens::new(60.0))
            .with_property(LookAt { target: "focus".into() })
            .with_behavior(Orbit { angle: 0.4 });

        let nodes = [ground, sun, focus, camera, tank(), jet()].into_iter().chain(shapes());
        for node in nodes {
            if let Err(error) = scene.spawn_node(app, node) {
                eprintln!("{error}");
            }
        }
        // The antenna isn't on the tank yet - loaded with the scene anyway.
        scene.will_use(Mesh::cylinder(0.05, 2.0));
        Self
    }
}

impl SceneBehaviour for MeshesAndModels {}

#[tokio::main]
async fn main() {
    let mut app = match App::new().await {
        Ok(app) => app,
        Err(err) => {
            eprintln!("Couldn't create the app: {err}");
            return;
        }
    };
    app.window.set_title("Meshes and models");

    app.scene_manager.create_scene("main", MeshesAndModels::new);
    app.scene_manager.open_scene("main");
    app.run();
}
