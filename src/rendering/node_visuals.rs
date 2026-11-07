//! What nodes draw besides their models: `Shape2D`, `Text` and `Line`.
//!
//! Refilled every frame from the scene's nodes (`App::prepare_node_visuals`).
//! A 3D node's text and lines are the frame plan's `Nodes3D` step, over the
//! finished world - first what the world's depth can hide, then what's always
//! on top. A 2D node's visuals are its `Nodes2D` step, each kind in runs, so
//! they stay in their order however the kinds are mixed.
//!
//! The renderers are made the first time a scene needs them: a text one has
//! its own glyph atlas, which a scene without text shouldn't pay for.

use std::ops::Range;

use nalgebra::{Vector2, Vector3};

use crate::app::App;
use crate::rendering::render_pipeline::render_pass::{color_attachment, depth_attachment};
use crate::rendering::renderer::Renderer;
use crate::rendering::sdf_text::{srgb_to_linear, SdfText, TextFrame};
use crate::rendering::shader_library::Overlay;
use crate::rendering::shapes_2d::ShapeInstance;
use crate::rendering::thick_lines::{LineInstance, ThickLines};
use crate::scene_manager::node::properties::{Line, Shape2D, ShapeKind, Text};
use crate::ui::color::UiColor;

/// The material all nodes' lines are drawn with (see App::set_up_materials).
pub(crate) const NODE_LINES: &str = "node_lines";
/// The material all nodes' text is drawn with.
pub(crate) const NODE_TEXT: &str = "node_text";

#[derive(Clone, Copy, PartialEq)]
enum Kind2D {
    Shapes,
    Lines,
    Text,
}

/// Some of a 2D kind in a row - a range of its renderer's instances.
struct Run2D {
    kind: Kind2D,
    range: Range<u32>,
}

/// One 2D visual, waiting for its turn in the drawing order.
enum Visual2D<'a> {
    Shape(ShapeInstance),
    Line(LineInstance),
    Text { text: &'a Text, frame: TextFrame },
}

#[derive(Default)]
pub struct NodeVisuals {
    /// What the world's depth can hide first, then what's always on top.
    world_lines: Option<ThickLines>,
    world_text: Option<SdfText>,
    /// How many of `world_lines`' segments can be hidden.
    hidden_lines: u32,
    /// How many of `world_text`'s glyphs can be hidden.
    hidden_glyphs: u32,
    screen_lines: Option<ThickLines>,
    screen_text: Option<SdfText>,
    runs_2d: Vec<Run2D>,
}

impl NodeVisuals {
    pub(crate) fn uses_lines(&self) -> bool {
        self.world_lines.is_some() || self.screen_lines.is_some()
    }

    pub(crate) fn uses_text(&self) -> bool {
        self.world_text.is_some() || self.screen_text.is_some()
    }

    fn clear(&mut self) {
        for lines in [&mut self.world_lines, &mut self.screen_lines].into_iter().flatten() {
            lines.dynamic.clear();
        }
        for text in [&mut self.world_text, &mut self.screen_text].into_iter().flatten() {
            text.clear();
        }
        self.hidden_lines = 0;
        self.hidden_glyphs = 0;
        self.runs_2d.clear();
    }
}

/// Says once per node when a line made with 2D points sits on a 3D node, or
/// the other way round. It's still drawn.
fn warn_if_line_mismatched(node: &str, line: &Line, on_2d_node: bool) {
    if line.is_2d() == on_2d_node {
        return;
    }
    thread_local! {
        static WARNED: std::cell::RefCell<std::collections::HashSet<String>> = Default::default();
    }
    WARNED.with(|warned| {
        if warned.borrow_mut().insert(node.to_owned()) {
            let (made, node_kind) = if line.is_2d() { ("2D", "3D") } else { ("3D", "2D") };
            eprintln!("Node '{node}': a line made with {made} points is on a {node_kind} node");
        }
    });
}

fn new_lines(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, screen_space: bool) -> ThickLines {
    let mut lines = ThickLines::new(device, layout);
    lines.screen_space = screen_space;
    lines
}

fn new_text(device: &wgpu::Device, format: wgpu::TextureFormat, layout: &wgpu::BindGroupLayout, screen_space: bool) -> SdfText {
    let mut text = SdfText::new(device, format, layout);
    text.screen_space = screen_space;
    text
}

impl App {
    /// Hands the nodes' shapes, text and lines to the renderer. 2D visuals go
    /// in the order they're drawn: by z index (the visual's own, or its
    /// node's), then by when each was put on - the later in front. Their
    /// units are the UI's (see `Transform2D`), so they're turned into pixels
    /// the way the UI's are.
    pub(crate) fn prepare_node_visuals(&mut self) {
        let Renderer { node_visuals: visuals, device, queue, config, shaders, shapes_2d, .. } = &mut self.renderer;
        visuals.clear();
        let mut shapes: Vec<ShapeInstance> = Vec::new();
        let screen_px = (config.width, config.height);
        if let Some(content) = self.scene_manager.content() {
            let pixels_per_unit = self.window.pixel_size.width as f32 / self.ui.layout_size().width as f32;
            let srgb = config.format.is_srgb();
            let line_color = |color: &UiColor| {
                let [r, g, b, a] = color.to_rgba_f32();
                if srgb { [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a] } else { [r, g, b, a] }
            };
            // Which way the camera's right and up are, for text facing it.
            let facing = self.scene_manager.cameras().filter(|cameras| cameras.has_active_camera()).and_then(|cameras| {
                let camera = &cameras.active().camera;
                let right = camera.calc_forward_direction().cross(&camera.up).try_normalize(1e-6)?;
                Some((right, right.cross(&camera.calc_forward_direction())))
            });

            let mut visuals_2d: Vec<((i32, u64), Visual2D)> = Vec::new();
            let mut world_lines: [Vec<LineInstance>; 2] = Default::default();
            let mut world_text: [Vec<(&Text, TextFrame)>; 2] = Default::default();
            for node in content.nodes.iter() {
                if let Some(transform) = node.transform_2d() {
                    if let Some(shape) = node.get_property::<Shape2D>() {
                        let placed = transform.place_visual(&shape.transform, shape.upright);
                        let (half_size, kind) = match shape.kind {
                            ShapeKind::Rect { width, height } => ([width / 2.0 * placed.scale.x, height / 2.0 * placed.scale.y], ShapeInstance::RECT),
                            ShapeKind::Circle { radius } => {
                                let radius = radius * placed.scale.x.max(placed.scale.y);
                                ([radius, radius], ShapeInstance::CIRCLE)
                            }
                        };
                        let (sin, cos) = placed.rotation.sin_cos();
                        let order = (placed.z_index, node.property_order::<Shape2D>().unwrap_or(0));
                        visuals_2d.push((order, Visual2D::Shape(ShapeInstance {
                            center: [placed.position.x * pixels_per_unit, placed.position.y * pixels_per_unit],
                            half_size: half_size.map(|half| half * pixels_per_unit),
                            rotation: [cos, sin],
                            kind: [kind, 0.0],
                            color: shape.color.to_rgba_f32(),
                        })));
                    }
                    if let Some(text) = node.get_property::<Text>() {
                        let placed = transform.place_visual(&text.transform, text.upright);
                        let (sin, cos) = placed.rotation.sin_cos();
                        let size = text.plane_size() * pixels_per_unit;
                        let frame = TextFrame {
                            center: Vector3::new(placed.position.x, placed.position.y, 0.0) * pixels_per_unit,
                            right: Vector3::new(cos, sin, 0.0) * size * placed.scale.x,
                            down: Vector3::new(-sin, cos, 0.0) * size * placed.scale.y,
                        };
                        let order = (placed.z_index, node.property_order::<Text>().unwrap_or(0));
                        visuals_2d.push((order, Visual2D::Text { text, frame }));
                    }
                    if let Some(line) = node.get_property::<Line>() {
                        warn_if_line_mismatched(&node.id, line, true);
                        let [start, end] = line.points_2d(transform);
                        let to_pixels = |point: Vector2<f32>| [point.x * pixels_per_unit, point.y * pixels_per_unit, 0.0];
                        let order = (transform.z_index + line.z_index, node.property_order::<Line>().unwrap_or(0));
                        visuals_2d.push((order, Visual2D::Line(LineInstance {
                            start: to_pixels(start),
                            end: to_pixels(end),
                            color: line_color(&line.color),
                            width: line.width,
                        })));
                    }
                } else if let Some(transform) = node.transform_3d() {
                    if let Some(line) = node.get_property::<Line>() {
                        warn_if_line_mismatched(&node.id, line, false);
                        let [start, end] = line.points_3d(transform);
                        world_lines[line.on_top as usize].push(LineInstance {
                            start: start.into(),
                            end: end.into(),
                            color: line_color(&line.color),
                            width: line.width,
                        });
                    }
                    if let Some(text) = node.get_property::<Text>() {
                        let (right, up) = match facing.filter(|_| text.faces_camera) {
                            Some(facing) => facing,
                            None => (transform.rotation * Vector3::x(), transform.rotation * Vector3::y()),
                        };
                        let size = text.world_size();
                        let frame = TextFrame { center: transform.position, right: right * size * transform.scale.x, down: -up * size * transform.scale.y };
                        world_text[text.on_top as usize].push((text, frame));
                    }
                }
            }

            // The 3D ones: first what can be hidden, then what can't.
            let [hidden_lines, top_lines] = world_lines;
            if !hidden_lines.is_empty() || !top_lines.is_empty() {
                let lines = visuals.world_lines.get_or_insert_with(|| new_lines(device, shaders.overlay_layout(Overlay::Lines), false));
                visuals.hidden_lines = hidden_lines.len() as u32;
                lines.dynamic.extend(hidden_lines);
                lines.dynamic.extend(top_lines);
            }
            let [hidden_text, top_text] = world_text;
            if !hidden_text.is_empty() || !top_text.is_empty() {
                let renderer = visuals.world_text.get_or_insert_with(|| new_text(device, config.format, shaders.overlay_layout(Overlay::Text), false));
                for (text, frame) in hidden_text {
                    renderer.push(&text.text, text.font, frame, text.color.to_rgba_f32());
                }
                visuals.hidden_glyphs = renderer.instance_count();
                for (text, frame) in top_text {
                    renderer.push(&text.text, text.font, frame, text.color.to_rgba_f32());
                }
            }

            // The 2D ones, in order - each kind's run goes on while the next
            // visual is the same kind.
            visuals_2d.sort_by_key(|(order, _)| *order);
            for (_, visual) in visuals_2d {
                let (kind, instance) = match visual {
                    Visual2D::Shape(shape) => {
                        shapes.push(shape);
                        (Kind2D::Shapes, shapes.len() as u32 - 1..shapes.len() as u32)
                    }
                    Visual2D::Line(line) => {
                        let lines = visuals.screen_lines.get_or_insert_with(|| new_lines(device, shaders.overlay_layout(Overlay::Lines), true));
                        lines.dynamic.push(line);
                        (Kind2D::Lines, lines.dynamic.len() as u32 - 1..lines.dynamic.len() as u32)
                    }
                    Visual2D::Text { text, frame } => {
                        let renderer = visuals.screen_text.get_or_insert_with(|| new_text(device, config.format, shaders.overlay_layout(Overlay::Text), true));
                        let start = renderer.instance_count();
                        renderer.push(&text.text, text.font, frame, text.color.to_rgba_f32());
                        (Kind2D::Text, start..renderer.instance_count())
                    }
                };
                match visuals.runs_2d.last_mut().filter(|run| run.kind == kind) {
                    Some(run) => run.range.end = instance.end,
                    None => visuals.runs_2d.push(Run2D { kind, range: instance }),
                }
            }
        }
        shapes_2d.set(device, queue, &shapes, screen_px);
    }

    /// The frame plan's `Nodes3D` step: the 3D nodes' lines and text, over
    /// `view`. Hidden by the world's depth - except `on_target`, drawing into
    /// a game's own image, whose size the depth needn't match: there they're
    /// all on top.
    pub(crate) fn render_nodes_3d(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, on_target: bool) {
        let Some(cameras) = self.scene_manager.cameras().filter(|cameras| cameras.has_active_camera()) else { return };
        let camera_position = cameras.active().camera.position();
        let camera_position = [camera_position.x, camera_position.y, camera_position.z];
        let screen = [self.window.pixel_size.width as f32, self.window.pixel_size.height as f32];
        let Renderer { node_visuals: visuals, device, queue, shaders, overlay_materials, camera_resources, light, depth_render, .. } = &mut self.renderer;

        let mut line_count = 0;
        if let Some(lines) = &mut visuals.world_lines {
            lines.prepare(device, queue, camera_position, screen);
            line_count = lines.dynamic.len() as u32;
        }
        let mut glyph_count = 0;
        if let Some(text) = &mut visuals.world_text {
            text.prepare(device, queue, camera_position, screen);
            glyph_count = text.instance_count();
        }
        let hidden_lines = if on_target { 0 } else { visuals.hidden_lines };
        let hidden_glyphs = if on_target { 0 } else { visuals.hidden_glyphs };
        let line_key = overlay_materials.get(NODE_LINES);
        let text_key = overlay_materials.get(NODE_TEXT);
        let camera = &camera_resources.bind_group;
        let light = &light.rendering_data.bind_group;

        if hidden_lines > 0 || hidden_glyphs > 0 {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D node visuals pass"),
                color_attachments: &[color_attachment(view, wgpu::LoadOp::Load)],
                depth_stencil_attachment: depth_attachment(&depth_render.texture.view, wgpu::LoadOp::Load),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            if let (Some(lines), Some(material)) = (&visuals.world_lines, line_key.and_then(|key| shaders.overlay_depth_tested(key, Overlay::Lines))) {
                lines.draw_range(&mut render_pass, material, camera, light, 0..hidden_lines);
            }
            if let (Some(text), Some(material)) = (&visuals.world_text, text_key.and_then(|key| shaders.overlay_depth_tested(key, Overlay::Text))) {
                text.draw_range(&mut render_pass, material, camera, light, 0..hidden_glyphs);
            }
        }
        if hidden_lines < line_count || hidden_glyphs < glyph_count {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("3D node visuals on top pass"),
                color_attachments: &[color_attachment(view, wgpu::LoadOp::Load)],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            if let (Some(lines), Some(material)) = (&visuals.world_lines, line_key.and_then(|key| shaders.overlay(key, Overlay::Lines))) {
                lines.draw_range(&mut render_pass, material, camera, light, hidden_lines..line_count);
            }
            if let (Some(text), Some(material)) = (&visuals.world_text, text_key.and_then(|key| shaders.overlay(key, Overlay::Text))) {
                text.draw_range(&mut render_pass, material, camera, light, hidden_glyphs..glyph_count);
            }
        }
    }

    /// The frame plan's `Nodes2D` step: the 2D nodes' visuals, in their order,
    /// over `view`.
    pub(crate) fn render_nodes_2d(&mut self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let screen = [self.window.pixel_size.width as f32, self.window.pixel_size.height as f32];
        let Renderer { node_visuals: visuals, device, queue, shaders, overlay_materials, camera_resources, light, shapes_2d, .. } = &mut self.renderer;
        if visuals.runs_2d.is_empty() {
            return;
        }
        if let Some(lines) = &mut visuals.screen_lines {
            lines.prepare(device, queue, [0.0; 3], screen);
        }
        if let Some(text) = &mut visuals.screen_text {
            text.prepare(device, queue, [0.0; 3], screen);
        }
        let line_material = overlay_materials.get(NODE_LINES).and_then(|key| shaders.overlay(key, Overlay::Lines));
        let text_material = overlay_materials.get(NODE_TEXT).and_then(|key| shaders.overlay(key, Overlay::Text));
        let camera = &camera_resources.bind_group;
        let light = &light.rendering_data.bind_group;

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("2D node visuals pass"),
            color_attachments: &[color_attachment(view, wgpu::LoadOp::Load)],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        for run in &visuals.runs_2d {
            match run.kind {
                Kind2D::Shapes => shapes_2d.draw_range(&mut render_pass, run.range.clone()),
                Kind2D::Lines => {
                    if let (Some(lines), Some(material)) = (&visuals.screen_lines, line_material) {
                        lines.draw_range(&mut render_pass, material, camera, light, run.range.clone());
                    }
                }
                Kind2D::Text => {
                    if let (Some(text), Some(material)) = (&visuals.screen_text, text_material) {
                        text.draw_range(&mut render_pass, material, camera, light, run.range.clone());
                    }
                }
            }
        }
    }
}
