/// Makes a node a camera - registered with the scene's cameras under the
/// node's id when it spawns, and kept where the node is every frame (see
/// `Scene::sync_cameras`). A camera looks along its node's +Z, its +Y up
/// (the same way a model faces) - so a camera child of a plane with no
/// rotation looks where the plane's nose points.
///
/// Perspective by default (`CameraLens::new`): things shrink with distance.
/// `CameraLens::orthographic` has no perspective - a thing is the same size
/// on screen however far away it is, for maps, top-down and side views.
#[derive(Debug, Clone, Copy)]
pub struct CameraLens {
    /// Vertical field of view, degrees - a perspective lens's.
    pub fovy: f32,
    /// Half the height of what an orthographic lens sees, in meters (its
    /// zoom: smaller sees less, bigger). None: a perspective lens.
    pub orthographic: Option<f32>,
    pub znear: f32,
    /// How far away things are still drawn. None: the engine's default
    /// (4000 km) - right for a perspective lens. An orthographic lens's
    /// depth is spread evenly from near to far, so keep its far plane just
    /// past the farthest thing it needs to see (or things z-fight).
    pub zfar: Option<f32>,
    /// The size it's drawn at (logical pixels), when it isn't the whole
    /// screen - a map drawn in only part of it, say.
    pub viewport: Option<(u32, u32)>,
}

/// An orthographic lens's far plane when none is given (meters).
const ORTHOGRAPHIC_FAR: f32 = 1000.0;

impl CameraLens {
    pub fn new(fovy: f32) -> Self {
        Self { fovy, orthographic: None, znear: 0.1, zfar: None, viewport: None }
    }

    /// An orthographic lens seeing `half_height` meters from its middle to
    /// its top (as wide as the screen's shape makes it), drawing what's
    /// within 1000 m in front of it (see `far`).
    pub fn orthographic(half_height: f32) -> Self {
        Self { fovy: 60.0, orthographic: Some(half_height), znear: 0.1, zfar: Some(ORTHOGRAPHIC_FAR), viewport: None }
    }

    /// Draws things up to `zfar` meters away.
    pub fn far(mut self, zfar: f32) -> Self {
        self.zfar = Some(zfar);
        self
    }

    pub fn is_orthographic(&self) -> bool {
        self.orthographic.is_some()
    }
}

impl Default for CameraLens {
    fn default() -> Self {
        Self::new(60.0)
    }
}
