@group(2) @binding(0) var shadow_depth: texture_depth_2d;
@group(2) @binding(1) var shadow_sampler: sampler_comparison;

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

fn shadow_visibility(world_position: vec3<f32>) -> f32 {
    let clip = object.shadow_clip_from_world * vec4<f32>(world_position, 1.0);
    let uv = clip.xy * vec2<f32>(0.5, -0.5) + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || clip.z < 0.0 || clip.z > 1.0 {
        return 1.0;
    }
    return textureSampleCompareLevel(shadow_depth, shadow_sampler, uv, clip.z - 0.0001);
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
    let upper_weight = clamp(normal.y * 0.5 + 0.5, 0.0, 1.0);
    let hemisphere = mix(object.lower_light, object.upper_light, upper_weight);
    let ambient = base.rgb * mix(hemisphere, vec3<f32>(0.10), metallic);
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
    let direct = (diffuse + specular) * object.light_intensity * n_dot_l
        * shadow_visibility(input.world_position);
    return vec4<f32>(ambient + direct, base.a);
}
