struct Object {
    clip_from_local: mat4x4<f32>,
    world_from_local: mat4x4<f32>,
    normal_from_local: mat3x3<f32>,
    color: vec4<f32>,
    eye: vec3<f32>,
    light_intensity: f32,
    direction_to_light: vec3<f32>,
    metallic: f32,
    roughness: f32,
    texture_index: u32,
    sampler_index: u32,
}

@group(0) @binding(0) var<uniform> object: Object;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world_position: vec3<f32>,
}

@vertex
fn vertex(
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
) -> VertexOutput {
    var output: VertexOutput;
    output.position = object.clip_from_local * vec4<f32>(position, 1.0);
    output.normal = object.normal_from_local * normal;
    output.uv = uv;
    output.world_position = (object.world_from_local * vec4<f32>(position, 1.0)).xyz;
    return output;
}

fn lit_color(input: VertexOutput, texel: vec4<f32>) -> vec4<f32> {
    let normal = normalize(input.normal);
    let light = object.direction_to_light;
    let n_dot_l = max(dot(normal, light), 0.0);
    let base = texel * object.color;
    if object.metallic < 0.0 {
        return vec4<f32>(base.rgb * (0.38 + 0.62 * n_dot_l), base.a);
    }

    let metallic = object.metallic;
    let ambient = base.rgb * mix(0.20, 0.10, metallic);
    let view = normalize(object.eye - input.world_position);
    let n_dot_v = max(dot(normal, view), 0.0);
    if n_dot_l <= 0.0 || n_dot_v <= 0.0 {
        return vec4<f32>(ambient, base.a);
    }

    let halfway = normalize(view + light);
    let n_dot_h = clamp(dot(normal, halfway), 0.0, 1.0);
    let v_dot_h = clamp(dot(view, halfway), 0.0, 1.0);
    let alpha = max(object.roughness, 0.08) * max(object.roughness, 0.08);
    let alpha_squared = alpha * alpha;
    let pi = 3.14159265;

    // GGX distribution and height-correlated Smith visibility.
    let denominator = n_dot_h * n_dot_h * (alpha_squared - 1.0) + 1.0;
    let distribution = alpha_squared / (pi * denominator * denominator);
    let lambda_v = n_dot_l * sqrt(n_dot_v * n_dot_v * (1.0 - alpha_squared) + alpha_squared);
    let lambda_l = n_dot_v * sqrt(n_dot_l * n_dot_l * (1.0 - alpha_squared) + alpha_squared);
    let visibility = 0.5 / max(lambda_v + lambda_l, 1e-5);

    let f0 = mix(vec3<f32>(0.04), base.rgb, metallic);
    let fresnel = f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - v_dot_h, 5.0);
    let diffuse = (vec3<f32>(1.0) - fresnel) * (1.0 - metallic) * base.rgb / pi;
    let specular = fresnel * distribution * visibility;
    let direct = (diffuse + specular) * object.light_intensity * n_dot_l;
    return vec4<f32>(ambient + direct, base.a);
}
