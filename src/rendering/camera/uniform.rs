use nalgebra::Matrix4;

use super::camera::Camera;
use super::projection::Projection;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct NearFarUniform {
    pub near: f32,
    pub far: f32,
}

// the cameraUniform will get us the positional matrix of the camera
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct CameraUniform {
    // pub - read directly by handler::CameraHandler::world_to_screen, a
    // sibling module now that this struct has its own file.
    pub view_proj: [[f32; 4]; 4],
    view_position: [f32; 4],
    // The active camera's near and far planes - what a shader turns a depth
    // into a distance with (see standard_template.wgsl's linear_depth).
    near: f32,
    far: f32,
    // 1 for an orthographic lens, 0 for a perspective one - how a depth
    // turns into a distance, and which way a pixel is seen from.
    orthographic: f32,
    // WGSL rounds the struct up to a multiple of 16 bytes.
    _padding: f32,
    // Which way the camera looks (unit length, w unused) - the way every
    // pixel is seen from, with an orthographic lens.
    forward: [f32; 4],
}

impl CameraUniform {
    pub fn new(near_far: NearFarUniform) -> Self {
        Self {
            view_proj: Matrix4::identity().into(),
            view_position: [0.0; 4],
            near: near_far.near,
            far: near_far.far,
            orthographic: 0.0,
            _padding: 0.0,
            forward: [0.0, 0.0, 1.0, 0.0],
        }
    }

    pub fn update_view_proj(&mut self, camera: &Camera, projection: &Projection) {
        // The camera is always at the origin in the space these matrices operate in.
        self.view_position = [0.0, 0.0, 0.0, 1.0];
        let view = camera.calc_matrix();
        self.view_proj = (projection.calc_matrix() * view).into();
        self.near = projection.znear;
        self.far = projection.zfar();
        self.orthographic = if projection.is_orthographic() { 1.0 } else { 0.0 };
        // The view matrix's third row is the camera's back (it looks down -Z).
        self.forward = [-view[(2, 0)], -view[(2, 1)], -view[(2, 2)], 0.0];
    }
}
