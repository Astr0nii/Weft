//! Distant terrain beyond the view: made on the GPU straight from the dimension's terrain at a
//! coarser step (`shaders/terrain.wgsl`'s `lod`), so it reaches as far as wanted for a few
//! kilobytes a tile. A tile is 32x32 cells of 2^level blocks; each cell is the column at its
//! middle: its top block, the one under it, its biome, and tree canopies over it. Where saved
//! cubes made a column's top other than the terrain's (a town, a crater), the server says so
//! (`server::lod_tops`) and that column is taken instead.
//! Tiles are picked by distance, nearer ones split into the four of the level below; one is
//! shown once made, its parent or children standing in until then. Where a column of real cubes
//! shows its ground the tiles' pixels are dropped (`fs_lod`, `Lod::mask`), so they fill in for
//! cubes not made or meshed yet (flying fast), and nowhere stand through them.

use std::sync::Arc;

use crossbeam_channel::{Receiver, Sender, unbounded};
use glam::{DVec3, IVec2};
use rustc_hash::{FxHashMap, FxHashSet};

/// Columns whose top isn't the terrain's: height, top block, the one under it.
pub type Columns = FxHashMap<IVec2, (i32, u32, u32)>;

use crate::content::{Content, content};
use crate::gpu_layout::face;

/// Cells a tile side.
pub const CELLS: i32 = 32;
/// Columns a tile is made from: its cells and one around them (for the walls at its edges).
pub const SAMPLES: usize = crate::gpu_layout::terrain::lod::SAMPLES as usize;
const SIDE: i32 = crate::gpu_layout::terrain::lod::SIDE as i32;
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

/// What came back for `job`, meshed on a worker.
pub fn made(job: LodJob, mut words: Vec<u32>) {
    crate::background().spawn(move || {
        if !job.columns.is_empty() {
            let (s, o) = (1 << job.key.level, job.key.origin());
            for k in 0..SAMPLES {
                let (i, j) = ((k as i32 % SIDE) - 1, (k as i32 / SIDE) - 1);
                let col = o + IVec2::new(i * s + s / 2, j * s + s / 2);
                if let Some(&(y, top, under)) = job.columns.get(&col) {
                    words[k * 2] = (y as u32 & 0xFFFF) | (top << 16);
                    words[k * 2 + 1] = (words[k * 2 + 1] & 0xFFFF) | (under << 16);
                }
            }
        }
        let (quads, y) = mesh(content(), job.key, &words);
        let _ = job
            .done
            .send(LodMesh { dim: job.dim, seed: job.seed, key: job.key, quads, y });
    });
}

/// A record of `world.wgsl`'s `vs_lod`: the `dir` face of a box at (x, y, z) in the tile, size
/// (sx, sy, sz) blocks, drawn in the average color of texture `tex` (its smallest mip), tinted
/// for `biome` when `tint`.
fn quad(dir: u32, x: i32, y: i32, z: i32, size: (i32, i32, i32), tex: u32, tint: u32, biome: u32, ao: u32) -> [u32; 4] {
    let mut r = crate::gpu_layout::lod_record(dir, [x, y, z], [size.0, size.1, size.2], tex, tint, biome);
    r[3] |= (ao & 0xFF) << 24;
    r
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

/// The tile's top faces (runs along x merged) and the walls down to lower neighbors: the top
/// block's top color for its own height (a grass side averages to the dirt under its rim, and
/// every slope came out striped brown), then the block under it for the rest.
pub fn mesh(c: &Content, key: TileKey, words: &[u32]) -> (Vec<[u32; 4]>, (i32, i32)) {
    let s = 1 << key.level;
    let at = |i: i32, j: i32| {
        let k = ((j + 1) * SIDE + i + 1) as usize * 2;
        let (a, b) = (words[k], words[k + 1]);
        (a >> 16, b & 0xFFFF, b >> 16)
    };
    let h = height_grid(words);
    let looks = super::looks::current();
    let ctm = super::ctm::current();
    let face_of = |state: u32, dir: usize| {
        let info = &c.states[state as usize];
        // No overlays out there.
        let tex = looks.drawn(state, info).textures[dir] & face::TEX_MASK;
        let paint = crate::content::face_paint(info, dir, ctm.has_overlay(tex));
        let tint = if paint.base_tinted || paint.overlay_tinted { info.tint as u32 } else { 0 };
        (tex, tint)
    };
    let mut out = Vec::new();
    let (mut lo, mut hi) = (i32::MAX, i32::MIN);
    for j in 0..CELLS {
        for i in 0..CELLS {
            let y = column(&h, i, j);
            let (top, biome, _) = at(i, j);
            let (tex, tint) = face_of(top, 3);
            out.push(quad(3, i * s, y, j * s, (s, 1, s), tex, tint, biome, ao_bits(&h, i, j, 3, y)));
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
                let ny = if (-1..=CELLS).contains(&ni) && (-1..=CELLS).contains(&nj) {
                    column(&h, ni, nj)
                } else {
                    y.min(y - 1) - 2 * s
                };
                // Past a neighbor as high, real cubes may stand in for it lower than its sample
                // says (it is interpolated): a skirt down below that, or the sky shows through
                // between them.
                if ny > y {
                    continue;
                }
                let skirt = ny.min(y - 1) - s - 2;
                if ny < y {
                    let (tex, tint) = face_of(top, dir);
                    out.push(quad(dir as u32, i * s, y, j * s, (s, 1, s), tex, tint, biome, ao_bits(&h, i, j, dir, y)));
                }
                let (tex, tint) = face_of(under, dir);
                let top_y = if ny < y { y - 1 } else { y };
                let ao = ao_span(&h, i, j, dir, top_y, (ny + 1).min(top_y));
                out.push(quad(dir as u32, i * s, skirt + 1, j * s, (s, y - 1 - skirt, s), tex, tint, biome, ao));
                lo = lo.min(skirt + 1);
            }
        }
    }
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
    let nrm = weft_abi::FACE_DIRS[dir];
    let axis = dir / 2;
    let (ta, tb) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let base = [i + nrm[0], by + nrm[1], j + nrm[2]];
    let mut bits = 0u32;
    for (k, corner) in crate::gpu_layout::CORNERS[dir].iter().enumerate() {
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
    for (k, c) in crate::gpu_layout::CORNERS[dir].iter().enumerate() {
        let src = if c[1] == 1 { hi } else { lo };
        bits |= ((src >> (2 * k)) & 3) << (2 * k);
    }
    bits
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
    pub fn set_columns(&mut self, dim: u32, all: bool, cols: &[(i32, i32, i32, u32, u32)]) {
        if all || dim != self.columns_dim {
            self.columns = Arc::default();
            self.columns_dim = dim;
            self.stale.extend(self.ready.iter().copied());
        }
        let map = Arc::make_mut(&mut self.columns);
        for &(x, z, y, top, under) in cols {
            let p = IVec2::new(x, z);
            if top == u32::MAX {
                map.remove(&p);
            } else {
                map.insert(p, (y, top, under));
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
                            if self.ready.contains(&key) {
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
            .map_or(0, |&id| crate::config::get(id) as i32)
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
            self.able = super::terrain_gpu::describe(c, dim, seed).is_some();
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
            let again = self.stale.remove(&key);
            if (self.ready.contains(&key) && !again) || self.pending.contains(&key) {
                continue;
            }
            self.pending.insert(key);
            let _ = self
                .jobs
                .send(LodJob { dim: self.dim, seed: self.seed, key, done: self.done.0.clone(), columns: columns.clone() });
        }
        self.stale.retain(|k| self.ready.contains(k));
    }
}
