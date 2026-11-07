//! Distant terrain beyond the view: made on the GPU straight from the dimension's terrain at a
//! coarser step (`shaders/terrain.wgsl`'s `lod`), so it reaches as far as wanted for a few
//! kilobytes a tile. A tile is 32x32 cells of 2^level blocks; each cell is the column at its
//! middle: its top block, the one under it, its biome, and tree canopies over it. Where saved
//! cubes made a column's top other than the terrain's (a town, a crater), the server says so
//! Tiles are picked by distance, nearer ones split into the four of the level below; one is
//! shown once made, its parent or children standing in until then. Where a column of real cubes
//! shows its ground the tiles' pixels are dropped (`fs_lod`, `Lod::mask`), so they fill in for
//! cubes not made or meshed yet (flying fast), and nowhere stand through them.

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, unbounded};
use glam::{DVec3, IVec2};
use rustc_hash::{FxHashMap, FxHashSet};
use weft_abi::block_flags as bf;
use weft_abi::{BlockModel, FACE_DIRS};

use crate::background;
use crate::config;
use crate::content::{Content, DimensionDef, StateInfo, content, face_paint};
use crate::gpu_layout::terrain::lod as lod_gpu;
use crate::gpu_layout::{CORNERS, face, lod_face, lod_record};
use crate::server::lod_tops::{Span, drawn, shows};

#[derive(Clone)]
pub struct Column {
    pub y: i32,
    pub top: u32,
    pub under: u32,
    pub spans: Vec<Span>,
}

pub type Columns = FxHashMap<IVec2, Column>;

/// Cells a tile side.
pub const CELLS: i32 = 32;
/// Columns a tile is made from: its cells and one around them (for the walls at its edges).
pub const SAMPLES: usize = lod_gpu::SAMPLES as usize;
const SIDE: i32 = lod_gpu::SIDE as i32;
const GRID: usize = (CELLS + 2) as usize;
/// Words per tile coming back from the GPU.
pub const LOD_WORDS_PER_TILE: usize = SAMPLES * 2;
pub const MAX_LEVEL: u32 = 5;
/// A tile splits into the level below while nearer than this many of its widths: a cell stays
/// about the same size on screen (a 32-block cell at 4096 blocks).
const SPLIT: f64 = 4.0;
/// Blocks under the land from where distant terrain is left out.
const UNDERGROUND: i32 = 32;
/// Tiles asked for and not back yet, at most.
const IN_FLIGHT: usize = 256;
const MAX_QUADS: usize = 48_000;
const MAX_MODEL_BOXES: usize = 6;
type Glow = (i32, i32, i32, [u8; 3]);

/// A tile: its level and place in the grid of its level's tiles.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileKey {
    pub level: u32,
    pub x: i32,
    pub z: i32,
}

impl TileKey {
    /// Width in blocks.
    pub fn size(self) -> i32 {
        CELLS << self.level
    }

    /// Lowest block corner (x, z).
    pub fn origin(self) -> IVec2 {
        IVec2::new(self.x, self.z) * self.size()
    }

    fn parent(self) -> TileKey {
        TileKey { level: self.level + 1, x: self.x.div_euclid(2), z: self.z.div_euclid(2) }
    }

    fn children(self) -> [TileKey; 4] {
        std::array::from_fn(|i| TileKey { level: self.level - 1, x: self.x * 2 + (i as i32 & 1), z: self.z * 2 + (i as i32 >> 1) })
    }

    /// Nearest and farthest horizontal distance from `p` (x, z) to it.
    fn distance(self, p: glam::DVec2) -> (f64, f64) {
        let lo = self.origin().as_dvec2();
        let hi = lo + glam::DVec2::splat(self.size() as f64);
        let near = (lo - p).max(p - hi).max(glam::DVec2::ZERO).length();
        let far = (p - lo).abs().max((p - hi).abs()).length();
        (near, far)
    }
}

/// A tile for the GPU to make.
pub struct LodJob {
    pub dim: u32,
    pub seed: u64,
    pub key: TileKey,
    pub done: Sender<LodMesh>,
    /// What the server said of columns, taken over the terrain's where the tile samples them.
    pub columns: Arc<Columns>,
}

/// A made tile's quads (`mesh`) and the heights they span.
pub struct LodMesh {
    pub dim: u32,
    pub seed: u64,
    pub key: TileKey,
    pub quads: Vec<[u32; 4]>,
    pub y: (i32, i32),
}

fn cuts_ground(c: &Content, block: u32) -> bool {
    c.states
        .get(block as usize)
        .is_some_and(|s| s.model == BlockModel::Cube && s.flags & bf::OPAQUE != 0)
}

fn ground_top(gy: i32, cuts: &[(i32, i32)]) -> i32 {
    match cuts.iter().map(|c| c.0).min() {
        Some(lo) => gy.min(lo - 1),
        None => gy,
    }
}

fn push_gap(
    out: &mut Vec<[u32; 4]>,
    dir: u32,
    x: i32,
    z: i32,
    from: i32,
    to: i32,
    cap_at: Option<i32>,
    taken: &[(i32, i32)],
    under: (u32, u32),
    cap: (u32, u32),
    biome: u32,
    light: [u8; 3],
    ao_wall: impl Fn(i32, i32) -> u32,
    ao_cap: impl Fn(i32) -> u32,
) {
    let (utex, utint) = under;
    let (ctex, ctint) = cap;
    let mut at = from;
    let mut gaps = Vec::new();
    for &(lo, hi) in taken {
        let lo = lo.max(from);
        let hi = (hi + 1).min(to);
        if hi <= at {
            continue;
        }
        if lo > at {
            gaps.push((at, lo));
        }
        at = at.max(hi);
    }
    if at < to {
        gaps.push((at, to));
    }
    for (a, b) in gaps {
        if a >= b {
            continue;
        }
        let Some(y) = cap_at.filter(|y| a <= *y && b > *y) else {
            out.push(quad(dir, x, a, z, (1, b - a, 1), utex, utint, biome, ao_wall(b, a), light));
            continue;
        };
        if a < y {
            out.push(quad(dir, x, a, z, (1, y - a, 1), utex, utint, biome, ao_wall(y, a), light));
        }
        out.push(quad(dir, x, y, z, (1, 1, 1), ctex, ctint, biome, ao_cap(y), light));
        if b > y + 1 {
            out.push(quad(dir, x, y + 1, z, (1, b - (y + 1), 1), utex, utint, biome, ao_wall(b, y + 1), light));
        }
    }
}

fn ground_sample(def: &DimensionDef, seed: u64, x: i32, z: i32) -> Option<(i32, u32, u32)> {
    let terrain = def.terrain.as_ref()?;
    let cols = terrain.columns(seed, x.div_euclid(16), z.div_euclid(16));
    let i = (z.rem_euclid(16) * 16 + x.rem_euclid(16)) as usize;
    let h = cols.cols[i].height;
    let rule = terrain.rules.get(cols.rules[i] as usize);
    let mut top = terrain.stone;
    let mut under = terrain.stone;
    let bare = rule.and_then(|r| r.bare_above).is_some_and(|y| h > y);
    if !bare {
        if let Some(b) = rule.and_then(|r| r.top) {
            top = b;
        }
        if let Some((b, below)) = rule.and_then(|r| r.deep_top) {
            if h < below {
                top = b;
            }
        }
        if rule.is_some_and(|r| r.depth > 0) {
            if let Some(b) = rule.and_then(|r| r.filler) {
                under = b;
            }
        }
    }
    let mut y = h;
    if let Some((sea, water)) = terrain.sea {
        if h < sea {
            y = sea;
            top = rule.and_then(|r| r.ice).unwrap_or(water);
            under = top;
        }
    }
    Some((y, top, under))
}

/// What came back for `job`, meshed on a worker.
pub fn made(job: LodJob, mut words: Vec<u32>) {
    background().spawn(move || {
        if !job.columns.is_empty() {
            let blocks = content();
            let (s, o) = (1 << job.key.level, job.key.origin());
            let mut uncanopy: FxHashSet<(i32, i32)> = FxHashSet::default();
            for (p, col) in job.columns.iter() {
                if col.spans.is_empty() {
                    continue;
                }
                let (i, j) = ((p.x - o.x).div_euclid(s), (p.y - o.y).div_euclid(s));
                uncanopy.insert((i, j));
            }
            let def = blocks.dims.get(job.dim as usize);
            for k in 0..SAMPLES {
                let (i, j) = ((k as i32 % SIDE) - 1, (k as i32 / SIDE) - 1);
                let at = o + IVec2::new(i * s + s / 2, j * s + s / 2);
                if let Some(col) = job.columns.get(&at) {
                    if s <= 2 && col.top != 0 && col.spans.is_empty() {
                        words[k * 2] = (col.y as u32 & 0xFFFF) | (col.top << 16);
                        words[k * 2 + 1] = (words[k * 2 + 1] & 0xFFFF) | (col.under << 16);
                    }
                }
                let (top, under) = (words[k * 2] >> 16, words[k * 2 + 1] >> 16);
                if uncanopy.contains(&(i, j)) && top == under && !shows(blocks, top) {
                    if let Some((y, top, under)) = def.and_then(|d| ground_sample(d, job.seed, at.x, at.y)) {
                        let cur = (words[k * 2] & 0xFFFF) as u16 as i16 as i32;
                        if cur > y {
                            words[k * 2] = (y as u32 & 0xFFFF) | (top << 16);
                            words[k * 2 + 1] = (words[k * 2 + 1] & 0xFFFF) | (under << 16);
                        }
                    }
                }
            }
        }
        let (quads, y) = mesh(content(), job.key, &words, &job.columns);
        let _ = job
            .done
            .send(LodMesh { dim: job.dim, seed: job.seed, key: job.key, quads, y });
    });
}

/// A record of `world.wgsl`'s `vs_lod`: the `dir` face of a box at (x, y, z) in the tile, size
/// (sx, sy, sz) blocks, drawn with texture `tex`, tinted for `biome` when `tint`, corners in `ao`.
fn quad(dir: u32, x: i32, y: i32, z: i32, size: (i32, i32, i32), tex: u32, tint: u32, biome: u32, ao: u32, light: [u8; 3]) -> [u32; 4] {
    let mut r = lod_record(dir, [x, y, z], [size.0, size.1, size.2], tex, tint, biome);
    let sy = (size.1.max(0) as u32).min(1023);
    r[2] = (r[2] & 0xFFFF) | (sy << 16);
    r[3] |= (ao & 0xFF) << 24;
    let (red, green, blue) = (light[0].min(15) as u32, light[1].min(15) as u32, light[2].min(15) as u32);
    let q = |v: u32| v / 2;
    r[2] |= (q(red) << 26) | (q(green) << 29);
    r[0] |= (q(blue) & 1) << 29;
    r[3] |= (red << lod_face::SIZE_BITS) | (green << 10) | ((blue & 3) << 14) | ((blue >> 2) << 22);
    r
}

fn push_top(
    out: &mut Vec<[u32; 4]>,
    lights: &[Glow],
    origin: IVec2,
    vary: bool,
    x: i32,
    y: i32,
    z: i32,
    sx: i32,
    sz: i32,
    tex: u32,
    tint: u32,
    biome: u32,
    ao: u32,
) {
    let sample = |dx: i32, dz: i32| face_light(lights, origin.x + x + dx, y + 1, y + 1, origin.y + z + dz);
    let (x0, x1, z0, z1) = (origin.x + x, origin.x + x + sx, origin.y + z, origin.y + z + sz);
    let hits = vary
        && lights.iter().any(|&(lx, ly, lz, rgb)| {
            let reach = rgb[0].max(rgb[1]).max(rgb[2]) as i32;
            if reach == 0 || x0 >= x1 || z0 >= z1 {
                return false;
            }
            let dx = if lx < x0 {
                x0 - lx
            } else if lx >= x1 {
                lx - (x1 - 1)
            } else {
                0
            };
            let dz = if lz < z0 {
                z0 - lz
            } else if lz >= z1 {
                lz - (z1 - 1)
            } else {
                0
            };
            let face = y + 1;
            let dy = if ly < face { face - ly } else { ly - face };
            dx + dz + dy < reach
        });
    if !hits {
        let light = if vary { [0, 0, 0] } else { sample(sx / 2, sz / 2) };
        out.push(quad(3, x, y, z, (sx, 1, sz), tex, tint, biome, ao, light));
        return;
    }
    for dz in 0..sz {
        let mut run = 0;
        let mut start = 0;
        let mut prev = [0u8; 3];
        for dx in 0..=sx {
            let on = dx < sx;
            let light = if on { sample(dx, dz) } else { [0, 0, 0] };
            if on && (run == 0 || light == prev) {
                if run == 0 {
                    start = dx;
                    prev = light;
                }
                run += 1;
            } else {
                if run > 0 {
                    out.push(quad(3, x + start, y, z + dz, (run, 1, 1), tex, tint, biome, ao, prev));
                    run = 0;
                }
                if on {
                    start = dx;
                    prev = light;
                    run = 1;
                }
            }
        }
    }
}

fn face_light(lights: &[Glow], x: i32, y0: i32, y1: i32, z: i32) -> [u8; 3] {
    let mut best = [0i32; 3];
    for &(lx, ly, lz, rgb) in lights {
        let reach = rgb[0].max(rgb[1]).max(rgb[2]) as i32;
        if reach == 0 {
            continue;
        }
        let d = (x - lx).abs()
            + (z - lz).abs()
            + if ly < y0 {
                y0 - ly
            } else if ly > y1 {
                ly - y1
            } else {
                0
            };
        if d < reach {
            let keep = reach - d;
            for i in 0..3 {
                best[i] = best[i].max(rgb[i] as i32 * keep / reach);
            }
        }
    }
    best.map(|v| v.min(15) as u8)
}

fn lights_near(c: &Content, columns: &Columns, x0: i32, z0: i32, size: i32) -> Vec<Glow> {
    let mut lights = Vec::new();
    for (p, col) in columns.iter() {
        if p.x < x0 - 16 || p.x >= x0 + size + 16 || p.y < z0 - 16 || p.y >= z0 + size + 16 {
            continue;
        }
        for span in &col.spans {
            let Some(st) = c.states.get(span.block as usize) else { continue };
            if st.light == [0, 0, 0] {
                continue;
            }
            lights.push((p.x, span.bottom, p.y, st.light));
            if span.top != span.bottom {
                lights.push((p.x, span.top, p.y, st.light));
            }
        }
    }
    lights
}

fn corner_y(words: &[u32], gi: i32, gj: i32) -> i32 {
    let gi = gi.clamp(-1, CELLS);
    let gj = gj.clamp(-1, CELLS);
    let k = ((gj + 1) * SIDE + gi + 1) as usize * 2;
    (words[k] & 0xFFFF) as u16 as i16 as i32
}

fn sample_y(words: &[u32], gx: f32, gz: f32) -> i32 {
    let i0 = gx.floor().clamp(-1.0, (CELLS - 1) as f32) as i32;
    let j0 = gz.floor().clamp(-1.0, (CELLS - 1) as f32) as i32;
    let fx = gx - i0 as f32;
    let fz = gz - j0 as f32;
    let h = |x: i32, z: i32| corner_y(words, x, z) as f32;
    let x = |z: i32| h(i0, z) * (1.0 - fx) + h(i0 + 1, z) * fx;
    (x(j0) * (1.0 - fz) + x(j0 + 1) * fz).round() as i32
}

/// The tile's top faces and the walls down to lower neighbors: the top block for its own
/// height, then the block under it for the rest.
pub fn mesh(c: &Content, key: TileKey, words: &[u32], columns: &Columns) -> (Vec<[u32; 4]>, (i32, i32)) {
    let s = 1 << key.level;
    let at = |i: i32, j: i32| {
        let k = ((j + 1) * SIDE + i + 1) as usize * 2;
        let (a, b) = (words[k], words[k + 1]);
        (a >> 16, b & 0xFFFF, b >> 16)
    };
    let h = height_grid(words);
    let origin = key.origin();
    let looks = super::looks::current();
    let ctm = super::ctm::current();
    let face_of = |state: u32, dir: usize| {
        let info = &c.states[state as usize];
        let tex = looks.drawn(state, info).textures[dir] & face::TEX_MASK;
        let paint = face_paint(info, dir, ctm.has_overlay(tex));
        let tint = if paint.base_tinted || paint.overlay_tinted { info.tint as u32 } else { 0 };
        (tex, tint)
    };
    let lights = lights_near(c, columns, origin.x, origin.y, CELLS * s);
    let taken_in = |i: i32, j: i32, dx: i32, dz: i32, from: i32, to: i32| {
        let p = IVec2::new(origin.x + i * s + dx, origin.y + j * s + dz);
        let mut v: Vec<(i32, i32)> = columns
            .get(&p)
            .map(|col| {
                col.spans
                    .iter()
                    .filter(|sp| sp.bottom < to && sp.top >= from && cuts_ground(c, sp.block))
                    .map(|sp| (sp.bottom, sp.top))
                    .collect()
            })
            .unwrap_or_default();
        v.sort_unstable();
        v
    };
    let saved = |i: i32, j: i32, dx: i32, dz: i32| {
        let p = IVec2::new(origin.x + i * s + dx, origin.y + j * s + dz);
        columns.get(&p).is_some_and(|col| !col.spans.is_empty())
    };
    let mut shaped = vec![false; (CELLS * CELLS) as usize];
    for p in columns.keys() {
        let (ci, cj) = ((p.x - origin.x).div_euclid(s), (p.y - origin.y).div_euclid(s));
        if (0..CELLS).contains(&ci) && (0..CELLS).contains(&cj) && columns.get(p).is_some_and(|col| !col.spans.is_empty()) {
            shaped[(cj * CELLS + ci) as usize] = true;
        }
    }
    let mut out = Vec::new();
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    for j in 0..CELLS {
        for i in 0..CELLS {
            let y = column(&h, i, j);
            let (top, biome, under) = at(i, j);
            let (tex, tint) = face_of(top, 3);
            let ao = ao_bits(&h, i, j, 3, y);
            let vary = s > 2;
            let mut hole: Vec<Option<(i32, Vec<(i32, i32)>)>> = Vec::new();
            if shaped[(j * CELLS + i) as usize] {
                hole.resize((s * s) as usize, None);
                for dz in 0..s {
                    for dx in 0..s {
                        if !saved(i, j, dx, dz) {
                            continue;
                        }
                        let cuts = taken_in(i, j, dx, dz, y - 512, y + 64);
                        let top_y = ground_top(y, &cuts);
                        if top_y < y {
                            hole[(dz * s + dx) as usize] = Some((top_y, cuts));
                        }
                    }
                }
            }
            if hole.iter().all(|h| h.is_none()) {
                push_top(&mut out, &lights, origin, vary, i * s, y, j * s, s, s, tex, tint, biome, ao);
            } else {
                let open = |dx: i32, dz: i32| hole[(dz * s + dx) as usize].is_none();
                for dz in 0..s {
                    let mut run = 0;
                    let mut start = 0;
                    for dx in 0..=s {
                        if dx < s && open(dx, dz) {
                            if run == 0 {
                                start = dx;
                            }
                            run += 1;
                        } else if run > 0 {
                            push_top(&mut out, &lights, origin, vary, i * s + start, y, j * s + dz, run, 1, tex, tint, biome, ao);
                            run = 0;
                        }
                    }
                }
                let under_state = if under == 0 { top } else { under };
                for dz in 0..s {
                    for dx in 0..s {
                        let Some((top_y, cuts)) = hole[(dz * s + dx) as usize].as_ref() else { continue };
                        let top_y = *top_y;
                        let (lx, lz) = (i * s + dx, j * s + dz);
                        let lit = |y0: i32, y1: i32| face_light(&lights, origin.x + lx, y0, y1, origin.y + lz);
                        let (ttex, ttint) = face_of(under_state, 3);
                        out.push(quad(
                            3,
                            lx,
                            top_y,
                            lz,
                            (1, 1, 1),
                            ttex,
                            ttint,
                            biome,
                            ao_bits(&h, i, j, 3, top_y),
                            lit(top_y + 1, top_y + 1),
                        ));
                        (lo, hi) = (lo.min(top_y), hi.max(top_y + 1));
                        for (dir, (di, dj)) in [(0usize, (-1, 0)), (1, (1, 0)), (4, (0, -1)), (5, (0, 1))] {
                            let (nx, nz) = (dx + di, dz + dj);
                            if !(0..s).contains(&nx) || !(0..s).contains(&nz) {
                                continue;
                            }
                            let n_surf = hole[(nz * s + nx) as usize]
                                .as_ref()
                                .map_or(y, |(ty, _)| *ty);
                            if n_surf <= top_y {
                                continue;
                            }
                            push_gap(
                                &mut out,
                                dir as u32,
                                lx,
                                lz,
                                top_y + 1,
                                n_surf + 1,
                                open(nx, nz).then_some(n_surf),
                                cuts,
                                face_of(under_state, dir),
                                face_of(top, dir),
                                biome,
                                lit(top_y + 1, n_surf),
                                |top, bot| ao_span(&h, i, j, dir, top, bot),
                                |at| ao_bits(&h, i, j, dir, at),
                            );
                        }
                    }
                }
            }
            (lo, hi) = (lo.min(y), hi.max(y + 1));
        }
    }
    for j in 0..CELLS {
        for i in 0..CELLS {
            let (top, biome, under) = at(i, j);
            let y = column(&h, i, j);
            for (dir, (di, dj)) in [(0usize, (-1, 0)), (1, (1, 0)), (4, (0, -1)), (5, (0, 1))] {
                // At the tile's edge the tile beside may be of another level, its ground not
                // where this one's sample says: the wall goes on down below it (a skirt), or
                // the sky shows through the crack.
                let (ni, nj) = (i + di, j + dj);
                let ny = if (-1..=CELLS).contains(&ni) && (-1..=CELLS).contains(&nj) { column(&h, ni, nj) } else { y.min(y - 1) - 2 * s };
                // Past a neighbor as high, real cubes may stand in for it lower than its sample
                // says (it is interpolated): a skirt down below that, or the sky shows through
                // between them.
                if ny > y {
                    continue;
                }
                let skirt = ny.min(y - 1) - s - 2;
                let (wx, wz) = (origin.x + i * s + s / 2, origin.y + j * s + s / 2);
                let light_at = |y0: i32, y1: i32| face_light(&lights, wx, y0, y1, wz);
                if ny == y && (dir == 0 || dir == 4) {
                    continue;
                }
                let edge = |t: i32| match dir {
                    0 => (0, t),
                    1 => (s - 1, t),
                    4 => (t, 0),
                    _ => (t, s - 1),
                };
                let mut along: Vec<Option<(i32, Vec<(i32, i32)>)>> = Vec::new();
                if shaped[(j * CELLS + i) as usize] {
                    along.resize(s as usize, None);
                    for t in 0..s {
                        let (dx, dz) = edge(t);
                        if !saved(i, j, dx, dz) {
                            continue;
                        }
                        let cuts = taken_in(i, j, dx, dz, y - 512, y + 64);
                        let top_y = ground_top(y, &cuts);
                        if top_y < y {
                            along[t as usize] = Some((top_y, cuts));
                        }
                    }
                }
                let base = skirt + 1;
                if along.iter().all(|c| c.is_none()) {
                    if ny < y {
                        let (tex, tint) = face_of(top, dir);
                        let ao = ao_bits(&h, i, j, dir, y);
                        let lit = light_at(y + 1, y + 1);
                        out.push(quad(dir as u32, i * s, y, j * s, (s, 1, s), tex, tint, biome, ao, lit));
                    }
                    let (tex, tint) = face_of(under, dir);
                    let top_y = if ny < y { y - 1 } else { y };
                    let ao = ao_span(&h, i, j, dir, top_y, (ny + 1).min(top_y));
                    let light = light_at(skirt + 1, y);
                    out.push(quad(dir as u32, i * s, base, j * s, (s, y - base, s), tex, tint, biome, ao, light));
                } else {
                    let (utex, utint) = face_of(under, dir);
                    let (ctex, ctint) = face_of(top, dir);
                    let side_top = if ny < y { y + 1 } else { y };
                    let mut run = 0;
                    let mut start = 0;
                    for t in 0..=s {
                        let open = t < s && along[t as usize].is_none();
                        if open {
                            if run == 0 {
                                start = t;
                            }
                            run += 1;
                            continue;
                        }
                        if run > 0 {
                            let (x, z, sx, sz) = match dir {
                                0 | 1 => (i * s, j * s + start, s, run),
                                _ => (i * s + start, j * s, run, s),
                            };
                            if ny < y {
                                let ao = ao_bits(&h, i, j, dir, y);
                                let lit = light_at(y + 1, y + 1);
                                out.push(quad(dir as u32, x, y, z, (sx, 1, sz), ctex, ctint, biome, ao, lit));
                            }
                            let ao = ao_span(&h, i, j, dir, y, base);
                            let light = light_at(base, y);
                            out.push(quad(dir as u32, x, base, z, (sx, y - base, sz), utex, utint, biome, ao, light));
                            run = 0;
                        }
                        if t >= s {
                            break;
                        }
                        let (top_y, cuts) = along[t as usize].as_ref().unwrap();
                        let top_y = *top_y;
                        let (x, z) = match dir {
                            0 | 1 => (i * s, j * s + t),
                            _ => (i * s + t, j * s),
                        };
                        let body = top_y + 1;
                        if body > base {
                            let light = light_at(base, body);
                            out.push(quad(
                                dir as u32,
                                x,
                                base,
                                z,
                                (1, body - base, 1),
                                utex,
                                utint,
                                biome,
                                ao_span(&h, i, j, dir, body, base),
                                light,
                            ));
                        }
                        push_gap(
                            &mut out,
                            dir as u32,
                            x,
                            z,
                            top_y + 1,
                            side_top,
                            (ny < y).then_some(y),
                            cuts,
                            (utex, utint),
                            (ctex, ctint),
                            biome,
                            light_at(top_y + 1, side_top),
                            |top, bot| ao_span(&h, i, j, dir, top, bot),
                            |at| ao_bits(&h, i, j, dir, at),
                        );
                    }
                }
                lo = lo.min(base);
            }
        }
    }
    add_structures(&mut out, &mut lo, &mut hi, c, key, words, columns, &h, &face_of, &lights);
    (out, (lo, hi))
}

fn height_grid(words: &[u32]) -> [[i32; GRID]; GRID] {
    let mut h = [[0i32; GRID]; GRID];
    for j in -1..=CELLS {
        for i in -1..=CELLS {
            let y = if i < CELLS && j < CELLS {
                sample_y(words, i as f32 + 0.5, j as f32 + 0.5)
            } else {
                let (i0, fx) = if i < CELLS { (i, 0.5) } else { (CELLS, 0.0) };
                let (j0, fz) = if j < CELLS { (j, 0.5) } else { (CELLS, 0.0) };
                let s = |x: i32, z: i32| corner_y(words, x, z) as f32;
                let a = s(i0, j0) * (1.0 - fx) + s((i0 + 1).min(CELLS), j0) * fx;
                let b = s(i0, (j0 + 1).min(CELLS)) * (1.0 - fx) + s((i0 + 1).min(CELLS), (j0 + 1).min(CELLS)) * fx;
                (a * (1.0 - fz) + b * fz).round() as i32
            };
            h[(i + 1) as usize][(j + 1) as usize] = y;
        }
    }
    h
}

fn column(h: &[[i32; GRID]; GRID], i: i32, j: i32) -> i32 {
    h[(i + 1).clamp(0, CELLS + 1) as usize][(j + 1).clamp(0, CELLS + 1) as usize]
}

fn ao_bits(h: &[[i32; GRID]; GRID], i: i32, j: i32, dir: usize, by: i32) -> u32 {
    let nrm = FACE_DIRS[dir];
    let axis = dir / 2;
    let (ta, tb) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let base = [i + nrm[0], by + nrm[1], j + nrm[2]];
    let mut bits = 0u32;
    for (k, corner) in CORNERS[dir].iter().enumerate() {
        let sa = corner[ta] * 2 - 1;
        let sb = corner[tb] * 2 - 1;
        let mut p1 = base;
        let mut p2 = base;
        let mut p3 = base;
        p1[ta] += sa;
        p2[tb] += sb;
        p3[ta] += sa;
        p3[tb] += sb;
        let solid = |p: [i32; 3]| column(h, p[0], p[2]) >= p[1];
        let (s1, s2, s3) = (solid(p1), solid(p2), solid(p3));
        let ao = if s1 && s2 { 0 } else { 3 - u32::from(s1) - u32::from(s2) - u32::from(s3) };
        bits |= ao << (2 * k);
    }
    bits
}

fn ao_span(h: &[[i32; GRID]; GRID], i: i32, j: i32, dir: usize, top: i32, bot: i32) -> u32 {
    let hi = ao_bits(h, i, j, dir, top);
    let lo = ao_bits(h, i, j, dir, bot);
    let mut bits = 0u32;
    for (k, c) in CORNERS[dir].iter().enumerate() {
        let src = if c[1] == 1 { hi } else { lo };
        bits |= ((src >> (2 * k)) & 3) << (2 * k);
    }
    bits
}

fn exposed(lo: i32, hi: i32, cover: &[Span]) -> Vec<(i32, i32)> {
    let mut segs = vec![(lo, hi)];
    for span in cover {
        let (a, b) = (span.bottom, span.top);
        let mut next = Vec::new();
        for (s, e) in segs {
            if b < s || a > e {
                next.push((s, e));
            } else {
                if s < a {
                    next.push((s, a - 1));
                }
                if e > b {
                    next.push((b + 1, e));
                }
            }
        }
        segs = next;
    }
    segs
}

fn model_boxes(st: &StateInfo) -> Vec<([u32; 3], [u32; 3], [u32; 6])> {
    let mut raw: Vec<([u32; 3], [u32; 3], [u32; 6])> = st
        .boxes
        .iter()
        .map(|b| {
            let edge = |v: f32| (v * 16.0).round().clamp(0.0, 16.0) as u32;
            (b.min.map(edge), b.max.map(edge), b.textures)
        })
        .filter(|(mn, mx, _)| mn.iter().zip(mx).all(|(a, c)| a < c))
        .collect();
    if raw.len() > MAX_MODEL_BOXES {
        let mut mn = [16u32; 3];
        let mut mx = [0u32; 3];
        for (a, c, _) in &raw {
            for i in 0..3 {
                mn[i] = mn[i].min(a[i]);
                mx[i] = mx[i].max(c[i]);
            }
        }
        let tex = raw[0].2;
        raw = vec![(mn, mx, tex)];
    }
    raw
}

fn hides_face(c: &Content, block: u32, neighbor: u32) -> bool {
    let Some(n) = c.states.get(neighbor as usize) else { return false };
    if n.flags & bf::OPAQUE != 0 {
        return true;
    }
    c.states
        .get(block as usize)
        .is_some_and(|s| n.flags & bf::CULL_SAME != 0 && n.block == s.block)
}

fn push_partial(
    out: &mut Vec<[u32; 4]>,
    dir: u32,
    x: i32,
    y: i32,
    z: i32,
    mn: [u32; 3],
    mx: [u32; 3],
    extrude: bool,
    height: i32,
    tex: u32,
    tint: u32,
    biome: u32,
    ao: u32,
    light: [u8; 3],
) {
    let mut r = quad(dir, x, y, z, (1, if extrude { height } else { 1 }, 1), tex, tint, biome, ao, light);
    r[0] |= 1 << lod_face::PARTIAL_SHIFT;
    if extrude {
        r[0] |= 1 << lod_face::EXTRUDE_SHIFT;
    } else {
        r[2] = (r[2] & 0xFC000000) | (r[2] & 0xFFFF) | mn[1] << 16 | mx[1] << 21;
    }
    let lit = r[3] & (3 << 22);
    r[3] = mn[0] | mx[0] << 5 | mn[2] << 10 | mx[2] << 15 | lit | (ao & 0xFF) << 24;
    out.push(r);
}

fn add_structures(
    out: &mut Vec<[u32; 4]>,
    lo: &mut i32,
    hi: &mut i32,
    c: &Content,
    key: TileKey,
    words: &[u32],
    columns: &Columns,
    h: &[[i32; GRID]; GRID],
    face_of: &impl Fn(u32, usize) -> (u32, u32),
    lights: &[Glow],
) {
    if columns.is_empty() {
        return;
    }
    let s = 1 << key.level;
    let origin = key.origin();
    let (x0, z0) = (origin.x, origin.y);
    let (x1, z1) = (x0 + CELLS * s, z0 + CELLS * s);
    let mut here: Vec<IVec2> = columns
        .iter()
        .filter(|(p, col)| !col.spans.is_empty() && p.x >= x0 && p.x < x1 && p.y >= z0 && p.y < z1)
        .map(|(p, _)| *p)
        .collect();
    if here.is_empty() {
        return;
    }
    let land_top = |col: &Column, cell_y: i32| {
        let cuts: Vec<(i32, i32)> = col
            .spans
            .iter()
            .filter(|sp| sp.bottom <= cell_y && cuts_ground(c, sp.block))
            .map(|sp| (sp.bottom, sp.top))
            .collect();
        ground_top(cell_y, &cuts)
    };
    let (cx, cz) = (x0 + CELLS * s / 2, z0 + CELLS * s / 2);
    here.sort_by_key(|p| (p.x - cx).abs() + (p.y - cz).abs());
    for p in here {
        if out.len() >= MAX_QUADS {
            return;
        }
        let Some(col) = columns.get(&p) else { continue };
        let (i, j) = ((p.x - x0) / s, (p.y - z0) / s);
        let k = ((j + 1) * SIDE + i + 1) as usize * 2;
        let biome = words[k + 1] & 0xFFFF;
        let cell_y = column(h, i, j);
        let our_top = land_top(col, cell_y);
        let skirt_hi = |np: IVec2, ni: i32, nj: i32| {
            let n_cell = column(h, ni, nj);
            if ni == i && nj == j || n_cell < cell_y {
                return i32::MIN;
            }
            let their = columns
                .get(&np)
                .map(|n| land_top(n, n_cell))
                .unwrap_or(n_cell);
            if n_cell > cell_y { their } else { their - 1 }
        };
        let (lx, lz) = (p.x - x0, p.y - z0);
        for span in &col.spans {
            let (bottom, span_hi, block) = (span.bottom, span.top, span.block);
            if bottom > span_hi {
                continue;
            }
            let shaped = c
                .states
                .get(block as usize)
                .is_some_and(|s| s.model == BlockModel::Boxes);
            if shaped {
                let boxes = model_boxes(&c.states[block as usize]);
                for (mn, mx, textures) in boxes {
                    let height = span_hi - bottom + 1;
                    let extrude = mn[1] == 0 && mx[1] == 16 && height > 1;
                    let mut y = bottom;
                    while y <= span_hi {
                        let top = if extrude { span_hi } else { y };
                        for dir in 0..6 {
                            let at_edge = match dir {
                                0 => mn[0] == 0,
                                1 => mx[0] == 16,
                                2 => mn[1] == 0,
                                3 => mx[1] == 16,
                                4 => mn[2] == 0,
                                5 => mx[2] == 16,
                                _ => false,
                            };
                            if at_edge {
                                let hidden = match dir {
                                    2 => {
                                        y <= our_top + 1
                                            || col
                                                .spans
                                                .iter()
                                                .any(|s| s.bottom <= y - 1 && s.top >= y - 1 && hides_face(c, block, s.block))
                                    }
                                    3 => {
                                        (y <= our_top && mx[1] == 16)
                                            || col
                                                .spans
                                                .iter()
                                                .any(|s| s.bottom <= top + 1 && s.top >= top + 1 && hides_face(c, block, s.block))
                                    }
                                    _ => {
                                        let (dx, dz) = [(-1, 0), (1, 0), (0, 0), (0, 0), (0, -1), (0, 1)][dir];
                                        let np = IVec2::new(p.x + dx, p.y + dz);
                                        let (ni, nj) = ((np.x - x0).div_euclid(s), (np.y - z0).div_euclid(s));
                                        let hide = skirt_hi(np, ni, nj);
                                        let beside = columns.get(&np).map(|n| n.spans.as_slice()).unwrap_or(&[]);
                                        (hide != i32::MIN && top <= hide)
                                            || beside
                                                .iter()
                                                .any(|s| s.bottom <= y && s.top >= top && hides_face(c, block, s.block))
                                    }
                                };
                                if hidden {
                                    continue;
                                }
                            }
                            let (fb, tint) = face_of(block, dir);
                            let own = textures[dir] & face::TEX_MASK;
                            let tex = if own == 0 { fb } else { own };
                            let ao = ao_bits(h, i, j, dir, top);
                            let ly = if dir == 3 { top + 1 } else { y };
                            let light = face_light(lights, p.x, ly, if extrude { span_hi + 1 } else { ly }, p.y);
                            push_partial(out, dir as u32, lx, y, lz, mn, mx, extrude, height, tex, tint, biome, ao, light);
                        }
                        if extrude {
                            break;
                        }
                        y += 1;
                    }
                }
            } else {
                if !col
                    .spans
                    .iter()
                    .any(|s| s.bottom == span_hi + 1 && hides_face(c, block, s.block))
                {
                    let (tex, tint) = face_of(block, 3);
                    let light = face_light(lights, p.x, span_hi + 1, span_hi + 1, p.y);
                    out.push(quad(3, lx, span_hi, lz, (1, 1, 1), tex, tint, biome, ao_bits(h, i, j, 3, span_hi), light));
                }
                let held = col
                    .spans
                    .iter()
                    .any(|s| s.top + 1 == bottom && hides_face(c, block, s.block));
                if !held && bottom > our_top + 1 {
                    let (tex, tint) = face_of(block, 2);
                    let light = face_light(lights, p.x, bottom, bottom, p.y);
                    out.push(quad(2, lx, bottom, lz, (1, 1, 1), tex, tint, biome, ao_bits(h, i, j, 2, bottom), light));
                }
                for (dir, dx, dz) in [(0usize, -1, 0), (1, 1, 0), (4, 0, -1), (5, 0, 1)] {
                    let (tex, tint) = face_of(block, dir);
                    let np = IVec2::new(p.x + dx, p.y + dz);
                    let beside = columns.get(&np).map(|n| n.spans.as_slice()).unwrap_or(&[]);
                    let mut cover: Vec<Span> = beside
                        .iter()
                        .copied()
                        .filter(|s| hides_face(c, block, s.block))
                        .collect();
                    let (ni, nj) = ((np.x - x0).div_euclid(s), (np.y - z0).div_euclid(s));
                    let n_top = skirt_hi(np, ni, nj);
                    let dirt = words[k + 1] >> 16;
                    if dirt != 0 && n_top != i32::MIN {
                        cover.push(Span { bottom: n_top.saturating_sub(4096), top: n_top, block: dirt });
                    }
                    for (s0, e0) in exposed(bottom, span_hi, &cover) {
                        let ao = ao_span(h, i, j, dir, e0 + 1, s0);
                        let light = face_light(lights, p.x, s0, e0 + 1, p.y);
                        out.push(quad(dir as u32, lx, s0, lz, (1, e0 - s0 + 1, 1), tex, tint, biome, ao, light));
                    }
                }
            }
            *lo = (*lo).min(bottom);
            *hi = (*hi).max(span_hi + 1);
            if out.len() >= MAX_QUADS {
                return;
            }
        }
    }
}

/// The client's distant terrain: which tiles to show, asking for missing ones, taking made ones.
pub struct Lod {
    /// The land's height at the camera's column, kept while it stays in it.
    ground: Option<((u32, IVec2), i32)>,
    jobs: Sender<LodJob>,
    done: (Sender<LodMesh>, Receiver<LodMesh>),
    dim: u32,
    seed: u64,
    /// Whether the GPU makes this dimension's terrain (`terrain_gpu`); if not, there is no
    /// distant terrain here.
    able: bool,
    ready: FxHashSet<TileKey>,
    pending: FxHashSet<TileKey>,
    /// Tiles wanted, nearest first, and where the camera was then.
    wanted: Vec<TileKey>,
    wanted_at: Option<(glam::DVec2, i32, i32)>,
    /// A bit per cube column around the camera's (`MASK_MARGIN` past the view, addressed
    /// modulo the window's width so it does not move with the camera): its cubes show its
    /// ground. Its half width and center, and whether it changed since uploaded.
    mask: Vec<u32>,
    half: i32,
    center: IVec2,
    mask_changed: bool,
    /// What the server said of columns of `columns_dim`.
    columns: Arc<Columns>,
    columns_dim: u32,
    /// Made tiles to make again for columns that changed, shown as they are till then.
    stale: FxHashSet<TileKey>,
}

/// Columns the mask reaches past the view radius (cubes are kept that far).
const MASK_MARGIN: i32 = 3;

impl Lod {
    pub fn new(jobs: Sender<LodJob>) -> Lod {
        Lod {
            ground: None,
            jobs,
            done: unbounded(),
            dim: u32::MAX,
            seed: 0,
            able: false,
            ready: FxHashSet::default(),
            pending: FxHashSet::default(),
            wanted: Vec::new(),
            wanted_at: None,
            mask: Vec::new(),
            half: 0,
            center: IVec2::ZERO,
            mask_changed: false,
            columns: Arc::default(),
            columns_dim: u32::MAX,
            stale: FxHashSet::default(),
        }
    }

    /// `ToClient::LodColumns`: tiles with those columns are made again.
    pub fn set_columns(&mut self, dim: u32, all: bool, cols: &[(i32, i32, i32, u32, u32, Vec<Span>)]) {
        if all || dim != self.columns_dim {
            self.columns = Arc::default();
            self.columns_dim = dim;
            self.stale.extend(self.ready.iter().copied());
            self.stale.extend(self.pending.iter().copied());
        }
        let blocks = content();
        let map = Arc::make_mut(&mut self.columns);
        for (x, z, y, top, under, spans) in cols {
            let p = IVec2::new(*x, *z);
            let spans: Vec<Span> = spans
                .iter()
                .copied()
                .filter(|s| drawn(blocks, s.block))
                .collect();
            if *top == u32::MAX || (*top == 0 && spans.is_empty()) {
                map.remove(&p);
            } else {
                map.insert(p, Column { y: *y, top: *top, under: *under, spans });
            }
            if !all {
                // Every level's tile with it, and the ones beside that sample it for their walls.
                for level in 0..=MAX_LEVEL {
                    let size = CELLS << level;
                    for dz in -1..=1 {
                        for dx in -1..=1 {
                            let key = TileKey {
                                level,
                                x: (x + dx * (size / CELLS)).div_euclid(size),
                                z: (z + dz * (size / CELLS)).div_euclid(size),
                            };
                            if self.ready.contains(&key) || self.pending.contains(&key) {
                                self.stale.insert(key);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn columns_len(&self) -> usize {
        self.columns.len()
    }

    /// Tiles asked for and not back yet.
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// Distance in blocks it reaches (`core:lod_distance` cubes), 0 when off.
    pub fn distance() -> i32 {
        let c = content();
        c.game
            .config_names
            .get("core:lod_distance")
            .map_or(0, |&id| config::get(id) as i32)
            * 16
    }

    /// Picks, asks for and hands the renderer this frame's tiles. `view`: the view radius in
    /// cubes (inside it the real cubes are drawn).
    pub fn update(
        &mut self,
        gpu: &mut dyn weft_render::Backend,
        render: &mut super::world_render::WorldRenderer,
        world: &mut super::world::ClientWorld,
        textures: &super::textures::GameTextures,
        seed: u64,
        cam: DVec3,
        view: i32,
    ) {
        let c = content();
        let dim = world.dim;
        self.update_mask(gpu, render, world, seed, cam, view);
        let reach = Self::distance();
        if (dim, seed) != (self.dim, self.seed) || reach == 0 {
            for key in self.ready.drain() {
                render.remove_lod(key);
            }
            self.pending.clear();
            self.wanted.clear();
            self.wanted_at = None;
        }
        if (dim, seed) != (self.dim, self.seed) {
            (self.dim, self.seed) = (dim, seed);
            self.able = super::terrain_gpu::describe_lod(c, dim, seed).is_some();
        }
        // Tiles stand in for cubes not shown yet; the inner half of the view is shown long
        // before the edge, where flying outruns the cubes.
        let mut hide = (view / 2 * 16).max(0);
        // High up, the view's cubes don't reach the ground: tiles have to fill that in too (where
        // cubes do show, the mask leaves them out).
        if self
            .land_height(c, cam)
            .is_some_and(|land| cam.y > (land + view * 8) as f64)
        {
            hide = 0;
        }
        while let Ok(m) = self.done.1.try_recv() {
            if (m.dim, m.seed) != (self.dim, self.seed) || !self.pending.remove(&m.key) {
                continue;
            }
            render.upload_lod(gpu, textures, m.key, &m.quads, m.y);
            self.ready.insert(m.key);
        }
        if reach == 0 || !self.able {
            render.lod_draw.clear();
            return;
        }
        let p = glam::DVec2::new(cam.x, cam.z);
        if self
            .wanted_at
            .is_none_or(|(at, h, r)| (at - p).length() > 16.0 || h != hide || r != reach)
        {
            self.wanted_at = Some((p, hide, reach));
            self.pick(p, hide as f64, reach as f64);
        }
        // Shown: each wanted tile when made, else its children when all four are, else its
        // nearest made ancestor.
        let mut shown = FxHashSet::default();
        let mut keep = FxHashSet::default();
        for &key in &self.wanted {
            keep.insert(key);
            if self.ready.contains(&key) {
                shown.insert(key);
                continue;
            }
            if key.level > 1 && key.children().iter().all(|k| self.ready.contains(k)) {
                shown.extend(key.children());
                keep.extend(key.children());
                continue;
            }
            let mut a = key;
            while a.level < MAX_LEVEL {
                a = a.parent();
                if self.ready.contains(&a) {
                    shown.insert(a);
                    keep.insert(a);
                    break;
                }
            }
        }
        render.lod_draw.clear();
        // It is a shell over the land with nothing under it: from deep below, its walls would hang
        // in the sky. Only rock is that way anyway.
        if !self.deep_underground(c, cam) {
            render.lod_draw.extend(shown);
        }
        let gone: Vec<TileKey> = self
            .ready
            .iter()
            .copied()
            .filter(|k| !keep.contains(k))
            .collect();
        for key in gone {
            self.ready.remove(&key);
            render.remove_lod(key);
        }
        self.request();
    }

    /// Well under the land at the camera's column, so its sky can't be seen. Near the surface a cave
    /// mouth or a ravine still looks out over the distance.
    fn deep_underground(&mut self, c: &Content, cam: DVec3) -> bool {
        self.land_height(c, cam)
            .is_some_and(|land| cam.y < (land - UNDERGROUND) as f64)
    }

    /// How high the land is at the camera's column.
    fn land_height(&mut self, c: &Content, cam: DVec3) -> Option<i32> {
        let at = IVec2::new(cam.x.floor() as i32, cam.z.floor() as i32);
        if self.ground.is_none_or(|g| g.0 != (self.dim, at)) {
            let height = c.dims[self.dim as usize]
                .column(self.seed, at.x, at.y)
                .map_or(i32::MIN, |col| col.height);
            self.ground = Some(((self.dim, at), height));
        }
        self.ground.map(|g| g.1).filter(|&h| h != i32::MIN)
    }

    /// Keeps the mask of columns shown by cubes current: columns that changed, and the ones
    /// coming into the window when the camera moved to another column.
    fn update_mask(
        &mut self,
        gpu: &mut dyn weft_render::Backend,
        render: &mut super::world_render::WorldRenderer,
        world: &mut super::world::ClientWorld,
        seed: u64,
        cam: DVec3,
        view: i32,
    ) {
        let half = view + MASK_MARGIN;
        let w = 2 * half + 1;
        let center = IVec2::new((cam.x / 16.0).floor() as i32, (cam.z / 16.0).floor() as i32);
        let slot = |col: IVec2| (col.y.rem_euclid(w) * w + col.x.rem_euclid(w)) as usize;
        let set = |mask: &mut Vec<u32>, col: IVec2, on: bool| {
            let i = slot(col);
            let (word, bit) = (i / 32, 1u32 << (i % 32));
            let old = mask[word];
            mask[word] = if on { old | bit } else { old & !bit };
            old != mask[word]
        };
        let changed = world.take_changed_columns();
        let inside = |col: IVec2, at: IVec2| (col - at).abs().max_element() <= half;
        if half != self.half || self.mask.is_empty() {
            self.half = half;
            self.mask = vec![0; ((w * w) as usize).div_ceil(32)];
            self.center = center;
            for z in -half..=half {
                for x in -half..=half {
                    let col = center + IVec2::new(x, z);
                    set(&mut self.mask, col, world.column_shown(col, seed));
                }
            }
            self.mask_changed = true;
        } else if center != self.center {
            let old = self.center;
            self.center = center;
            for z in -half..=half {
                for x in -half..=half {
                    let col = center + IVec2::new(x, z);
                    if !inside(col, old) {
                        self.mask_changed |= set(&mut self.mask, col, world.column_shown(col, seed));
                    }
                }
            }
        }
        for col in changed {
            if inside(col, center) {
                self.mask_changed |= set(&mut self.mask, col, world.column_shown(col, seed));
            }
        }
        render.lod_half = half as u32;
        if self.mask_changed {
            self.mask_changed = false;
            render.set_lod_mask(gpu, &self.mask);
        }
    }

    /// The tiles to show: level 5 tiles within `reach`, split while near, leaving out what
    /// lies inside `hide` (where the view's cubes are always shown).
    fn pick(&mut self, p: glam::DVec2, hide: f64, reach: f64) {
        let top = CELLS << MAX_LEVEL;
        let (lo, hi) = (((p - reach) / top as f64).floor().as_ivec2(), ((p + reach) / top as f64).floor().as_ivec2());
        let mut out = Vec::new();
        let mut stack: Vec<TileKey> = Vec::new();
        for z in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                stack.push(TileKey { level: MAX_LEVEL, x, z });
            }
        }
        while let Some(key) = stack.pop() {
            let (near, far) = key.distance(p);
            if near > reach || far < hide {
                continue;
            }
            if key.level > 1 && near < SPLIT * key.size() as f64 {
                stack.extend(key.children());
            } else {
                out.push((near, key));
            }
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.wanted = out.into_iter().map(|t| t.1).collect();
    }

    /// Asks for the nearest wanted tiles not made yet, or made before columns changed.
    fn request(&mut self) {
        let columns = if self.columns_dim == self.dim { self.columns.clone() } else { Arc::default() };
        for &key in &self.wanted {
            if self.pending.len() >= IN_FLIGHT {
                break;
            }
            if self.pending.contains(&key) {
                continue;
            }
            let again = self.stale.remove(&key);
            if self.ready.contains(&key) && !again {
                continue;
            }
            self.pending.insert(key);
            let _ = self
                .jobs
                .send(LodJob { dim: self.dim, seed: self.seed, key, done: self.done.0.clone(), columns: columns.clone() });
        }
        self.stale
            .retain(|k| self.ready.contains(k) || self.pending.contains(k));
    }
}
