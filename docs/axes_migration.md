# Migrating to Godot-style axes (-Z forward)

Pankarta now uses Godot's axis convention: **right-handed, +Y up, -Z forward, +X right**. Before this change, a camera looked along its node's **+Z**. In a right-handed world that put **+X on the left of the screen**, which is the bug this change fixes.

The world itself did not change: positions, meshes, models, physics, text and particles are all exactly as they were. What changed is **which local axis counts as "forward"** for cameras, `LookAt`, directional lights and `facing()`.

## What changed in the engine

| Thing | Before | Now |
|---|---|---|
| Camera node (`CameraLens`) | looks along its node's **+Z** | looks along its node's **-Z** |
| `LookAt { target }` | turns the node's **+Z** toward the target | turns the node's **-Z** toward the target |
| `Light::directional` | shines along its node's **+Z** | shines along its node's **-Z** |
| `scene_nodes::facing(dir)` / `Transform3D::facing(dir)` | points **+Z** along `dir` | points **-Z** along `dir` |
| Models (glTF) | front is +Z | front is still **+Z**, unchanged (same as Godot) |
| Particle emitters | `Motion::direction` / shapes measured from +Z | unchanged |
| 3D `Text` | read from the node's +Z side | unchanged |
| Internal `Camera` yaw (`yaw_pitch_rotation`, 0 = +X, 90 = +Z) | | unchanged |

New helpers:

- `Transform3D::forward()` returns `rotation * -Z`.
- `back()` returns `+Z`. On a model, this is its nose.
- `right()` returns `+X`, and `up()` returns `+Y`.
- `scene_nodes::model_facing(dir)` / `Transform3D::model_facing(dir)` point **+Z** along `dir`. This is the old `facing` behaviour, for models (Godot's `use_model_front`).

## What stays correct without any change

- **Anything that only uses `Transform3D::facing(dir)` or `facing(dir)` for a camera or a light.** For example `Transform3D::facing(sun_dir)` on a `Light`, or `facing(forward)` used to orient a camera node. Both sides flipped together, so the result looks the same.
- **Cameras with `LookAt`.** They end up looking at the same thing with the same up vector, so the rendered image is identical.
- **Anything about models**, like a nose computed as `rotation * Vector3::z()` on an aircraft or missile, HUD boresights from a model's rotation, or `rotation_between(&Vector3::z(), ..)` for a missile's attitude. A model's front is still +Z.
- **Screen-space math built from a world look direction and an up vector**, like `look.cross(&up)` for a screen right. It's pure geometry, so the convention doesn't affect it.

## What breaks and how to fix it

### 1. `facing()` used to orient a **model** (aircraft, missile, any glTF)
`facing(dir)` now points the node's **back** (+Z is the nose, -Z is the tail) along `dir`, so the model flies tail-first.

**Fix:** use `model_facing` instead.
```rust
// before
world.rotation = facing(heading).unwrap_or(world.rotation);
// after
use pankarta_software::scene_manager::scene_nodes::model_facing;
world.rotation = model_facing(heading).unwrap_or(world.rotation);
```

### 2. A camera node's rotation built by hand (yaw/pitch quaternions, cockpit, free look, chase)
With an identity rotation, the camera used to look along +Z. Now it looks along -Z, so every hand-built camera rotation now looks **backwards**.

**Fix:** either work in the new convention, or put a half turn about Y in front of the old rotation:
```rust
let half_turn = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), std::f32::consts::PI);
transform.rotation = old_rotation * half_turn;   // exactly the old view
```
If you rebuild the rotation from yaw/pitch in the new convention instead:
- **Yaw** about +Y keeps its sign: a positive angle turns left in both conventions.
- **Pitch** about local +X **flips sign**. Before, a positive pitch looked down. Now it looks up. So `pitch += mouse_y` becomes `pitch -= mouse_y`, and asymmetric clamps swap ends (`clamp(-1.3, 1.4)` becomes `clamp(-1.4, 1.3)`).
- A camera riding a model (a cockpit view) needs the half turn, because the model's front is +Z: `plane.rotation * half_turn * yaw_q * pitch_q`.

### 3. Reading a camera's look direction from its node as `rotation * Vector3::z()`
**Fix:** use `transform.forward()` (`rotation * -Vector3::z()`). Likewise, a camera's screen-right is now `rotation * Vector3::x()` (`transform.right()`), where before it was `rotation * -Vector3::x()`.

### 4. A directional light with a **hand-written rotation** (not `Transform3D::facing`)
It now shines the opposite way. **Fix:** build it with `Transform3D::facing(direction)`, or apply the same half turn about Y.

### 5. Saved data: camera or light rotations stored as numbers (RON, map files, camera keys, editor saves)
A stored rotation is a hand-built rotation (case 2), so any camera key **without** a `look_at` now faces backwards. Keys with a `look_at` are fine, because they go through `facing()`.

**Fix:** migrate the stored rotation once: `new = old * half_turn` (a half turn about the node's own Y). Alternatively, version the file and apply the half turn when loading old files. Model and object rotations in saved data do **not** change.

### 6. Debug drawing of a camera's facing
An editor or gizmo that draws a camera's view direction as `rotation * Vector3::z()` now draws it backwards. Use `-Z` / `forward()` for cameras and lights. Keep `+Z` for models.

### 7. `yaw_pitch_rotation(...) * Vector3::x()` turned into a camera rotation via a helper
The internal yaw convention is unchanged, so the **forward vector** this produces is still correct. What matters is how it becomes a node rotation:
- through `facing(forward)` / `Transform3D::facing`: still correct.
- through `face_towards(&forward, ..)` or another "+Z along this" helper: now faces backwards. Use `facing(forward)` or `face_towards(&-forward, &up)`.

## Places in Project Skies to check

This is from a quick grep. Each one still needs to be classified as camera/light (needs a fix) or model (usually fine).

| File | What to look at | Likely case |
|---|---|---|
| `src/scenes/play/path_follow.rs` (~401-406) | `facing(heading)` sets an aircraft's rotation | **1: use `model_facing`** |
| `src/scenes/play/animations.rs` (~81, ~130, ~157) | camera keys: `rotation_from_degrees` key rotations, objective as `pose.rotation * Vector3::z()`, `facing(...)` for look-at | 2/3/5: `facing` for the camera is fine; `rotation * z()` as the camera look direction becomes `forward()`; stored key rotations need the half turn |
| `src/prefabs/camera/behavior.rs` (~583, ~602, ~754, ~853, ~885, ~923-953, ~983, ~1098) | cockpit/free/track/orbit rotations built from yaw/pitch, `cockpit_current_rotation * Vector3::z()` | 2/3. `target.rotation * Vector3::z()` on the **aircraft** stays as it is. `facing(direction)` at ~1098 is fine for a camera |
| `src/prefabs/camera/control.rs` (~361, ~467) | `rotation * Vector3::z()` as a camera forward; `facing(forward)` | 3 at ~361 if it's a camera's rotation. ~467 is fine |
| `src/scenes/main_menu/scene.rs` (~174, ~257) | `camera_transform` uses `yaw_pitch_rotation * x()` then `looking_along(forward)` | 7: check what `looking_along` does |
| `src/scenes/play/editor/gizmos.rs` (~82) | draws `rotation * Vector3::z()` as a node's blue axis | 6: fine as a plain axis gizmo. If it's meant as "where the camera looks", use -Z for cameras |
| `src/scenes/play/editor.rs`, `editor/flight_editor*.rs`, timeline/hierarchy | authored camera rotations (`rotation_from_degrees`) | 5: migrate saved camera rotations |
| `src/scenes/play/tactical.rs` (~78), `tactical_map.rs` | `set_editor_view(.., Vector3::z())`, billboard right/up | check whether `Vector3::z()` is a camera look direction |
| `src/scenes/sandbox/scene.rs`, `follow_test/scene.rs` | `Transform3D::facing(...)` on a camera and a light | fine |
| `src/scenes/play/hud.rs`, `weapon/fire_control.rs`, `prefabs/missile/flight.rs` | model noses as `rotation * Vector3::z()`, `rotation_between(&Vector3::z(), ..)` | fine (models) |

## Quick check after migrating

- A camera with no rotation shows +X on the right, +Y up, and +Z coming toward you.
- Every camera view (menu shot, chase, cockpit, free, cinematic keys, editor) shows what it showed before.
- Aircraft and missiles fly nose-first.
- The sun lights the same side of things as before.
- Mouse up looks up and mouse right turns right in every camera.
