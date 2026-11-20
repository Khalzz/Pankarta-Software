# Gizmos

Gizmos are lines, icons and text drawn over the world **for debugging**: a force as an arrow, a center of mass as an icon, a hit as a cross that stays up for two seconds. You draw them from anywhere that has the app, which means any behavior or scene.

The example is `examples/3D/gizmos.rs`. Run it with `cargo run --example gizmos`.

Gizmos are not for game content. A name over a player belongs on the player's node (a `Text` with `.face_camera()`): it follows the node, goes away with it, and stays visible when debug drawing is switched off.

---

## 1. Drawing

Gizmos are drawn in a **channel**, from a closure:

```rust
app.gizmos.channel("aero", |g| {
    g.icon(self.center_of_mass, Icon::Mass).on_top().label("CoM");
    g.ray(self.center_of_lift, self.lift * 0.001).color(GizmoColor::CYAN).arrow().label("lift");
    g.outline(&self.wing_corners).dashed();
});
```

A gizmo is up **for this frame only**. Draw it every frame (from `update`) for as long as you want to see it. What's on screen is always whatever was drawn this frame, so nothing needs to be removed.

The arguments say **what and where**. Everything else is an optional setting chained after it.

### Lines and shapes

| Call | Draws |
|---|---|
| `g.line(a, b)` | a line from `a` to `b` |
| `g.ray(origin, vector)` | a line from `origin` along `vector`, such as a force from where it's applied |
| `g.path(&points)` | lines through the points, in order |
| `g.outline(&points)` | the same, closed back to the first point |
| `g.cross(center, size)` | three lines across each other, `size` long |
| `g.circle(center, normal, radius)` | a circle square to `normal` |
| `g.sphere(center, radius)` | three circles |
| `g.cube(pose, half_extents)` | a box's 12 edges |
| `g.axes(pose, length)` | local axes: X red, Y green, Z blue |

Points can be a `Vector3`, a `Point3` (straight from Rapier) or `[x, y, z]`. A `pose` is a position, a `Transform3D`, an `Isometry3` (Rapier's body poses) or `(position, rotation)`.

Line settings:

| Setting | Does |
|---|---|
| `.color(c)` | its color |
| `.colors(start, end)` | fades from one color to the other along it |
| `.width(px)` | width on screen, in pixels (default 2) |
| `.arrow()` | an arrowhead at the end (lines, rays, paths and axes) |
| `.dashed()` | dashed instead of solid |
| `.on_top()` | drawn through everything |
| `.label("text")` | text at a line's end, a path's last point, or a shape's middle |
| `.duration(seconds)` | stays up that long (see section 3) |

### Icons

```rust
g.icon(position, Icon::Fire).size(24.0).color(GizmoColor::ORANGE).label("thrust");
```

An icon faces the camera and stays the same size on screen at any distance. It has a dark outline, so it reads on any background. Settings: `.size(px)` (default 20), `.color(c)`, `.on_top()`, `.label("text")` (shown above it) and `.duration(seconds)`.

### Text

```rust
g.text(position, format!("{:.0} N", lift.norm())).size(16.0);
```

Text is centered on the point and always drawn on top. Settings: `.size(px)` (default 14), `.color(c)` and `.duration(seconds)`.

### Colors

`GizmoColor::WHITE`, `RED`, `GREEN`, `BLUE`, `YELLOW`, `ORANGE`, `CYAN`, `MAGENTA`, `GRAY` and `BLACK`, or any `[r, g, b]`, `[r, g, b, a]` (0..1) or `UiColor`. `GizmoColor::RED.alpha(0.5)` makes a color half see-through.

---

## 2. What hides a gizmo

By default the world hides a gizmo, the way it hides anything else. The hidden part is still drawn, **faint**, so you can find it behind a wall.

`.on_top()` draws a gizmo through everything at full strength. Use it for points **inside** a mesh, such as a center of mass inside a plane's body. Otherwise the body itself always hides the point, and it always shows faint.

Text and labels are always on top.

---

## 3. Channels

A channel groups gizmos so they can be switched on and off together:

```rust
app.gizmos.set_enabled("aero", false);  // off
app.gizmos.toggle("aero");              // the other way; returns whether it's on now
app.gizmos.is_enabled("aero");
app.gizmos.visible = false;             // every channel off; the default is true
app.gizmos.channels();                  // every channel and whether it's on, e.g. for a debug window
```

While a channel is off, **its closure doesn't run**. Work that's only there to draw gizmos, like sampling 50 points along a wing, can go inside it at no cost while the channel is off.

A channel is on from the first time it's drawn in. A channel switched off before it's ever drawn in stays off.

Each channel has a style: the defaults for whatever its gizmos don't set themselves.

```rust
app.gizmos.style("aero").color(GizmoColor::CYAN).width(3.0).icon_size(24.0).text_size(16.0);
```

The closure borrows `app.gizmos`, so the rest of `app` can't be used inside it. Read what you need (a body's velocity, say) before the call.

### Drawing from `fixed_update`: `.duration`

`fixed_update` runs once per physics step, which can be zero, one or several times a frame. A gizmo drawn there with no duration only shows on frames that ran a step, so it flickers. There are two fixes:

- keep the latest values on the behavior and draw them from `update`;
- or give the gizmo a duration, which keeps it up that long whatever happens to the frames:

```rust
fn fixed_update(&mut self, node: &mut Node, scene: &mut SceneAccess, app: &mut App) {
    if let Some(hit) = self.hit_this_step {
        app.gizmos.channel("hits", |g| {
            g.cross(hit, 0.5).color(GizmoColor::RED).duration(2.0);
        });
    }
}
```

A duration is in game time, so with the game paused (`time_scale` 0) the gizmo stays up. `app.gizmos.clear()` takes every gizmo down, including ones with time left. Opening another scene does that too.

---

## 4. Icons

### The engine's

| | | | |
|---|---|---|---|
| **Markers** | `Dot`, `Ring`, `Cross`, `Target` | `Diamond`, `Star`, `Pin`, `Flag` | |
| **Things** | `Fire`, `Ball`, `Bolt` | `Drop`, `Wind`, `Gear` | |
| **Status** | `Warning`, `Info` | | |
| **Physics** | `Mass`: the checkered circle for a center of mass | | |

`Icon::BUILT_IN` lists them all, and `icon.name()` gives the name (`"fire"`).

### Your own

Any SVG works: one you drew, or one downloaded from an icon set (Lucide's SVGs, for example). Load it under a name, then draw it by that name:

```rust
app.gizmos.icons.load("wing", "icons/wing.svg")?;              // a file in the assets folder
app.gizmos.icons.load_svg("wing", include_str!("wing.svg"))?;  // or built into the game

g.icon(position, Icon::named("wing"));
```

An icon is **one color**. Only the shape the SVG covers matters, not its colors, and `.color()` sets the color it's drawn in. Fills and strokes both work. The SVG is fitted into a square, so square artwork works best (24 × 24 is typical).

Each icon is drawn the first time it's used (64 × 64, turned into a distance field), so it stays sharp at any size on screen. Loading a name again replaces it. A name that was never loaded is reported once and not drawn.

---

## 5. Where they're drawn

Gizmos are drawn once a frame, at the frame plan's **first UI step**: over the world, the 3D nodes' text and lines, and the 2D nodes, and under the UI. A frame plan with no UI step draws no gizmos.

Drawn into one of the game's own images (a `.target(...)` step), there's no depth of the right size, so every gizmo there is drawn on top.

They follow the floating origin like everything else in the world.

The physics debug view (`app.renderer.debug.physics`) and `app.physics.debug_lines()` are separate and work as before.
