//! Noise in 32-bit integers only (wrapping arithmetic, shifts, integer division), so the CPU and
//! the GPU compute the same bits: WGSL has neither doubles nor 64-bit integers, and float
//! results may differ between GPUs. The engine's `shaders/terrain.wgsl` is a line-by-line
//! copy; keep them in step (testkit's `gpu_terrain` and `core:terrain_check` compare cubes).
//!
//! Positions are in 1/16 block (`Q4`), noise values, weights and fractions in 1/4096 (`Q12`).

/// What the terrain's noises are salted with, one name each: the CPU uses these and the GPU gets
/// the seeds made from them (`gpu_layout::terrain::SEEDS`).
pub mod salt {
    pub const CONTINENT: u64 = 0x11;
    pub const TEMPERATURE: u64 = 0x22;
    pub const HUMIDITY: u64 = 0x33;
    pub const HILLS: u64 = 0x55;
    pub const MOUNTAINS: u64 = 0x66;
    pub const PLATEAUS: u64 = 0x77;
    pub const CANYONS: u64 = 0x88;
    pub const DUNES: u64 = 0x99;
    pub const CRATERS: u64 = 0xC7A7;
    /// The two noodle cave fields, the cheese caverns and the cavern roof's patches.
    pub const CAVE_A: u64 = 0xC1;
    pub const CAVE_B: u64 = 0xC2;
    pub const CAVERN: u64 = 0xC3;
    pub const CAVERN_ROOF: u64 = 0xC4;
    pub const LAVA: u64 = 0x1A7A;
    /// Cave biome `k` uses `CAVE_BIOME ^ k`.
    pub const CAVE_BIOME: u64 = 0xCB10;
}

/// 1.0 in Q12.
pub const ONE: i32 = 4096;

#[inline]
pub fn fmix(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    h = h.wrapping_mul(0xC2B2_AE35);
    h ^ (h >> 16)
}

#[inline]
pub fn hash(seed: u32, x: i32, y: i32, z: i32) -> u32 {
    fmix(seed ^ fmix(x as u32 ^ fmix(y as u32 ^ fmix(z as u32 ^ 0x27D4_EB2F))))
}

/// A 32-bit seed for one field of a 64-bit world seed.
pub fn seed32(seed: u64, salt: u64) -> u32 {
    let s = seed ^ salt;
    fmix(s as u32 ^ fmix((s >> 32) as u32))
}

/// Floor division and the remainder in `0..d` (`d > 0`).
#[inline]
pub fn div_floor(p: i32, d: i32) -> (i32, i32) {
    let mut q = p / d;
    if p % d != 0 && p < 0 {
        q -= 1;
    }
    (q, p - q * d)
}

/// Cell and Q12 fraction of position `p` (Q4) on a lattice of `scale` (Q4, at most 2^20).
#[inline]
fn lattice(p: i32, scale: i32) -> (i32, i32) {
    let s = scale.clamp(1, (1 << 20) - 1);
    let (cell, r) = div_floor(p, s);
    (cell, ((r as u32 * 4096) / s as u32) as i32)
}

#[inline]
fn fade(t: i32) -> i32 {
    let t3 = (((t * t) >> 12) * t) >> 12;
    (t3 * (((t * (t * 6 - 15 * ONE)) >> 12) + 10 * ONE)) >> 12
}

#[inline]
fn lerp(a: i32, b: i32, t: i32) -> i32 {
    a + (((b - a) * t) >> 12)
}

#[inline]
fn grad2(h: u32, x: i32, y: i32) -> i32 {
    match h & 7 {
        0 => x + y,
        1 => x - y,
        2 => -x + y,
        3 => -x - y,
        4 => x,
        5 => -x,
        6 => y,
        _ => -y,
    }
}

#[inline]
fn grad3(h: u32, x: i32, y: i32, z: i32) -> i32 {
    match h % 12 {
        0 => x + y,
        1 => -x + y,
        2 => x - y,
        3 => -x - y,
        4 => x + z,
        5 => -x + z,
        6 => x - z,
        7 => -x - z,
        8 => y + z,
        9 => -y + z,
        10 => y - z,
        _ => -y - z,
    }
}

/// About -1..1 (Q12) at (`x`, `y`) (Q4), `scale` (Q4) per lattice cell.
pub fn perlin2(seed: u32, x: i32, y: i32, scale: i32) -> i32 {
    let (xi, xf) = lattice(x, scale);
    let (yi, yf) = lattice(y, scale);
    let (u, v) = (fade(xf), fade(yf));
    let g = |dx: i32, dy: i32| grad2(hash(seed, xi + dx, yi + dy, 0), xf - dx * ONE, yf - dy * ONE);
    let a = lerp(g(0, 0), g(1, 0), u);
    let b = lerp(g(0, 1), g(1, 1), u);
    (lerp(a, b, v) * 2867) >> 12
}

/// Like `perlin2` in three dimensions, with a scale per axis.
pub fn perlin3(seed: u32, x: i32, y: i32, z: i32, sx: i32, sy: i32, sz: i32) -> i32 {
    let (xi, xf) = lattice(x, sx);
    let (yi, yf) = lattice(y, sy);
    let (zi, zf) = lattice(z, sz);
    let (u, v, w) = (fade(xf), fade(yf), fade(zf));
    let g = |dx: i32, dy: i32, dz: i32| grad3(hash(seed, xi + dx, yi + dy, zi + dz), xf - dx * ONE, yf - dy * ONE, zf - dz * ONE);
    let x00 = lerp(g(0, 0, 0), g(1, 0, 0), u);
    let x10 = lerp(g(0, 1, 0), g(1, 1, 0), u);
    let x01 = lerp(g(0, 0, 1), g(1, 0, 1), u);
    let x11 = lerp(g(0, 1, 1), g(1, 1, 1), u);
    lerp(lerp(x00, x10, v), lerp(x01, x11, v), w)
}

/// Octaves of `perlin2`, each twice as fine and half as strong, about -1..1.
pub fn fbm(seed: u32, x: i32, z: i32, scale: i32, octaves: u32) -> i32 {
    let (mut sum, mut norm) = (0i32, 0i32);
    for o in 0..octaves.min(12) {
        let amp = ONE >> o;
        sum += (perlin2(seed.wrapping_add(o * 7919), x, z, (scale >> o).max(1)) * amp) >> 12;
        norm += amp;
    }
    if norm == 0 { 0 } else { sum * ONE / norm }
}

/// 0 at `from`, 1 at `to` (Q12), linear in between; `from > to` gives a falling edge.
pub fn ramp(v: i32, from: i32, to: i32) -> i32 {
    if to == from {
        return if v >= to { ONE } else { 0 };
    }
    // Parameters are within +-2^18 (`q12` clamps), so this stays in 32 bits.
    ((v.wrapping_sub(from)).clamp(-(1 << 18), 1 << 18) * ONE / (to - from)).clamp(0, ONE)
}

/// To Q12 (and Q4 below), rounded, for parameters given as decimals; out of range (an open
/// range's infinity) saturates at +-2^18, far past any field.
pub fn q12(v: f64) -> i32 {
    (v * 4096.0)
        .round()
        .clamp(-(1 << 18) as f64, (1 << 18) as f64) as i32
}

pub fn q4(v: f64) -> i32 {
    (v * 16.0).round().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

/// Where `weft_abi::expr::SITE` looks: the square of the distance from (`x`, `z`) (Q4) to the
/// nearest point of the grid cells around, in units of that point's radius (Q12). A cell of
/// `spacing` (Q4) holds one point at a seeded spot with chance `chance` (Q12), and its radius is
/// `radius` (Q4) times 0.5 to 1.5. 8192 where none is within reach.
pub fn site(seed: u32, x: i32, z: i32, spacing: i32, chance: i32, radius: i32) -> i32 {
    let sp = spacing.clamp(16, 512 * 16);
    let (cx, cz) = (div_floor(x, sp).0, div_floor(z, sp).0);
    let (bx, bz) = (x >> 4, z >> 4);
    let mut nearest = 2 * ONE;
    for dz in -1..=1 {
        for dx in -1..=1 {
            let h = hash(seed, cx + dx, 0, cz + dz);
            let h2 = fmix(h);
            if (h & 4095) as i32 > chance {
                continue;
            }
            // In blocks from here on: squares of Q4 distances overflow 32 bits.
            let r = ((radius.clamp(16, 512 * 16) * (2048 + ((h >> 12) & 4095) as i32)) >> 12) >> 4;
            let r2 = (r * r).max(1);
            let px = ((cx + dx) * sp + (((h2 & 4095) as i32 * sp) >> 12)) >> 4;
            let pz = ((cz + dz) * sp + ((((h2 >> 12) & 4095) as i32 * sp) >> 12)) >> 4;
            let d2 = (bx - px) * (bx - px) + (bz - pz) * (bz - pz);
            if d2 >= r2 * 2 {
                continue;
            }
            nearest = nearest.min(((d2 as u32 * 4096) / r2 as u32) as i32);
        }
    }
    nearest
}
