//! Entity serialization for saves. Kinds, items and components are stored by name so a save
//! survives mods being added, removed or reordered.

use glam::{DVec3, IVec3};
use weft_abi::{ItemStack, component_flags as cf};

use super::entity::Entity;
use crate::content::Content;
use crate::net::codec::{self, R};

/// An entity read back: itself, its inventory, components and, for a moving structure, its
/// blocks.
pub type Saved = (Entity, Option<Vec<ItemStack>>, Vec<(u32, Box<[u8]>)>, Option<Vec<(IVec3, u32)>>);
pub type ToSave<'a> = (&'a Entity, Option<&'a Vec<ItemStack>>, Vec<(u32, &'a [u8])>, Option<Vec<(IVec3, String)>>);

struct W<'a>(&'a mut Vec<u8>);

impl W<'_> {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u16(s.len() as u16);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn stack(&mut self, c: &Content, s: ItemStack) {
        if s.is_empty() {
            self.str("");
            return;
        }
        self.str(&c.items[s.item as usize].name);
        self.u32(s.count);
        let mut named = codec::W::default();
        crate::stack::put_named(&mut named, crate::stack::for_save(s.data));
        self.0.extend_from_slice(&named.0);
    }
}

impl R<'_> {
    fn str(&mut self) -> Option<String> {
        let n = self.u16()? as usize;
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn stack(&mut self, c: &Content) -> Option<ItemStack> {
        let name = self.str()?;
        if name.is_empty() {
            return Some(ItemStack::EMPTY);
        }
        let count = self.u32()?;
        let data = crate::stack::get_named(self)?;
        // Items of removed mods are dropped rather than turned into something else.
        Some(
            c.item_names
                .get(&name)
                .map_or(ItemStack::EMPTY, |&i| ItemStack { item: i, count, data }),
        )
    }
}

pub fn write(c: &Content, out: &mut Vec<u8>, list: &[ToSave<'_>]) {
    let mut w = W(out);
    w.u32(list.len() as u32);
    for (e, inv, comps, blocks) in list {
        w.str(&c.game.entities[e.kind as usize].name);
        w.u64(e.id);
        for v in e.pos.to_array().into_iter().chain(e.vel.to_array()) {
            w.f64(v);
        }
        w.f32(e.yaw);
        w.f32(e.pitch);
        w.f32(e.health);
        w.u64(e.age);
        w.stack(c, e.item);
        // 2: on a body, which it says.
        match (e.anchor, e.block_frame()) {
            (Some(a), frame) => {
                w.u8(if frame != 0 { 2 } else { 1 });
                for v in a.to_array() {
                    w.u32(v as u32);
                }
                if frame != 0 {
                    w.u64(frame);
                }
            }
            (None, _) => w.u8(0),
        }
        match inv {
            Some(inv) => {
                w.u16(inv.len() as u16);
                for s in inv.iter() {
                    w.stack(c, *s);
                }
            }
            None => w.u16(u16::MAX),
        }
        let saved: Vec<&(u32, &[u8])> = comps
            .iter()
            .filter(|(id, _)| c.game.components[*id as usize].flags & cf::SAVE != 0)
            .collect();
        w.u16(saved.len() as u16);
        for (id, bytes) in saved {
            w.str(&c.game.components[*id as usize].name);
            let named = crate::stack::holds_stacks(c, *id).then(|| crate::stack::contents_to_names(c, bytes));
            let bytes = named.as_deref().unwrap_or(bytes);
            // Component values over 64 KB say so with a marker, older saves never have it.
            if bytes.len() >= u16::MAX as usize {
                w.u16(u16::MAX);
                w.u32(bytes.len() as u32);
            } else {
                w.u16(bytes.len() as u16);
            }
            w.0.extend_from_slice(bytes);
        }
        match blocks {
            Some(list) => {
                w.u32(list.len() as u32);
                for (p, name) in list {
                    for v in p.to_array() {
                        w.u32(v as u32);
                    }
                    w.str(name);
                }
            }
            None => w.u32(u32::MAX),
        }
    }
}

/// Marks the trailer of a cube's saved data that holds its pending block ticks: (position in the
/// cube, ticks left) pairs, their count, this. Older saves simply don't end in it.
const TICKS_MAGIC: u32 = 0x4B43_4954;

pub fn append_ticks(out: &mut Vec<u8>, ticks: &[(u16, u32)]) {
    for (at, left) in ticks {
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&left.to_le_bytes());
    }
    out.extend_from_slice(&(ticks.len() as u32).to_le_bytes());
    out.extend_from_slice(&TICKS_MAGIC.to_le_bytes());
}

pub fn read_ticks(data: &[u8]) -> Vec<(u16, u32)> {
    let word = |at: usize| {
        data.get(at..at + 4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
    };
    let Some(end) = data.len().checked_sub(8) else { return Vec::new() };
    if word(end + 4) != Some(TICKS_MAGIC) {
        return Vec::new();
    }
    let n = word(end).unwrap_or(0) as usize;
    let Some(start) = end.checked_sub(n * 6) else { return Vec::new() };
    data[start..end]
        .chunks_exact(6)
        .map(|p| (u16::from_le_bytes([p[0], p[1]]), u32::from_le_bytes([p[2], p[3], p[4], p[5]])))
        .collect()
}

pub fn read(c: &Content, data: &[u8]) -> Vec<Saved> {
    let mut r = R(data);
    let mut out = Vec::new();
    let Some(n) = r.u32() else { return out };
    for _ in 0..n {
        let Some(e) = read_one(c, &mut r) else { break };
        if let Some(e) = e {
            out.push(e);
        }
    }
    out
}

fn read_one(c: &Content, r: &mut R) -> Option<Option<Saved>> {
    let kind_name = r.str()?;
    let id = r.u64()?;
    let mut v = [0.0f64; 6];
    for x in v.iter_mut() {
        *x = r.f64()?;
    }
    let yaw = r.f32()?;
    let pitch = r.f32()?;
    let health = r.f32()?;
    let age = r.u64()?;
    let item = r.stack(c)?;
    let anchor_kind = r.u8()?;
    let anchor = if anchor_kind != 0 { Some(IVec3::new(r.u32()? as i32, r.u32()? as i32, r.u32()? as i32)) } else { None };
    let frame = if anchor_kind == 2 { Some(r.u64()?) } else { None };
    let inv_len = r.u16()?;
    let inv = if inv_len == u16::MAX {
        None
    } else {
        let mut v = Vec::with_capacity(inv_len as usize);
        for _ in 0..inv_len {
            v.push(r.stack(c)?);
        }
        Some(v)
    };
    let nc = r.u16()?;
    let mut comps = Vec::new();
    for _ in 0..nc {
        let name = r.str()?;
        let len = r.u16()? as usize;
        let len = if len == u16::MAX as usize { r.u32()? as usize } else { len };
        let bytes = r.take(len)?.to_vec();
        if let Some(id) = c.game.component(&name) {
            let bytes = if crate::stack::holds_stacks(c, id) { crate::stack::contents_from_names(c, &bytes) } else { bytes };
            comps.push((id, c.game.fit_component(id, bytes)));
        }
    }
    let nb = r.u32()?;
    let blocks = if nb == u32::MAX {
        None
    } else {
        let mut v = Vec::with_capacity(nb as usize);
        for _ in 0..nb {
            let p = IVec3::new(r.u32()? as i32, r.u32()? as i32, r.u32()? as i32);
            // Blocks of removed mods are left out.
            if let Some(s) = c.state_by_name(&r.str()?) {
                v.push((p, s));
            }
        }
        Some(v)
    };
    let Some(&kind) = c.game.entity_names.get(&kind_name) else { return Some(None) };
    let mut e = Entity::new(id, kind, DVec3::new(v[0], v[1], v[2]), health);
    e.vel = DVec3::new(v[3], v[4], v[5]);
    e.yaw = yaw;
    e.pitch = pitch;
    e.age = age;
    e.item = item;
    e.anchor = anchor;
    e.frame = frame;
    if kind == c.game.item_kind && item.is_empty() {
        return Some(None);
    }
    Some(Some((e, inv, comps, blocks)))
}
