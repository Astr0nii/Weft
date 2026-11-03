//! Shapes as values: what `World::fill`, `World::entities_in`, `draw_shape` and particle
//! bursts take. Built from a few primitives and combined; plain data, so a fill can be saved
//! and replayed where cubes load later.
//!
//! ```ignore
//! w.fill(Shape::blast(at, 4.0, w.random()), 0).keep(Filter::unbreakable());
//! let geode = Shape::ball(c, 4.0).subtract(Shape::ball(c, 3.0));
//! let on_belt = w.entities_in(&Shape::cuboid(lo, hi), EntityFilter::any());
//! ```

use crate::abi::{self, ShapeStep, shape_kind as k};
use crate::{Dir, host};

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    steps: Vec<ShapeStep>,
}

fn prim(kind: u32, v: &[f64]) -> Shape {
    let mut s = ShapeStep { kind, ..ShapeStep::DEFAULT };
    s.v[..v.len()].copy_from_slice(v);
    Shape { steps: vec![s] }
}

impl Shape {
    pub fn ball(center: [f64; 3], radius: f64) -> Shape {
        prim(k::BALL, &[center[0], center[1], center[2], radius])
    }
    /// The box between two corners.
    pub fn cuboid(min: [f64; 3], max: [f64; 3]) -> Shape {
        prim(k::BOX, &[min[0], min[1], min[2], max[0], max[1], max[2]])
    }
    /// From `base` along `dir` for `height` blocks (a mine shaft: `Dir::Down`).
    pub fn cylinder(base: [f64; 3], radius: f64, height: f64, dir: Dir) -> Shape {
        let mut s = prim(k::CYLINDER, &[base[0], base[1], base[2], radius, height]);
        s.steps[0].axis = dir.face();
        s
    }
    pub fn ellipsoid(center: [f64; 3], radii: [f64; 3]) -> Shape {
        prim(k::ELLIPSOID, &[center[0], center[1], center[2], radii[0], radii[1], radii[2]])
    }
    /// A ball whose edge wanders in and out by up to `amount` of the radius (0..1), with
    /// `bumps` of them across; the same `seed`, the same edge.
    pub fn noisy(center: [f64; 3], radius: f64, amount: f64, bumps: f64, seed: u64) -> Shape {
        let mut s = prim(k::NOISY, &[center[0], center[1], center[2], radius, amount, bumps]);
        s.steps[0].seed = seed;
        s
    }
    /// A crater like vanilla's explosions leave, without their rays: a ball of radius `power`
    /// with a ragged edge.
    pub fn blast(center: [f64; 3], power: f64, seed: u64) -> Shape {
        Shape::noisy(center, power, 0.25, 1.5, seed)
    }

    fn join(mut self, other: Shape, op: u32) -> Shape {
        self.steps.extend(other.steps);
        self.steps
            .push(ShapeStep { kind: op, ..ShapeStep::DEFAULT });
        self
    }
    /// Inside either.
    pub fn union(self, other: Shape) -> Shape {
        self.join(other, k::UNION)
    }
    /// Inside both.
    pub fn intersect(self, other: Shape) -> Shape {
        self.join(other, k::INTERSECT)
    }
    /// Inside this and not in `other`: a shell, a tunnel's hollow.
    pub fn subtract(self, other: Shape) -> Shape {
        self.join(other, k::SUBTRACT)
    }

    pub fn contains(&self, p: [f64; 3]) -> bool {
        unsafe { (host().shape_contains)(abi::Slice::new(&self.steps), &p) != 0 }
    }
    /// A box around everything inside (min, max).
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        let (mut lo, mut hi) = ([0.0; 3], [0.0; 3]);
        unsafe { (host().shape_bounds)(abi::Slice::new(&self.steps), &mut lo, &mut hi) };
        (lo, hi)
    }

    pub fn steps(&self) -> &[ShapeStep] {
        &self.steps
    }

    /// Lines tracing each primitive (circles around balls and cylinders, a box's edges), for
    /// drawing it.
    pub fn outline(&self) -> Vec<([f64; 3], [f64; 3])> {
        let mut out = Vec::new();
        for s in &self.steps {
            let v = s.v;
            let c = [v[0], v[1], v[2]];
            match s.kind {
                k::BALL => ellipse_rings(&mut out, c, [v[3]; 3]),
                k::NOISY => ellipse_rings(&mut out, c, [v[3]; 3]),
                k::ELLIPSOID => ellipse_rings(&mut out, c, [v[3], v[4], v[5]]),
                k::BOX => box_edges(&mut out, c, [v[3], v[4], v[5]]),
                k::CYLINDER => {
                    let d = abi::FACE_DIRS[(s.axis as usize).min(5)].map(|x| x as f64);
                    let top = [c[0] + d[0] * v[4], c[1] + d[1] * v[4], c[2] + d[2] * v[4]];
                    let a = d.iter().position(|x| *x != 0.0).unwrap_or(1);
                    let (u, w) = ((a + 1) % 3, (a + 2) % 3);
                    for end in [c, top] {
                        ring(&mut out, end, u, w, v[3], v[3]);
                    }
                    for i in 0..4 {
                        let t = i as f64 * std::f64::consts::FRAC_PI_2;
                        let mut p = c;
                        p[u] += t.cos() * v[3];
                        p[w] += t.sin() * v[3];
                        let q = [p[0] + top[0] - c[0], p[1] + top[1] - c[1], p[2] + top[2] - c[2]];
                        out.push((p, q));
                    }
                }
                _ => {}
            }
        }
        out
    }
}

fn ring(out: &mut Vec<([f64; 3], [f64; 3])>, c: [f64; 3], u: usize, w: usize, ru: f64, rw: f64) {
    const N: usize = 24;
    let at = |i: usize| {
        let t = i as f64 / N as f64 * std::f64::consts::TAU;
        let mut p = c;
        p[u] += t.cos() * ru;
        p[w] += t.sin() * rw;
        p
    };
    for i in 0..N {
        out.push((at(i), at(i + 1)));
    }
}

fn ellipse_rings(out: &mut Vec<([f64; 3], [f64; 3])>, c: [f64; 3], r: [f64; 3]) {
    ring(out, c, 0, 2, r[0], r[2]);
    ring(out, c, 0, 1, r[0], r[1]);
    ring(out, c, 2, 1, r[2], r[1]);
}

fn box_edges(out: &mut Vec<([f64; 3], [f64; 3])>, lo: [f64; 3], hi: [f64; 3]) {
    let corner =
        |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    for i in 0..8 {
        for bit in [1, 2, 4] {
            if i & bit == 0 {
                out.push((corner(i), corner(i | bit)));
            }
        }
    }
}

/// Which blocks a fill keeps (`Fill::keep`) or only changes (`Fill::only`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter(abi::BlockFilter);

impl Filter {
    pub fn liquids() -> Filter {
        Filter(abi::BlockFilter { kind: abi::block_filter::LIQUIDS, id: 0 })
    }
    /// Blocks with negative hardness (bedrock).
    pub fn unbreakable() -> Filter {
        Filter(abi::BlockFilter { kind: abi::block_filter::UNBREAKABLE, id: 0 })
    }
    /// Blocks with a tag (`"#stone"` or `"stone"`); a tag nothing has is a mistake the fill
    /// logs instead of happening.
    pub fn tag(tag: &str) -> Filter {
        let id = unsafe { (host().find_component)(abi::Str::new(tag.trim_start_matches('#')), abi::Str::EMPTY) };
        Filter(abi::BlockFilter { kind: abi::block_filter::TAG, id })
    }
    pub fn block(block: &crate::Block) -> Filter {
        Filter(abi::BlockFilter { kind: abi::block_filter::BLOCK, id: block.id() })
    }
}

/// A fill in the making (`World::fill`); it happens as this goes.
pub struct Fill {
    pub(crate) world: crate::World,
    pub(crate) shape: Shape,
    pub(crate) state: u32,
    pub(crate) keep: Vec<abi::BlockFilter>,
    pub(crate) only: Vec<abi::BlockFilter>,
}

impl Fill {
    /// Blocks this matches stay.
    pub fn keep(mut self, what: Filter) -> Self {
        self.keep.push(what.0);
        self
    }
    /// Only blocks this (or another `only`) matches change: ore veins in stone only.
    pub fn only(mut self, what: Filter) -> Self {
        self.only.push(what.0);
        self
    }
    pub fn keep_unbreakable(self) -> Self {
        self.keep(Filter::unbreakable())
    }
    pub fn keep_liquids(self) -> Self {
        self.keep(Filter::liquids())
    }
}

impl Drop for Fill {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let desc = abi::FillDesc {
                shape: abi::Slice::new(&self.shape.steps),
                state: self.state,
                keep: abi::Slice::new(&self.keep),
                only: abi::Slice::new(&self.only),
            };
            unsafe { (host().fill)(self.world.raw(), &desc) }
        }
    }
}

/// What `World::entities_in` picks.
#[derive(Clone, Copy, Debug)]
pub struct EntityFilter {
    kind: u32,
    flags: u32,
}

impl EntityFilter {
    pub fn any() -> EntityFilter {
        EntityFilter { kind: u32::MAX, flags: 0 }
    }
    pub fn kind(kind: &crate::EntityKind) -> EntityFilter {
        EntityFilter { kind: kind.id(), flags: 0 }
    }
    /// Only what can be hit: no dropped items, block entities, spectators or the dead.
    pub fn targets(mut self) -> Self {
        self.flags |= abi::entity_filter::TARGETS;
        self
    }
    /// Any part of its box in the shape, not just its feet (an arrow catching a tall mob's head).
    pub fn touching(mut self) -> Self {
        self.flags |= abi::entity_filter::TOUCHING;
        self
    }
}

impl crate::World {
    /// Every block whose middle is inside `shape` becomes `state` (air for a crater), at the
    /// end of the tick and across the whole dimension: loaded cubes at once on every core, the
    /// rest when they load. Far faster than `set_block` per block for anything big. It doesn't ask
    /// `BREAK_BLOCK` or drop anything.
    pub fn fill(&self, shape: Shape, state: u32) -> Fill {
        Fill { world: *self, shape, state, keep: Vec::new(), only: Vec::new() }
    }
    /// Entities in `shape` that `filter` lets through.
    pub fn entities_in(&self, shape: &Shape, filter: EntityFilter) -> Vec<u64> {
        let mut buf = vec![0u64; 64];
        let call = |buf: &mut Vec<u64>| unsafe {
            (host().entities_in)(self.raw(), abi::Slice::new(&shape.steps), filter.kind, filter.flags, buf.as_mut_ptr(), buf.len())
        };
        let n = call(&mut buf);
        if n > buf.len() {
            buf.resize(n, 0);
            call(&mut buf);
        }
        buf.truncate(n);
        buf
    }
    /// Entity ids whose feet are within `radius`; `kind` None = any kind.
    pub fn entities_near(&self, pos: [f64; 3], radius: f64, kind: Option<&crate::EntityKind>) -> Vec<u64> {
        let f = kind.map_or(EntityFilter::any(), EntityFilter::kind);
        self.entities_in(&Shape::ball(pos, radius), f)
    }
    /// Like `entities_near`, only what can be hit.
    pub fn targets_near(&self, pos: [f64; 3], radius: f64, kind: Option<&crate::EntityKind>) -> Vec<u64> {
        let f = kind.map_or(EntityFilter::any(), EntityFilter::kind);
        self.entities_in(&Shape::ball(pos, radius), f.targets())
    }
}
