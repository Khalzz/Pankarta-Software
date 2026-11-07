# Scene loading: what changed

Splash screens and loading screens are now **ordinary scenes that you write yourself**. The engine has no built-in splash or loading screen any more. Instead it gives you one ability: **preparing a scene in the background while another scene keeps running**, and then opening it.

The full example is `examples/scenes/loading.rs`. Run it with `cargo run --example scene_loading`.

---

## 1. The new API (`app.scene_manager`)

| Call | What it does |
|---|---|
| `create_scene(name, factory)` | Unchanged. Registers a scene under a name. Nothing is built yet. |
| `open_scene(name)` | Makes `name` the active scene, at the start of the next frame. |
| `open(factory)` | **New.** Opens a scene straight from a closure, without registering it. This is how you pass parameters to a scene. |
| `preload(name)` | **New.** Builds `name` and loads its assets in the background. The current scene keeps running. Calling it again while the scene waits does nothing. |
| `is_ready(name)` | **New.** `true` once a preloaded scene has everything loaded. |
| `load_progress(name)` | **New.** `0.0` → `1.0`. Returns `1.0` when it's ready, or once it's the active scene. |
| `cancel_preload(name)` | **New.** Drops a preloaded scene. |

### What `open_scene` does in each case

- **Preloaded and ready:** it switches instantly. Nothing is built or loaded again.
- **Preloaded but still loading**, or **not preloaded at all:** the current scene **stays on screen, frozen** (still drawn, but not updated, no physics, no clicks) until the new scene is loaded. Then it switches.
- **The very first scene** (nothing is open yet): the window stays black until it's ready.

There is only ever **one active scene**. A preloaded scene just waits: it isn't drawn or updated, its behaviours don't run, and it has nothing on the GPU or in physics until it opens.

---

## 2. How to use it

### A splash screen

```rust
struct Splash { shown_for: f32 }

impl SceneBehaviour for Splash {
    fn on_spawn(&mut self, _scene: &mut Scene, app: &mut App) {
        app.scene_manager.preload("menu");          // the menu loads while the logo shows
    }

    fn update(&mut self, _scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        self.shown_for += app.time.unscaled_delta_time;
        if self.shown_for >= 2.0 && app.scene_manager.is_ready("menu") {
            app.scene_manager.open_scene("menu");   // instant
        }
    }
}

app.scene_manager.create_scene("splash", Splash::new);
app.scene_manager.open_scene("splash");
```

### A loading screen that receives the level to load

```rust
fn go_to(app: &mut App, level: &'static str) {
    app.scene_manager.open(move |scene, app| Loading::new(scene, app, level));
}

struct Loading { target: &'static str, shown_for: f32 }

impl Loading {
    fn new(scene: &mut Scene, app: &mut App, target: &'static str) -> Self {
        app.scene_manager.preload(target);
        // spawn its UI node: title, progress bar, animation...
        Self { target, shown_for: 0.0 }
    }
}

impl SceneBehaviour for Loading {
    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        self.shown_for += app.time.unscaled_delta_time;
        let progress = app.scene_manager.load_progress(self.target);
        // ...update the bar with `progress`...
        if app.scene_manager.is_ready(self.target) && self.shown_for >= 1.5 {
            app.scene_manager.open_scene(self.target);   // goes on by itself
        }
    }
}

// anywhere, e.g. a menu button:
go_to(app, "hangar");
```

You only register the levels. The loading scene is opened with `open(..)` each time, so it needs no registration.

### No loading screen

Just call `open_scene("level")`. The current scene freezes until the level is ready.

---

## 3. What was removed

| Removed | Replaced by |
|---|---|
| `set_splash_scene(duration, factory)`, `Splash`, `SplashRunning`, `begin_splash`, `advance_splash` | A normal scene (see the splash above) |
| `LoadingScreenScene` and the whole `loading_screen.rs` | A normal scene (see the loading screen above) |
| The fade in `advance_load`, which used string keys (`loading_backdrop` / `loading_panel` / `loading_step`) | Nothing. A loading scene animates its own UI. |
| `app.loading_step` | `load_progress(name)` |
| `SceneManager::reset` | An internal "requested open" |

---

## 4. Where the code is now

```
src/scene_manager/
├── manager.rs        SceneManager: create_scene, open_scene, open, preload,
│                     is_ready, load_progress, cancel_preload
└── loading/
    ├── assets.rs     what a scene needs loaded, and what the thread hands back
    ├── settings.rs   saves and restores the engine-wide settings around a build
    ├── thread.rs     a waiting scene (PendingSceneLoad), the loading thread,
    │                 and progress counting
    └── switch.rs     building ahead (prepare), opening (apply_open),
                      the per-frame check (update_loads), and the switch itself
```

The old `scene_switch.rs`, `loading.rs` and `loading_screen.rs` are gone. Their code lives in `loading/`.

---

## 5. Behaviour changes worth knowing

### Node `on_spawn` now runs when the scene starts, not while it's built

Before, a node's behaviours ran `on_spawn` *while the factory was building the scene*: before its model was on the GPU, and before physics. Now, for nodes spawned in the factory, `on_spawn` runs **when the scene opens**, after its models and bodies are set up. Nodes spawned while a scene is already running work as before: their `on_spawn` runs right away.

Why it had to change: with `preload`, a scene is built while *another* scene is active. Its behaviours must not run then.

The order when a scene opens is:
1. Models go on the GPU and bodies are created.
2. Every node's `on_spawn` runs.
3. The scene behaviour's `on_spawn` runs.
4. Physics starts.

One side effect: a **model** that a behaviour adds to its node *inside `on_spawn`* is no longer loaded in the background with the rest. It loads on the spot when the scene opens, which can cause a small hitch. Bodies and cameras added in `on_spawn` are fine; the walking player still works. To avoid the hitch, put the `Model` on the node directly, or name it with `scene.will_use(..)`.

### Settings a factory changes wait until the scene opens

A factory may call `app.renderer.set_environment(..)`, `set_ambient_light`, `set_shadows`, `app.physics.set_gravity` / `set_timestep`, or `app.particles.set_wind` / `set_sea_level`. Since the scene can now be built while another one is running, the engine:
- notes what the factory changed,
- puts the running scene's values back right away,
- applies only the changed ones when the new scene opens.

A setting the factory didn't touch stays as the game has it at that moment.

The **frame plan** and **line/text sets** (`app.renderer.frame_plan`, `add_line_set`, `add_text_set`) work the same way. The factory sets them up for its own scene, and the renderer takes them when the scene opens.

### Assets

- A waiting scene's assets are **kept loaded** when another scene opens in between. The splash → loading → level flow depends on this.
- An opened scene unloads only what neither it nor any waiting scene uses.
- While a preload is running, the old scene's and the new scene's assets are both in memory. Before, the old ones were dropped first. That's the price of not waiting.
- A skybox a factory sets loads on the background thread with the rest. A color or the procedural sky is built when the scene opens.

### A scene built early sees the game as it was then

If a factory reads something like "which character the player picked", it reads it at `preload` time. If that can still change, preload later, or call `cancel_preload` and preload again.

### Scenes opened with `open(..)`

Their `app.scene_manager.active` name is `"(unnamed)"`.

---

## 6. Checked

- Every example builds, and all 61 lib tests pass.
- Every example runs for 6–7 s without panicking. The walking example on OpenGL still hits the old `shadow_debug` shader crash; with the default backend it runs, with the player standing on the ground and the aim line drawn.
- `scene_loading`, run with a temporary auto-click version, went through: splash → menu (preloaded, instant) → loading scene (`open(..)`, progress bar to 100%) → hangar → Esc-style `open_scene("menu")` (freeze, then menu) → the loading scene again.

Not tested by hand: real mouse clicks on the menu buttons, and resizing the window while a scene is preloading.
