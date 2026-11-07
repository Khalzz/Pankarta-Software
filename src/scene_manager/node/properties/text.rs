use crate::ui::color::UiColor;
use crate::ui::components::label::{Font, DEFAULT_FONT_SIZE};

use super::transform::Transform2D;

/// How tall 3D text is when nothing says otherwise (meters).
const DEFAULT_WORLD_SIZE: f32 = 1.0;

/// Text drawn on a node, centered on it - in the 3D world or on the 2D plane,
/// by which transform the node has:
///
///     Node::new("name")
///         .with_property(Transform3D { position, ..Default::default() })
///         .with_property(Text::new("F-16").size(1.0).face_camera())
///
///     Node::new("score")
///         .with_property(Transform2D { position, ..Default::default() })
///         .with_property(Text::new("0").size(24.0).color(UiColor::Hex("#3B82F6")))
///
/// `size` is meters in 3D and UI units in 2D, and it scales with the node. In
/// 3D it's read from the node's +Z side, and hidden by what's in front of it
/// unless it's `always_on_top` (see `FramePlan::nodes_3d`). In 2D it's drawn
/// in order with the node's other visuals, like a `Shape2D` (see
/// `FramePlan::nodes_2d`).
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub text: String,
    /// None: 1 m in 3D, the UI's default font size in 2D.
    pub size: Option<f32>,
    pub color: UiColor,
    pub font: Font,
    /// Where it sits on a 2D node (see `Transform2D::place_visual`) - on a
    /// 3D node, give it a child with its own `Transform3D` instead.
    pub transform: Transform2D,
    /// Doesn't turn with its 2D node.
    pub upright: bool,
    /// Turned to the camera instead of with the node - 3D.
    pub faces_camera: bool,
    /// Drawn over everything, not hidden by what's in front of it - 3D.
    pub on_top: bool,
}

impl Text {
    /// White text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: None,
            color: UiColor::WHITE,
            font: Font::default(),
            transform: Transform2D::default(),
            upright: false,
            faces_camera: false,
            on_top: false,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = Some(size);
        self
    }

    pub fn color(mut self, color: UiColor) -> Self {
        self.color = color;
        self
    }

    pub fn font(mut self, font: Font) -> Self {
        self.font = font;
        self
    }

    /// Where it sits on its 2D node: moved, turned, scaled, and its z index
    /// added to the node's.
    pub fn transform(mut self, transform: Transform2D) -> Self {
        self.transform = transform;
        self
    }

    /// Moves with its 2D node, but doesn't turn with it.
    pub fn upright(mut self) -> Self {
        self.upright = true;
        self
    }

    /// Always turned to the camera - a nameplate.
    pub fn face_camera(mut self) -> Self {
        self.faces_camera = true;
        self
    }

    /// Seen through whatever's in front of it.
    pub fn always_on_top(mut self) -> Self {
        self.on_top = true;
        self
    }

    /// Its size on a 3D node, before the node's scale.
    pub(crate) fn world_size(&self) -> f32 {
        self.size.unwrap_or(DEFAULT_WORLD_SIZE)
    }

    /// Its size on a 2D node, before the node's scale.
    pub(crate) fn plane_size(&self) -> f32 {
        self.size.unwrap_or(DEFAULT_FONT_SIZE)
    }
}
