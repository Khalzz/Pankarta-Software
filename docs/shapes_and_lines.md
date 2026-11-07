# Lines and 2D shapes: what changed

A `Line` now has a **start and an end**, both chosen by you, instead of always starting at its node and pointing at something. `Shape2D` and 2D `Text` are placed on their node with a normal `Transform2D`, instead of the old `Offset2D`.

The example is `examples/3D/text_and_lines.rs`. Run it with `cargo run --example text_and_lines`.

---

## 1. Lines

### Making one

There's one constructor for both 2D and 3D. The kind of vector you pass decides which it is:

```rust
Line::new(Vector3::zeros(), Vector3::z() * 14.0)        // a 3D line
Line::new(Vector2::zeros(), Vector2::new(60.0, 0.0))    // a 2D line
Line::new(Vector2::zeros(), Vector3::z())               // doesn't compile: both points must be the same kind
```

### Where the points are

By default both points are **local to the line's node**: they move, turn and scale with it. Since the node follows its parent (the usual child rules), a line on a child node with no transform starts right at the parent.

| Call | Start | End |
|---|---|---|
| `Line::new(a, b)` | local | local |
| `.end_in_world()` | local | a world position |
| `.start_in_world()` | a world position | local |
| `.world()` | a world position | a world position |

"World" means a place in the 3D world for a 3D line, and a place on the 2D plane for a 2D line.

The fields are `start_space` and `end_space` (`LineSpace::Local` / `LineSpace::World`) if you want to change them later.

### Moving the points

A behaviour moves them with `set_start` and `set_end`, using the same kind of vector the line was made with:

```rust
// a line from the drone to the tower, kept on the tower every frame
Node::new("tether")
    .with_property(Line::new(Vector3::zeros(), Vector3::zeros()).end_in_world())
    .with_behavior(Tether { target: "tower" })

impl Behavior for Tether {
    fn update(&mut self, node: &mut Node, scene: &mut SceneAccess, _app: &mut App) {
        let Some(tower) = scene.get_node(self.target).and_then(|n| n.transform_3d()).map(|t| t.position) else { return };
        node.get_property_mut::<Line>().unwrap().set_end(tower);
    }
}
```

`start()`, `end()` and `is_2d()` read them back.

**Mixing kinds:** `set_end` with a `Vector3` on a 2D line, or the other way round, is ignored, and a warning is printed once. A line made with 2D points on a 3D node, or the other way round, is still drawn, and a warning is printed once for that node.

### Settings

| Call | What it does |
|---|---|
| `.width(px)` | Thickness in pixels, the same however far away. 2 by default. |
| `.color(UiColor)` | White by default. |
| `.always_on_top()` | 3D: seen through what's in front of it. |
| `.z_index(i32)` | 2D: added to its node's z index (see section 2). |
| `.upright()` | Its local points move with the node but don't turn with it. |

---

## 2. `Shape2D` and 2D `Text`

### Placing them on their node

Both now take a `Transform2D`, relative to their node:

```rust
Shape2D::square(14.0).transform(Transform2D {
    position: Vector2::new(80.0, 0.0),   // moved from the node, turned with it
    rotation: 0.3,                        // added to the node's rotation
    scale: Vector2::new(1.0, 2.0),        // times the node's scale
    z_index: 1,                           // added to the node's z index
})

Text::new("Enemy").transform(Transform2D { position: Vector2::new(0.0, -40.0), ..Default::default() })
```

How it's placed:
- **Position** is moved from the node, turned with the node's rotation, and scaled by the node's scale.
- **Rotation** is added to the node's.
- **Scale** is multiplied by the node's.
- **z index** is added to the node's. `z_index: 1` puts it one step in front of its node's other visuals, and the default `0` keeps it level with them. Before, a visual's z index *replaced* the node's.

Text on a 3D node ignores this transform. Give it a child node with its own `Transform3D` instead, as before.

### `upright()`

`.upright()` keeps a shape or text level while its node turns. It still moves with the node, and its offset doesn't swing around either. Use it for a name over something that spins:

```rust
Node::new("square")
    .with_property(Shape2D::square(44.0))
    .with_property(Text::new("Stays upright").transform(Transform2D { position: Vector2::new(0.0, -42.0), ..Default::default() }).upright())
```

`Node::follow_position_only()` is still there. It does the same for a whole **child node** (everything on it and its children, in 2D or 3D). `.upright()` is for one visual on a node.

---

## 3. What was removed

| Removed | Use instead |
|---|---|
| `Line::to_node(id)` | A line with `.end_in_world()` and a behaviour that calls `set_end` with the other node's position |
| `Line::to_point(p)`, `to_point_2d(p)` | `Line::new(start, p).end_in_world()`, or `.world()` for both |
| `Line::to_local(p)`, `to_local_2d(p)` | `Line::new(zeros, p)` |
| `LineEnd`, `line.end` field | `start_space` / `end_space`, `start()` / `end()` / `set_start` / `set_end` |
| `Offset2D` | `Transform2D`, through `.transform(..)` |
| `Shape2D::offset(x, y)`, `rotated(r)`, `z_index(z)` | `.transform(Transform2D { .. })` |
| `Text::offset(x, y)`, `rotated(r)`, `z_index(z)` | `.transform(Transform2D { .. })` |
| `shape.offset`, `text.offset`, `z_index: Option<i32>` fields | `transform: Transform2D`, `upright: bool` |

---

## 4. Where the code is

- `src/scene_manager/node/properties/line.rs`: `Line`, `LinePoint` (the trait that lets `Vector2` and `Vector3` both work), `LineSpace`.
- `src/scene_manager/node/properties/transform.rs`: `Transform2D::place_visual`, the one rule for placing a 2D visual on its node.
- `src/scene_manager/node/properties/shape_2d.rs` and `text.rs`: the `transform` field and `.upright()`.
- `src/rendering/node_visuals.rs`: draws them, and prints the mixed-kinds warning.

---

## 5. Checked

- All 66 lib tests pass, including new ones for local and world points, a line from its node to a world point, a 2D line scaling and staying upright, a point of the other kind being ignored, and `place_visual`.
- `text_and_lines` runs, and a screenshot shows the drone's tether reaching the tower, the turret's laser, the fixed blue line, the upright label on the circling square with its line to the dot, the small square going around "Spinning", the spinning 2D line, and the bars in z order.
- Not decided yet: HUD lines inside the UI. The plan is thin UI containers, with rotation for UI nodes later if a HUD needs to roll.
