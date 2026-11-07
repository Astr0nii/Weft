//! What distant terrain needs of the cubes that are saved: the rest generates the same again, so
//! the client's tiles take it from the terrain formula, and only columns whose saved cubes make
//! their top differ from it (a town, a crater, a tower) are kept here and sent. Per saved cube,

use std::collections::BTreeMap;
use std::path::Path;

use glam::{IVec2, IVec3};
use rustc_hash::FxHashMap;

use weft_abi::BlockModel;
use weft_abi::block_flags as bf;

use crate::content::Content;
use crate::world::Cube;
use crate::world::cube::index;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub bottom: i32,
    pub top: i32,
    pub block: u32,
}

/// A column whose top is not the terrain's: its height, top block and the one under it.
/// `top` `u32::MAX`: as the terrain says again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LodColumn {
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub top: u32,
    pub under: u32,
    pub spans: Vec<Span>,
}

/// One column of a saved cube: its highest block that shows from afar (local y, `NONE` for
/// none), that block and the one under it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Top {
    y: u8,
    top: u32,
    under: u32,
}

#[derive(Clone, PartialEq, Eq)]
struct CubeTops {
    tops: Box<[Top; 256]>,
    at: Box<[u16; 257]>,
    runs: Vec<Span>,
}

const NONE: u8 = u8::MAX;
const MAX_RUNS: usize = 6;
const MAX_SPANS: usize = 8;
const MAGIC: &[u8] = b"LOD3";
const MAGIC_V2: &[u8] = b"LOD2";

/// A dimension's saved cubes' tops, by cube column then cube height.
#[derive(Default)]
pub struct DimTops {
    cubes: FxHashMap<IVec2, BTreeMap<i32, CubeTops>>,
    /// Columns whose top differs from the terrain's, as last worked out.
    pub columns: FxHashMap<IVec2, LodColumn>,
    /// Columns that changed since `take_changed`.
    changed: Vec<IVec2>,
    dirty: bool,
    rebuild: bool,
}

pub fn shows(c: &Content, s: u32) -> bool {
    let Some(st) = c.states.get(s as usize) else { return false };
    s != 0 && (st.liquid.is_some() || (st.flags & bf::SOLID != 0 && st.flags & bf::OPAQUE != 0))
}

pub fn drawn(c: &Content, s: u32) -> bool {
    let Some(st) = c.states.get(s as usize) else { return false };
    if st.liquid.is_some() {
        return true;
    }
    s != 0
        && st.flags & bf::REPLACEABLE == 0
        && match st.model {
            BlockModel::Cube => true,
            BlockModel::Boxes => !st.boxes.is_empty(),
            _ => false,
        }
}

fn tops_of(c: &Content, cube: &Cube) -> CubeTops {
    let mut tops = Box::new([Top { y: NONE, top: 0, under: 0 }; 256]);
    let mut at = Box::new([0u16; 257]);
    let mut runs = Vec::new();
    for z in 0..16 {
        for x in 0..16 {
            let i = z * 16 + x;
            at[i] = runs.len() as u16;
            let mut local = [Span { bottom: 0, top: 0, block: 0 }; MAX_RUNS];
            let mut n = 0;
            let mut y = 0;
            while y < 16 {
                let s = cube.get(index(x, y, z));
                if !drawn(c, s) {
                    y += 1;
                    continue;
                }
                let lo = y;
                y += 1;
                while y < 16 && cube.get(index(x, y, z)) == s {
                    y += 1;
                }
                if n == MAX_RUNS {
                    local.copy_within(1..MAX_RUNS, 0);
                    n = MAX_RUNS - 1;
                }
                local[n] = Span { bottom: lo as i32, top: y as i32 - 1, block: s };
                n += 1;
            }
            runs.extend_from_slice(&local[..n]);
            for y in (0..16).rev() {
                let s = cube.get(index(x, y, z));
                if shows(c, s) {
                    let under = if y > 0 { cube.get(index(x, y - 1, z)) } else { s };
                    tops[i] = Top { y: y as u8, top: s, under: if under == 0 { s } else { under } };
                    break;
                }
            }
        }
    }
    at[256] = runs.len() as u16;
    CubeTops { tops, at, runs }
}

impl DimTops {
    /// A cube was saved as `cube`.
    pub fn saved(&mut self, c: &Content, dim: u32, seed: u64, pos: IVec3, cube: &Cube) {
        let tops = tops_of(c, cube);
        let col = IVec2::new(pos.x, pos.z);
        let list = self.cubes.entry(col).or_default();
        if list.get(&pos.y).is_some_and(|t| *t == tops) {
            return;
        }
        list.insert(pos.y, tops);
        self.dirty = true;
        self.refresh(c, dim, seed, col);
    }

    /// Works out the 256 columns of cube column `col` again.
    fn refresh(&mut self, c: &Content, dim: u32, seed: u64, col: IVec2) {
        let def = &c.dims[dim as usize];
        let Some(list) = self.cubes.get(&col) else { return };
        let sea = def.sea_level();
        for i in 0..256 {
            let (x, z) = (col.x * 16 + (i % 16) as i32, col.y * 16 + (i / 16) as i32);
            let Some(t) = def.column(seed, x, z) else { continue };
            // As distant terrain shows it untouched: the ground, or the sea over it.
            let (fy, ftop) = match sea {
                Some(s) if t.height < s => (s, u32::MAX),
                _ => (t.height, def.surface_block(seed, x, z)),
            };
            let ground = t.height.div_euclid(16);
            let lowest = list.keys().next().copied().unwrap_or(ground);
            let highest = list
                .keys()
                .next_back()
                .copied()
                .unwrap_or(ground)
                .max(ground);
            // From the highest saved cube down: what is in it; where one isn't saved above the
            // ground, air; at or under it, the terrain, solid up to its ground.
            let mut here = None;
            for cy in (lowest.min(ground) - 1..=highest).rev() {
                match list.get(&cy) {
                    Some(tops) if tops.tops[i].y != NONE => {
                        let tp = tops.tops[i];
                        here = Some((cy * 16 + tp.y as i32, tp.top, tp.under));
                        break;
                    }
                    Some(_) => {}
                    None if cy == ground => break,
                    None if cy < ground => {
                        let stone = def.terrain.as_ref().map_or(0, |t| t.stone);
                        here = Some((cy * 16 + 15, stone, stone));
                        break;
                    }
                    None => {}
                }
            }
            let mut spans: Vec<Span> = Vec::new();
            for cy in lowest..=highest {
                let Some(tops) = list.get(&cy) else { continue };
                let runs = &tops.runs[tops.at[i] as usize..tops.at[i + 1] as usize];
                for r in runs {
                    let bottom = cy * 16 + r.bottom;
                    let top = cy * 16 + r.top;
                    let extend = spans
                        .last()
                        .is_some_and(|prev| prev.top + 1 == bottom && prev.block == r.block);
                    if extend {
                        spans.last_mut().unwrap().top = top;
                    } else {
                        spans.push(Span { bottom, top, block: r.block });
                    }
                }
            }
            let mut above = Vec::new();
            for span in spans {
                if span.top <= fy {
                    continue;
                }
                above.push(Span { bottom: span.bottom.max(fy), ..span });
            }
            if above.len() > MAX_SPANS {
                above.drain(0..above.len() - MAX_SPANS);
            }
            let spans = above;
            let p = IVec2::new(x, z);
            // Nothing found: its ground cube isn't saved, the terrain's own top stands.
            let differs =
                here.filter(|&(y, top, _)| !(y == fy && (top == ftop || ftop == u32::MAX && c.states[top as usize].liquid.is_some())));
            let before = self.columns.get(&p).cloned();
            match differs {
                Some((y, top, under)) => {
                    let now = LodColumn { x, z, y, top, under, spans };
                    if before.as_ref() != Some(&now) {
                        self.columns.insert(p, now);
                        self.changed.push(p);
                    }
                }
                None if !spans.is_empty() => {
                    let now = LodColumn { x, z, y: fy, top: 0, under: 0, spans };
                    if before.as_ref() != Some(&now) {
                        self.columns.insert(p, now);
                        self.changed.push(p);
                    }
                }
                None => {
                    if self.columns.remove(&p).is_some() {
                        self.changed.push(p);
                    }
                }
            }
        }
    }

    /// Columns changed since the last call, as they are now (`top` `u32::MAX` for gone).
    pub fn take_changed(&mut self) -> Vec<LodColumn> {
        let mut ps = std::mem::take(&mut self.changed);
        ps.sort_unstable_by_key(|p| (p.x, p.y));
        ps.dedup();
        ps.into_iter()
            .map(|p| {
                self.columns
                    .get(&p)
                    .cloned()
                    .unwrap_or(LodColumn { x: p.x, z: p.y, y: 0, top: u32::MAX, under: 0, spans: Vec::new() })
            })
            .collect()
    }

    /// Written by world ids, lz4 packed.
    pub fn write(&mut self, path: &Path, world_id: &mut impl FnMut(u32) -> u32) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(MAGIC);
        for (col, list) in &self.cubes {
            for (y, tops) in list {
                for v in [col.x, *y, col.y] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                for i in 0..256 {
                    let t = tops.tops[i];
                    b.push(t.y);
                    if t.y != NONE {
                        b.extend_from_slice(&world_id(t.top).to_le_bytes());
                        b.extend_from_slice(&world_id(t.under).to_le_bytes());
                    }
                    let rs = &tops.runs[tops.at[i] as usize..tops.at[i + 1] as usize];
                    b.push(rs.len() as u8);
                    for r in rs {
                        b.push(r.bottom as u8);
                        b.push(r.top as u8);
                        b.extend_from_slice(&world_id(r.block).to_le_bytes());
                    }
                }
            }
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(path, lz4_flex::compress_prepend_size(&b)) {
            log::error!("saving {}: {e}", path.display());
        }
    }

    /// What `write` wrote, its columns worked out.
    pub fn read(path: &Path, c: &Content, dim: u32, seed: u64, runtime_id: &mut impl FnMut(u32) -> u32) -> DimTops {
        let mut d = DimTops::default();
        let Some(b) = std::fs::read(path)
            .ok()
            .and_then(|raw| lz4_flex::decompress_size_prepended(&raw).ok())
        else {
            return d;
        };
        let v3 = b.starts_with(MAGIC);
        let v2 = b.starts_with(MAGIC_V2);
        let mut r = if v3 || v2 { &b[4..] } else { &b[..] };
        let i32_ = |r: &mut &[u8]| -> Option<i32> {
            let (v, rest) = r.split_first_chunk::<4>()?;
            *r = rest;
            Some(i32::from_le_bytes(*v))
        };
        let u8_ = |r: &mut &[u8]| -> Option<u8> {
            let (v, rest) = r.split_first()?;
            *r = rest;
            Some(*v)
        };
        'cubes: while !r.is_empty() {
            let (Some(x), Some(y), Some(z)) = (i32_(&mut r), i32_(&mut r), i32_(&mut r)) else { break };
            let mut tops = Box::new([Top { y: NONE, top: 0, under: 0 }; 256]);
            let mut at = Box::new([0u16; 257]);
            let mut runs = Vec::new();
            for i in 0..256 {
                at[i] = runs.len() as u16;
                let Some(ty) = u8_(&mut r) else { break 'cubes };
                tops[i].y = ty;
                if ty != NONE {
                    let (Some(a), Some(u)) = (i32_(&mut r), i32_(&mut r)) else { break 'cubes };
                    (tops[i].top, tops[i].under) = (runtime_id(a as u32), runtime_id(u as u32));
                }
                if v3 || (v2 && ty != NONE) {
                    let Some(n) = u8_(&mut r) else { break 'cubes };
                    for _ in 0..n {
                        let (Some(lo), Some(hi), Some(block)) = (u8_(&mut r), u8_(&mut r), i32_(&mut r)) else {
                            break 'cubes;
                        };
                        runs.push(Span { bottom: lo as i32, top: hi as i32, block: runtime_id(block as u32) });
                    }
                }
            }
            at[256] = runs.len() as u16;
            d.cubes
                .entry(IVec2::new(x, z))
                .or_default()
                .insert(y, CubeTops { tops, at, runs });
        }
        let cols: Vec<IVec2> = d.cubes.keys().copied().collect();
        for col in cols {
            d.refresh(c, dim, seed, col);
        }
        d.changed.clear();
        d.rebuild = !v3 && !d.cubes.is_empty();
        d
    }

    pub fn take_rebuild(&mut self, c: &Content, dim: u32, seed: u64) -> Vec<IVec3> {
        if !self.rebuild {
            return Vec::new();
        }
        self.rebuild = false;
        let def = &c.dims[dim as usize];
        let mut cols: Vec<IVec2> = self
            .columns
            .keys()
            .map(|p| IVec2::new(p.x.div_euclid(16), p.y.div_euclid(16)))
            .collect();
        cols.sort_unstable_by_key(|p| (p.x, p.y));
        cols.dedup();
        let mut out = Vec::new();
        for cc in cols {
            let Some(list) = self.cubes.get(&cc) else { continue };
            let mut low = i32::MAX;
            for dz in 0..16 {
                for dx in 0..16 {
                    if let Some(t) = def.column(seed, cc.x * 16 + dx, cc.y * 16 + dz) {
                        low = low.min(t.height);
                    }
                }
            }
            let min_y = if low == i32::MAX { i32::MIN } else { low.div_euclid(16) - 1 };
            for &y in list.keys() {
                if y >= min_y {
                    out.push(IVec3::new(cc.x, y, cc.y));
                }
            }
        }
        out
    }
}
