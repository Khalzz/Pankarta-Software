# Pankarta Software

A game engine written in Rust: **wgpu** for rendering, **rapier** for physics, **SDL2** for the window, input and audio. Games are built from **scenes** made of **nodes**. A node holds **properties** (data) and **behaviours** (logic).

> **Early days.** The engine is young and its API still changes between versions. Expect breaking changes until 1.0.

---

## Contents

1. [Getting started](#getting-started)
2. [The core ideas](#the-core-ideas)
3. [The app](#the-app)
4. [Scenes](#scenes)
5. [Nodes](#nodes)
6. [Properties](#properties)
7. [Behaviours](#behaviours)
8. [Scene behaviours](#scene-behaviours)
9. [Children and the hierarchy](#children-and-the-hierarchy)
10. [Input](#input)
11. [Assets](#assets)
12. [Examples and further reading](#examples-and-further-reading)

---

## Getting started

### Requirements

- Rust (2021 edition).
- **SDL2.** On Windows, put `SDL2.dll` (plus `SDL2_image`, `SDL2_mixer` and `SDL2_ttf`) next to your executable or in your project root. On Linux and macOS, install SDL2 from your package manager.
- A GPU that wgpu supports (Vulkan, DirectX 12, Metal or OpenGL).

### Adding it to a project

```toml
[dependencies]
pankarta-software = "0.1"
tokio = { version = "1", features = ["full"] }
```

`App::new` is `async`, so `main` runs on tokio.

### A first game: a spinning cube

```rust
use pankarta_software::prelude::*;

/// Logic for one node: turns it around Y.
struct Spin {
    speed: f32,
}

impl Behavior for Spin {
    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            let turn = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), self.speed * app.time.delta_time);
            transform.rotation = turn * transform.rotation;
        }
    }
}

/// The scene: builds its nodes once, when it's created.
struct MainScene;

impl MainScene {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        app.renderer.set_environment(Environment::ProceduralSky { sun: true });

        let cube = Node::new("cube")
            .with_property(Transform3D { position: Vector3::new(0.0, 1.0, 0.0), ..Default::default() })
            .with_property(Mesh::cube().material(Material::standard().color(UiColor::Hex("#3B82F6"))))
            .with_behavior(Spin { speed: 1.5 });

        let sun = Node::new("sun")
            .with_property(Transform3D::facing(Vector3::new(-0.3, -1.0, -0.2)))
            .with_property(Light::directional([1.0, 1.0, 1.0]));

        let camera = Node::new("camera")
            .with_property(Transform3D { position: Vector3::new(0.0, 2.5, -6.0), ..Default::default() })
            .with_property(CameraLens::new(60.0))
            .with_property(LookAt { target: "cube".into() });

        for node in [cube, sun, camera] {
            scene.spawn_node(app, node).unwrap();
        }
        MainScene
    }
}

impl SceneBehaviour for MainScene {}

#[tokio::main]
async fn main() {
    let mut app = App::new().await.expect("couldn't create the app");
    app.window.set_title("My game");

    app.scene_manager.create_scene("main", MainScene::new);
    app.scene_manager.open_scene("main");
    app.run();
}
```

Everything above comes from one import: `use pankarta_software::prelude::*;`.

---

## The core ideas

```
App                         the engine: window, renderer, physics, input, audio, assets
└── SceneManager            which scene is open; opens and preloads the others
    └── Scene               one "screen" of the game: a menu, a level
        ├── SceneBehaviour  the scene's own logic
        └── Node            one thing in the scene: a player, a crate, a light, a UI layer
            ├── properties  its data: Transform3D, Mesh, Physics, Light, ...
            ├── behaviours  its logic: your own structs, with update hooks
            └── children    other nodes that follow it
```

| Piece | What it is | You write... |
|---|---|---|
| **App** | The engine itself | Nothing. You create it and run it. |
| **Scene** | A container of nodes, plus one `SceneBehaviour` | A struct implementing `SceneBehaviour`, and a function that builds the scene |
| **Node** | An id with properties, behaviours and children | `Node::new("id").with_property(..).with_behavior(..)` |
| **Property** | Plain data any system can read | Usually nothing: use the built-in ones, or any struct of your own |
| **Behaviour** | One node's own logic and state | A struct implementing `Behavior` |

The rule of thumb: **properties are *what* a node is, behaviours are *what it does*.**

---

## The app

`App` owns every engine system. You reach them through its fields:

| Field | What it's for |
|---|---|
| `app.window` | Title, size, window mode (`set_title`, `set_size`, `set_window_mode`), cursor lock |
| `app.renderer` | Sky (`set_environment`), ambient light, shadows, vsync, shaders, the frame plan |
| `app.physics` | Gravity (`set_gravity`), timestep, the bodies (`body_mut(id)`) |
| `app.input` | Keys, mouse, controllers, named actions (see [Input](#input)) |
| `app.time` | `delta_time` (seconds since last frame), `unscaled_delta_time` |
| `app.assets` | Loaded models, images, textures and sounds; `add_mesh` for your own geometry |
| `app.particles` | Wind and sea level for particle effects |
| `app.scene_manager` | Creating, opening and preloading scenes |
| `app.audio` / `app.play_sound(path)` | Sound |

These settings are **global**: set once, they hold for every scene until they're set again.

```rust
let mut app = App::new().await?;   // hidden 800 x 600 window, shown by run()
app.window.set_title("My game");
app.physics.set_gravity(Vector3::new(0.0, -9.81, 0.0));
// ... register scenes ...
app.run();                          // the main loop - returns when the window closes
```

---

## Scenes

A scene is **registered by name** with a function that builds it, then **opened**:

```rust
app.scene_manager.create_scene("level_1", LevelOne::new);
app.scene_manager.open_scene("level_1");
```

The build function receives the new `Scene` and the `App`, spawns the scene's nodes, and returns the scene's `SceneBehaviour`:

```rust
struct LevelOne { score: u32 }

impl LevelOne {
    fn new(scene: &mut Scene, app: &mut App) -> Self {
        scene.spawn_node(app, Node::new("player") /* ... */).unwrap();
        LevelOne { score: 0 }
    }
}

impl SceneBehaviour for LevelOne {}
```

### Switching scenes

| Call | What it does |
|---|---|
| `open_scene(name)` | Makes `name` the active scene at the start of the next frame. The old scene and its nodes are removed. |
| `open(\|scene, app\| Loading::new(scene, app, "level_2"))` | Opens a scene built by a closure, without registering it. This is how you pass parameters to a scene. |
| `preload(name)` | Builds `name` and loads its assets in the background, while the current scene keeps running |
| `is_ready(name)` / `load_progress(name)` | Whether a preloaded scene is ready, and how far it got (`0.0` to `1.0`) |
| `cancel_preload(name)` | Drops a preloaded scene |

There is only ever **one active scene**. If you open a scene that isn't loaded yet, the current one stays on screen, frozen, until the new one is ready. Splash and loading screens are just ordinary scenes that preload the next one; see [docs/scene_loading.md](docs/scene_loading.md).

### Assets load by themselves

You don't list what a scene needs. When it's built, the engine looks at its nodes (their meshes, models, textures, colliders) and loads all of it before the scene starts.

### Spawning and finding nodes in a scene

```rust
scene.spawn_node(app, node)?;                 // add a node (with its children)
scene.despawn_node(app, "enemy_3");           // remove one (and its children)
let player = scene.get_node("player");        // look one up by id
let gun = scene.child_mut("player", "gun");   // a child, by its parent and its name
```

---

## Nodes

A node is **an id plus whatever you put on it**. It has no fixed type: a light is a node with a `Light` property, and a player is a node with a mesh, a physics body and a `Player` behaviour.

```rust
let crate_node = Node::new("crate_1")                           // a unique id in the scene
    .with_property(Transform3D { position, ..Default::default() })
    .with_property(Mesh::cube())
    .with_property(Physics {
        rigidbody: RigidBodyData::dynamic(20.0),
        colliders: vec![Collider::new(ColliderShape::Cuboid { half_extents: (0.5, 0.5, 0.5) })],
    })
    .with_behavior(Breakable { hits_left: 3 });
```

| Method | What it does |
|---|---|
| `Node::new(id)` | A new node. The id must be unique in the scene. |
| `.with_property(p)` | Builder: adds a property |
| `.with_behavior(b)` | Builder: adds a behaviour |
| `.with_child(node)` | Builder: adds a child (see [Children](#children-and-the-hierarchy)) |
| `get_property::<T>()` / `get_property_mut::<T>()` | Reads or changes a property, by its type |
| `set_property(p)` / `remove_property::<T>()` | Adds or replaces one, or takes one off, while the game runs |
| `get_behavior::<B>()` / `get_behavior_mut::<B>()` | Reaches a behaviour, by its type |
| `transform_3d()` / `transform_2d()` (and `_mut`) | Shortcuts for the node's transform |
| `node.id` | Its id |

A node has **at most one property of each type**. Adding a second `Mesh` replaces the first. To draw several meshes, use a `Model` with parts, or child nodes.

---

## Properties

A property is **plain data** on a node. Engine systems read them: the renderer draws every node with a `Mesh`, physics simulates every node with `Physics`, and so on. You reach them all the same way, by type:

```rust
if let Some(transform) = node.get_property_mut::<Transform3D>() {
    transform.position.y += 1.0;
}
```

### The built-in properties

| Property | What it gives the node |
|---|---|
| `Transform3D` | A position, rotation and scale in the 3D world |
| `Transform2D` | A position, rotation, scale and z index on the 2D plane (screen units). A node has one transform or the other, never both. |
| `Mesh` | One piece of geometry with one material: `Mesh::cube()`, `plane`, `sphere`, `cylinder`, `capsule`, `custom(name)` |
| `Model` | A 3D object made of named parts: from a file (`Model::file("models/ship.glb")`), from meshes, or both |
| `Physics` | A rigid body (`RigidBodyData`) and its colliders |
| `CameraLens` | Makes the node a camera (field of view, near plane). A scene without one gets a default camera. |
| `LookAt` | Turns the node to face another node every frame |
| `Light` | The sun: `Light::directional(color)`, shining along the node's +Z |
| `Shape2D` | A rectangle, square or circle on a 2D node |
| `Text` | Text on a node, in 3D or 2D |
| `Line` | A line in 3D or 2D, `Line::new(start, end)`, with points local to the node or in the world |
| `Emitter` | A particle effect (`pankarta_software::particles::Emitter`) |
| `UiLayer` | A UI tree: panels, labels, buttons, images |

More detail is in [docs/meshes_and_models.md](docs/meshes_and_models.md) and [docs/shapes_and_lines.md](docs/shapes_and_lines.md).

### Your own properties

**Any type can be a property.** No trait to implement, no registration:

```rust
struct Health(u32);
struct Team { name: String }

let enemy = Node::new("enemy")
    .with_property(Health(100))
    .with_property(Team { name: "red".into() });

// later, from anywhere that has the node:
if let Some(health) = enemy.get_property_mut::<Health>() {
    health.0 -= 10;
}
```

Use your own property for data that **other code reads**: a health value an enemy's attack lowers, or a team that the AI checks. For data only one node's own logic uses, keep it in its behaviour instead.

---

## Behaviours

A behaviour is **a struct with its own state, plus hooks the engine calls**. It's how a node does things: a player that moves, an enemy that patrols, a door that opens.

### Writing one

1. Make a struct holding the behaviour's own state.
2. Implement `Behavior` for it, writing only the hooks you need. They're all empty by default.
3. Put it on a node with `.with_behavior(..)`.

```rust
use pankarta_software::prelude::*;

struct Patrol {
    from: Vector3<f32>,
    to: Vector3<f32>,
    speed: f32,
    going_back: bool,
}

impl Behavior for Patrol {
    fn on_spawn(&mut self, node: &mut Node, _scene: &mut SceneAccess, _app: &mut App) {
        // Runs once, when the node enters a running scene.
        if let Some(transform) = node.get_property_mut::<Transform3D>() {
            transform.position = self.from;
        }
    }

    fn update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
        // Runs every frame.
        let Some(transform) = node.get_property_mut::<Transform3D>() else { return };
        let target = if self.going_back { self.from } else { self.to };
        let to_target = target - transform.position;
        let step = self.speed * app.time.delta_time;
        if to_target.magnitude() <= step {
            transform.position = target;
            self.going_back = !self.going_back;
        } else {
            transform.position += to_target.normalize() * step;
        }
    }
}

let guard = Node::new("guard")
    .with_property(Transform3D::default())
    .with_property(Mesh::capsule(0.4, 1.8))
    .with_behavior(Patrol { from: Vector3::zeros(), to: Vector3::new(10.0, 0.0, 0.0), speed: 2.0, going_back: false });
```

### The hooks

| Hook | When it runs | Use it for |
|---|---|---|
| `on_spawn` | Once, when the node enters a running scene. For nodes built with the scene, that's when the scene opens. | Setting the node up: reading its properties, adding ones it builds itself |
| `update` | Every rendered frame. The frame's length is `app.time.delta_time`. | Most game logic: movement, input, animation, timers |
| `fixed_update` | Every physics step (120 a second by default), right before the physics world advances | Forces, velocities, anything that must run the same at any frame rate |
| `on_origin_shift` | When the floating origin moves the world (very large worlds) | Shifting world positions the behaviour keeps in its own fields |

### What a hook can reach

Every hook gets three things:

| Parameter | What it is |
|---|---|
| `node: &mut Node` | **Its own node**: properties, other behaviours, id |
| `scene: &mut SceneAccess` | **The rest of the scene**: other nodes, the cameras, spawning and despawning |
| `app: &mut App` | **The engine**: time, input, physics, audio, assets |

```rust
fn update(&mut self, node: &mut Node, scene: &mut SceneAccess, app: &mut App) {
    // another node
    let player_position = scene.get_node("player")
        .and_then(|player| player.transform_3d())
        .map(|transform| transform.position);

    // input
    if app.input.is_action_just_pressed("fire") {
        // spawn a node - it's added once every behaviour has run this frame
        scene.spawn(Node::new(format!("bullet_{}", self.shots))
            .with_property(Transform3D { position: node.transform_3d().unwrap().position, ..Default::default() })
            .with_property(Mesh::sphere(0.1)));
        self.shots += 1;
    }

    // remove this node
    if self.health == 0 {
        scene.despawn(node.id.clone());
    }
}
```

`SceneAccess` has `get_node`, `get_node_mut`, `child`, `child_mut`, `nodes()`, `cameras()`, `spawn(node)` and `despawn(id)`.

### How they run

- While a node's behaviours run, **the node is taken out of the scene and handed to them**. That's why its own data comes as `node`, and `scene.get_node(own_id)` finds nothing.
- **Spawns and despawns wait** until every behaviour has run that frame, so no behaviour sees the scene change halfway through.
- A node can have **several behaviours** of different types. They run in the order they were added.

### Physics in a behaviour

Push bodies in `fixed_update`, through `app.physics` with the node's id:

```rust
fn fixed_update(&mut self, node: &mut Node, _scene: &mut SceneAccess, app: &mut App) {
    if let Some(body) = app.physics.body_mut(&node.id) {
        body.apply_impulse(Vector3::new(0.0, 5.0, 0.0), true);
    }
}
```

The node's `Transform3D` follows its body by itself.

---

## Scene behaviours

A scene's own logic is a `SceneBehaviour`, the struct its build function returns. It works like a node's behaviour, but receives the whole `Scene`:

| Hook | When it runs |
|---|---|
| `on_spawn(scene, app)` | Once, when the scene opens, after its nodes' `on_spawn` |
| `update(scene, app, ctx)` | Every frame, before the nodes' behaviours |
| `fixed_update(scene, app)` | Every physics step, after the nodes' `fixed_update` |
| `late_update(scene, app, ctx)` | Every frame, after all the nodes' behaviours |
| `after_cameras(scene, app)` | Every frame, once cameras and transforms are final. Use it for anything placed relative to the camera. |
| `debug_ui(ctx, scene, app)` | Every frame the debug UI is open: your own egui windows |
| `on_origin_shift(scene, app, shift)` | When the floating origin moves |

```rust
struct LevelOne { time_left: f32 }

impl SceneBehaviour for LevelOne {
    fn update(&mut self, scene: &mut Scene, app: &mut App, _ctx: &mut FrameContext) {
        self.time_left -= app.time.delta_time;
        if self.time_left <= 0.0 {
            app.scene_manager.open_scene("game_over");
        }
    }
}
```

Put logic in the scene behaviour when it's about **the scene as a whole** (rules, timers, win conditions). Put it in a node behaviour when it's about **one thing**.

---

## Children and the hierarchy

A node can have children. They follow it: they move, turn and are removed with it.

```rust
let ship = Node::new("ship")
    .with_property(Transform3D::default())
    .with_property(Model::file("models/ship.glb"))
    .with_child(Node::new("engine_glow")
        .with_property(Transform3D { position: Vector3::new(0.0, 0.0, -3.0), ..Default::default() })   // relative to the ship
        .with_property(Mesh::sphere(0.3).material(Material::standard().color(UiColor::Hex("#FF8800")))))
    .with_child(Node::new("chase_camera")
        .with_property(Transform3D { position: Vector3::new(0.0, 3.0, -12.0), ..Default::default() })
        .with_property(CameraLens::new(70.0))
        .follow_position_only());   // follows the ship but doesn't turn with it
```

- **A child's transform is relative to its parent.** The engine works out where it really is every frame and writes that to the child's own `Transform3D`.
- **A child's id is `"parent/child"`**: `"ship/engine_glow"`. Reach it with `scene.child("ship", "engine_glow")`.
- **A child with no transform of its own sits right at its parent.**
- `.follow_position_only()` makes the child follow the parent's position but not its rotation. `.global()` makes it stay where it is in the world (still removed with its parent).
- A child with a `Physics` body is moved by physics, not by its parent.
- 2D works the same with `Transform2D`.

---

## Input

Read keys and buttons directly, or through **named actions** loaded from a bindings file:

```rust
// directly
if app.input.is_pressed(InputSource::key(Key::Space)) { /* held */ }
if app.input.is_just_pressed(InputSource::key(Key::E)) { /* this frame */ }

// by action - the game ships its default bindings in a file
app.input.load_bindings("input.ron")?;
if app.input.is_action_pressed("jump") { /* ... */ }
let throttle = app.input.action_strength("forward");   // 0.0 - 1.0, for sticks and triggers
```

A bindings file:

```ron
(
    actions: [
        (action: "forward", bindings: [Key("W"), ControllerAxis(axis: "LeftY", positive: false)]),
        (action: "jump", bindings: [Key("Space"), ControllerButton("A")]),
    ],
    mouse: (x_sensitivity: 3.0, y_sensitivity: 3.0),
)
```

Players' own rebinds can be saved to their user folder: see `user_dir` and `app.input.set_user_bindings_path`. The `walking` example shows the whole setup.

---

## Assets

Assets live in an **`assets/` folder** in your project. Everything is referred to by its path inside it:

```rust
Model::file("models/ship.glb")
Material::standard().texture("textures/metal.png")
app.play_sound("sounds/explosion.ogg");
```

- The folder is indexed when the game starts, but only names are read; **files load when a scene uses them**.
- A scene's assets **load before it starts**, found from its nodes. One a node gets later is loaded on the spot; to have it loaded with the scene instead, name it with `scene.will_use(Mesh::..)` or `scene.will_use(Model::file(..))`.
- Assets a new scene doesn't use are dropped when it opens.
- Models are glTF (`.glb` / `.gltf`). Images are PNG or JPEG.

---

## Examples and further reading

Run any example with `cargo run --example <name>`:

| Example | Shows |
|---|---|
| `basic_window`, `configurable_window` | The window |
| `input` | Keys, mouse, controllers, actions |
| `2d` | 2D nodes and shapes |
| `ui_components`, `ui_greetings` | The UI |
| `free_camera` | A flying camera, meshes, tiled textures |
| `meshes_and_models` | Every mesh shape, a model built from parts, a file model |
| `model_viewer` | Loading a glTF model |
| `text_and_lines` | Text and lines in 3D and 2D |
| `particles` | Particle emitters |
| `walking` | A first-person player with physics, bindings, and a custom shader (the sign glass) |
| `water` | A game-side shader and render pass, buoyancy |
| `far_from_origin` | The floating origin for huge worlds |
| `scene_loading` | A splash screen, a menu and a loading screen as ordinary scenes |

In-depth docs, in [docs/](docs/):

- [scene_loading.md](docs/scene_loading.md): preloading and switching scenes, splash and loading screens
- [meshes_and_models.md](docs/meshes_and_models.md): `Mesh`, `Model`, parts and materials
- [shapes_and_lines.md](docs/shapes_and_lines.md): `Line`, `Shape2D`, 2D `Text`
- [sign_glass.md](docs/sign_glass.md): a custom shader drawing a sign seen only through a glass
- [animation_tracks.md](docs/animation_tracks.md): animation
