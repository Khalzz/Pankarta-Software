//! Collider shapes Rapier has no constructor for - each one Rapier's own
//! `ColliderBuilder`, ready for `Physics::collider` or `Sensor::from`:
//!
//!     use pankarta_software::physics::shapes;
//!     Physics::fixed().collider(shapes::cylinder_with_sides(1.0, 1.0, 32).friction(0.5))

use std::f32::consts::{FRAC_PI_2, TAU};

use nalgebra::Point3;
use rapier3d::prelude::ColliderBuilder;

/// Half a ball of `radius`, its flat side down on the center - a dome. A
/// convex hull around points on it.
pub fn hemisphere(radius: f32) -> ColliderBuilder {
    const RINGS: usize = 6;
    const SEGMENTS: usize = 16;
    let mut points = vec![Point3::new(0.0, radius, 0.0)];
    for ring in 0..RINGS {
        // From the flat edge (0) up to just under the top.
        let (rise, out) = (ring as f32 / RINGS as f32 * FRAC_PI_2).sin_cos();
        for segment in 0..SEGMENTS {
            let (sin, cos) = (segment as f32 / SEGMENTS as f32 * TAU).sin_cos();
            points.push(Point3::new(cos * out, rise, sin * out) * radius);
        }
    }
    hull(&points)
}

/// A standing cylinder with `sides` flat sides (3 or more) instead of a
/// perfectly round one - a prism, like Blender's cylinder (32 sides by
/// default there, Radius 1, Depth 2: `cylinder_with_sides(1.0, 1.0, 32)`).
/// Rapier's own `ColliderBuilder::cylinder` is exactly round.
pub fn cylinder_with_sides(half_height: f32, radius: f32, sides: usize) -> ColliderBuilder {
    let sides = sides.max(3);
    let mut points = Vec::with_capacity(sides * 2);
    for side in 0..sides {
        let (sin, cos) = (side as f32 / sides as f32 * TAU).sin_cos();
        for y in [-half_height, half_height] {
            points.push(Point3::new(cos * radius, y, sin * radius));
        }
    }
    hull(&points)
}

/// The convex hull around `points` - an empty ball for a size of 0, whose
/// points have no hull.
fn hull(points: &[Point3<f32>]) -> ColliderBuilder {
    ColliderBuilder::convex_hull(points).unwrap_or_else(|| ColliderBuilder::ball(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_are_their_size() {
        let dome = hemisphere(2.0).shape.compute_local_aabb();
        assert!((dome.maxs.y - 2.0).abs() < 1e-4 && dome.mins.y.abs() < 1e-4);
        let prism = cylinder_with_sides(1.0, 0.5, 6).shape;
        let hull = prism.as_convex_polyhedron().expect("a convex hull");
        assert_eq!(hull.points().len(), 12);
    }
}
