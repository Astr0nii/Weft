// Particles on the GPU: every live one moved a frame, stopped by what `cells` says of the blocks
// around the camera, answering what happens to it by its kind's `rules`, and written out for
// `particle.wgsl` to draw. The prelude
// (`gpu_layout::particle_sim_prelude`) declares the bindings and `args`.

const THROUGH: u32 = 65536u;
const ON_GROUND: u32 = 131072u;
const STOPPED: u32 = 262144u;
const IN_LIQUID: u32 = 524288u;
// Vanilla's box: 0.2 across and 0.2 up from where it is.
const BOX_LOW: vec3<f32> = vec3<f32>(-0.1, 0.0, -0.1);
const BOX_HIGH: vec3<f32> = vec3<f32>(0.1, 0.2, 0.1);

// A block's word: light (a nibble each: sky, red, green, blue), then what is solid of it from
// bottom to top in sixteenths, then whether it is a liquid and its tags. What is not in the
// window is nothing, under the open sky.
fn cell_word(c: vec3<i32>) -> u32 {
    let cube = c >> vec3<u32>(4u);
    let far = abs(cube - args.center.xyz);
    if far.x > WINDOW_REACH || far.y > WINDOW_REACH || far.z > WINDOW_REACH {
        return 15u;
    }
    let side = WINDOW_REACH * 2 + 1;
    let at = ((cube % side) + side) % side;
    let slot = u32(at.x + side * (at.y + side * at.z));
    let local = c & vec3<i32>(15);
    return cells[slot * 4096u + u32(local.x + 16 * (local.z + 16 * local.y))];
}

// How far up and from where a block is solid, in blocks: empty when bottom is not under top.
fn solid_span(word: u32) -> vec2<f32> {
    return vec2<f32>(f32((word >> 16u) & 31u), f32((word >> 21u) & 31u)) / 16.0;
}

// Whether the box around `p` overlaps anything solid (`p` from the origin).
fn overlaps(p: vec3<f32>) -> bool {
    let lo = vec3<i32>(floor(p + BOX_LOW));
    let hi = vec3<i32>(floor(p + BOX_HIGH));
    for (var z = lo.z; z <= hi.z; z++) {
        for (var y = lo.y; y <= hi.y; y++) {
            for (var x = lo.x; x <= hi.x; x++) {
                let s = solid_span(cell_word(vec3(x, y, z) + args.origin.xyz));
                if s.y > s.x && p.y + BOX_HIGH.y > f32(y) + s.x && p.y + BOX_LOW.y < f32(y) + s.y {
                    return true;
                }
            }
        }
    }
    return false;
}

// How far the box around `p` gets along `axis` of the distance `d`, up to what stops it.
fn sweep(p: vec3<f32>, axis: u32, d: f32) -> f32 {
    var to = p;
    to[axis] = p[axis] + d;
    let lo = vec3<i32>(floor(min(p, to) + BOX_LOW));
    let hi = vec3<i32>(floor(max(p, to) + BOX_HIGH));
    let box_low = p + BOX_LOW;
    let box_high = p + BOX_HIGH;
    var left = d;
    for (var z = lo.z; z <= hi.z; z++) {
        for (var y = lo.y; y <= hi.y; y++) {
            for (var x = lo.x; x <= hi.x; x++) {
                let s = solid_span(cell_word(vec3(x, y, z) + args.origin.xyz));
                if s.y <= s.x {
                    continue;
                }
                let low = vec3<f32>(f32(x), f32(y) + s.x, f32(z));
                let high = vec3<f32>(f32(x + 1), f32(y) + s.y, f32(z + 1));
                // Beside it on the other two axes: never in its way.
                var beside = false;
                for (var i = 0u; i < 3u; i++) {
                    if i != axis && (box_high[i] <= low[i] || box_low[i] >= high[i]) {
                        beside = true;
                    }
                }
                if beside {
                    continue;
                }
                if d > 0.0 && box_high[axis] <= low[axis] + 1e-4 {
                    left = min(left, max(low[axis] - box_high[axis], 0.0));
                } else if d < 0.0 && box_low[axis] >= high[axis] - 1e-4 {
                    left = max(left, min(high[axis] - box_low[axis], 0.0));
                }
            }
        }
    }
    return left;
}

fn mix_color(a: u32, b: u32, t: f32) -> u32 {
    var out = 0u;
    for (var s = 0u; s < 32u; s += 8u) {
        let x = f32((a >> s) & 255u);
        let y = f32((b >> s) & 255u);
        out |= u32(round(x + (y - x) * t)) << s;
    }
    return out;
}

// A block's word past its span: bit 26 a liquid, from bit 27 the tags `Game::particle_tags` name.
const LIQUID_BIT: u32 = 26u;
const TAG_SHIFT: u32 = 27u;

// Whether rule `r` holds at a block with word `word`.
fn holds(r: ParticleRule, word: u32) -> bool {
    let when = (r.data.x >> 4u) & 15u;
    if when == WHEN_LIQUID {
        return ((word >> LIQUID_BIT) & 1u) != 0u;
    }
    if when == WHEN_TAG {
        return ((word >> (TAG_SHIFT + ((r.data.x >> 8u) & 255u))) & 1u) != 0u;
    }
    return true;
}

// The first of the kind's rules for `event` that holds at `word`, or `none`.
const NO_RULE: u32 = 0xFFFFFFFFu;
fn find_rule(kind: ParticleKind, event: u32, word: u32) -> u32 {
    for (var n = 0u; n < kind.frames.w; n++) {
        let i = kind.frames.z + n;
        let r = rules[i];
        if (r.data.x & 15u) == event && holds(r, word) {
            return i;
        }
    }
    return NO_RULE;
}

fn action(rule: u32) -> u32 {
    return (rules[rule].data.x >> 16u) & 255u;
}

// A number in 0..1 of its own for particle `i` at `age`.
fn random01(i: u32, age: f32) -> f32 {
    var h = i * 747796405u + bitcast<u32>(age) * 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return f32((h >> 22u) ^ h) / 4294967295.0;
}

// What a particle is while one step runs.
struct Live {
    kind: u32,
    flags: u32,
    age: f32,
    life: f32,
    size: f32,
    vel: vec3<f32>,
}

// Rule `rule` on `l`, which hit something along `axis` (3: nothing) at the speed `before`.
fn apply(rule: u32, l: ptr<function, Live>, axis: u32, before: vec3<f32>, i: u32) {
    let a = action(rule);
    if a == THEN_DIE {
        (*l).age = (*l).life;
    } else if a == THEN_STOP {
        (*l).flags |= STOPPED;
        (*l).vel = vec3(0.0);
    } else if a == THEN_SLIDE {
        if axis < 3u {
            (*l).vel[axis] = 0.0;
        }
    } else if a == THEN_BOUNCE {
        if axis < 3u {
            (*l).vel[axis] = -before[axis] * bitcast<f32>(rules[rule].data.y);
        }
    } else if a == THEN_BECOME || a == THEN_BECOME_STILL {
        let into = rules[rule].data.z;
        let start = kinds[into].start;
        (*l).kind = into;
        (*l).age = 0.0;
        (*l).life = mix(start.x, start.y, random01(i, (*l).life));
        (*l).size = mix(start.z, start.w, random01(i + 1u, (*l).life));
        (*l).flags &= ~(STOPPED | ON_GROUND);
        if a == THEN_BECOME_STILL {
            (*l).vel = vec3(0.0);
        }
    }
}

@compute @workgroup_size(256)
fn simulate(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= args.count {
        return;
    }
    let i = args.first + id.x;
    let p = sim[i];
    var position = p.pos_size.xyz + args.shift.xyz;
    if p.vel_age.w >= p.life.x {
        dst[i].pos_size = vec4(0.0);
        sim[i].pos_size = vec4(position, p.pos_size.w);
        return;
    }
    var l = Live(p.data.x & 65535u, p.data.x & ~65535u, p.vel_age.w, p.life.x, p.pos_size.w, p.vel_age.xyz);
    var kind = kinds[l.kind];
    let dt = args.dt;
    l.age += dt;
    l.vel.y -= kind.motion.x * dt;
    l.vel *= kind.motion.y;
    // At most a block a step, the most `sweep` looks ahead.
    let delta = clamp(l.vel * dt, vec3(-1.0), vec3(1.0));
    let collides = (kind.data.z & PARTICLE_FLAG_COLLIDE) != 0u;
    if (l.flags & THROUGH) != 0u {
        position += delta;
        if !overlaps(position) {
            l.flags &= ~THROUGH;
        }
    } else if collides && (l.flags & STOPPED) == 0u {
        let was_on_ground = (l.flags & ON_GROUND) != 0u;
        let before = l.vel;
        var hit = vec3<bool>(false);
        var stuck = false;
        for (var n = 0u; n < 3u; n++) {
            // Up and down first, as anything else in the world moves.
            let axis = (n + 1u) % 3u;
            if delta[axis] == 0.0 {
                continue;
            }
            let moved = sweep(position, axis, delta[axis]);
            position[axis] += moved;
            // As in vanilla: one that can't go up or down any more stays where it is for good.
            if axis == 1u && abs(delta.y) >= 1e-5 && abs(moved) < 1e-5 {
                stuck = true;
            }
            if moved != delta[axis] {
                hit[axis] = true;
                l.vel[axis] = 0.0;
            }
        }
        // What it hit, by the side it hit it on: the kind's rules for that, else vanilla's way.
        var answered = false;
        for (var axis = 0u; axis < 3u; axis++) {
            if !hit[axis] || answered {
                continue;
            }
            var event = EVENT_WALL;
            var reach = 0.11;
            if axis == 1u {
                event = select(EVENT_CEILING, EVENT_LAND, delta.y < 0.0);
                reach = select(0.21, 0.01, delta.y < 0.0);
            }
            // Landing counts once, not every step it lies there.
            if event == EVENT_LAND && was_on_ground {
                continue;
            }
            var probe = position + vec3(0.0, 0.1, 0.0);
            probe[axis] = position[axis] + sign(delta[axis]) * reach;
            if axis == 1u && delta.y < 0.0 {
                probe.y = position.y - reach;
            }
            let rule = find_rule(kind, event, cell_word(vec3<i32>(floor(probe)) + args.origin.xyz));
            if rule != NO_RULE {
                apply(rule, &l, axis, before, i);
                answered = true;
            }
        }
        if !answered && stuck {
            l.flags |= STOPPED;
        }
        let on_ground = (hit.y && delta.y < 0.0) || (was_on_ground && hit.y);
        l.flags = (l.flags & ~ON_GROUND) | select(0u, ON_GROUND, on_ground);
        if on_ground && (l.flags & STOPPED) == 0u {
            l.vel.x *= kind.motion.z;
            l.vel.z *= kind.motion.z;
        }
    } else if !collides {
        position += delta;
    }
    // In or out of a liquid, by the middle of its box.
    let here = cell_word(vec3<i32>(floor(position + vec3(0.0, 0.1, 0.0))) + args.origin.xyz);
    let wet = ((here >> LIQUID_BIT) & 1u) != 0u;
    if wet != ((l.flags & IN_LIQUID) != 0u) {
        l.flags ^= IN_LIQUID;
        let rule = find_rule(kind, select(EVENT_LEAVE_LIQUID, EVENT_ENTER_LIQUID, wet), here);
        if rule != NO_RULE {
            apply(rule, &l, 3u, l.vel, i);
        }
    }
    if l.age >= l.life {
        let rule = find_rule(kind, EVENT_EXPIRE, here);
        if rule != NO_RULE {
            apply(rule, &l, 3u, l.vel, i);
        }
    }
    kind = kinds[l.kind];
    sim[i].pos_size = vec4(position, l.size);
    sim[i].vel_age = vec4(l.vel, l.age);
    sim[i].life.x = l.life;
    sim[i].data.x = l.kind | l.flags;
    if l.age >= l.life {
        dst[i].pos_size = vec4(0.0);
        return;
    }

    let t = clamp(l.age / l.life, 0.0, 1.0);
    let size = select(l.size, l.size * (1.0 - t), (kind.data.z & PARTICLE_FLAG_SHRINK) != 0u);
    // A kind's frames over its life, or the block a piece of dust is of.
    var texture = p.data.z;
    var uv = p.data.w;
    if kind.frames.y > 0u {
        texture = frames[kind.frames.x + min(u32(t * f32(kind.frames.y)), kind.frames.y - 1u)];
        uv = 0xFFFF0000u;
    }
    var light = 0xFFFFFFFFu;
    if (kind.data.z & PARTICLE_FLAG_FULLBRIGHT) == 0u {
        light = (here & 15u) | (((here >> 4u) & 15u) << 8u) | (((here >> 8u) & 15u) << 16u) | (((here >> 12u) & 15u) << 24u);
    }
    let color = mix_color(kind.data.x, kind.data.y, t);
    let tint = p.data.y;
    let tinted = (((((color >> 24u) & 255u) * ((tint >> 16u) & 255u)) / 255u) << 24u)
        | (((((color >> 16u) & 255u) * ((tint >> 8u) & 255u)) / 255u) << 16u)
        | (((((color >> 8u) & 255u) * (tint & 255u)) / 255u) << 8u)
        | (color & 255u);
    dst[i].pos_size = vec4(position + args.rel.xyz, size);
    dst[i].data = vec4(tinted, (texture & PARTICLE_TEX_MASK) | (kind.data.w << PARTICLE_SLOT_SHIFT), uv, light);
    dst[i].extra = vec4(t, 0.0, 0.0, 0.0);
}
