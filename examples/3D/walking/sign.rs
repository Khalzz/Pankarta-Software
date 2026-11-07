//! The sign glass: a big pane of glass on two posts, with a neon sign
//! floating behind it that's only seen through it. The sign is drawn by the
//! glass's shader (sign_glass.wgsl / .ron) from its image. See
//! docs/sign_glass.md.

use nalgebra::Vector3;
use pankarta_software::prelude::*;

/// The sign's image, in the assets folder.
const SIGN_IMAGE: &str = "textures/signs/neon_sign.png";
/// How high the glass's middle is, and its size (m).
const GLASS_HEIGHT: f32 = 1.8;
const GLASS_SIZE: (f32, f32) = (3.2, 1.8);

/// The sign glass, standing at `position` (on the ground), turned `yaw`
/// radians around Y. You look through it along its +Z.
pub fn sign_glass(id: &str, position: Vector3<f32>, yaw: f32) -> Node {
    let rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw);
    Node::new(id)
        .with_property(Transform3D { position, rotation, ..Default::default() })
        .with_property(Model::new()
            .part("left_post", post(-1.0))
            .part("right_post", post(1.0))
            .part("glass", glass(position, rotation)))
        .with_property(Physics { rigidbody: RigidBodyData::fixed(), colliders: vec![post_collider(-1.0), post_collider(1.0), glass_collider()] })
}

/// A thin box where the pane is, so you can't walk through it.
fn glass_collider() -> Collider {
    Collider::new(ColliderShape::Cuboid { half_extents: (GLASS_SIZE.0 / 2.0, GLASS_SIZE.1 / 2.0, 0.03) }).offset(Vector3::y() * GLASS_HEIGHT)
}

/// The pane - a plane (which faces up) turned to stand facing back along
/// -Z, the side you look through it from. Its shader is told where the glass
/// is and which way it points, once: it stands still.
fn glass(position: Vector3<f32>, rotation: UnitQuaternion<f32>) -> Mesh {
    let center = position + rotation * (Vector3::y() * GLASS_HEIGHT);
    let forward = rotation * Vector3::z();
    let up = rotation * Vector3::y();
    let right = forward.cross(&up);
    let material = Material::shader("sign_glass")
        .texture(SIGN_IMAGE)
        .param("center", [center.x, center.y, center.z])
        .param("forward", [forward.x, forward.y, forward.z])
        .param("up", [up.x, up.y, up.z])
        .param("right", [right.x, right.y, right.z]);

    Mesh::plane(0)
        .transform(Transform3D {
            position: Vector3::y() * GLASS_HEIGHT,
            rotation: UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -std::f32::consts::FRAC_PI_2),
            scale: Vector3::new(GLASS_SIZE.0, 1.0, GLASS_SIZE.1),
        })
        .material(material)
        .casts_shadows(false)
}

fn post_height() -> f32 {
    GLASS_HEIGHT - GLASS_SIZE.1 / 2.0
}

/// Where the post on `side` (-1 left, 1 right) stands, from the glass's foot.
fn post_position(side: f32) -> Vector3<f32> {
    Vector3::new(side * (GLASS_SIZE.0 / 2.0 - 0.1), post_height() / 2.0, 0.0)
}

fn post(side: f32) -> Mesh {
    Mesh::cylinder(0.05, post_height())
        .transform(Transform3D { position: post_position(side), ..Default::default() })
        .material(Material::standard().color(UiColor::Hex("#1C1C24")))
}

fn post_collider(side: f32) -> Collider {
    Collider::new(ColliderShape::Cuboid { half_extents: (0.06, post_height() / 2.0, 0.06) }).offset(post_position(side))
}
