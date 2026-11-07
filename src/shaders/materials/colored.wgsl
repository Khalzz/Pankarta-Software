// engine/colored - engine/standard, its texture multiplied by a color (see
// `Material::color`).

struct Params {
    color: vec4<f32>,
}

fn fragment(f: FragmentIn) -> vec4<f32> {
    return vec4<f32>(f.lighting * f.base_color.rgb * params.color.rgb, f.base_color.a * params.color.a);
}
