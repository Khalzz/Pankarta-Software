//! The floating origin - `app.origin`.
//!
//! Positions everywhere in the engine (nodes, physics, the renderer) are
//! `f32`, which has about 7 significant digits: the further from (0, 0, 0),
//! the coarser every coordinate gets (at 100 km a position can only move in
//! ~8 mm steps, at 1000 km in ~6 cm steps, and the physics stops moving
//! things at all). So the engine keeps the open scene's numbers small: once
//! what it follows (the node with an `OriginAnchor`, or the active camera)
//! gets `threshold` meters out on an axis, the whole world - every node,
//! physics body, camera and particle - is moved back toward (0, 0, 0) in one
//! step, at the start of a frame, by a whole number of `grid`s. Where the
//! local (0, 0, 0) really is in the world is kept here, in `f64`.
//!
//! Gameplay mostly doesn't notice: everything moves together, and the next
//! frame looks the same. What the game needs to know:
//!
//! - A world position for showing, saving or sending is
//!   `app.origin.to_world(local)`; a world position to place something at is
//!   `app.origin.to_local(world)`.
//! - A position a behavior keeps in its own fields (a patrol route, a spawn
//!   point) is in the scene's local numbers too - move it in
//!   `Behavior::on_origin_shift` / `SceneBehaviour::on_origin_shift`, or keep
//!   it as a world position (`f64`) and convert it when it's used.
//! - To put a node far away (a teleport, a respawn), `app.origin.teleport` -
//!   the origin moves there first, so the node lands precisely.
//!
//! Each scene starts with the origin at (0, 0, 0) (see `start_at` to start it
//! elsewhere). The settings (`set_enabled`, `set_threshold`, `set_grid`) are
//! global, like every system's: they hold for every scene until set again.

use nalgebra::{UnitQuaternion, Vector3};

/// Where the scene's local (0, 0, 0) is in the world, and when it moves -
/// see the module doc. `app.origin`.
#[derive(Debug, Clone)]
pub struct WorldOrigin {
    origin: Vector3<f64>,
    enabled: bool,
    threshold: f32,
    grid: f32,
    /// How far everything moved this frame (local numbers) - None: it didn't.
    last_shift: Option<Vector3<f32>>,
    /// Teleports asked for since the last time they were done.
    pending: Vec<Teleport>,
}

/// A node to put at a world position once the origin is next moved - see
/// `WorldOrigin::teleport`.
#[derive(Debug, Clone)]
pub(crate) struct Teleport {
    pub node: String,
    pub world: Vector3<f64>,
    pub rotation: Option<UnitQuaternion<f32>>,
}

impl Default for WorldOrigin {
    fn default() -> Self {
        Self::new()
    }
}

impl WorldOrigin {
    /// On, moving the world once what it follows is 4096 m out, by whole
    /// kilometers (1024 m).
    pub fn new() -> Self {
        Self { origin: Vector3::zeros(), enabled: true, threshold: 4096.0, grid: 1024.0, last_shift: None, pending: Vec::new() }
    }

    /// Where the scene's local (0, 0, 0) is in the world.
    pub fn origin(&self) -> Vector3<f64> {
        self.origin
    }

    /// The world position of the local position `local`.
    pub fn to_world(&self, local: Vector3<f32>) -> Vector3<f64> {
        self.origin + local.cast::<f64>()
    }

    /// The local position - what nodes, bodies and cameras use - of the world
    /// position `world`.
    pub fn to_local(&self, world: Vector3<f64>) -> Vector3<f32> {
        (world - self.origin).cast::<f32>()
    }

    /// How far everything moved this frame, in local numbers (what was at
    /// `p` is now at `p + shift`) - None when it didn't. Every hook that runs
    /// after the move sees it, until the next frame starts.
    pub fn last_shift(&self) -> Option<Vector3<f32>> {
        self.last_shift
    }

    /// Whether the world is moved back toward (0, 0, 0) on its own. On by
    /// default. Off, the origin stays where it is (`teleport` still moves
    /// it).
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// How far out (m, on any axis) what the origin follows goes before the
    /// world is moved back - 4096 by default, where a position still moves
    /// in steps of half a millimeter.
    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// See `threshold`. At least one `grid`.
    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.max(self.grid);
    }

    /// What every move of the world is a whole number of (m) - 1024 by
    /// default. A power of two moves positions without rounding them, and
    /// keeps the origin exact.
    pub fn grid(&self) -> f32 {
        self.grid
    }

    /// See `grid`. At least 1 m - the threshold is raised to it if it's less.
    pub fn set_grid(&mut self, grid: f32) {
        self.grid = grid.max(1.0);
        self.threshold = self.threshold.max(self.grid);
    }

    /// Starts the scene with its origin at `world` (on the grid nearest it) -
    /// for a scene that starts far out (a saved game). Call it while the
    /// scene is being built, before spawning anything: what's already
    /// placed isn't moved. Later, use `teleport`.
    pub fn start_at(&mut self, world: Vector3<f64>) {
        self.origin = self.snap(world);
    }

    /// Puts the node `node` at the world position `world` - turned
    /// `rotation`, or as it is - and, if it has a physics body, stopped. The
    /// origin moves there first (the whole world moving with it), so the node
    /// lands exactly there however far out it is. Done at the next point the
    /// engine can move the world: before the next physics step, after this
    /// frame's behaviors, or at the start of the next frame.
    pub fn teleport(&mut self, node: impl Into<String>, world: Vector3<f64>, rotation: Option<UnitQuaternion<f32>>) {
        self.pending.push(Teleport { node: node.into(), world, rotation });
    }

    // ── Engine side ──

    /// The grid point nearest `world`.
    fn snap(&self, world: Vector3<f64>) -> Vector3<f64> {
        let grid = self.grid as f64;
        world.map(|value| (value / grid).round() * grid)
    }

    /// How far to move everything (local numbers) for a node at `anchor`
    /// (local) - None while it's inside the threshold on every axis. Only the
    /// axes it's out on move.
    pub(crate) fn shift_for(&self, anchor: Vector3<f32>) -> Option<Vector3<f32>> {
        if !self.enabled || anchor.iter().all(|value| value.abs() <= self.threshold) {
            return None;
        }
        let grid = self.grid as f64;
        let shift = anchor.map(|value| if value.abs() > self.threshold { -((value as f64 / grid).round() * grid) as f32 } else { 0.0 });
        (shift != Vector3::zeros()).then_some(shift)
    }

    /// How far to move everything (local numbers) to bring the origin to the
    /// grid point nearest `world` - None when it's there already.
    pub(crate) fn shift_to(&self, world: Vector3<f64>) -> Option<Vector3<f32>> {
        let shift = (self.origin - self.snap(world)).cast::<f32>();
        (shift != Vector3::zeros()).then_some(shift)
    }

    /// Notes that everything moved by `shift`: what was at local `p` is at
    /// `p + shift` now, so the same world position's local (0, 0, 0) is
    /// `shift` further back.
    pub(crate) fn record_shift(&mut self, shift: Vector3<f32>) {
        self.origin -= shift.cast::<f64>();
        self.last_shift = Some(self.last_shift.unwrap_or_else(Vector3::zeros) + shift);
    }

    /// The teleports asked for, to do now.
    pub(crate) fn take_pending(&mut self) -> Vec<Teleport> {
        std::mem::take(&mut self.pending)
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// A new frame: nothing has moved in it yet.
    pub(crate) fn begin_frame(&mut self) {
        self.last_shift = None;
    }

    /// A new scene: its origin starts at (0, 0, 0), nothing pending.
    pub(crate) fn reset(&mut self) {
        self.origin = Vector3::zeros();
        self.last_shift = None;
        self.pending.clear();
    }

    /// `origin` modulo `period`, in [0, period) on each axis - worked out in
    /// `f64`, so it's precise however far out the origin is. What a
    /// world-tiled texture repeating every `period` meters is offset by.
    pub(crate) fn wrapped(&self, period: f32) -> Vector3<f32> {
        let period = period.max(1e-4) as f64;
        self.origin.map(|value| value.rem_euclid(period) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_moves_inside_the_threshold() {
        let origin = WorldOrigin::new();
        assert_eq!(origin.shift_for(Vector3::new(4000.0, -4096.0, 10.0)), None);
    }

    #[test]
    fn only_the_axes_out_past_the_threshold_move_by_whole_grids() {
        let origin = WorldOrigin::new();
        let shift = origin.shift_for(Vector3::new(5000.0, 300.0, -9000.0)).unwrap();
        assert_eq!(shift, Vector3::new(-5120.0, 0.0, 9216.0));
    }

    #[test]
    fn disabled_it_never_moves() {
        let mut origin = WorldOrigin::new();
        origin.set_enabled(false);
        assert_eq!(origin.shift_for(Vector3::new(1.0e7, 0.0, 0.0)), None);
    }

    #[test]
    fn a_shift_keeps_every_world_position_where_it_is() {
        let mut origin = WorldOrigin::new();
        let local = Vector3::new(5000.25_f32, 1.5, -3.0);
        let world = origin.to_world(local);
        let shift = origin.shift_for(local).unwrap();
        origin.record_shift(shift);
        // The local position moves by the shift - exactly, being whole grids.
        let moved = local + shift;
        assert_eq!(origin.to_world(moved), world);
        assert_eq!(origin.to_local(world), moved);
    }

    #[test]
    fn many_shifts_dont_drift() {
        let mut origin = WorldOrigin::new();
        for _ in 0..10_000 {
            let shift = origin.shift_for(Vector3::new(4100.0, 0.0, 0.0)).unwrap();
            origin.record_shift(shift);
        }
        assert_eq!(origin.origin(), Vector3::new(4096.0 * 10_000.0, 0.0, 0.0));
    }

    #[test]
    fn far_out_a_position_is_as_precise_as_at_the_start() {
        let mut origin = WorldOrigin::new();
        let target = Vector3::new(10_000_000.123_f64, 2.0, -3_000_000.5);
        let shift = origin.shift_to(target).unwrap();
        origin.record_shift(shift);
        let local = origin.to_local(target);
        assert!(local.iter().all(|value| value.abs() <= 512.0));
        assert!((origin.to_world(local) - target).norm() < 1e-4);
    }

    #[test]
    fn the_tile_offset_wraps_precisely() {
        let mut origin = WorldOrigin::new();
        origin.start_at(Vector3::new(10_240_000.0 + 3.0, 0.0, -1024.0));
        // On the grid: 10_240_000 (a multiple of 1024 - 10000 grids) and -1024.
        assert_eq!(origin.origin(), Vector3::new(10_240_000.0, 0.0, -1024.0));
        assert_eq!(origin.wrapped(3.0), Vector3::new(1.0, 0.0, 2.0));
    }
}
