# Meshes and models: what changed

3D objects are now one of two node properties:

- **`Mesh`**: one piece of geometry with one material. A cube, a sphere, a plane, a mesh you built.
- **`Model`**: a 3D object made of named **parts**. They come from a file (`.glb`/`.gltf`), from meshes you give it, or both. Parts can be added, removed, moved and hidden while the game runs.

They replace `Model::new(ModelSource::...)` plus a separate `Material` on the node.

The example is `examples/3D/meshes_and_models.rs`. Run it with `cargo run --example meshes_and_models`.

---

## 1. `Mesh`

```rust
Node::new("crate")
    .with_property(Transform3D { position: Vector3::new(0.0, 0.5, 0.0), ..Default::default() })
    .with_property(Mesh::cube().material(Material::standard().texture("textures/crate.png")))
```

### Shapes

| Call | Shape |
|---|---|
| `Mesh::cube()` | 1 × 1 × 1 |
| `Mesh::plane(subdivisions)` | 1 × 1 across X and Z, facing up. `subdivisions` adds vertices for shaders that move them (waves). |
| `Mesh::sphere(radius)` | |
| `Mesh::cylinder(radius, height)` | Standing along Y |
| `Mesh::capsule(radius, height)` | Standing along Y, `height` from tip to tip |
| `Mesh::custom(name)` | A mesh you added with `app.assets.add_mesh(name, vertices, indices)` |

Sphere and cylinder share one loaded unit shape that's scaled to size. Each capsule size gets its own geometry, because scaling a capsule would squash its round ends.

### Settings

| Call / field | What it does |
|---|---|
| `.transform(Transform3D)` | Where it sits relative to its node (or its model): position, rotation, scale. Use `scale` to size a cube or a plane. |
| `.material(Material)` | How it looks. Without one: engine/standard, plain white. |
| `.casts_shadows(bool)`, `.receives_shadows(bool)` | On by default. |
| `visible` (field) | `false` hides it. |

---

## 2. `Model`

### From a file

```rust
Node::new("jet").with_property(Model::file("models/f16.glb"))
```

### From meshes

```rust
Node::new("tank").with_property(Model::new()
    .material(Material::standard().color(UiColor::Hex("#4D7C4A")))
    .part("body",   Mesh::cube().transform(Transform3D { scale: Vector3::new(3.0, 1.0, 4.5), ..Default::default() }))
    .part("turret", Mesh::cylinder(1.0, 0.7).transform(Transform3D { position: Vector3::y() * 1.45, ..Default::default() })))
```

### Both

A file model can also get parts of its own, like the red pod under the F-16 in the example:

```rust
Model::file("models/f16.glb").part("pod", Mesh::sphere(0.05))
```

### Placement

- `Model::transform(..)` places the **whole model** relative to its node. It replaces the old `.scaled(15.0)` / `.with_scale(..)`: write `.transform(Transform3D { scale: Vector3::new(15.0, 15.0, 15.0), ..Default::default() })`.
- A part's own `transform` places it **inside the model**: moved, turned and scaled along with the model.

### Materials

You can set them per part or for the whole model:

| Call | Applies to |
|---|---|
| `Model::material(m)` | Every part without its own material, and the file's meshes |
| `Mesh::material(m)` on a part | That part. It wins over the model's. |
| `Model::part_material("name", m)` | One part by name: one you added, or a **mesh inside the file** (its name in the glTF) |

### While the game runs

From a behaviour:

```rust
let model = node.get_property_mut::<Model>().unwrap();
model.part_mut("turret").unwrap().transform.rotation = aim;     // move or turn a part
model.part_mut("barrel").unwrap().visible = false;              // hide it
model.add_part("antenna", Mesh::cylinder(0.05, 2.0));           // add one (replaces one with the same name)
model.remove_part("antenna");                                    // take it off
model.set_part_material("body", Material::standard().color(UiColor::Hex("#F00")));
```

The engine notices on the next frame and puts parts on the renderer or takes them off. A part whose geometry nothing has loaded yet is loaded on the spot, which can cause a small hitch. To load it with the scene instead, name it beforehand with `scene.will_use(Mesh::cylinder(0.05, 2.0))` (or `will_use(Model::file(..))`).

Other calls: `get_part(name)`, `parts()` (the parts you added, in order), `file_path()`.

### The file's own meshes

A file model's meshes (by their name in the glTF) can be moved, turned, scaled and hidden **per node**:

```rust
let model = node.get_property_mut::<Model>().unwrap();
model.mesh_mut("left_aleron").unwrap().rotate_from_original(UnitQuaternion::from_axis_angle(&Vector3::x_axis(), 0.3));
model.mesh_mut("Afterburner").unwrap().visible = false;
for mesh in model.meshes() { println!("{}", mesh.name()); }
```

- `mesh_mut(name)` / `mesh(name)` / `meshes_mut()` / `meshes()` return `ModelMesh`es: `transform` (where it sits in the model, starting where the file put it) and `visible` are public; `name()`, `original_transform()`, `local_bounds()`, `rotate_from_original(rotation)` and `reset()` help.
- They're empty until the file is loaded: from when the node is spawned (for nodes spawned in `on_spawn`, once the scene has loaded - so use them from a behaviour's `update`).
- A node shares the one loaded copy of its file with every other node using it until one of its meshes changes. From then on it's drawn with its own light copy (see `Model::share`: its own mesh transforms, the same geometry and textures), so nothing else moves with it. That node no longer shares an instance buffer with the others.
- An object with several materials is one `ModelMesh` (all its `#primN` pieces move together).

### File-only settings (unchanged)

`.double_sided(DoubleSidedMeshes::..)` and `.variant(name)` work as before, on the file's meshes.

---

## 3. Color

Color lives in the material:

```rust
Mesh::sphere(0.6).material(Material::standard().color(UiColor::Hex("#3B82F6")))
Mesh::cube().material(Material::standard().texture("textures/grid.png").color(UiColor::Hex("#FFD166")))   // tints the texture
```

- On engine/standard, `.color(..)` switches the material to a new built-in shader, **engine/colored** (standard with the texture multiplied by the color).
- On `.tiled(..)` materials, it tints the tiled texture. engine/tiled has a new `color` parameter, white by default, so existing tiled materials look the same.
- On your own shader, it sets a `color` parameter, which your shader needs to declare as `color: vec4<f32>` in its `Params`.

The hex color is converted to linear, so it matches what the same hex looks like in the UI.

---

## 4. Behaviour changes worth knowing

- **A `Material` put directly on a node isn't used any more.** Give it to the `Mesh` or `Model` with `.material(..)`. The engine prints a warning when a node still has one.
- **One node can be several drawn instances.** A file model is drawn under the node's id. Each added part is drawn under `"node#part"`. This is internal, but it shows up in shader material keys.
- **Physics colliders read from a model file** (`TrimeshFromModel`) are scaled by the node's scale times the model's `transform.scale`, just as they were scaled by `.scaled(..)` before.
- **Not done yet:** a `Mesh` that is one single mesh taken out of a file (e.g. only the F-16's wing). For now, use the whole file as a `Model` and change its meshes' materials with `part_material`, and move or hide them with `mesh_mut` (see "The file's own meshes").

---

## 5. What was removed

| Removed | Use instead |
|---|---|
| `Model::new(ModelSource::Cube)` | `Mesh::cube()` |
| `Model::new(ModelSource::Plane { subdivisions })` | `Mesh::plane(subdivisions)` |
| `Model::new(ModelSource::file(path))` | `Model::file(path)` |
| `Model::new(ModelSource::mesh(name))` | `Mesh::custom(name)` |
| `.scaled(s)`, `.with_scale(v)`, `model.scale`, `render_scale` | `.transform(Transform3D { scale, .. })` |
| `Material` as a node property | `Mesh::material` / `Model::material` / `part_material` |
| `ModelSource` in the public API | It's engine-internal now |
| `scene.will_use(ModelSource)` | `scene.will_use(Mesh::..)` or `scene.will_use(Model::..)` |

All examples were moved over.

---

## 6. Where the code is

- `src/scene_manager/node/properties/mesh.rs`: `Mesh`, `MeshShape`.
- `src/scene_manager/node/properties/model.rs`: `Model`.
- `src/scene_manager/render_bridge.rs`: `draws(node)`, the list of instances a node draws. Spawning, the per-frame sync (adding and removing parts), material setup, scene loading and colliders all read that one list.
- `src/asset_manager/primitives.rs`: sphere, cylinder and capsule geometry.
- `src/shaders/materials/colored.wgsl`: engine/colored. `tiled.wgsl` has the new `color` parameter.

---

## 7. Checked

- All 66 lib tests pass, including new ones: a part placed inside its model, a model drawing its file and each visible part (a hidden one skipped), a part falling back to the model's material, and a sphere being the unit sphere scaled.
- Every example builds and runs for 6 seconds without panicking. The one exception is walking on OpenGL, which still hits the existing `shadow_debug` shader crash. On the default backend, walking runs, and a screenshot shows its tiled cubes, the F-16 and the aim line.
- In `meshes_and_models`, a screenshot shows the five colored shapes, the tank with its turret, barrel and antenna, and the F-16 with its red pod.
- Not checked on screen: the antenna appearing and disappearing every second (only one frame was captured, with it on), and the turret turning.
