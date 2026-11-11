use nalgebra::{Matrix4, Orthographic3, Perspective3};

// Maps OpenGL's [-1, 1] NDC z-range to wgpu's [0, 1] range, and reverses it
// (near -> 1, far -> 0) so depth precision concentrates on distant geometry
// instead of being wasted right in front of the near plane. Must be paired
// with CompareFunction::Greater and a depth clear value of 0.0.
#[rustfmt::skip]
pub const OPENGL_TO_WGPU_MATRIX: Matrix4<f32> = Matrix4::new(
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, -0.5, 0.5,
    0.0, 0.0, 0.0, 1.0,
);

// A camera's lens: perspective (things shrink with distance, `fovy` wide)
// or, with `orthographic` set, orthographic - no perspective at all, a box
// `orthographic` meters from its middle to its top (and as wide as the
// screen's shape makes it), the same size on screen however far away.
pub struct Projection {
    aspect: f32,
    pub fovy: f32,
    /// Half the height of what an orthographic lens sees, in meters - None
    /// for a perspective one.
    pub orthographic: Option<f32>,
    pub znear: f32,
    zfar: f32,
}

impl Projection {
    pub fn new(width: u32, height: u32, fovy: f32, znear: f32, zfar: f32) -> Self {
        Self {
            aspect: width as f32 / height as f32,
            fovy,
            orthographic: None,
            znear,
            zfar,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.aspect = width as f32 / height as f32;
    }

    /// Its far plane - how far away things are still drawn.
    pub fn zfar(&self) -> f32 {
        self.zfar
    }

    pub fn set_zfar(&mut self, zfar: f32) {
        self.zfar = zfar.max(self.znear + 0.001);
    }

    pub fn is_orthographic(&self) -> bool {
        self.orthographic.is_some()
    }

    pub fn calc_matrix(&self) -> Matrix4<f32> {
        // Both are OpenGL-style (looking down -Z, depth -1..1) - the same
        // matrix turns either into wgpu's reversed 0..1. An orthographic
        // one's depth comes out linear, not bunched up near the camera.
        let lens = match self.orthographic {
            Some(half_height) => {
                let half_height = half_height.max(0.0001);
                let half_width = half_height * self.aspect;
                Orthographic3::new(-half_width, half_width, -half_height, half_height, self.znear, self.zfar).to_homogeneous()
            }
            None => Perspective3::new(self.aspect, self.fovy.to_radians(), self.znear, self.zfar).to_homogeneous(),
        };
        OPENGL_TO_WGPU_MATRIX * lens
    }
}
