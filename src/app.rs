use std::collections::HashMap;
use std::env;
use std::hash::{DefaultHasher, Hash, Hasher};

use glyphon::TextArea;
use nalgebra::Vector3;
use sdl2::EventPump;

use crate::asset_manager::AssetManager;
use crate::audio::audio::{Audio, Sound};
use crate::timing::Timing;
use crate::input::input::Input;
use crate::particles::Emitter;
use crate::particles::renderer::{EmitterSource, ParticleFrameInfo, ParticleRenderer};
use crate::physics::PhysicsWorld;
use crate::rendering::egui_overlay::DebugUi;
use crate::rendering::graphics_backend::GraphicsBackend;
use crate::rendering::enviroment::environment::PreparedEnvironment;
use crate::rendering::instance_management::{InstanceRaw, ShadowRun};
use crate::rendering::render_pipeline::shadow_renderer::hash_floats;
use crate::rendering::renderer::Renderer;
use crate::rendering::ui::ui::Ui;
use crate::scene_manager::node::properties::{Light, LightKind, UiLayer};
use crate::scene_manager::node::Node;
use crate::scene_manager::scene_access::{SceneAccess, SceneChanges};
use crate::scene_manager::manager::SceneManager;
use crate::scene_manager::scene::{FrameContext, Scene};
use crate::ui::color::UiColor;
use crate::ui::ui_node::{ClickHandler, UiNode};
use crate::ui::ui_transform::PositionValue;
use crate::window::Window;
use crate::world_origin::WorldOrigin;
use crate::scene_manager::node::properties::OriginAnchor;

#[derive(Clone)]
pub struct AppState {
    pub is_running: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32
}

/*
    # App

    The engine: one window, the systems that run for as long as the app does,
    and the scene currently open. Set it up, then hand control to `run`:

        let mut app = App::new().await?;
        app.window.set_title("My Game");
        app.physics.set_gravity(Vector3::new(0.0, -9.81, 0.0));
        app.input.load_bindings("settings/input.ron")?;
        app.scene_manager.create_scene("level", Level::new);
        app.scene_manager.open_scene("level");
        app.run();

    - window: title, size, window mode, screen, mouse lock.
    - assets: the assets folder - models are named by their path in it.
    - renderer: everything that draws - and what's drawn behind everything
      (set_environment).
    - scene_manager: the scenes, and the one open now.
    - physics: the open scene's physics world, and the gravity and timestep
      every scene runs with (see PhysicsWorld).
    - particles: every node's particles and trails, and the wind and sea
      they move in (see ParticleRenderer).
    - origin: the floating origin - where the open scene's (0, 0, 0) is in
      the world, moved along to keep positions precise far out (see
      WorldOrigin).

    Each system's settings are its own, and global: set once, they hold for
    every scene until they're set again. The sun is a node (see Light).
    - ui, audio, time (delta time, time scale), debug_ui (the game's egui
      debug windows).
    - input: keys, mouse, controllers - and the game's bindings (see
      Input: load_bindings, set_user_bindings_path).
*/
pub struct App {
    pub window: Window,
    pub renderer: Renderer,
    pub assets: AssetManager,
    pub scene_manager: SceneManager,
    pub ui: Ui,
    pub audio: Audio,
    pub time: Timing,
    pub debug_ui: DebugUi,
    pub physics: PhysicsWorld,
    pub input: Input,
    pub particles: ParticleRenderer,
    pub origin: WorldOrigin,
    pub should_quit: bool,
    event_pump: Option<EventPump>,
}

impl App {
    pub async fn new() -> Result<App, String> {
        Self::new_with_backend(GraphicsBackend::Auto).await
    }

    // Creates a window with a backend allowing the user to define the backend in particular it wants to use
    pub async fn new_with_backend(backend: GraphicsBackend) -> Result<App, String> {
        env::set_var("SDL_VIDEO_MINIMIZE_ON_FOCUS_LOSS", "0");
        let sdl = sdl2::init()?;
        let window = Window::new(sdl.video()?)?;
        let input = Input::new(sdl.game_controller()?, sdl.joystick()?);
        let event_pump = sdl.event_pump()?;

        GraphicsBackend::available();

        let renderer = Renderer::new(&window, backend).await?;
        let particles = ParticleRenderer::new(&renderer.device, &renderer.queue, renderer.config.format, &renderer.camera_resources.bind_group_layout, &renderer.depth_render.foam_depth_copy.view);

        let ui = Ui::new(
            &renderer.device,
            &renderer.queue,
            &renderer.config,
            &renderer.glyphon.cache,
            &renderer.blur,
            renderer.shaders.overlay_layout(crate::rendering::shader_library::Overlay::UiBox),
        );
        let assets = AssetManager::new(renderer.shaders.overlay_layout(crate::rendering::shader_library::Overlay::UiImage).clone(), crate::rendering::models::textures::Texture::create_bind_group_layout(&renderer.device));
        let debug_ui = DebugUi::new(&renderer.device, renderer.config.format);

        Ok(App {
            window,
            renderer,
            assets,
            scene_manager: SceneManager::new(),
            ui,
            audio: Audio::new(),
            time: Timing::new(),
            debug_ui,
            physics: PhysicsWorld::new(),
            input,
            particles,
            origin: WorldOrigin::new(),
            should_quit: false,
            event_pump: Some(event_pump),
        })
    }

    fn sync_window_size(&mut self) {
        let (width, height) = self.window.sdl_window().drawable_size();
        if width != self.renderer.config.width || height != self.renderer.config.height {
            self.resize();
        } else if self.ui.update_scale(self.window.size.width, self.window.size.height) {
            // The UI's scaling changed (app.ui.scaling / scale), the window didn't.
            self.ui.relayout();
        }
    }

    pub(crate) fn resize(&mut self) {
        self.window.refresh_size();
        // Real backing pixels (see Window::pixel_size) - these size actual GPU
        // render targets/aspect ratio, which have to match the surface's real
        // resolution, not the window's points size. UI layout/hit-testing
        // stays on window.size (points) - see that field's own doc comment.
        let width = self.window.pixel_size.width;
        let height = self.window.pixel_size.height;
        // Minimized - a 0-sized surface can't be configured. Restoring it
        // changes the size again, which lands back here.
        if width == 0 || height == 0 {
            return;
        }

        self.renderer.resize(width, height);
        // The depth copy particles fade against was just rebuilt.
        self.particles.rebuild_depth_bind_group(&self.renderer.device, &self.renderer.depth_render.foam_depth_copy.view);
        if let Some(cameras) = self.scene_manager.cameras_mut() {
            cameras.resize(width, height);
        }
        for scene in self.scene_manager.waiting_scenes_mut() {
            scene.cameras.resize(width, height);
        }
        self.ui.resize_blur_binding(&self.renderer.device, &self.renderer.queue, &self.renderer.blur, width, height);

        // The UI's scale follows the window's size (see UiScaling), and its
        // layout the new room.
        self.ui.update_scale(self.window.size.width, self.window.size.height);
        self.ui.relayout();
    }

    /// The environment drawn now (see `Renderer::set_environment`).
    pub(crate) fn current_environment(&self) -> &PreparedEnvironment {
        &self.renderer.prepared_environment
    }

    /// The active scene's sun: toward it (normalized), the light it gives,
    /// and whether it casts shadows - from its first node with a directional
    /// `Light` and a `Transform3D`, which shines along the node's +Z. None:
    /// no sun.
    fn sun(&self) -> Option<(Vector3<f32>, [f32; 3], bool)> {
        let content = self.scene_manager.content()?;
        content.nodes.iter().find_map(|node| {
            let light = node.get_property::<Light>().filter(|light| light.kind == LightKind::Directional)?;
            let transform = node.transform_3d()?;
            let toward = -(transform.rotation * Vector3::z());
            Some((toward, light.radiance(), light.shadows))
        })
    }

    /// Builds the environment again when it changed - unless a scene's
    /// assets are loading, which builds it with them - and fills the light
    /// buffer and the sky's sun disc from the active scene's sun.
    fn update_lighting(&mut self, camera_position: Vector3<f32>) {
        if self.renderer.environment() != &self.renderer.prepared_from && !self.scene_manager.is_switching() {
            let environment = self.renderer.environment().clone();
            self.renderer.prepared_environment = self.renderer.prepare_environment(environment.clone());
            self.renderer.prepared_from = environment;
        }

        // The sun is infinitely far away - its direction is placed that far
        // along in the shaders' camera-relative space, so it's one fixed
        // direction for every surface on screen. The sky's sun disc follows
        // the same direction, so disc, lighting and glint all agree. No sun:
        // no sunlight, no disc - only the ambient light.
        const SUN_DISTANCE: f32 = 1.0e9;
        let sun = self.sun();
        if let Some((toward, radiance, _)) = sun {
            let light_position = toward * SUN_DISTANCE;
            self.renderer.light.uniform.position = [light_position.x, light_position.y, light_position.z];
            self.renderer.light.uniform.color = radiance;
        } else {
            self.renderer.light.uniform.color = [0.0; 3];
        }

        // The sun's shadows - only with a camera to see them (the shadow map
        // follows it). Without, the shaders treat everything as sunlit.
        let has_camera = self.scene_manager.cameras().is_some_and(|cameras| cameras.has_active_camera());
        let shadowing = sun.filter(|(_, _, shadows)| *shadows && has_camera && self.renderer.shadows().enabled);
        self.renderer.shadows.active = shadowing.is_some();
        if let Some((toward, _, _)) = shadowing {
            let settings = self.renderer.shadows();
            let fit = self.renderer.shadows.update(&self.renderer.queue, toward, camera_position, &settings);
            let cascades = self.renderer.shadows.cascades.len() as f32;
            let resolution = self.renderer.shadows.resolution() as f32;
            self.renderer.light.uniform.shadow_view_proj = fit.view_proj;
            self.renderer.light.uniform.shadow_texels = fit.texel;
            self.renderer.light.uniform.shadow_params = [cascades, 1.0 / resolution, settings.softness, settings.blur.max(0.0)];
        } else {
            self.renderer.light.uniform.shadow_params[0] = 0.0;
        }
        self.renderer.light.uniform.ambient = self.renderer.ambient_light();
        self.renderer.light.uniform.ambient_ground = self.renderer.ambient_ground();
        self.renderer.light.uniform.time = self.time.shader_time();
        self.renderer.light.uniform.camera_position = camera_position.into();
        self.renderer.queue.write_buffer(&self.renderer.light.rendering_data.buffer, 0, bytemuck::cast_slice(&[self.renderer.light.uniform]));
        // The same light, unshadowed - for see-through meshes when shadows
        // don't fall on them (Shadows::on_transparent).
        if !self.renderer.shadows().on_transparent {
            let mut unshadowed = self.renderer.light.uniform;
            unshadowed.shadow_params[0] = 0.0;
            self.renderer.queue.write_buffer(&self.renderer.light.rendering_data.no_shadows_buffer, 0, bytemuck::cast_slice(&[unshadowed]));
        }
        let queue = self.renderer.queue.clone();
        if let Some(skybox) = self.renderer.prepared_environment.skybox.as_mut() {
            skybox.set_sun(&queue, sun.map(|(toward, _, _)| toward));
        }
    }

    /// Feeds every node's `Emitter` (at wherever the node is being
    /// drawn this frame) to the particle renderer - see ParticleRenderer::
    /// update. Once per frame, after the camera has moved.
    fn update_particles(&mut self) {
        let Some(cameras) = self.scene_manager.cameras() else { return };
        if !cameras.has_active_camera() {
            return;
        }
        let active = cameras.active();
        let camera_position = active.camera.position().coords;
        let light = &self.renderer.light.uniform;
        let frame = ParticleFrameInfo {
            camera_position,
            camera_view: active.camera.calc_matrix(),
            near: active.projection.znear,
            far: active.projection.zfar(),
            orthographic: active.projection.is_orthographic(),
            delta_time: self.time.delta_time,
            gravity: self.physics.gravity(),
            sun_direction: Vector3::from(light.position),
            sun_color: Vector3::from(light.color),
            ambient: Vector3::from(light.ambient),
            origin_y: self.origin.origin().y,
        };

        let Some(content) = self.scene_manager.content() else { return };
        let sources = content.nodes.iter().filter_map(|node| {
            let emitter = node.get_property::<Emitter>()?;
            // Where the node is being drawn - its render instance if it has
            // one (moved by physics), otherwise its own Transform3D.
            let (position, rotation) = content.renderizable_instances.get(&node.id)
                .map(|instance| (instance.transform.position, instance.transform.rotation))
                .or_else(|| node.transform_3d().map(|transform| (transform.position, transform.rotation)))?;
            Some(EmitterSource { node_id: &node.id, emitter, position, rotation })
        });
        self.particles.update(&self.renderer.device, &self.renderer.queue, &frame, sources);
    }

    /// Plays the sound file `path` (inside the assets folder) - read the
    /// first time, then kept (see `SoundCache`). Set it up by chaining - see
    /// engine::audio::audio. Nothing plays when the file can't be read (said
    /// once) or there's no audio device.
    pub fn play_sound(&mut self, path: &str) -> Sound<'_> {
        match self.assets.sounds.load_for_playing(path) {
            Some(clip) => self.audio.play(clip),
            None => self.audio.nothing(),
        }
    }

    /// Registers a line set for the frame plan's `Lines` steps - see
    /// engine::rendering::render_sets. `screen_space`: in screen pixels, not
    /// the world.
    pub fn add_line_set(&mut self, name: &str, screen_space: bool) {
        let mut lines = crate::rendering::thick_lines::ThickLines::new(&self.renderer.device, self.renderer.shaders.overlay_layout(crate::rendering::shader_library::Overlay::Lines));
        lines.screen_space = screen_space;
        self.renderer.render_sets.insert_lines(name, lines);
    }

    /// Registers an SDF text set for the frame plan's `Text` steps (see
    /// `add_line_set`).
    pub fn add_text_set(&mut self, name: &str, screen_space: bool) {
        let mut text = crate::rendering::sdf_text::SdfText::new(&self.renderer.device, self.renderer.config.format, self.renderer.shaders.overlay_layout(crate::rendering::shader_library::Overlay::Text));
        text.screen_space = screen_space;
        self.renderer.render_sets.insert_text(name, text);
    }

    pub(crate) fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        self.sync_no_camera_message();
        let ui_height = self.ui.size().1;
        crate::ui::widgets::toast::update_toasts(&mut self.ui, self.time.unscaled_delta_time, ui_height);
        self.prepare_ui_content();
        self.prepare_node_visuals();
        self.apply_cursor();
        self.fire_ui_click_handlers();

        // WGPU - getting the screen's next image can wait on the GPU or vsync.
        let output = self.renderer.surface.get_current_texture()?;

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.renderer.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Render Encoder"),
        });

        self.render_scene_passes(&mut encoder, &output.texture, &view);
        // The game's debug windows, over everything (see DebugUi) - only
        // built on the frames they're up.
        let screen_size_px = [self.renderer.config.width, self.renderer.config.height];
        self.debug_ui.paint(&self.renderer.device, &self.renderer.queue, &mut encoder, &view, screen_size_px);

        self.renderer.queue.submit(std::iter::once(encoder.finish()));
        // Drawn - the game fills it again for the next frame.
        self.renderer.lines.clear();
        output.present();

        Ok(())
    }

    // Reserved UI key for the message below - never something a scene should
    // add/remove itself.
    const NO_CAMERA_MESSAGE_KEY: &str = "__no_camera_message";

    // Ensures/removes the "Add a camera to the scene" label to match the
    // active scene's SceneCameras::has_active_camera() each frame -
    // render_pass.rs's own render_opaque_pass already clears to black and
    // skips every draw in that case (see its own comment); this is the other
    // half, the actual visible text, via the ordinary UI pass, which runs
    // unconditionally on top regardless of what the 3D passes did.
    fn sync_no_camera_message(&mut self) {
        // Suppressed while: a real camera exists; a scene is being opened;
        // or no scene has opened yet at all.
        if self.scene_manager.cameras().is_some_and(|c| c.has_active_camera()) || self.scene_manager.is_switching() || self.scene_manager.active_scene.is_none() {
            if self.ui.renderizable_elements.remove(Self::NO_CAMERA_MESSAGE_KEY).is_some() {
                self.ui.has_changed = true;
            }
            return;
        }

        if self.ui.renderizable_elements.contains_key(Self::NO_CAMERA_MESSAGE_KEY) {
            return;
        }

        let (screen_width, screen_height) = self.ui.size();
        let mut label = UiNode::label("Add a camera to the scene", Some(500.0), Some(40.0))
            .set_position(PositionValue::Center(0.0), PositionValue::Center(0.0))
            .set_text_color(UiColor::Rgb(255, 255, 255))
            // set_position alone only centers the label's own box on screen -
            // text inside it defaults to left-aligned (see ui_node.rs's
            // effective_style.align.unwrap_or(Align::Left)), which reads as
            // visibly off-center since the box (500px) is wider than the text.
            .set_align(glyphon::cosmic_text::Align::Center);
        label.resolve(screen_width, screen_height);
        self.ui.add_to_ui(Self::NO_CAMERA_MESSAGE_KEY.to_owned(), label);
        self.ui.has_changed = true;
    }

    // Rebuilds UI vertex/index/text buffers from this frame's UI - every frame
    // while there's any: this pass is also where hover, press, clicks,
    // scrolling and style transitions are worked out, so it has to run for
    // them to respond. With none, only when something marked it changed (the
    // last node removed, say) - then it's cleared once and skipped after.
    //
    // What's drawn, bottom to top: the engine's own UI (the "add a camera"
    // message), the active scene's `UiLayer`s by z index, the engine's
    // always_on_top UI (a dropdown's open list), then its
    // overlay (toasts). Something that blocks input - a layer with
    // `blocks_input`, or any active always_on_top node - keeps hover and
    // clicks from everything under it. Text is drawn after the boxes of its
    // pass, so there are two passes: everything from the lowest blocking
    // layer up goes in the second, so a modal's box covers the text under it.
    fn prepare_ui_content(&mut self) {
        let has_layers = self.scene_manager.active_scene.as_ref().is_some_and(|scene| !scene_layer_order(scene).is_empty());
        if !self.ui.has_changed && self.ui.renderizable_elements.is_empty() && !has_layers {
            self.ui.ui_rendering.hover_cursor = None;
            return;
        }

        // Two separate batches (see TextRendering::text_renderer_on_top's own doc
        // comment for why one shared Vec/one glyphon TextRenderer can't do this).
        let mut text_areas: Vec<TextArea> = Vec::new();
        let mut text_areas_on_top: Vec<TextArea> = Vec::new();

        self.ui.ui_rendering.clear();

        // Taken out up front: this function's TextAreas borrow from inside
        // the nodes for its whole remaining duration, so the map can't be
        // borrowed again for them partway through. Put back once text_areas
        // has been consumed.
        let mut on_top_nodes: Vec<(String, UiNode)> = Vec::new();
        for key in &self.ui.always_on_top {
            if let Some(node) = self.ui.renderizable_elements.remove(key) {
                on_top_nodes.push((key.clone(), node));
            }
        }
        let mut overlay_nodes: Vec<(String, UiNode)> = Vec::new();
        for key in &self.ui.overlay {
            if let Some(node) = self.ui.renderizable_elements.remove(key) {
                overlay_nodes.push((key.clone(), node));
            }
        }

        // UI units -> real backing pixels: the display's own (see
        // Window::pixel_size's own doc comment) times the UI's scale (see
        // UiScaling) - node_content_preparation needs this to place glyphon
        // TextAreas and draw every box. Worked out from the same rounded room
        // the layout uses, so the two always agree.
        let ui_size = self.ui.layout_size();
        let dpi_scale = self.window.pixel_size.width as f32 / ui_size.width as f32;
        let (room_width, room_height) = self.ui.size();

        // The scene's layers, bottom to top (the same order as
        // scene_layer_order), each laid out in the window's room - again
        // whenever that changes.
        let mut layers: Vec<(String, &mut UiLayer)> = match self.scene_manager.active_scene.as_mut() {
            Some(scene) => scene.content.nodes.iter_mut()
                .filter_map(|node| {
                    let id = node.id.clone();
                    node.get_property_mut::<UiLayer>().filter(|layer| layer.visible).map(|layer| (id, layer))
                })
                .collect(),
            None => Vec::new(),
        };
        layers.sort_by(|(a_id, a), (b_id, b)| a.z_index.cmp(&b.z_index).then_with(|| a_id.cmp(b_id)));
        for (_, layer) in &mut layers {
            layer.lay_out(room_width, room_height);
        }

        // Each thing drawn has an index, bottom to top: the engine's own UI,
        // then the layers, then always_on_top, then the overlay.
        let layers_start = self.ui.renderizable_elements.len();
        let on_top_start = layers_start + layers.len();
        // The topmost thing that blocks input - nothing under it takes any.
        let topmost_blocking = on_top_nodes.iter().rposition(|(_, node)| node.is_active).map(|index| on_top_start + index)
            .or_else(|| layers.iter().rposition(|(_, layer)| layer.blocks_input).map(|index| layers_start + index));
        let hit_testable = |index: usize| topmost_blocking.map_or(true, |topmost| index >= topmost);
        // Where the second pass starts - the lowest blocking layer, or the
        // engine's always_on_top UI.
        let second_pass_start = layers.iter().position(|(_, layer)| layer.blocks_input).map_or(on_top_start, |index| layers_start + index);

        // The engine's one font system, held for the whole pass (and handed
        // down) - text shaped in here can't take it again.
        let mut fonts = crate::rendering::ui::ui::font_system();
        let delta_time = self.time.unscaled_delta_time;

        // Debug bounds overlay (Ui::debug_bounds, no key) - has to run before
        // node_content_preparation, not after: that call's returned TextAreas
        // keep the node borrowed until text_areas is consumed. Reads the rect
        // from the start of this frame - an invisible tradeoff for a debug
        // overlay. A macro rather than a closure, since a closure capturing
        // &mut self.ui.ui_rendering would conflict with the borrows of the
        // nodes around it.
        let mut index = 0usize;
        let mut first_pass_closed = false;
        macro_rules! prepare_node {
            ($ui_node:expr) => {
                // Everything up to here is the first pass - see UiRendering::
                // main_index_count's own doc comment.
                if !first_pass_closed && index >= second_pass_start {
                    self.ui.ui_rendering.main_index_count = self.ui.ui_rendering.num_indices;
                    self.ui.ui_rendering.main_uses_blur = std::mem::take(&mut self.ui.ui_rendering.uses_blur);
                    first_pass_closed = true;
                }
                if self.ui.debug_bounds {
                    $ui_node.debug_bounds_preparation(&ui_size, dpi_scale, &mut self.ui.ui_rendering);
                }
                $ui_node.measure_fit();
                // None - a tree's root starts unclipped; a scrollable
                // container clips its own descendants.
                let (areas, _vertices, _indices) = $ui_node.node_content_preparation(&ui_size, dpi_scale, &mut self.ui.ui_rendering, &self.input, &mut fonts, &mut self.ui.text.text_cache, delta_time, hit_testable(index), None, None, false);
                if first_pass_closed { text_areas_on_top.extend(areas) } else { text_areas.extend(areas) }
                index += 1;
            };
        }

        for (_key, ui_node) in &mut self.ui.renderizable_elements {
            prepare_node!(ui_node);
        }
        for (_id, layer) in &mut layers {
            prepare_node!(layer.root);
        }
        for (_key, ui_node) in &mut on_top_nodes {
            prepare_node!(ui_node);
        }
        for (_key, ui_node) in &mut overlay_nodes {
            prepare_node!(ui_node);
        }
        if !first_pass_closed {
            self.ui.ui_rendering.main_index_count = self.ui.ui_rendering.num_indices;
            self.ui.ui_rendering.main_uses_blur = std::mem::take(&mut self.ui.ui_rendering.uses_blur);
        }

        // Only update buffers if we have data
        if !self.ui.ui_rendering.vertices.is_empty() {
            self.ui.ui_rendering.ensure_vertex_capacity(&self.renderer.device);
            self.renderer.queue.write_buffer(&self.ui.ui_rendering.vertex_buffer, 0, bytemuck::cast_slice(self.ui.ui_rendering.vertices.as_slice()));
        }
        if !self.ui.ui_rendering.indices.is_empty() {
            self.ui.ui_rendering.ensure_index_capacity(&self.renderer.device);
            self.renderer.queue.write_buffer(&self.ui.ui_rendering.index_buffer, 0, bytemuck::cast_slice(&self.ui.ui_rendering.indices));
        }

        // Always prepare, even with zero text areas - glyphon's prepare() is what
        // clears its *previous* batch, so skipping it with nothing to show (the
        // frame a modal closes) would keep drawing stale text.
        self.ui.text.text_renderer.prepare(&self.renderer.device, &self.renderer.queue, &mut fonts, &mut self.ui.text.text_atlas, &self.renderer.glyphon.viewport, text_areas, &mut self.ui.text.text_cache).unwrap();
        self.ui.text.text_renderer_on_top.prepare(&self.renderer.device, &self.renderer.queue, &mut fonts, &mut self.ui.text.text_atlas, &self.renderer.glyphon.viewport, text_areas_on_top, &mut self.ui.text.text_cache).unwrap();
        drop(fonts);

        // Puts the engine's always_on_top and overlay nodes back now that
        // text_areas (the last thing that held any borrow into them) is
        // fully consumed - see where they were taken out, above.
        for (key, node) in on_top_nodes {
            self.ui.renderizable_elements.insert(key, node);
        }
        for (key, node) in overlay_nodes {
            self.ui.renderizable_elements.insert(key, node);
        }

        // An image that turned up while the scene runs loads now. Not while
        // a scene loads - its images are on the loading thread.
        if !self.scene_manager.is_switching() {
            let paths = self.ui.ui_rendering.image_quads.keys().map(|(_, path)| path.as_str());
            self.assets.images.load_missing(&self.renderer.device, &self.renderer.queue, paths);
        }

        self.ui.build_image_draws(&self.renderer.device, &self.renderer.queue);

        self.ui.has_changed = false;
    }

    // The mouse cursor for this frame: a debug window's while the mouse is over
    // one, otherwise whatever the UI node under it asks for (UiNode::set_cursor),
    // otherwise the arrow. Left alone while the mouse is locked - it's hidden.
    fn apply_cursor(&mut self) {
        if self.window.is_cursor_locked() {
            return;
        }
        let icon = if self.debug_windows_shown() && self.debug_ui.ctx.is_pointer_over_area() {
            self.debug_ui.cursor
        } else {
            self.ui.ui_rendering.hover_cursor.unwrap_or_default()
        };
        self.window.set_cursor_icon(icon);
    }

    // Runs the `.on_click(...)` of the node clicked this frame (see
    // UiNode::clicked_paths, from the drawing pass above) - only the one drawn
    // on top, when a click lands on several (a dropdown's list over the row
    // under it). Then the `on_select` of a dropdown whose list was picked from.
    //
    // A handler in a scene layer runs like a behavior: its node is taken out
    // of the scene while it runs, and handed to it with the rest of the scene
    // (`SceneAccess`) and the app - what it spawns or despawns is done right
    // after. One in the engine's own UI only gets the app.
    fn fire_ui_click_handlers(&mut self) {
        // A node is only clicked the frame the left button goes down (see
        // UiNode::is_clicked) - every other frame there's nothing to look for.
        if !self.input.mouse_left_button_just_pressed() {
            return;
        }
        // A debug window sits on top of the game's own UI. A click on it must
        // not also hit the button underneath.
        if self.debug_windows_shown() && self.debug_ui.ctx.is_pointer_over_area() {
            return;
        }
        // A frozen scene (another one is being opened) doesn't react.
        if self.scene_manager.is_switching() {
            return;
        }

        // Everything clicked, in the order it's drawn (see prepare_ui_content).
        enum Clicked {
            Engine(String),
            Layer { node: String, path: String },
        }
        let mut clicked: Vec<Clicked> = Vec::new();
        let mut paths = Vec::new();
        let ui = &self.ui;
        let layered = |key: &String| ui.always_on_top.contains(key) || ui.overlay.contains(key);
        for (key, node) in ui.renderizable_elements.iter().filter(|(key, _)| !layered(key)) {
            node.clicked_paths(key, &mut paths);
            clicked.extend(paths.drain(..).map(Clicked::Engine));
        }
        if let Some(scene) = self.scene_manager.active_scene.as_ref() {
            for id in scene_layer_order(scene) {
                if let Some(layer) = scene.content.nodes.get(&id).and_then(|node| node.get_property::<UiLayer>()) {
                    layer.root.clicked_paths("", &mut paths);
                    clicked.extend(paths.drain(..).map(|path| Clicked::Layer { node: id.clone(), path }));
                }
            }
        }
        for key in ui.always_on_top.iter().chain(&ui.overlay) {
            if let Some(node) = ui.renderizable_elements.get(key) {
                node.clicked_paths(key, &mut paths);
                clicked.extend(paths.drain(..).map(Clicked::Engine));
            }
        }

        match clicked.pop() {
            Some(Clicked::Engine(path)) => {
                if let Some(handler) = Ui::get_ui_node(&mut self.ui.renderizable_elements, &path).and_then(|node| node.take_click_handler()) {
                    // A game's handler has no node to be given in the
                    // engine's own UI - only the engine's own run there.
                    let handler = match handler {
                        ClickHandler::Engine(mut on_click) => {
                            on_click(self);
                            ClickHandler::Engine(on_click)
                        }
                        game => game,
                    };
                    if let Some(node) = Ui::get_ui_node(&mut self.ui.renderizable_elements, &path) {
                        node.restore_click_handler(handler);
                    }
                }
            }
            Some(Clicked::Layer { node, path }) => {
                self.with_layer_node(&node, |node, scene, app| {
                    let Some(handler) = layer_node(node, &path).and_then(|ui_node| ui_node.take_click_handler()) else { return };
                    let handler = match handler {
                        ClickHandler::Engine(mut on_click) => {
                            on_click(app);
                            ClickHandler::Engine(on_click)
                        }
                        ClickHandler::Node(mut on_click) => {
                            on_click(node, scene, app);
                            ClickHandler::Node(on_click)
                        }
                    };
                    if let Some(ui_node) = layer_node(node, &path) {
                        ui_node.restore_click_handler(handler);
                    }
                });
            }
            None => {}
        }

        // An option picked from a dropdown's list (the engine's own UI, just
        // clicked) - its dropdown's on_select, from its layer.
        let mut picked = Vec::new();
        if let Some(scene) = self.scene_manager.active_scene.as_ref() {
            for id in scene_layer_order(scene) {
                let mut found = Vec::new();
                if let Some(layer) = scene.content.nodes.get(&id).and_then(|node| node.get_property::<UiLayer>()) {
                    layer.root.picked_dropdowns("", &mut found);
                }
                picked.extend(found.into_iter().map(|(_, index, on_select)| (id.clone(), index, on_select)));
            }
        }
        for (id, index, on_select) in picked {
            self.with_layer_node(&id, |node, scene, app| {
                // Taken out while it runs, so it can set a new one.
                let Some(mut on_select_fn) = on_select.borrow_mut().take() else { return };
                on_select_fn(node, scene, app, index);
                on_select.borrow_mut().get_or_insert(on_select_fn);
            });
        }
    }

    /// Runs `run` with the active scene's node `id` - taken out of the scene
    /// while it runs, like a behavior's node (see `SceneAccess`) - then does
    /// what it spawned and despawned.
    fn with_layer_node(&mut self, id: &str, run: impl FnOnce(&mut Node, &mut SceneAccess, &mut App)) {
        let Some(mut scene) = self.scene_manager.active_scene.take() else { return };
        let mut changes = SceneChanges::default();
        if let Some(mut node) = scene.content.nodes.remove(id) {
            run(&mut node, &mut SceneAccess::new(&mut scene.content.nodes, &mut scene.cameras, &mut changes), self);
            scene.content.nodes.put_back(node);
        }
        scene.apply_changes(self, changes);
        self.scene_manager.active_scene = Some(scene);
    }

    /// Shows the window and runs the game until it's closed (or
    /// `should_quit` is set). `scene_manager` is set up by the caller before
    /// this - registering a scene and opening it only take effect here.
    pub fn run(mut self) {
        let mut app_state = AppState { is_running: true };
        let mut event_pump = self.event_pump.take().expect("run consumes the App, so the event pump is still here");

        self.window.show();
        // Whatever was set on the window before run, the renderer follows.
        self.sync_window_size();

        loop {
            self.time.update();
            // true if the OS/window manager requested a close (SDL2's own
            // Event::Quit - clicking the window's close button, Cmd+Q, etc.)
            let quit_requested = self.input.update(&mut event_pump, self.time.unscaled_delta_time, false);
            if quit_requested {
                app_state.is_running = false;
            }
            self.sync_window_size();

            if !app_state.is_running || self.should_quit {
                self.physics.stop();
                break
            }

            if self.scene_manager.requested.is_some() {
                SceneManager::apply_open(&mut self);
            } else {
                SceneManager::update_loads(&mut self);

                // The UI bounds overlay (no key - set Ui::debug_bounds) -
                // rebuilt every frame while it's on, since it has to track
                // wherever nodes actually are right now.
                if self.ui.debug_bounds {
                    self.ui.has_changed = true;
                }

                // Tick the active scene - taken out of its slot first so there's no
                // conflicting borrow with the &mut self it needs, put back once done.
                // Its nodes tick right alongside it, in the same taken-out window:
                // the physics steps due (each after every fixed_update), then
                // every update.
                // While another scene is being opened, the active one is
                // frozen: still drawn, not updated.
                let ticked = if self.scene_manager.is_switching() { None } else { self.scene_manager.active_scene.take() };
                if let Some(mut scene) = ticked {
                    // The floating origin first, so the whole frame sees
                    // the world in the same place.
                    self.origin.begin_frame();
                    self.update_origin(&mut scene, true);
                    self.run_physics(&mut scene);

                    let mut ctx = FrameContext {
                        app_state: &mut app_state,
                        event_pump: &mut event_pump,
                    };
                    scene.run_update(&mut self, &mut ctx);

                    let mut changes = SceneChanges::default();
                    scene.content.nodes.update(&mut scene.cameras, &mut self, &mut changes);
                    // What the behaviors spawned and despawned, now they've all run.
                    scene.apply_changes(&mut self, changes);
                    // Teleports they asked for, before anything's drawn.
                    if self.origin.has_pending() {
                        self.update_origin(&mut scene, false);
                    }
                    // Every moving body's colliders, for the debug view - after
                    // the behaviors, which may have moved one.
                    self.physics.draw_colliders();

                    // The animation's turn, after the behaviors - then the
                    // hierarchy (children onto their parents, LookAt nodes
                    // turned), and from that every renderer instance and
                    // camera node's camera.
                    scene.run_late_update(&mut self, &mut ctx);
                    scene.content.nodes.resolve_hierarchy();
                    scene.sync_render_instances(&mut self);
                    scene.sync_cameras();

                    // Re-aims every look_at-configured camera at its current
                    // target - last, so it wins over whatever a Behavior/
                    // scene's own update already did to a camera's
                    // position/orientation this frame (see SceneCameras::
                    // update's own doc comment).
                    scene.cameras.update(&scene.content.renderizable_instances);
                    scene.run_after_cameras(&mut self);

                    self.scene_manager.active_scene = Some(scene);
                }

                // Sounds that got to an `at`, or played to their end, this frame.
                for callback in self.audio.take_due() {
                    callback(&mut self);
                }


                let camera_position = self.scene_manager.cameras().map(|c| c.active().camera.position().coords).unwrap_or_else(Vector3::zeros);
                self.write_instance_buffers(camera_position);
                self.update_lighting(camera_position);

                // No-op unless something called SceneCameras::transition_to(...) -
                // advances the blend (if any) before this frame's view_proj upload.
                if let Some(cameras) = self.scene_manager.cameras_mut() {
                    cameras.update_transition(self.time.delta_time);
                    self.renderer.camera_resources.update_buffer(&self.renderer.queue, cameras.active());
                }
                self.update_particles();
                // A game shader's file saved: built again (see ShaderLibrary::watch).
                // (The depth's near/far buffer isn't written here: it's made
                // with those values, and they never change.)
                self.renderer.shaders.watch();

                // TEMP: debug_text! messages have nowhere on-screen to render yet
                // - dump to stdout for now so debug_text! is actually visible somewhere.
                let messages = crate::tooling::debug_console::drain_messages();
                if crate::tooling::debug_console::is_console_visible() {
                    for message in messages {
                        println!("{message}");
                    }
                }

                self.run_debug_ui();
            }

            match self.render() {
                Ok(_) => {},
                Err(wgpu::SurfaceError::Outdated) => {
                    self.resize()
                }
                Err(wgpu::SurfaceError::Lost) => {
                    eprintln!("Device lost! You need to recreate the device and all resources.");
                    break;
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
    }

    /// The physics steps this frame's time calls for - each right after
    /// every node's `fixed_update` (and the scene's), what they spawned and
    /// despawned done in between - then every moving body's node placed
    /// where it's drawn this frame (see `PhysicsWorld::render_poses`).
    /// The floating origin's turn (see WorldOrigin): the teleports asked
    /// for, each moving the origin to where its node goes first - then, at
    /// the start of a frame (`check_anchor`), whether what the origin follows
    /// is far enough out to move the world back.
    fn update_origin(&mut self, scene: &mut Scene, check_anchor: bool) {
        for teleport in self.origin.take_pending() {
            if let Some(shift) = self.origin.shift_to(teleport.world) {
                self.shift_origin(scene, shift);
            }
            let position = self.origin.to_local(teleport.world);
            let Some(mut world) = scene.world_transform(&teleport.node) else {
                eprintln!("origin: no node '{}' to teleport", teleport.node);
                continue;
            };
            world.position = position;
            if let Some(rotation) = teleport.rotation {
                world.rotation = rotation;
            }
            scene.set_world_transform(&teleport.node, world);
            self.physics.teleport(&teleport.node, world.position, world.rotation);
        }
        if check_anchor {
            if let Some(shift) = Self::origin_anchor(scene).and_then(|anchor| self.origin.shift_for(anchor)) {
                self.shift_origin(scene, shift);
            }
        }
    }

    /// Where what the floating origin follows is: the node with an
    /// `OriginAnchor`, or the active camera.
    fn origin_anchor(scene: &Scene) -> Option<Vector3<f32>> {
        scene.content.nodes.iter()
            .find(|node| node.get_property::<OriginAnchor>().is_some())
            .and_then(|node| node.transform_3d())
            .map(|transform| transform.position)
            .or_else(|| scene.cameras.has_active_camera().then(|| scene.cameras.active().camera.position().coords))
    }

    /// Moves the whole world by `shift` - the scene's nodes and cameras, the
    /// physics, particles and world-space line and text sets - notes it on
    /// `origin`, then tells the behaviors (`on_origin_shift`).
    fn shift_origin(&mut self, scene: &mut Scene, shift: Vector3<f32>) {
        scene.shift_origin(shift);
        self.physics.shift_origin(shift);
        self.particles.shift_origin(shift);
        self.renderer.render_sets.shift_origin(&self.renderer.device, shift);
        self.origin.record_shift(shift);
        scene.run_origin_shift(self, shift);
    }

    fn run_physics(&mut self, scene: &mut Scene) {
        let steps = self.physics.steps_due(self.time.delta_time, self.renderer.debug.physics, self.renderer.debug.sensors);
        for _ in 0..steps {
            self.input.begin_fixed_step();
            let mut changes = SceneChanges::default();
            scene.content.nodes.fixed_update(&mut scene.cameras, self, &mut changes);
            scene.run_fixed_update(self);
            scene.apply_changes(self, changes);
            self.input.end_fixed_step();
            // Teleports the fixed updates asked for, before the step moves anything.
            if self.origin.has_pending() {
                self.update_origin(scene, false);
            }
            // Sensors without a body of their own go where their node is.
            let followed: Vec<(String, crate::scene_manager::node::properties::Transform3D)> = self.physics.followers()
                .filter_map(|id| Some((id.to_owned(), scene.world_transform(id)?)))
                .collect();
            for (id, world) in followed {
                self.physics.follow(&id, world.position, world.rotation);
            }
            self.physics.step();
            // What the step's sensors detected, to their behaviors.
            let events = self.physics.sensor_events().to_vec();
            if !events.is_empty() {
                let mut changes = SceneChanges::default();
                scene.content.nodes.sensor_events(&events, &mut scene.cameras, self, &mut changes);
                scene.apply_changes(self, changes);
            }
        }

        // Not onto a node an animation is moving (SceneContent::kinematic) -
        // its body is held still meanwhile.
        for (id, pose) in self.physics.render_poses() {
            if scene.content.kinematic.contains(id) {
                continue;
            }
            let (position, rotation) = (pose.translation.vector, pose.rotation);
            if let Some(renderizable) = scene.content.renderizable_instances.get_mut(id) {
                renderizable.transform.position = position;
                renderizable.transform.rotation = rotation;
            }
            if let Some(transform) = scene.content.nodes.get_mut(id).and_then(|node| node.transform_3d_mut()) {
                transform.position = position;
                transform.rotation = rotation;
            }
        }
    }

    /// Every model's instance buffer from its nodes' poses - grouped by
    /// model, each model's sorted by material, so every material draws one
    /// run of them (ModelDataInstance::material_ranges). Within a material,
    /// the shadow casters that held still since the last frame come first,
    /// then the moving ones, then the ones that don't cast - the runs the
    /// shadow pass draws (ModelDataInstance::shadow_runs; see
    /// render_pipeline::shadow_renderer for why the still ones are kept
    /// apart).
    fn write_instance_buffers(&mut self, camera_position: Vector3<f32>) {
        // One instance: its material, what it is to the shadows (0 a still
        // caster, 1 a moving one, 2 none), how it's drawn, where it is
        // (camera-relative) and its biggest scale.
        struct Placed<'a> {
            material: &'a str,
            shadows: u8,
            raw: InstanceRaw,
            position: Vector3<f32>,
            size: f32,
        }
        let mut model_instances: HashMap<&str, Vec<Placed>> = HashMap::new();
        let last_poses = std::mem::take(&mut self.renderer.shadows.poses);
        let mut poses = HashMap::new();
        // Which still casters there are, where, drawn with what - summed, so
        // it doesn't matter which order they come in.
        let mut still_casters = 0u64;
        if let Some(content) = self.scene_manager.content() {
            for (id, renderizable) in content.renderizable_instances.iter() {
                let transform = &renderizable.transform;
                let rotation = transform.rotation.coords;
                let pose = [
                    transform.position.x, transform.position.y, transform.position.z,
                    rotation.x, rotation.y, rotation.z, rotation.w,
                    transform.scale.x, transform.scale.y, transform.scale.z,
                ];
                let still = last_poses.get(id) == Some(&pose);
                let shadows = match (renderizable.casts_shadows, still) {
                    (false, _) => 2,
                    (true, true) => 0,
                    (true, false) => 1,
                };
                if shadows == 0 {
                    let mut hasher = DefaultHasher::new();
                    (id, &renderizable.model_ref, &renderizable.material).hash(&mut hasher);
                    hash_floats(&pose, &mut hasher);
                    still_casters = still_casters.wrapping_add(hasher.finish());
                }
                poses.insert(id.clone(), pose);
                let mut raw = InstanceRaw::from_transform(transform, camera_position);
                raw.receives_shadows = if renderizable.receives_shadows { 1.0 } else { 0.0 };
                model_instances.entry(renderizable.model_ref.as_str()).or_default().push(Placed {
                    material: renderizable.material.as_str(),
                    shadows,
                    raw,
                    position: transform.position - camera_position,
                    size: transform.scale.abs().max(),
                });
            }
        }
        self.renderer.shadows.poses = poses;

        for (model_ref, model) in self.assets.models.drawn.iter_mut() {
            model.material_ranges.clear();
            model.shadow_runs.clear();
            let Some(instances) = model_instances.get_mut(model_ref.as_str()) else { continue };
            instances.sort_by(|a, b| a.material.cmp(b.material).then(a.shadows.cmp(&b.shadows)));
            let mut start = 0;
            for index in 1..=instances.len() {
                if index == instances.len() || instances[index].material != instances[start].material {
                    model.material_ranges.push((instances[start].material.to_owned(), start as u32..index as u32));
                    start = index;
                }
            }
            // The shadow runs, each in the sphere around its instances.
            let radius = model.model.radius();
            let mut start = 0;
            for index in 1..=instances.len() {
                let first = &instances[start];
                if index < instances.len() && instances[index].material == first.material && instances[index].shadows == first.shadows {
                    continue;
                }
                if first.shadows < 2 {
                    let run = &instances[start..index];
                    let low = run.iter().fold(first.position, |low, placed| low.inf(&placed.position));
                    let high = run.iter().fold(first.position, |high, placed| high.sup(&placed.position));
                    let size = run.iter().fold(0.0f32, |size, placed| size.max(placed.size));
                    model.shadow_runs.push(ShadowRun {
                        material: first.material.to_owned(),
                        instances: start as u32..index as u32,
                        still: first.shadows == 0,
                        center: (low + high) / 2.0,
                        radius: (high - low).norm() / 2.0 + radius * size,
                    });
                }
                start = index;
            }
            // A still caster's meshes can still move on it (animation) - its
            // shadow with them.
            if model.shadow_runs.iter().any(|run| run.still) {
                let mut hasher = DefaultHasher::new();
                model_ref.hash(&mut hasher);
                for mesh in model.model.mesh_lists.values().flat_map(|meshes| meshes.values()) {
                    mesh.visible.hash(&mut hasher);
                    for transform in std::iter::once(&mesh.transform).chain(mesh.parent_transform.as_ref()) {
                        hash_floats(transform.position.as_slice(), &mut hasher);
                        hash_floats(transform.rotation.coords.as_slice(), &mut hasher);
                        hash_floats(transform.scale.as_slice(), &mut hasher);
                    }
                }
                still_casters = still_casters.wrapping_add(hasher.finish());
            }
            let raws: Vec<InstanceRaw> = instances.iter().map(|placed| placed.raw).collect();
            if !raws.is_empty() {
                self.renderer.queue.write_buffer(&model.instance_buffer, 0, bytemuck::cast_slice(&raws));
            }
        }
        self.renderer.shadows.still_casters = still_casters;
    }

    /// The game's debug windows (see DebugUi): its toggle key, the mouse
    /// freed while they're up, and - while they are - the active scene's
    /// `debug_ui` run to build them - drawn by `render`.
    fn run_debug_ui(&mut self) {
        // The debug windows free the mouse to click them.
        let shown = self.debug_windows_shown();
        if let Some(locked) = self.debug_ui.cursor_lock_change(shown, self.window.is_cursor_locked()) {
            self.window.set_cursor_locked(locked);
        }
        if !self.debug_windows_shown() {
            return;
        }

        let raw_input = self.debug_ui.raw_input(&self.input, [self.window.size.width, self.window.size.height]);
        // egui lays out in points; the window's pixels can be more of them
        // (a HiDPI/Retina display).
        let pixels_per_point = self.window.pixel_size.width as f32 / self.window.size.width.max(1) as f32;
        // A clone (it's shared inside) - the closure below needs &mut self.
        let ctx = self.debug_ui.ctx.clone();
        ctx.set_pixels_per_point(pixels_per_point);
        let mut scene = self.scene_manager.active_scene.take();
        let output = ctx.run(raw_input, |ctx| {
            if let Some(scene) = scene.as_mut() {
                scene.run_debug_ui(self, ctx);
            }
        });
        if let Some(scene) = scene {
            self.scene_manager.active_scene = Some(scene);
        }
        self.debug_ui.set_frame(output);
    }

    /// Whether the game's debug windows (`debug_ui.visible`) are drawn this frame.
    fn debug_windows_shown(&self) -> bool {
        self.debug_ui.visible
    }
}

/// The ids of the scene's visible `UiLayer` nodes, bottom to top: by z
/// index, then by id - the order they're drawn and clicked in.
fn scene_layer_order(scene: &Scene) -> Vec<String> {
    let mut layers: Vec<(i32, String)> = scene.content.nodes.iter()
        .filter_map(|node| node.get_property::<UiLayer>().filter(|layer| layer.visible).map(|layer| (layer.z_index, node.id.clone())))
        .collect();
    layers.sort();
    layers.into_iter().map(|(_, id)| id).collect()
}

/// The node at `path` in `node`'s `UiLayer`.
fn layer_node<'a>(node: &'a mut Node, path: &str) -> Option<&'a mut UiNode> {
    node.get_property_mut::<UiLayer>()?.get_mut(path)
}
