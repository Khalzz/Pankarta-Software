use nalgebra::{Vector2, Vector3};

use crate::ui::color::UiColor;

use super::transform::{Transform2D, Transform3D};

/// What a line's points can be given as: a `Vector3` (a 3D line) or a
/// `Vector2` (a 2D line).
pub trait LinePoint {
    const IS_2D: bool;
    fn into_point(self) -> Vector3<f32>;
}

impl LinePoint for Vector3<f32> {
    const IS_2D: bool = false;
    fn into_point(self) -> Vector3<f32> {
        self
    }
}

impl LinePoint for Vector2<f32> {
    const IS_2D: bool = true;
    fn into_point(self) -> Vector3<f32> {
        Vector3::new(self.x, self.y, 0.0)
    }
}

/// Whether a line's points are relative to its node, or fixed places.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineSpace {
    /// Relative to the line's node: they move, turn and scale with it.
    #[default]
    Local,
    /// Places in the 3D world, or on the 2D plane.
    World,
}

/// A straight line from `start` to `end`, the same number of pixels wide
/// however far away. 3D or 2D by the points it's made with:
///
///     Node::new("laser").with_property(Line::new(Vector3::zeros(), Vector3::z() * 14.0))
///     Node::new("link").with_property(Line::new(Vector2::zeros(), Vector2::new(60.0, 0.0)))
///
/// See docs/shapes_and_lines.md.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    start: Vector3<f32>,
    end: Vector3<f32>,
    is_2d: bool,
    pub start_space: LineSpace,
    pub end_space: LineSpace,
    /// In pixels.
    pub width: f32,
    pub color: UiColor,
    /// Added to its node's z index - 2D.
    pub z_index: i32,
    /// Its local points don't turn with its node.
    pub upright: bool,
    /// Drawn over everything, not hidden by what's in front of it - 3D.
    pub on_top: bool,
    warned: bool,
}

impl Line {
    /// A white line, 2 pixels wide, with points relative to its node.
    pub fn new<P: LinePoint>(start: P, end: P) -> Self {
        Self {
            start: start.into_point(),
            end: end.into_point(),
            is_2d: P::IS_2D,
            start_space: LineSpace::Local,
            end_space: LineSpace::Local,
            width: 2.0,
            color: UiColor::WHITE,
            z_index: 0,
            upright: false,
            on_top: false,
            warned: false,
        }
    }

    /// Both points are places in the world (or on the 2D plane), not
    /// relative to its node.
    pub fn world(mut self) -> Self {
        self.start_space = LineSpace::World;
        self.end_space = LineSpace::World;
        self
    }

    /// Only the start is a place in the world.
    pub fn start_in_world(mut self) -> Self {
        self.start_space = LineSpace::World;
        self
    }

    /// Only the end is a place in the world - a line from its node to a
    /// target.
    pub fn end_in_world(mut self) -> Self {
        self.end_space = LineSpace::World;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn color(mut self, color: UiColor) -> Self {
        self.color = color;
        self
    }

    /// Drawn this many steps in front of its node's other 2D visuals.
    pub fn z_index(mut self, z_index: i32) -> Self {
        self.z_index = z_index;
        self
    }

    /// Moves with its node, but its points don't turn with it.
    pub fn upright(mut self) -> Self {
        self.upright = true;
        self
    }

    /// Seen through whatever's in front of it.
    pub fn always_on_top(mut self) -> Self {
        self.on_top = true;
        self
    }

    pub fn start(&self) -> Vector3<f32> {
        self.start
    }

    pub fn end(&self) -> Vector3<f32> {
        self.end
    }

    pub fn is_2d(&self) -> bool {
        self.is_2d
    }

    pub fn set_start<P: LinePoint>(&mut self, point: P) {
        if self.accepts::<P>("set_start") {
            self.start = point.into_point();
        }
    }

    pub fn set_end<P: LinePoint>(&mut self, point: P) {
        if self.accepts::<P>("set_end") {
            self.end = point.into_point();
        }
    }

    fn accepts<P: LinePoint>(&mut self, call: &str) -> bool {
        if P::IS_2D == self.is_2d {
            return true;
        }
        if !self.warned {
            self.warned = true;
            let (made, given) = if self.is_2d { ("2D", "3D") } else { ("3D", "2D") };
            eprintln!("Line::{call}: a {given} point given to a {made} line - ignored");
        }
        false
    }

    /// Moves its points along with the world when the floating origin moves
    /// (see `WorldOrigin`) - only world points, on a 3D line.
    pub(crate) fn shift_world_points(&mut self, shift: Vector3<f32>) {
        if self.is_2d {
            return;
        }
        if self.start_space == LineSpace::World {
            self.start += shift;
        }
        if self.end_space == LineSpace::World {
            self.end += shift;
        }
    }

    /// Both points in the world, for a line on a node at `transform`.
    pub(crate) fn points_3d(&self, transform: &Transform3D) -> [Vector3<f32>; 2] {
        [(self.start, self.start_space), (self.end, self.end_space)].map(|(point, space)| match space {
            LineSpace::World => point,
            LineSpace::Local => {
                let scaled = point.component_mul(&transform.scale);
                transform.position + if self.upright { scaled } else { transform.rotation * scaled }
            }
        })
    }

    /// Both points on the plane, for a line on a node at `transform`.
    pub(crate) fn points_2d(&self, transform: &Transform2D) -> [Vector2<f32>; 2] {
        [(self.start.xy(), self.start_space), (self.end.xy(), self.end_space)].map(|(point, space)| match space {
            LineSpace::World => point,
            LineSpace::Local => {
                let local = Transform2D { position: point, ..Default::default() };
                transform.place_visual(&local, self.upright).position
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use nalgebra::UnitQuaternion;

    use super::*;

    #[test]
    fn local_points_turn_with_their_node() {
        let transform = Transform3D {
            position: Vector3::new(1.0, 2.0, 3.0),
            rotation: UnitQuaternion::from_axis_angle(&Vector3::y_axis(), std::f32::consts::FRAC_PI_2),
            ..Default::default()
        };
        let [start, end] = Line::new(Vector3::zeros(), Vector3::z() * 10.0).points_3d(&transform);
        assert!((start - transform.position).magnitude() < 1e-4);
        assert!((end - Vector3::new(11.0, 2.0, 3.0)).magnitude() < 1e-4);
    }

    #[test]
    fn world_points_ignore_their_node() {
        let transform = Transform3D { position: Vector3::new(5.0, 5.0, 5.0), ..Default::default() };
        let [start, end] = Line::new(Vector3::x(), Vector3::y()).world().points_3d(&transform);
        assert_eq!((start, end), (Vector3::x(), Vector3::y()));
    }

    #[test]
    fn a_line_can_start_at_its_node_and_end_in_the_world() {
        let transform = Transform3D { position: Vector3::new(5.0, 0.0, 0.0), ..Default::default() };
        let [start, end] = Line::new(Vector3::zeros(), Vector3::new(-3.0, 1.0, 0.0)).end_in_world().points_3d(&transform);
        assert_eq!((start, end), (Vector3::new(5.0, 0.0, 0.0), Vector3::new(-3.0, 1.0, 0.0)));
    }

    #[test]
    fn a_2d_line_scales_with_its_node_and_upright_ignores_its_turn() {
        let transform = Transform2D { position: Vector2::new(100.0, 50.0), rotation: std::f32::consts::FRAC_PI_2, scale: Vector2::new(2.0, 2.0), z_index: 0 };
        let line = Line::new(Vector2::zeros(), Vector2::new(10.0, 0.0));
        // A quarter turn clockwise (y down): +x becomes +y.
        assert!((line.points_2d(&transform)[1] - Vector2::new(100.0, 70.0)).magnitude() < 1e-4);
        assert!((line.clone().upright().points_2d(&transform)[1] - Vector2::new(120.0, 50.0)).magnitude() < 1e-4);
    }

    #[test]
    fn a_point_of_the_other_kind_is_ignored() {
        let mut line = Line::new(Vector2::zeros(), Vector2::new(1.0, 0.0));
        line.set_end(Vector3::new(9.0, 9.0, 9.0));
        assert_eq!(line.end(), Vector3::new(1.0, 0.0, 0.0));
        line.set_end(Vector2::new(3.0, 4.0));
        assert_eq!(line.end(), Vector3::new(3.0, 4.0, 0.0));
    }
}
