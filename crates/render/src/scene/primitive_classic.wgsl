@group(1) @binding(0) var material_texture: texture_2d<f32>;
@group(1) @binding(1) var material_sampler: sampler;

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    return lit_color(input, textureSample(material_texture, material_sampler, input.uv));
}
