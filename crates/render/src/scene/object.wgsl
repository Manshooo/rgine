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
    shadow_clip_from_world: mat4x4<f32>,
    upper_light: vec3<f32>,
    lower_light: vec3<f32>,
}

@group(0) @binding(0) var<uniform> object: Object;
