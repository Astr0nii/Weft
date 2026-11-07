// Lit up whatever the light around it.
fn effect(p: EntityPixel, c: vec4<f32>, strength: f32) -> vec4<f32> {
    return vec4(mix(c.rgb, max(c.rgb, p.albedo.rgb), strength), c.a);
}
