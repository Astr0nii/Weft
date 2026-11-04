//! Stairs, slabs and vertical slabs of any block, by tag or name: `r.stairs("#has_shapes")`. Each
//! block found gets its own, with its textures and what it is like to mine, also from mods this
//! one has never heard of. `.tab(..)` and the rest go on what they return, as on any block.

use crate::Dir;
use crate::abi::BlockPos;
use crate::component::Tag;
use crate::content::{Block, BlockBuilder, Place, Props, Registry, WithComponents};
use crate::world::state_prop;

/// How neighboring stairs find each other: every stair has it.
static STAIRS: Tag = Tag::named("stairs");

fn pos_step(pos: BlockPos, side: Dir) -> BlockPos {
    let o = side.offset();
    [pos.x + o.x, pos.y + o.y, pos.z + o.z].into()
}

/// The part of a layer towards `a` (and `b`, for a corner), in 1/16 of a block: (x, z) ranges.
fn towards(a: Dir, b: Option<Dir>) -> ([f32; 2], [f32; 2]) {
    let (mut x, mut z) = ([0.0, 16.0], [0.0, 16.0]);
    for side in std::iter::once(a).chain(b) {
        match side {
            Dir::North => z = [0.0, 8.0],
            Dir::South => z = [8.0, 16.0],
            Dir::East => x = [8.0, 16.0],
            _ => x = [0.0, 8.0],
        }
    }
    (x, z)
}

/// Whether what is placed goes in the top half.
fn upper_half(p: &Place) -> bool {
    p.hit()[1] > 0.5
}

/// A stair's shape from the stairs around it, as vanilla has it.
fn stair_shape(w: &crate::World, pos: BlockPos) -> &'static str {
    let (Some(facing), Some(half)) = (w.prop(pos, "facing").and_then(Dir::from_name), w.prop(pos, "half")) else { return "straight" };
    let stair_at = |at: BlockPos| {
        let s = w.block(at);
        (STAIRS.on_block(s) && state_prop(s, "half") == Some(half))
            .then(|| state_prop(s, "facing").and_then(Dir::from_name))
            .flatten()
    };
    let across = |side: Dir| side != facing && side != facing.opposite();
    // Nothing the same way on that side to keep in line with.
    let free = |side: Dir| stair_at(pos_step(pos, side)) != Some(facing);
    if let Some(back) = stair_at(pos_step(pos, facing)).filter(|&b| across(b) && free(b.opposite())) {
        return if back == facing.counter_clockwise() { "outer_left" } else { "outer_right" };
    }
    if let Some(front) = stair_at(pos_step(pos, facing.opposite())).filter(|&f| across(f) && free(f)) {
        return if front == facing.counter_clockwise() { "inner_left" } else { "inner_right" };
    }
    "straight"
}

impl Registry {
    /// Blocks made of others: `sources` are names and tags (`#has_shapes`, `#planks/*`, `base:stone`),
    /// and each full cube among them, of any mod, gets a block `<its name><suffix>` from this one.
    /// It shows the source's textures on what `.states(..)` leaves to it, and is mined and
    /// sounds like it. The lang key `shape.<name>` names them, `{0}` being the source. Each is
    /// tagged `<name>/<source>` and its source `<name>_of/<source>`, for one recipe to cover all:
    /// `r.shaped(C, "#wall/{m}", 6, &["###", "###"], &[('#', "#wall_of/{m}")])`.
    pub fn shapes_of<'a>(&'a mut self, name: &str, sources: &str, suffix: &str) -> BlockBuilder<'a> {
        self.block(Block::generated(name.to_string()))
            .shapes_of(sources, suffix)
    }

    /// Stairs of `sources` (see `shapes_of`): `<name>_stairs`, turned the way the player looks,
    /// joining into corners with those around them.
    pub fn stairs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a> {
        self.shapes_of("stairs", sources, "_stairs")
            .tag("stairs")
            .signal_blocker()
            .holds_liquid()
            .prop("facing", ["north", "east", "south", "west"])
            .prop("half", ["bottom", "top"])
            .prop("shape", ["straight", "inner_left", "inner_right", "outer_left", "outer_right"])
            .states(|p: &Props, mut s| {
                let (low, high) = if p.is("half", "top") { ([8.0, 16.0], [0.0, 8.0]) } else { ([0.0, 8.0], [8.0, 16.0]) };
                s = s.model_box([0.0, low[0], 0.0], [16.0, low[1], 16.0]);
                let f = p.facing();
                let parts = match p.get("shape") {
                    "outer_left" => vec![towards(f, Some(f.counter_clockwise()))],
                    "outer_right" => vec![towards(f, Some(f.clockwise()))],
                    "inner_left" => vec![towards(f, None), towards(f.counter_clockwise(), Some(f.opposite()))],
                    "inner_right" => vec![towards(f, None), towards(f.clockwise(), Some(f.opposite()))],
                    _ => vec![towards(f, None)],
                };
                for (x, z) in parts {
                    s = s.model_box([x[0], high[0], z[0]], [x[1], high[1], z[1]]);
                }
                s
            })
            .placement(|p, props| {
                props.set("facing", p.look_facing().name());
                props.set("half", if upper_half(p) { "top" } else { "bottom" });
            })
            .derived("shape", |n| stair_shape(&n.world(), n.pos()))
    }

    /// Slabs of `sources` (see `shapes_of`): `<name>_slab`, in the half of the block clicked. The
    /// other half takes one of any block, the same one making it a whole block of it.
    pub fn slabs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a> {
        self.shapes_of("slab", sources, "_slab")
            .tag("slabs")
            .prop("type", ["bottom", "top", "double"])
            .states(|p: &Props, s| match p.get("type") {
                "bottom" => s
                    .model_box([0.0, 0.0, 0.0], [16.0, 8.0, 16.0])
                    .signal_blocker()
                    .holds_liquid(),
                "top" => s
                    .model_box([0.0, 8.0, 0.0], [16.0, 16.0, 16.0])
                    .signal_blocker()
                    .holds_liquid(),
                _ => s.loot_rolls(2),
            })
            .placement(|p, props| props.set("type", if upper_half(p) { "top" } else { "bottom" }))
            .merge(|p, props| {
                if p.placed("type") != Some(props.get("type")) {
                    props.set("type", "double");
                }
            })
    }

    /// Vertical slabs of `sources` (see `shapes_of`): `<name>_vertical_slab`, against what was
    /// clicked. On a floor or a ceiling, on the side of the block nearest to where it was
    /// clicked. The other side takes one of any block, the same one making it a whole block of it.
    pub fn vertical_slabs<'a>(&'a mut self, sources: &str) -> BlockBuilder<'a> {
        self.shapes_of("vertical_slab", sources, "_vertical_slab")
            .tag("vertical_slabs")
            .prop("side", ["north", "south", "east", "west", "double"])
            .states(|p: &Props, s| match p.get("side") {
                "double" => s.loot_rolls(2),
                side => {
                    let (x, z) = towards(Dir::from_name(side).unwrap_or(Dir::North), None);
                    s.model_box([x[0], 0.0, z[0]], [x[1], 16.0, z[1]])
                        .signal_blocker()
                        .holds_liquid()
                }
            })
            .placement(|p, props| {
                let [x, _, z] = p.hit().map(|v| v - 0.5);
                let along_x = match p.face() {
                    Dir::West | Dir::East => true,
                    Dir::North | Dir::South => false,
                    _ => x.abs() > z.abs(),
                };
                let side = match along_x {
                    true => {
                        if x > 0.0 {
                            "east"
                        } else {
                            "west"
                        }
                    }
                    false => {
                        if z > 0.0 {
                            "south"
                        } else {
                            "north"
                        }
                    }
                };
                props.set("side", side);
            })
            .merge(|p, props| {
                let there = Dir::from_name(props.get("side"));
                if p.placed("side")
                    .and_then(Dir::from_name)
                    .is_some_and(|side| Some(side.opposite()) == there)
                {
                    props.set("side", "double");
                }
            })
    }
}
