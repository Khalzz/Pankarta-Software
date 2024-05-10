struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

// on WGSL we define entry points with the @vertex and the @fragment respectively.

// the vertex shader defines the form or the shape
// we are passing the 0..3 here or 0,1,2
/*
    the way it works FOR THE X AXIS is the next way {
        1. (1 - 0) * 0.5 = -0.5
        2. (1 - 1) * 0.5 = 0
        3. (2 - 2) * 0.5 = 0.5

        this is a line between -0.5 to 0.5 in the x axis
    }
    the way it works FOR THE Y AXIS is the next way {
        the bitwise & will work this way:
            if (0, 2, 4) = 0
            if (1, 3, 5) = 1

        1. ((0) * 2 - 1) * 0.5 = -0.5
        2. ((1) * 2 - 1) * 0.5 = 0.5
        3. ((0) * 2 - 1) * 0.5 = -0.5

        this is a line between 0 to 1 in the x axis
    }
*/
@vertex
fn vs_main(@builtin(vertex_index) in_vertex_index: u32,) -> VertexOutput {
    var out: VertexOutput;
    let x = f32(1 - i32(in_vertex_index)) * 0.4;
    let y = f32(i32(in_vertex_index & 1u) * 2 - 1) * 0.5;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}

// the fragment shader defines the color of the things we are doing
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(0.3, 0.2, 0.1, 1.0);
}