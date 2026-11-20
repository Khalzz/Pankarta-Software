//! Gizmos - lines, icons and text drawn over the world for debugging,
//! from anywhere that has the app (a behavior's `update`, a scene's):
//!
//!     app.gizmos.channel("aero", |g| {
//!         g.icon(com, Icon::Mass);
//!         g.ray(center_of_lift, lift * 0.001).color(GizmoColor::BLUE).arrow().label("lift");
//!         g.outline(&wing_corners).dashed();
//!     });
//!
//! Each call puts a gizmo up for this frame only - drawn once, then gone, so
//! what's on screen is whatever was drawn this frame. Settings are chained
//! after it (`.color()`, `.width()`, `.on_top()`...); what isn't set comes
//! from its channel's style (`app.gizmos.style("aero")`).
//!
//! Channels group them, to switch on and off together:
//! `app.gizmos.set_enabled("aero", false)` - while a channel's off its
//! closure doesn't even run. A channel is on from the first time it's drawn
//! in; `app.gizmos.visible = false` hides every one.
//!
//! By default a gizmo is hidden by the world in front of it - drawn faint
//! there, so it's still found - and `.on_top()` draws it over everything.
//! Text (and labels) are always on top. Sizes are in pixels on screen
//! (widths, icons, text, arrowheads), lengths in the world (a cross, a
//! circle).
//!
//! A gizmo drawn from `fixed_update` shows only on the frames a physics step
//! runs - draw from `update`, or give it a `.duration(seconds)`: it stays up
//! that long (in game time - it holds while the game's paused). Something
//! that happens in one step, a hit, is seen that way.

use std::collections::HashMap;
use std::sync::Arc;

use nalgebra::{Isometry3, Point3, UnitQuaternion, Vector3};

use crate::rendering::gizmos::icons::{Icon, IconLibrary};
use crate::scene_manager::node::properties::Transform3D;
use crate::ui::color::UiColor;

/// A gizmo's color: red, green, blue, alpha, 0..1 (as the UI's colors are -
/// not linear). From `[r, g, b]`, `[r, g, b, a]`, a `UiColor`, or one of
/// the constants here.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoColor(pub [f32; 4]);

impl GizmoColor {
    pub const WHITE: GizmoColor = GizmoColor([1.0, 1.0, 1.0, 1.0]);
    pub const BLACK: GizmoColor = GizmoColor([0.0, 0.0, 0.0, 1.0]);
    pub const GRAY: GizmoColor = GizmoColor([0.6, 0.6, 0.6, 1.0]);
    pub const RED: GizmoColor = GizmoColor([1.0, 0.25, 0.25, 1.0]);
    pub const GREEN: GizmoColor = GizmoColor([0.3, 0.95, 0.4, 1.0]);
    pub const BLUE: GizmoColor = GizmoColor([0.3, 0.6, 1.0, 1.0]);
    pub const YELLOW: GizmoColor = GizmoColor([1.0, 0.9, 0.2, 1.0]);
    pub const ORANGE: GizmoColor = GizmoColor([1.0, 0.55, 0.1, 1.0]);
    pub const CYAN: GizmoColor = GizmoColor([0.2, 0.95, 0.95, 1.0]);
    pub const MAGENTA: GizmoColor = GizmoColor([1.0, 0.3, 0.9, 1.0]);

    /// The same color, `alpha` see-through (0 clear, 1 solid).
    pub fn alpha(self, alpha: f32) -> Self {
        let [r, g, b, _] = self.0;
        GizmoColor([r, g, b, alpha])
    }
}

impl From<[f32; 3]> for GizmoColor {
    fn from([r, g, b]: [f32; 3]) -> Self {
        GizmoColor([r, g, b, 1.0])
    }
}

impl From<[f32; 4]> for GizmoColor {
    fn from(rgba: [f32; 4]) -> Self {
        GizmoColor(rgba)
    }
}

impl From<UiColor> for GizmoColor {
    fn from(color: UiColor) -> Self {
        GizmoColor(color.to_rgba_f32())
    }
}

/// Where a shape is and which way it's turned - from a position alone
/// (`Vector3`, `Point3`, `[x, y, z]`: not turned), a `Transform3D`, an
/// `Isometry3` (Rapier's poses), or `(position, rotation)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vector3<f32>,
    pub rotation: UnitQuaternion<f32>,
}

impl Pose {
    fn at(position: Vector3<f32>) -> Self {
        Pose { position, rotation: UnitQuaternion::identity() }
    }

    /// `local` (in its own frame) in the world.
    pub fn point(&self, local: Vector3<f32>) -> Vector3<f32> {
        self.position + self.rotation * local
    }
}

impl From<Vector3<f32>> for Pose {
    fn from(position: Vector3<f32>) -> Self {
        Pose::at(position)
    }
}

impl From<Point3<f32>> for Pose {
    fn from(position: Point3<f32>) -> Self {
        Pose::at(position.coords)
    }
}

impl From<[f32; 3]> for Pose {
    fn from(position: [f32; 3]) -> Self {
        Pose::at(position.into())
    }
}

impl From<Transform3D> for Pose {
    fn from(transform: Transform3D) -> Self {
        Pose { position: transform.position, rotation: transform.rotation }
    }
}

impl From<&Transform3D> for Pose {
    fn from(transform: &Transform3D) -> Self {
        Pose { position: transform.position, rotation: transform.rotation }
    }
}

impl From<Isometry3<f32>> for Pose {
    fn from(pose: Isometry3<f32>) -> Self {
        Pose { position: pose.translation.vector, rotation: pose.rotation }
    }
}

impl From<&Isometry3<f32>> for Pose {
    fn from(pose: &Isometry3<f32>) -> Self {
        Pose::from(*pose)
    }
}

impl From<(Vector3<f32>, UnitQuaternion<f32>)> for Pose {
    fn from((position, rotation): (Vector3<f32>, UnitQuaternion<f32>)) -> Self {
        Pose { position, rotation }
    }
}

/// A point in the world - a `Vector3`, a `Point3` or `[x, y, z]`.
pub trait GizmoPoint {
    fn to_vector(self) -> Vector3<f32>;
}

impl GizmoPoint for Vector3<f32> {
    fn to_vector(self) -> Vector3<f32> {
        self
    }
}

impl GizmoPoint for &Vector3<f32> {
    fn to_vector(self) -> Vector3<f32> {
        *self
    }
}

impl GizmoPoint for Point3<f32> {
    fn to_vector(self) -> Vector3<f32> {
        self.coords
    }
}

impl GizmoPoint for &Point3<f32> {
    fn to_vector(self) -> Vector3<f32> {
        self.coords
    }
}

impl GizmoPoint for [f32; 3] {
    fn to_vector(self) -> Vector3<f32> {
        self.into()
    }
}

/// How a channel's gizmos look where they don't say - `app.gizmos.style`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoStyle {
    pub color: GizmoColor,
    /// Lines' width (px).
    pub width: f32,
    /// Icons' size (px).
    pub icon_size: f32,
    /// Text's and labels' size (px).
    pub text_size: f32,
}

impl Default for GizmoStyle {
    fn default() -> Self {
        Self { color: GizmoColor::WHITE, width: 2.0, icon_size: 20.0, text_size: 14.0 }
    }
}

impl GizmoStyle {
    pub fn color(&mut self, color: impl Into<GizmoColor>) -> &mut Self {
        self.color = color.into();
        self
    }

    pub fn width(&mut self, pixels: f32) -> &mut Self {
        self.width = pixels;
        self
    }

    pub fn icon_size(&mut self, pixels: f32) -> &mut Self {
        self.icon_size = pixels;
        self
    }

    pub fn text_size(&mut self, pixels: f32) -> &mut Self {
        self.text_size = pixels;
        self
    }
}

/// What a line gizmo draws.
#[derive(Clone, Debug)]
pub(crate) enum Shape {
    Line(Vector3<f32>, Vector3<f32>),
    Path { points: Vec<Vector3<f32>>, closed: bool },
    Cross { center: Vector3<f32>, size: f32 },
    Circle { center: Vector3<f32>, normal: Vector3<f32>, radius: f32 },
    Sphere { center: Vector3<f32>, radius: f32 },
    Cube { pose: Pose, half_extents: Vector3<f32> },
    Axes { pose: Pose, length: f32 },
}

impl Shape {
    /// Where its label goes: a line's end, a path's last point, a shape's
    /// middle.
    pub(crate) fn label_point(&self) -> Vector3<f32> {
        match self {
            Shape::Line(_, end) => *end,
            Shape::Path { points, closed: false } => points.last().copied().unwrap_or_default(),
            Shape::Path { points, closed: true } => points.iter().sum::<Vector3<f32>>() / points.len().max(1) as f32,
            Shape::Cross { center, .. } | Shape::Circle { center, .. } | Shape::Sphere { center, .. } => *center,
            Shape::Cube { pose, .. } | Shape::Axes { pose, .. } => pose.position,
        }
    }

    fn shift(&mut self, shift: Vector3<f32>) {
        match self {
            Shape::Line(start, end) => {
                *start += shift;
                *end += shift;
            }
            Shape::Path { points, .. } => points.iter_mut().for_each(|point| *point += shift),
            Shape::Cross { center, .. } | Shape::Circle { center, .. } | Shape::Sphere { center, .. } => *center += shift,
            Shape::Cube { pose, .. } | Shape::Axes { pose, .. } => pose.position += shift,
        }
    }
}

/// What every gizmo has: its channel, how long it stays, its label.
#[derive(Clone, Debug)]
pub(crate) struct Common {
    pub(crate) channel: Arc<str>,
    /// Game seconds left - 0: this frame only.
    pub(crate) remaining: f32,
    pub(crate) on_top: bool,
    pub(crate) label: Option<String>,
}

/// A line, a path or a shape made of lines - settings chained after it.
#[derive(Clone, Debug)]
pub struct LineGizmo {
    pub(crate) shape: Shape,
    pub(crate) common: Common,
    color: Option<GizmoColor>,
    end_color: Option<GizmoColor>,
    width: Option<f32>,
    pub(crate) arrow: bool,
    pub(crate) dashed: bool,
}

impl LineGizmo {
    /// Its color - the channel's otherwise (red, green, blue for `axes`).
    pub fn color(&mut self, color: impl Into<GizmoColor>) -> &mut Self {
        self.color = Some(color.into());
        self.end_color = None;
        self
    }

    /// Fading from `start`'s color at its start to `end`'s at its end.
    pub fn colors(&mut self, start: impl Into<GizmoColor>, end: impl Into<GizmoColor>) -> &mut Self {
        self.color = Some(start.into());
        self.end_color = Some(end.into());
        self
    }

    /// How wide it is on screen (px) - the channel's otherwise.
    pub fn width(&mut self, pixels: f32) -> &mut Self {
        self.width = Some(pixels);
        self
    }

    /// An arrowhead at its end - a line's, a ray's, a path's.
    pub fn arrow(&mut self) -> &mut Self {
        self.arrow = true;
        self
    }

    /// Dashed instead of solid.
    pub fn dashed(&mut self) -> &mut Self {
        self.dashed = true;
        self
    }

    /// Drawn over everything, not hidden by what's in front of it.
    pub fn on_top(&mut self) -> &mut Self {
        self.common.on_top = true;
        self
    }

    /// `text` by it - at a line's end, a path's last point, a shape's middle.
    pub fn label(&mut self, text: impl Into<String>) -> &mut Self {
        self.common.label = Some(text.into());
        self
    }

    /// Kept up for `seconds` of game time, not only this frame.
    pub fn duration(&mut self, seconds: f32) -> &mut Self {
        self.common.remaining = seconds.max(0.0);
        self
    }

    /// Its colors at its start and end, once its channel's style is in.
    pub(crate) fn resolved_colors(&self) -> Option<(GizmoColor, GizmoColor)> {
        let start = self.color?;
        Some((start, self.end_color.unwrap_or(start)))
    }

    pub(crate) fn resolved_width(&self) -> f32 {
        self.width.unwrap_or(GizmoStyle::default().width)
    }

    fn apply_style(&mut self, style: &GizmoStyle) {
        // Axes keep their own colors unless they're given one.
        if self.color.is_none() && !matches!(self.shape, Shape::Axes { .. }) {
            self.color = Some(style.color);
        }
        self.width.get_or_insert(style.width);
    }
}

/// An icon at a point, facing the camera, the same size on screen at any
/// distance - settings chained after it.
#[derive(Clone, Debug)]
pub struct IconGizmo {
    pub(crate) position: Vector3<f32>,
    pub(crate) icon: Icon,
    pub(crate) common: Common,
    size: Option<f32>,
    color: Option<GizmoColor>,
}

impl IconGizmo {
    /// How big it is on screen (px) - the channel's otherwise.
    pub fn size(&mut self, pixels: f32) -> &mut Self {
        self.size = Some(pixels);
        self
    }

    /// Its color - the channel's otherwise.
    pub fn color(&mut self, color: impl Into<GizmoColor>) -> &mut Self {
        self.color = Some(color.into());
        self
    }

    /// Drawn over everything, not hidden by what's in front of it.
    pub fn on_top(&mut self) -> &mut Self {
        self.common.on_top = true;
        self
    }

    /// `text` above it.
    pub fn label(&mut self, text: impl Into<String>) -> &mut Self {
        self.common.label = Some(text.into());
        self
    }

    /// Kept up for `seconds` of game time, not only this frame.
    pub fn duration(&mut self, seconds: f32) -> &mut Self {
        self.common.remaining = seconds.max(0.0);
        self
    }

    pub(crate) fn resolved_size(&self) -> f32 {
        self.size.unwrap_or(GizmoStyle::default().icon_size)
    }

    pub(crate) fn resolved_color(&self) -> GizmoColor {
        self.color.unwrap_or(GizmoColor::WHITE)
    }

    fn apply_style(&mut self, style: &GizmoStyle) {
        self.size.get_or_insert(style.icon_size);
        self.color.get_or_insert(style.color);
    }
}

/// Text at a point, centered on it, over everything - settings chained
/// after it.
#[derive(Clone, Debug)]
pub struct TextGizmo {
    pub(crate) position: Vector3<f32>,
    pub(crate) text: String,
    pub(crate) common: Common,
    size: Option<f32>,
    color: Option<GizmoColor>,
}

impl TextGizmo {
    /// How tall it is on screen (px) - the channel's otherwise.
    pub fn size(&mut self, pixels: f32) -> &mut Self {
        self.size = Some(pixels);
        self
    }

    /// Its color - the channel's otherwise.
    pub fn color(&mut self, color: impl Into<GizmoColor>) -> &mut Self {
        self.color = Some(color.into());
        self
    }

    /// Kept up for `seconds` of game time, not only this frame.
    pub fn duration(&mut self, seconds: f32) -> &mut Self {
        self.common.remaining = seconds.max(0.0);
        self
    }

    pub(crate) fn resolved_size(&self) -> f32 {
        self.size.unwrap_or(GizmoStyle::default().text_size)
    }

    pub(crate) fn resolved_color(&self) -> GizmoColor {
        self.color.unwrap_or(GizmoColor::WHITE)
    }

    fn apply_style(&mut self, style: &GizmoStyle) {
        self.size.get_or_insert(style.text_size);
        self.color.get_or_insert(style.color);
    }
}

/// The gizmos up now - this frame's, and the ones with a duration left.
#[derive(Default)]
pub(crate) struct GizmoList {
    pub(crate) lines: Vec<LineGizmo>,
    pub(crate) icons: Vec<IconGizmo>,
    pub(crate) texts: Vec<TextGizmo>,
}

impl GizmoList {
    fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.icons.is_empty() && self.texts.is_empty()
    }
}

/// What a channel's closure draws with - see the module doc.
pub struct GizmoDrawer<'a> {
    list: &'a mut GizmoList,
    channel: Arc<str>,
}

impl GizmoDrawer<'_> {
    fn common(&self) -> Common {
        Common { channel: self.channel.clone(), remaining: 0.0, on_top: false, label: None }
    }

    fn shape(&mut self, shape: Shape) -> &mut LineGizmo {
        let common = self.common();
        self.list.lines.push(LineGizmo { shape, common, color: None, end_color: None, width: None, arrow: false, dashed: false });
        self.list.lines.last_mut().expect("just pushed")
    }

    /// A line from `start` to `end`.
    pub fn line(&mut self, start: impl GizmoPoint, end: impl GizmoPoint) -> &mut LineGizmo {
        self.shape(Shape::Line(start.to_vector(), end.to_vector()))
    }

    /// A line from `origin` along `vector` - a force from where it's
    /// applied, a velocity.
    pub fn ray(&mut self, origin: impl GizmoPoint, vector: Vector3<f32>) -> &mut LineGizmo {
        let origin = origin.to_vector();
        self.shape(Shape::Line(origin, origin + vector))
    }

    /// Lines through `points`, in order.
    pub fn path<P: GizmoPoint + Copy>(&mut self, points: &[P]) -> &mut LineGizmo {
        self.shape(Shape::Path { points: points.iter().map(|point| point.to_vector()).collect(), closed: false })
    }

    /// Lines through `points`, in order, and back to the first.
    pub fn outline<P: GizmoPoint + Copy>(&mut self, points: &[P]) -> &mut LineGizmo {
        self.shape(Shape::Path { points: points.iter().map(|point| point.to_vector()).collect(), closed: true })
    }

    /// Three lines across each other at `center`, `size` long (world).
    pub fn cross(&mut self, center: impl GizmoPoint, size: f32) -> &mut LineGizmo {
        self.shape(Shape::Cross { center: center.to_vector(), size })
    }

    /// A circle round `center`, square to `normal`.
    pub fn circle(&mut self, center: impl GizmoPoint, normal: Vector3<f32>, radius: f32) -> &mut LineGizmo {
        self.shape(Shape::Circle { center: center.to_vector(), normal, radius })
    }

    /// A ball's outline - three circles round `center`.
    pub fn sphere(&mut self, center: impl GizmoPoint, radius: f32) -> &mut LineGizmo {
        self.shape(Shape::Sphere { center: center.to_vector(), radius })
    }

    /// A box's 12 edges, `half_extents` from its middle each way, at `pose`
    /// (a position, or a position turned - see `Pose`).
    pub fn cube(&mut self, pose: impl Into<Pose>, half_extents: Vector3<f32>) -> &mut LineGizmo {
        self.shape(Shape::Cube { pose: pose.into(), half_extents })
    }

    /// The local axes at `pose`, `length` long: X red, Y green, Z blue.
    pub fn axes(&mut self, pose: impl Into<Pose>, length: f32) -> &mut LineGizmo {
        self.shape(Shape::Axes { pose: pose.into(), length })
    }

    /// `icon` at `position`.
    pub fn icon(&mut self, position: impl GizmoPoint, icon: Icon) -> &mut IconGizmo {
        let common = self.common();
        self.list.icons.push(IconGizmo { position: position.to_vector(), icon, common, size: None, color: None });
        self.list.icons.last_mut().expect("just pushed")
    }

    /// `text` at `position`.
    pub fn text(&mut self, position: impl GizmoPoint, text: impl Into<String>) -> &mut TextGizmo {
        let mut common = self.common();
        common.on_top = true;
        self.list.texts.push(TextGizmo { position: position.to_vector(), text: text.into(), common, size: None, color: None });
        self.list.texts.last_mut().expect("just pushed")
    }
}

/// A channel: whether it's drawn, and how its gizmos look by default.
#[derive(Clone, Copy, Debug, Default)]
struct Channel {
    disabled: bool,
    style: GizmoStyle,
}

/// `Gizmos`, split for the renderer - see `Gizmos::parts`.
pub(crate) struct GizmoParts<'a> {
    pub(crate) list: &'a GizmoList,
    pub(crate) icons: &'a mut IconLibrary,
    pub(crate) channels: Channels<'a>,
}

/// The channels' settings, read by the renderer.
pub(crate) struct Channels<'a> {
    channels: &'a HashMap<Arc<str>, Channel>,
    visible: bool,
}

impl Channels<'_> {
    /// Whether gizmos of the channel `channel` are drawn now.
    pub(crate) fn shows(&self, channel: &str) -> bool {
        self.visible && self.channels.get(channel).is_none_or(|channel| !channel.disabled)
    }

    /// How big the channel `channel`'s labels are (px).
    pub(crate) fn text_size(&self, channel: &str) -> f32 {
        self.channels.get(channel).map_or(GizmoStyle::default().text_size, |channel| channel.style.text_size)
    }
}

/// The gizmos - `app.gizmos`. See the module doc.
pub struct Gizmos {
    /// Every channel drawn, or none - on by default.
    pub visible: bool,
    /// The icons they can draw - the engine's, and the game's own (see
    /// `IconLibrary::load`).
    pub icons: IconLibrary,
    channels: HashMap<Arc<str>, Channel>,
    pub(crate) list: GizmoList,
}

impl Default for Gizmos {
    fn default() -> Self {
        Self { visible: true, icons: IconLibrary::default(), channels: HashMap::new(), list: GizmoList::default() }
    }
}

impl Gizmos {
    pub fn new() -> Self {
        Self::default()
    }

    /// Draws `draw`'s gizmos in the channel `name` - only while it's on (and
    /// gizmos are `visible`): off, `draw` isn't run.
    pub fn channel(&mut self, name: &str, draw: impl FnOnce(&mut GizmoDrawer)) {
        if !self.visible {
            return;
        }
        let (channel, settings) = match self.channels.get_key_value(name) {
            Some((channel, settings)) => (channel.clone(), *settings),
            None => {
                let channel: Arc<str> = Arc::from(name);
                self.channels.insert(channel.clone(), Channel::default());
                (channel, Channel::default())
            }
        };
        if settings.disabled {
            return;
        }
        let starts = (self.list.lines.len(), self.list.icons.len(), self.list.texts.len());
        draw(&mut GizmoDrawer { list: &mut self.list, channel });
        // What it didn't set, from the channel's style - now its settings
        // are all chained on.
        let style = settings.style;
        self.list.lines[starts.0..].iter_mut().for_each(|line| line.apply_style(&style));
        self.list.icons[starts.1..].iter_mut().for_each(|icon| icon.apply_style(&style));
        self.list.texts[starts.2..].iter_mut().for_each(|text| text.apply_style(&style));
    }

    /// Whether the channel `name` is drawn - a channel not drawn in yet is.
    pub fn is_enabled(&self, name: &str) -> bool {
        self.channels.get(name).is_none_or(|channel| !channel.disabled)
    }

    /// Switches the channel `name` on or off.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) {
        self.channel_mut(name).disabled = !enabled;
    }

    /// Switches the channel `name` the other way - whether it's on now.
    pub fn toggle(&mut self, name: &str) -> bool {
        let channel = self.channel_mut(name);
        channel.disabled = !channel.disabled;
        !channel.disabled
    }

    /// How the channel `name`'s gizmos look where they don't say:
    ///
    ///     app.gizmos.style("aero").color(GizmoColor::BLUE).width(3.0);
    pub fn style(&mut self, name: &str) -> &mut GizmoStyle {
        &mut self.channel_mut(name).style
    }

    /// Every channel known - drawn in or set up - and whether it's on, by
    /// name.
    pub fn channels(&self) -> Vec<(&str, bool)> {
        let mut channels: Vec<(&str, bool)> = self.channels.iter().map(|(name, channel)| (&**name, !channel.disabled)).collect();
        channels.sort();
        channels
    }

    /// Takes every gizmo down, the ones with a duration left too.
    pub fn clear(&mut self) {
        self.list = GizmoList::default();
    }

    fn channel_mut(&mut self, name: &str) -> &mut Channel {
        if !self.channels.contains_key(name) {
            self.channels.insert(Arc::from(name), Channel::default());
        }
        self.channels.get_mut(name).expect("just made sure")
    }

    /// What the renderer reads, apart - so it can draw icons into the
    /// library while it goes through the list.
    pub(crate) fn parts(&mut self) -> GizmoParts<'_> {
        GizmoParts { list: &self.list, icons: &mut self.icons, channels: Channels { channels: &self.channels, visible: self.visible } }
    }

    /// Whether there's anything up at all.
    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// After a frame's drawn: `delta_time` (game seconds) off every gizmo's
    /// duration - the ones with none left come down.
    pub(crate) fn end_frame(&mut self, delta_time: f32) {
        let keep = |common: &mut Common| {
            common.remaining -= delta_time;
            common.remaining > 0.0
        };
        self.list.lines.retain_mut(|line| keep(&mut line.common));
        self.list.icons.retain_mut(|icon| keep(&mut icon.common));
        self.list.texts.retain_mut(|text| keep(&mut text.common));
    }

    /// Moves every gizmo by `shift`, with the floating origin (see
    /// `WorldOrigin`).
    pub(crate) fn shift_origin(&mut self, shift: Vector3<f32>) {
        self.list.lines.iter_mut().for_each(|line| line.shape.shift(shift));
        self.list.icons.iter_mut().for_each(|icon| icon.position += shift);
        self.list.texts.iter_mut().for_each(|text| text.position += shift);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_chain_and_the_channel_fills_in_the_rest() {
        let mut gizmos = Gizmos::new();
        gizmos.style("aero").color(GizmoColor::BLUE).width(3.0);
        gizmos.channel("aero", |g| {
            g.line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
            g.ray(Vector3::zeros(), Vector3::y()).color(GizmoColor::RED).width(5.0).arrow().dashed().label("lift");
            g.icon(Vector3::zeros(), Icon::Mass).size(30.0);
            g.text(Vector3::zeros(), "hi");
        });
        let [plain, ray] = &gizmos.list.lines[..] else { panic!("two lines") };
        assert_eq!(plain.resolved_colors(), Some((GizmoColor::BLUE, GizmoColor::BLUE)));
        assert_eq!(plain.resolved_width(), 3.0);
        assert_eq!(ray.resolved_colors(), Some((GizmoColor::RED, GizmoColor::RED)));
        assert_eq!(ray.resolved_width(), 5.0);
        assert!(ray.arrow && ray.dashed);
        assert_eq!(ray.common.label.as_deref(), Some("lift"));
        assert_eq!(gizmos.list.icons[0].resolved_size(), 30.0);
        assert_eq!(gizmos.list.icons[0].resolved_color(), GizmoColor::BLUE);
        assert!(gizmos.list.texts[0].common.on_top);
    }

    #[test]
    fn a_channel_off_doesnt_run() {
        let mut gizmos = Gizmos::new();
        gizmos.set_enabled("aero", false);
        let mut ran = false;
        gizmos.channel("aero", |_| ran = true);
        assert!(!ran);
        assert!(gizmos.toggle("aero"));
        gizmos.channel("aero", |_| ran = true);
        assert!(ran);
        gizmos.visible = false;
        ran = false;
        gizmos.channel("aero", |_| ran = true);
        assert!(!ran);
    }

    #[test]
    fn a_gizmo_lasts_its_duration() {
        let mut gizmos = Gizmos::new();
        gizmos.channel("hits", |g| {
            g.cross(Vector3::zeros(), 1.0).duration(0.5);
            g.cross(Vector3::zeros(), 1.0);
        });
        gizmos.end_frame(0.2);
        assert_eq!(gizmos.list.lines.len(), 1, "the one with no duration is gone");
        gizmos.end_frame(0.2);
        assert_eq!(gizmos.list.lines.len(), 1);
        gizmos.end_frame(0.2);
        assert!(gizmos.is_empty());
    }

    #[test]
    fn axes_keep_their_colors() {
        let mut gizmos = Gizmos::new();
        gizmos.channel("a", |g| {
            g.axes(Vector3::zeros(), 1.0);
        });
        assert_eq!(gizmos.list.lines[0].resolved_colors(), None);
    }
}
