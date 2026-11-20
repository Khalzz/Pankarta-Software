// The gizmos' icons (see rendering::gizmos) - one instance per icon, a
// square facing the camera, `size` pixels on screen at any distance, cut
// out of its distance field in the icon atlas (see gizmos/icons.rs) with a
// dark outline round it so it reads on any background. Drawn the three ways
// the lines are (see gizmo_lines.wgsl).

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec4<f32>,
};

struct Gizmo {
    camera_position: vec4<f32>,
    screen: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(1) @binding(0)
var<uniform> gizmo: Gizmo;
@group(1) @binding(1)
var t_atlas: texture_2d<f32>;
@group(1) @binding(2)
var s_atlas: sampler;

struct IconInstance {
    @location(0) position: vec3<f32>,
    // The square's side on screen (px) - the icon's field, margin included.
    @location(1) size: f32,
    @location(2) color: vec4<f32>,
    // Where its field is in the atlas: u0, v0, u1, v1.
    @location(3) uv: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
};

const HIDDEN_ALPHA: f32 = 0.3;
// The outline round an icon (px), and how dark it is.
const OUTLINE_PIXELS: f32 = 1.5;
const OUTLINE_ALPHA: f32 = 0.65;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, icon: IconInstance) -> VertexOutput {
    var output: VertexOutput;
    let center = camera.view_proj * vec4<f32>(icon.position - gizmo.camera_position.xyz, 1.0);
    if (center.w < 0.0001) {
        output.position = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        return output;
    }
    var xs = array<f32, 6>(-1.0, 1.0, -1.0, -1.0, 1.0, 1.0);
    var ys = array<f32, 6>(-1.0, -1.0, 1.0, 1.0, -1.0, 1.0);
    let corner = vec2<f32>(xs[vertex_index], ys[vertex_index]);
    // Half the square in pixels, as clip space at the icon's depth.
    let offset = corner * icon.size / gizmo.screen.xy * center.w;
    output.position = vec4<f32>(center.xy + offset, center.z, center.w);
    output.uv = vec2<f32>(mix(icon.uv.x, icon.uv.z, corner.x * 0.5 + 0.5), mix(icon.uv.w, icon.uv.y, corner.y * 0.5 + 0.5));
    output.color = icon.color;
    return output;
}

fn shade(input: VertexOutput) -> vec4<f32> {
    // The field is 0.5 on the icon's edge, more inside - its change over a
    // pixel keeps the edge one pixel soft at any size.
    let distance = textureSample(t_atlas, s_atlas, input.uv).r;
    let pixel = max(fwidth(distance), 1e-5);
    let fill = clamp((distance - 0.5) / pixel + 0.5, 0.0, 1.0);
    let outline = clamp((distance - (0.5 - OUTLINE_PIXELS * pixel)) / pixel + 0.5, 0.0, 1.0) * OUTLINE_ALPHA;
    // The fill over its dark outline.
    let alpha = fill + outline * (1.0 - fill);
    if (alpha <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(input.color.rgb * fill / alpha, alpha * input.color.a);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return shade(input);
}

@fragment
fn fs_hidden(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = shade(input);
    return vec4<f32>(color.rgb, color.a * HIDDEN_ALPHA);
}
