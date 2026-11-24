@vertex
fn vertex(@location(0) position: vec3<f32>) -> @builtin(position) vec4<f32> {
    return object.shadow_clip_from_world * (object.world_from_local * vec4<f32>(position, 1.0));
}
