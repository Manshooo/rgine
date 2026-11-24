@group(1) @binding(0) var material_textures: binding_array<texture_2d<f32>>;
@group(1) @binding(1) var material_samplers: binding_array<sampler>;

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(
        material_textures[object.texture_index],
        material_samplers[object.sampler_index],
        input.uv,
    );
    return lit_color(input, texel);
}
