//! What distant terrain needs of the cubes that are saved: the rest generates the same again, so
//! the client's tiles take it from the terrain formula, and only columns whose saved cubes make
//! their top differ from it (a town, a crater, a tower) are kept here and sent. Per saved cube,
//! each of its columns' highest solid block; a column's top then comes from those and the formula
//! for the cubes of it that aren't saved.

use std::collections::BTreeMap;
use std::path::Path;

use glam::{IVec2, IVec3};
use rustc_hash::FxHashMap;

use crate::content::Content;
use crate::world::Cube;

/// A column whose top is not the terrain's: its height, top block and the one under it.
/// `top` `u32::MAX`: as the terrain says again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodColumn {
    pub x: i32,
    pub z: i32,
    pub y: i32,
    pub top: u32,
    pub under: u32,
}

/// One column of a saved cube: its highest block that shows from afar (local y, `NONE` for
/// none), that block and the one under it.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Top {
    y: u8,
    top: u32,
    under: u32,
}

const NONE: u8 = u8::MAX;

/// A dimension's saved cubes' tops, by cube column then cube height.
#[derive(Default)]
pub struct DimTops {
    cubes: FxHashMap<IVec2, BTreeMap<i32, Box<[Top; 256]>>>,
    /// Columns whose top differs from the terrain's, as last worked out.
    pub columns: FxHashMap<IVec2, LodColumn>,
    /// Columns that changed since `take_changed`.
    changed: Vec<IVec2>,
    dirty: bool,
}

/// Whether a block shows as the top from afar: solid and opaque (not leaves, glass or a plant),
/// or a liquid.
fn shows(c: &Content, s: u32) -> bool {
    use weft_abi::block_flags as bf;
    let st = &c.states[s as usize];
    s != 0 && (st.liquid.is_some() || (st.flags & bf::SOLID != 0 && st.flags & bf::OPAQUE != 0))
}

fn tops_of(c: &Content, cube: &Cube) -> Box<[Top; 256]> {
    let mut out = Box::new([Top { y: NONE, top: 0, under: 0 }; 256]);
    for z in 0..16 {
        for x in 0..16 {
            for y in (0..16).rev() {
                let s = cube.get(crate::world::cube::index(x, y, z));
                if shows(c, s) {
                    let under = if y > 0 { cube.get(crate::world::cube::index(x, y - 1, z)) } else { s };
                    out[z * 16 + x] = Top { y: y as u8, top: s, under: if under == 0 { s } else { under } };
                    break;
                }
            }
        }
    }
    out
}

impl DimTops {
    /// A cube was saved as `cube`.
    pub fn saved(&mut self, c: &Content, dim: u32, seed: u64, pos: IVec3, cube: &Cube) {
        let tops = tops_of(c, cube);
        let col = IVec2::new(pos.x, pos.z);
        let list = self.cubes.entry(col).or_default();
        if list.get(&pos.y).is_some_and(|t| **t == *tops) {
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
                    Some(tops) if tops[i].y != NONE => {
                        let tp = tops[i];
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
            let p = IVec2::new(x, z);
            // Nothing found: its ground cube isn't saved, the terrain's own top stands.
            let differs =
                here.filter(|&(y, top, _)| !(y == fy && (top == ftop || ftop == u32::MAX && c.states[top as usize].liquid.is_some())));
            let before = self.columns.get(&p).copied();
            match differs {
                Some((y, top, under)) => {
                    let now = LodColumn { x, z, y, top, under };
                    if before != Some(now) {
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
                    .copied()
                    .unwrap_or(LodColumn { x: p.x, z: p.y, y: 0, top: u32::MAX, under: 0 })
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
        for (col, list) in &self.cubes {
            for (y, tops) in list {
                for v in [col.x, *y, col.y] {
                    b.extend_from_slice(&v.to_le_bytes());
                }
                for t in tops.iter() {
                    b.push(t.y);
                    if t.y != NONE {
                        b.extend_from_slice(&world_id(t.top).to_le_bytes());
                        b.extend_from_slice(&world_id(t.under).to_le_bytes());
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
        let mut r = &b[..];
        let i32_ = |r: &mut &[u8]| -> Option<i32> {
            let (v, rest) = r.split_first_chunk::<4>()?;
            *r = rest;
            Some(i32::from_le_bytes(*v))
        };
        'cubes: while !r.is_empty() {
            let (Some(x), Some(y), Some(z)) = (i32_(&mut r), i32_(&mut r), i32_(&mut r)) else { break };
            let mut tops = Box::new([Top::default(); 256]);
            for t in tops.iter_mut() {
                let Some((&ty, rest)) = r.split_first() else { break 'cubes };
                r = rest;
                t.y = ty;
                if ty != NONE {
                    let (Some(a), Some(u)) = (i32_(&mut r), i32_(&mut r)) else { break 'cubes };
                    (t.top, t.under) = (runtime_id(a as u32), runtime_id(u as u32));
                }
            }
            d.cubes.entry(IVec2::new(x, z)).or_default().insert(y, tops);
        }
        let cols: Vec<IVec2> = d.cubes.keys().copied().collect();
        for col in cols {
            d.refresh(c, dim, seed, col);
        }
        d.changed.clear();
        d
    }
}
