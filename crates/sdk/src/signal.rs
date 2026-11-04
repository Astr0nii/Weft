//! Models that lie on any face: boxes written for a floor with the front to the north, placed for
//! what holds the block and where it faces.

use crate::abi::BlockPos;
use crate::{Dir, abi};

fn pos_add(a: BlockPos, b: BlockPos, k: i32) -> BlockPos {
    [a.x + b.x * k, a.y + b.y * k, a.z + b.z * k].into()
}

/// How a block is turned, from a floor with its front to the north.
#[derive(Clone, Copy)]
pub struct Orientation {
    east: BlockPos,
    up: BlockPos,
    north: BlockPos,
}

impl Orientation {
    /// `attach`: where what holds it is, `front`: where it faces, along that. One that can't be
    /// (front along attach) faces another way.
    pub fn new(attach: Dir, front: Dir) -> Orientation {
        let (attach, front) = if attach.face() / 2 == front.face() / 2 {
            (attach, if attach.horizontal() { Dir::Up } else { Dir::North })
        } else {
            (attach, front)
        };
        let (up, n) = (attach.opposite().offset(), front.offset());
        Orientation { east: [n.y * up.z - n.z * up.y, n.z * up.x - n.x * up.z, n.x * up.y - n.y * up.x].into(), up, north: n }
    }

    /// For a torch on a wall: its own west toward what holds it, its top up.
    pub fn leaning(attach: Dir) -> Orientation {
        let east = attach.opposite().offset();
        Orientation { east, up: [0, 1, 0].into(), north: [east.z, 0, -east.x].into() }
    }

    /// A point in texels, in the world's axes.
    pub(crate) fn at(&self, p: [f32; 3]) -> [f32; 3] {
        self.point(p)
    }

    /// One of its own directions, in the world's axes.
    pub(crate) fn along(&self, d: Dir) -> [f32; 3] {
        self.vector(d.offset()).to_array().map(|v| v as f32)
    }

    /// A box's corners, in the world's axes.
    pub(crate) fn corners(&self, min: [f32; 3], max: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        let (a, b) = (self.point(min), self.point(max));
        (std::array::from_fn(|i| a[i].min(b[i])), std::array::from_fn(|i| a[i].max(b[i])))
    }

    /// A vector in the block's own axes.
    fn vector(&self, v: BlockPos) -> BlockPos {
        pos_add(pos_add(pos_add(BlockPos::ZERO, self.east, v.x), self.up, v.y), self.north, -v.z)
    }

    /// A point in texels, 0..16.
    fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let c = [p[0] - 8.0, p[1] - 8.0, p[2] - 8.0];
        let (e, u, n) = (self.east.to_array(), self.up.to_array(), self.north.to_array());
        std::array::from_fn(|i| 8.0 + c[0] * e[i] as f32 + c[1] * u[i] as f32 - c[2] * n[i] as f32)
    }

    /// Where a corner of the block goes, 0 or 1 each way.
    fn corner(&self, c: BlockPos) -> BlockPos {
        let p = self.point([c.x as f32 * 16.0, c.y as f32 * 16.0, c.z as f32 * 16.0]);
        p.map(|v| (v / 16.0).round() as i32).into()
    }

    /// Which face a face of its own (-X, +X, -Y, +Y, -Z, +Z) becomes.
    pub(crate) fn face(&self, face: usize) -> usize {
        let d = Dir::from_face(face as u32).offset();
        let w = self.vector(d);
        Dir::ALL
            .iter()
            .find(|x| x.offset() == w)
            .map_or(face, |x| x.face() as usize)
    }

    /// Quarter turns for a face's picture to read as it does on the floor.
    fn turn(&self, face: usize, to: usize) -> u8 {
        let first = self.corner(abi::FACE_CORNERS[face][0].into());
        abi::FACE_CORNERS[to]
            .iter()
            .position(|&c| BlockPos::from(c) == first)
            .unwrap_or(0) as u8
    }

    /// A box and its faces turned: (min, max, per world face what was given for it, and its turn).
    pub(crate) fn place<T: Copy>(&self, min: [f32; 3], max: [f32; 3], faces: [T; 6]) -> ([f32; 3], [f32; 3], [T; 6], [u8; 6]) {
        let (a, b) = (self.point(min), self.point(max));
        let (lo, hi) = (std::array::from_fn(|i| a[i].min(b[i])), std::array::from_fn(|i| a[i].max(b[i])));
        let mut out = faces;
        let mut turn = [0u8; 6];
        for (l, f) in faces.iter().enumerate() {
            let to = self.face(l);
            out[to] = *f;
            turn[to] = self.turn(l, to);
        }
        (lo, hi, out, turn)
    }
}

/// The direction along a face closest to where a player looks.
pub(crate) fn dir_along_face(attach: Dir, look: [f32; 3]) -> Dir {
    let mut best = (Dir::North, -1.0f32);
    for d in Dir::ALL {
        if d.face() / 2 != attach.face() / 2 {
            let o = d.offset();
            let v = o.x as f32 * look[0] + o.y as f32 * look[1] + o.z as f32 * look[2];
            if v > best.1 {
                best = (d, v);
            }
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_floor_to_the_north_is_as_it_is() {
        let f = Orientation::new(Dir::Down, Dir::North);
        let (lo, hi, faces, turn) = f.place([1.0, 0.0, 2.0], [3.0, 4.0, 5.0], [0, 1, 2, 3, 4, 5]);
        assert_eq!((lo, hi), ([1.0, 0.0, 2.0], [3.0, 4.0, 5.0]));
        assert_eq!(faces, [0, 1, 2, 3, 4, 5]);
        assert_eq!(turn, [0; 6]);
    }

    #[test]
    fn turning_keeps_a_box_a_box_and_the_top_away_from_what_holds_it() {
        for attach in Dir::ALL {
            for front in Dir::ALL {
                if attach.face() / 2 == front.face() / 2 {
                    continue;
                }
                let f = Orientation::new(attach, front);
                let (lo, hi, faces, _) = f.place([0.0, 0.0, 0.0], [16.0, 2.0, 16.0], [0, 1, 2, 3, 4, 5]);
                // The slab lies against what holds it.
                let axis = attach.face() as usize / 2;
                let thin = hi[axis] - lo[axis];
                assert_eq!(thin, 2.0, "{attach:?} {front:?}");
                assert_eq!(faces[attach.opposite().face() as usize], 3, "top away, {attach:?} {front:?}");
                assert_eq!(faces[attach.face() as usize], 2, "bottom on it, {attach:?} {front:?}");
                assert_eq!(faces[front.face() as usize], 4, "north to the front, {attach:?} {front:?}");
                let at = if attach.offset().to_array()[axis] < 0 { lo[axis] } else { hi[axis] };
                assert!(at == 0.0 || at == 16.0, "{attach:?} {front:?}");
            }
        }
    }

    #[test]
    fn a_box_out_of_its_cell_keeps_its_faces() {
        // Three texels out: as a texel rectangle that must not read as "no face".
        for face in 0..6 {
            let uv = abi::box_face_uv(face, [-3.0, 0.0, -3.0], [2.0, 19.0, 2.0]);
            assert!(uv.iter().all(|v| (0.0..=16.0).contains(v)), "{face}: {uv:?}");
            assert!(uv != abi::UV_NO_FACE);
        }
    }

    #[test]
    fn along_stays_on_the_face() {
        assert_eq!(dir_along_face(Dir::Down, [0.0, -1.0, 0.1]), Dir::South);
        assert_eq!(dir_along_face(Dir::West, [-1.0, 0.9, 0.1]), Dir::Up);
    }
}
