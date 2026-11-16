# Colliders

A node's `Physics` property is a rigid body and its **colliders**: the shapes it touches things with. **Colliders are Rapier's own `ColliderBuilder`.** The engine adds only what Rapier can't do on its own:

| Engine part | What it does |
|---|---|
| `Physics::dynamic(..)` / `fixed()` / `new(..)` + `.collider(..)` | Puts the body and its colliders on a node |
| `Collider::from_model` / `from_own_model` | Makes a Rapier collider's shape from a model file's meshes, read with the scene's other assets |
| `Meshes` | Picks which meshes of the file, merged or `.each()` one collider apiece |
| `Model::hide_meshes` | Doesn't draw the meshes that are only there for collision |
| `Sensor` + `.detects(..)` / `.debug(..)` | A trigger zone: who it sees, and how it's drawn for debugging |
| `physics::shapes` | A few shapes Rapier has no constructor for, as `ColliderBuilder`s |

The prelude has `ColliderBuilder` and `CoefficientCombineRule`. The rest of Rapier is in `pankarta_software::rapier3d`, the engine's own version, so the types always match.

The full example is the walking scene: `cargo run --example walking`.

---

## 1. A body and its colliders

```rust
use pankarta_software::prelude::*;

// A crate: a 1 x 1 x 1 box (half its size each way).
Node::new("crate")
    .with_property(Transform3D { position: Vector3::new(0.0, 2.0, 0.0), ..Default::default() })
    .with_property(Physics::dynamic(5.0)
        .collider(ColliderBuilder::cuboid(0.5, 0.5, 0.5).friction(0.7).restitution(0.1)))

// The ground: never moves.
Node::new("ground")
    .with_property(Transform3D::default())
    .with_property(Physics::fixed().collider(ColliderBuilder::halfspace(Vector3::y_axis())))

// A player: a pill that never tips over.
Node::new("player")
    .with_property(Transform3D::default())
    .with_property(Physics::new(RigidBodyData { lock_rotations: true, ccd: true, ..RigidBodyData::dynamic(70.0) })
        .collider(ColliderBuilder::capsule_y(0.5, 0.3)
            .translation(Vector3::new(0.0, 0.8, 0.0))      // feet at the node's origin
            .friction(0.0)
            .friction_combine_rule(CoefficientCombineRule::Min)))
```

| | |
|---|---|
| `Physics::dynamic(mass)` | A moving body of `mass` kg |
| `Physics::fixed()` | A body that never moves |
| `Physics::new(RigidBodyData { .. })` | A body with more settings: `lock_rotations`, `ccd`, `gravity_scale`, `initial_velocity`, `center_of_mass` |
| `.collider(c)` | One more collider: a `ColliderBuilder`, or a `Collider` with `.debug(..)` |

A body has as many colliders as you give it, each placed with its own Rapier `translation` and `rotation`, in the body's frame.

### Rapier settings worth knowing

```rust
// Where it sits on the body, and how it's turned.
ColliderBuilder::cylinder(0.2, 0.5).translation(Vector3::new(0.0, 1.0, 0.0)).rotation(Vector3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2))

// Two touching colliders average their friction. So that a 0 always slides,
// whatever it touches, the lower one wins:
ColliderBuilder::capsule_y(0.5, 0.3).friction(0.0).friction_combine_rule(CoefficientCombineRule::Min)

// A bouncy ball whose bounce wins over what it hits:
ColliderBuilder::ball(0.2).restitution(0.9).restitution_combine_rule(CoefficientCombineRule::Max)

// Anything else Rapier has: cones, round boxes, compounds, height fields...
ColliderBuilder::cone(1.0, 0.5)
ColliderBuilder::round_cuboid(1.0, 0.5, 1.0, 0.1)
```

### Shapes Rapier doesn't have: `physics::shapes`

```rust
use pankarta_software::physics::shapes;

shapes::cylinder_with_sides(1.0, 1.0, 32)   // a cylinder with flat sides, like Blender's (Radius 1, Depth 2, 32 vertices)
shapes::hemisphere(2.0)                     // a dome, flat side down
```

They're `ColliderBuilder`s like any other: `shapes::hemisphere(2.0).friction(0.4)`. Rapier's own `ColliderBuilder::cylinder` is perfectly round.

---

## 2. Shapes from a model file

`Collider::from_model` gives a **Rapier `ColliderBuilder`** whose shape comes from a model's meshes, so every Rapier setting chains straight onto it:

```rust
let table = Model::file("models/table.glb");

Node::new("table")
    .with_property(Transform3D::default())
    .with_property(Physics::dynamic(15.0)
        .collider(Collider::from_model(&table, Meshes::all(), ModelShape::ConvexHull)
            .friction(0.6)
            .density(2.0)))
    .with_property(table)
```

`Collider::from_own_model(meshes, shape)` is the same with the node's own `Model` file, whatever it is when the node spawns:

```rust
Node::new("crate")
    .with_property(Transform3D::default())
    .with_property(Model::file("models/crate.glb"))
    .with_property(Physics::dynamic(20.0)
        .collider(Collider::from_own_model(Meshes::named(["crate_col"]), ModelShape::ConvexHull)))
```

### Which shape: `ModelShape`

| | Gives | Use it for |
|---|---|---|
| `ModelShape::ConvexHull` | The shrink-wrapped outline, solid, dents filled in | Moving bodies; sensors |
| `ModelShape::Trimesh` | The exact surface, hollow | **Static** bodies only: terrain, buildings, a level |
| `ModelShape::ConvexDecomposition` | Solid convex pieces Rapier works out for you | Moving concave bodies, when you didn't model pieces yourself |

### What happens, and when

1. **Writing the node:** nothing is loaded. The collider's shape is a stand-in saying which file, which meshes and which `ModelShape`. Your Rapier settings are already on the builder.
2. **Loading the scene:** the file's meshes are read **once**, on the loading thread with the models and textures (the loading screen shows `Collider models/table.glb`). They're kept in `app.assets.collision_meshes` while the game runs, so other nodes, later spawns and other scenes reuse them.
3. **The node spawns:** the meshes are placed and scaled **as the node draws them** (the node's scale and the model's own `transform`), made into the shape, and swapped into the builder. Your settings stay. A table scaled ×2 gets a ×2 collider.

If a collider can't be made (no file, no mesh with that name, a mesh too flat for a hull), the node spawns without it and the console says why.

A node spawned later, while the game runs, with a model nothing else used yet, reads the file on the spot. To have it read with the scene instead:

```rust
scene.will_use_collision_meshes(&Model::file("models/barrel.glb"));
```

### Picking meshes: `Meshes`

A model file often holds several meshes. In Blender each object becomes a mesh with the object's name.

| | Picks |
|---|---|
| `Meshes::all()` | Every mesh of the file |
| `Meshes::named(["rock_col"])` | The meshes with these names |
| `Meshes::prefix("col_")` | The meshes whose names start with `col_` |

A common practice is to model a **simple collision mesh** next to the detailed one in the same file, named so it can be picked (`rock_col`, or `col_` pieces), and hide it (section 3).

`app.assets.collision_meshes.mesh_names("models/table.glb")` lists a loaded file's mesh names, for checking a selector that matches nothing.

### One collider, or one per mesh: `.each()`

The meshes picked are **merged** into one shape. With `.each()`, each mesh becomes **its own collider**, all with the same settings.

This matters for convex hulls. Take a table modeled for collision as five boxes, `col_top` and `col_leg1` to `col_leg4`:

```rust
// Merged: one hull wrapping the top and all four legs - a solid block,
// with nothing able to roll under it.
Collider::from_model(&table, Meshes::prefix("col_"), ModelShape::ConvexHull)

// Each: one hull per piece - a top and four legs, with the space under it free.
Collider::from_model(&table, Meshes::prefix("col_").each(), ModelShape::ConvexHull)
```

`.each()` is how a **moving** concave object gets a shape that follows it, from the simple pieces modeled for it. Merging is right when it loses nothing: a single collision mesh, a `Trimesh` (it keeps every gap), or a `ConvexDecomposition` (Rapier splits it itself).

---

## 3. Hiding the collision meshes: `Model::hide_meshes`

```rust
let table = Model::file("models/table.glb").hide_meshes(Meshes::prefix("col_"));

Node::new("table")
    .with_property(Transform3D::default())
    .with_property(Physics::dynamic(15.0)
        .collider(Collider::from_model(&table, Meshes::prefix("col_").each(), ModelShape::ConvexHull).friction(0.6)))
    .with_property(table)
```

The hidden meshes cast no shadow either. Every node hiding the same meshes of a file shares one copy of it on the GPU. A behaviour can still show one again with `model.mesh_mut(name).unwrap().visible = true`.

---

## 4. Sensors

A `Sensor` is a collider that detects and stops nothing. Its node's behaviours hear about what enters and leaves it (`on_sensor_enter` / `on_sensor_exit`). Any `ColliderBuilder` becomes one, model ones too:

```rust
Sensor::from(ColliderBuilder::ball(1.5).translation(Vector3::new(0.0, 1.5, 0.0)))
Sensor::from(shapes::hemisphere(2.0))
Sensor::from(Collider::from_model(&level, Meshes::named(["zone_exit"]), ModelShape::ConvexHull))
Sensor::from(Collider::from_model(&level, Meshes::prefix("zone_l_").each(), ModelShape::ConvexHull))   // an L from two boxes
Sensor::from(Collider::from_own_model(Meshes::all(), ModelShape::ConvexHull))
    .detects(["player"])          // only nodes with Layers::new(["player"])
    .debug(ColliderDebug::new())  // drawn, with no debug view on
```

On a node without `Physics`, a sensor follows the node. On a node with `Physics`, it's one more collider on that body.

---

## 5. Things to know

- **Your node's `Layers`** set each collider's collision groups, unless you gave the builder groups of your own (`.collision_groups(..)`).
- **Rotational inertia** is worked out from the colliders: each solid collider counts as the upright box around it. Sensors, half spaces and triangle meshes don't count.
- **Rapier's API changes between versions.** Code using `ColliderBuilder` may need small changes when the engine moves to a newer Rapier.
- **Debug drawing** draws boxes, balls, cylinders, cones, capsules, convex shapes, triangle meshes and compounds of those.
