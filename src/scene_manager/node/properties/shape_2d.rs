use crate::ui::color::UiColor;

use super::transform::Transform2D;

/// What a `Shape2D` is - sizes in UI units (see `Transform2D`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeKind {
    Rect { width: f32, height: f32 },
    Circle { radius: f32 },
}

/// A flat shape drawn on a 2D node (one with a `Transform2D`), centered on
/// the node - moved from there by its own `transform`:
///
///     Node::new("box")
///         .with_property(Transform2D { position, ..Default::default() })
///         .with_property(Shape2D::square(80.0).color(UiColor::Hex("#3B82F6")))
///
/// See docs/shapes_and_lines.md.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape2D {
    pub kind: ShapeKind,
    pub color: UiColor,
    /// Relative to its node (see `Transform2D::place_visual`).
    pub transform: Transform2D,
    /// Doesn't turn with its node.
    pub upright: bool,
}

impl Shape2D {
    /// A white `width` x `height` rectangle.
    pub fn rect(width: f32, height: f32) -> Self {
        Self::new(ShapeKind::Rect { width, height })
    }

    /// A white square, `size` on each side.
    pub fn square(size: f32) -> Self {
        Self::rect(size, size)
    }

    /// A white circle.
    pub fn circle(radius: f32) -> Self {
        Self::new(ShapeKind::Circle { radius })
    }

    fn new(kind: ShapeKind) -> Self {
        Self { kind, color: UiColor::WHITE, transform: Transform2D::default(), upright: false }
    }

    pub fn color(mut self, color: UiColor) -> Self {
        self.color = color;
        self
    }

    /// Where it sits on its node: moved, turned, scaled, and its z index
    /// added to the node's.
    pub fn transform(mut self, transform: Transform2D) -> Self {
        self.transform = transform;
        self
    }

    /// Moves with its node, but doesn't turn with it.
    pub fn upright(mut self) -> Self {
        self.upright = true;
        self
    }
}
