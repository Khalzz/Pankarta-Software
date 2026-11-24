use std::collections::HashMap;
use nalgebra::{Matrix4, Point3, UnitQuaternion, Vector3};
use sdl2::rect::Point;
use wgpu::{util::DeviceExt, BindGroup, BindGroupLayout, BindGroupLayoutDescriptor, Buffer, Device, Queue};
use crate::scene_manager::node::properties::{CameraLens, Transform3D};

use super::camera::Camera;
use super::projection::Projection;
use super::uniform::{CameraUniform, NearFarUniform};

// Stands in for "no real camera exists" - `SceneCameras::new` selects this
// instead of a scene starting with no active camera at all. A real, if
// meaningless, `CameraInstance` sits in `cameras` under this name so
// `active()`'s `.expect()` never has anything to panic on - only
// `has_active_camera()` (and the render loop, via that) treats this specially.
const NO_CAMERA_SENTINEL: &str = "__no_camera";
/// The camera node a scene gets when it starts without one of its own - at
/// the origin, unrotated (see `Scene::ensure_camera`). A node like any
/// other: found, moved and selected by this name. A camera node the scene
/// spawns later takes over from it.
pub const DEFAULT_CAMERA: &str = "default_camera";
/// A camera's far plane unless its lens says otherwise (meters) - kept in
/// sync with CameraResources::new's.
const DEFAULT_FAR: f32 = 4_000_000.0;

/// A camera as the renderer sees it this frame - read-only. It's copied
/// from its camera node every frame (see `Scene::sync_cameras`): to move,
/// turn or zoom a camera, change its node's `Transform3D` and `CameraLens`.
pub struct CameraInstance {
    pub(crate) camera: Camera,
    pub(crate) projection: Projection,
}

impl CameraInstance {
    /// Where it is (world space, meters).
    pub fn position(&self) -> Vector3<f32> {
        self.camera.position().coords
    }

    /// Which way it looks (unit length) - its node's -Z.
    pub fn forward(&self) -> Vector3<f32> {
        self.camera.calc_forward_direction()
    }

    /// Its up (unit length) - its node's +Y.
    pub fn up(&self) -> Vector3<f32> {
        self.camera.up
    }

    /// Vertical field of view, degrees (a perspective lens's).
    pub fn fovy(&self) -> f32 {
        self.projection.fovy
    }

    pub fn is_orthographic(&self) -> bool {
        self.projection.is_orthographic()
    }

    /// The view matrix, camera-relative: the camera sits at the origin, so
    /// subtract `position()` from a world point before using it.
    pub fn view_matrix(&self) -> Matrix4<f32> {
        self.camera.calc_matrix()
    }

    /// The projection matrix (wgpu's reversed 0..1 depth).
    pub fn projection_matrix(&self) -> Matrix4<f32> {
        self.projection.calc_matrix()
    }
}

/// The persistent, app-level half of the camera system - the one shared set
/// of GPU resources (uniform/buffer/bind group) that actually feeds the
/// renderer each frame, created once and reused across every scene switch.
/// Lives on `App` as `app.camera_resources`. Doesn't hold any camera data
/// itself (no cameras, no "active" concept) - that's scene-scoped (see
/// `SceneCameras`, on `Scene::cameras`); this just uploads whatever
/// `CameraInstance` it's handed (`update_buffer`) and answers screen-space
/// queries against whatever it last uploaded (`world_to_screen`).
pub struct CameraResources {
    // Remembered so SceneCameras::new/create_camera never need width/height
    // passed in explicitly - every camera's aspect ratio is always just
    // "whatever the screen currently is". Kept current by `resize`.
    width: u32,
    height: u32,
    pub(crate) uniform: CameraUniform,
    pub(crate) buffer: Buffer,
    pub(crate) bind_group_layout: BindGroupLayout,
    pub(crate) bind_group: BindGroup,
}

impl CameraResources {
    pub(crate) fn new(device: &Device, config: &wgpu::SurfaceConfiguration) -> Self {
        let near_far_uniform = NearFarUniform {
            near: 0.1,
            // Pushed way out - reversed-Z keeps near/mid depth precision fine
            // regardless of how far this goes. Only until the first frame:
            // from then on the active camera's own (see update_view_proj).
            far: 4_000_000.0,
        };

        let uniform = CameraUniform::new(near_far_uniform);

        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Camera Buffer"),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("camera_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        CameraResources {
            width: config.width,
            height: config.height,
            uniform,
            buffer,
            bind_group_layout,
            bind_group,
        }
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    // Only remembers the new size for future SceneCameras::new/create_camera
    // calls - resizing every already-registered camera's own aspect is
    // scene-scoped now (see SceneCameras::resize), since the cameras
    // themselves live there, not here.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
    }

    // Recomputes the view_proj uniform from `active` and uploads it - called
    // once per frame from App, replacing what used to be two lines inlined
    // at every call site.
    pub(crate) fn update_buffer(&mut self, queue: &Queue, active: &CameraInstance) {
        self.uniform.update_view_proj(&active.camera, &active.projection);
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&[self.uniform]));
    }

    pub fn world_to_screen(&self, active: &CameraInstance, pos_world: Point3<f32>, screen_width: u32, screen_height: u32) -> Option<Point> {
        // view_proj now assumes the camera sits at the origin, so every
        // position fed into it must first be made camera-relative.
        let camera_to_point = pos_world - active.camera.position();
        let forward = active.camera.calc_forward_direction();

        if camera_to_point.dot(&forward) < 0.0 {
            return None;
        }

        let view_proj = Matrix4::from(self.uniform.view_proj);
        let pos_homogeneous = view_proj * Point3::from(camera_to_point).to_homogeneous();

        if pos_homogeneous.w != 0.0 {
            let ndc = pos_homogeneous.xyz() / pos_homogeneous.w;

            if ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && ndc.z >= 0.0 && ndc.z <= 1.0 {
                let x = ((ndc.x + 1.0) * 0.5) * screen_width as f32;
                let y = ((1.0 - (ndc.y + 1.0) * 0.5)) * screen_height as f32;

                Some(Point::new(x as i32, y as i32))
            } else {
                None
            }
        } else {
            None
        }
    }

    /// The reverse of `world_to_screen`: the ray from the camera through
    /// screen pixel (x, y) - its start (the camera, world space - or, with
    /// an orthographic lens, the pixel's own point on the near plane: every
    /// pixel's ray runs the same way, from a different place) and direction
    /// (unit length). `None` if the projection can't be inverted.
    pub fn screen_ray(&self, active: &CameraInstance, x: f32, y: f32, screen_width: u32, screen_height: u32) -> Option<(Point3<f32>, Vector3<f32>)> {
        let inverse = Matrix4::from(self.uniform.view_proj).try_inverse()?;
        let ndc_x = x / screen_width.max(1) as f32 * 2.0 - 1.0;
        let ndc_y = 1.0 - y / screen_height.max(1) as f32 * 2.0;
        // Two points along the pixel's ray (depths inside the view either
        // way round) - camera-relative, like view_proj.
        let unproject = |depth: f32| {
            let point = inverse * nalgebra::Vector4::new(ndc_x, ndc_y, depth, 1.0);
            (point.w.abs() > 1e-12).then(|| point.xyz() / point.w)
        };
        let (near, far) = (unproject(0.25)?, unproject(0.75)?);
        let mut direction = (far - near).try_normalize(1e-9)?;
        // Pointing away from the camera, whichever way the depth runs.
        if direction.dot(&active.camera.calc_forward_direction()) < 0.0 {
            direction = -direction;
        }
        if active.projection.is_orthographic() {
            // Reversed depth: 1 is the near plane.
            let start = unproject(1.0)?;
            return Some((active.camera.position() + start, direction));
        }
        Some((active.camera.position(), direction))
    }
}


/// The scene-scoped half of the camera system - every camera the currently
/// active scene has registered and which one's active. Lives on `Scene` as
/// `scene.cameras`, so it's dropped and rebuilt fresh on every scene switch.
///
/// Cameras are made by camera nodes - a node with a `CameraLens` - not here:
/// each one's camera is registered under the node's id when it spawns, kept
/// where the node is every frame (see `Scene::sync_cameras`), and removed
/// with the node. What a game uses here: `select_camera`, `active_name`,
/// `active` and `get`.
pub struct SceneCameras {
    cameras: HashMap<String, CameraInstance>,
    active: String,
    // Copied from CameraResources at construction (see `Scene::new`) - kept
    // here too so create_camera never needs width/height passed in
    // explicitly. Stays in sync with CameraResources via `resize`.
    width: u32,
    height: u32,
}

impl SceneCameras {
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let mut cameras = SceneCameras {
            cameras: HashMap::new(),
            active: NO_CAMERA_SENTINEL.to_owned(),
            width,
            height,
        };

        // Placeholder so active()'s .expect() has something to find before
        // this scene creates its own first camera - never meant to be seen,
        // the render loop skips drawing while this is active.
        cameras.create_camera(NO_CAMERA_SENTINEL, Transform3D::default(), 45.0);
        cameras
    }

    /// Registers (or replaces) a named camera. `transform.scale` is ignored.
    /// Aspect ratio always comes from this scene's own tracked screen size,
    /// kept current by `resize`.
    ///
    /// If nothing else is currently active (see `NO_CAMERA_SENTINEL`), or
    /// only the automatic `DEFAULT_CAMERA` is, this camera becomes active -
    /// a scene with exactly one camera of its own never needs its own
    /// `select_camera` call. A second one doesn't repeat that - choosing
    /// between a scene's own cameras still needs `select_camera`.
    pub(crate) fn create_camera(&mut self, name: impl Into<String>, transform: Transform3D, fovy: f32) -> &mut CameraInstance {
        let camera = Camera::new(transform);
        let projection = Projection::new(self.width, self.height, fovy, 0.1, DEFAULT_FAR);

        let name = name.into();
        if self.active == NO_CAMERA_SENTINEL || self.active == DEFAULT_CAMERA {
            self.active = name.clone();
        }
        self.cameras.insert(name.clone(), CameraInstance { camera, projection });
        self.cameras.get_mut(&name).unwrap()
    }

    /// Whether a real, scene-created camera is currently active - `false`
    /// until a camera node spawns. The render loop uses this to fall back to
    /// a black screen instead of drawing from a meaningless placeholder.
    pub(crate) fn has_active_camera(&self) -> bool {
        self.active != NO_CAMERA_SENTINEL
    }

    /// Switches the active camera (an instant cut) to camera node `name`.
    /// Returns false (no-op) if there's no such camera.
    pub fn select_camera(&mut self, name: &str) -> bool {
        if self.cameras.contains_key(name) {
            self.active = name.to_owned();
            true
        } else {
            false
        }
    }

    /// Takes the camera `name` away - if it was the active one, back to no
    /// camera until another is selected.
    pub(crate) fn remove_camera(&mut self, name: &str) {
        if name == NO_CAMERA_SENTINEL || self.cameras.remove(name).is_none() {
            return;
        }
        if self.active == name {
            self.active = NO_CAMERA_SENTINEL.to_owned();
        }
    }

    /// Puts camera `name` (registering it first if it's new) where a camera
    /// node is: at `position`, looking along `rotation`'s -Z with its +Y up
    /// and +X to its right (Godot's convention - see `Transform3D::forward`),
    /// with `lens` - drawn its viewport big, or the whole screen. See
    /// `Scene::sync_cameras`.
    pub(crate) fn place(&mut self, name: &str, position: Vector3<f32>, rotation: UnitQuaternion<f32>, lens: &CameraLens) {
        if !self.cameras.contains_key(name) {
            self.create_camera(name, Transform3D { position, ..Default::default() }, lens.fovy);
        }
        let (width, height) = lens.viewport.unwrap_or((self.width, self.height));
        let Some(instance) = self.cameras.get_mut(name) else { return };
        instance.camera.set_position(Point3::from(position));
        instance.camera.set_orientation(rotation * -Vector3::z(), rotation * Vector3::y());
        instance.projection.fovy = lens.fovy;
        instance.projection.orthographic = lens.orthographic;
        instance.projection.znear = lens.znear;
        instance.projection.set_zfar(lens.zfar.unwrap_or(DEFAULT_FAR));
        instance.projection.resize(width.max(1), height.max(1));
    }

    /// Moves every camera by `shift`, for the floating origin (see
    /// `WorldOrigin`).
    pub(crate) fn shift_origin(&mut self, shift: Vector3<f32>) {
        for instance in self.cameras.values_mut() {
            let position = instance.camera.position();
            instance.camera.set_position(position + shift);
        }
    }

    /// The camera being looked through's name - its camera node's id.
    pub fn active_name(&self) -> &str {
        &self.active
    }

    /// The camera being looked through - read-only (see `CameraInstance`).
    pub fn active(&self) -> &CameraInstance {
        self.cameras.get(&self.active).expect("active camera must always exist")
    }

    /// Camera node `name`'s camera - read-only (see `CameraInstance`).
    pub fn get(&self, name: &str) -> Option<&CameraInstance> {
        self.cameras.get(name)
    }

    // Every registered camera's aspect stays in sync with the screen, not just
    // the active one - so selecting a different camera later never shows a
    // stale aspect ratio for a frame.
    pub(crate) fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        for instance in self.cameras.values_mut() {
            instance.projection.resize(width, height);
        }
    }
}
