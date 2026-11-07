// The sign glass - a Surface shader's fragment hook, settings in
// sign_glass.ron. See docs/sign_glass.md.
//
// A neon sign floats `distance` meters behind the glass, seen only through
// it. Each pixel follows the eye's ray through the glass to the sign's
// plane, and shows what the sign has there - so the sign stays put in the
// world as you walk around, and the glass is its only window.

struct Params {
    // The glass's middle and axes in the world.
    center: vec3<f32>,
    forward: vec3<f32>,
    up: vec3<f32>,
    right: vec3<f32>,
    // How far behind the glass the sign floats, and its size (m).
    distance: f32,
    size: vec2<f32>,
    glass_tint: vec4<f32>,
    brightness: f32,
}

// The image's rows: the sign above, the ticker strip below (see
// assets/textures/signs/neon_sign.png).
const TICKER_TOP: f32 = 0.8;

fn hash(value: f32) -> f32 {
    return fract(sin(value * 91.345) * 47453.5453);
}

// The image at `uv` - the ticker strip's part scrolling sideways.
fn sign_image(uv: vec2<f32>, time: f32) -> vec4<f32> {
    let in_ticker = uv.y > TICKER_TOP;
    let scrolled = vec2<f32>(fract(uv.x + time * 0.08), uv.y);
    let at = select(uv, scrolled, in_ticker);
    return textureSampleLevel(t_diffuse, s_diffuse, at, 0.0);
}

fn fragment(f: FragmentIn) -> vec4<f32> {
    let time = engine.time;
    let eye = engine.camera_position;
    let ray = normalize(f.world_position - eye);

    // Where the ray meets the sign's plane, in meters across and up from
    // the sign's middle.
    let sign_middle = params.center + params.forward * params.distance;
    let facing = dot(ray, params.forward);
    let along = dot(sign_middle - eye, params.forward) / max(facing, 1e-4);
    let hit = eye + ray * along - sign_middle;
    let uv = vec2<f32>(0.5 + dot(hit, params.right) / params.size.x, 0.5 - dot(hit, params.up) / params.size.y);
    let on_sign = step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0) * step(1e-4, facing);

    // A little color split, like a cheap projector.
    let split = vec2<f32>(0.0025, 0.0);
    let base = sign_image(uv, time);
    let red = sign_image(uv + split, time).r;
    let blue = sign_image(uv - split, time).b;
    var sign = vec4<f32>(red, base.g, blue, base.a);

    // Flicker: a steady hum, and now and then a dropout.
    let tick = floor(time * 12.0);
    let dropout = select(1.0, 0.35, hash(tick) > 0.96);
    let hum = 0.92 + 0.08 * sin(time * 47.0);
    // Scanlines across the sign, and a bright band sweeping over it.
    let scan = 0.82 + 0.18 * sin(uv.y * 420.0 + time * 6.0);
    let sweep = 1.0 + 0.6 * exp(-pow((uv.x - fract(time * 0.2) * 1.6 + 0.3) * 6.0, 2.0));
    let light = params.brightness * dropout * hum * scan * sweep;

    let shown = sign.a * on_sign;
    let color = mix(params.glass_tint.rgb, sign.rgb * light, shown);
    return vec4<f32>(color, max(params.glass_tint.a, min(shown * light, 1.0)));
}
