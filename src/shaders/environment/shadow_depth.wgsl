// The shadow pass (see engine::rendering::render_pipeline::shadow_renderer):
// every shadow caster's depth, seen from the sun. Bound like the model
// pipelines - 0 the mesh's texture, 1 the camera (here the cascade's view
// from the sun), 2 the mesh's transform - so a material whose own vertex()
// hook moves its vertices can be drawn in the same pass with its own shader.
//
// Most meshes draw depth only (no fragment stage); one whose texture cuts
// its shape out (glTF alpha mode Mask - leaves, fences) runs fs_cutout,
// which leaves out what's see-through.

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec4<f32>,
    near: f32,
    far: f32,
};

struct Transform {
    model_matrix: mat4x4<f32>,
    bounds_min: vec4<f32>,
};

@group(0) @binding(0)
var t_diffuse: texture_2d<f32>;
@group(0) @binding(1)
var s_diffuse: sampler;

@group(1) @binding(0)
var<uniform> camera: CameraUniform;

@group(2) @binding(0)
var<uniform> transform: Transform;

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>, instance: InstanceInput) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );
    // Camera-relative, like everything drawn - so is the sun's matrix.
    let world_position = model_matrix * transform.model_matrix * vec4<f32>(position, 1.0);
    var out: VertexOutput;
    out.clip_position = camera.view_proj * world_position;
    out.uv = uv;
    return out;
}

@fragment
fn fs_cutout(in: VertexOutput) {
    if (textureSample(t_diffuse, s_diffuse, in.uv).a < 0.5) {
        discard;
    }
}
