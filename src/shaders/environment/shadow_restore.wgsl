// Puts a cascade's still casters' map back into the map that's read, as
// the first draw of the pass the moving casters are drawn in (see
// App::render_shadow_pass): one triangle over the whole map, each pixel's
// depth the still map's there. The cascade (the array layer) is the
// instance.
//
// Instead of copying the still map over and then drawing the moving
// casters on top of the result: on a tile-based GPU (Apple's, phones') a
// pass that keeps what's in its depth reads the whole map in first, and a
// copy reads and writes it all again - this starts from a cleared map,
// which costs nothing, and reads the still map once.

@group(0) @binding(0)
var still: texture_depth_2d_array;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) @interpolate(flat) cascade: u32,
};

@vertex
fn vs_main(@builtin(vertex_index) vertex: u32, @builtin(instance_index) cascade: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    var out: VertexOutput;
    out.clip_position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.cascade = cascade;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @builtin(frag_depth) f32 {
    return textureLoad(still, vec2<i32>(in.clip_position.xy), i32(in.cascade), 0);
}
