//! Terrain from parts: a base height plus height features, biome rules that pick each column's
//! biome and surface, water and caves. The mod that owns a dimension describes its terrain and
//! any mod can edit it afterwards (add a biome before another, a feature, a cave biome), so the
//! engine evaluates the merged result. Heights, masks and fields are expressions (`Expr`) the
//! engine runs on the CPU and the GPU alike, so a mod's own shapes are as fast as the presets.
//!
//! ```ignore
//! r.terrain(&MARS)
//!     .stone("basalt")
//!     .base_height(60.0)
//!     .field("mars:peaks", Expr::noise(500.0).ramp(0.2, 0.5))
//!     .feature(preset::hills(8.0, 220.0))
//!     .feature(preset::mountains(60.0, 160.0).mask(Expr::field("mars:peaks")))
//!     .biome(BiomeRule::new("peaks").field("mars:peaks", 0.5..))
//!     .biome(BiomeRule::new("dust"));
//! // Another mod, whose savanna biome declares its climate and surface:
//! r.terrain(&OVERWORLD).biome_before("base:desert", BiomeRule::new("savanna"));
//! ```

use crate::abi::BlockPos;
use std::marker::PhantomData;
use std::ops::{Add, Bound, Mul, Neg, RangeBounds, Sub};

use crate::abi::{self, Slice, Str, expr as ek, rule_field as rf, terrain_op as op};
use crate::noise::fixed::{ONE, q4, q12, salt};
use crate::{Dimension, Gen, Registry, Scatter, callback, host, qualify};

/// Surface `height` and `biome` id of a column.
pub type Column = abi::TerrainColumn;

#[derive(Clone)]
struct Step {
    kind: u32,
    salt: u64,
    a: i32,
    b: i32,
    c: i32,
    name: String,
}

impl Step {
    fn new(kind: u32, a: i32) -> Step {
        Step { kind, salt: 0, a, b: 0, c: 0, name: String::new() }
    }

    fn desc(&self) -> abi::ExprStep {
        abi::ExprStep { kind: self.kind, salt: self.salt, a: self.a, b: self.b, c: self.c, name: Str::new(&self.name) }
    }
}

/// A number for every column of a terrain, worked out from where the column is: the height a
/// feature adds, the strength of a mask, a field biome rules ask for. Built from numbers, the
/// position and noise with `+`, `-`, `*` and the methods here; the engine runs it on the CPU
/// and the GPU with the same integer math, so every machine gets the same world.
///
/// Lengths (heights, positions, sizes) are in blocks, `Expr::blocks(12.0)`; everything else
/// (noise, strengths, factors) is a plain number, `Expr::value(0.5)`. Multiplying a length by a
/// number gives a length. Results run about -2048..2048 blocks or -64..64 as numbers; a factor
/// of a product is held to about +-11 (+-2896 blocks).
#[derive(Clone)]
pub struct Expr(Vec<Step>);

impl Expr {
    fn leaf(step: Step) -> Expr {
        Expr(vec![step])
    }

    fn then(mut self, step: Step) -> Expr {
        self.0.push(step);
        self
    }

    fn with(mut self, other: Expr, kind: u32) -> Expr {
        self.0.extend(other.0);
        self.then(Step::new(kind, 0))
    }

    fn noise_raw(salt: u64, scale: i32, octaves: u32) -> Expr {
        Expr(vec![Step::new(ek::X, 0), Step::new(ek::Z, 0), Step { salt, b: octaves as i32, ..Step::new(ek::NOISE, scale) }])
    }

    fn ramp_raw(self, from: i32, to: i32) -> Expr {
        self.then(Step { b: to, ..Step::new(ek::RAMP, from) })
    }

    /// A plain number.
    pub fn value(v: f64) -> Expr {
        Expr::leaf(Step::new(ek::CONST, q12(v)))
    }

    /// A length in blocks.
    pub fn blocks(b: f64) -> Expr {
        Expr::leaf(Step::new(ek::CONST, q4(b)))
    }

    /// An integer as it is, for `idiv` and `modulo` by a count.
    pub fn int(n: i32) -> Expr {
        Expr::leaf(Step::new(ek::CONST, n))
    }

    /// The column's position, a length.
    pub fn x() -> Expr {
        Expr::leaf(Step::new(ek::X, 0))
    }

    pub fn z() -> Expr {
        Expr::leaf(Step::new(ek::Z, 0))
    }

    /// A field of the terrain: `temperature`, `humidity`, `continent`, or one declared with
    /// `TerrainBuilder::field` before what uses it. Worked out once per column however many
    /// times it is used, so share costly parts through a field.
    pub fn field(name: &str) -> Expr {
        Expr::leaf(Step { name: name.to_string(), ..Step::new(ek::FIELD, 0) })
    }

    /// About -1.4..1.4, cold to hot.
    pub fn temperature() -> Expr {
        Expr::field("temperature")
    }

    /// About -1.4..1.4, dry to wet.
    pub fn humidity() -> Expr {
        Expr::field("humidity")
    }

    /// Continentalness: below 0 is sea, coasts are about 0..0.1.
    pub fn continent() -> Expr {
        Expr::field("continent")
    }

    /// Noise about -1..1 in patches about `scale` blocks across, `octaves` layers of it each
    /// twice as fine and half as strong. Noises with the same `salt` and size line up.
    pub fn fbm(salt: u64, scale: f64, octaves: u32) -> Expr {
        Expr::noise_raw(salt, q4(scale), octaves)
    }

    /// `fbm` of 3 octaves with a salt of its own size.
    pub fn noise(scale: f64) -> Expr {
        Expr::fbm(scale.to_bits(), scale, 3)
    }

    /// 1 along lines where the noise crosses 0, falling to 0 a quarter of the way to its peaks:
    /// winding ridges or cuts `scale` blocks apart.
    pub fn ridge(salt: u64, scale: f64) -> Expr {
        Expr::value(1.0)
            - (Expr::fbm(salt, scale, 4) * Expr::value(2.0))
                .abs()
                .min(Expr::value(1.0))
    }

    /// Points scattered one per `spacing` x `spacing` square at most, each there with `chance`
    /// and `radius` blocks big times 0.5 to 1.5: the square of the distance to the nearest, in
    /// units of its radius (below 1 inside it), 2 or more where none is near.
    pub fn sites(salt: u64, spacing: f64, chance: f64, radius: f64) -> Expr {
        Expr(vec![Step::new(ek::X, 0), Step::new(ek::Z, 0)]).then(Step {
            salt,
            b: q12(chance),
            c: q4(radius),
            ..Step::new(ek::SITE, q4(spacing))
        })
    }

    /// This expression at another position: `noise.at(Expr::x() + warp, Expr::z())` bends it.
    /// Fields stay what they are at the column.
    pub fn at(self, x: Expr, z: Expr) -> Expr {
        let mut out = Vec::with_capacity(self.0.len());
        for s in self.0 {
            match s.kind {
                ek::X => out.extend(x.0.iter().cloned()),
                ek::Z => out.extend(z.0.iter().cloned()),
                _ => out.push(s),
            }
        }
        Expr(out)
    }

    pub fn abs(self) -> Expr {
        self.then(Step::new(ek::ABS, 0))
    }

    pub fn min(self, other: Expr) -> Expr {
        self.with(other, ek::MIN)
    }

    pub fn max(self, other: Expr) -> Expr {
        self.with(other, ek::MAX)
    }

    pub fn clamp(self, lo: Expr, hi: Expr) -> Expr {
        self.max(lo).min(hi)
    }

    /// Integer division, toward zero: a length by `Expr::int(2)` is half as long.
    pub fn idiv(self, other: Expr) -> Expr {
        self.with(other, ek::DIV)
    }

    /// What is left over below `other` (a length or a count), never negative: repeats along
    /// the world.
    pub fn modulo(self, other: Expr) -> Expr {
        self.with(other, ek::REM)
    }

    /// 0 at `from`, 1 at `to` and straight in between, a strength for a mask; `from > to`
    /// turns it around.
    pub fn ramp(self, from: f64, to: f64) -> Expr {
        self.ramp_raw(q12(from), q12(to))
    }

    /// `ramp` of a length, `from` and `to` in blocks.
    pub fn ramp_blocks(self, from: f64, to: f64) -> Expr {
        self.ramp_raw(q4(from), q4(to))
    }

    /// 1 minus it, for masks.
    pub fn not(self) -> Expr {
        Expr::value(1.0) - self
    }

    /// Both masks: their product.
    pub fn and(self, other: Expr) -> Expr {
        self * other
    }

    fn steps(&self) -> Vec<abi::ExprStep> {
        self.0.iter().map(Step::desc).collect()
    }
}

impl Add for Expr {
    type Output = Expr;
    fn add(self, other: Expr) -> Expr {
        self.with(other, ek::ADD)
    }
}

impl Sub for Expr {
    type Output = Expr;
    fn sub(self, other: Expr) -> Expr {
        self.with(other, ek::SUB)
    }
}

impl Mul for Expr {
    type Output = Expr;
    fn mul(self, other: Expr) -> Expr {
        self.with(other, ek::MUL)
    }
}

impl Neg for Expr {
    type Output = Expr;
    fn neg(self) -> Expr {
        Expr::int(0) - self
    }
}

/// Adds to the height of every column, scaled by its mask.
pub struct Feature {
    name: String,
    height: Expr,
    mask: Option<Expr>,
}

impl Feature {
    /// A feature adding `height` (a length) at every column.
    pub fn new(height: Expr) -> Feature {
        Feature { name: String::new(), height, mask: None }
    }

    /// Other mods find the feature by this name.
    pub fn named(mut self, name: &str) -> Feature {
        self.name = name.to_string();
        self
    }

    /// Where it applies, 0..1: nothing where it is 0 or less (and its height isn't worked out
    /// there), all of it from 1.
    pub fn mask(mut self, mask: Expr) -> Feature {
        self.mask = Some(mask);
        self
    }
}

impl From<Expr> for Feature {
    fn from(height: Expr) -> Feature {
        Feature::new(height)
    }
}

/// Ready-made features. Each is `Feature::new` of its `shape`, which can be changed or
/// combined into a new kind.
pub mod preset {
    use super::*;

    /// Oceans and land masses: `depth` blocks above or below the base height.
    pub fn continents(depth: f64) -> Feature {
        Feature::new(shape::continents(depth))
    }

    /// Rolling hills.
    pub fn hills(height: f64, size: f64) -> Feature {
        Feature::new(shape::hills(height, size))
    }

    /// Mountain ranges along ridge lines `width` blocks apart, with rough detail on top.
    pub fn mountains(height: f64, width: f64) -> Feature {
        Feature::new(shape::mountains(height, width))
    }

    /// Flat-topped mesas `height` blocks high in terraces of `step`.
    pub fn plateaus(height: f64, size: f64, step: f64) -> Feature {
        Feature::new(shape::plateaus(height, size, step))
    }

    /// Winding cuts `depth` deep and about `width` wide; below sea level they fill with water.
    pub fn canyons(depth: f64, width: f64) -> Feature {
        Feature::new(shape::canyons(depth, width))
    }

    /// Parallel sand waves `wavelength` blocks apart.
    pub fn dunes(height: f64, wavelength: f64) -> Feature {
        Feature::new(shape::dunes(height, wavelength))
    }

    /// Bowls with a raised rim, one per `spacing`-block cell at most.
    pub fn craters(radius: f64, depth: f64, spacing: f64) -> Feature {
        Feature::new(shape::craters(radius, depth, spacing))
    }
}

/// The heights of the presets, as expressions.
pub mod shape {
    use super::*;

    pub fn continents(depth: f64) -> Expr {
        let c = (Expr::continent() * Expr::value(5.0)).idiv(Expr::int(2));
        c.clamp(Expr::value(-1.0), Expr::value(1.0)) * Expr::blocks(depth)
    }

    pub fn hills(height: f64, size: f64) -> Expr {
        Expr::fbm(salt::HILLS, size, 4) * Expr::blocks(height)
    }

    pub fn mountains(height: f64, width: f64) -> Expr {
        let r = || Expr::ridge(salt::MOUNTAINS, width);
        r() * r() * Expr::blocks(height) + (Expr::fbm(salt::HILLS, 60.0, 4) * Expr::blocks(height)).idiv(Expr::int(7))
    }

    pub fn plateaus(height: f64, size: f64, step: f64) -> Expr {
        let v = || Expr::fbm(salt::PLATEAUS, size, 3).ramp(0.0, 0.25) * Expr::blocks(height);
        v() - v().modulo(Expr::blocks(step))
    }

    pub fn canyons(depth: f64, width: f64) -> Expr {
        let scale = q4(width).min(1 << 20) * 40;
        let r = Expr::value(1.0)
            - (Expr::noise_raw(salt::CANYONS, scale, 4) * Expr::value(2.0))
                .abs()
                .min(Expr::value(1.0));
        -(r.ramp_raw(3994, ONE) * Expr::blocks(depth))
    }

    pub fn dunes(height: f64, wavelength: f64) -> Expr {
        let wl = q4(wavelength).clamp(1, (1 << 18) - 1);
        let t = || {
            let warp = Expr::noise_raw(salt::DUNES, wl * 4, 2) * Expr::int(wl);
            (Expr::x() + Expr::z().idiv(Expr::int(2)) + warp)
                .modulo(Expr::int(wl))
                .ramp_raw(0, wl)
        };
        // Gentle windward slope, steep lee side.
        t().ramp_raw(0, 3277).min(t().ramp_raw(ONE, 3277)) * Expr::blocks(height)
    }

    pub fn craters(radius: f64, depth: f64, spacing: f64) -> Expr {
        let d = || Expr::sites(salt::CRATERS, spacing, 0.6, radius);
        let bowl = d().ramp(1.0, 0.0) * Expr::blocks(depth);
        let rim = (d() - Expr::int(5325)).abs().ramp_raw(1229, 0) * Expr::blocks(depth);
        -bowl + rim.idiv(Expr::int(4))
    }
}

/// Where a biome stands in a terrain. Its climate and surface come from the biome
/// (`r.biome(&X).temperature(..).surface(..)`); a rule adds where else it must hold (height,
/// continent, any field) and sets only what differs here. Rules are tried in order; the first
/// match wins and the last one is used where none does.
#[derive(Clone)]
pub struct BiomeRule {
    name: String,
    fields: u32,
    biome: String,
    ranges: Vec<(String, f64, f64)>,
    height: (i32, i32),
    top: String,
    filler: String,
    depth: i32,
    under: (String, i32),
    deep_top: (String, i32),
    bare_above: i32,
    ice: String,
}

impl BiomeRule {
    /// A rule for `biome`, named like it.
    pub fn new(biome: &str) -> BiomeRule {
        BiomeRule { fields: rf::BIOME, biome: biome.to_string(), ..BiomeRule::blank() }
    }

    pub(crate) fn blank() -> BiomeRule {
        BiomeRule {
            name: String::new(),
            fields: 0,
            biome: String::new(),
            ranges: Vec::new(),
            height: (i32::MIN, i32::MAX),
            top: String::new(),
            filler: String::new(),
            depth: 3,
            under: (String::new(), 0),
            deep_top: (String::new(), i32::MIN),
            bare_above: i32::MAX,
            ice: String::new(),
        }
    }

    /// A name of its own, for a second rule of the same biome (other mods find rules by name).
    pub fn named(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }
    /// In an edit: the rule picks this biome instead.
    pub fn biome(mut self, biome: &str) -> Self {
        (self.biome, self.fields) = (biome.to_string(), self.fields | rf::BIOME);
        self
    }
    /// Where the field (`temperature`, `humidity`, `continent` or one the terrain declares) is in
    /// `range`, like `0.35..` or `..-0.45` (from inclusive, to exclusive). Given again for the
    /// same field, the new range replaces the old.
    pub fn field(mut self, name: &str, range: impl RangeBounds<f64>) -> Self {
        let (min, max) = range_f64(range);
        match self.ranges.iter_mut().find(|r| r.0 == name) {
            Some(r) => (r.1, r.2) = (min, max),
            None => self.ranges.push((name.to_string(), min, max)),
        }
        self.fields |= rf::RANGES;
        self
    }
    /// Climate, about -1.4..1.4. Usually the biome's.
    pub fn temperature(self, range: impl RangeBounds<f64>) -> Self {
        self.field("temperature", range)
    }
    pub fn humidity(self, range: impl RangeBounds<f64>) -> Self {
        self.field("humidity", range)
    }
    /// Continentalness: below 0 is sea, coasts are about 0..0.1.
    pub fn continent(self, range: impl RangeBounds<f64>) -> Self {
        self.field("continent", range)
    }
    /// Surface heights, like `..=SEA + 1`.
    pub fn height(mut self, range: impl RangeBounds<i32>) -> Self {
        (self.height, self.fields) = (range_i32(range), self.fields | rf::HEIGHT);
        self
    }
    /// Top block, then `depth` blocks of filler, then the terrain's stone.
    pub fn surface(mut self, top: &str, filler: &str, depth: i32) -> Self {
        (self.top, self.filler, self.depth) = (top.to_string(), filler.to_string(), depth);
        self.fields |= rf::SURFACE;
        self
    }
    /// The lowest `layers` of the filler become `block` (sandstone under sand).
    pub fn under(mut self, block: &str, layers: i32) -> Self {
        (self.under, self.fields) = ((block.to_string(), layers), self.fields | rf::UNDER);
        self
    }
    /// Top block for columns lower than `below` (gravel on deep sea floors).
    pub fn deep_top(mut self, block: &str, below: i32) -> Self {
        (self.deep_top, self.fields) = ((block.to_string(), below), self.fields | rf::DEEP_TOP);
        self
    }
    /// Plain stone above this height (bare peaks).
    pub fn bare_above(mut self, y: i32) -> Self {
        (self.bare_above, self.fields) = (y, self.fields | rf::BARE_ABOVE);
        self
    }
    /// The water surface freezes into `block`.
    pub fn frozen(mut self, block: &str) -> Self {
        (self.ice, self.fields) = (block.to_string(), self.fields | rf::FROZEN);
        self
    }

    /// What `desc` points at: its names qualified with `ns`, and its ranges.
    pub(crate) fn names(&self, ns: &str) -> RuleNames {
        let q = |s: &str| if s.is_empty() { String::new() } else { qualify(ns, s) };
        let names = [
            if self.name.is_empty() { &self.biome } else { &self.name },
            &self.biome,
            &self.top,
            &self.filler,
            &self.under.0,
            &self.deep_top.0,
            &self.ice,
        ]
        .map(|s| q(s));
        let ranges = self
            .ranges
            .iter()
            .map(|(name, min, max)| abi::FieldRange { name: Str::new(name), min: *min, max: *max })
            .collect();
        RuleNames { names, ranges }
    }

    pub(crate) fn desc(&self, n: &RuleNames) -> abi::BiomeRuleDesc {
        let names = &n.names;
        abi::BiomeRuleDesc {
            name: Str::new(&names[0]),
            fields: self.fields,
            biome: Str::new(&names[1]),
            ranges: Slice::new(&n.ranges),
            height: [self.height.0, self.height.1],
            top: Str::new(&names[2]),
            filler: Str::new(&names[3]),
            depth: self.depth,
            under: Str::new(&names[4]),
            under_layers: self.under.1,
            deep_top: Str::new(&names[5]),
            deep_below: self.deep_top.1,
            bare_above: self.bare_above,
            ice: Str::new(&names[6]),
        }
    }
}

/// The strings a rule's `BiomeRuleDesc` points at, kept while it is sent.
pub(crate) struct RuleNames {
    names: [String; 7],
    ranges: Vec<abi::FieldRange>,
}

fn range_f64(r: impl RangeBounds<f64>) -> (f64, f64) {
    let at = |b: Bound<&f64>, open: f64| match b {
        Bound::Included(v) | Bound::Excluded(v) => *v,
        Bound::Unbounded => open,
    };
    (at(r.start_bound(), f64::NEG_INFINITY), at(r.end_bound(), f64::INFINITY))
}

/// Inclusive.
fn range_i32(r: impl RangeBounds<i32>) -> (i32, i32) {
    let min = match r.start_bound() {
        Bound::Included(v) => *v,
        Bound::Excluded(v) => v + 1,
        Bound::Unbounded => i32::MIN,
    };
    let max = match r.end_bound() {
        Bound::Included(v) => *v,
        Bound::Excluded(v) => v - 1,
        Bound::Unbounded => i32::MAX,
    };
    (min, max)
}

/// Two-noise tunnels plus large caverns. Between `lava_from` and `lava_to` some layers of height
/// hold flat lava lakes.
pub struct Caves {
    lava: String,
    lava_from: i32,
    lava_to: i32,
}

impl Caves {
    pub fn new(lava: &str, lava_from: i32, lava_to: i32) -> Caves {
        Caves { lava: lava.to_string(), lava_from, lava_to }
    }
}

impl Registry {
    /// The terrain of a dimension: yours to describe, or another mod's to edit
    /// (`r.terrain(&OVERWORLD)`). Edits apply after the owner's description, in load order.
    pub fn terrain<'a>(&'a mut self, dimension: &crate::Dimension) -> TerrainBuilder<'a> {
        TerrainBuilder { raw: self.raw, ns: self.ns, dimension: dimension.name().to_string(), _p: PhantomData }
    }
}

pub struct TerrainBuilder<'a> {
    raw: *mut abi::Registrar,
    ns: &'static str,
    dimension: String,
    _p: PhantomData<&'a mut Registry>,
}

/// One edit, owning everything its `TerrainDesc` points at while it is sent.
struct Op {
    op: u32,
    anchor: String,
    block: String,
    level: i32,
    to: i32,
    value: f64,
    size: f64,
    feature: Option<Feature>,
    rule: Option<BiomeRule>,
    field: Option<Expr>,
}

impl Op {
    fn new(op: u32) -> Op {
        Op {
            op,
            anchor: String::new(),
            block: String::new(),
            level: 0,
            to: 0,
            value: 0.0,
            size: 0.0,
            feature: None,
            rule: None,
            field: None,
        }
    }
}

impl TerrainBuilder<'_> {
    fn send(self, o: Op) -> Self {
        let q = |s: &str| if s.is_empty() { String::new() } else { qualify(self.ns, s) };
        // Field names are used as written.
        let anchor = if o.op == op::FIELD { o.anchor.clone() } else { q(&o.anchor) };
        let block = q(&o.block);
        let fname = o.feature.as_ref().map_or(String::new(), |f| q(&f.name));
        let height = o.feature.as_ref().map_or(Vec::new(), |f| f.height.steps());
        let mask = o
            .feature
            .as_ref()
            .and_then(|f| f.mask.as_ref())
            .map_or(Vec::new(), Expr::steps);
        let field = o.field.as_ref().map_or(Vec::new(), Expr::steps);
        let feature = abi::FeatureDesc { name: Str::new(&fname), height: Slice::new(&height), mask: Slice::new(&mask) };
        let r = o.rule.unwrap_or_else(BiomeRule::blank);
        let names = r.names(self.ns);
        let rule = r.desc(&names);
        let desc = abi::TerrainDesc {
            dimension: Str::new(&self.dimension),
            op: o.op,
            anchor: Str::new(&anchor),
            block: Str::new(&block),
            level: o.level,
            to: o.to,
            value: o.value,
            size: o.size,
            feature,
            rule,
            field: Slice::new(&field),
        };
        unsafe { (host().reg_terrain)(self.raw, &desc) }
        self
    }

    /// The block the ground is made of.
    pub fn stone(self, block: &str) -> Self {
        self.send(Op { block: block.to_string(), ..Op::new(op::STONE) })
    }
    /// The ground height where no feature adds anything.
    pub fn base_height(self, y: f64) -> Self {
        self.send(Op { value: y, ..Op::new(op::BASE_HEIGHT) })
    }
    /// Everything up to `level` that isn't ground is `water`.
    pub fn sea(self, level: i32, water: &str) -> Self {
        self.send(Op { level, block: water.to_string(), ..Op::new(op::SEA) })
    }
    pub fn caves(self, caves: Caves) -> Self {
        self.send(Op { block: caves.lava, level: caves.lava_from, to: caves.lava_to, ..Op::new(op::CAVES) })
    }
    /// A named value over the terrain's columns, for biome rules (`BiomeRule::field`) and
    /// other expressions (`Expr::field`), worked out once per column. The name is used as written:
    /// give yours your mod's prefix (`mymod:aridity`). Declaring `temperature`, `humidity`,
    /// `continent` or another mod's field again replaces it for everything that uses it.
    pub fn field(self, name: &str, value: Expr) -> Self {
        self.send(Op { anchor: name.to_string(), field: Some(value), ..Op::new(op::FIELD) })
    }
    pub fn feature(self, f: impl Into<Feature>) -> Self {
        self.send(Op { feature: Some(f.into()), ..Op::new(op::FEATURE) })
    }
    pub fn feature_remove(self, name: &str) -> Self {
        self.send(Op { anchor: name.to_string(), ..Op::new(op::FEATURE_REMOVE) })
    }
    /// Limits the named feature further: its mask is multiplied by `mask`.
    pub fn feature_mask(self, name: &str, mask: Expr) -> Self {
        self.send(Op { anchor: name.to_string(), feature: Some(Feature::new(Expr::int(0)).mask(mask)), ..Op::new(op::FEATURE_MASK) })
    }
    /// A rule after all others.
    pub fn biome(self, rule: BiomeRule) -> Self {
        self.send(Op { rule: Some(rule), ..Op::new(op::BIOME) })
    }
    /// A rule tried right before the rule named `rule` (a biome's rule is named like the biome).
    pub fn biome_before(self, rule: &str, new: BiomeRule) -> Self {
        self.send(Op { anchor: rule.to_string(), rule: Some(new), ..Op::new(op::BIOME_BEFORE) })
    }
    pub fn biome_after(self, rule: &str, new: BiomeRule) -> Self {
        self.send(Op { anchor: rule.to_string(), rule: Some(new), ..Op::new(op::BIOME_AFTER) })
    }
    pub fn biome_replace(self, rule: &str, new: BiomeRule) -> Self {
        self.send(Op { anchor: rule.to_string(), rule: Some(new), ..Op::new(op::BIOME_REPLACE) })
    }
    pub fn biome_remove(self, rule: &str) -> Self {
        self.send(Op { anchor: rule.to_string(), ..Op::new(op::BIOME_REMOVE) })
    }
    /// Changes what the closure sets on the rule and keeps the rest:
    /// `biome_edit("base:desert", |r| r.temperature(0.5..))`.
    pub fn biome_edit(self, rule: &str, edit: impl FnOnce(BiomeRule) -> BiomeRule) -> Self {
        self.send(Op { anchor: rule.to_string(), rule: Some(edit(BiomeRule::blank())), ..Op::new(op::BIOME_EDIT) })
    }
    /// A biome in 3D: whole cubes at least `depth` blocks under the surface where a 3D noise of
    /// patches about `size` blocks across is above `threshold` (-1..1). Biomes are kept per
    /// column of each cube, so underground ones change every 16 blocks of height at most.
    pub fn cave_biome(self, biome: &str, depth: i32, size: f64, threshold: f64) -> Self {
        self.send(Op { rule: Some(BiomeRule::new(biome)), level: depth, size, value: threshold, ..Op::new(op::CAVE_BIOME) })
    }
    pub fn cave_biome_remove(self, biome: &str) -> Self {
        self.send(Op { anchor: biome.to_string(), ..Op::new(op::CAVE_BIOME_REMOVE) })
    }
}

impl Dimension {
    /// Height and biome of column (`x`, `z`) for `seed`, generated or not: from the terrain or
    /// the dimension's `column`. A dimension with neither gives height `i32::MIN`.
    pub fn column(&self, seed: u64, x: i32, z: i32) -> Column {
        let mut out = Column { height: i32::MIN, biome: 0 };
        unsafe { (host().dim_column)(self.id(), seed, x, z, &mut out) };
        out
    }

    /// Whether caves took the surface block of column (`x`, `z`), so nothing should stand there.
    pub fn surface_carved(&self, seed: u64, x: i32, z: i32) -> bool {
        unsafe { (host().dim_surface_carved)(self.id(), seed, x, z) != 0 }
    }
}

impl Registry {
    /// Something scattered over the ground (trees): one site per `cell` x `cell` square at a
    /// seeded spot, kept with the chance the biome's `density` component gives, in every
    /// dimension where such biomes stand. The engine skips sites under the sea and where caves
    /// took the ground, fits the parts that reach into neighboring cubes, and keeps players
    /// from spawning in one.
    ///
    /// ```ignore
    /// static TREES: Scatter = Scatter::named("trees");
    /// r.scatter(&TREES).cell(5).reach(2).density(&TREE_DENSITY).shape(|s| {
    ///     let [x, y, z] = s.pos();
    ///     for dy in 0..5 { s.force([x, y + dy, z], LOG.id()); }
    /// });
    /// ```
    pub fn scatter<'a>(&'a mut self, scatter: &Scatter) -> ScatterBuilder<'a> {
        ScatterBuilder {
            raw: self.raw,
            name: scatter.name().to_string(),
            dimensions: String::new(),
            density: String::new(),
            ground: String::new(),
            desc: abi::ScatterDesc::DEFAULT,
            _p: PhantomData,
        }
    }

    /// Runs `f` on every generated cube after the generator and scatters (grass, flowers,
    /// boulders), in every dimension but bare ones unless `.dimension(..)` limits it.
    /// `g.columns()` has the surface of every column. Pure, like a generator.
    pub fn decoration<'a>(&'a mut self, f: impl Fn(&mut Gen) + Send + Sync + 'static) -> DecorationBuilder<'a> {
        DecorationBuilder {
            raw: self.raw,
            dimensions: String::new(),
            cb: callback(move |a: &mut abi::GenArgs| f(Gen::wrap(a))),
            _p: PhantomData,
        }
    }
}

pub struct ScatterBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    dimensions: String,
    density: String,
    ground: String,
    desc: abi::ScatterDesc,
    _p: PhantomData<&'a mut Registry>,
}

impl ScatterBuilder<'_> {
    /// Grid cell size in blocks (8 by default): at most one site per cell.
    pub fn cell(mut self, blocks: i32) -> Self {
        self.desc.cell = blocks;
        self
    }
    /// How far the shape puts blocks sideways from its site. Up and down it may go 32 and 16
    /// unless `reach_y` says otherwise.
    pub fn reach(mut self, blocks: i32) -> Self {
        self.desc.reach = blocks;
        self
    }
    /// Only place where the surface block belongs to this block or tag (for example,
    /// `.ground("#plant_soil")`). Without this constraint, any valid surface is allowed.
    pub fn ground(mut self, blocks: &str) -> Self {
        self.ground = blocks.to_string();
        self
    }
    /// How far above and below its site the shape may put blocks (a giant tree, roots going
    /// deep); what goes further is left out.
    pub fn reach_y(mut self, up: i32, down: i32) -> Self {
        self.desc.reach_up = up;
        self.desc.reach_down = down;
        self
    }
    /// The biome component (0..1) that is the chance of a site per cell; biomes without it get
    /// none.
    pub fn density(mut self, component: &crate::Component<f32>) -> Self {
        self.density = component.name().to_string();
        self
    }
    /// Only in this dimension (call again for more).
    pub fn dimension(mut self, dimension: &Dimension) -> Self {
        self.dimensions = format!("{} {}", self.dimensions, dimension.name());
        self
    }
    /// The blocks of one site. Pure: it runs again for each cube the site reaches.
    pub fn shape(mut self, f: impl Fn(&mut Site) + Send + Sync + 'static) -> Self {
        self.desc.shape = callback(move |a: &mut abi::ScatterArgs| f(Site::wrap(a)));
        self
    }
}

impl Drop for ScatterBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let desc = abi::ScatterDesc {
            name: Str::new(&self.name),
            dimensions: Str::new(&self.dimensions),
            density: Str::new(&self.density),
            ground: Str::new(&self.ground),
            ..self.desc
        };
        unsafe { (host().reg_scatter)(self.raw, &desc) }
    }
}

/// One site of a scatter: where it stands and where its blocks go (world coordinates).
pub struct Site {
    args: abi::ScatterArgs,
}

impl Site {
    fn wrap(a: &mut abi::ScatterArgs) -> &mut Site {
        unsafe { &mut *(a as *mut abi::ScatterArgs as *mut Site) }
    }
    /// The block above the ground.
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    pub fn biome(&self) -> u32 {
        self.args.biome
    }
    pub fn dimension(&self) -> u32 {
        self.args.dim
    }
    pub fn seed(&self) -> u64 {
        self.args.seed
    }
    /// Random bits of this site, the same every time it is shaped.
    pub fn random(&self) -> u64 {
        self.args.random
    }
    /// `state` at `pos` where there is air.
    pub fn put(&mut self, pos: impl Into<BlockPos>, state: u32) {
        self.push(pos.into(), state, 0);
    }
    /// `state` at `pos` whatever is there (a trunk through another tree's leaves).
    pub fn put_force(&mut self, pos: impl Into<BlockPos>, state: u32) {
        self.push(pos.into(), state, 1);
    }
    fn push(&mut self, pos: BlockPos, state: u32, force: u32) {
        let a = &mut self.args;
        if a.len < a.cap {
            let offset = [pos.x - a.pos.x, pos.y - a.pos.y, pos.z - a.pos.z];
            unsafe { *a.out.add(a.len as usize) = abi::ScatterBlock { offset: offset.into(), state, force } };
            a.len += 1;
        }
    }
}

pub struct DecorationBuilder<'a> {
    raw: *mut abi::Registrar,
    dimensions: String,
    cb: abi::Callback,
    _p: PhantomData<&'a mut Registry>,
}

impl DecorationBuilder<'_> {
    /// Only in this dimension (call again for more).
    pub fn dimension(mut self, dimension: &Dimension) -> Self {
        self.dimensions = format!("{} {}", self.dimensions, dimension.name());
        self
    }
}

impl Drop for DecorationBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        unsafe { (host().reg_decoration)(self.raw, Str::new(&self.dimensions), self.cb) }
    }
}

impl Gen {
    /// The dimension being generated.
    pub fn dimension(&self) -> u32 {
        self.args.dim
    }

    /// The surface of the cube's 256 columns, index `z * 16 + x`: from the terrain or the
    /// dimension's `column`. None for a dimension with neither.
    pub fn columns(&self) -> Option<&[Column; 256]> {
        unsafe { self.args.columns.cast::<[Column; 256]>().as_ref() }
    }

    /// Any column of this dimension, in world coordinates.
    pub fn column(&self, x: i32, z: i32) -> Column {
        let mut out = Column { height: i32::MIN, biome: 0 };
        unsafe { (host().dim_column)(self.args.dim, self.args.seed, x, z, &mut out) };
        out
    }

    /// Whether caves took the surface block of column (`x`, `z`), in world coordinates.
    pub fn surface_carved(&self, x: i32, z: i32) -> bool {
        unsafe { (host().dim_surface_carved)(self.args.dim, self.args.seed, x, z) != 0 }
    }

    /// The block the terrain leaves at the top of column (`x`, `z`), in world coordinates: 0
    /// where caves took it or the dimension has no terrain. `get` only sees this cube, so this is
    /// how to know the ground under its lowest layer.
    pub fn surface_block(&self, x: i32, z: i32) -> u32 {
        unsafe { (host().dim_surface_block)(self.args.dim, self.args.seed, x, z) }
    }
}
