// What is behind what the last frame drew doesn't get drawn. `depths` holds that frame's depth and
// under it ever smaller copies, each texel the farthest of those it covers; `test` zeroes the
// instance count of each draw whose whole cube is farther than all there is where it would be.
// The prelude (`gpu_layout::occlusion_prelude`) declares the bindings and `args`.

// The depth buffer has 1 at the near plane and 0 at infinity: farther is less.
@compute @workgroup_size(32, 32)
fn down(@builtin(global_invocation_id) id: vec3<u32>) {
    let s = args.size;
    let d = (s + vec2<u32>(1u)) / 2u;
    if id.x >= d.x || id.y >= d.y {
        return;
    }
    let lo = id.xy * 2u;
    let hi = min(lo + vec2<u32>(1u), s - vec2<u32>(1u));
    let far = min(
        min(depths[args.src + lo.y * s.x + lo.x], depths[args.src + lo.y * s.x + hi.x]),
        min(depths[args.src + hi.y * s.x + lo.x], depths[args.src + hi.y * s.x + hi.x]),
    );
    depths[args.dst + id.y * d.x + id.x] = far;
}

// Where `level` starts in `depths`, and how big it is.
fn level_of(level: u32) -> vec3<u32> {
    var s = args.size;
    var at = 0u;
    for (var l = 0u; l < level; l++) {
        at += s.x * s.y;
        s = (s + vec2<u32>(1u)) / 2u;
    }
    return vec3<u32>(at, s);
}

fn hidden(lo: vec3<f32>, hi: vec3<f32>) -> bool {
    var small = vec2<f32>(1e9);
    var large = vec2<f32>(-1e9);
    var nearest = 0.0;
    for (var c = 0u; c < 8u; c++) {
        let p = vec3<f32>(
            select(lo.x, hi.x, (c & 1u) != 0u),
            select(lo.y, hi.y, (c & 2u) != 0u),
            select(lo.z, hi.z, (c & 4u) != 0u),
        );
        let clip = args.prev * vec4<f32>(p, 1.0);
        // Reaches behind that camera: there is nothing to go by.
        if clip.w < 0.05 {
            return false;
        }
        let ndc = clip.xyz / clip.w;
        small = min(small, ndc.xy);
        large = max(large, ndc.xy);
        nearest = max(nearest, ndc.z);
    }
    // Any of it off that frame's screen: what is there wasn't seen.
    if small.x < -1.0 || small.y < -1.0 || large.x > 1.0 || large.y > 1.0 {
        return false;
    }
    let screen = vec2<f32>(args.size);
    let a = vec2<f32>(small.x + 1.0, 1.0 - large.y) * 0.5 * screen;
    let b = vec2<f32>(large.x + 1.0, 1.0 - small.y) * 0.5 * screen;
    let level = min(u32(ceil(log2(max(max(b.x - a.x, b.y - a.y), 1.0)))), args.levels - 1u);
    let info = level_of(level);
    let top = vec2<u32>(screen) - vec2<u32>(1u);
    let from_ = min(vec2<u32>(a), top) >> vec2<u32>(level);
    let to = min(vec2<u32>(b), top) >> vec2<u32>(level);
    var far = 1.0;
    for (var y = from_.y; y <= to.y; y++) {
        for (var x = from_.x; x <= to.x; x++) {
            far = min(far, depths[info.x + y * info.y + x]);
        }
    }
    return nearest < far;
}

@compute @workgroup_size(256)
fn test(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    // Distant terrain isn't in cubes, and what moves isn't where it was.
    if i >= args.count || (i >= args.skip.x && i < args.skip.y) {
        return;
    }
    let o = origins[i];
    if o.w != 0 {
        return;
    }
    let lo = vec3<f32>(o.xyz) * 16.0 - args.cam_frac.xyz + args.moved.xyz - vec3<f32>(args.moved.w);
    if hidden(lo, lo + vec3<f32>(16.0 + 2.0 * args.moved.w)) {
        draws[i * 5u + 1u] = 0u;
    }
}
