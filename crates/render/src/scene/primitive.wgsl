struct Object {
    clip_from_local: mat4x4<f32>,
    normal_from_local: mat4x4<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var<uniform> object: Object;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
}

@vertex
fn vertex(@location(0) position: vec3<f32>, @location(1) normal: vec3<f32>) -> VertexOutput {
    var output: VertexOutput;
    output.position = object.clip_from_local * vec4<f32>(position, 1.0);
    output.normal = (object.normal_from_local * vec4<f32>(normal, 0.0)).xyz;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let light = normalize(vec3<f32>(-0.5, 1.0, 0.8));
    let shade = 0.38 + 0.62 * max(dot(normalize(input.normal), light), 0.0);
    return vec4<f32>(object.color.rgb * shade, object.color.a);
}
