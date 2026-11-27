//! The sun's shadows: a field of pillars, boxes and balls - some of them
//! moving - to look at shadows up close and far away, and to compare the
//! shadow settings (`Shadows`) side by side.
//!
//! The shadow maps (cascades) are read within circles around the camera,
//! the nearest the sharpest - T tints every surface by the one it reads
//! (red, green, blue, yellow, cyan, magenta, nearest first) to see them. G cycles the
//! presets below, M shows the maps themselves, F turns shadows off.
//!
//! WASD to move (Space up, Left Ctrl down, Left Shift faster), the mouse to
//! look around; Escape frees the mouse, clicking takes it back.
//!
//! B runs the benchmark: the camera flies a fixed path while every preset
//! is timed in turn, and the results are printed. With SHADOW_BENCH set
//! (`SHADOW_BENCH=1 cargo run --example shadows`) it starts on its own and
//! quits when it's done. SHADOW_BENCH=0,4,7 times just those presets.
//! Frame times only show what a preset costs while the GPU is what limits
//! them: where the display caps the frame rate (a 120 Hz Mac screen, even
//! with vsync off), every preset fast enough reads the same.

use nalgebra::{UnitQuaternion, Vector3};
use pankarta_software::prelude::*;
use pankarta_software::ui::ui_node::UiNode;
use pankarta_software::ui::ui_transform::SizeValue;
use pankarta_software::ui::widgets::text;
use pankarta_software::ui::TextStyle;

const GROUND_TEXTURE: &str = "textures/grid/Dark/texture_01.png";
const TEXTURES: [&str; 4] = [
    "textures/grid/Orange/texture_01.png",
    "textures/grid/Green/texture_01.png",
    "textures/grid/Purple/texture_01.png",
    "textures/grid/Red/texture_01.png",
];

/// A named set of shadow settings.
struct Preset {
    name: &'static str,
    shadows: Shadows,
}

fn presets() -> Vec<Preset> {
    let base = Shadows::default();
    // Each map 3 times the one before - the nearest distance / 3^(maps - 1).
    let preset = |name, distance: f32, cascades: u32, resolution, softness| Preset {
        name,
        shadows: Shadows { distance, cascades, near: distance / 3.0_f32.powi(cascades as i32 - 1), resolution, softness, ..base },
    };
    // A long reach with a small sharp map up close - a cockpit's shadows and
    // a landscape's: the maps between spread evenly (Shadows::near).
    let far = |name, distance, cascades, near, resolution, softness| Preset {
        name,
        shadows: Shadows { distance, cascades, near, resolution, softness, ..base },
    };
    vec![
        Preset { name: "default", shadows: base },
        Preset { name: "off", shadows: Shadows { enabled: false, ..base } },
        preset("40 m, 4 maps, 2048, soft 2", 40.0, 4, 2048, 2.0),
        preset("40 m, 4 maps, 1024, soft 2", 40.0, 4, 1024, 2.0),
        preset("40 m, 4 maps, 1024, soft 1", 40.0, 4, 1024, 1.0),
        preset("40 m, 4 maps, 1536, soft 1", 40.0, 4, 1536, 1.0),
        preset("40 m, 4 maps, 2048, soft 1", 40.0, 4, 2048, 1.0),
        preset("40 m, 3 maps, 1024, soft 1", 40.0, 3, 1024, 1.0),
        preset("40 m, 3 maps, 1536, soft 1", 40.0, 3, 1536, 1.0),
        preset("40 m, 3 maps, 2048, soft 1", 40.0, 3, 2048, 1.0),
        preset("40 m, 2 maps, 2048, soft 1", 40.0, 2, 2048, 1.0),
        preset("40 m, 4 maps, 4096, soft 2", 40.0, 4, 4096, 2.0),
        preset("120 m, 4 maps, 1024, soft 1", 120.0, 4, 1024, 1.0),
        preset("120 m, 4 maps, 1536, soft 1", 120.0, 4, 1536, 1.0),
        // Skies' flight today: 4 km in 4 maps - the nearest reaches 148 m.
        preset("4 km, 4 maps, 1024, soft 2", 4000.0, 4, 1024, 2.0),
        far("4 km, 6 maps from 2 m, 1024, soft 1", 4000.0, 6, 2.0, 1024, 1.0),
        far("4 km, 6 maps from 2 m, 1536, soft 1", 4000.0, 6, 2.0, 1536, 1.0),
        far("4 km, 6 maps from 5 m, 1024, soft 1", 4000.0, 6, 5.0, 1024, 1.0),
    ]
}

/// Radians the camera turns for one count of mouse movement.
const RADIANS_PER_COUNT: f32 = 0.0004;

/// Seconds each preset runs before it's timed, and is timed for.
const WARM_UP: f32 = 1.5;
const MEASURE: f32 = 5.0;

/// Flies the camera: by hand (mouse and WASD), or along the benchmark's
/// path while one runs.
struct FlyCamera {
    yaw: f32,
    pitch: f32,
    speed: f32,
}

impl Behavior for FlyCamera {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        if app.input.is_just_pressed(InputSource::key(Key::Escape)) {
            app.window.set_cursor_locked(false);
        } else if app.input.mouse_left_button_just_pressed() {
            app.window.set_cursor_locked(true);
        }
        if app.window.is_cursor_locked() {
            let look = app.input.mouse_look() * RADIANS_PER_COUNT;
            self.yaw -= look.x;
            self.pitch = (self.pitch - look.y).clamp(-1.5, 1.5);
        }
        let rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.yaw) * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), self.pitch);
        let forward = rotation * -Vector3::z();
        let right = rotation * Vector3::x();
        let walk = app.input.get_vector("left", "right", "forward", "back").normalize();
        let climb = app.input.get_axis("down", "up");
        let speed = if app.input.is_pressed("fast") { self.speed * 4.0 } else { self.speed };
        let movement = (forward * -walk.0.y + right * walk.0.x + Vector3::y() * climb) * speed * app.time.unscaled_delta_time;
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            transform.position += movement;
            transform.rotation = rotation;
        }
    }
}

/// Turns a node about Y, or carries it around a circle.
struct Spin {
    /// Radians a second.
    rate: f32,
    /// Some((centre, radius)): goes around it, else turns where it is.
    orbit: Option<(Vector3<f32>, f32)>,
    angle: f32,
}

impl Behavior for Spin {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        self.angle += self.rate * app.time.delta_time;
        let Some(transform) = node.get_property_mut::<Transform3D>() else { return };
        transform.rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.angle);
        if let Some((centre, radius)) = self.orbit {
            transform.position = centre + Vector3::new(self.angle.cos() * radius, 0.0, self.angle.sin() * radius);
        }
    }
}

/// A benchmark run: the presets still to time (every one twice - in
/// order, then back - so a slow start or a hot GPU later doesn't favor
/// either end), how long the current one's been on, and the frame times
/// measured so far.
struct Bench {
    queue: Vec<usize>,
    preset: usize,
    elapsed: f32,
    frames: Vec<f32>,
    results: Vec<(usize, f32, f32)>,
    /// Quits once it's done.
    quit: bool,
}

impl Bench {
    fn new(presets: usize, quit: bool) -> Self {
        // SHADOW_BENCH=0,1,4 times just those presets.
        let only: Option<Vec<usize>> = std::env::var("SHADOW_BENCH").ok().filter(|value| value.contains(',')).map(|value| value.split(',').filter_map(|index| index.trim().parse().ok()).collect());
        let chosen: Vec<usize> = (0..presets).filter(|index| only.as_ref().is_none_or(|only| only.contains(index))).collect();
        let queue: Vec<usize> = chosen.iter().copied().chain(chosen.iter().rev().copied()).collect();
        Self { preset: queue[0], queue, elapsed: 0.0, frames: Vec::new(), results: Vec::new(), quit }
    }
}

struct ShadowsScene {
    presets: Vec<Preset>,
    preset: usize,
    bench: Option<Bench>,
}

impl ShadowsScene {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::ProceduralSky { sun: true });

        let mut nodes = vec![
            Node::new("ground")
                .with_property(Transform3D::default())
                .with_property(Mesh::plane(0)
                    .transform(Transform3D { scale: Vector3::new(400.0, 1.0, 400.0), ..Default::default() })
                    .material(Material::standard().texture(GROUND_TEXTURE).tiled(4.0, TextureSpace::World))),
            Node::new("camera")
                .with_property(Transform3D { position: Vector3::new(0.0, 3.0, 12.0), ..Default::default() })
                .with_property(CameraLens::new(70.0))
                .with_behavior(FlyCamera { yaw: 0.0, pitch: -0.15, speed: 6.0 }),
            // Low in the sky, for long shadows.
            Node::new("sun")
                .with_property(Transform3D::facing(Vector3::new(-0.5, -0.6, -0.35)))
                .with_property(Light::directional([1.0, 0.97, 0.9])),
        ];

        // A field of pillars and boxes out to ~140 m - something casting
        // in every cascade.
        let mut index = 0;
        for x in -14i32..=14 {
            for z in -14..=14 {
                if (x * x + z * z) < 4 || (x + z) % 2 != 0 {
                    continue;
                }
                index += 1;
                let position = Vector3::new(x as f32 * 10.0, 0.0, z as f32 * 10.0);
                let height = 1.5 + ((x * 7 + z * 13).rem_euclid(5)) as f32 * 1.5;
                let material = Material::standard().texture(TEXTURES[index % 4]).tiled(1.0, TextureSpace::Object);
                let mesh = if index % 3 == 0 {
                    Mesh::cylinder(0.6, height).transform(Transform3D { position: Vector3::new(0.0, height / 2.0, 0.0), ..Default::default() })
                } else {
                    Mesh::cube().transform(Transform3D { position: Vector3::new(0.0, height / 2.0, 0.0), scale: Vector3::new(2.0, height, 2.0), ..Default::default() })
                };
                nodes.push(Node::new(format!("block_{index}"))
                    .with_property(Transform3D { position, rotation: UnitQuaternion::from_axis_angle(&Vector3::y_axis(), index as f32 * 0.7), ..Default::default() })
                    .with_property(mesh.material(material)));
            }
        }
        // Up close: a fence of thin posts and a ball on a stand - fine
        // detail for the nearest map.
        for post in 0..12 {
            nodes.push(Node::new(format!("post_{post}"))
                .with_property(Transform3D { position: Vector3::new(-6.0 + post as f32, 0.0, 4.0), ..Default::default() })
                .with_property(Mesh::cube().transform(Transform3D { position: Vector3::new(0.0, 0.6, 0.0), scale: Vector3::new(0.08, 1.2, 0.08), ..Default::default() })));
        }
        nodes.push(Node::new("ball")
            .with_property(Transform3D { position: Vector3::new(3.0, 1.6, 6.0), ..Default::default() })
            .with_property(Mesh::sphere(0.6).material(Material::standard().texture(TEXTURES[2]).tiled(0.5, TextureSpace::Object))));

        // What moves - redrawn into the maps every frame: boxes going
        // around the camera's start, and turning bars.
        for mover in 0..8 {
            let radius = 6.0 + mover as f32 * 4.0;
            nodes.push(Node::new(format!("mover_{mover}"))
                .with_property(Transform3D::default())
                .with_property(Mesh::cube().transform(Transform3D { position: Vector3::new(0.0, 1.0, 0.0), scale: Vector3::new(1.2, 2.0, 1.2), ..Default::default() })
                    .material(Material::standard().texture(TEXTURES[mover % 4]).tiled(1.0, TextureSpace::Object)))
                .with_behavior(Spin { rate: 0.4 - mover as f32 * 0.03, orbit: Some((Vector3::zeros(), radius)), angle: mover as f32 }));
        }
        for bar in 0..4 {
            let position = Vector3::new(if bar % 2 == 0 { -12.0 } else { 12.0 }, 2.5, if bar < 2 { -8.0 } else { 18.0 });
            nodes.push(Node::new(format!("bar_{bar}"))
                .with_property(Transform3D { position, ..Default::default() })
                .with_property(Mesh::cube().transform(Transform3D { scale: Vector3::new(6.0, 0.3, 0.3), ..Default::default() }))
                .with_behavior(Spin { rate: 0.8, orbit: None, angle: bar as f32 }));
        }
        for node in nodes {
            if let Err(error) = scene.spawn_node(app, node) {
                eprintln!("{error}");
            }
        }

        let hud = Node::new("hud").with_property(UiLayer::new(UiNode::container().set_child("info",
            text("", &TextStyle::default()).set_padding((10.0, 10.0)).set_size(SizeValue::Pixels(900.0), SizeValue::Fit))));
        if let Err(error) = scene.spawn_node(app, hud) {
            eprintln!("{error}");
        }

        let presets = presets();
        app.renderer.set_shadows(presets[0].shadows);
        let bench = std::env::var("SHADOW_BENCH").is_ok().then(|| Bench::new(presets.len(), true));
        app.window.set_cursor_locked(bench.is_none());
        Self { presets, preset: 0, bench }
    }

    fn select(&mut self, preset: usize, app: &mut App) {
        self.preset = preset;
        app.renderer.set_shadows(self.presets[preset].shadows);
    }

    /// One benchmark frame: the camera on its path, this frame's time
    /// noted, and on to the next preset once it's had its time - printing
    /// the results after the last.
    fn run_bench(&mut self, scene: &mut Scene, app: &mut App) {
        let Some(bench) = &mut self.bench else { return };
        if bench.elapsed == 0.0 {
            app.renderer.set_shadows(self.presets[bench.preset].shadows);
            self.preset = bench.preset;
        }
        let dt = app.time.unscaled_delta_time;
        bench.elapsed += dt;
        if bench.elapsed > WARM_UP {
            bench.frames.push(dt * 1000.0);
        }
        // The same path for every preset: around the field at head height,
        // looking a little down and out - the time since it started.
        let angle = bench.elapsed * 0.25;
        if let Some(camera) = scene.get_node_mut("camera").and_then(|node| node.get_property_mut::<Transform3D>()) {
            camera.position = Vector3::new(angle.cos() * 25.0, 4.0, angle.sin() * 25.0);
            camera.rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -angle) * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), -0.2);
        }
        if bench.elapsed < WARM_UP + MEASURE {
            return;
        }
        let mut frames = std::mem::take(&mut bench.frames);
        frames.sort_by(f32::total_cmp);
        let average = frames.iter().sum::<f32>() / frames.len().max(1) as f32;
        let p95 = frames.get((frames.len() as f32 * 0.95) as usize).copied().unwrap_or(0.0);
        bench.results.push((bench.preset, average, p95));
        bench.elapsed = 0.0;
        bench.queue.remove(0);
        if let Some(next) = bench.queue.first() {
            bench.preset = *next;
            return;
        }
        println!("\nshadow benchmark ({} s a preset, twice, after {} s to settle) - frame time in ms", MEASURE, WARM_UP);
        println!("{:<38} {:>7} {:>7} {:>7} {:>7} {:>6}", "preset", "run 1", "run 2", "avg", "p95", "fps");
        for preset in 0..self.presets.len() {
            let runs: Vec<&(usize, f32, f32)> = bench.results.iter().filter(|run| run.0 == preset).collect();
            if runs.is_empty() {
                continue;
            }
            let average = runs.iter().map(|run| run.1).sum::<f32>() / runs.len() as f32;
            let p95 = runs.iter().map(|run| run.2).sum::<f32>() / runs.len() as f32;
            println!("{:<38} {:>7.2} {:>7.2} {:>7.2} {:>7.2} {:>6.0}", self.presets[preset].name, runs[0].1, runs[1].1, average, p95, 1000.0 / average);
        }
        let quit = bench.quit;
        self.bench = None;
        self.select(0, app);
        if quit {
            app.should_quit = true;
        }
    }
}

impl SceneBehaviour for ShadowsScene {
    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        if self.bench.is_some() {
            self.run_bench(scene, app);
        } else {
            if app.input.is_just_pressed(InputSource::key(Key::G)) {
                self.select((self.preset + 1) % self.presets.len(), app);
            }
            if app.input.is_just_pressed(InputSource::key(Key::B)) {
                self.bench = Some(Bench::new(self.presets.len(), false));
            }
        }
        if app.input.is_just_pressed(InputSource::key(Key::T)) {
            app.renderer.debug.shadow_cascades = !app.renderer.debug.shadow_cascades;
        }
        if app.input.is_just_pressed(InputSource::key(Key::M)) {
            app.renderer.debug.shadows = !app.renderer.debug.shadows;
        }
        if app.input.is_just_pressed(InputSource::key(Key::F)) {
            let mut shadows = app.renderer.shadows();
            shadows.enabled = !shadows.enabled;
            app.renderer.set_shadows(shadows);
        }

        let shadows = app.renderer.shadows();
        let running = match &self.bench {
            Some(bench) => format!("benchmark: {} presets to go", bench.queue.len()),
            None => "B: benchmark every preset".to_owned(),
        };
        let info = format!(
            "{:.0} fps ({:.2} ms)\nG: preset - {} ({}, {} maps, {} px, softness {})\nT: cascade tint   M: shadow maps   F: shadows {}\n{running}",
            app.time.fps(), app.time.unscaled_delta_time * 1000.0,
            self.presets[self.preset].name, shadows.distance, shadows.cascades, shadows.resolution, shadows.softness,
            if shadows.enabled { "on" } else { "off" },
        );
        if let Some(label) = scene.get_node_mut("hud").and_then(|node| node.get_property_mut::<UiLayer>()).and_then(|ui| ui.label_mut("info")) {
            label.set_text(info);
        }
    }
}

#[tokio::main]
async fn main() {
    let mut app = match App::new().await {
        Ok(app) => app,
        Err(err) => {
            eprintln!("Couldn't create the app: {err}");
            return;
        }
    };
    app.window.set_title("Shadows");
    // Uncapped, so the presets' costs show in the frame times.
    app.renderer.set_vsync(false);
    for (action, key) in [("forward", Key::W), ("back", Key::S), ("left", Key::A), ("right", Key::D), ("up", Key::Space), ("down", Key::LCtrl), ("fast", Key::LShift)] {
        app.input.bind(action, InputSource::key(key));
    }
    app.scene_manager.create_scene("main", ShadowsScene::new);
    app.scene_manager.open_scene("main");
    app.run();
}
