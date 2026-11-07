//! The shape of redstone wire, the same for the server's simulation and the client's picture. A
//! wire block lies on any of a cell's six faces, a face being named by the direction of the block
//! holding it up. Nothing here knows which way is up, so a build works the same whichever way it
//! is turned.

use glam::IVec3;
use weft_abi::{block_flags as bf, signal_kind as sk};

use crate::content::{Content, Signal, StateInfo};
use crate::world::{Cube, DIRS};

/// A direction's name as `facing` properties have it.
pub fn dir_name(d: u8) -> &'static str {
    weft_abi::FACE_NAMES[d as usize % 6]
}

/// The faces a wire state lies on.
pub fn mask(c: &Content, state: u32) -> u8 {
    c.states[state as usize].sides
}

/// Whether wire can't pass through a block on its way past an edge.
pub fn in_the_way(c: &Content, state: u32) -> bool {
    let s = &c.states[state as usize];
    s.flags & bf::SOLID != 0 || !matches!(s.signal.kind, sk::NONE | sk::WIRE)
}

/// Whether wire can lie on a block.
pub fn holds_wire(c: &Content, state: u32) -> bool {
    c.states[state as usize].flags & bf::SOLID != 0
}

/// Whether a block carries power from what feeds it to what touches it.
pub fn conducts(c: &Content, state: u32) -> bool {
    let s = &c.states[state as usize];
    s.flags & bf::SOLID != 0 && s.flags & bf::SIGNAL_BLOCKER == 0 && s.signal.kind == sk::NONE
}

/// Whether a kind of signal block is a device, something other than wire with a circuit.
pub fn is_device(kind: u32) -> bool {
    !matches!(kind, sk::NONE | sk::WIRE)
}

/// Whether going from one state to the other leaves wire around it as it is: the same block turned
/// the same way (a gate going on or off, water falling a level). Only wire itself changes.
pub fn same_shape(a: &StateInfo, b: &StateInfo) -> bool {
    a.block == b.block && a.signal.kind != sk::WIRE && a.signal.front == b.signal.front && a.signal.attach == b.signal.attach
}

/// `same_shape` of a device: it keeps its circuit, changing its level.
pub fn same_device(a: &StateInfo, b: &StateInfo) -> bool {
    is_device(a.signal.kind) && is_device(b.signal.kind) && same_shape(a, b)
}

/// The two directions to the side of a device: a comparator's and a gate's other inputs.
pub fn sides(s: &Signal) -> [u8; 2] {
    let up = if s.attach < 6 { -DIRS[s.attach as usize] } else { IVec3::Y };
    let right = DIRS.get(s.front as usize).map_or(IVec3::X, |f| f.cross(up));
    let r = DIRS.iter().position(|d| *d == right).unwrap_or(0) as u8;
    [r ^ 1, r]
}

/// The direction a gate's face (`abi::gate_side`) looks.
pub fn gate_dir(s: &Signal, side: u32) -> u8 {
    use weft_abi::gate_side as g;
    let bottom = if s.attach < 6 { s.attach } else { 2 };
    let [left, right] = sides(s);
    match side {
        g::FRONT => s.front,
        g::REAR => s.front ^ 1,
        g::LEFT => left,
        g::RIGHT => right,
        g::TOP => bottom ^ 1,
        _ => bottom,
    }
}

/// Whether a device gives what is next to it in direction `dir`.
pub fn gives(s: &Signal, dir: u8) -> bool {
    match s.kind {
        sk::SWITCH => true,
        sk::TORCH => dir != s.attach,
        sk::REPEATER | sk::COMPARATOR | sk::GATE => dir == s.front,
        sk::OBSERVER => dir ^ 1 == s.front,
        _ => false,
    }
}

/// Whether a device takes what comes from direction `dir`.
pub fn reads(s: &Signal, dir: u8) -> bool {
    match s.kind {
        sk::TORCH => dir == s.attach,
        sk::REPEATER => dir == s.front ^ 1,
        sk::COMPARATOR => dir == s.front ^ 1 || sides(s).contains(&dir),
        sk::GATE => (0..6).any(|i| s.inputs & (1 << i) != 0 && gate_dir(s, i) == dir),
        sk::SINK => true,
        // Their front is stuck to what they take along, or is where the head goes.
        sk::FLOATER | sk::PISTON => dir != s.front,
        _ => false,
    }
}

/// Whether a cube has wire.
pub fn has_wire(c: &Content, cube: &Cube) -> bool {
    cube.palette()
        .iter()
        .any(|&s| c.states[s as usize].signal.kind == sk::WIRE)
}

/// The cells with wire in the cube at `pos`.
pub fn cells(c: &Content, pos: IVec3, cube: &Cube) -> Vec<IVec3> {
    if !has_wire(c, cube) {
        return Vec::new();
    }
    (0..4096usize)
        .filter(|&i| c.states[cube.get(i) as usize].signal.kind == sk::WIRE)
        .map(|i| pos * 16 + crate::world::cube::offset(i))
        .collect()
}

/// The four directions along a face, which are its edges.
pub fn edges(face: u8) -> impl Iterator<Item = u8> {
    (0..6u8).filter(move |d| d / 2 != face / 2)
}

/// What lies past an edge of a piece of wire.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Link {
    /// Another piece of the same wire: `(cell, face)`.
    Piece(IVec3, u8),
    /// No wire goes on from here.
    End,
    /// Past it isn't loaded.
    Unknown,
}

/// Follows edge `e` of the piece on `face` of `cell`: into the corner on the wall at that edge,
/// on along the same plane, or around the edge of the block it lies on, in that order.
pub fn link(c: &Content, get: &impl Fn(IVec3) -> Option<u32>, cell: IVec3, face: u8, e: u8) -> Link {
    let Some(here) = get(cell) else { return Link::Unknown };
    let block = c.states[here as usize].block;
    if mask(c, here) & (1 << e) != 0 {
        return Link::Piece(cell, e);
    }
    let same = |s: u32, f: u8| c.states[s as usize].block == block && mask(c, s) & (1 << f) != 0;
    let next = cell + DIRS[e as usize];
    let Some(ahead) = get(next) else { return Link::Unknown };
    if same(ahead, face) {
        return Link::Piece(next, face);
    }
    if in_the_way(c, ahead) {
        return Link::End;
    }
    let around = next + DIRS[face as usize];
    let Some(below) = get(around) else { return Link::Unknown };
    if same(below, e ^ 1) {
        return Link::Piece(around, e ^ 1);
    }
    Link::End
}

/// Whether the piece on `face` of a cell touches what is next to that cell in direction `e`: not
/// from the far side of the cell.
pub fn touches(face: u8, e: u8) -> bool {
    e != face ^ 1
}

/// What a broken piece leaves behind: `face` is the side of the selection that was hit.
pub fn after_break(c: &Content, state: u32, face: u8) -> u32 {
    let m = mask(c, state);
    if m == 0 {
        return crate::content::AIR;
    }
    // A flat piece is hit on its broad side, which looks away from what holds it up.
    let hit = if face < 6 && m & (1 << (face ^ 1)) != 0 { face ^ 1 } else { m.trailing_zeros() as u8 };
    let left = m & !(1 << hit);
    if left == 0 { crate::content::AIR } else { c.block_of(state).with_sides(state, left) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_are_along_the_face() {
        for f in 0..6u8 {
            let e: Vec<u8> = edges(f).collect();
            assert_eq!(e.len(), 4);
            assert!(!e.contains(&f) && !e.contains(&(f ^ 1)));
        }
    }

    fn device(kind: u32, front: u8, attach: u8) -> Signal {
        Signal { kind, front, attach, ..Signal::default() }
    }

    #[test]
    fn what_a_device_takes_and_gives() {
        // On the floor, its front to the north.
        let gate = Signal { inputs: 0b1110, ..device(sk::GATE, 4, 2) };
        assert!(gate.level == 0 && reads(&gate, 5), "from behind");
        assert!(reads(&gate, 0) && reads(&gate, 1), "and from the sides");
        assert!(!reads(&gate, 4) && gives(&gate, 4), "its front is where it gives");
        let piston = device(sk::PISTON, 3, 6);
        assert!(reads(&piston, 2) && reads(&piston, 0));
        assert!(!reads(&piston, 3) && !gives(&piston, 3), "nothing at its front");
        let floater = device(sk::FLOATER, 4, 6);
        assert!(!reads(&floater, 4) && reads(&floater, 5));
        let observer = device(sk::OBSERVER, 4, 6);
        assert!(gives(&observer, 5) && !gives(&observer, 4) && !reads(&observer, 5));
    }

    #[test]
    fn a_piece_reaches_all_but_the_far_side() {
        assert!(touches(2, 2), "what holds it up");
        assert!(touches(2, 0), "at an edge");
        assert!(!touches(2, 3), "the cell's far side");
    }
}
