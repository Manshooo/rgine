@group(0) @binding(0) var<uniform> world_from_clip: mat4x4<f32>;

struct SkyVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) ray: vec3<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> SkyVertex {
    let positions = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    let clip = vec4<f32>(positions[index], 1.0, 1.0);
    return SkyVertex(clip, (world_from_clip * clip).xyz);
}

@fragment
fn fragment(input: SkyVertex) -> @location(0) vec4<f32> {
    let direction = normalize(input.ray);
    let horizon = vec3<f32>(0.40, 0.56, 0.70);
    let zenith = vec3<f32>(0.035, 0.12, 0.30);
    let below = vec3<f32>(0.055, 0.075, 0.10);
    let height = direction.y;
    let upper = mix(horizon, zenith, pow(max(height, 0.0), 0.55));
    let sky = mix(upper, below, smoothstep(0.0, 0.45, -height));

    let sun = max(dot(direction, normalize(vec3<f32>(-0.35, 0.28, -1.0))), 0.0);
    let glow = 0.24 * pow(sun, 8.0);
    let disc = smoothstep(0.9995, 0.9998, sun);
    return vec4<f32>(sky + vec3<f32>(1.0, 0.65, 0.30) * (glow + disc), 1.0);
}
