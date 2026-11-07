use nalgebra::{Matrix4, UnitQuaternion, Vector2, Vector3};

/// A 3D position/rotation/scale - a node in the 3D world. A `Node`
/// property, like any other:
///
///     Node::new("plane").with_property(Transform3D { position, ..Default::default() })
///
/// A node is in the 3D world or on the 2D plane, never both: putting this
/// on takes a `Transform2D` off.
///
/// The engine's one 3D transform - the renderer's too: where a mesh sits in
/// its model, a camera, a drawn instance.
#[derive(Debug, Clone, Copy)]
pub struct Transform3D {
    pub position: Vector3<f32>,
    pub rotation: UnitQuaternion<f32>,
    pub scale: Vector3<f32>,
}

impl Default for Transform3D {
    fn default() -> Self {
        Transform3D {
            position: Vector3::zeros(),
            rotation: UnitQuaternion::identity(),
            scale: Vector3::new(1.0, 1.0, 1.0),
        }
    }
}

impl Transform3D {
    /// Scaled, then turned, then moved - as a matrix.
    pub fn matrix(&self) -> Matrix4<f32> {
        Matrix4::new_translation(&self.position) * self.rotation.to_homogeneous() * Matrix4::new_nonuniform_scaling(&self.scale)
    }

    /// At the origin, turned so its +Z points along `direction` (+Y as close
    /// to up as it can) - for a node whose facing is what matters: a
    /// directional `Light`, a camera. No direction: not turned.
    pub fn facing(direction: Vector3<f32>) -> Self {
        let rotation = crate::scene_manager::scene_nodes::facing(direction).unwrap_or_else(UnitQuaternion::identity);
        Self { rotation, ..Default::default() }
    }

    /// Where a child sitting at `local` from this transform (its parent) is
    /// in the world - see `Parent`. The offset is in meters, turned with the
    /// parent when `inherit_rotation` (otherwise along the world's axes, and
    /// the child keeps its own rotation as is); the parent's scale touches
    /// neither, and the child keeps its own scale.
    pub fn compose(&self, local: &Transform3D, inherit_rotation: bool) -> Transform3D {
        if inherit_rotation {
            Transform3D {
                position: self.position + self.rotation * local.position,
                rotation: self.rotation * local.rotation,
                scale: local.scale,
            }
        } else {
            Transform3D { position: self.position + local.position, rotation: local.rotation, scale: local.scale }
        }
    }

    /// The reverse of `compose`: what `local` puts a child at `world`, from
    /// this transform.
    pub fn relative(&self, world: &Transform3D, inherit_rotation: bool) -> Transform3D {
        if inherit_rotation {
            let inverse = self.rotation.inverse();
            Transform3D {
                position: inverse * (world.position - self.position),
                rotation: inverse * world.rotation,
                scale: world.scale,
            }
        } else {
            Transform3D { position: world.position - self.position, rotation: world.rotation, scale: world.scale }
        }
    }
}

/// A 2D position/rotation/scale - a node on the screen or a 2D plane
/// rather than in the 3D world. A `Node` property - putting it on takes a
/// `Transform3D` off (see `Transform3D`). Position in UI units
/// from the top left (see `Ui::scale_factor`), rotation in radians,
/// clockwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2D {
    pub position: Vector2<f32>,
    pub rotation: f32,
    pub scale: Vector2<f32>,
    /// Which draws over which - higher over lower.
    pub z_index: i32,
}

impl Default for Transform2D {
    fn default() -> Self {
        Transform2D { position: Vector2::zeros(), rotation: 0.0, scale: Vector2::new(1.0, 1.0), z_index: 0 }
    }
}

impl Transform2D {
    /// Where a child sitting at `local` from this transform (its parent) is
    /// on the plane - see `Parent`. The offset is in UI units, turned with
    /// the parent when `inherit_rotation` (otherwise along the plane's axes,
    /// and the child keeps its own rotation as is); the parent's scale and
    /// z index touch neither.
    pub fn compose(&self, local: &Transform2D, inherit_rotation: bool) -> Transform2D {
        if inherit_rotation {
            Transform2D {
                position: self.position + turned(local.position, self.rotation),
                rotation: self.rotation + local.rotation,
                ..*local
            }
        } else {
            Transform2D { position: self.position + local.position, ..*local }
        }
    }

    /// The reverse of `compose`: what `local` puts a child at `world`, from
    /// this transform.
    pub fn relative(&self, world: &Transform2D, inherit_rotation: bool) -> Transform2D {
        if inherit_rotation {
            Transform2D {
                position: turned(world.position - self.position, -self.rotation),
                rotation: world.rotation - self.rotation,
                ..*world
            }
        } else {
            Transform2D { position: world.position - self.position, ..*world }
        }
    }

    /// Where a visual (`Shape2D`, `Text`, `Line`) with its own `local`
    /// transform on a node at this one ends up. Unlike a child node, it
    /// scales with the node, and its z index is added to the node's.
    /// `upright`: it doesn't turn with the node.
    pub(crate) fn place_visual(&self, local: &Transform2D, upright: bool) -> Transform2D {
        let offset = local.position.component_mul(&self.scale);
        let (position, rotation) = if upright {
            (self.position + offset, local.rotation)
        } else {
            (self.position + turned(offset, self.rotation), self.rotation + local.rotation)
        };
        Transform2D { position, rotation, scale: self.scale.component_mul(&local.scale), z_index: self.z_index + local.z_index }
    }
}

/// `point` turned `radians` clockwise (y down) around the origin.
fn turned(point: Vector2<f32>, radians: f32) -> Vector2<f32> {
    let (sin, cos) = radians.sin_cos();
    Vector2::new(point.x * cos - point.y * sin, point.x * sin + point.y * cos)
}

/// Degrees about X, Y and Z (the map file's convention) to a rotation.
pub fn rotation_from_degrees(degrees: Vector3<f32>) -> UnitQuaternion<f32> {
    UnitQuaternion::from_euler_angles(degrees.x.to_radians(), degrees.y.to_radians(), degrees.z.to_radians())
}

/// The reverse of `rotation_from_degrees`.
pub fn degrees_from_rotation(rotation: &UnitQuaternion<f32>) -> Vector3<f32> {
    let (x, y, z) = rotation.euler_angles();
    Vector3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent() -> Transform3D {
        Transform3D { position: Vector3::new(100.0, 50.0, -20.0), rotation: UnitQuaternion::from_euler_angles(0.2, 1.1, -0.4), scale: Vector3::new(3.0, 3.0, 3.0) }
    }

    #[test]
    fn a_local_offset_turns_with_the_parent_and_ignores_its_scale() {
        let local = Transform3D { position: Vector3::new(0.0, 0.0, -10.0), ..Default::default() };
        let world = parent().compose(&local, true);
        assert!(((world.position - parent().position).magnitude() - 10.0).abs() < 1e-4);
        assert!((world.position - (parent().position + parent().rotation * local.position)).magnitude() < 1e-4);
    }

    #[test]
    fn a_2d_offset_turns_with_the_parent() {
        let parent = Transform2D { position: Vector2::new(100.0, 50.0), rotation: std::f32::consts::FRAC_PI_2, ..Default::default() };
        let local = Transform2D { position: Vector2::new(10.0, 0.0), ..Default::default() };
        // A quarter turn clockwise (y down): +x becomes +y.
        assert!((parent.compose(&local, true).position - Vector2::new(100.0, 60.0)).magnitude() < 1e-4);
        assert!((parent.compose(&local, false).position - Vector2::new(110.0, 50.0)).magnitude() < 1e-4);
    }

    #[test]
    fn a_visual_turns_scales_and_adds_its_z_index() {
        let node = Transform2D { position: Vector2::new(100.0, 50.0), rotation: std::f32::consts::FRAC_PI_2, scale: Vector2::new(2.0, 2.0), z_index: 3 };
        let local = Transform2D { position: Vector2::new(10.0, 0.0), rotation: 0.25, z_index: 1, ..Default::default() };
        let placed = node.place_visual(&local, false);
        assert!((placed.position - Vector2::new(100.0, 70.0)).magnitude() < 1e-4);
        assert!((placed.rotation - (std::f32::consts::FRAC_PI_2 + 0.25)).abs() < 1e-6);
        assert_eq!((placed.scale, placed.z_index), (Vector2::new(2.0, 2.0), 4));

        let upright = node.place_visual(&local, true);
        assert!((upright.position - Vector2::new(120.0, 50.0)).magnitude() < 1e-4);
        assert_eq!(upright.rotation, 0.25);
    }

    #[test]
    fn relative_undoes_compose_in_2d() {
        let parent = Transform2D { position: Vector2::new(100.0, 50.0), rotation: 0.7, scale: Vector2::new(3.0, 3.0), z_index: 2 };
        let world = Transform2D { position: Vector2::new(-20.0, 8.0), rotation: 1.9, scale: Vector2::new(1.0, 1.0), z_index: 5 };
        for inherit_rotation in [true, false] {
            let local = parent.relative(&world, inherit_rotation);
            let back = parent.compose(&local, inherit_rotation);
            assert!((back.position - world.position).magnitude() < 1e-3, "{inherit_rotation}");
            assert!((back.rotation - world.rotation).abs() < 1e-5, "{inherit_rotation}");
            assert_eq!(back.z_index, world.z_index);
        }
    }

    #[test]
    fn relative_undoes_compose() {
        let world = Transform3D { position: Vector3::new(7.0, -3.0, 12.0), rotation: UnitQuaternion::from_euler_angles(-0.5, 0.3, 2.0), scale: Vector3::new(1.0, 1.0, 1.0) };
        for inherit_rotation in [true, false] {
            let local = parent().relative(&world, inherit_rotation);
            let back = parent().compose(&local, inherit_rotation);
            assert!((back.position - world.position).magnitude() < 1e-3, "{inherit_rotation}");
            assert!(back.rotation.angle_to(&world.rotation) < 1e-4, "{inherit_rotation}");
        }
    }
}
