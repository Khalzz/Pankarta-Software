# Bodies and colliders

A node's physics is two properties, both **Rapier's**:

- **`RigidBodyBuilder`**: Rapier's rigid body, put on the node as it is.
- **`Colliders`**: a list of Rapier `ColliderBuilder`s, `Colliders::new().add(..).add(..)`.

Either can be there without the other. The engine adds only what Rapier can't know about:

| Engine part | What it does |
|---|---|
| Placing the body | The body starts where its node's `Transform3D` is |
| Kinematic bodies | A `kinematic_position_based` body follows its node every frame |
| A node with colliders and no body | Gets a fixed body, or one following the node if all its colliders are sensors |
| `Colliders` + `.detects(..)` / `.debug(..)` | The list, which layers a sensor sees, and how a collider is drawn for debugging |
| `Collider::from_model` / `from_own_model` + `Meshes` | A collider's shape from a model's meshes: its file's (read with the scene's other assets) and its parts |
| `Collider::from_mesh` / `from_own_mesh` | A collider's shape from a `Mesh`: exactly the geometry it's drawn with |
| `Model::hide_meshes` | Doesn't draw the meshes that are only there for collision |
| `Collider::hemisphere` | A dome: a shape Rapier has no constructor for |

The prelude has `RigidBodyBuilder`, `ColliderBuilder`, `MassProperties` and `CoefficientCombineRule`. The rest of Rapier is in `pankarta_software::rapier3d`, the engine's own version, so the types always match.

The full examples: `cargo run --example walking` (player, model colliders, trigger zones) and `cargo run --example water` (floating crates).

---

## 1. A body and its colliders

```rust
use pankarta_software::prelude::*;

// A crate: a 1 x 1 x 1 box (half its size each way) weighing 20 kg.
Node::new("crate")
    .with_property(Transform3D { position: Vector3::new(0.0, 2.0, 0.0), ..Default::default() })
    .with_property(RigidBodyBuilder::dynamic())
    .with_property(Colliders::new()
        .add(ColliderBuilder::cuboid(0.5, 0.5, 0.5).mass(20.0).friction(0.7)))

// A player: any Rapier body setting, straight on the builder.
Node::new("player")
    .with_property(Transform3D::default())
    .with_property(RigidBodyBuilder::dynamic().lock_rotations().ccd_enabled(true))
    .with_property(Colliders::new()
        .add(ColliderBuilder::capsule_y(0.5, 0.3)
            .translation(Vector3::new(0.0, 0.8, 0.0))      // feet at the node's origin
            .mass(80.0)
            .friction(0.0)
            .friction_combine_rule(CoefficientCombineRule::Min)))

// The ground, a wall: colliders and no body - they stay put.
Node::new("ground")
    .with_property(Transform3D::default())
    .with_property(Colliders::new().add(ColliderBuilder::halfspace(Vector3::y_axis())))
```

| Body | Moved by | |
|---|---|---|
| `RigidBodyBuilder::dynamic()` | forces, gravity, collisions | crates, players, debris |
| `RigidBodyBuilder::fixed()` | nothing | the same as no body, for non-sensor colliders |
| `RigidBodyBuilder::kinematic_position_based()` | its node's `Transform3D` (section 2) | moving platforms, doors |
| `RigidBodyBuilder::kinematic_velocity_based()` | the velocity you give it (`app.physics.body_mut(id)`); the engine leaves it alone | |
| no `RigidBodyBuilder` | — | static colliders; or, if all are sensors, a zone following its node |

The body starts **where its node is**: a `translation` or `rotation` on the `RigidBodyBuilder` is replaced by the node's `Transform3D`. Colliders are placed on the body with their own `translation` and `rotation`, in the body's frame. A body can have as many colliders as you `add`.

A body with no colliders is allowed too. It needs a mass of its own (`additional_mass_properties`, section 3) to move sensibly.

### Rapier settings worth knowing

```rust
// A body that slows down and stops spinning on its own.
RigidBodyBuilder::dynamic().linear_damping(0.2).angular_damping(2.0)

// Two touching colliders average their friction. So that a 0 always slides,
// whatever it touches, the lower one wins:
ColliderBuilder::capsule_y(0.5, 0.3).friction(0.0).friction_combine_rule(CoefficientCombineRule::Min)

// A bouncy ball whose bounce wins over what it hits:
ColliderBuilder::ball(0.2).restitution(0.9).restitution_combine_rule(CoefficientCombineRule::Max)

// Any shape Rapier has: cones, round boxes, compounds, height fields...
ColliderBuilder::cone(1.0, 0.5)
ColliderBuilder::round_cuboid(1.0, 0.5, 1.0, 0.1)
```

Bodies have **no damping unless you give them some**: a dynamic body spins and slides until friction or something else stops it.

---

## 2. Kinematic bodies: following the node

A `kinematic_position_based` body goes **wherever its node's `Transform3D` is**, every frame, and nothing else moves it: no gravity, no pushing. It follows its node's world position, so a parent or an animation moving the node moves it too.

```rust
// A moving platform: a behaviour moves the node, the player rides it.
Node::new("platform")
    .with_property(Transform3D::default())
    .with_property(RigidBodyBuilder::kinematic_position_based())
    .with_property(Colliders::new().add(ColliderBuilder::cuboid(2.0, 0.2, 2.0)))
    .with_behavior(MoveUpAndDown)
```

It pushes moving bodies it runs into, with its real speed.

---

## 3. Mass

Mass is Rapier's, and can be set on the colliders, on the body, or both.

### On the colliders: each part weighs something

**Each collider carries some mass, and the body is their sum.** Rapier also works out the center of mass (where the weight is) and how the body spins (its inertia) from where the colliders are and their shapes.

```rust
.with_property(RigidBodyBuilder::dynamic())
.with_property(Colliders::new()
    .add(ColliderBuilder::cuboid(1.0, 0.5, 4.0).mass(2000.0))                                         // fuselage
    .add(ColliderBuilder::cuboid(3.0, 0.1, 1.0).translation(Vector3::new(0.0, 0.2, 0.0)).mass(800.0))   // wings
    .add(ColliderBuilder::cuboid(0.4, 0.4, 1.0).translation(Vector3::new(0.0, 0.0, -3.5)).mass(200.0))) // engine: pulls the center back

// Or a density, in kg per cubic meter - the weight follows the size:
ColliderBuilder::cuboid(0.5, 0.5, 0.5).density(600.0)   // wood: this 1 m³ crate weighs 600 kg
```

A collider with neither has a density of 1: a 1 m³ box weighs 1 kg. Add a part (a missile under a wing, with its `.mass(..)`) and the total, the center of mass and the inertia all follow.

### On the body: numbers of your own

| | |
|---|---|
| `RigidBodyBuilder::dynamic().additional_mass(kg)` | adds `kg` to whatever the colliders weigh; Rapier scales the inertia from the colliders' shapes |
| `RigidBodyBuilder::dynamic().additional_mass_properties(MassProperties::new(center, kg, inertia))` | your exact mass, center of mass and inertia |

Body mass is **added to** the colliders' mass. For the body's numbers to be **the final** ones (a real aircraft's documented mass, center of mass and inertia), make the colliders weigh nothing:

```rust
let colliders = Colliders::new()
    .add(ColliderBuilder::cuboid(1.0, 0.5, 4.0).density(0.0))   // shape only, no weight
    .add(ColliderBuilder::cuboid(3.0, 0.1, 1.0).density(0.0));
let inertia = compute_principal_inertia(3000.0, center_of_mass, colliders.builders());

Node::new("jet")
    .with_property(Transform3D::default())
    .with_property(RigidBodyBuilder::dynamic()
        .additional_mass_properties(MassProperties::new(center_of_mass.into(), 3000.0, inertia)))
    .with_property(colliders)
```

`pankarta_software::physics::compute_principal_inertia(mass, center_of_mass, colliders)` approximates the inertia from where the colliders are, each counted as the upright box around it; it takes any list of `ColliderBuilder`s. While the game runs, Rapier's `app.physics.body_mut(id).set_additional_mass_properties(..)` changes them (a loadout, fuel burning off).

---

## 4. Sensors: trigger zones

A sensor is any collider with Rapier's **`.sensor(true)`**: it detects what overlaps it and stops nothing. Its node's behaviours hear about it (`on_sensor_enter` / `on_sensor_exit`), and `app.physics.entered(..)` / `exited(..)` / `is_inside(..)` tell anyone else.

**`.detects(..)` and `.debug(..)` configure the collider added just before them**, so write them indented under it:

```rust
// A zone: only sensors and no body, so it follows its node.
Node::new("zone")
    .with_property(Transform3D { position, ..Default::default() })
    .with_property(Colliders::new()
        .add(ColliderBuilder::cylinder(1.0, 2.0).sensor(true))
            .detects(["player"])            // only nodes with Layers::new(["player"])
            .debug(ColliderDebug::new()))   // drawn, with no gizmos on
    .with_behavior(Zone)

// A pickup radius around a player: one more collider on its body.
Node::new("player")
    .with_property(RigidBodyBuilder::dynamic().lock_rotations())
    .with_property(Colliders::new()
        .add(ColliderBuilder::capsule_y(0.5, 0.3).mass(80.0))
        .add(ColliderBuilder::ball(2.0).sensor(true))
            .detects(["coin"]))
```

A sensor detects moving bodies (and ones an animation holds still), not fixed ones.

### Domes

For a dome on the ground, a whole ball centered on the ground works exactly like one: its lower half is under the floor, where nothing goes.

```rust
Colliders::new().add(ColliderBuilder::ball(2.0).sensor(true))
```

When something can be under it (a dome on a platform, or floating), use `Collider::hemisphere` (section 6).

---

## 5. Shapes from a model or a mesh

`Collider::from_model` gives a **Rapier `ColliderBuilder`** whose shape comes from a model's meshes, so every Rapier setting chains straight onto it:

```rust
let table = Model::file("models/table.glb");

Node::new("table")
    .with_property(Transform3D::default())
    .with_property(RigidBodyBuilder::dynamic())
    .with_property(Colliders::new()
        .add(Collider::from_model(&table, Meshes::all(), ModelShape::ConvexHull)
            .mass(15.0)
            .friction(0.6)))
    .with_property(table)
```

`Collider::from_own_model(meshes, shape)` is the same with the node's own `Model`, whatever it is when the node spawns:

```rust
Node::new("crate")
    .with_property(Transform3D::default())
    .with_property(Model::file("models/crate.glb"))
    .with_property(RigidBodyBuilder::dynamic())
    .with_property(Colliders::new()
        .add(Collider::from_own_model(Meshes::named(["crate_col"]), ModelShape::ConvexHull).mass(20.0)))
```

A model collider can be a sensor too: `.add(Collider::from_model(&level, Meshes::named(["zone_exit"]), ModelShape::ConvexHull).sensor(true))`.

### A model's parts

A `Model`'s meshes are its file's **and its parts** (`Model::new().part(name, mesh)`). Parts are picked by name like a file's meshes, and need nothing read:

```rust
// A stool from parts: a seat and three legs, each its own collider.
let stool = Model::new()
    .part("seat", Mesh::cylinder_shape(0.4, 0.08, 16).transform(Transform3D { position: Vector3::new(0.0, 0.64, 0.0), ..Default::default() }))
    .part("leg_a", Mesh::cylinder_shape(0.05, 0.6, 6).transform(/* .. */))
    .part("leg_b", Mesh::cylinder_shape(0.05, 0.6, 6).transform(/* .. */))
    .part("leg_c", Mesh::cylinder_shape(0.05, 0.6, 6).transform(/* .. */));

Node::new("stool")
    .with_property(Transform3D::default())
    .with_property(RigidBodyBuilder::dynamic())
    .with_property(Colliders::new()
        .add(Collider::from_model(&stool, Meshes::all().each(), ModelShape::ConvexHull).mass(1.5)))
    .with_property(stool)
```

A part is placed as it's drawn: by its own `transform`, then the model's, then the node's scale.

### A mesh: `Collider::from_mesh`

`Collider::from_mesh(&mesh, shape)` collides with **exactly the geometry a `Mesh` is drawn with**: a cube, a sphere, a flat-sided cylinder or any shape turned through rings, bent or oval as you like (cones, buckets, vases, a jet's nose: `Mesh::profile_shape`) with as many sides as you give it, or your own mesh (`app.assets.add_mesh`, used as `Mesh::custom(name)`).

```rust
// A barrel like Blender's cylinder: Radius 0.5, Depth 1.2, 12 Vertices - drawn and colliding the same.
let barrel = Mesh::cylinder_shape(0.5, 1.2, 12);

Node::new("barrel")
    .with_property(Transform3D::default())
    .with_property(barrel.clone())
    .with_property(RigidBodyBuilder::dynamic())
    .with_property(Colliders::new().add(Collider::from_mesh(&barrel, ModelShape::ConvexHull).mass(30.0)))

// Or from the node's own Mesh, written once:
    .with_property(Colliders::new().add(Collider::from_own_mesh(ModelShape::ConvexHull).mass(30.0)))

// The mesh doesn't have to be drawn: an invisible 8-sided zone.
Colliders::new().add(Collider::from_mesh(&Mesh::cylinder_shape(2.0, 3.0, 8), ModelShape::ConvexHull).sensor(true))
```

It's `from_model(&Model::from(mesh), Meshes::all(), shape)`: a one-part model. Nothing is loaded: the engine's meshes are generated on the spot, and your own are already in memory.

For a perfectly round collider, use Rapier's own `ColliderBuilder::cylinder(half_height, radius)` / `ball(radius)` instead: they're exact shapes with no vertices at all.

### Which shape: `ModelShape`

| | Gives | Use it for |
|---|---|---|
| `ModelShape::ConvexHull` | The shrink-wrapped outline, solid, dents filled in | Moving bodies; sensors |
| `ModelShape::Trimesh` | The exact surface, hollow | Bodies that never move: terrain, buildings, a level |
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

A common practice is to model a **simple collision mesh** next to the detailed one in the same file, named so it can be picked (`rock_col`, or `col_` pieces), and hide it (below).

`app.assets.collision_meshes.mesh_names("models/table.glb")` lists a loaded file's mesh names, for checking a selector that matches nothing.

### One collider, or one per mesh: `.each()`

The meshes picked are **merged** into one shape. With `.each()`, each mesh becomes **its own collider**, all with the same settings (a `.mass(..)` is each one's mass).

This matters for convex hulls. Take a table modeled for collision as five boxes, `col_top` and `col_leg1` to `col_leg4`:

```rust
// Merged: one hull wrapping the top and all four legs - a solid block,
// with nothing able to roll under it.
Collider::from_model(&table, Meshes::prefix("col_"), ModelShape::ConvexHull)

// Each: one hull per piece - a top and four legs, with the space under it free.
Collider::from_model(&table, Meshes::prefix("col_").each(), ModelShape::ConvexHull)
```

`.each()` is how a **moving** concave object gets a shape that follows it, from the simple pieces modeled for it. Merging is right when it loses nothing: a single collision mesh, a `Trimesh` (it keeps every gap), or a `ConvexDecomposition` (Rapier splits it itself).

### Hiding the collision meshes: `Model::hide_meshes`

```rust
let table = Model::file("models/table.glb").hide_meshes(Meshes::prefix("col_"));

Node::new("table")
    .with_property(Transform3D::default())
    .with_property(RigidBodyBuilder::dynamic())
    .with_property(Colliders::new()
        .add(Collider::from_model(&table, Meshes::prefix("col_").each(), ModelShape::ConvexHull).mass(3.0)))
    .with_property(table)
```

The hidden meshes cast no shadow either. Every node hiding the same meshes of a file shares one copy of it on the GPU. A behaviour can still show one again with `model.mesh_mut(name).unwrap().visible = true`.

---

## 6. A dome: `Collider::hemisphere`

```rust
Collider::hemisphere(2.0)   // a dome of radius 2, flat side down on its center
```

It gives a `ColliderBuilder` like any other, so Rapier settings chain on: `.add(Collider::hemisphere(2.0).sensor(true))`. It's a convex hull around points on the dome (6 rings of 16 and the top), so it's a little faceted, and drawn that way in the debug view.

---

## 7. Things to know

- **Your node's `Layers`** set each collider's collision groups, unless you gave the builder groups of your own (`.collision_groups(..)`).
- **Rapier's API changes between versions.** Code using its builders may need small changes when the engine moves to a newer Rapier.
- **Debug drawing:** `app.renderer.debug.physics` is a set of gizmos, each on its own (see `PhysicsGizmos`):

  ```rust
  app.renderer.debug.physics.colliders = true;   // every collider: moving ones orange, fixed ones dim blue-gray
  app.renderer.debug.physics.sensors = true;     // sensor areas: cyan, green while something's inside
  app.renderer.debug.physics.rigidbodies = true; // each dynamic body's center of mass and velocity
  app.renderer.debug.physics.toggle_all();       // all on, or all off if any is on (walking: F5)
  ```

  The center of mass is drawn as three axes along the body's principal axes of inertia, gray while the body is asleep; the yellow line is how far it'll go in a second. What a game draws into `app.physics.debug_lines()` (forces, pushes) shows with `rigidbodies`. A collider with `.debug(..)` is drawn whatever's on.
- **Shapes in debug drawing:** the engine draws Rapier's exact shapes as what they are: a ball as three circles, a cylinder as its two end circles and side lines, a capsule as its circles and sides, a cone as its base circle and lines to its tip. Convex hulls (`from_model`, `from_mesh`, `hemisphere`) and triangle meshes show their real edges, and compounds each of their parts. A collider made from a `Mesh::profile_shape` is drawn by **its rings instead**: each ring and the lines along its length, traced onto the collider's real surface. Where the collider is the mesh, they sit exactly on your rings. Where a convex hull fills a dent, they're drawn out on the hull, so you can see that the dent doesn't collide.
