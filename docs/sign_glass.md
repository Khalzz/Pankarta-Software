# Sign glass: a neon sign seen only through a glass

This is a big pane of glass on two posts in the walking example. Through it you see a cyberpunk neon sign floating just behind the glass. It has a title, a subtitle in Japanese and English, a small "OPEN 24H" box, a neon frame and a scrolling ticker, and it flickers, has scanlines and a moving light sweep. The sign is **only visible through the glass**: outside its edges there's nothing.

It's **game code only**. No engine changes were needed. Run `cargo run --example walking` and walk up behind the glass near the start.

Files (all in `examples/3D/walking/` unless noted):

| File | What it is |
|---|---|
| `sign_glass.wgsl` | The glass's shader. It projects the sign. |
| `sign_glass.ron` | Its settings: see-through, its own pass, and the sign's distance and size. |
| `sign.rs` | `sign_glass(id, position, yaw)` builds the glass and its two posts. |
| `scene.rs` | Puts one sign glass in the scene. The frame plan gets the `"hud"` pass. |
| `main.rs` | Registers the shader from its file (saving it while the game runs reloads it). |
| `assets/textures/signs/neon_sign.png` | The sign's artwork. |

This replaces the earlier HUD glass (reticle and pitch ladder at infinity). Its files are gone.

---

## 1. The idea

The sign isn't an object in the world. It's a **virtual plane** `distance` meters behind the glass, and only the glass's shader knows about it.

For every pixel of the glass, the shader:
1. takes the ray from the eye through that pixel (`engine.camera_position` → `f.world_position`),
2. finds where that ray meets the sign's plane,
3. turns that point into a position on the sign image, and shows that pixel of the image.

Because it follows the real ray, the sign behaves like a real object behind the glass. It stays put as you walk, and you see different parts of it from different angles (parallax). Because only the glass runs this, the glass is the sign's only window.

---

## 2. The artwork

`neon_sign.png` is 1024 × 640 with a transparent background:
- **top 512 rows:** the sign itself (frame, "PANKARTA", "電脳 NOODLE BAR", "ラーメン", "OPEN 24H"),
- **bottom 128 rows:** the ticker line ("» HOT RAMEN // NEURAL LINK CAFE //"), which the shader scrolls sideways (`TICKER_TOP` in the shader).

The glow is part of the image: wide, faint strokes under narrow, bright ones. To change the sign, draw a new image in any editor and save it over this one, keeping the layout (the sign in the top 512 rows, the ticker in the bottom 128, a transparent background).

The material gets the image like any texture: `Material::shader("sign_glass").texture("textures/signs/neon_sign.png")`. The shader reads it as `t_diffuse`.

---

## 3. Effects in the shader

| Effect | How |
|---|---|
| Scrolling ticker | The ticker rows are read with `u + time × 0.08`, wrapped |
| Color split | Red and blue are read slightly left and right of green |
| Hum | Brightness wobbles a little, fast |
| Dropouts | Now and then (random, 12 times a second) the sign dims for a moment |
| Scanlines | Brightness ripples along the sign's height |
| Light sweep | A bright band moves across the sign |

All of these are a few lines each in `sign_glass.wgsl`. `brightness` in the `.ron` scales them all.

---

## 4. Placing the glass

`sign_glass(id, position, yaw)` puts the glass on the ground at `position`, turned `yaw` radians around Y. It works out the glass's middle (`center`) and its axes (`forward`, `up`, `right`) once and gives them to the material as parameters, because the shader isn't told the object's position or rotation. The glass stands still, so they never change.

To put the sign glass on something that moves, update those four parameters every frame from the scene's `after_cameras`, as described in the reticle version. Each change makes the engine redo its material setup that frame, which is fine for one sign.

Settings in `sign_glass.ron`:

| Param | Default | What it does |
|---|---|---|
| `distance` | 1.5 | How far behind the glass the sign floats (m). Further means more parallax, so you see less of it from the side. 0 puts it on the glass. |
| `size` | (4.0, 2.5) | The sign's width and height (m). |
| `glass_tint` | faint blue | The glass's own color and opacity. |
| `brightness` | 1.3 | How bright the neon is. |

Glass size and height are `GLASS_SIZE` and `GLASS_HEIGHT` in `sign.rs` (3.2 × 1.8 m, middle at 1.8 m). The posts and the pane have colliders, so you can't walk through them.

---

## 5. Limits

- The sign is an image, so changing its text means redrawing the image. Live text, like a changing price, isn't possible inside the shader as is. It would need a digit or font texture and code to lay it out.
- It reads the image at full resolution (`textureSampleLevel`, level 0), so from far away the text can shimmer a little.

---

## 6. Checked

- The walking example builds.
- With the player temporarily started just behind the glass, a screenshot showed:
  - the whole sign through the glass, reading correctly (not mirrored),
  - clipped at the glass's edges,
  - with the scanlines and color split visible, the ticker scrolled partway, and the scene behind showing through.

  The start position was put back.
- An earlier try with the sign 6 m back showed only its edge from a slightly off-center position, which is why it's now 1.5 m back and larger.
- Not checked: watching the flicker, sweep and ticker move over time (only still shots), and walking around it with real input.
