// The template every Surface shader is built from (see engine::rendering::
// shader_library): the engine's inputs, the vertex and fragment work, and
// two hooks the shader fills in - pasted where the HOOKS line is. A hook it
// leaves out gets the default: `vertex` changes nothing, `fragment` returns
// the lit color. "engine/standard" is this with both defaults.
//
// Inputs (the model pipelines' bind groups - there are only four):
//   0 - the mesh's texture   1 - the camera
//   2 - the mesh's transform 3 - the light (it carries the time too, and
//                                  the sun's shadow map)

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec4<f32>,
    near: f32,
    far: f32,
    // 1: an orthographic lens (see CameraLens::orthographic), 0: perspective.
    orthographic: f32,
    _padding: f32,
    // Which way the camera looks (xyz, unit length).
    forward: vec4<f32>,
};

struct Transform {
    model_matrix: mat4x4<f32>,
    // The lowest corner of the mesh's geometry, in its model (see
    // MeshUniform).
    bounds_min: vec4<f32>,
};

struct Light {
    position: vec3<f32>,
    color: vec3<f32>,
    time: f32,
    camera_position: vec3<f32>,
    ambient: vec3<f32>,
    // How much of `ambient` reaches a surface facing straight down (see
    // Renderer::set_ambient_ground).
    ambient_ground: f32,
    // Where a camera-relative position falls in each cascade's shadow map;
    // (x) how many cascades there are this frame (0: no shadows), (y) a
    // texel in a map's 0..1 coordinates, (z) how soft the edges are, in
    // texels, (w) the least they fade over, in meters (Shadows::blur); and
    // a texel of each cascade's map in the world.
    shadow_view_proj: array<mat4x4<f32>, 4>,
    shadow_params: vec4<f32>,
    shadow_texels: vec4<f32>,
}

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;

@group(1) @binding(0)
var<uniform> camera: CameraUniform;

@group(2) @binding(0)
var<uniform> transform: Transform;

@group(3) @binding(0)
var<uniform> light: Light;
@group(3) @binding(1)
var shadow_map: texture_depth_2d_array;
@group(3) @binding(2)
var shadow_sampler: sampler_comparison;

// What every hook can read, as `engine`.
struct Engine {
    // Seconds, counting up (back to 0 every 10000 s).
    time: f32,
    // The camera's true position in the world - every position a hook gets
    // is relative to it (add it back for the true world position).
    camera_position: vec3<f32>,
    // Which way the sun is, and its light's color.
    sun_direction: vec3<f32>,
    sun_color: vec3<f32>,
    // The light from the whole sky, falling on every surface whichever
    // way it faces.
    ambient_color: vec3<f32>,
    // The camera's near and far planes - what `linear_depth` uses.
    near: f32,
    far: f32,
    // Which way the camera looks (unit length), and whether its lens is
    // orthographic - every pixel seen from that same way, not from a point
    // (see `view_direction`).
    camera_forward: vec3<f32>,
    orthographic: bool,
}
var<private> engine: Engine;

fn engine_values() -> Engine {
    return Engine(light.time, light.camera_position, normalize(light.position), light.color, light.ambient, camera.near, camera.far, camera.forward.xyz, camera.orthographic > 0.5);
}

// How far from the camera (world units) a raw depth is - one read from the
// depth so far (`reads: [Depth]`), or a point's own (its clip-space z / w).
// Depths here are reversed: 1 at the near plane, 0 at the far one.
fn linear_depth(raw: f32) -> f32 {
    if (camera.orthographic > 0.5) {
        // An orthographic lens spreads depth evenly from near to far.
        return camera.near + (1.0 - raw) * (camera.far - camera.near);
    }
    let z = 1.0 - 2.0 * raw;
    return 2.0 * camera.near * camera.far / (camera.far + camera.near - z * (camera.far - camera.near));
}

// The way a point (camera-relative, like every hook's `position`) is seen
// from: from the camera toward it (unit length). With an orthographic lens
// every point is seen the same way - the way the camera looks. Turned
// around (`-view_direction(p)`) it's toward the camera: for reflections,
// glints and fresnel.
fn view_direction(position: vec3<f32>) -> vec3<f32> {
    if (camera.orthographic > 0.5) {
        return camera.forward.xyz;
    }
    return normalize(position);
}

// The `vertex` hook's vertex: where it is (relative to the camera), its
// normal and its texture coordinates.
struct VertexIn {
    position: vec3<f32>,
    normal: vec3<f32>,
    uv: vec2<f32>,
}

// The `fragment` hook's pixel.
struct FragmentIn {
    // Where it is: relative to the camera, and in the world.
    position: vec3<f32>,
    world_position: vec3<f32>,
    // The surface's normal (unit length), facing the side that's seen.
    normal: vec3<f32>,
    uv: vec2<f32>,
    // The mesh's own texture here, and the color engine/standard draws -
    // lit by the sun, fogged far away.
    base_color: vec4<f32>,
    lit_color: vec4<f32>,
    front_facing: bool,
    // Where it is on its object, and the way its surface faces there - in
    // the object's own space, scaled like it: a point 1 m up a cube scaled
    // to 4 m tall is at y = 1 - turning or moving the object changes
    // neither.
    local_position: vec3<f32>,
    local_normal: vec3<f32>,
    // The lowest corner of the mesh's geometry, in the same space as
    // `local_position` - `local_position - local_bounds_min` measures from it
    // (0 at that corner, the mesh's size at the opposite one).
    local_bounds_min: vec3<f32>,
    // The light falling here (the sun and the ambient light) - times a
    // color, that color lit the way `lit_color` is (before far fog).
    lighting: vec3<f32>,
    // How much of the sun reaches here: 1 in full sun, 0 in shadow (or
    // facing away from it) - in between at a shadow's soft edge. 1 without
    // shadows, or on a node they don't fall on (Model::receives_shadows).
    shadow: f32,
}

// A number in 0..1 that looks random from one `p` to the next (interleaved
// gradient noise) - here, per shadow-map texel, so it stays put on surfaces.
fn shadow_noise(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// Where a point the sun's view `clip` puts falls in its map (0..1).
fn shadow_uv(clip: vec4<f32>) -> vec2<f32> {
    return vec2<f32>(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
}

// One read of cascade `cascade`'s map at `uv` + `offset`: 1 lit, 0 shadowed
// (blended over the 2x2 texels around it). The depth compared is the
// surface's own there - `depth` at `uv`, tilted along the surface by
// `tilt` (depth per map unit) - so a slope isn't taken for what's in its
// way; at most `max_tilt` either way, for a surface the sun sees edge-on.
fn shadow_read(uv: vec2<f32>, offset: vec2<f32>, depth: f32, tilt: vec2<f32>, max_tilt: f32, cascade: i32) -> f32 {
    let along = clamp(dot(tilt, offset), -max_tilt, max_tilt);
    return textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, cascade, depth + along);
}

// How much of the sun reaches `position` (camera-relative) on a surface
// facing `normal` in cascade `cascade`'s map - 1 lit, 0 shadowed: what the
// sun sees there is compared with how far the point is from it, at a few
// texels around (softer edges the farther apart). And how near the map's
// edge it is (0 at its middle, 1 at its edge) - or -1: not in it.
// `normal` is the triangle's own (flat) one, not the smoothed normal it's
// lit with: the depth compared follows the actual surface, and on low-poly
// geometry a smoothed normal strays from it enough to shade itself.
fn cascade_shadow(position: vec3<f32>, normal: vec3<f32>, cascade: i32) -> vec2<f32> {
    let view_proj = light.shadow_view_proj[cascade];
    let texel_world = light.shadow_texels[cascade];
    let texel_uv = light.shadow_params.y;
    // How far either way the edge fades, in the world: `softness` texels, or
    // `blur` meters if that's wider - so a fine map still fades its edges.
    let spread_world = max(light.shadow_params.z * texel_world, light.shadow_params.w);
    let spread_texels = spread_world / texel_world;
    let spread = spread_texels * texel_uv;
    // Far enough inside the map that every texel read is in it.
    let margin = spread + texel_uv;

    // Kept from shading itself (acne - stripes, rings and speckles where the
    // map's depth and the surface's swap back and forth), without moving it
    // past what's right over it (that leaks light around shadows' edges):
    // - lifted off the surface by 1.5 texels, never less than 3 mm (on a
    //   fine map, a texel is less than how far smooth normals stray from a
    //   low-poly surface), and toward the sun by a texel per how edge-on it
    //   is (each read's own 2x2 texels);
    // - each read around the point compares the surface's depth where that
    //   read lands, not the point's (see shadow_read): the tilt of its plane
    //   as the sun sees it, from two steps along the surface.
    let sun = normalize(light.position);
    let facing = clamp(dot(normal, sun), 0.05, 1.0);
    let slope = min(sqrt(1.0 - facing * facing) / facing, 4.0);
    let lifted = position + normal * max(texel_world * 1.5, 0.003) + sun * texel_world * slope;
    let clip = view_proj * vec4<f32>(lifted, 1.0);
    let uv = shadow_uv(clip);
    if (any(uv < vec2<f32>(margin)) || any(uv > vec2<f32>(1.0 - margin)) || clip.z > 1.0) {
        return vec2<f32>(1.0, -1.0);
    }

    // Two steps along the surface (any two at right angles), and where each
    // moves it in the map and in depth - the plane's tilt solves
    // depth = tilt . uv for both.
    let other = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(normal.y) > 0.9);
    let along_a = normalize(cross(normal, other));
    let along_b = cross(normal, along_a);
    let clip_a = view_proj * vec4<f32>(lifted + along_a, 1.0);
    let clip_b = view_proj * vec4<f32>(lifted + along_b, 1.0);
    let step_a = vec3<f32>(shadow_uv(clip_a) - uv, clip_a.z - clip.z);
    let step_b = vec3<f32>(shadow_uv(clip_b) - uv, clip_b.z - clip.z);
    let determinant = step_a.x * step_b.y - step_a.y * step_b.x;
    var tilt = vec2<f32>(0.0);
    if (abs(determinant) > 1e-12) {
        tilt = vec2<f32>(step_a.z * step_b.y - step_b.z * step_a.y, step_b.z * step_a.x - step_a.z * step_b.x) / determinant;
    }
    // A slope of at most 4 (as `slope`) across the spread: how much depth
    // that is, from a step toward the sun.
    let depth_per_meter = abs((view_proj * vec4<f32>(lifted + sun, 1.0)).z - clip.z);
    let max_tilt = depth_per_meter * (spread_world + texel_world) * 4.0;

    // Each read already blends the 2x2 texels around it (the sampler is
    // filtered) - so up to 1.5 texels, 4 reads half a spread apart cover
    // the ground without gaps. Wider, 16 spread over a disc, turned a
    // different way each texel so they don't show as copies of the edge -
    // a smooth fade instead.
    var lit = 0.0;
    if (spread_texels > 1.5) {
        var disc = array<vec2<f32>, 16>(
            vec2<f32>(-0.9420, -0.3991), vec2<f32>(0.9456, -0.7689), vec2<f32>(-0.0942, -0.9294), vec2<f32>(0.3450, 0.2939),
            vec2<f32>(-0.9159, 0.4577), vec2<f32>(-0.8154, -0.8791), vec2<f32>(-0.3828, 0.2768), vec2<f32>(0.9748, 0.7565),
            vec2<f32>(0.4432, -0.9751), vec2<f32>(0.5374, -0.4737), vec2<f32>(-0.2650, -0.4189), vec2<f32>(0.7920, 0.1909),
            vec2<f32>(-0.2419, 0.9971), vec2<f32>(-0.8141, 0.9144), vec2<f32>(0.1998, 0.7864), vec2<f32>(0.1438, -0.1410),
        );
        let angle = shadow_noise(floor(uv / texel_uv)) * 6.2831853;
        let turn = mat2x2<f32>(cos(angle), sin(angle), -sin(angle), cos(angle));
        for (var i = 0; i < 16; i++) {
            lit += shadow_read(uv, turn * disc[i] * spread, clip.z, tilt, max_tilt, cascade);
        }
        lit /= 16.0;
    } else {
        let half_spread = spread * 0.5;
        lit += shadow_read(uv, vec2<f32>(-half_spread, -half_spread), clip.z, tilt, max_tilt, cascade);
        lit += shadow_read(uv, vec2<f32>(half_spread, -half_spread), clip.z, tilt, max_tilt, cascade);
        lit += shadow_read(uv, vec2<f32>(-half_spread, half_spread), clip.z, tilt, max_tilt, cascade);
        lit += shadow_read(uv, vec2<f32>(half_spread, half_spread), clip.z, tilt, max_tilt, cascade);
        lit /= 4.0;
    }
    let edge = max(abs(uv.x - 0.5), abs(uv.y - 0.5)) * 2.0;
    return vec2<f32>(lit, edge);
}

// How much of the sun reaches `position` (camera-relative) on a surface
// facing `normal` - 1 lit, 0 shadowed - from the smallest (sharpest) cascade
// it's in, blended into the next one toward its edge so where one hands
// over isn't a line; faded out toward the edge of the last one. Outside
// every one, lit.
// `normal`: the triangle's own (flat) normal - see cascade_shadow.
fn sun_shadow(position: vec3<f32>, normal: vec3<f32>) -> f32 {
    let cascades = i32(light.shadow_params.x);
    for (var cascade = 0; cascade < cascades; cascade++) {
        let here = cascade_shadow(position, normal, cascade);
        if (here.y < 0.0) {
            continue;
        }
        let blend = smoothstep(0.8, 1.0, here.y);
        if (blend == 0.0) {
            return here.x;
        }
        // The next one out - or, after the last, full sun.
        var next = 1.0;
        if (cascade + 1 < cascades) {
            let out = cascade_shadow(position, normal, cascade + 1);
            next = select(out.x, 1.0, out.y < 0.0);
        }
        return mix(here.x, next, blend);
    }
    return 1.0;
}

//@HOOKS

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) normal: vec3<f32>,
}

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,

    // Lighting rotation
    @location(9) normal_matrix_0: vec3<f32>,
    @location(10) normal_matrix_1: vec3<f32>,
    @location(11) normal_matrix_2: vec3<f32>,

    // 1 if shadows fall on it, 0 if not (its node's Model::receives_shadows).
    @location(12) receives_shadows: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) world_position: vec3<f32>,
    @location(3) view_depth: f32, // Used for fog calculation
    @location(4) local_position: vec3<f32>,
    @location(5) local_normal: vec3<f32>,
    @location(6) local_bounds_min: vec3<f32>,
    @location(7) receives_shadows: f32,
};

@vertex
fn vs_main(model: VertexInput, instance: InstanceInput) -> VertexOutput {
    engine = engine_values();
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    let normal_matrix = mat3x3<f32>(
        instance.normal_matrix_0,
        instance.normal_matrix_1,
        instance.normal_matrix_2,
    );

    let world_position = model_matrix * transform.model_matrix * vec4<f32>(model.position, 1.0);
    let v = vertex(VertexIn(world_position.xyz, normal_matrix * model.normal, model.tex_coords));

    var out: VertexOutput;
    out.tex_coords = v.uv;
    out.world_normal = v.normal;
    out.world_position = v.position;
    out.clip_position = camera.view_proj * vec4<f32>(v.position, 1.0);

    // How far away it is, for fog: from the camera - or, with an
    // orthographic lens, along the way it looks (its position isn't where
    // the view starts from).
    out.view_depth = length(camera.view_pos.xyz - out.world_position);
    if (camera.orthographic > 0.5) {
        out.view_depth = max(dot(out.world_position, camera.forward.xyz), 0.0);
    }

    // Back into the object's own (scaled) space: the instance's matrix is
    // place x turn x size, and `normal_matrix` its turn alone - so undoing
    // the place and the turn leaves the size in.
    let unturn = transpose(normal_matrix);
    out.local_position = unturn * (v.position - model_matrix[3].xyz);
    out.local_normal = unturn * v.normal;
    // The corner sized like the object: its size is the length of each of
    // its matrix's first three columns.
    let size = vec3<f32>(length(model_matrix[0].xyz), length(model_matrix[1].xyz), length(model_matrix[2].xyz));
    out.local_bounds_min = size * transform.bounds_min.xyz;
    out.receives_shadows = instance.receives_shadows;

    return out;
}

@fragment
fn fs_main(in: VertexOutput, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    engine = engine_values();
    let object_color: vec4<f32> = textureSample(t_diffuse, s_diffuse, in.tex_coords);

    // For double-sided meshes (backface culling off) the back face is
    // rasterized with the same interpolated normal as the front - flip it so
    // it's lit as the surface it actually is. Single-sided meshes only ever
    // hit the front face, so this is a no-op for them.
    var normal = normalize(in.world_normal);
    if (!front_facing) {
        normal = -normal;
    }
    // The triangle's own flat normal (from how the position changes across
    // neighbouring pixels - here, before any branch, where that's defined),
    // turned to the side `normal` is on - what the shadow is read with (see
    // cascade_shadow). The smoothed one, if it's degenerate.
    let face_cross = cross(dpdx(in.world_position), dpdy(in.world_position));
    var face_normal = normal;
    if (dot(face_cross, face_cross) > 1e-20) {
        face_normal = normalize(face_cross);
        if (dot(face_normal, normal) < 0.0) {
            face_normal = -face_normal;
        }
    }

    let light_dir = normalize(light.position - in.world_position);

    let diffuse_strength = max(dot(normal, light_dir), 0.0);
    var shadow = 1.0;
    if (in.receives_shadows > 0.5) {
        // Facing away from the sun it doesn't reach - no map to read.
        shadow = 0.0;
        if (diffuse_strength > 0.0) {
            shadow = sun_shadow(in.world_position, face_normal);
        }
    }
    let diffuse_color = light.color * diffuse_strength * shadow;

    // The sky is brighter than the ground: a surface gets all the ambient
    // light facing up, `ambient_ground` of it facing down, blended between.
    // Undersides and downward faces - under a dashboard, in a footwell -
    // read darker than tops instead of all the same flat grey.
    let ambient = light.ambient * mix(light.ambient_ground, 1.0, normal.y * 0.5 + 0.5);
    let lighting = ambient + diffuse_color;
    let result = lighting * object_color.xyz;

    // Long-distance atmospheric haze - same start/end as water.wgsl's fog,
    // fading toward the sky's horizon haze (SkyboxRender::new_procedural's
    // haze_color), since distant objects are seen against the sky.
    let fog_start = 80000.0;
    let fog_end = 1400000.0;
    let fog_factor = smoothstep(fog_start, fog_end, in.view_depth);
    let fogged_color = mix(result, vec3<f32>(0.448, 0.604, 0.711), fog_factor);
    let lit = vec4<f32>(fogged_color, object_color.a);

    return fragment(FragmentIn(in.world_position, in.world_position + engine.camera_position, normal, in.tex_coords, object_color, lit, front_facing, in.local_position, normalize(in.local_normal), in.local_bounds_min, lighting, shadow));
}
