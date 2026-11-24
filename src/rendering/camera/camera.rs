use nalgebra::{Matrix4, Point3, UnitQuaternion, Vector3};

use crate::scene_manager::node::properties::Transform3D;

#[derive(Copy, Clone, Debug)]
pub struct Camera {
    pub transform: Transform3D,
    pub up: Vector3<f32>,
}

impl Camera {
    pub fn new(transform: Transform3D) -> Self {
        Self {
            transform,
            up: Vector3::y_axis().into_inner(),
        }
    }

    pub fn position(&self) -> Point3<f32> {
        Point3::from(self.transform.position)
    }

    pub fn set_position(&mut self, position: Point3<f32>) {
        self.transform.position = position.coords;
    }

    pub fn set_yaw_pitch(&mut self, yaw: f32, pitch: f32) {
        self.transform.rotation = yaw_pitch_rotation(yaw.to_degrees(), pitch.to_degrees());
    }

    /// Points it along `forward` with `up` as its up - a full orientation,
    /// roll included.
    pub fn set_orientation(&mut self, forward: Vector3<f32>, up: Vector3<f32>) {
        let Some(forward) = forward.try_normalize(1e-6) else { return };
        let pitch = forward.y.clamp(-1.0, 1.0).asin();
        let yaw = forward.z.atan2(forward.x);
        self.set_yaw_pitch(yaw, pitch);
        self.up = up;
    }

    pub fn calc_forward_direction(&self) -> Vector3<f32> {
        self.transform.rotation * Vector3::new(1.0, 0.0, 0.0)
    }

    pub fn calc_matrix(&self) -> Matrix4<f32> {
        let origin = Point3::origin();
        Matrix4::look_at_rh(&origin, &(origin + self.calc_forward_direction()), &self.up)
    }
}

pub fn yaw_pitch_rotation(yaw_deg: f32, pitch_deg: f32) -> UnitQuaternion<f32> {
    UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -yaw_deg.to_radians())
        * UnitQuaternion::from_axis_angle(&Vector3::z_axis(), pitch_deg.to_radians())
}
