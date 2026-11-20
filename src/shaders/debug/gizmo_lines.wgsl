// The gizmos' lines (see rendering::gizmos) - one instance per segment,
// widened on screen to `width` pixels, with a color at each end and
// optionally dashed. Drawn three ways (see gizmos/renderer.rs): where the
// world doesn't hide them (fs_main, depth-tested), faint where it does
// (fs_hidden, the depth test turned round), and over everything (fs_main,
// no depth).

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec4<f32>,
};

struct Gizmo {
    // The camera's world position - instances are in world space, the
    // camera's view_proj expects positions relative to the camera.
    camera_position: vec4<f32>,
    // Screen size in pixels (x, y).
    screen: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(1) @binding(0)
var<uniform> gizmo: Gizmo;

struct LineInstance {
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
    @location(2) start_color: vec4<f32>,
    @location(3) end_color: vec4<f32>,
    // x: width (px); y: dash length (px, 0 solid); z: how far into the dash
    // pattern its start is (px - the path's length on screen before it).
    @location(4) params: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Pixels from the line's middle across it, and along it from its start
    // (plus the dash offset) - straight on screen, so not perspective
    // corrected.
    @location(1) @interpolate(linear) across: f32,
    @location(2) @interpolate(linear) along: f32,
    @location(3) @interpolate(flat) half_width: f32,
    @location(4) @interpolate(flat) dash: f32,
};

// Moves `a` along to `b` until it's in front of the camera (clip w > NEAR) -
// a segment running behind the camera is cut where it crosses, not flipped.
const NEAR: f32 = 0.0001;
// How faint a gizmo is where the world hides it.
const HIDDEN_ALPHA: f32 = 0.3;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, line: LineInstance) -> VertexOutput {
    var output: VertexOutput;

    var a = camera.view_proj * vec4<f32>(line.start - gizmo.camera_position.xyz, 1.0);
    var b = camera.view_proj * vec4<f32>(line.end - gizmo.camera_position.xyz, 1.0);
    if (a.w < NEAR && b.w < NEAR) {
        output.position = vec4<f32>(0.0, 0.0, 0.0, 0.0);
        return output;
    }
    var start_color = line.start_color;
    var end_color = line.end_color;
    if (a.w < NEAR) {
        let t = (NEAR - a.w) / (b.w - a.w);
        a = mix(a, b, t);
        start_color = mix(start_color, end_color, t);
    }
    if (b.w < NEAR) {
        let t = (NEAR - b.w) / (a.w - b.w);
        b = mix(b, a, t);
        end_color = mix(end_color, start_color, t);
    }

    let half_screen = gizmo.screen.xy * 0.5;
    let screen_a = a.xy / a.w * half_screen;
    let screen_b = b.xy / b.w * half_screen;
    var direction = screen_b - screen_a;
    let length_pixels = length(direction);
    if (length_pixels < 1e-4) {
        direction = vec2<f32>(1.0, 0.0);
    }
    direction = normalize(direction);
    let across = vec2<f32>(-direction.y, direction.x);

    // Two triangles: corners (end, side) - end 0 = start, 1 = end.
    var ends = array<f32, 6>(0.0, 1.0, 0.0, 0.0, 1.0, 1.0);
    var sides = array<f32, 6>(-1.0, -1.0, 1.0, 1.0, -1.0, 1.0);
    let at_end = ends[vertex_index];
    let side = sides[vertex_index];

    var base = a;
    var along = -1.0;
    output.color = start_color;
    if (at_end > 0.5) {
        base = b;
        along = 1.0;
        output.color = end_color;
    }
    // Half the width either side (and a pixel more, for the soft edge), and
    // half past each end so joints meet.
    let half_width = max(line.params.x, 1.0) * 0.5;
    let reach = half_width + 1.0;
    let offset_pixels = across * side * reach + direction * along * half_width;
    output.position = vec4<f32>(base.xy + offset_pixels / half_screen * base.w, base.z, base.w);
    output.across = side * reach;
    output.along = line.params.z + at_end * length_pixels + along * half_width;
    output.half_width = half_width;
    output.dash = line.params.y;
    return output;
}

fn shade(input: VertexOutput) -> vec4<f32> {
    // How much of this pixel the line covers, across it - and, dashed,
    // along it: on for `dash` pixels, off for 3/4 of that.
    var coverage = clamp(input.half_width + 0.5 - abs(input.across), 0.0, 1.0);
    if (input.dash > 0.0) {
        let period = input.dash * 1.75;
        let phase = input.along - floor(input.along / period) * period;
        coverage = coverage * clamp(min(phase, input.dash - phase) + 0.5, 0.0, 1.0);
    }
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
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
