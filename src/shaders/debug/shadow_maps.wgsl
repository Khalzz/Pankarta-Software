// DebugViews::shadows - one square per cascade's shadow map, each drawn
// into its own viewport (see ShadowRender::draw_debug): near the sun dark,
// far light, nothing in it white.

@group(0) @binding(0)
var maps: texture_depth_2d_array;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) cascade: u32,
};

// One triangle over the whole viewport - the cascade is the instance.
@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) cascade: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    var out: VertexOutput;
    out.clip_position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    out.cascade = cascade;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(maps));
    let texel = vec2<i32>(clamp(in.uv * size, vec2<f32>(0.0), size - 1.0));
    let depth = textureLoad(maps, texel, i32(in.cascade), 0);
    return vec4<f32>(vec3<f32>(depth), 1.0);
}
