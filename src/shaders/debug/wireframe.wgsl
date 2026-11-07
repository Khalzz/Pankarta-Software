// The wireframe debug view (see rendering::debug_views) - a mesh's edges,
// read straight from its model's own vertex and index buffers. Drawn as a
// line list of two points an edge, three edges a triangle: the vertex's
// number says which triangle, which edge and which end, and the index
// buffer which of the model's vertices that is.

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_pos: vec4<f32>,
};

struct Transform {
    model_matrix: mat4x4<f32>,
};

// A model's vertices, as plain numbers - ModelVertex: position (3),
// texture coordinates (2), normal (3).
@group(0) @binding(0)
var<storage, read> vertices: array<f32>;
@group(0) @binding(1)
var<storage, read> indices: array<u32>;
// The mesh's first vertex in `vertices` - its indices count from there.
@group(0) @binding(2)
var<uniform> base_vertex: vec4<i32>;

@group(1) @binding(0)
var<uniform> camera: CameraUniform;

@group(2) @binding(0)
var<uniform> transform: Transform;

const FLOATS_PER_VERTEX: u32 = 8u;
const COLOR: vec4<f32> = vec4<f32>(0.2, 1.0, 0.4, 1.0);

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) number: u32, instance: InstanceInput) -> @builtin(position) vec4<f32> {
    let triangle = number / 6u;
    let edge = (number % 6u) / 2u;
    let corner = (edge + number % 2u) % 3u;
    let vertex = u32(i32(indices[triangle * 3u + corner]) + base_vertex.x) * FLOATS_PER_VERTEX;
    let position = vec3<f32>(vertices[vertex], vertices[vertex + 1u], vertices[vertex + 2u]);

    let model_matrix = mat4x4<f32>(instance.model_matrix_0, instance.model_matrix_1, instance.model_matrix_2, instance.model_matrix_3);
    var clip = camera.view_proj * model_matrix * transform.model_matrix * vec4<f32>(position, 1.0);
    // A hair nearer than the surface it lies on (reversed-Z: nearer is
    // greater), so the surface doesn't hide it.
    clip.z = clip.z * 1.0005;
    return clip;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return COLOR;
}
