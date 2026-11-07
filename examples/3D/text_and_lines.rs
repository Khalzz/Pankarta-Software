//! `Text` and `Line`: properties of a node, in the 3D world or on the 2D
//! plane by which transform the node has. Nothing here fills or draws them -
//! the nodes are placed, and the text and lines go with them.
//!
//! In the world, with the camera going around it:
//! - a sign on a wall, turned with it, with a pillar hiding part of it;
//! - a tower with a name that always faces the camera;
//! - a drone with a name seen through everything, and a line from it to the
//!   tower (its end kept on the tower by a behaviour);
//! - a turret with a laser (both points local) turning with it;
//! - a line between two fixed places (`world`), seen through everything.
//!
//! On the plane, top left:
//! - a square going round with an `upright` name, and a line to a dot;
//! - a spinning label with a small square moved off it, turning around it;
//! - a spinning line (both points local);
//! - a label between two bars - in front of one, behind the other, by z index.
//!
//! See docs/shapes_and_lines.md.

use pankarta_software::prelude::*;

const GROUND_TEXTURE: &str = "textures/grid/Dark/texture_01.png";
const WALL_TEXTURE: &str = "textures/grid/Purple/texture_01.png";
const PILLAR_TEXTURE: &str = "textures/grid/Orange/texture_01.png";
const TOWER_TEXTURE: &str = "textures/grid/Green/texture_01.png";
const MOVER_TEXTURE: &str = "textures/grid/Red/texture_01.png";

/// Takes the camera around the "focus" node (it looks at it - see `LookAt`).
struct Orbit {
    angle: f32,
}

impl Behavior for Orbit {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.angle += 0.15 * app.time.delta_time;
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            transform.position = Vector3::new(self.angle.sin() * 24.0, 8.0, self.angle.cos() * 24.0);
        }
    }
}

/// Flies its node in a circle over the ground.
struct Drone {
    angle: f32,
}

impl Behavior for Drone {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.angle += 0.6 * app.time.delta_time;
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            transform.position = Vector3::new(self.angle.cos() * 9.0, 5.0, self.angle.sin() * 9.0);
        }
    }
}

/// Spins its node around Y.
struct Turret;

impl Behavior for Turret {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            transform.rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), 0.8 * app.time.delta_time) * transform.rotation;
        }
    }
}

/// Keeps its node's line ending at another node, wherever that is.
struct Tether {
    target: &'static str,
}

impl Behavior for Tether {
    fn update(&mut self, node: &mut Node, scene: &mut SceneAccess, _app: &mut App) {
        let Some(target) = scene.get_node(self.target) else { return };
        let (Some(line), Some(position)) = (node.get_property_mut::<Line>(), target.transform_3d().map(|t| t.position)) else { return };
        line.set_end(position);
    }
}

/// `Tether`, on the 2D plane.
struct Tether2D {
    target: &'static str,
}

impl Behavior for Tether2D {
    fn update(&mut self, node: &mut Node, scene: &mut SceneAccess, _app: &mut App) {
        let Some(target) = scene.get_node(self.target) else { return };
        let (Some(line), Some(position)) = (node.get_property_mut::<Line>(), target.transform_2d().map(|t| t.position)) else { return };
        line.set_end(position);
    }
}

/// Takes its 2D node round a circle, turning as it goes.
struct Rounds {
    center: Vector2<f32>,
    angle: f32,
}

impl Behavior for Rounds {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.angle += 1.0 * app.time.delta_time;
        if let Some(transform) = node.get_property_mut::<Transform2D>() {
            transform.position = self.center + Vector2::new(self.angle.cos(), self.angle.sin()) * 40.0;
            transform.rotation = self.angle;
        }
    }
}

/// Spins its 2D node.
struct Spin;

impl Behavior for Spin {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        if let Some(transform) = node.get_property_mut::<Transform2D>() {
            transform.rotation += 1.2 * app.time.delta_time;
        }
    }
}

fn cube(id: &str, position: Vector3<f32>, size: Vector3<f32>, texture: &str) -> Node {
    Node::new(id)
        .with_property(Transform3D { position, ..Default::default() })
        .with_property(Mesh::cube()
            .transform(Transform3D { scale: size, ..Default::default() })
            .material(Material::standard().texture(texture).tiled(1.0, TextureSpace::Object)))
}

fn nodes_3d() -> Vec<Node> {
    let ground = Node::new("ground")
        .with_property(Transform3D::default())
        .with_property(Mesh::plane(0)
            .transform(Transform3D { scale: Vector3::new(200.0, 1.0, 200.0), ..Default::default() })
            .material(Material::standard().texture(GROUND_TEXTURE).tiled(4.0, TextureSpace::World)));
    let sun = Node::new("sun")
        .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.2)))
        .with_property(Light::directional([1.0, 1.0, 1.0]));
    let focus = Node::new("focus").with_property(Transform3D { position: Vector3::new(0.0, 2.0, 0.0), ..Default::default() });
    let camera = Node::new("camera")
        .with_property(Transform3D::default())
        .with_property(CameraLens::new(60.0))
        .with_property(LookAt { target: "focus".into() })
        .with_behavior(Orbit { angle: 0.3 });

    // The wall's +Z side is the one the sign reads from. A child with its
    // own transform sits that far from its parent.
    let wall = cube("wall", Vector3::new(0.0, 2.5, -6.0), Vector3::new(16.0, 5.0, 0.5), WALL_TEXTURE)
        .with_child(Node::new("sign")
            .with_property(Transform3D { position: Vector3::new(0.0, 0.5, 0.3), ..Default::default() })
            .with_property(Text::new("Behind the pillar").size(1.5)));
    let pillar = cube("pillar", Vector3::new(0.0, 2.5, -3.5), Vector3::new(1.5, 5.0, 1.5), PILLAR_TEXTURE);

    let tower = cube("tower", Vector3::new(-8.0, 3.0, 2.0), Vector3::new(2.0, 6.0, 2.0), TOWER_TEXTURE)
        .with_child(Node::new("name")
            .with_property(Transform3D { position: Vector3::y() * 4.0, ..Default::default() })
            .with_property(Text::new("Tower").size(1.2).color(UiColor::Hex("#7CFFB2")).face_camera()));

    // No transform on the line's node: it sits right at the drone, so the
    // line starts there. Its end is a world point the behaviour keeps on the
    // tower.
    let drone = cube("drone", Vector3::new(9.0, 5.0, 0.0), Vector3::new(1.0, 0.4, 1.0), MOVER_TEXTURE)
        .with_behavior(Drone { angle: 0.0 })
        .with_child(Node::new("name")
            .with_property(Transform3D { position: Vector3::y() * 1.2, ..Default::default() })
            .with_property(Text::new("Drone").size(0.8).color(UiColor::Hex("#FFD166")).face_camera().always_on_top()))
        .with_child(Node::new("tether")
            .with_property(Line::new(Vector3::zeros(), Vector3::zeros()).end_in_world().width(3.0).color(UiColor::Hex("#FFD166")))
            .with_behavior(Tether { target: "tower" }));

    // Both points local: the laser turns with the turret.
    let turret = cube("turret", Vector3::new(8.0, 1.0, 4.0), Vector3::new(1.5, 2.0, 1.5), MOVER_TEXTURE)
        .with_behavior(Turret)
        .with_child(Node::new("laser").with_property(Line::new(Vector3::zeros(), Vector3::z() * 14.0).width(2.0).color(UiColor::Hex("#FF3B3B"))));

    // Both points in the world.
    let marker = Node::new("marker")
        .with_property(Transform3D::default())
        .with_property(Line::new(Vector3::new(-14.0, 0.2, 10.0), Vector3::new(14.0, 0.2, -12.0)).world().width(4.0).color(UiColor::Hex("#3B82F6")).always_on_top());

    vec![ground, sun, focus, camera, wall, pillar, tower, drone, turret, marker]
}

fn nodes_2d() -> Vec<Node> {
    let dot = Node::new("dot")
        .with_property(Transform2D { position: Vector2::new(340.0, 90.0), ..Default::default() })
        .with_property(Shape2D::circle(10.0).color(UiColor::Hex("#3B82F6")));

    // The name is on the square's own node, kept level with `upright`. A
    // child with no transform sits on its 2D parent - the line's start.
    let square = Node::new("square")
        .with_property(Transform2D { position: Vector2::new(120.0, 110.0), ..Default::default() })
        .with_property(Shape2D::square(44.0).color(UiColor::Hex("#F63B3B")))
        .with_property(Text::new("Stays upright").size(20.0).transform(Transform2D { position: Vector2::new(0.0, -42.0), ..Default::default() }).upright())
        .with_behavior(Rounds { center: Vector2::new(120.0, 110.0), angle: 0.0 })
        .with_child(Node::new("to_dot")
            .with_property(Line::new(Vector2::zeros(), Vector2::zeros()).end_in_world().width(3.0).color(UiColor::Hex("#3B82F6")))
            .with_behavior(Tether2D { target: "dot" }));

    // A shape moved off its node turns around it - a second, smaller square
    // in front (z index 1 over the node's).
    let spinner = Node::new("spinner")
        .with_property(Transform2D { position: Vector2::new(540.0, 110.0), ..Default::default() })
        .with_property(Text::new("Spinning").size(28.0).color(UiColor::Hex("#FFD166")))
        .with_property(Shape2D::square(14.0).color(UiColor::Hex("#7CFFB2")).transform(Transform2D { position: Vector2::new(80.0, 0.0), z_index: 1, ..Default::default() }))
        .with_behavior(Spin);

    // A 2D line with local points turns with its node.
    let hand = Node::new("hand")
        .with_property(Transform2D { position: Vector2::new(340.0, 190.0), ..Default::default() })
        .with_property(Line::new(Vector2::zeros(), Vector2::new(50.0, 0.0)).width(4.0).color(UiColor::Hex("#FFFFFF")))
        .with_behavior(Spin);

    // Whatever order they're added in, z index decides: back bar, then the
    // label, then the front bar.
    let front_bar = Node::new("front_bar")
        .with_property(Transform2D { position: Vector2::new(860.0, 100.0), z_index: 2, ..Default::default() })
        .with_property(Shape2D::rect(40.0, 90.0).color(UiColor::Hex("#3B82F6")));
    let label = Node::new("between")
        .with_property(Transform2D { position: Vector2::new(800.0, 100.0), z_index: 1, ..Default::default() })
        .with_property(Text::new("Between the bars").size(30.0));
    let back_bar = Node::new("back_bar")
        .with_property(Transform2D { position: Vector2::new(740.0, 100.0), ..Default::default() })
        .with_property(Shape2D::rect(40.0, 90.0).color(UiColor::Hex("#F63B3B")));

    vec![dot, square, spinner, hand, front_bar, label, back_bar]
}

struct TextAndLinesScene;

impl TextAndLinesScene {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::ProceduralSky { sun: true });
        for node in nodes_3d().into_iter().chain(nodes_2d()) {
            if let Err(error) = scene.spawn_node(app, node) {
                eprintln!("{error}");
            }
        }
        Self
    }
}

impl SceneBehaviour for TextAndLinesScene {}

#[tokio::main]
async fn main() {
    let mut app = match App::new().await {
        Ok(app) => app,
        Err(err) => {
            eprintln!("Couldn't create the app: {err}");
            return;
        }
    };
    app.window.set_title("Text and lines");

    app.scene_manager.create_scene("main", TextAndLinesScene::new);
    app.scene_manager.open_scene("main");
    app.run();
}
