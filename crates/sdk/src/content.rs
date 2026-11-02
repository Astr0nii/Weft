use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::abi::{self, Layer, Model, Slice, Str, block_flags as bf};
use crate::component::{self as c, Component, Value};
use crate::{World, callback, host, qualify};

/// What a handle can name. Kinds with engine ids (`HasId`) can also be resolved to one.
pub mod kind {
    pub trait Kind {
        /// `abi::entry_kind`, or `u32::MAX` for things known only by name.
        const KIND: u32;
    }
    /// Kinds the engine numbers (blocks, items...): their handles give ids.
    pub trait HasId: Kind {}
    macro_rules! kinds {
        ($($n:ident = $v:expr),*) => {
            $(pub struct $n;
            impl Kind for $n {
                const KIND: u32 = $v;
            }
            impl HasId for $n {})*
        };
    }
    kinds!(
        Block = crate::abi::entry_kind::BLOCK,
        Item = crate::abi::entry_kind::ITEM,
        Entity = crate::abi::entry_kind::ENTITY,
        Biome = crate::abi::entry_kind::BIOME,
        Dimension = crate::abi::entry_kind::DIMENSION,
        Menu = crate::abi::entry_kind::MENU,
        Particle = crate::abi::entry_kind::PARTICLE,
        Config = crate::abi::entry_kind::CONFIG,
        Sound = crate::abi::entry_kind::SOUND
    );
    macro_rules! named_kinds {
        ($($n:ident),*) => {
            $(pub struct $n;
            impl Kind for $n {
                const KIND: u32 = u32::MAX;
            })*
        };
    }
    named_kinds!(
        SkyObject,
        Key,
        Screen,
        HudElement,
        GizmoGroup,
        TintKind,
        EntityCategory,
        SoundCategory,
        CreativeTab,
        Question,
        Chain,
        Attribute,
        RecipeKind,
        LootTable,
        TextCondition,
        Music,
        EquipmentSlot,
        Scatter,
        Template
    );
}

const UNRESOLVED: u32 = u32::MAX - 1;

/// A thing a mod owns (block, item, entity kind, menu, key, screen...), named in its handle:
///
/// ```ignore
/// static STONE: Block = Block::named("stone");        // this mod's, "<ns>:stone"
/// static COAL: Item = Item::named("base:coal");       // another mod's
/// r.block(&STONE).hardness(1.5);                      // registered by its handle
/// if STONE.is(w.get(pos)) { .. }
/// ```
///
/// A name without a namespace is the declaring mod's, the same when registering and reading.
/// Ids are known once the engine froze the registries.
pub struct Handle<K: kind::Kind> {
    id: AtomicU32,
    count: AtomicU32,
    name: &'static str,
    full: std::sync::OnceLock<String>,
    _k: PhantomData<fn() -> K>,
}

impl<K: kind::Kind> Handle<K> {
    pub const fn named(name: &'static str) -> Self {
        Handle { id: AtomicU32::new(UNRESOLVED), count: AtomicU32::new(1), name, full: std::sync::OnceLock::new(), _k: PhantomData }
    }

    /// A handle for a name made at runtime, for families of content built in a loop
    /// (`r.item(Item::generated(format!("{metal}_ingot")))`). Lives as long as the game, like
    /// a `static`; meant for registration, not for every tick.
    pub fn generated(name: String) -> &'static Self {
        Box::leak(Box::new(Self::named(Box::leak(name.into_boxed_str()))))
    }

    /// Full name, `ns:name`.
    pub fn name(&self) -> &str {
        self.full.get_or_init(|| crate::full_name(self.name))
    }

    /// The name without its namespace.
    pub fn path(&self) -> &str {
        let n = self.name();
        n.split_once(':').map_or(n, |x| x.1)
    }
}

impl Item {
    /// `count` of this item, plain; `.with(&COMP, v)` and `.with_damage(n)` give it data.
    pub fn stack(&self, count: u32) -> abi::ItemStack {
        abi::ItemStack::new(self.id(), count)
    }
}

impl<K: kind::HasId> Handle<K> {
    /// For blocks, the first state. `u32::MAX` if unknown.
    #[inline]
    pub fn id(&self) -> u32 {
        let id = self.id.load(Ordering::Relaxed);
        if id < UNRESOLVED { id } else { self.resolve(id) }
    }

    #[cold]
    fn resolve(&self, id: u32) -> u32 {
        let frozen = unsafe { !(host().components)().is_null() };
        if id == u32::MAX || !frozen {
            return u32::MAX;
        }
        let mut count = 0;
        let id = unsafe { (host().find)(K::KIND, Str::new(self.name()), &mut count) };
        if id != u32::MAX {
            self.count.store(count, Ordering::Relaxed);
            self.id.store(id, Ordering::Relaxed);
        } else {
            // Frozen and still unknown: a typo or a missing mod. Say so once.
            crate::error!(
                "no {} named {}",
                abi::entry_kind::NAMES
                    .get(K::KIND as usize)
                    .unwrap_or(&"entry"),
                self.name()
            );
            self.id.store(u32::MAX, Ordering::Relaxed);
        }
        id
    }

    /// The id of what a player typed (`/give` style input), `None` if there is no such thing.
    /// Code names things with handles; this is for text that only exists at runtime.
    pub fn lookup(name: &str) -> Option<u32> {
        let id = unsafe { (host().find)(K::KIND, Str::new(name), std::ptr::null_mut()) };
        (id != u32::MAX).then_some(id)
    }

    /// The block's state with properties `props` set, the others at their defaults:
    /// `FURNACE.with(&[("lit", "true")])`. `u32::MAX` if the block or a value is unknown.
    pub fn with(&self, props: &[(&str, &str)]) -> u32 {
        props
            .iter()
            .fold(self.id(), |s, (k, v)| if s == u32::MAX { s } else { unsafe { (host().state_with)(s, Str::new(k), Str::new(v)) } })
    }

    /// True if `state` belongs to this block.
    #[inline]
    pub fn is(&self, state: u32) -> bool {
        state.wrapping_sub(self.id()) < self.count.load(Ordering::Relaxed)
    }

    pub(crate) fn cell(&self) -> *const AtomicU32 {
        &self.id
    }

    pub(crate) fn set_count(&self, n: u32) {
        self.count.store(n, Ordering::Relaxed);
    }
}

fn cell<K: kind::HasId>(h: Option<&'static Handle<K>>) -> *const AtomicU32 {
    h.map_or(std::ptr::null(), |h| h.cell())
}

pub type Block = Handle<kind::Block>;
pub type Item = Handle<kind::Item>;
pub type Biome = Handle<kind::Biome>;
pub type Dimension = Handle<kind::Dimension>;
pub type Particle = Handle<kind::Particle>;
pub type SkyObject = Handle<kind::SkyObject>;
pub type Sound = Handle<kind::Sound>;
pub type Entity = Handle<kind::Entity>;

/// A thing by its handle, or by the id that handle gives: what takes this takes either.
pub struct Id<H> {
    raw: u32,
    _p: std::marker::PhantomData<fn() -> H>,
}

impl<H> Id<H> {
    pub fn raw(&self) -> u32 {
        self.raw
    }
}

impl<H> From<u32> for Id<H> {
    fn from(raw: u32) -> Self {
        Id { raw, _p: std::marker::PhantomData }
    }
}

impl<K: kind::HasId> From<&Handle<K>> for Id<Handle<K>> {
    fn from(h: &Handle<K>) -> Self {
        h.id().into()
    }
}
pub type Menu = Handle<kind::Menu>;
pub type Key = Handle<kind::Key>;

pub type Screen = Handle<kind::Screen>;
pub type HudElement = Handle<kind::HudElement>;
pub type GizmoGroup = Handle<kind::GizmoGroup>;
pub type TintKind = Handle<kind::TintKind>;
pub type Scatter = Handle<kind::Scatter>;
/// A fixed shape of blocks (`Registry::template`), for world structures, moving structures and
/// `World::place`.
pub type Template = Handle<kind::Template>;

/// The engine's tint kinds; biomes color them with `.tint(&GRASS, 0x..)`.
pub mod tints {
    use super::TintKind;
    pub static GRASS: TintKind = TintKind::named("core:grass");
    pub static FOLIAGE: TintKind = TintKind::named("core:foliage");
    pub static WATER: TintKind = TintKind::named("core:water");
}
pub type EntityCategory = Handle<kind::EntityCategory>;
pub type SoundCategory = Handle<kind::SoundCategory>;
pub type CreativeTab = Handle<kind::CreativeTab>;
pub type Question = Handle<kind::Question>;
pub type Chain = Handle<kind::Chain>;
pub type Attribute = Handle<kind::Attribute>;

/// The creative tab every entity's spawn egg goes to.
pub static SPAWN_EGGS: CreativeTab = CreativeTab::named("core:spawn_eggs");
pub type RecipeKind = Handle<kind::RecipeKind>;
pub type LootTable = Handle<kind::LootTable>;
pub type TextCondition = Handle<kind::TextCondition>;
pub type Music = Handle<kind::Music>;
pub type EquipmentSlot = Handle<kind::EquipmentSlot>;
/// The second hand, in the player's equipment: register it to have one, with any background.
pub static OFF_HAND: EquipmentSlot = EquipmentSlot::named(abi::OFF_HAND_SLOT);

/// Things that name a loot table: a block or entity kind (what it drops) or a table of its own.
pub trait LootSource {
    fn table_name(&self) -> &str;
}
impl LootSource for Block {
    fn table_name(&self) -> &str {
        self.name()
    }
}
impl LootSource for Entity {
    fn table_name(&self) -> &str {
        self.name()
    }
}
impl LootSource for LootTable {
    fn table_name(&self) -> &str {
        self.name()
    }
}

/// Builders of content carrying components (blocks, items, biomes, dimensions, liquids, entity
/// kinds) tag and set values the same way.
pub trait WithComponents: Sized {
    #[doc(hidden)]
    fn pending(&mut self) -> &mut Vec<Pending>;

    fn tag(mut self, tag: &str) -> Self {
        self.pending().push(Pending::tag(tag));
        self
    }
    /// A component value (on every state of a block, and its item).
    fn set<T: Value>(mut self, comp: &Component<T>, v: T) -> Self {
        self.pending().push(comp.raw_value(&v));
        self
    }
    /// A value written as text: plain text, or block names the engine resolves (`BlockRef`,
    /// `BlockList`).
    fn set_text<T: c::TextValue>(mut self, comp: &Component<T>, text: &str) -> Self {
        self.pending()
            .push(Pending::text_as(comp.name(), &T::schema(), text));
        self
    }
}

macro_rules! with_components {
    ($($b:ident),*) => {
        $(impl WithComponents for $b<'_> {
            fn pending(&mut self) -> &mut Vec<Pending> {
                &mut self.components
            }
        })*
    };
}

with_components!(BlockBuilder, ItemBuilder, BiomeBuilder, DimensionBuilder, LiquidBuilder);

/// A component value waiting to be sent with a descriptor. Text is kept as a `String` until then.
#[derive(Clone)]
pub struct Pending {
    pub(crate) name: String,
    schema: String,
    bytes: Vec<u8>,
    text: Option<String>,
}

impl Pending {
    pub(crate) fn value(name: &str, schema: String, bytes: Vec<u8>) -> Pending {
        Pending { name: name.to_string(), schema, bytes, text: None }
    }
    pub(crate) fn text(name: &str, text: &str) -> Pending {
        Pending::text_as(name, "str", text)
    }
    pub(crate) fn text_as(name: &str, schema: &str, text: &str) -> Pending {
        // Block names without a namespace are this mod's, like everywhere else.
        let text = match schema {
            "block" | "blocks" | "item" => text
                .split_whitespace()
                .map(|e| match e.rsplit_once(':') {
                    // `oak_log:4`: the weight stays, the name gets the namespace.
                    Some((n, w)) if w.parse::<u32>().is_ok() => format!("{}:{w}", crate::full_name(n)),
                    _ => crate::full_name(e),
                })
                .collect::<Vec<_>>()
                .join(" "),
            _ => text.to_string(),
        };
        Pending { name: name.to_string(), schema: schema.into(), bytes: Vec::new(), text: Some(text) }
    }
    fn tag(name: &str) -> Pending {
        Pending { name: name.to_string(), schema: String::new(), bytes: Vec::new(), text: None }
    }
}

/// `CompValue`s pointing into `list`; for text the `Str` bytes live in `scratch`.
pub(crate) fn raw_values_pub(list: &[Pending], scratch: &mut Vec<Vec<u8>>) -> Vec<abi::CompValue> {
    raw_values(list, scratch)
}

fn raw_values(list: &[Pending], scratch: &mut Vec<Vec<u8>>) -> Vec<abi::CompValue> {
    scratch.clear();
    for p in list {
        let bytes = match &p.text {
            Some(t) => c::pod_bytes(&Str::new(t)),
            None => p.bytes.clone(),
        };
        scratch.push(bytes);
    }
    list.iter()
        .zip(scratch.iter())
        .map(|(p, b)| abi::CompValue { name: Str::new(&p.name), schema: Str::new(&p.schema), value: Slice::new(b) })
        .collect()
}

/// What `Registry::set_on` applies to; names without a namespace are this mod's.
pub enum Target<'a> {
    /// The block and item of that name; `name[prop=value]` only the block's states with that value.
    Name(&'a str),
    Entity(&'a str),
    Biome(&'a str),
    Dimension(&'a str),
}

pub struct Registry {
    pub(crate) raw: *mut abi::Registrar,
    pub(crate) ns: &'static str,
}

impl Registry {
    pub fn namespace(&self) -> &'static str {
        self.ns
    }

    /// Registers a block and, unless `.no_item()` is called, an item that places it (and carries
    /// the block's components). Texture defaults to `block/<name>` for all faces.
    pub fn block<'a>(&'a mut self, block: &'static Block) -> BlockBuilder<'a> {
        let name = block.name().to_string();
        let tex = crate::asset("block", &name);
        BlockBuilder {
            reg: self,
            name,
            handle: Some(block),
            states: vec![StateDef::new(&tex)],
            components: Vec::new(),
            drops: None,
            item: true,
            item_texture: String::new(),
            callbacks: BlockCallbacks::default(),
            replace: false,
            own_states: false,
            retexture: Vec::new(),
            props: Vec::new(),
            opens: String::new(),
            looks: None,
            facing: None,
            placement: None,
            merge: None,
            made_of: None,
            shares_cell: Vec::new(),
            signal: None,
        }
    }

    /// Texture defaults to `item/<name>`.
    pub fn item<'a>(&'a mut self, item: &'static Item) -> ItemBuilder<'a> {
        ItemBuilder {
            _p: PhantomData,
            raw: self.raw,
            name: item.name().to_string(),
            texture: crate::asset("item", item.name()),
            handle: Some(item),
            block: String::new(),
            components: Vec::new(),
            on_use: abi::Callback::NONE,
        }
    }

    pub fn biome<'a>(&'a mut self, biome: &'static Biome) -> BiomeBuilder<'a> {
        BiomeBuilder {
            _p: PhantomData,
            raw: self.raw,
            ns: self.ns,
            name: biome.name().to_string(),
            handle: Some(biome),
            rule: crate::terrain::BiomeRule::blank(),
            tints: Vec::new(),
            components: Vec::new(),
        }
    }

    /// Creative inventory tab named `tab.<ns>.<name>` in the language files, shown with `icon`.
    /// Blocks and items join with `.tab(&TAB)`; untabbed items land in a tab named after the mod.
    pub fn creative_tab(&mut self, tab: &CreativeTab, icon: &Item) {
        unsafe { (host().reg_creative_tab)(self.raw, Str::new(tab.name()), Str::new(icon.name())) }
    }

    /// Ore veins of `block` in any mod's dimension, e.g.
    /// `r.ore(&OVERWORLD, &TIN_ORE).y(0, 64).veins(6).size(8).replace("#stone")`.
    /// An ore, in every dimension whose ground it may `replace` (`.dimension(..)` limits it).
    pub fn ore<'a>(&'a mut self, block: &Block) -> OreBuilder<'a> {
        OreBuilder {
            raw: self.raw,
            ns: self.ns,
            dimensions: String::new(),
            block: block.name().to_string(),
            replace: String::new(),
            biome: String::new(),
            desc: abi::OreDesc::DEFAULT,
            _p: PhantomData,
        }
    }

    /// A template placed now and then on the surface of `dimension` (the structure is called
    /// like the template, for `World::structure_near`).
    pub fn structure<'a>(&'a mut self, dimension: &Dimension, template: &Template) -> StructureBuilder<'a> {
        StructureBuilder {
            raw: self.raw,
            template: template.name().to_string(),
            dimension: dimension.name().to_string(),
            biome: String::new(),
            desc: abi::StructureDesc::DEFAULT,
            _p: PhantomData,
        }
    }

    /// A fixed shape of blocks, drawn in layers of rows (`layer` + `key`) or filled with boxes
    /// (`fill`), checked when the game loads.
    pub fn template<'a>(&'a mut self, template: &Template) -> TemplateBuilder<'a> {
        TemplateBuilder {
            raw: self.raw,
            ns: self.ns,
            name: template.name().to_string(),
            layers: Vec::new(),
            keys: Vec::new(),
            fills: Vec::new(),
            _p: PhantomData,
        }
    }

    pub fn dimension<'a>(&'a mut self, dimension: &'static Dimension) -> DimensionBuilder<'a> {
        DimensionBuilder {
            raw: self.raw,
            name: dimension.name().to_string(),
            handle: Some(dimension),
            desc: abi::DimensionDesc::DEFAULT,
            components: Vec::new(),
            _p: PhantomData,
        }
    }

    /// Sets a component on blocks and items of any mod, by reference: `r.set("base:coal", &FUEL,
    /// 1600)`, `r.set("#planks/*", &FUEL, 300)`, `r.set("base:furnace[lit=true]", &HOT, ..)`. The
    /// last mod in load order wins; overrides are logged.
    pub fn set<T: Value>(&mut self, what: &str, comp: &Component<T>, v: T) {
        self.set_on(Target::Name(what), comp, v)
    }

    pub fn set_on<T: Value>(&mut self, target: Target, comp: &Component<T>, v: T) {
        let p = comp.raw_value(&v);
        self.send_set(target, &p, false);
    }

    /// Adds `tag` to blocks and items of any mod, by reference: `r.tag("ingots/tin",
    /// &["othermod:tin_ingot"])`, `r.tag("plant_soil", &["#dirt"])`.
    pub fn tag(&mut self, tag: &str, names: &[&str]) {
        for n in names {
            self.send_set(Target::Name(n), &Pending::tag(tag), false);
        }
    }

    pub fn tag_on(&mut self, target: Target, tag: &str) {
        self.send_set(target, &Pending::tag(tag), false);
    }

    /// What an item or an entity of any mod looks like. From a file, `models/<file>.geo.json` with
    /// its `textures/<file>.png`, shown everywhere the item shows, or from code:
    /// `r.set_model("base:apple", "foodmod:apple")`, `r.set_model("core:player", biped(false))`.
    /// A whole tag at once takes `{name}`; items without a file stay as they were:
    /// `r.set_model("#food", "foodmod:food/{name}")`.
    pub fn set_model(&mut self, what: impl ModelTarget, model: impl ModelSource) {
        model.set_on(self, what.target());
    }

    /// The clips of an item or an entity of any mod: `r.set_animations("core:player",
    /// "mymod:player")` plays `animations/player.animation.json` of `mymod` on every player.
    pub fn set_animations(&mut self, what: impl ModelTarget, file: &str) {
        let file = crate::asset("", file);
        self.set_text_on(what.target(), &c::ANIMATIONS, &file);
    }

    /// Every entity of `kind` has `model` on its bone `bone`, drawn in that model's own pixels, with
    /// the same origin as the kind's. What plays on the entity plays on it too, by the names of
    /// clips and bones: `r.attach_model(&PLAYER, "tailmod:tail", "body")`.
    pub fn attach_model(&mut self, kind: impl ModelTarget, model: &str, bone: &str) {
        let (model, kind) = (crate::asset("", model), qualify(self.ns, kind.target().name()));
        unsafe { (host().reg_attach_model)(self.raw, Str::new(&kind), Str::new(&model), Str::new(bone)) }
    }

    /// `bone` of `model` moves by itself. A bone called `hanging` or `hanging_<anything>` does without this.
    pub fn bone_motion(&mut self, model: &str, bone: &str, motion: BoneMotion) {
        let model = crate::asset("", model);
        unsafe { (host().reg_bone_motion)(self.raw, Str::new(&model), Str::new(bone), motion.gravity, motion.spring, motion.drag) }
    }

    /// Which model a stack of `item` shows as, by what is in it: `None` for its usual one. Asked once
    /// per kind of stack. Other players' hands show the usual one: their stacks aren't sent.
    pub fn item_model_for_stack<R: AsRef<str>>(
        &mut self,
        item: impl ModelTarget,
        f: impl Fn(&abi::ItemStack) -> Option<R> + Send + Sync + 'static,
    ) {
        thread_local! {
            static NAME: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
        }
        let cb = callback(move |a: &mut abi::ItemModelArgs| {
            if let Some(name) = f(&a.stack) {
                NAME.with(|n| {
                    *n.borrow_mut() = crate::asset("", name.as_ref());
                    a.model = Str::new(&n.borrow());
                });
            }
        });
        let item = qualify(self.ns, item.target().name());
        unsafe { (host().reg_item_model_when)(self.raw, Str::new(&item), cb) }
    }

    /// A text component on entries of any mod.
    pub fn set_text_on<T: c::TextValue>(&mut self, target: Target, comp: &Component<T>, text: &str) {
        self.send_set(target, &Pending::text_as(comp.name(), &T::schema(), text), false);
    }

    /// Takes a component or tag away from entries of any mod.
    pub fn remove(&mut self, target: Target, component: &str) {
        self.send_set(target, &Pending::tag(component), true);
    }

    fn send_set(&mut self, target: Target, value: &Pending, remove: bool) {
        use abi::set_target as st;
        let (kind, name) = match target {
            Target::Name(n) => (st::NAME, qualify(self.ns, n)),
            Target::Entity(n) => (st::ENTITY, qualify(self.ns, n)),
            Target::Biome(n) => (st::BIOME, qualify(self.ns, n)),
            Target::Dimension(n) => (st::DIMENSION, qualify(self.ns, n)),
        };
        let mut scratch = Vec::new();
        let values = raw_values(std::slice::from_ref(value), &mut scratch);
        let desc =
            abi::SetDesc { target_kind: kind, target: Str::new(&name), value: values[0], remove: remove as u32, ..abi::SetDesc::DEFAULT };
        unsafe { (host().reg_set)(self.raw, &desc) }
    }
}

/// How a bone moves by itself, on top of its clips. Numbers don't depend on the bone's size.
#[derive(Clone, Copy, Debug)]
pub struct BoneMotion {
    /// Pull toward hanging straight down. The bigger, the faster it swings.
    pub gravity: f32,
    /// Pull back to where the clips have it.
    pub spring: f32,
    /// How fast the swing dies down.
    pub drag: f32,
}

impl BoneMotion {
    /// On a chain: a lantern. What a bone called `hanging` does.
    pub const HANGING: BoneMotion = BoneMotion { gravity: 40.0, spring: 0.0, drag: 2.5 };
    /// Keeps its shape, droops a little and lags behind: a tail.
    pub const SPRINGY: BoneMotion = BoneMotion { gravity: 6.0, spring: 30.0, drag: 5.0 };
}

/// What `Registry::set_model` takes: a file's name, or a model made in code.
pub trait ModelSource {
    fn set_on(self, r: &mut Registry, what: Target<'_>);
}

impl ModelSource for &str {
    fn set_on(self, r: &mut Registry, what: Target<'_>) {
        let file = crate::asset("", self);
        r.set_text_on(what, &c::MODEL, &file);
    }
}

/// What has a model: a name or `#tag` as text, or the handle of an item, a block or an entity.
pub trait ModelTarget {
    fn target(&self) -> Target<'_>;
}

impl ModelTarget for &str {
    fn target(&self) -> Target<'_> {
        Target::Name(self)
    }
}

impl ModelTarget for &Item {
    fn target(&self) -> Target<'_> {
        Target::Name(self.name())
    }
}

impl ModelTarget for &Block {
    fn target(&self) -> Target<'_> {
        Target::Name(self.name())
    }
}

impl ModelTarget for &crate::Entity {
    fn target(&self) -> Target<'_> {
        Target::Entity(self.name())
    }
}

impl Target<'_> {
    pub(crate) fn name(&self) -> &str {
        match self {
            Target::Name(n) | Target::Entity(n) | Target::Biome(n) | Target::Dimension(n) => n,
        }
    }
}

/// The actions the engine plays by itself, by the names clips have in files. Any other name is a
/// mod's own.
pub mod action {
    pub const IDLE: &str = "idle";
    pub const WALK: &str = "walk";
    pub const SNEAK: &str = "sneak";
    pub const SWIM: &str = "swim";
    pub const CRAWL: &str = "crawl";
    pub const DEATH: &str = "death";
    pub const HOLD: &str = "hold";
    pub const HOLD_TOOL: &str = "hold_tool";
    pub const HOLD_EMPTY: &str = "hold_empty";
    pub const SWING: &str = "swing";
    pub const SWING_HELD: &str = "swing_held";
    pub const USE: &str = "use";
    pub const EQUIP: &str = "equip";
    /// An item's model lying on the ground, and in a slot.
    pub const GROUND: &str = "ground";
    pub const GUI: &str = "gui";
}

#[derive(Clone)]
struct StateDef {
    flags: u32,
    filter: [u8; 3],
    /// `None`: what its flags say, whatever came before or after.
    opacity: Option<u8>,
    layer: Layer,
    model: Model,
    textures: [String; 6],
    boxes: Vec<([f32; 3], [f32; 3], [String; 6], [[f32; 4]; 6], [u8; 6], abi::BoxTilt)>,
    /// In sixteenths of a block.
    hitbox: Vec<[f32; 6]>,
    /// What boxes from here on are turned by.
    frame: Option<crate::signal::Frame>,
    components: Vec<Pending>,
    signal: SignalSpec,
}

/// What a state is to redstone, see `abi::SignalDesc`.
#[derive(Clone, Copy)]
struct SignalSpec {
    kind: u32,
    attach: u32,
    front: u32,
    delay: u32,
    mode: u32,
    head: &'static str,
}

impl Default for SignalSpec {
    fn default() -> Self {
        let d = abi::SignalDesc::DEFAULT;
        SignalSpec { kind: d.kind, attach: d.attach, front: d.front, delay: d.delay, mode: d.mode, head: "" }
    }
}

impl StateDef {
    /// A later value of the same component wins at freeze.
    fn put<T: Value>(&mut self, comp: &Component<T>, v: T) {
        self.components.push(comp.raw_value(&v));
    }
    fn put_text(&mut self, name: &str, schema: &str, text: &str) {
        self.components.push(Pending::text_as(name, schema, text));
    }
}

impl StateDef {
    /// One more box, turned by the frame if there is one. A model that isn't a cube doesn't hide
    /// what is behind it, so nothing else about a block says how it is to be drawn.
    fn add_box(&mut self, min: [f32; 3], max: [f32; 3], textures: [String; 6], uv: [[f32; 4]; 6]) {
        if self.model != Model::Boxes {
            self.boxes.clear();
        }
        self.model = Model::Boxes;
        self.flags &= !bf::OPAQUE;
        if self.layer == Layer::Opaque {
            self.layer = Layer::Cutout;
        }
        let Some(frame) = self.frame else {
            self.boxes
                .push((min, max, textures, uv, [0; 6], abi::BoxTilt::NONE));
            return;
        };
        // From where the box is on the floor, before it turns.
        let uv: [[f32; 4]; 6] = std::array::from_fn(|i| if uv[i] == abi::UV_FROM_BOX { abi::box_face_uv(i, min, max) } else { uv[i] });
        let (lo, hi, own, turn) = frame.place(min, max, [0usize, 1, 2, 3, 4, 5]);
        self.boxes
            .push((lo, hi, std::array::from_fn(|i| textures[own[i]].clone()), own.map(|i| uv[i]), turn, abi::BoxTilt::NONE));
    }

    fn new(tex: &str) -> Self {
        let d = abi::StateDesc::DEFAULT;
        StateDef {
            flags: d.flags,
            filter: d.filter,
            opacity: None,
            layer: d.layer,
            model: d.model,
            textures: std::array::from_fn(|_| tex.to_string()),
            boxes: Vec::new(),
            hitbox: Vec::new(),
            frame: None,
            components: Vec::new(),
            signal: SignalSpec::default(),
        }
    }
}

#[derive(Default)]
struct BlockCallbacks {
    on_use: Option<abi::Callback>,
    on_neighbor: Option<abi::Callback>,
    on_scheduled: Option<abi::Callback>,
    on_pass: Option<abi::Callback>,
    signal_read: Option<abi::Callback>,
    placement: Option<abi::Callback>,
    merge: Option<abi::Callback>,
    on_grow: Option<abi::Callback>,
    on_signal: Option<abi::Callback>,
    signal_logic: Option<abi::Callback>,
}

/// Per-state settings, used directly and through `BlockBuilder::states`.
pub struct State<'a> {
    def: &'a mut StateDef,
    ns: &'static str,
}

macro_rules! state_setters {
    () => {
        /// Seconds to mine by hand: the `hardness` component, as are the light, friction, drag,
        /// support and tint setters (other mods change them with `r.set`).
        pub fn hardness(mut self, h: f32) -> Self {
            self.each(|s, _| s.put(&c::HARDNESS, h));
            self
        }
        pub fn unbreakable(self) -> Self {
            self.hardness(-1.0)
        }
        /// White light. 15 is full strength; more is allowed and reaches further.
        pub fn light(self, level: u8) -> Self {
            self.light_rgb(level, level, level)
        }
        pub fn light_rgb(mut self, r: u8, g: u8, b: u8) -> Self {
            self.each(|s, _| s.put(&c::LIGHT, [r, g, b]));
            self
        }
        /// Per-channel absorption of passing light (stained glass).
        pub fn filter(mut self, r: u8, g: u8, b: u8) -> Self {
            self.each(|s, _| s.filter = [r, g, b]);
            self
        }
        /// Same texture on every face, relative names are under `block/`.
        pub fn texture(mut self, t: &str) -> Self {
            self.each(|s, ns| s.textures = std::array::from_fn(|_| block_tex(ns, t)));
            self
        }
        /// Top, sides, bottom.
        pub fn textures(mut self, top: &str, side: &str, bottom: &str) -> Self {
            self.each(|s, ns| {
                let (t, si, b) = (block_tex(ns, top), block_tex(ns, side), block_tex(ns, bottom));
                s.textures = [si.clone(), si.clone(), b, t, si.clone(), si];
            });
            self
        }
        /// Faces in -X, +X, -Y, +Y, -Z, +Z order.
        pub fn faces(mut self, f: [&str; 6]) -> Self {
            self.each(|s, ns| s.textures = std::array::from_fn(|i| block_tex(ns, f[i])));
            self
        }
        /// Not opaque, rendered alpha-tested, lets light through (glass, leaves).
        pub fn cutout(mut self) -> Self {
            self.each(|s, _| {
                s.flags &= !bf::OPAQUE;
                s.layer = Layer::Cutout;
            });
            self
        }
        /// Hide faces between neighbors of the same block (glass panes of a window).
        pub fn cull_same(mut self) -> Self {
            self.each(|s, _| s.flags |= bf::CULL_SAME);
            self
        }
        /// Light it takes away, 0..15 (leaves 1, ice 2), when what it looks like isn't enough to
        /// go by: otherwise 15 for what is opaque and 0 for the rest.
        pub fn opacity(mut self, o: u8) -> Self {
            self.each(|s, _| s.opacity = Some(o));
            self
        }
        pub fn translucent(mut self) -> Self {
            self.each(|s, _| {
                s.flags &= !bf::OPAQUE;
                s.flags |= bf::CULL_SAME;
                s.layer = Layer::Translucent;
            });
            self
        }
        /// Needs a block with `tag` underneath (`#plant_soil`): placed only there, and breaks
        /// off when it goes.
        pub fn needs_below(self, tag: &str) -> Self {
            self.needs(crate::Dir::Down, tag)
        }
        /// Needs the neighbor that way to carry `tag` (`#plant_soil`), or to be solid if `tag` is
        /// empty.
        pub fn needs(mut self, side: crate::Dir, tag: &str) -> Self {
            self.each(|s, _| {
                s.put(&c::SUPPORT_FACE, side.face() as u8);
                if !tag.is_empty() {
                    s.put_text(abi::comp::SUPPORT, "tag", tag);
                }
            });
            self
        }
        /// The supporting block must have a block tagged `tag` beside it (sugar cane: `#water`),
        /// unless it is this same block.
        pub fn needs_near_support(mut self, tag: &str) -> Self {
            self.each(|s, _| s.put_text(abi::comp::SUPPORT_NEAR, "tag", tag));
            self
        }
        /// A plant: tagged `plantable`, cross model, no collision, replaced by placing a block.
        pub fn plant(mut self) -> Self {
            self.each(|s, _| {
                s.flags = (s.flags & !(bf::SOLID | bf::OPAQUE)) | bf::REPLACEABLE;
                s.model = Model::Cross;
                s.layer = Layer::Cutout;
                s.put(&c::HARDNESS, 0.0);
                s.components.push(Pending::tag("plantable"));
            });
            self
        }
        /// Keeps the liquid source it is placed in, and a bucket fills it: waterlogging.
        pub fn holds_liquids(mut self) -> Self {
            self.each(|s, _| s.flags |= bf::HOLDS_LIQUID);
            self
        }
        pub fn not_solid(mut self) -> Self {
            self.each(|s, _| s.flags &= !bf::SOLID);
            self
        }
        pub fn replaceable(mut self) -> Self {
            self.each(|s, _| s.flags |= bf::REPLACEABLE);
            self
        }
        pub fn climbable(mut self) -> Self {
            self.each(|s, _| s.flags |= bf::CLIMBABLE);
            self
        }
        /// Colored by the biome it stands in: `tints::GRASS`, `FOLIAGE`, `WATER` or a mod's own
        /// kind (`Registry::tint_kind`).
        pub fn tint(mut self, kind: &TintKind) -> Self {
            self.each(|s, _| s.put_text(abi::comp::TINT, "tint", kind.name()));
            self
        }
        /// `tint` on the top face only (grass blocks: the sides carry their own color).
        pub fn tint_top(mut self, kind: &TintKind) -> Self {
            self.each(|s, _| {
                s.put_text(abi::comp::TINT, "tint", kind.name());
                s.put(&c::TINT_FACES, 1 << 3);
            });
            self
        }
        pub fn friction(mut self, f: f32) -> Self {
            self.each(|s, _| s.put(&c::FRICTION, f));
            self
        }
        pub fn drag(mut self, d: f32) -> Self {
            self.each(|s, _| s.put(&c::DRAG, d));
            self
        }
        /// How many times its loot is rolled when it breaks: 2 for two slabs in one block.
        pub fn loot_rolls(mut self, n: u32) -> Self {
            self.each(|s, _| s.put(&c::LOOT_ROLLS, n));
            self
        }
        /// Custom model made of boxes in 1/16 units, each face showing the state's texture of
        /// that face (`texture`, `textures`, `faces`). It collides and is selected box by box, a
        /// stair being one slab and a quarter, unless `hitbox` is set. A
        /// `models/block/<name>.json` (Blockbench's Java block model) does the same from a file.
        pub fn model_box(mut self, min: [f32; 3], max: [f32; 3]) -> Self {
            self.each(|s, _| s.add_box(min, max, Default::default(), [abi::UV_FROM_BOX; 6]));
            self
        }
        /// `model_box` with its own textures and texel rectangles (u0, v0, u1, v1, 0..16) per face
        /// in -X, +X, -Y, +Y, -Z, +Z order, as a model file's element has; `UV_FROM_BOX` takes it
        /// from where the box is, `UV_NO_FACE` leaves a face out.
        pub fn model_box_faces(mut self, min: [f32; 3], max: [f32; 3], faces: [(&str, [f32; 4]); 6]) -> Self {
            self.each(|s, ns| {
                let textures = std::array::from_fn(|i| block_tex(ns, faces[i].0));
                s.add_box(min, max, textures, faces.map(|f| f.1));
            });
            self
        }
        /// `model_box`, `model_box_faces` and `hitbox` from here on are written for a floor with
        /// the front to the north, and turned as `frame` says (`Props::frame`), pictures included.
        pub fn frame(mut self, frame: crate::signal::Frame) -> Self {
            self.each(|s, _| s.frame = Some(frame));
            self
        }
        /// The last box's pictures get quarter turns of their own, per face.
        pub fn turned(mut self, turns: [u8; 6]) -> Self {
            self.each(|s, _| {
                let frame = s.frame;
                let Some(last) = s.boxes.last_mut() else { return };
                for (own, t) in turns.iter().enumerate() {
                    let to = frame.map_or(own, |f| f.face(own));
                    last.4[to] = (last.4[to] + t) % 4;
                }
            });
            self
        }
        /// The last box is tilted: around a point (texels) about an axis (its own -X, +X ... as a
        /// `Dir`) by degrees, right-handed, like a model file's element.
        pub fn tilted(mut self, origin: [f32; 3], axis: crate::Dir, degrees: f32) -> Self {
            self.each(|s, _| {
                let frame = s
                    .frame
                    .unwrap_or(crate::signal::Frame::new(crate::Dir::Down, crate::Dir::North));
                let Some(last) = s.boxes.last_mut() else { return };
                last.5 = abi::BoxTilt { origin: frame.at(origin), axis: frame.along(axis), degrees };
            });
            self
        }
        /// Doesn't carry redstone power, the way glass doesn't. Wire can still lie on it.
        pub fn redstone_blocker(mut self) -> Self {
            self.each(|s, _| s.flags |= bf::SIGNAL_BLOCKER);
            self
        }
        /// Pistons and floaters can't move it. A state with this on a platform stops the platform, if
        /// more of the platform is pushing it.
        pub fn immovable(mut self) -> Self {
            self.each(|s, _| s.flags |= bf::IMMOVABLE);
            self
        }
        /// A piston moving it takes what touches it along, like slime.
        pub fn sticky(mut self) -> Self {
            self.each(|s, _| s.flags |= bf::STICKY);
            self
        }
        /// Its shape, in 1/16 units like the model's (a slab: `[0.0; 3]` to `[16.0, 8.0, 16.0]`), one
        /// box a call: what stops a walker, what the crosshair outlines and cracks. On a block that
        /// isn't solid (`not_solid`, `plant`), only the outline. Without any, the model's boxes,
        /// else a plant's cross, else the whole block.
        pub fn hitbox(mut self, min: [f32; 3], max: [f32; 3]) -> Self {
            self.each(|s, _| {
                let (lo, hi) = s.frame.map_or((min, max), |f| f.corners(min, max));
                s.hitbox.push([lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]])
            });
            self
        }
        /// Air-like: invisible, no collision, not targetable.
        pub fn air(mut self) -> Self {
            self.each(|s, _| {
                s.flags = (s.flags & !(bf::SOLID | bf::OPAQUE)) | bf::REPLACEABLE | bf::NOT_TARGETABLE;
                s.model = Model::None;
                s.layer = Layer::Opaque;
            });
            self
        }
        pub fn flags(mut self, add: u32, remove: u32) -> Self {
            self.each(|s, _| s.flags = (s.flags | add) & !remove);
            self
        }
    };
}

fn block_tex(_ns: &str, t: &str) -> String {
    crate::asset("block", t)
}

impl<'a> State<'a> {
    fn each(&mut self, f: impl Fn(&mut StateDef, &str)) {
        f(self.def, self.ns)
    }
    state_setters!();
    /// `front` toward `facing`, `side` around it and `top` above and below (a furnace).
    pub fn front_faces(self, facing: crate::Dir, side: &str, top: &str, front: &str) -> Self {
        let mut f = [side; 6];
        f[2] = top;
        f[3] = top;
        f[facing.face() as usize] = front;
        self.faces(f)
    }

    /// A component on this state only (on top of the block's).
    pub fn set<T: Value>(self, comp: &Component<T>, v: T) -> Self {
        self.def.components.push(comp.raw_value(&v));
        self
    }
    pub fn tag(self, tag: &str) -> Self {
        self.def.components.push(Pending::tag(tag));
        self
    }
}

pub struct BlockBuilder<'a> {
    reg: &'a mut Registry,
    name: String,
    handle: Option<&'static Block>,
    states: Vec<StateDef>,
    components: Vec<Pending>,
    /// A loot table to register: (item, count), an empty item = nothing.
    drops: Option<(String, u32)>,
    item: bool,
    item_texture: String,
    callbacks: BlockCallbacks,
    replace: bool,
    own_states: bool,
    retexture: Vec<(String, String)>,
    props: Vec<(String, Vec<String>)>,
    opens: String,
    looks: Option<Box<dyn for<'s> Fn(&Props, State<'s>) -> State<'s>>>,
    facing: Option<(crate::Facings, crate::Toward)>,
    signal: Option<SignalBuild>,
    placement: Option<std::sync::Arc<dyn Fn(&Place, &mut Props) + Send + Sync>>,
    merge: Option<Box<dyn Fn(&Place, &mut Props) + Send + Sync>>,
    made_of: Option<(String, String)>,
    shares_cell: Vec<String>,
}

/// Values a property can take: names (`["false", "true"]`) or numbers (`0..=7`).
pub trait PropValues {
    fn values(self) -> Vec<String>;
}
impl<const N: usize> PropValues for [&str; N] {
    fn values(self) -> Vec<String> {
        self.iter().map(|v| v.to_string()).collect()
    }
}
impl PropValues for &[&str] {
    fn values(self) -> Vec<String> {
        self.iter().map(|v| v.to_string()).collect()
    }
}
impl PropValues for std::ops::RangeInclusive<u32> {
    fn values(self) -> Vec<String> {
        self.map(|v| v.to_string()).collect()
    }
}

/// The property values of one state (in `.states(..)`) or of the state being placed (in
/// `.placement(..)`, where `set` changes them).
#[derive(Clone)]
pub struct Props {
    values: Vec<(String, String)>,
}

impl Props {
    /// The value of `name` (empty if the block has no such property).
    pub fn get(&self, name: &str) -> &str {
        self.values
            .iter()
            .find(|v| v.0 == name)
            .map_or("", |v| v.1.as_str())
    }
    pub fn is(&self, name: &str, value: &str) -> bool {
        self.get(name) == value
    }
    /// A number property (`age`); 0 if it is not a number.
    pub fn int(&self, name: &str) -> u32 {
        self.get(name).parse().unwrap_or(0)
    }
    /// The `facing` property.
    pub fn facing(&self) -> crate::Dir {
        crate::Dir::from_name(self.get("facing")).unwrap_or(crate::Dir::Up)
    }
    /// How the block is turned, from its `attach` and `front` or `facing` properties, for
    /// `.frame`.
    pub fn frame(&self) -> crate::signal::Frame {
        let attach = crate::Dir::from_name(self.get("attach")).unwrap_or(crate::Dir::Down);
        let front = crate::Dir::from_name(self.get("front")).unwrap_or_else(|| self.facing());
        crate::signal::Frame::new(attach, front)
    }
    /// A block that faces along `facing`, with its top that way: for `.frame`.
    pub fn frame_up(&self) -> crate::signal::Frame {
        let f = self.facing();
        let north = if f.face() / 2 == 1 { crate::Dir::North } else { crate::Dir::Up };
        crate::signal::Frame::new(f.opposite(), north)
    }
    /// A block with its north to `facing`, the way vanilla turns one: its top stays up unless it
    /// faces up or down.
    pub fn frame_front(&self) -> crate::signal::Frame {
        let f = self.facing();
        let attach = match f {
            crate::Dir::Up => crate::Dir::North,
            crate::Dir::Down => crate::Dir::South,
            _ => crate::Dir::Down,
        };
        crate::signal::Frame::new(attach, f)
    }
    pub fn set(&mut self, name: &str, value: &str) {
        if let Some(v) = self.values.iter_mut().find(|v| v.0 == name) {
            v.1 = value.to_string();
        }
    }
}

/// Every combination of property values, the last property changing fastest.
fn combinations(props: &[(String, Vec<String>)]) -> Vec<Props> {
    let mut out = vec![Props { values: Vec::new() }];
    for (name, values) in props {
        out = out
            .into_iter()
            .flat_map(|p| {
                values.iter().map(move |v| {
                    let mut q = p.clone();
                    q.values.push((name.clone(), v.clone()));
                    q
                })
            })
            .collect();
    }
    out
}

/// The state index of `p` among `combinations(props)`; values it does not know count as the first.
fn index_of(props: &[(String, Vec<String>)], p: &Props) -> u32 {
    props
        .iter()
        .fold(0, |i, (name, values)| i * values.len() as u32 + values.iter().position(|v| v == p.get(name)).unwrap_or(0) as u32)
}

impl<'a> BlockBuilder<'a> {
    fn each(&mut self, f: impl Fn(&mut StateDef, &str)) {
        let ns = self.reg.ns;
        for s in &mut self.states {
            f(s, ns);
        }
    }

    state_setters!();

    /// A named property with its values, the first being the default: `.prop("lit", ["false",
    /// "true"])`, `.prop("age", 0..=7)`. The block has a state for every combination of its
    /// properties; read and change them with `World::prop` / `set_prop`.
    pub fn prop(mut self, name: &str, values: impl PropValues) -> Self {
        self.own_states = true;
        self.props.push((name.to_string(), values.values()));
        self
    }

    /// Customizes each state by its property values, on top of the block's own settings:
    /// `.states(|p, s| if p.is("lit", "true") { s.light(13) } else { s })`.
    pub fn states(mut self, f: impl for<'s> Fn(&Props, State<'s>) -> State<'s> + 'static) -> Self {
        self.own_states = true;
        self.looks = Some(Box::new(f));
        self
    }

    /// Tools of this class (`tools/<class>` items) mine it at their speed: tags `mineable/<class>`.
    pub fn mineable(mut self, class: &str) -> Self {
        self.components
            .push(Pending::tag(&format!("mineable/{class}")));
        self
    }

    /// Drops nothing unless mined with a fitting tool of at least this level (`abi::tool_tier`:
    /// wood 0, stone 100, iron 200, diamond 300; any number between works), like vanilla's
    /// "requires correct tool". It is the `harvest_level` component, checked by
    /// `Cond::Harvested` on the block's loot.
    pub fn needs_tool(mut self, level: u32) -> Self {
        self.components.push(c::HARVEST_LEVEL.raw_value(&level));
        self
    }

    /// Shades its textures with `shaders/block/<name>.wgsl` (namespace optional): the shader goes
    /// with the textures, so blocks sharing one share it.
    pub fn shader(mut self, name: &str) -> Self {
        let name = crate::asset("", name);
        self.components
            .push(Pending::text(abi::comp::SHADER, &name));
        self
    }

    /// Creative inventory tab of the block's item, see `Registry::creative_tab`.
    pub fn tab(mut self, tab: &CreativeTab) -> Self {
        let tag = format!("tab/{}", tab.name());
        self.components.push(Pending::tag(&tag));
        self
    }

    /// Sounds from the group `<ns>:block.<group>` (`.break`, `.place`, `.step`, `.hit` events);
    /// a name with a namespace is used as it is (`base:block.stone` from another mod).
    pub fn sound_group(mut self, group: &str) -> Self {
        let prefix = if group.contains(':') { group.to_string() } else { format!("{}:block.{group}", self.reg.ns) };
        self.components
            .push(Pending::text(abi::comp::SOUND, &prefix));
        self
    }

    /// Its loot table: `count` of `item` when harvested (`Cond::Harvested`). Without it the
    /// block drops its own item the same way; `r.loot(&BLOCK)` writes any other table.
    pub fn drops(mut self, item: &str, count: u32) -> Self {
        self.drops = Some((item.to_string(), count));
        self
    }

    /// An empty loot table.
    pub fn drops_nothing(mut self) -> Self {
        self.drops = Some((String::new(), 0));
        self
    }

    /// The block's item shows this flat texture from `item/` instead of the block model.
    pub fn item_texture(mut self, t: &str) -> Self {
        self.item_texture = crate::asset("item", t);
        self
    }

    pub fn no_item(mut self) -> Self {
        self.item = false;
        self
    }

    /// Replaces the block of this name registered by a mod this one depends on. Without
    /// `.states(...)` it keeps that block's states (shapes, textures, support, placement and other
    /// callbacks it does not set itself), with the components this builder sets on top.
    pub fn replacing(mut self) -> Self {
        self.replace = true;
        self.item = false;
        self
    }

    /// On a replacement keeping the replaced block's states: block texture `from` (another
    /// mod's, `base:torch`) becomes `to` there, and the item texture of that name too.
    pub fn retexture(mut self, from: &str, to: &str) -> Self {
        self.retexture
            .push((crate::asset("block", from), crate::asset("block", to)));
        self.retexture
            .push((crate::asset("item", from), crate::asset("item", to)));
        self
    }

    /// Grows over time, a step every `mean_seconds` on average (spread like random ticks) while
    /// the light is at least `min_light`: by default to the next state until the last. Timed by
    /// world time, so unloaded plants catch up when their area loads again.
    pub fn growth(mut self, mean_seconds: f32, min_light: u32) -> Self {
        self.each(|s, _| {
            s.put(&c::GROW_TIME, mean_seconds);
            s.put(&c::GROW_LIGHT, min_light);
        });
        self
    }

    /// What a growth step does instead of moving to the next state (growing a tree, a block on
    /// top); it keeps growing as long as the block stays.
    pub fn on_grow(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_grow = Some(callback(move |a: &mut abi::BlockEventArgs| f(BlockEvent::wrap(a))));
        self
    }

    /// Spreads onto blocks with `tag` near it (grass onto `#grass_spreads_to`): a target
    /// within 1 block sideways, 3 down and 1 up with `min_light` above it and nothing opaque on
    /// top turns into this block in `mean_seconds` on average (sooner with more of this around),
    /// also while nobody is near (caught up on loading).
    pub fn spreads(mut self, tag: &str, mean_seconds: f32, min_light: u32) -> Self {
        self.each(|s, _| {
            s.put_text(abi::comp::SPREADS_TO, "tag", tag);
            s.put(&c::SPREAD_TIME, mean_seconds);
            s.put(&c::SPREAD_LIGHT, min_light);
        });
        self
    }

    /// Turns into `block` in `mean_seconds` on average once something opaque covers it (grass
    /// into dirt).
    pub fn smothered(mut self, block: &str, mean_seconds: f32) -> Self {
        self.each(|s, _| {
            s.put_text(abi::comp::SMOTHERED_INTO, "block", block);
            s.put(&c::SMOTHER_TIME, mean_seconds);
        });
        self
    }

    /// Particles of `kind` clients show around it now and then, near the player (vanilla's
    /// torch smoke, a crystal's sparks). A kind nothing registers is a load error.
    pub fn ambient_particle(self, kind: &str) -> Self {
        self.ambient_particles(&[kind])
    }

    /// Like `ambient_particle` with several kinds at once (a torch's flame and smoke).
    pub fn ambient_particles(mut self, kinds: &[&str]) -> Self {
        let kinds = kinds
            .iter()
            .map(|k| crate::full_name(k))
            .collect::<Vec<_>>()
            .join(",");
        self.each(|s, _| s.put_text(abi::comp::AMBIENT_PARTICLE, "str", &kinds));
        self
    }

    pub fn on_use(mut self, f: impl Fn(&mut Use) -> bool + Send + Sync + 'static) -> Self {
        self.callbacks.on_use = Some(callback(move |a: &mut abi::UseArgs| {
            let handled = f(Use::wrap(a));
            a.handled = handled as u32;
        }));
        self
    }

    /// One of these was placed: `BLOCK_PLACED` for this block, at the end of the tick.
    pub fn on_placed(self, f: impl Fn(&World, &crate::BlockChange) + Send + Sync + 'static) -> Self {
        let Some(block) = self.handle else { return self };
        self.reg.on(&crate::BLOCK_PLACED, move |b| {
            let w = b.world();
            for p in b.iter().filter(|p| block.is(p.state)) {
                f(&w, p);
            }
        });
        self
    }

    /// One of these was broken, by a player (`player`) or by losing its support (0).
    pub fn on_broken(self, f: impl Fn(&World, &crate::BlockChange) + Send + Sync + 'static) -> Self {
        let Some(block) = self.handle else { return self };
        self.reg.on(&crate::BLOCK_BROKEN, move |b| {
            let w = b.world();
            for p in b.iter().filter(|p| block.is(p.state)) {
                f(&w, p);
            }
        });
        self
    }

    pub fn on_neighbor(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_neighbor = Some(callback(move |a: &mut abi::BlockEventArgs| f(BlockEvent::wrap(a))));
        self
    }

    /// On a moving body, it came to another cell of the world: a drill, a plow. `pos` is that cell,
    /// in the world's coordinates; `other` where this is on the body, `entity` the body.
    pub fn on_pass(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_pass = Some(callback(move |a: &mut abi::BlockEventArgs| f(BlockEvent::wrap(a))));
        self
    }
    pub fn on_scheduled(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_scheduled = Some(callback(move |a: &mut abi::BlockEventArgs| f(BlockEvent::wrap(a))));
        self
    }

    /// Two cells long, told apart by a property `prop` of it: it is placed as `parts[0]`, and
    /// `parts[1]` goes where `way` says from there (given the state), which needs room. Breaking
    /// one takes the other, whatever took the one (an explosion doesn't break it): `drops` and
    /// the like are for the two together.
    pub fn two_part(
        self,
        prop: &'static str,
        parts: [&'static str; 2],
        way: impl Fn(u32) -> crate::Dir + Copy + Send + Sync + 'static,
    ) -> Self {
        let Some(block) = self.handle else {
            crate::mistake("two_part needs a block registered by its handle");
            return self;
        };
        let other = move |pos: [i32; 3], state: u32| crate::other_part(pos, state, prop, parts[1], way);
        self.reg.intercept(&crate::question::PLACE_BLOCK, move |q| {
            if !block.is(q.state()) {
                return crate::Verdict::Abstain;
            }
            let there = q.world().get(other(q.pos(), q.state()));
            if there == 0 || crate::state_flags(there) & bf::REPLACEABLE != 0 { crate::Verdict::Abstain } else { crate::Verdict::Deny }
        });
        self.prop(prop, parts)
            .on_neighbor(|e| e.world().schedule(e.pos(), 1))
            .on_scheduled(move |e| {
                let w = e.world();
                if block.is(e.state()) && !block.is(w.get(other(e.pos(), e.state()))) {
                    w.set(e.pos(), 0);
                }
            })
            .on_placed(move |w, c| {
                if let Some(s) = crate::state_with(c.state, prop, parts[1]) {
                    w.set(other(c.pos, c.state), s);
                }
            })
            .on_broken(move |w, c| {
                let there = other(c.pos, c.state);
                if block.is(w.get(there)) {
                    w.set(there, 0);
                }
            })
    }

    /// Falls like sand when nothing holds it up: a block next to it changing checks two ticks
    /// later whether below is free, and if so it drops as a moving block (`World::fall`).
    pub fn falling(self) -> Self {
        self.on_neighbor(|e| e.world().schedule(e.pos(), 2))
            .on_scheduled(|e| {
                let (w, p) = (e.world(), e.pos());
                if w.flags([p[0], p[1] - 1, p[2]]) & abi::block_flags::REPLACEABLE != 0 {
                    w.fall(p);
                }
            })
    }

    /// Using it opens `menu`, on the block's block entity if it has one (machines), else a menu
    /// of its own (a crafting table). A menu with `.recipes(..)` makes the block a station of
    /// those recipes in the recipe viewer.
    pub fn opens(mut self, menu: &crate::Menu) -> Self {
        self.opens = menu.name().to_string();
        self
    }

    /// A `facing` property with the directions in `allowed` (`p.facing()` in `.states`), set by
    /// `toward` when placed: three allowed directions are three values.
    pub fn facing(mut self, allowed: crate::Facings, toward: crate::Toward) -> Self {
        self.facing = Some((allowed, toward));
        self.prop(
            "facing",
            allowed
                .dirs()
                .map(|d| d.name())
                .collect::<Vec<_>>()
                .as_slice(),
        )
    }

    /// Which way it faces as an item and by default (after `facing`).
    pub fn facing_first(mut self, d: crate::Dir) -> Self {
        if let Some((_, values)) = self.props.iter_mut().find(|p| p.0 == "facing") {
            if let Some(i) = values.iter().position(|v| v == d.name()) {
                values.swap(0, i);
            }
        }
        self
    }

    /// Wire that lies on any face of a block, connecting to wire of the same block only; textures
    /// are the dot on top and the line on the sides. A property `faces` holds where it lies.
    pub fn wire(mut self) -> Self {
        self.each(|s, _| {
            s.flags &= !(bf::SOLID | bf::OPAQUE);
            s.model = Model::Wire;
            s.layer = Layer::Cutout;
            s.put(&c::HARDNESS, 0.0);
        });
        self.signal = Some(SignalBuild { kind: abi::signal_kind::WIRE, ..SignalBuild::default() });
        self.prop("faces", 0..=63)
    }

    fn signal_props(mut self, kind: u32, levels: u32, attach: bool, front: bool) -> Self {
        self.signal = Some(SignalBuild { kind, attaches: attach, ..self.signal.unwrap_or_default() });
        if attach {
            self = self.prop("attach", ["down", "up", "north", "south", "west", "east"]);
        }
        if front {
            self = self.prop("front", ["north", "south", "west", "east", "up", "down"]);
        }
        match levels {
            0 => self,
            2 => self.prop("power", ["off", "on"]),
            3 => self.prop("power", 0..=2),
            _ => self.prop("power", 0..=15),
        }
    }

    /// Always gives full power to what is next to it.
    pub fn signal_block(self) -> Self {
        self.signal_props(abi::signal_kind::SOURCE, 0, false, false)
    }
    /// A lever or button: goes on the clicked face; its `power` property (`off`, `on`) gives full
    /// power around it and strongly into the block it is on.
    pub fn signal_switch(self) -> Self {
        self.signal_props(abi::signal_kind::SOURCE, 2, true, true)
    }
    /// Goes on the clicked face and gives full power unless the block it is on has any.
    pub fn signal_torch(self) -> Self {
        self.signal_props(abi::signal_kind::TORCH, 2, true, false)
    }
    /// Goes on the clicked face; full power out of the front, a `delay` property (1..4 redstone
    /// ticks, two game ticks each) after what is behind it has any.
    pub fn signal_repeater(self) -> Self {
        self.signal_props(abi::signal_kind::REPEATER, 2, true, true)
            .prop("delay", 1..=4)
    }
    /// Goes on the clicked face; out of the front what is behind it, unless a side has more. Its
    /// `mode` property (`compare`, `subtract`) takes the sides from it instead.
    pub fn signal_comparator(self) -> Self {
        self.signal_props(abi::signal_kind::COMPARATOR, 16, true, true)
            .prop("mode", ["compare", "subtract"])
    }
    /// A pulse out of its back when the block at its `facing` changes (after `facing`).
    pub fn signal_observer(self) -> Self {
        self.signal_props(abi::signal_kind::OBSERVER, 2, false, false)
    }
    /// Its `power` property, if it has one, and `on_signal` follow the power around it.
    pub fn signal_sink(self) -> Self {
        self.signal_props(abi::signal_kind::SINK, 0, false, false)
    }
    /// What a comparator behind it reads, 0..15, instead of how full it is. It looks again when
    /// the block changes; for anything else it depends on, call `World::signal_read_changed`.
    pub fn signal_read(mut self, f: impl Fn(&World, [i32; 3]) -> u8 + Send + Sync + 'static) -> Self {
        self.callbacks.signal_read =
            Some(callback(move |a: &mut abi::SignalReadArgs| a.output = f(&World::from_raw(a.world), a.pos) as u32));
        self
    }
    /// Goes along its `facing` (after `facing`), any of the six ways. Its front is what sticks: with
    /// power from any other side it carries itself, the block in front of it and what is stuck to
    /// that the way slime does, half a block a second per level of power. Circuits on it go on.
    /// Without power, or against what it can't go through, it all lands on a whole block.
    /// `power`: 0 idle, 1 going, 2 stuck. `breaks`: it breaks what is in its way instead.
    pub fn signal_floater(mut self, breaks: bool) -> Self {
        self.signal = Some(SignalBuild { mode: breaks as u32, ..SignalBuild::default() });
        self.signal_props(abi::signal_kind::FLOATER, 3, false, false)
    }
    /// Pushes blocks at its `facing` with power from any other side, the `power` property saying
    /// it is out (after `facing`). `head`: the block it pushes out, with a `facing` property.
    /// `sticky` brings a block back.
    pub fn signal_piston(mut self, head: &'static str, sticky: bool) -> Self {
        self.signal = Some(SignalBuild { head, mode: sticky as u32, ..SignalBuild::default() });
        self.signal_props(abi::signal_kind::PISTON, 2, false, false)
    }
    /// The moving part of a piston (after `facing`): it goes when the piston does.
    pub fn signal_piston_head(mut self) -> Self {
        self.each(|s, _| s.flags |= bf::NOT_STOOD_ON);
        self.signal_props(abi::signal_kind::PISTON_HEAD, 0, false, false)
    }
    /// A gate with logic of your own, on the clicked face: what comes out of the front `delay`
    /// game ticks after what goes in calls for it. Runs only when something around it changes.
    pub fn signal_logic(mut self, delay: u32, f: impl Fn(&SignalLogic) -> u8 + Send + Sync + 'static) -> Self {
        self.callbacks.signal_logic = Some(callback(move |a: &mut abi::LogicArgs| {
            a.output = f(SignalLogic::wrap(a)) as u32;
        }));
        // In redstone ticks, like a repeater's: two game ticks each.
        self.signal = Some(SignalBuild { delay: Some(delay * 2), ..self.signal.unwrap_or_default() });
        self.signal_props(abi::signal_kind::LOGIC, 16, true, true)
    }
    /// Called when the power around a `signal_sink` changes.
    pub fn on_signal(mut self, f: impl Fn(&SignalEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_signal = Some(callback(move |a: &mut abi::SignalArgs| f(SignalEvent::wrap(a))));
        self
    }

    /// Its own item placed in its cell: `.merge(|p, props| props.set("type", "double"))` takes the
    /// item in, with `props` those of what is there and `p.placed("type")` how the item would go;
    /// leaving them as they are places it as usual.
    pub fn merge(mut self, f: impl Fn(&Place, &mut Props) + Send + Sync + 'static) -> Self {
        self.merge = Some(Box::new(f));
        self
    }

    /// What may be in one cell with it though their shapes overlap (`#tags`, names): snow on a
    /// plant. Blocks whose shapes don't overlap share a cell without asking.
    pub fn shares_cell_with(mut self, what: &str) -> Self {
        self.shares_cell.push(what.to_string());
        self
    }

    /// Not a block but a template: see `Registry::shapes_of`.
    pub(crate) fn made_of(mut self, sources: &str, suffix: &str) -> Self {
        self.made_of = Some((sources.to_string(), suffix.to_string()));
        self
    }

    /// Chooses property values when a player places it (after `facing`):
    /// `.placement(|p, props| props.set("axis", if p.face().horizontal() { "x" } else { "y" }))`.
    pub fn placement(mut self, f: impl Fn(&Place, &mut Props) + Send + Sync + 'static) -> Self {
        self.placement = Some(std::sync::Arc::new(f));
        self
    }
}

impl Drop for BlockBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        // One state per combination of property values, each customized by `.states(..)`.
        let looks = self.looks.take();
        let signal = self.signal;
        if !self.props.is_empty() || looks.is_some() {
            let template = self.states[0].clone();
            let ns = self.reg.ns;
            self.states = combinations(&self.props)
                .iter()
                .map(|p| {
                    let mut d = template.clone();
                    if let Some(f) = &looks {
                        f(p, State { def: &mut d, ns });
                    }
                    if let Some(b) = &signal {
                        b.apply(p, &mut d, ns);
                    }
                    d
                })
                .collect();
        } else if let Some(b) = &signal {
            let none = Props { values: Vec::new() };
            let ns = self.reg.ns;
            for d in &mut self.states {
                b.apply(&none, d, ns);
            }
        }
        let attaches = signal.is_some_and(|b| b.attaches);
        let switch = signal.is_some_and(|b| b.kind == abi::signal_kind::SOURCE);
        if self.facing.is_some() || self.placement.is_some() || attaches {
            let (props, facing, user) = (self.props.clone(), self.facing, self.placement.take());
            self.callbacks.placement = Some(callback(move |a: &mut abi::PlaceArgs| {
                let mut p = Props {
                    values: props
                        .iter()
                        .map(|(n, v)| (n.clone(), v[0].clone()))
                        .collect(),
                };
                if let Some((allowed, toward)) = facing {
                    p.set("facing", toward.choose(allowed, a.face, a.look).name());
                }
                if attaches {
                    let attach = crate::Dir::from_face(a.face).opposite();
                    p.set("attach", attach.name());
                    // A switch on a wall is always upright, as in vanilla; a gate goes with the look.
                    let front = if switch && attach.horizontal() { crate::Dir::Down } else { crate::signal::along(attach, a.look) };
                    p.set("front", front.name());
                }
                let first = a.state;
                if let Some(f) = &user {
                    f(Place::wrap(a), &mut p);
                }
                a.state = first + index_of(&props, &p);
            }));
        }
        if let Some(f) = self.merge.take() {
            let props = self.props.clone();
            self.callbacks.merge = Some(callback(move |a: &mut abi::PlaceArgs| {
                let Some(mut p) = combinations(&props)
                    .into_iter()
                    .nth((a.state - a.first) as usize)
                else {
                    return;
                };
                f(Place::wrap(a), &mut p);
                a.state = a.first + index_of(&props, &p);
            }));
        }
        let shares_cell: Vec<Str> = self.shares_cell.iter().map(|x| Str::new(x)).collect();
        let prop_values: Vec<Vec<Str>> = self
            .props
            .iter()
            .map(|(_, v)| v.iter().map(|x| Str::new(x)).collect())
            .collect();
        let props: Vec<abi::PropDesc> = self
            .props
            .iter()
            .zip(&prop_values)
            .map(|((n, _), v)| abi::PropDesc { name: Str::new(n), values: Slice::new(v) })
            .collect();
        let boxes: Vec<Vec<abi::ModelBox>> = self
            .states
            .iter()
            .map(|s| {
                s.boxes
                    .iter()
                    .map(|(min, max, t, uv, turn, tilt)| abi::ModelBox {
                        turn: *turn,
                        tilt: *tilt,
                        min: *min,
                        max: *max,
                        textures: std::array::from_fn(|i| Str::new(&t[i])),
                        uv: *uv,
                    })
                    .collect()
            })
            .collect();
        let mut scratch: Vec<Vec<Vec<u8>>> = vec![Vec::new(); self.states.len()];
        let state_values: Vec<Vec<abi::CompValue>> = self
            .states
            .iter()
            .zip(scratch.iter_mut())
            .map(|(s, sc)| raw_values(&s.components, sc))
            .collect();
        let states: Vec<abi::StateDesc> = self
            .states
            .iter()
            .zip(&boxes)
            .zip(&state_values)
            .map(|((s, b), v)| abi::StateDesc {
                flags: s.flags,
                filter: s.filter,
                opacity: s
                    .opacity
                    .unwrap_or(if s.flags & bf::OPAQUE != 0 { 15 } else { 0 }),
                layer: s.layer,
                model: s.model,
                textures: std::array::from_fn(|i| Str::new(&s.textures[i])),
                boxes: Slice::new(b),
                hitbox: Slice::new(&s.hitbox),
                components: Slice::new(v),
                signal: abi::SignalDesc {
                    kind: s.signal.kind,
                    attach: s.signal.attach,
                    front: s.signal.front,
                    delay: s.signal.delay,
                    mode: s.signal.mode,
                    head: Str::new(s.signal.head),
                },
                ..abi::StateDesc::DEFAULT
            })
            .collect();
        let mut scratch = Vec::new();
        let values = raw_values(&self.components, &mut scratch);
        let retexture: Vec<Str> = self
            .retexture
            .iter()
            .flat_map(|(a, b)| [Str::new(a), Str::new(b)])
            .collect();
        let cb = &self.callbacks;
        let desc = abi::BlockDesc {
            name: Str::new(&self.name),
            cell: cell(self.handle),
            states: Slice::new(&states),
            props: Slice::new(&props),
            components: Slice::new(&values),
            on_use: cb.on_use.unwrap_or(abi::Callback::NONE),

            on_neighbor: cb.on_neighbor.unwrap_or(abi::Callback::NONE),
            on_scheduled: cb.on_scheduled.unwrap_or(abi::Callback::NONE),
            on_pass: cb.on_pass.unwrap_or(abi::Callback::NONE),
            signal_read: cb.signal_read.unwrap_or(abi::Callback::NONE),
            merge: cb.merge.unwrap_or(abi::Callback::NONE),
            derive_from: self.made_of.as_ref().map_or(Str::EMPTY, |m| Str::new(&m.0)),
            derive_suffix: self.made_of.as_ref().map_or(Str::EMPTY, |m| Str::new(&m.1)),
            shares_cell: Slice::new(&shares_cell),
            placement: cb.placement.unwrap_or(abi::Callback::NONE),
            replace: self.replace as u32,
            on_grow: cb.on_grow.unwrap_or(abi::Callback::NONE),
            on_signal: cb.on_signal.unwrap_or(abi::Callback::NONE),
            signal_logic: cb.signal_logic.unwrap_or(abi::Callback::NONE),
            own_states: (self.own_states || !self.replace) as u32,
            retexture: Slice::new(&retexture),
            opens: Str::new(&self.opens),
            ..abi::BlockDesc::DEFAULT
        };
        if let Some(h) = self.handle {
            h.set_count(self.states.len() as u32);
        }
        unsafe { (host().reg_block)(self.reg.raw, &desc) };

        if self.item {
            // The item gets the block's components from the engine.
            let item = ItemBuilder {
                _p: PhantomData,
                raw: self.reg.raw,
                name: self.name.clone(),
                texture: std::mem::take(&mut self.item_texture),
                handle: None,
                block: self.name.clone(),
                components: Vec::new(),
                on_use: abi::Callback::NONE,
            };
            drop(item);
        }
        if let Some((item, count)) = self.drops.take() {
            let pools = if item.is_empty() {
                vec![]
            } else {
                vec![
                    crate::extend::Pool::new()
                        .when(crate::extend::Cond::Harvested)
                        .items(&item, count, count),
                ]
            };
            let raw = crate::extend::raw_pools(self.reg.ns, &pools);
            let desc = abi::LootDesc { name: Str::new(&self.name), pools: Slice::new(&raw.pools), ..abi::LootDesc::DEFAULT };
            unsafe { (host().reg_loot)(self.reg.raw, &desc) }
        }
    }
}

pub struct ItemBuilder<'a> {
    _p: PhantomData<&'a mut Registry>,
    raw: *mut abi::Registrar,
    name: String,
    texture: String,
    handle: Option<&'static Item>,
    block: String,
    components: Vec<Pending>,
    on_use: abi::Callback,
}

impl ItemBuilder<'_> {
    pub fn texture(mut self, t: &str) -> Self {
        self.texture = crate::asset("item", t);
        self
    }
    /// Stack size (`max_stack`).
    pub fn max_stack(self, n: u32) -> Self {
        self.set(&c::MAX_STACK, n)
    }
    /// Its clips, from `animations/<name>.animation.json`; a file named after the item is used
    /// even without this. They play on whoever holds it.
    pub fn animations(mut self, name: &str) -> Self {
        let name = crate::asset("", name);
        self.components
            .push(Pending::text(abi::comp::ANIMATIONS, &name));
        self
    }
    /// Needs both hands: while the other holds something it does nothing, and it can't go in
    /// the other hand. Its `hold` action shows the two.
    pub fn two_handed(self) -> Self {
        self.tag(abi::comp::TWO_HANDED)
    }
    /// A `two_handed` item may be held in one hand after all, playing this action. It goes in
    /// the other hand then too.
    pub fn one_handed_action(mut self, action: &str) -> Self {
        self.components
            .push(Pending::text(abi::comp::ONE_HANDED, action));
        self.tag(abi::comp::TWO_HANDED)
    }
    /// On a right click the hand whose item has the higher priority goes first, whichever hand
    /// that is (default 0, then the main hand first). The other hand gets its turn when this
    /// item does nothing: a bow with no arrows.
    pub fn use_priority(self, priority: i32) -> Self {
        self.set(&c::USE_PRIORITY, priority)
    }
    /// Uses before it breaks (`durability`); damageable items do not stack.
    pub fn durability(self, d: u32) -> Self {
        self.set(&c::DURABILITY, d).set(&c::MAX_STACK, 1)
    }
    /// A tool of `class` (tag `tools/<class>`): it mines blocks `mineable/<class>`, at its
    /// `mining_speed`, those needing up to its `tool_level`. Held like one (`handheld`).
    pub fn tool(self, class: &str) -> Self {
        let tag = format!("tools/{class}");
        self.tag(&tag).handheld()
    }
    /// Held like a tool in third person, by its end and pointing ahead (tag `handheld`, vanilla's
    /// `item/handheld` model), instead of flat in the hand. Tools have it already.
    pub fn handheld(self) -> Self {
        self.tag("handheld")
    }
    /// The highest `needs_tool` level it harvests (`abi::tool_tier`: wood 0 ... diamond 300).
    pub fn tool_level(self, level: u32) -> Self {
        self.set(&c::TOOL_LEVEL, level)
    }
    /// How fast it mines its class's blocks (vanilla wood 2 ... gold 12).
    pub fn mining_speed(self, speed: f32) -> Self {
        self.set(&c::MINING_SPEED, speed)
    }
    /// A full swing on the ground also hits what stands next to the target, like a sword.
    pub fn sweeps(self) -> Self {
        self.tag(abi::comp::SWEEPS)
    }
    /// For fighting: in creative it doesn't break blocks, as a sword in modern. Client and server
    /// both know, so nothing flickers. A rule that depends on more is `question::BREAK_BLOCK`.
    pub fn no_creative_break(self) -> Self {
        self.tag(abi::comp::NO_CREATIVE_BREAK)
    }
    /// Takes effect after holding the use button this long (food 32 ticks), slowing the player down.
    pub fn use_time(self, seconds: f32) -> Self {
        self.set(&c::USE_TICKS, (seconds * 20.0).round() as u32)
    }
    /// Full swings per second: hitting sooner does less (a sword 1.6, an empty hand 4).
    pub fn attack_speed(self, per_second: f32) -> Self {
        self.set(&c::ATTACK_SPEED, per_second)
    }
    pub fn attack_damage(self, damage: f32) -> Self {
        self.set(&c::ATTACK_DAMAGE, damage)
    }
    pub fn placed_block(mut self, block: &Block) -> Self {
        self.block = block.name().to_string();
        self
    }
    /// Worn in equipment `slot`, where it shows as `model` on the wearer's bone `bone`, moved by
    /// what plays on the wearer, like `Registry::attach_model`: a tail only those who wear it have.
    pub fn wearable_model(self, slot: &EquipmentSlot, model: &str, bone: &str) -> Self {
        let model = format!("{}@{bone}", crate::asset("", model));
        self.tag(&format!("worn/{}", slot.name()))
            .set(&c::WORN, abi::Worn { roles: 0, inflate: 0.0 })
            .set_text(&c::WORN_MODEL, &model)
    }

    /// Creative inventory tab, see `Registry::creative_tab`.
    pub fn tab(self, tab: &CreativeTab) -> Self {
        let tag = format!("tab/{}", tab.name());
        self.tag(&tag)
    }
    /// Worn in equipment `slot` (any mod's), where it shows on the wearer's model over the parts
    /// the slot covers: `texture` (under `entity/`, like `armor/iron_1`) in the wearer's skin
    /// layout, `inflate` pixels out from the body.
    pub fn wearable(self, slot: &EquipmentSlot, inflate: f32, texture: &str) -> Self {
        let tex = crate::asset("entity", texture);
        self.tag(&format!("worn/{}", slot.name()))
            .set(&c::WORN, abi::Worn { roles: 0, inflate })
            .set_text(&c::WORN_TEXTURE, &tex)
    }

    /// Used on a block, puts `entity` on that side of it. One is used up, outside creative. Entities
    /// have an egg of their own without this; it is for something else that hatches one.
    pub fn spawns(mut self, entity: &'static crate::Entity) -> Self {
        self.components
            .push(Pending::text(abi::comp::SPAWN_EGG, entity.name()));
        self.on_use(move |u| {
            let Some((p, face)) = u.target() else { return false };
            let (w, side) = (u.world(), crate::Dir::from_face(face).offset());
            w.spawn(entity, [(p[0] + side[0]) as f64 + 0.5, (p[1] + side[1]) as f64, (p[2] + side[2]) as f64 + 0.5]);
            if !w.creative(u.player()) {
                u.set_stack(crate::StackData::shrink(u.stack(), 1));
            }
            true
        })
    }

    pub fn on_use(mut self, f: impl Fn(&mut ItemUse) -> bool + Send + Sync + 'static) -> Self {
        self.on_use = callback(move |a: &mut abi::ItemUseArgs| {
            let handled = f(ItemUse::wrap(a));
            a.handled = handled as u32;
        });
        self
    }
}

impl Drop for ItemBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let mut scratch = Vec::new();
        let values = raw_values(&self.components, &mut scratch);
        let desc = abi::ItemDesc {
            name: Str::new(&self.name),
            cell: cell(self.handle),
            texture: Str::new(&self.texture),
            block: Str::new(&self.block),
            components: Slice::new(&values),
            on_use: self.on_use,
            ..abi::ItemDesc::DEFAULT
        };
        unsafe { (host().reg_item)(self.raw, &desc) }
    }
}

pub struct BiomeBuilder<'a> {
    _p: PhantomData<&'a mut Registry>,
    raw: *mut abi::Registrar,
    ns: &'static str,
    name: String,
    handle: Option<&'static Biome>,
    rule: crate::terrain::BiomeRule,
    tints: Vec<(&'static TintKind, u32)>,
    components: Vec<Pending>,
}

/// A biome's climate and surface. Terrain rules placing it start from these
/// (`BiomeRule::new("desert")` needs nothing else) and may change them there.
impl BiomeBuilder<'_> {
    fn edit(mut self, f: impl FnOnce(crate::terrain::BiomeRule) -> crate::terrain::BiomeRule) -> Self {
        self.rule = f(std::mem::replace(&mut self.rule, crate::terrain::BiomeRule::blank()));
        self
    }
    /// Where the climate fits it, like `0.35..` or `..-0.45` (from inclusive, to exclusive);
    /// the terrain's fields are about -1.4..1.4. Without one it fits everywhere.
    pub fn temperature(self, range: impl std::ops::RangeBounds<f64>) -> Self {
        self.edit(|r| r.temperature(range))
    }
    pub fn humidity(self, range: impl std::ops::RangeBounds<f64>) -> Self {
        self.edit(|r| r.humidity(range))
    }
    /// Top block, then `depth` blocks of filler, then the terrain's stone.
    pub fn surface(self, top: &str, filler: &str, depth: i32) -> Self {
        self.edit(|r| r.surface(top, filler, depth))
    }
    /// The lowest `layers` of the filler become `block` (sandstone under sand).
    pub fn under(self, block: &str, layers: i32) -> Self {
        self.edit(|r| r.under(block, layers))
    }
    /// Top block for columns lower than `below` (gravel on deep sea floors).
    pub fn deep_top(self, block: &str, below: i32) -> Self {
        self.edit(|r| r.deep_top(block, below))
    }
    /// Plain stone above this height (bare peaks).
    pub fn bare_above(self, y: i32) -> Self {
        self.edit(|r| r.bare_above(y))
    }
    /// A frozen biome: the water surface freezes into `block`, and it has the tag `frozen`
    /// (decorations put no flowers there).
    pub fn frozen(self, block: &str) -> Self {
        self.edit(|r| r.frozen(block)).tag("frozen")
    }
    /// Rain falls as snow here.
    pub fn snows(self) -> Self {
        self.tag(abi::comp::SNOWY)
    }
    /// The color (`0xRRGGBB`) blocks tinted with `kind` take here (`tints::GRASS`, or a mod's
    /// own kind). Kinds not given keep their default.
    pub fn tint(mut self, kind: &'static TintKind, color: u32) -> Self {
        self.tints.push((kind, color));
        self
    }
}

impl Drop for BiomeBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let mut scratch = Vec::new();
        let values = raw_values(&self.components, &mut scratch);
        let names = self.rule.names(self.ns);
        let desc = abi::BiomeDesc {
            name: Str::new(&self.name),
            cell: cell(self.handle),
            rule: self.rule.desc(&names),
            components: Slice::new(&values),
        };
        unsafe { (host().reg_biome)(self.raw, &desc) }
        for (kind, color) in &self.tints {
            unsafe { (host().reg_biome_tint)(self.raw, Str::new(&self.name), Str::new(kind.name()), *color) }
        }
    }
}

pub struct DimensionBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    handle: Option<&'static Dimension>,
    desc: abi::DimensionDesc,
    components: Vec<Pending>,
    _p: PhantomData<&'a mut Registry>,
}

impl DimensionBuilder<'_> {
    /// Must be a pure function of its input: it runs in parallel and must give the same cube for the
    /// same seed and position on every machine.
    pub fn generator(mut self, f: impl Fn(&mut Gen) + Send + Sync + 'static) -> Self {
        self.desc.generator = callback(move |a: &mut abi::GenArgs| f(Gen::wrap(a)));
        self
    }
    /// The block a player starts on top of in this dimension, for a seed. Without one the engine
    /// finds open ground near 0, 0 on the dimension's columns (biomes tagged `spawn_biome`
    /// first), out of caves and trees.
    pub fn spawn(mut self, f: impl Fn(u64) -> [i32; 3] + Send + Sync + 'static) -> Self {
        self.desc.spawn = callback(move |a: &mut abi::SpawnArgs| a.pos = f(a.seed));
        self
    }
    /// Players start here and respawn here. When several mods claim it, the one loaded last
    /// wins; without any, the first dimension registered is used.
    pub fn player_spawn(mut self) -> Self {
        self.desc.flags |= abi::dim_flags::PLAYER_SPAWN;
        self
    }
    /// Blocks never update here (no neighbor updates, block ticks or growth).
    pub fn no_block_updates(mut self) -> Self {
        self.desc.flags |= abi::dim_flags::NO_BLOCK_UPDATES;
        self
    }
    /// Only what names it: ores, scatters and decorations meant for every dimension stay out
    /// (a superflat test world).
    pub fn bare(mut self) -> Self {
        self.desc.flags |= abi::dim_flags::BARE;
        self
    }
    /// Surface height and biome id of column (x, z) for (seed, x, z), for a dimension without a
    /// terrain. The generator reads them from `g.columns()` instead of working them out again,
    /// and structures, scatters (trees) and the spawn search stand on them.
    pub fn column(mut self, f: impl Fn(u64, i32, i32) -> (i32, u32) + Send + Sync + 'static) -> Self {
        self.desc.column = callback(move |a: &mut abi::ColumnArgs| (a.height, a.biome) = f(a.seed, a.x, a.z));
        self
    }
    /// Inclusive limits in cubes (16 blocks).
    pub fn height(mut self, min_cube: i32, max_cube: i32) -> Self {
        self.desc.min_cube_y = min_cube;
        self.desc.max_cube_y = max_cube;
        self
    }
    /// Everything at or above this cube y is empty, so sky light there needs no generation.
    pub fn sky_above(mut self, cube_y: i32) -> Self {
        self.desc.sky_cube_y = cube_y;
        self
    }
    /// It rains and thunders here now and then, by vanilla's timing.
    pub fn weather(mut self) -> Self {
        self.desc.flags |= abi::dim_flags::WEATHER;
        self
    }
    pub fn no_sky_light(mut self) -> Self {
        self.desc.sky_light = 0;
        self
    }
    /// Blocks per second squared.
    pub fn gravity(mut self, g: f32) -> Self {
        self.desc.gravity = g;
        self
    }
    pub fn day_length(mut self, ticks: u32) -> Self {
        self.desc.day_length = ticks;
        self
    }
    pub fn fixed_time(mut self, time: u32) -> Self {
        self.desc.day_length = 0;
        self.desc.fixed_time = time;
        self
    }
    pub fn sky(mut self, sky: u32, fog: u32) -> Self {
        self.desc.sky_color = sky;
        self.desc.fog_color = fog;
        self
    }
    pub fn ambient(mut self, level: u32) -> Self {
        self.desc.ambient = level;
        self
    }
}

impl Drop for DimensionBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let mut scratch = Vec::new();
        let values = raw_values(&self.components, &mut scratch);
        let desc = abi::DimensionDesc { name: Str::new(&self.name), cell: cell(self.handle), components: Slice::new(&values), ..self.desc };
        unsafe { (host().reg_dimension)(self.raw, &desc) }
    }
}

/// A structure placed while generating (see `abi::StructureDesc`), drawn in layers from the
/// bottom up, rows along z, characters along x; `' '` leaves what is there, other characters
/// are keys:
///
/// ```ignore
/// r.structure("overworld", "well")
///     .layer(&["ccc", "cwc", "ccc"]).layer(&["c c", "   ", "c c"])
///     .key('c', "cobblestone").key('w', "water").spacing(64).chance(0.3).biome("#animals").buried(1);
/// ```
pub struct StructureBuilder<'a> {
    raw: *mut abi::Registrar,
    template: String,
    dimension: String,
    biome: String,
    desc: abi::StructureDesc,
    _p: PhantomData<&'a mut Registry>,
}

impl StructureBuilder<'_> {
    /// One chance per `blocks` x `blocks` cell.
    pub fn spacing(mut self, blocks: u32) -> Self {
        self.desc.spacing = blocks;
        self
    }
    pub fn chance(mut self, c: f32) -> Self {
        self.desc.chance = c;
        self
    }
    /// Only where the biome has this tag.
    pub fn biome(mut self, tag: &str) -> Self {
        self.biome = tag.to_string();
        self
    }
    /// How far into the ground its bottom layer goes.
    pub fn buried(mut self, blocks: i32) -> Self {
        self.desc.sink = blocks;
        self
    }
}

impl Drop for StructureBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let desc = abi::StructureDesc {
            template: Str::new(&self.template),
            dimension: Str::new(&self.dimension),
            biome: Str::new(&self.biome),
            ..self.desc
        };
        unsafe { (host().reg_structure)(self.raw, &desc) }
    }
}

/// Layers from the bottom up, each a list of rows along z, each character one block along x:
/// a key says which block it means, a space leaves what is there, `air` clears.
pub struct TemplateBuilder<'a> {
    raw: *mut abi::Registrar,
    ns: &'static str,
    name: String,
    layers: Vec<Vec<String>>,
    keys: Vec<(char, String)>,
    fills: Vec<(String, [i32; 3], [i32; 3])>,
    _p: PhantomData<&'a mut Registry>,
}

impl TemplateBuilder<'_> {
    pub fn layer(mut self, rows: &[&str]) -> Self {
        self.layers
            .push(rows.iter().map(|r| r.to_string()).collect());
        self
    }
    /// `block` may name a state (`furnace[lit=true]`) or `air` to clear.
    pub fn key(mut self, c: char, block: &str) -> Self {
        self.keys.push((c, block_ref(self.ns, block)));
        self
    }
    /// A box of `block` from `from` to `to`, inclusive (a moving structure's deck).
    pub fn fill(mut self, block: &str, from: [i32; 3], to: [i32; 3]) -> Self {
        self.fills.push((block_ref(self.ns, block), from, to));
        self
    }
}

fn block_ref(ns: &str, block: &str) -> String {
    if block == "air" { "core:air".to_string() } else { qualify(ns, block) }
}

impl Drop for TemplateBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let mut names: Vec<([i32; 3], &str)> = Vec::new();
        for (y, layer) in self.layers.iter().enumerate() {
            for (z, row) in layer.iter().enumerate() {
                for (x, ch) in row.chars().enumerate() {
                    if let Some((_, b)) = self.keys.iter().find(|k| k.0 == ch) {
                        names.push(([x as i32, y as i32, z as i32], b));
                    }
                }
            }
        }
        for (b, f, t) in &self.fills {
            for y in f[1].min(t[1])..=f[1].max(t[1]) {
                for z in f[2].min(t[2])..=f[2].max(t[2]) {
                    for x in f[0].min(t[0])..=f[0].max(t[0]) {
                        names.push(([x, y, z], b));
                    }
                }
            }
        }
        let blocks: Vec<abi::StructureBlock> = names
            .iter()
            .map(|(p, b)| abi::StructureBlock { pos: *p, block: Str::new(b) })
            .collect();
        let desc = abi::TemplateDesc { name: Str::new(&self.name), blocks: Slice::new(&blocks) };
        unsafe { (host().reg_template)(self.raw, &desc) }
    }
}

pub struct OreBuilder<'a> {
    raw: *mut abi::Registrar,
    ns: &'static str,
    dimensions: String,
    block: String,
    replace: String,
    biome: String,
    desc: abi::OreDesc,
    _p: PhantomData<&'a mut Registry>,
}

impl OreBuilder<'_> {
    /// Only in this dimension (call again for more). Without, in every dimension but bare ones.
    pub fn dimension(mut self, dimension: &Dimension) -> Self {
        self.dimensions = format!("{} {}", self.dimensions, dimension.name());
        self
    }
    /// Only in biomes with this tag (underground ones too: `cave`).
    pub fn biome(mut self, tag: &str) -> Self {
        self.biome = tag.to_string();
        self
    }
    /// Heights the veins start at, inclusive.
    pub fn y(mut self, min: i32, max: i32) -> Self {
        (self.desc.min_y, self.desc.max_y) = (min, max);
        self
    }
    /// Veins started per cube.
    pub fn veins(mut self, n: u32) -> Self {
        self.desc.veins = n;
        self
    }
    /// Blocks per vein.
    pub fn size(mut self, n: u32) -> Self {
        self.desc.blocks = n;
        self
    }
    /// Blocks or `#tags` the ore may replace, space separated.
    pub fn replace(mut self, blocks: &str) -> Self {
        let names: Vec<String> = blocks
            .split_whitespace()
            .map(|b| if b.starts_with('#') { b.to_string() } else { qualify(self.ns, b) })
            .collect();
        self.replace = names.join(" ");
        self
    }
}

impl Drop for OreBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let desc = abi::OreDesc {
            dimensions: Str::new(&self.dimensions),
            block: Str::new(&self.block),
            replace: Str::new(&self.replace),
            biome: Str::new(&self.biome),
            ..self.desc
        };
        unsafe { (host().reg_ore)(self.raw, &desc) }
    }
}

/// Generation context for one cube.
pub struct Gen {
    pub(crate) args: abi::GenArgs,
}

impl Gen {
    pub(crate) fn wrap(a: &mut abi::GenArgs) -> &mut Gen {
        unsafe { &mut *(a as *mut abi::GenArgs as *mut Gen) }
    }
    pub fn seed(&self) -> u64 {
        self.args.seed
    }
    /// Cube coordinates (block coords = cube * 16).
    pub fn cube(&self) -> [i32; 3] {
        self.args.cube
    }
    pub fn origin(&self) -> [i32; 3] {
        let c = self.args.cube;
        [c[0] * 16, c[1] * 16, c[2] * 16]
    }
    pub fn blocks(&mut self) -> &mut [u32; 4096] {
        unsafe { &mut *(self.args.blocks as *mut [u32; 4096]) }
    }
    pub fn biomes(&mut self) -> &mut [u16; 256] {
        unsafe { &mut *(self.args.biomes as *mut [u16; 256]) }
    }
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, z: usize, state: u32) {
        self.blocks()[(y * 16 + z) * 16 + x] = state;
    }
    #[inline]
    pub fn get(&mut self, x: usize, y: usize, z: usize) -> u32 {
        self.blocks()[(y * 16 + z) * 16 + x]
    }
}

/// How `BlockBuilder` makes a state's `SignalSpec` from its properties.
#[derive(Clone, Copy, Default)]
struct SignalBuild {
    kind: u32,
    attaches: bool,
    delay: Option<u32>,
    mode: u32,
    head: &'static str,
}

impl SignalBuild {
    fn apply(&self, p: &Props, d: &mut StateDef, ns: &'static str) {
        let dir = |name: &str| crate::Dir::from_name(p.get(name));
        let front = dir("front").or(dir("facing"));
        let mut s = SignalSpec { kind: self.kind, mode: self.mode, head: self.head, ..SignalSpec::default() };
        if let Some(a) = dir("attach") {
            s.attach = a.face();
            if self.attaches {
                let _ = State { def: d, ns }.needs(a, "");
            }
            // A torch powers the block on the far side of it, as high as it stands.
            if self.kind == abi::signal_kind::TORCH {
                s.front = a.opposite().face();
            }
        }
        if let Some(f) = front {
            s.front = f.face();
        }
        if let Some(t) = self.delay {
            s.delay = t;
        }
        // A `delay` is in redstone ticks, two game ticks each.
        if p.int("delay") > 0 {
            s.delay = p.int("delay") * 2;
        }
        if p.is("mode", "subtract") {
            s.mode = 1;
        }
        d.signal = s;
        if self.kind == abi::signal_kind::WIRE {
            let mask = p.int("faces");
            // A thin plate on each face it lies on.
            const PLATES: [[f32; 6]; 6] = [
                [0.0, 0.0, 0.0, 1.0, 16.0, 16.0],
                [15.0, 0.0, 0.0, 16.0, 16.0, 16.0],
                [0.0, 0.0, 0.0, 16.0, 1.0, 16.0],
                [0.0, 15.0, 0.0, 16.0, 16.0, 16.0],
                [0.0, 0.0, 0.0, 16.0, 16.0, 1.0],
                [0.0, 0.0, 15.0, 16.0, 16.0, 16.0],
            ];
            d.hitbox = (0..6)
                .filter(|f| mask & (1 << f) != 0)
                .map(|f| PLATES[f as usize])
                .collect();
            if mask == 0 {
                d.hitbox = vec![PLATES[2]];
            }
        }
    }
}

/// What `BlockBuilder::on_signal` is told.
pub struct SignalEvent {
    args: abi::SignalArgs,
}

impl SignalEvent {
    fn wrap(a: &mut abi::SignalArgs) -> &mut SignalEvent {
        unsafe { &mut *(a as *mut abi::SignalArgs as *mut SignalEvent) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> [i32; 3] {
        self.args.pos
    }
    pub fn state(&self) -> u32 {
        self.args.state
    }
    /// Power around it now, 0..15.
    pub fn level(&self) -> u8 {
        self.args.level as u8
    }
    /// What it was.
    pub fn was(&self) -> u8 {
        self.args.was as u8
    }
    /// It just got power.
    pub fn rising(&self) -> bool {
        self.args.was == 0 && self.args.level > 0
    }
}

/// What came in on each side of a `BlockBuilder::signal_logic` block.
pub struct SignalLogic {
    args: abi::LogicArgs,
}

impl SignalLogic {
    fn wrap(a: &mut abi::LogicArgs) -> &mut SignalLogic {
        unsafe { &mut *(a as *mut abi::LogicArgs as *mut SignalLogic) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> [i32; 3] {
        self.args.pos
    }
    pub fn rear(&self) -> u8 {
        self.args.rear as u8
    }
    pub fn sides(&self) -> [u8; 2] {
        [self.args.sides[0] as u8, self.args.sides[1] as u8]
    }
    /// What it gives now.
    pub fn output(&self) -> u8 {
        self.args.output as u8
    }
}

pub struct BlockEvent {
    args: abi::BlockEventArgs,
}

impl BlockEvent {
    fn wrap(a: &mut abi::BlockEventArgs) -> &mut BlockEvent {
        unsafe { &mut *(a as *mut abi::BlockEventArgs as *mut BlockEvent) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> [i32; 3] {
        self.args.pos
    }
    pub fn state(&self) -> u32 {
        self.args.state
    }
    /// The neighbor that changed, for `on_neighbor`.
    pub fn other(&self) -> [i32; 3] {
        self.args.other
    }
    pub fn entity(&self) -> u64 {
        self.args.entity
    }
}

pub struct Use {
    args: abi::UseArgs,
}

impl Use {
    fn wrap(a: &mut abi::UseArgs) -> &mut Use {
        unsafe { &mut *(a as *mut abi::UseArgs as *mut Use) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> [i32; 3] {
        self.args.pos
    }
    pub fn state(&self) -> u32 {
        self.args.state
    }
    pub fn face(&self) -> u32 {
        self.args.face
    }
    pub fn player(&self) -> u64 {
        self.args.player
    }
}

pub struct Place {
    args: abi::PlaceArgs,
}

impl Place {
    fn wrap(a: &mut abi::PlaceArgs) -> &Place {
        unsafe { &*(a as *mut abi::PlaceArgs as *const Place) }
    }
    pub fn pos(&self) -> [i32; 3] {
        self.args.pos
    }
    /// Face of the block that was clicked.
    pub fn face(&self) -> crate::Dir {
        crate::Dir::from_face(self.args.face)
    }
    pub fn look(&self) -> [f32; 3] {
        self.args.look
    }
    /// Just outside where it was clicked, 0..1 from this cell's corner: for a slab, `hit()[1] > 0.5`
    /// is the upper half, on a floor, under a ceiling, against a wall or on another slab alike.
    pub fn hit(&self) -> [f32; 3] {
        self.args.hit
    }
    /// In `merge`: a property of how its item would go.
    pub fn placed(&self, prop: &str) -> Option<&'static str> {
        crate::world::state_prop(self.args.placed, prop)
    }
    /// Which way around the player looks.
    pub fn looking(&self) -> crate::Dir {
        crate::Dir::nearest(self.args.look, crate::Facings::HORIZONTAL)
    }
}

pub struct ItemUse {
    args: abi::ItemUseArgs,
}

impl ItemUse {
    fn wrap(a: &mut abi::ItemUseArgs) -> &mut ItemUse {
        unsafe { &mut *(a as *mut abi::ItemUseArgs as *mut ItemUse) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn player(&self) -> u64 {
        self.args.player
    }
    pub fn target(&self) -> Option<([i32; 3], u32)> {
        (self.args.has_target != 0).then_some((self.args.pos, self.args.face))
    }
    pub fn stack(&self) -> crate::ItemStack {
        self.args.stack
    }
    /// Replaces the held stack (use count 0 to consume it entirely).
    pub fn set_stack(&mut self, s: crate::ItemStack) {
        self.args.stack = s;
    }
}

/// A liquid: its block (16 states) and flow rules, optionally a filled bucket item.
///
/// ```ignore
/// r.liquid(&OIL).texture("oil_still").flow_delay(15).drop_off(2).bucket(&OIL_BUCKET, &BUCKET, &TOOLS);
/// r.liquid_reaction("#hot", "#water").source_block("obsidian").flowing_block("cobblestone").turns_below_into("stone");
/// ```
pub struct LiquidBuilder<'a> {
    reg: &'a mut Registry,
    name: String,
    handle: &'static Block,
    texture: String,
    tint: Option<&'static TintKind>,
    light: [u8; 3],
    opacity: u8,
    drag: f32,
    translucent: bool,
    components: Vec<Pending>,
    desc: abi::LiquidDesc,
    overlay: String,
    bucket: Option<(&'static Item, String, String)>,
}

impl LiquidBuilder<'_> {
    /// Still texture, relative names are under `block/`.
    pub fn texture(mut self, t: &str) -> Self {
        self.texture = t.to_string();
        self
    }
    /// Colored by the biome it stands in (`tints::WATER`).
    pub fn tint(mut self, kind: &'static TintKind) -> Self {
        self.tint = Some(kind);
        self
    }
    pub fn light(mut self, level: u8) -> Self {
        self.light = [level; 3];
        self
    }
    /// How much light it absorbs per block.
    pub fn opacity(mut self, o: u8) -> Self {
        self.opacity = o;
        self
    }
    /// How fast things move inside: water 1, lava 0.4.
    pub fn drag(mut self, d: f32) -> Self {
        self.drag = d;
        self
    }
    /// Seen through (water); otherwise drawn solid like lava.
    pub fn translucent(mut self) -> Self {
        self.translucent = true;
        self
    }
    /// Ticks between flow steps (water 5, lava 30).
    pub fn flow_delay(mut self, ticks: u32) -> Self {
        self.desc.delay = ticks;
        self
    }
    /// Level lost per block spread (water 1: 7 blocks, lava 2: 3 blocks).
    pub fn drop_off(mut self, n: u32) -> Self {
        self.desc.drop_off = n;
        self
    }
    /// How far it looks for a way down (water 4, lava 2).
    pub fn slope_find(mut self, n: u32) -> Self {
        self.desc.slope_find = n;
        self
    }
    /// New sources form where at least `sources` sources are beside a spot over solid ground
    /// (vanilla water 2). 0: always, one bucket fills everything up to its level; more than the
    /// sides there are: never, as without this call.
    pub fn renewable(mut self, sources: u32) -> Self {
        self.desc.renewable = sources;
        self
    }
    /// Rises instead of falling.
    pub fn flows_up(mut self) -> Self {
        self.desc.density = -self.desc.density.abs().max(1);
        self
    }
    /// Roughly Kelvin; 1000 and more burns what touches it.
    pub fn temperature(mut self, t: u32) -> Self {
        self.desc.temperature = t;
        self
    }
    /// Sprinting in it does not swim, whatever its temperature. Someone with the
    /// `core:swim_anywhere` attribute still can, for as long as a modifier on it lasts.
    pub fn not_swimmable(mut self) -> Self {
        self.desc.swimmable = 0;
        self
    }
    /// The fog seen from inside: its color (0xRRGGBB) and how many blocks away it hides
    /// everything. By default the average color of its texture, 96 blocks.
    pub fn fog(mut self, rgb: u32, blocks: f32) -> Self {
        self.desc.fog_color = rgb;
        self.desc.fog_distance = blocks;
        self
    }
    /// The picture over the screen from inside, relative names are under `block/`, and how much
    /// of it shows. By default its own texture at 0.1; for an inside that looks unlike the surface, set
    /// this and `fog` apart from its texture.
    pub fn overlay(mut self, texture: &str, opacity: f32) -> Self {
        self.overlay = crate::asset("block", texture);
        self.desc.overlay_opacity = opacity;
        self
    }
    /// Nothing over the screen from inside.
    pub fn no_overlay(mut self) -> Self {
        self.desc.overlay_opacity = 0.0;
        self
    }
    /// Also registers `item`: `empty` (a container item, any mod's) filled with this liquid, in
    /// creative tab `tab` (any mod's). The engine fills and pours it; no code needed.
    pub fn bucket(mut self, item: &'static Item, empty: &Item, tab: &CreativeTab) -> Self {
        self.bucket = Some((item, empty.name().to_string(), tab.name().to_string()));
        self
    }
}

impl Drop for LiquidBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let short = self.handle.path().to_string();
        let tex = if self.texture.is_empty() { format!("{short}_still") } else { self.texture.clone() };
        let (tint, light, opacity, drag) = (self.tint, self.light, self.opacity, self.drag);
        // The engine makes its 16 states liquid (flags, look, level) from `reg_liquid`.
        let mut b = self
            .reg
            .block(self.handle)
            .texture(&tex)
            .prop("level", 0..=15)
            .unbreakable()
            .light_rgb(light[0], light[1], light[2])
            .opacity(opacity)
            .drag(drag)
            .no_item()
            .drops_nothing();
        if let Some(kind) = tint {
            b = b.tint(kind);
        }
        if self.translucent {
            b = b.translucent();
        }
        b.components = std::mem::take(&mut self.components);
        drop(b);
        let desc = abi::LiquidDesc { name: Str::new(&self.name), overlay: Str::new(&self.overlay), ..self.desc };
        unsafe { (host().reg_liquid)(self.reg.raw, &desc) };
        if let Some((item, empty, tab)) = self.bucket.take() {
            self.reg
                .item(item)
                .max_stack(1)
                .set_text(&c::LIQUID, &self.name)
                .set_text(&c::REMAINDER, &empty)
                .tag(&format!("tab/{tab}"));
        }
    }
}

pub struct ReactionBuilder<'a> {
    reg: &'a mut Registry,
    names: [String; 5],
}

impl ReactionBuilder<'_> {
    /// What a source of the liquid becomes.
    pub fn source_block(mut self, block: &str) -> Self {
        self.names[2] = qualify(self.reg.ns, block);
        self
    }
    /// What flowing liquid becomes.
    pub fn flowing_block(mut self, block: &str) -> Self {
        self.names[3] = qualify(self.reg.ns, block);
        self
    }
    /// What the touched liquid becomes when this one flows down into it.
    pub fn turns_below_into(mut self, block: &str) -> Self {
        self.names[4] = qualify(self.reg.ns, block);
        self
    }
}

impl Drop for ReactionBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let n = &self.names;
        let desc = abi::ReactionDesc {
            liquid: Str::new(&n[0]),
            touching: Str::new(&n[1]),
            source: Str::new(&n[2]),
            flowing: Str::new(&n[3]),
            falls_into: Str::new(&n[4]),
            ..abi::ReactionDesc::DEFAULT
        };
        unsafe { (host().reg_liquid_reaction)(self.reg.raw, &desc) }
    }
}

/// Changes how any mod's liquid looks from inside and whether it can be swum in; what you don't
/// set stays, and its flow is the mod's own. See `Registry::change_liquid`.
pub struct LiquidChangeBuilder<'a> {
    reg: &'a mut Registry,
    name: String,
    overlay: String,
    desc: abi::LiquidChange,
}

impl LiquidChangeBuilder<'_> {
    pub fn swimmable(mut self, on: bool) -> Self {
        self.desc.swimmable = on as u32;
        self
    }
    /// The fog seen from inside: its color (0xRRGGBB) and how many blocks away it hides everything.
    pub fn fog(mut self, rgb: u32, blocks: f32) -> Self {
        self.desc.fog_color = rgb;
        self.desc.fog_distance = blocks;
        self
    }
    /// The picture over the screen from inside, relative names are under `block/`, and how much
    /// of it shows.
    pub fn overlay(mut self, texture: &str, opacity: f32) -> Self {
        self.overlay = crate::asset("block", texture);
        self.desc.overlay_opacity = opacity;
        self
    }
    /// Nothing over the screen from inside.
    pub fn no_overlay(mut self) -> Self {
        self.desc.overlay_opacity = 0.0;
        self
    }
}

impl Drop for LiquidChangeBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let desc = abi::LiquidChange { name: Str::new(&self.name), overlay: Str::new(&self.overlay), ..self.desc };
        unsafe { (host().reg_liquid_change)(self.reg.raw, &desc) };
    }
}

impl Registry {
    /// Changes another mod's liquid: `r.change_liquid(&WATER).fog(0x204020, 12.0)`.
    pub fn change_liquid<'a>(&'a mut self, liquid: &Block) -> LiquidChangeBuilder<'a> {
        LiquidChangeBuilder { name: liquid.name().to_string(), reg: self, overlay: String::new(), desc: abi::LiquidChange::DEFAULT }
    }

    /// A liquid the engine flows; see `LiquidBuilder`. Defaults are vanilla water's numbers, an
    /// average of its texture for the fog's color.
    pub fn liquid<'a>(&'a mut self, liquid: &'static Block) -> LiquidBuilder<'a> {
        LiquidBuilder {
            name: liquid.name().to_string(),
            reg: self,
            handle: liquid,
            texture: String::new(),
            tint: None,
            light: [0; 3],
            opacity: 2,
            drag: 1.0,
            translucent: false,
            components: Vec::new(),
            desc: abi::LiquidDesc::DEFAULT,
            overlay: String::new(),
            bucket: None,
        }
    }

    /// When `liquid` touches `touching` (names or `#tags` of liquid blocks), see `ReactionBuilder`.
    pub fn liquid_reaction<'a>(&'a mut self, liquid: &str, touching: &str) -> ReactionBuilder<'a> {
        let names = [qualify(self.ns, liquid), qualify(self.ns, touching), String::new(), String::new(), String::new()];
        ReactionBuilder { reg: self, names }
    }
}

/// Liquid id of a block state (`None` for other blocks).
pub fn liquid_of(state: u32) -> Option<u32> {
    let l = unsafe { (host().liquid_of)(state) };
    (l != u32::MAX).then_some(l)
}

#[cfg(test)]
mod order_tests {
    use super::*;

    fn built(f: impl FnOnce(State<'_>) -> State<'_>) -> StateDef {
        let mut d = StateDef::new("x");
        f(State { def: &mut d, ns: "test" });
        d
    }

    #[test]
    fn what_comes_after_keeps_what_came_before() {
        let d = built(|s| s.holds_liquids().plant());
        assert_ne!(d.flags & bf::HOLDS_LIQUID, 0);
        assert_eq!(d.flags & bf::SOLID, 0);
        assert_eq!(built(|s| s.opacity(2).translucent()).opacity, Some(2));
        assert_eq!(built(|s| s.translucent().opacity(2)).opacity, Some(2));
        assert_eq!(built(|s| s.translucent().model_box([0.0; 3], [16.0; 3])).layer, Layer::Translucent);
    }

    #[test]
    fn a_frame_turns_boxes_and_hitboxes() {
        let f = crate::signal::Frame::new(crate::Dir::Down, crate::Dir::South);
        let d = built(|s| {
            s.frame(f)
                .model_box([0.0; 3], [16.0, 16.0, 1.0])
                .hitbox([0.0; 3], [16.0, 16.0, 1.0])
        });
        // Its north end comes to the south.
        assert_eq!((d.boxes[0].0, d.boxes[0].1), ([0.0, 0.0, 15.0], [16.0, 16.0, 16.0]));
        assert_eq!(d.hitbox[0], [0.0, 0.0, 15.0, 16.0, 16.0, 16.0]);
    }
}
