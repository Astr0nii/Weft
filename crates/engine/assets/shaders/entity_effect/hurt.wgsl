// Red, what taking damage shows.
fn effect(p: EntityPixel, c: vec4<f32>, strength: f32) -> vec4<f32> {
    return vec4(c.rgb * mix(vec3(1.0), vec3(1.0, 0.133, 0.133), strength), c.a);
}
