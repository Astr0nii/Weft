use crate::abi::BlockPos;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::abi::{self, BlockModel, RenderLayer, Slice, Str, block_flags as bf};
use crate::component::{self as c, Component, Data, Value};
use crate::{World, callback, host, qualify};

/// What a handle can name. Kinds with engine ids (`HasId`) can also be resolved to one.
pub mod kind {
    pub trait Kind {
        /// `abi::entry_kind`, or `u32::MAX` for things known only by name.
        const KIND: u32;
    }
    /// Kinds the engine numbers (blocks, items...): their handles give ids.
    pub trait HasId: Kind {}
    /// A component of kind marker `Self` goes on registrations of kind `K`.
    pub trait On<K> {}
    /// Components of any kind: mods' own vocabulary.
    pub struct Any;
    impl<K> On<K> for Any {}
    /// Kinds `Registry::edit` changes, by the builder it gives.
    pub trait Edits: Kind + Sized {
        type Builder<'a>
        where
            Self: 'a;
        fn edit<'a>(r: &'a mut crate::Registry, h: &'static super::Handle<Self>) -> Self::Builder<'a>;
    }
    /// Kinds registered with a builder, which `Registry::replace` starts again.
    pub trait Builds: Kind + Sized {
        type Builder<'a>
        where
            Self: 'a;
        fn builder<'a>(r: &'a mut crate::Registry, h: &'static super::Handle<Self>) -> Self::Builder<'a>;
        fn replace<'a>(r: &'a mut crate::Registry, h: &'static super::Handle<Self>) -> Self::Builder<'a> {
            unsafe { (crate::host().reg_override)(r.raw, Self::KIND, crate::abi::Str::new(h.name()), 0) };
            Self::builder(r, h)
        }
    }
    macro_rules! kinds {
        ($($n:ident = $v:expr),*) => {
            $(pub struct $n;
            impl Kind for $n {
                const KIND: u32 = $v;
            }
            impl HasId for $n {}
            impl On<$n> for $n {})*
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
        Sound = crate::abi::entry_kind::SOUND,
        DamageCause = crate::abi::entry_kind::DAMAGE_CAUSE,
        EntityEffect = crate::abi::entry_kind::ENTITY_EFFECT,
        TextPlace = crate::abi::entry_kind::TEXT_PLACE,
        GameMode = crate::abi::entry_kind::GAME_MODE,
        Pose = crate::abi::entry_kind::POSE,
        Weather = crate::abi::entry_kind::WEATHER,
        Precipitation = crate::abi::entry_kind::PRECIPITATION,
        Condition = crate::abi::entry_kind::CONDITION,
        Music = crate::abi::entry_kind::MUSIC,
        CameraMode = crate::abi::entry_kind::CAMERA,
        EquipmentSlot = crate::abi::entry_kind::EQUIPMENT_SLOT,
        CreativeTab = crate::abi::entry_kind::CREATIVE_TAB,
        SoundCategory = crate::abi::entry_kind::SOUND_CATEGORY,
        EntityCategory = crate::abi::entry_kind::ENTITY_CATEGORY,
        TintKind = crate::abi::entry_kind::TINT_KIND,
        RecipeKind = crate::abi::entry_kind::RECIPE_KIND,
        Attribute = crate::abi::entry_kind::ATTRIBUTE,
        Key = crate::abi::entry_kind::KEY,
        Liquid = crate::abi::entry_kind::LIQUID
    );
    /// Kinds without ids: entries of their own number, or things known only by name.
    macro_rules! named_kinds {
        ($($n:ident $(= $v:expr)?),*) => {
            $(pub struct $n;
            impl Kind for $n {
                const KIND: u32 = named_kinds!(@kind $($v)?);
            }
            impl On<$n> for $n {})*
        };
        (@kind $v:expr) => { $v };
        (@kind) => { u32::MAX };
    }
    named_kinds!(
        SkyObject = crate::abi::entry_kind::SKY,
        HudElement = crate::abi::entry_kind::HUD_ELEMENT,
        ScreenPart = crate::abi::entry_kind::SCREEN_PART,
        WordSet = crate::abi::entry_kind::WORDS,
        Declaration = crate::abi::entry_kind::COMPONENT,
        Recipe = crate::abi::entry_kind::RECIPE,
        LootTable = crate::abi::entry_kind::LOOT,
        Ore = crate::abi::entry_kind::ORE,
        Reaction = crate::abi::entry_kind::REACTION,
        Scatter = crate::abi::entry_kind::SCATTER,
        Decoration = crate::abi::entry_kind::DECORATION,
        Structure = crate::abi::entry_kind::STRUCTURE,
        Template = crate::abi::entry_kind::TEMPLATE,
        SpawnRule = crate::abi::entry_kind::SPAWN,
        Terrain = crate::abi::entry_kind::TERRAIN,
        BoneMotion = crate::abi::entry_kind::BONE_MOTION,
        GizmoGroup,
        Shader,
        Question,
        Chain
    );
    /// The components saying where things go in the world (`comps::common`).
    pub struct Placement;
    impl On<Ore> for Placement {}
    impl On<Scatter> for Placement {}
    impl On<Decoration> for Placement {}
    impl On<Structure> for Placement {}
    impl On<SpawnRule> for Placement {}
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

    /// `generated`, the name as written, even without a namespace (a component's).
    pub(crate) fn as_written(name: &str) -> &'static Self {
        let h = Self::generated(name.to_string());
        let _ = h.full.set(name.to_string());
        h
    }

    /// Full name, `ns:name`.
    pub fn name(&self) -> &str {
        self.full.get_or_init(|| crate::full_name(self.name))
    }

    pub(crate) fn id_cell(&self) -> *const AtomicU32 {
        &self.id
    }

    /// The name without its namespace.
    pub fn path(&self) -> &str {
        let n = self.name();
        n.split_once(':').map_or(n, |x| x.1)
    }
}

impl Block {
    /// Its ore, for `Registry::edit`: `r.edit(base::IRON_ORE.ore()).exposed(0.5)`.
    pub fn ore(&self) -> &'static Ore {
        Ore::generated(self.name().to_string())
    }
    /// Its liquid, for `Registry::edit`: `r.edit(base::WATER.liquid()).fog(0x204020, 12.0)`.
    pub fn liquid(&self) -> &'static Liquid {
        Liquid::generated(self.name().to_string())
    }
    /// What it drops, for `Registry::edit`: `r.edit(base::GRAVEL.loot()).pool(..)`.
    pub fn loot(&self) -> &'static LootTable {
        LootTable::generated(self.name().to_string())
    }
}

impl Dimension {
    /// Its object `object` of the sky, for `Registry::edit` and `remove`:
    /// `r.remove(OVERWORLD.sky(&base::STARS))`.
    pub fn sky(&self, object: &SkyObject) -> &'static SkyObject {
        SkyObject::generated(format!("{}/{}", self.name(), object.name()))
    }
    /// Its terrain, for `Registry::edit`: `r.edit(OVERWORLD.terrain()).rule_before(..)`.
    pub fn terrain(&self) -> &'static Terrain {
        Terrain::generated(self.name().to_string())
    }
}

impl Template {
    /// The structure placing it, for `Registry::edit`: `r.edit(WELL.structure()).spacing(96)`.
    pub fn structure(&self) -> &'static Structure {
        Structure::generated(self.name().to_string())
    }
}

impl EntityKind {
    /// What it drops, for `Registry::edit`: `r.edit(base::ZOMBIE.loot()).pool(..)`.
    pub fn loot(&self) -> &'static LootTable {
        LootTable::generated(self.name().to_string())
    }
}

impl Item {
    /// `count` of this item, plain; `.with(&COMP, v)` and `.with_wear(n)` give it data.
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
        let frozen = unsafe { !(host().component_table)().is_null() };
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

    pub(crate) fn set_count(&self, n: u32) {
        self.count.store(n, Ordering::Relaxed);
    }
}

fn cell<K: kind::HasId>(h: Option<&'static Handle<K>>) -> *const AtomicU32 {
    h.map_or(std::ptr::null(), |h| h.id_cell())
}

pub type Block = Handle<kind::Block>;
pub type Item = Handle<kind::Item>;
pub type Biome = Handle<kind::Biome>;
pub type Dimension = Handle<kind::Dimension>;
pub type Particle = Handle<kind::Particle>;
pub type SkyObject = Handle<kind::SkyObject>;
pub type Sound = Handle<kind::Sound>;
pub type EntityKind = Handle<kind::Entity>;

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
pub type CameraMode = Handle<kind::CameraMode>;

pub type HudElement = Handle<kind::HudElement>;
/// Something a mod adds to a place of a screen (`Registry::screen_part`), named
/// `<mod>:<screen>/<area>`.
pub type ScreenPart = Handle<kind::ScreenPart>;
/// Words command parameters `word(#set)` complete from (`Registry::word_set`); the engine's are
/// in `word_sets`.
pub type WordSet = Handle<kind::WordSet>;
/// A component's declaration (`Registry::entity_component`, `stack_component`, `storage`),
/// named like the component: `HUNGER.declaration()`.
pub type Declaration = Handle<kind::Declaration>;
/// A recipe (`Registry::shaped`, `shapeless`), for `Registry::edit` and `remove`.
pub type Recipe = Handle<kind::Recipe>;

/// The engine's word sets.
pub mod word_sets {
    use super::WordSet;
    /// `/gizmo`'s groups (`Registry::gizmo_group`).
    pub static GIZMOS: WordSet = WordSet::named("core:gizmos");
}
pub type GizmoGroup = Handle<kind::GizmoGroup>;
/// A shader file of a mod, by its name: `shaders/<block|entity|particle|sky|post>/<name>.wgsl`,
/// the folder by what uses it. Nothing to register, the file is it.
pub type Shader = Handle<kind::Shader>;
/// The ore of a block, named like it (`r.ore(&IRON_ORE)` makes one), for `Registry::edit`.
pub type Ore = Handle<kind::Ore>;
/// The liquid of a block, named like it (`r.liquid(&WATER)` makes one), for `Registry::edit`;
/// its id is what `liquid_of` gives.
pub type Liquid = Handle<kind::Liquid>;
/// What happens where two liquids meet (`Registry::liquid_reaction`).
pub type Reaction = Handle<kind::Reaction>;
pub type TintKind = Handle<kind::TintKind>;
pub type Scatter = Handle<kind::Scatter>;
/// What runs on every generated cube (`Registry::decoration`), named `<mod>:decoration`, `/2`..
/// unless `named`.
pub type Decoration = Handle<kind::Decoration>;
/// A fixed shape of blocks (`Registry::template`), for world structures, moving bodies and
/// `World::place`.
pub type Template = Handle<kind::Template>;
/// A template placed while generating, named like it (`Registry::structure`).
pub type Structure = Handle<kind::Structure>;
/// A natural spawning rule (`Registry::natural_spawn`), named `<mod>:<entity>`, `/2`.. unless
/// `named`.
pub type SpawnRule = Handle<kind::SpawnRule>;
/// A dimension's terrain, named like it (`Registry::terrain`, `OVERWORLD.terrain()`).
pub type Terrain = Handle<kind::Terrain>;

/// The engine's tint kinds; biomes color them with `.tint(&GRASS, 0x..)`.
pub mod tints {
    use super::TintKind;
    pub static GRASS: TintKind = TintKind::named("core:grass");
    pub static FOLIAGE: TintKind = TintKind::named("core:foliage");
    pub static WATER: TintKind = TintKind::named("core:water");
}
pub type EntityCategory = Handle<kind::EntityCategory>;
/// What hurt an entity (`World::damage`): its armor and invulnerability rules and death message
/// come with it (`Registry::damage_cause`). The engine's are in `damage_causes`.
pub type DamageCause = Handle<kind::DamageCause>;

/// A shader effect drawn on entities (`World::entity_effect`): `shaders/entity_effect/<name>.wgsl`
/// of the mod with `fn effect(p: EntityPixel, c: vec4<f32>, strength: f32) -> vec4<f32>`.
pub type EntityEffect = Handle<kind::EntityEffect>;
/// A place on the screen for text (`World::show_at`); the engine's are in `text_places`.
pub type TextPlace = Handle<kind::TextPlace>;

/// The engine's entity effects.
pub mod entity_effects {
    use super::EntityEffect;
    /// White.
    pub static FLASH: EntityEffect = EntityEffect::named("core:flash");
    /// Red, what damage shows by itself.
    pub static HURT: EntityEffect = EntityEffect::named("core:hurt");
    /// Lit up whatever the light around it.
    pub static GLOW: EntityEffect = EntityEffect::named("core:glow");
}

/// An entity effect being registered: drawn by `shaders/entity_effect/<its name>.wgsl` unless
/// `shader` says another.
pub struct EntityEffectBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(EntityEffect => EntityEffectBuilder, entity_effect;);

impl crate::Registry {
    pub fn entity_effect<'a>(&'a mut self, effect: &'static EntityEffect) -> EntityEffectBuilder<'a> {
        EntityEffectBuilder { reg: self, e: crate::entry::Entry::of(effect) }
    }

    /// Something to fall from the sky (`.texture`, `.speed`, `.density`); a biome lets it fall
    /// with `.precipitation(..)`.
    pub fn precipitation<'a>(&'a mut self, p: &'static Precipitation) -> PrecipitationBuilder<'a> {
        PrecipitationBuilder { reg: self, e: crate::entry::Entry::of(p) }
    }
}

impl EntityEffectBuilder<'_> {
    pub fn shader(self, shader: &crate::Shader) -> Self {
        let name = shader.name().to_string();
        self.set_text(&crate::comps::entity_effect::SHADER, &name)
    }
}

/// What falls from the sky, rain's numbers unless set.
pub struct PrecipitationBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(Precipitation => PrecipitationBuilder, precipitation;);

impl PrecipitationBuilder<'_> {
    /// `textures/<texture>.png`, a strip of drops like vanilla's rain.
    pub fn texture(self, texture: &str) -> Self {
        let tex = crate::asset("", texture);
        self.set_text(&crate::comps::precipitation::TEXTURE, &tex)
    }
    /// As a share of rain's (snow drifts at 0.1).
    pub fn speed(self, share: f32) -> Self {
        self.set(&crate::comps::precipitation::SPEED, share)
    }
    /// The share of the picture shown (rain 0.1).
    pub fn density(self, share: f32) -> Self {
        self.set(&crate::comps::precipitation::DENSITY, share)
    }
}

/// A weather (`Registry::weather`); the engine's are in `weathers`.
pub type Weather = Handle<kind::Weather>;
/// What falls from the sky in a biome (`Registry::precipitation`, `BiomeBuilder::precipitation`).
pub type Precipitation = Handle<kind::Precipitation>;

impl Weather {
    /// `abi::weather_flags` of the weather with this id (`w.weather()`).
    pub fn flags_of(weather: u32) -> u32 {
        unsafe { (crate::host().weather_flags)(weather) }
    }
}

pub mod weathers {
    use super::Weather;
    pub static CLEAR: Weather = Weather::named("core:clear");
    pub static RAIN: Weather = Weather::named("core:rain");
    pub static THUNDER: Weather = Weather::named("core:thunder");
}

pub mod precipitations {
    use super::Precipitation;
    pub static RAIN: Precipitation = Precipitation::named("core:rain");
    pub static SNOW: Precipitation = Precipitation::named("core:snow");
}

/// A weather being registered: rain's rules unless told otherwise.
///
/// ```ignore
/// static ACID_RAIN: Weather = Weather::named("acid_rain");
/// r.weather(&ACID_RAIN).darkness(0.5).duration(2400, 6000).weight(1);
/// ```
/// `/weather mymod:acid_rain` sets it; the engine brings it now and then after clear weather.
pub struct WeatherBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(Weather => WeatherBuilder, weather;);

impl crate::Registry {
    pub fn weather<'a>(&'a mut self, weather: &'static Weather) -> WeatherBuilder<'a> {
        WeatherBuilder { reg: self, e: crate::entry::Entry::of(weather) }
    }
}

impl WeatherBuilder<'_> {
    /// Nothing falls (by default rain, snow, each biome's own does).
    pub fn no_falling(self) -> Self {
        self.tag(abi::comp::weather::DRY)
    }
    pub fn lightning(self) -> Self {
        self.tag(abi::comp::weather::LIGHTNING)
    }
    /// Things burn in the sun under it, a sandstorm (under rain they don't).
    pub fn no_shelter(self) -> Self {
        self.tag(abi::comp::weather::UNSHELTERED)
    }
    /// Share of daylight it takes away (rain 0.3125).
    pub fn darkness(self, share: f32) -> Self {
        self.set(&crate::comps::weather::DARKNESS, share)
    }
    /// How far the sky (rain 0.75), the fog (0.5) and the clouds (0.95) go gray.
    pub fn gray(self, sky: f32, fog: f32, clouds: f32) -> Self {
        self.set(&crate::comps::weather::SKY_GRAY, sky)
            .set(&crate::comps::weather::FOG_GRAY, fog)
            .set(&crate::comps::weather::CLOUD_GRAY, clouds)
    }
    /// A sound event played now and then around players under the open sky while it falls.
    pub fn sound(self, sound: &str) -> Self {
        let name = crate::full_name(sound);
        self.set_text(&crate::comps::weather::SOUND, &name)
    }
    /// Ticks it lasts, at least and at most.
    pub fn duration(self, min: u32, max: u32) -> Self {
        self.set(&crate::comps::weather::DURATION, [min, max])
    }
    /// How likely it comes after clear weather next to the others (rain 3, thunder 1); 0 only
    /// by command or code.
    pub fn weight(self, w: u32) -> Self {
        self.set(&crate::comps::weather::WEIGHT, w)
    }
}

/// A way to hold a body (`Registry::pose`): its box and eyes, its speed, how the model lies and
/// what it plays. `EntityState::pose` holds one; the engine's are in `poses`.
pub type Pose = Handle<kind::Pose>;

/// The engine's poses; it picks among the first four by itself as players move.
pub mod poses {
    use super::Pose;
    pub static STAND: Pose = Pose::named("core:stand");
    pub static CROUCH: Pose = Pose::named("core:crouch");
    pub static SWIM: Pose = Pose::named("core:swim");
    pub static CRAWL: Pose = Pose::named("core:crawl");
    pub static SLEEP: Pose = Pose::named("core:sleep");
}

/// A pose being registered: standing's box and speed unless told otherwise.
///
/// ```ignore
/// static SIT: Pose = Pose::named("sit");
/// r.pose(&SIT).height(0.7).eye(0.62).speed(0.0).action("sit");
/// w.set_pose(pet, Some(&SIT), None);
/// // Taken by the player: a slide while sprinting and holding crawl, above the engine's crawl.
/// r.pose(&SLIDE).height(0.4).eye(0.35).speed(1.2).key(&keys::CRAWL).when(pose_when::SPRINTING).priority(25);
/// ```
pub struct PoseBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(Pose => PoseBuilder, pose;);

impl crate::Registry {
    pub fn pose<'a>(&'a mut self, pose: &'static Pose) -> PoseBuilder<'a> {
        PoseBuilder { reg: self, e: crate::entry::Entry::of(pose) }
    }
}

impl PoseBuilder<'_> {
    /// Box height as a share of standing (crouching is 1.5 / 1.8).
    pub fn height(self, share: f32) -> Self {
        self.set(&crate::comps::pose::HEIGHT, share)
    }
    /// Eye height as a share of standing.
    pub fn eye(self, share: f32) -> Self {
        self.set(&crate::comps::pose::EYE, share)
    }
    /// Walking speed as a share of normal, 0 to stay put.
    pub fn speed(self, share: f32) -> Self {
        self.set(&crate::comps::pose::SPEED, share)
    }
    /// How the model lies (`abi::pose_tilt`): along the look like swimming, face down, on its back.
    pub fn tilt(self, tilt: u32) -> Self {
        self.set(&crate::comps::pose::TILT, tilt)
    }
    /// The model's clip for it, by action name.
    pub fn action(self, name: &str) -> Self {
        self.set_text(&crate::comps::pose::ACTION, name)
    }
    /// The camera doesn't bob walking in it.
    pub fn no_bob(self) -> Self {
        self.tag(abi::comp::pose::NO_BOB)
    }
    /// Of the poses a player may take, the highest is taken (the engine's: stand 0, crouch 10,
    /// crawl 20, swim 30). Without `key` or `when` a pose is only ever held by a mod.
    pub fn priority(self, p: i32) -> Self {
        self.set(&crate::comps::pose::PRIORITY, p)
    }
    /// Taken while the player holds this action.
    pub fn key(self, key: &crate::Key) -> Self {
        let name = key.name().to_string();
        self.set_text(&crate::comps::pose::KEY, &name)
    }
    /// And only where these hold (`abi::pose_when`).
    pub fn when(self, flags: u32) -> Self {
        abi::comp::pose::WHEN
            .iter()
            .filter(|w| flags & w.1 != 0)
            .fold(self, |b, w| b.tag(w.0))
    }
    /// Where it has no room, the first of these that has and whose `when` holds; the last
    /// even without room.
    pub fn fallback(self, poses: &[&Pose]) -> Self {
        self.set(&crate::comps::pose::FALLBACK, poses.iter().map(|p| p.name().to_string()).collect())
    }
}

/// A game mode: what its players may do and what they are spared (`Registry::game_mode`); the
/// engine's are in `game_modes`.
pub type GameMode = Handle<kind::GameMode>;

/// The engine's game modes.
pub mod game_modes {
    use super::GameMode;
    pub static SURVIVAL: GameMode = GameMode::named("core:survival");
    pub static CREATIVE: GameMode = GameMode::named("core:creative");
    pub static SPECTATOR: GameMode = GameMode::named("core:spectator");
}

impl GameMode {
    /// `abi::game_mode_flags` of the mode with this id.
    pub fn flags_of(mode: u32) -> u32 {
        unsafe { (crate::host().game_mode_flags)(mode) }
    }
}

/// A game mode being registered: survival's rules unless told otherwise.
///
/// ```ignore
/// static ADVENTURE: GameMode = GameMode::named("adventure");
/// r.game_mode(&ADVENTURE).breaks(Breaking::None).no_placing();
/// ```
/// `/gamemode mymod:adventure` sets it, `{if:mymod:adventure}` in text asks for it.
pub struct GameModeBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(GameMode => GameModeBuilder, game_mode;);

/// How a game mode breaks blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Breaking {
    Normal,
    Instant,
    None,
}

/// How a game mode flies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flight {
    None,
    /// Jumping twice starts and stops it.
    Allowed,
    Always,
}

impl crate::Registry {
    pub fn game_mode<'a>(&'a mut self, mode: &'static GameMode) -> GameModeBuilder<'a> {
        GameModeBuilder { reg: self, e: crate::entry::Entry::of(mode) }
    }
}

impl GameModeBuilder<'_> {
    /// Raw `abi::game_mode_flags`, what the presets below set.
    pub fn with(self, flags: u32) -> Self {
        abi::comp::game_mode::FLAGS
            .iter()
            .filter(|f| flags & f.1 != 0)
            .fold(self, |b, f| b.tag(f.0))
    }
    pub fn breaks(self, how: Breaking) -> Self {
        use abi::game_mode_flags as f;
        self.with(match how {
            Breaking::Instant => f::INSTANT_BREAK,
            Breaking::None => f::NO_BREAK,
            Breaking::Normal => 0,
        })
    }
    pub fn flight(self, how: Flight) -> Self {
        use abi::game_mode_flags as f;
        self.with(match how {
            Flight::Allowed => f::CAN_FLY,
            Flight::Always => f::ALWAYS_FLY,
            Flight::None => 0,
        })
    }
    pub fn no_placing(self) -> Self {
        self.with(abi::game_mode_flags::NO_PLACE)
    }
    /// Nothing gets used up: placed blocks, poured buckets, wear. Picking a block gives a stack.
    pub fn no_costs(self) -> Self {
        self.with(abi::game_mode_flags::NO_COSTS)
    }
    /// No damage, hunger or air.
    pub fn invulnerable(self) -> Self {
        self.with(abi::game_mode_flags::INVULNERABLE)
    }
    /// Through blocks.
    pub fn noclip(self) -> Self {
        self.with(abi::game_mode_flags::NOCLIP)
    }
    /// Unnoticed by mobs, spawning and other players, and not a body to hit.
    pub fn hidden(self) -> Self {
        self.with(abi::game_mode_flags::HIDDEN)
    }
    /// It can't use, hit, pick up, drop or hold anything.
    pub fn no_interacting(self) -> Self {
        self.with(abi::game_mode_flags::NO_INTERACT)
    }
    /// The hotbar and bars don't show.
    pub fn no_hud(self) -> Self {
        self.with(abi::game_mode_flags::NO_HUD)
    }
    /// The inventory key opens the creative inventory.
    pub fn creative_inventory(self) -> Self {
        self.with(abi::game_mode_flags::CREATIVE_INVENTORY)
    }
    /// An attribute modifier its players have while in it (longer reach, `visibility` 0).
    pub fn modifier(self, attribute: &crate::Attribute, op: u32, amount: f64) -> Self {
        let m = abi::ModeModifier { attribute: attribute.name().to_string(), op, amount };
        self.push(&crate::comps::game_mode::MODIFIERS, m)
    }
}

/// The engine's damage causes.
pub mod damage_causes {
    use super::DamageCause;
    pub static GENERIC: DamageCause = DamageCause::named("core:generic");
    pub static FALL: DamageCause = DamageCause::named("core:fall");
    pub static ATTACK: DamageCause = DamageCause::named("core:attack");
    pub static FIRE: DamageCause = DamageCause::named("core:fire");
    pub static STARVE: DamageCause = DamageCause::named("core:starve");
    pub static DROWN: DamageCause = DamageCause::named("core:drown");
    pub static VOID: DamageCause = DamageCause::named("core:void");
    pub static LAVA: DamageCause = DamageCause::named("core:lava");
    pub static CRUSH: DamageCause = DamageCause::named("core:crush");
    pub static SUFFOCATE: DamageCause = DamageCause::named("core:suffocate");
    pub static EXPLOSION: DamageCause = DamageCause::named("core:explosion");
    pub static HEAL: DamageCause = DamageCause::named("core:heal");
}

impl DamageCause {
    /// `abi::damage_flags` of the cause with this id: a mod's armor asks
    /// `DamageCause::flags_of(q.cause) & damage_flags::BYPASSES_ARMOR`.
    pub fn flags_of(cause: u32) -> u32 {
        unsafe { (crate::host().damage_cause_flags)(cause) }
    }
}

/// `r.damage_cause(&RADIATION).bypasses_armor().death_message("death.mymod.radiation")`.
pub struct DamageCauseBuilder<'a> {
    reg: &'a mut crate::Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(DamageCause => DamageCauseBuilder, damage_cause;);

impl crate::Registry {
    pub fn damage_cause<'a>(&'a mut self, cause: &'static DamageCause) -> DamageCauseBuilder<'a> {
        DamageCauseBuilder { reg: self, e: crate::entry::Entry::of(cause) }
    }
}

impl DamageCauseBuilder<'_> {
    /// Armor doesn't soften it.
    pub fn bypasses_armor(self) -> Self {
        self.tag(abi::comp::damage_cause::BYPASSES_ARMOR)
    }
    /// Hurts invulnerable entities and players outside survival too.
    pub fn pierces_invulnerable(self) -> Self {
        self.tag(abi::comp::damage_cause::PIERCES_INVULNERABLE)
    }
    /// Lands in full even right after another hit.
    pub fn ignores_cooldown(self) -> Self {
        self.tag(abi::comp::damage_cause::IGNORES_COOLDOWN)
    }
    /// What dies of it drops cooked food.
    pub fn burning(self) -> Self {
        self.tag(abi::comp::damage_cause::BURNING)
    }
    pub fn no_knockback(self) -> Self {
        self.tag(abi::comp::damage_cause::NO_KNOCKBACK)
    }
    /// No hurt sound.
    pub fn silent(self) -> Self {
        self.tag(abi::comp::damage_cause::SILENT)
    }
    /// Translation key said when a player dies of it, `{0}` their name.
    pub fn death_message(self, key: &str) -> Self {
        self.set_text(&crate::comps::damage_cause::DEATH_MESSAGE, key)
    }
    /// The key when something did it, `{1}` that thing's name.
    pub fn death_message_by(self, key: &str) -> Self {
        self.set_text(&crate::comps::damage_cause::DEATH_MESSAGE_BY, key)
    }
}
pub type SoundCategory = Handle<kind::SoundCategory>;
pub type CreativeTab = Handle<kind::CreativeTab>;
pub type Question = Handle<kind::Question>;
pub type Chain = Handle<kind::Chain>;
pub type Attribute = Handle<kind::Attribute>;

/// What puts a block or an item in `tab`.
fn tab_tag(tab: &CreativeTab) -> String {
    format!("tab/{}", tab.name())
}

/// A creative tab being registered (`Registry::creative_tab`).
pub struct CreativeTabBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(CreativeTab => CreativeTabBuilder, creative_tab_entry;);

/// The creative tab every entity's spawn egg goes to.
pub static SPAWN_EGGS: CreativeTab = CreativeTab::named("core:spawn_eggs");
pub type RecipeKind = Handle<kind::RecipeKind>;
pub type LootTable = Handle<kind::LootTable>;
/// A predicate music, text, loot and spawning share (`Registry::condition`); the engine's are in
/// `conditions`.
pub type Condition = Handle<kind::Condition>;

pub mod conditions {
    use super::Condition;
    pub static DAY: Condition = Condition::named("core:day");
    pub static NIGHT: Condition = Condition::named("core:night");
    /// No sky over it.
    pub static UNDERGROUND: Condition = Condition::named("core:underground");
    pub static SURFACE: Condition = Condition::named("core:surface");
}
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
impl LootSource for EntityKind {
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
    /// The kind it registers (`kind`): which components it takes.
    type Kind;
    #[doc(hidden)]
    fn pending(&mut self) -> &mut Vec<Pending>;

    fn tag(mut self, tag: &str) -> Self {
        self.pending().push(Pending::tag(tag));
        self
    }
    /// Takes a tag away (another mod's, in an edit).
    fn untag(mut self, tag: &str) -> Self {
        self.pending()
            .push(Pending::tag(tag).as_op(abi::value_op::UNSET, ""));
        self
    }
    /// A component value (on every state of a block, and its item).
    fn set<T: Data, K: kind::On<Self::Kind>>(mut self, comp: &Component<T, K>, v: T) -> Self {
        self.pending().push(comp.raw_value(&v));
        self
    }
    /// Takes a component away (another mod's, in an edit).
    fn unset<T: Data, K: kind::On<Self::Kind>>(mut self, comp: &Component<T, K>) -> Self {
        self.pending()
            .push(Pending::tag(comp.name()).as_op(abi::value_op::UNSET, ""));
        self
    }
    /// A value written as text: plain text, or names the engine resolves (`BlockRef`,
    /// `BlockList`).
    fn set_text<T: c::TextValue, K: kind::On<Self::Kind>>(mut self, comp: &Component<T, K>, text: &str) -> Self {
        self.pending()
            .push(Pending::text_as(comp.name(), &T::schema(), text));
        self
    }
    /// One more element at the end of a list.
    fn push<T: Data, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, item: T) -> Self {
        self.pending()
            .push(element(list, &item, abi::value_op::PUSH, ""));
        self
    }
    /// An element before the one `anchor` names (its `name`, or `#index`).
    fn insert_before<T: Data, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, anchor: &str, item: T) -> Self {
        self.pending()
            .push(element(list, &item, abi::value_op::INSERT_BEFORE, anchor));
        self
    }
    fn insert_after<T: Data, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, anchor: &str, item: T) -> Self {
        self.pending()
            .push(element(list, &item, abi::value_op::INSERT_AFTER, anchor));
        self
    }
    /// The element `anchor` names becomes `item`.
    fn replace_item<T: Data, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, anchor: &str, item: T) -> Self {
        self.pending()
            .push(element(list, &item, abi::value_op::REPLACE_ITEM, anchor));
        self
    }
    fn remove_item<T: Data + Default, K: kind::On<Self::Kind>>(mut self, list: &Component<Vec<T>, K>, anchor: &str) -> Self {
        self.pending()
            .push(element(list, &T::default(), abi::value_op::REMOVE_ITEM, anchor));
        self
    }
}

/// A list operation's value: one element, with the element's schema.
fn element<T: Data, K>(list: &Component<Vec<T>, K>, item: &T, op: u32, anchor: &str) -> Pending {
    Pending::value(list.name(), T::schema(), abi::data::bytes_of(item)).as_op(op, anchor)
}

macro_rules! with_components {
    ($($b:ident: $k:ident),*) => {
        $(impl WithComponents for $b<'_> {
            type Kind = kind::$k;
            fn pending(&mut self) -> &mut Vec<Pending> {
                &mut self.components
            }
        })*
    };
}

with_components!(BlockBuilder: Block);

/// What carries items: entity kinds (block entities among them) and item stacks.
pub trait Holds: WithComponents {
    /// Each one carries an inventory of `slots` (`w.inventory(holder)`): an entity's, a block
    /// entity's (`w.inventory(pos)`, a menu's `Slots::holder()` groups), a stack's (a backpack,
    /// `w.inventory(Holder::held(player, slot))`, saved in the stack; stacks holding something
    /// only stack with stacks holding the same).
    fn inventory(self, slots: u32) -> Self {
        self.set(&crate::component::INVENTORY_SLOTS, slots)
    }
}
impl Holds for ItemBuilder<'_> {}
impl Holds for crate::EntityBuilder<'_> {}

/// On one state only, on top of the block's.
impl WithComponents for State<'_> {
    type Kind = kind::Block;
    fn pending(&mut self) -> &mut Vec<Pending> {
        &mut self.def.components
    }
}

/// A component value waiting to be sent with a descriptor. Text is kept as a `String` until then.
#[derive(Clone)]
pub struct Pending {
    pub(crate) name: String,
    schema: String,
    bytes: Vec<u8>,
    text: Option<String>,
    /// `abi::value_op`, and the list element it is about.
    pub(crate) op: u32,
    pub(crate) anchor: String,
}

impl Pending {
    pub(crate) fn value(name: &str, schema: String, bytes: Vec<u8>) -> Pending {
        Pending { name: name.to_string(), schema, bytes, text: None, op: abi::value_op::SET, anchor: String::new() }
    }
    /// The same value as another operation (`abi::value_op`) on `anchor`.
    pub(crate) fn as_op(self, op: u32, anchor: &str) -> Pending {
        Pending { op, anchor: anchor.to_string(), ..self }
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
        Pending {
            name: name.to_string(),
            schema: schema.into(),
            bytes: Vec::new(),
            text: Some(text),
            op: abi::value_op::SET,
            anchor: String::new(),
        }
    }
    pub(crate) fn tag(name: &str) -> Pending {
        Pending::value(name, String::new(), Vec::new())
    }
    fn is_tag(&self) -> bool {
        self.schema.is_empty() && self.text.is_none() && self.bytes.is_empty()
    }
}

/// `CompValue`s pointing into `list`; for text the `Str` bytes live in `scratch`.
pub(crate) fn raw_values(list: &[Pending], scratch: &mut Vec<Vec<u8>>) -> Vec<abi::CompValue> {
    scratch.clear();
    for p in list {
        let bytes = match &p.text {
            Some(t) => abi::data::bytes_of(t),
            None => p.bytes.clone(),
        };
        scratch.push(bytes);
    }
    list.iter()
        .zip(scratch.iter())
        .map(|(p, b)| abi::CompValue { name: Str::new(&p.name), schema: Str::new(&p.schema), value: Slice::new(b) })
        .collect()
}

/// What `Registry::set_on` applies to; names without a namespace are this mod's. An entity kind's
/// components are set by editing it, `r.edit(&PLAYER).set(..)`.
pub enum Target<'a> {
    /// The block and item of that name; `name[prop=value]` only the block's states with that value.
    Name(&'a str),
    Biome(&'a str),
    Dimension(&'a str),
}

pub struct Registry {
    pub(crate) raw: *mut abi::Registrar,
    pub(crate) ns: &'static str,
    /// Names given to what has none of its own (recipes, screen parts), each once.
    pub(crate) derived: Vec<String>,
}

impl kind::Edits for kind::Block {
    type Builder<'a> = BlockBuilder<'a>;
    fn edit<'a>(r: &'a mut Registry, h: &'static Block) -> BlockBuilder<'a> {
        let mut b = r.block(h);
        b.replace = true;
        b
    }
}
impl kind::Builds for kind::Block {
    type Builder<'a> = BlockBuilder<'a>;
    fn builder<'a>(r: &'a mut Registry, h: &'static Block) -> BlockBuilder<'a> {
        r.block(h)
    }
    /// Its item replaces theirs too.
    fn replace<'a>(r: &'a mut Registry, h: &'static Block) -> BlockBuilder<'a> {
        unsafe { (host().reg_override)(r.raw, abi::entry_kind::BLOCK, Str::new(h.name()), 0) };
        let mut b = r.block(h);
        b.replacing = true;
        b
    }
}

impl Registry {
    pub fn namespace(&self) -> &'static str {
        self.ns
    }

    /// `<this mod>:<path>`, numbered `/2`, `/3`.. when this mod gave it already.
    pub(crate) fn derive_name(&mut self, path: &str) -> String {
        let base = format!("{}:{path}", self.ns);
        let name = (1..)
            .map(|n| if n == 1 { base.clone() } else { format!("{base}/{n}") })
            .find(|n| !self.derived.contains(n))
            .unwrap_or_default();
        self.derived.push(name.clone());
        name
    }

    /// Changes only what the builder sets of something registered already, another mod's or
    /// this one's; the rest stays. Blocks (`r.edit(&base::CACTUS).light(5)`: without
    /// `.states(...)` its states stay, the components set go on top), items, biomes, dimensions,
    /// menus, ores (`IRON_ORE.ore()`), liquids (`WATER.liquid()`), terrains
    /// (`OVERWORLD.terrain()`), loot tables (`ZOMBIE.loot()`), entity kinds
    /// (`r.edit(&PLAYER).attach(&MANA)`) and the rest by their handles.
    pub fn edit<K: kind::Edits>(&mut self, handle: &'static Handle<K>) -> K::Builder<'_> {
        K::edit(self, handle)
    }

    /// Registers `handle` from scratch over what other mods registered as it, nothing of theirs
    /// staying: `r.replace(&base::TORCH).texture("blue_torch")`. Of several mods replacing one
    /// thing, the last in load order wins.
    pub fn replace<K: kind::Builds>(&mut self, handle: &'static Handle<K>) -> K::Builder<'_> {
        K::replace(self, handle)
    }

    /// As if nobody had registered `handle` (a block takes its item along).
    pub fn remove<K: kind::Kind>(&mut self, handle: &Handle<K>) {
        unsafe { (host().reg_override)(self.raw, K::KIND, Str::new(handle.name()), 1) };
        let mut e = crate::entry::Entry::of(handle);
        e.mode = abi::entry_mode::REMOVE;
        e.send(self);
        if K::KIND == abi::entry_kind::BLOCK {
            drop(self.block_item(handle.name(), String::new(), abi::entry_mode::REMOVE));
        }
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
            replacing: false,
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
            entity: None,
            model_file: String::new(),
            model_textures: Vec::new(),
            neighbor_fns: Vec::new(),
        }
    }

    /// Texture defaults to `item/<name>`.
    pub fn item<'a>(&'a mut self, item: &'static Item) -> ItemBuilder<'a> {
        ItemBuilder { reg: self, e: crate::entry::Entry::of(item) }
    }

    /// The item placing `block`, named like it, registered as `mode` says (`entry_mode`): the
    /// engine gives it the block's components, and without `texture` it shows the block's model.
    fn block_item(&mut self, block: &str, texture: String, mode: u32) -> ItemBuilder<'_> {
        let mut e = crate::entry::Entry::new(abi::entry_kind::ITEM, block.to_string());
        e.mode = mode;
        if mode == abi::entry_mode::REMOVE {
            return ItemBuilder { reg: self, e };
        }
        ItemBuilder { reg: self, e }
            .set_text(&crate::comps::item::BLOCK, block)
            .set_text(&crate::comps::item::TEXTURE, &texture)
    }

    pub fn biome<'a>(&'a mut self, biome: &'static Biome) -> BiomeBuilder<'a> {
        BiomeBuilder { reg: self, e: crate::entry::Entry::of(biome) }
    }

    /// Creative inventory tab named `tab.<ns>.<name>` in the language files, shown with `icon`.
    /// Blocks and items join with `.tab(&TAB)`; untabbed items land in a tab named after the mod.
    /// Tabs come in mod load order, then registration order.
    pub fn creative_tab(&mut self, tab: &'static CreativeTab, icon: &Item) -> CreativeTabBuilder<'_> {
        self.creative_tab_entry(tab)
            .set_text(&crate::comps::creative_tab::ICON, icon.name())
    }

    fn creative_tab_entry(&mut self, tab: &'static CreativeTab) -> CreativeTabBuilder<'_> {
        CreativeTabBuilder { reg: self, e: crate::entry::Entry::of(tab) }
    }

    /// An ore, in every dimension whose ground it may `replace` (`.dimension(..)` limits it):
    /// `r.ore(&TIN_ORE).y(0, 64).veins(6).vein_size(8).replaces("#stone")`.
    pub fn ore(&mut self, block: &Block) -> OreBuilder<'_> {
        self.ore_entry(block.ore())
    }

    fn ore_entry(&mut self, ore: &'static Ore) -> OreBuilder<'_> {
        OreBuilder { reg: self, e: crate::entry::Entry::of(ore) }
    }

    /// A template placed now and then on the surface, in every dimension but bare ones unless
    /// `.dimension(..)` says (the structure is called like the template, for `World::structure_near`).
    pub fn structure(&mut self, template: &Template) -> StructureBuilder<'_> {
        self.structure_entry(template.structure())
    }

    fn structure_entry(&mut self, structure: &'static Structure) -> StructureBuilder<'_> {
        StructureBuilder { reg: self, e: crate::entry::Entry::of(structure) }
    }

    /// A fixed shape of blocks, drawn in layers of rows (`layer` + `key`) or filled with boxes
    /// (`fill`), checked when the game loads.
    pub fn template(&mut self, template: &'static Template) -> TemplateBuilder<'_> {
        TemplateBuilder { reg: self, e: crate::entry::Entry::of(template), layers: Vec::new(), keys: Vec::new(), fills: Vec::new() }
    }

    pub fn dimension<'a>(&'a mut self, dimension: &'static Dimension) -> DimensionBuilder<'a> {
        DimensionBuilder { reg: self, e: crate::entry::Entry::of(dimension) }
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
    /// `r.set_model("base:apple", "foodmod:apple")`, `r.set_model(&PLAYER, humanoid(false))`.
    /// A whole tag at once takes `{name}`; items without a file stay as they were:
    /// `r.set_model("#food", "foodmod:food/{name}")`. An entity kind's is an edit of it.
    pub fn set_model(&mut self, what: impl ModelTarget, model: impl ModelSource) {
        model.set_on(self, &what);
    }

    /// The clips of an item or an entity of any mod: `r.set_animations(&PLAYER, "mymod:player")`
    /// plays `animations/player.animation.json` of `mymod` on every player.
    pub fn set_animations(&mut self, what: impl ModelTarget, file: &str) {
        let file = crate::asset("", file);
        self.model_text(&what, &c::ANIMATIONS, &file);
    }

    /// A text component of what `set_model` reaches: an edit of an entity kind, or blocks and items.
    fn model_text(&mut self, what: &dyn ModelTarget, comp: &Component<c::Text>, text: &str) {
        if what.is_entity() {
            drop(self.edit_entity(what.model_name()).set_text(comp, text));
        } else {
            self.set_text_on(Target::Name(what.model_name()), comp, text);
        }
    }

    /// Every entity of `kind` has `model` on its bone `bone`, drawn in that model's own pixels, with
    /// the same origin as the kind's. What plays on the entity plays on it too, by the names of
    /// clips and bones: `r.bone_model(&PLAYER, "tailmod:tail", "body")`.
    pub fn bone_model(&mut self, kind: &crate::EntityKind, model: &str, bone: &str) {
        let model = crate::asset("", model);
        let place = abi::BoneModel { model, bone: bone.to_string() };
        drop(
            self.edit_entity(kind.name())
                .push(&crate::comps::entity::BONE_MODELS, place),
        );
    }

    /// `bone` of `model` moves by itself. A bone called `hanging` or `hanging_<anything>` does without this.
    pub fn bone_motion(&mut self, model: &str, bone: &str, motion: BoneMotion) {
        use crate::comps::bone_motion as m;
        let name = format!("{}/{bone}", crate::asset("", model));
        let mut e = crate::entry::Entry::new(abi::entry_kind::BONE_MOTION, name);
        e.ops = vec![m::GRAVITY.raw_value(&motion.gravity), m::SPRING.raw_value(&motion.spring), m::DRAG.raw_value(&motion.drag)];
        e.send(self);
    }

    /// A text component on entries of any mod.
    pub fn set_text_on<T: c::TextValue>(&mut self, target: Target, comp: &Component<T>, text: &str) {
        self.send_set(target, &Pending::text_as(comp.name(), &T::schema(), text), false);
    }

    /// Takes a component or tag away from entries of any mod.
    pub fn unset_on(&mut self, target: Target, component: &str) {
        self.send_set(target, &Pending::tag(component), true);
    }

    fn send_set(&mut self, target: Target, value: &Pending, remove: bool) {
        use abi::set_target as st;
        let (kind, name) = match target {
            Target::Name(n) => (st::NAME, qualify(self.ns, n)),
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
    fn set_on(self, r: &mut Registry, what: &dyn ModelTarget);
}

impl ModelSource for &str {
    fn set_on(self, r: &mut Registry, what: &dyn ModelTarget) {
        let file = crate::asset("", self);
        r.model_text(what, &c::MODEL, &file);
    }
}

/// What has a model: a name or `#tag` of blocks and items as text, or the handle of an item, a
/// block or an entity kind.
pub trait ModelTarget {
    #[doc(hidden)]
    fn model_name(&self) -> &str;
    #[doc(hidden)]
    fn is_entity(&self) -> bool {
        false
    }
}

impl ModelTarget for &str {
    fn model_name(&self) -> &str {
        self
    }
}

impl ModelTarget for &Item {
    fn model_name(&self) -> &str {
        self.name()
    }
}

impl ModelTarget for &Block {
    fn model_name(&self) -> &str {
        self.name()
    }
}

impl ModelTarget for &crate::EntityKind {
    fn model_name(&self) -> &str {
        self.name()
    }
    fn is_entity(&self) -> bool {
        true
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
    /// Sitting on a mount, instead of walking.
    pub const RIDE: &str = "ride";
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
    layer: RenderLayer,
    model: BlockModel,
    textures: [String; 6],
    /// Quarter turns of each cube face's picture.
    turns: [u8; 6],
    boxes: Vec<ModelBox>,
    /// Corners, in sixteenths of a block.
    hitbox: Vec<([f32; 3], [f32; 3])>,
    /// What is outlined instead, the same way.
    outline: Vec<([f32; 3], [f32; 3])>,
    /// What boxes from here on are turned by.
    orientation: Option<crate::signal::Orientation>,
    components: Vec<Pending>,
    /// What it is to redstone.
    signal: abi::SignalDesc,
    /// `abi::StateDesc::liquid_walls`, already turned.
    liquid_walls: u8,
}

/// A box of a state's model, in 1/16 units.
#[derive(Clone)]
struct ModelBox {
    min: [f32; 3],
    max: [f32; 3],
    textures: [String; 6],
    uv: [[f32; 4]; 6],
    /// Quarter turns of each face's picture.
    turns: [u8; 6],
    tilt: abi::BoxTilt,
}

impl StateDef {
    /// A later value of the same component wins at freeze.
    fn set<T: Value>(&mut self, comp: &Component<T>, v: T) {
        self.components.push(comp.raw_value(&v));
    }
    fn set_text(&mut self, name: &str, schema: &str, text: &str) {
        self.components.push(Pending::text_as(name, schema, text));
    }

    /// One more box, turned by its orientation if it has one. A model that isn't a cube doesn't hide
    /// what is behind it, so nothing else about a block says how it is to be drawn.
    fn add_box(&mut self, min: [f32; 3], max: [f32; 3], textures: [String; 6], uv: [[f32; 4]; 6]) {
        if self.model != BlockModel::Boxes {
            self.boxes.clear();
        }
        self.model = BlockModel::Boxes;
        self.flags &= !bf::OPAQUE;
        if self.layer == RenderLayer::Opaque {
            self.layer = RenderLayer::Cutout;
        }
        let Some(o) = self.orientation else {
            self.boxes
                .push(ModelBox { min, max, textures, uv, turns: [0; 6], tilt: abi::BoxTilt::NONE });
            return;
        };
        // From where the box is on the floor, before it turns.
        let uv: [[f32; 4]; 6] = std::array::from_fn(|i| if uv[i] == abi::UV_FROM_BOX { abi::box_face_uv(i, min, max) } else { uv[i] });
        let (min, max, own, turns) = o.place(min, max, [0usize, 1, 2, 3, 4, 5]);
        let textures = std::array::from_fn(|i| textures[own[i]].clone());
        self.boxes
            .push(ModelBox { min, max, textures, uv: own.map(|i| uv[i]), turns, tilt: abi::BoxTilt::NONE });
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
            turns: [0; 6],
            boxes: Vec::new(),
            hitbox: Vec::new(),
            outline: Vec::new(),
            orientation: None,
            components: Vec::new(),
            signal: abi::SignalDesc::DEFAULT,
            liquid_walls: u8::MAX,
        }
    }

    /// Sides a liquid in its cell can't pass, before its orientation turns them.
    fn set_liquid_walls(&mut self, sides: &[crate::Dir]) {
        let mask = sides.iter().fold(0u8, |m, d| m | 1 << d.face());
        self.liquid_walls = match self.orientation {
            Some(o) => {
                let (_, _, own, _) = o.place([0.0; 3], [16.0; 3], [0usize, 1, 2, 3, 4, 5]);
                (0..6).fold(0, |m, i| if mask & 1 << own[i] != 0 { m | 1 << i } else { m })
            }
            None => mask,
        };
    }
}

#[derive(Default)]
struct BlockCallbacks {
    on_use: abi::Callback,
    on_neighbor: abi::Callback,
    on_scheduled: abi::Callback,
    on_carried: abi::Callback,
    signal_read: abi::Callback,
    placement: abi::Callback,
    merge: abi::Callback,
    on_grow: abi::Callback,
    on_signal: abi::Callback,
    signal_gate: abi::Callback,
}

/// Per-state settings, used directly and through `BlockBuilder::states`.
pub struct State<'a> {
    def: &'a mut StateDef,
}

macro_rules! state_setters {
    () => {
        /// Seconds to mine by hand: the `hardness` component, as are the light, friction, drag,
        /// support and tint setters (other mods change them with `r.set`).
        pub fn hardness(mut self, h: f32) -> Self {
            self.each(|s| s.set(&c::HARDNESS, h));
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
            self.each(|s| s.set(&c::LIGHT, [r, g, b]));
            self
        }
        /// Per-channel absorption of passing light (stained glass).
        pub fn light_filter(mut self, r: u8, g: u8, b: u8) -> Self {
            self.each(|s| s.filter = [r, g, b]);
            self
        }
        /// Same texture on every face, relative names are under `block/`.
        pub fn texture(mut self, t: &str) -> Self {
            self.each(|s| s.textures = std::array::from_fn(|_| crate::asset("block", t)));
            self
        }
        /// Top, sides, bottom.
        pub fn textures(mut self, top: &str, side: &str, bottom: &str) -> Self {
            self.each(|s| {
                let (t, si, b) = (crate::asset("block", top), crate::asset("block", side), crate::asset("block", bottom));
                s.textures = [si.clone(), si.clone(), b, t, si.clone(), si];
            });
            self
        }
        /// Faces in -X, +X, -Y, +Y, -Z, +Z order.
        pub fn faces(mut self, f: [&str; 6]) -> Self {
            self.each(|s| s.textures = std::array::from_fn(|i| crate::asset("block", f[i])));
            self
        }
        /// Quarter turns of each face's picture, -X, +X, -Y, +Y, -Z, +Z: a log lying on its side
        /// turns its bark along it. For a cube; boxes have `turned`.
        pub fn face_turns(mut self, turns: [u8; 6]) -> Self {
            self.each(|s| s.turns = turns.map(|t| t % 4));
            self
        }
        /// Not opaque, rendered alpha-tested, lets light through (glass, leaves).
        pub fn cutout(mut self) -> Self {
            self.each(|s| {
                s.flags &= !bf::OPAQUE;
                s.layer = RenderLayer::Cutout;
            });
            self
        }
        /// Hide faces between neighbors of the same block (glass panes of a window).
        pub fn cull_same(mut self) -> Self {
            self.each(|s| s.flags |= bf::CULL_SAME);
            self
        }
        /// Light it takes away, 0..15 (leaves 1, ice 2), when what it looks like isn't enough to
        /// go by: otherwise 15 for what is opaque and 0 for the rest.
        pub fn light_opacity(mut self, o: u8) -> Self {
            self.each(|s| s.opacity = Some(o));
            self
        }
        pub fn translucent(mut self) -> Self {
            self.each(|s| {
                s.flags &= !bf::OPAQUE;
                s.flags |= bf::CULL_SAME;
                s.layer = RenderLayer::Translucent;
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
            self.each(|s| {
                s.set(&c::SUPPORT_FACE, side.face() as u8);
                if !tag.is_empty() {
                    s.set_text(abi::comp::SUPPORT, "tag", tag);
                }
            });
            self
        }
        /// The supporting block must have a block tagged `tag` beside it (sugar cane: `#water`),
        /// unless it is this same block.
        pub fn needs_near(mut self, tag: &str) -> Self {
            self.each(|s| s.set_text(abi::comp::SUPPORT_NEAR, "tag", tag));
            self
        }
        /// A plant: tagged `plantable`, cross model, no collision. Placing a block into it is
        /// refused, as for vanilla's flowers and sugar cane; grass that gives way is `.replaceable()`.
        pub fn plant(mut self) -> Self {
            self.each(|s| {
                s.flags &= !(bf::SOLID | bf::OPAQUE);
                s.model = BlockModel::Cross;
                s.layer = RenderLayer::Cutout;
                s.set(&c::HARDNESS, 0.0);
                s.components.push(Pending::tag("plantable"));
            });
            self
        }
        /// Keeps the liquid source it is placed in, and a bucket fills it: waterlogging.
        pub fn holds_liquid(mut self) -> Self {
            self.each(|s| s.flags |= bf::HOLDS_LIQUID);
            self
        }
        pub fn not_solid(mut self) -> Self {
            self.each(|s| s.flags &= !bf::SOLID);
            self
        }
        pub fn replaceable(mut self) -> Self {
            self.each(|s| s.flags |= bf::REPLACEABLE);
            self
        }
        pub fn climbable(mut self) -> Self {
            self.each(|s| s.flags |= bf::CLIMBABLE);
            self
        }
        /// Colored by the biome it stands in: `tints::GRASS`, `FOLIAGE`, `WATER` or a mod's own
        /// kind (`Registry::tint_kind`).
        pub fn tint(mut self, kind: &TintKind) -> Self {
            self.each(|s| s.set_text(abi::comp::TINT, "tint", kind.name()));
            self
        }
        /// `tint` on the top face only (grass blocks: the sides carry their own color).
        pub fn tint_top(mut self, kind: &TintKind) -> Self {
            self.each(|s| {
                s.set_text(abi::comp::TINT, "tint", kind.name());
                s.set(&c::TINT_FACES, 1 << 3);
            });
            self
        }
        pub fn friction(mut self, f: f32) -> Self {
            self.each(|s| s.set(&c::FRICTION, f));
            self
        }
        pub fn drag(mut self, d: f32) -> Self {
            self.each(|s| s.set(&c::DRAG, d));
            self
        }
        /// How many times its loot is rolled when it breaks: 2 for two slabs in one block.
        pub fn loot_rolls(mut self, n: u32) -> Self {
            self.each(|s| s.set(&c::LOOT_ROLLS, n));
            self
        }
        /// Custom model made of boxes in 1/16 units, each face showing the state's texture of
        /// that face (`texture`, `textures`, `faces`). It collides and is selected box by box, a
        /// stair being one slab and a quarter, unless `hitbox` is set. A
        /// `models/block/<name>.json` (Blockbench's Java block model) does the same from a file.
        pub fn model_box(mut self, min: [f32; 3], max: [f32; 3]) -> Self {
            self.each(|s| s.add_box(min, max, Default::default(), [abi::UV_FROM_BOX; 6]));
            self
        }
        /// `model_box` with its own textures and texel rectangles (u0, v0, u1, v1, 0..16) per face
        /// in -X, +X, -Y, +Y, -Z, +Z order, as a model file's element has; `UV_FROM_BOX` takes it
        /// from where the box is, `UV_NO_FACE` leaves a face out.
        pub fn model_box_faces(mut self, min: [f32; 3], max: [f32; 3], faces: [(&str, [f32; 4]); 6]) -> Self {
            self.each(|s| {
                let textures = std::array::from_fn(|i| crate::asset("block", faces[i].0));
                s.add_box(min, max, textures, faces.map(|f| f.1));
            });
            self
        }
        /// `model_box`, `model_box_faces` and `hitbox` from here on are written for a floor with
        /// the front to the north, and turned as `o` says (`Props::orientation`), pictures included.
        pub fn oriented(mut self, o: crate::signal::Orientation) -> Self {
            self.each(|s| s.orientation = Some(o));
            self
        }
        /// The last box's pictures get quarter turns of their own, per face.
        pub fn turned(mut self, turns: [u8; 6]) -> Self {
            self.each(|s| {
                let o = s.orientation;
                let Some(last) = s.boxes.last_mut() else { return };
                for (own, t) in turns.iter().enumerate() {
                    let to = o.map_or(own, |f| f.face(own));
                    last.turns[to] = (last.turns[to] + t) % 4;
                }
            });
            self
        }
        /// The last box is tilted: around a point (texels) about an axis (its own -X, +X ... as a
        /// `Dir`) by degrees, right-handed, like a model file's element.
        pub fn tilted(mut self, origin: [f32; 3], axis: crate::Dir, degrees: f32) -> Self {
            self.each(|s| {
                let o = s
                    .orientation
                    .unwrap_or(crate::signal::Orientation::new(crate::Dir::Down, crate::Dir::North));
                let Some(last) = s.boxes.last_mut() else { return };
                last.tilt = abi::BoxTilt { origin: o.at(origin), axis: o.along(axis), degrees };
            });
            self
        }
        /// Doesn't carry redstone power, the way glass doesn't. Wire can still lie on it.
        pub fn signal_blocker(mut self) -> Self {
            self.each(|s| s.flags |= bf::SIGNAL_BLOCKER);
            self
        }
        /// Pistons and floaters can't move it. A state with this on a platform stops the platform, if
        /// more of the platform is pushing it.
        pub fn immovable(mut self) -> Self {
            self.each(|s| s.flags |= bf::IMMOVABLE);
            self
        }
        /// A piston moving it takes what touches it along, like slime.
        pub fn sticky(mut self) -> Self {
            self.each(|s| s.flags |= bf::STICKY);
            self
        }
        /// Its shape, in 1/16 units like the model's (a slab: `[0.0; 3]` to `[16.0, 8.0, 16.0]`), one
        /// box a call: what stops a walker, what the crosshair outlines and cracks. On a block that
        /// isn't solid (`not_solid`, `plant`), only the outline. Without any, the model's boxes,
        /// else a plant's cross, else the whole block.
        pub fn hitbox(mut self, min: [f32; 3], max: [f32; 3]) -> Self {
            self.each(|s| {
                s.hitbox
                    .push(s.orientation.map_or((min, max), |f| f.corners(min, max)))
            });
            self
        }
        /// What the crosshair outlines and cracks, one box a call, when it shouldn't be the
        /// hitbox's: a hollow log stopped by its walls and outlined whole. Turned by `oriented`.
        pub fn outline(mut self, min: [f32; 3], max: [f32; 3]) -> Self {
            self.each(|s| {
                s.outline
                    .push(s.orientation.map_or((min, max), |f| f.corners(min, max)))
            });
            self
        }
        /// The sides a liquid in it can't pass (`.holds_liquid()`), turned by `oriented`: without
        /// this, the ones its hitbox covers whole (a stair's back holds water, a hollow log's
        /// open ends don't). `&[]` lets it out everywhere.
        pub fn liquid_walls(mut self, sides: &[crate::Dir]) -> Self {
            self.each(|s| s.set_liquid_walls(sides));
            self
        }
        /// Air-like: invisible, no collision, not targetable.
        pub fn air(mut self) -> Self {
            self.each(|s| {
                s.flags = (s.flags & !(bf::SOLID | bf::OPAQUE)) | bf::REPLACEABLE | bf::NOT_TARGETABLE;
                s.model = BlockModel::None;
                s.layer = RenderLayer::Opaque;
            });
            self
        }
        /// Raw `block_flags` on top of what the presets set.
        pub fn with(mut self, f: u32) -> Self {
            self.each(|s| s.flags |= f);
            self
        }
    };
}

impl<'a> State<'a> {
    fn each(&mut self, f: impl Fn(&mut StateDef)) {
        f(self.def)
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
}

type BlockLook = Box<dyn for<'r> FnOnce(BlockBuilder<'r>) -> BlockBuilder<'r>>;
type EntityLook = Box<dyn for<'r> FnOnce(crate::EntityBuilder<'r>) -> crate::EntityBuilder<'r>>;

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
    /// `Registry::replace`: its item replaces theirs.
    replacing: bool,
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
    /// Its block entity's kind, `entity`.
    entity: Option<EntityLook>,
    model_file: String,
    model_textures: Vec<(String, String)>,
    /// What its `on_neighbor` does, `joins` and `derived` included, run in order.
    neighbor_fns: Vec<NeighborFn>,
}

type NeighborFn = Box<dyn Fn(&mut BlockEvent) + Send + Sync>;

fn block_event_cb(f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::BlockEventArgs| f(BlockEvent::wrap(a)))
}

/// The blocks around one (`BlockBuilder::derived`).
pub struct Neighbors {
    world: World,
    pos: BlockPos,
}

impl Neighbors {
    pub fn pos(&self) -> BlockPos {
        self.pos
    }
    pub fn world(&self) -> World {
        self.world
    }
    /// The block on that side, air where nothing is loaded.
    pub fn at(&self, side: crate::Dir) -> u32 {
        let o = side.offset();
        self.world
            .block([self.pos.x + o.x, self.pos.y + o.y, self.pos.z + o.z])
    }
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
    /// `.oriented`.
    pub fn orientation(&self) -> crate::signal::Orientation {
        let attach = crate::Dir::from_name(self.get("attach")).unwrap_or(crate::Dir::Down);
        let front = crate::Dir::from_name(self.get("front")).unwrap_or_else(|| self.facing());
        crate::signal::Orientation::new(attach, front)
    }
    /// A block that faces along `facing`, with its top that way: for `.oriented`.
    pub fn orientation_up(&self) -> crate::signal::Orientation {
        let f = self.facing();
        let north = if f.face() / 2 == 1 { crate::Dir::North } else { crate::Dir::Up };
        crate::signal::Orientation::new(f.opposite(), north)
    }
    /// A block with its north to `facing`, the way vanilla turns one: its top stays up unless it
    /// faces up or down.
    pub fn orientation_front(&self) -> crate::signal::Orientation {
        let f = self.facing();
        let attach = match f {
            crate::Dir::Up => crate::Dir::North,
            crate::Dir::Down => crate::Dir::South,
            _ => crate::Dir::Down,
        };
        crate::signal::Orientation::new(attach, f)
    }
    pub fn set(&mut self, name: &str, value: &str) {
        if let Some(v) = self.values.iter_mut().find(|v| v.0 == name) {
            v.1 = value.to_string();
        }
    }
}

/// Every combination of property values, the last property changing fastest.
fn state_combinations(props: &[(String, Vec<String>)]) -> Vec<Props> {
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

/// The state index of `p` among `state_combinations(props)`; values it does not know count as the first.
fn state_index(props: &[(String, Vec<String>)], p: &Props) -> u32 {
    props
        .iter()
        .fold(0, |i, (name, values)| i * values.len() as u32 + values.iter().position(|v| v == p.get(name)).unwrap_or(0) as u32)
}

impl<'a> BlockBuilder<'a> {
    fn each(&mut self, f: impl Fn(&mut StateDef)) {
        for s in &mut self.states {
            f(s);
        }
    }

    state_setters!();

    /// A named property with its values, the first being the default: `.prop("lit", ["false",
    /// "true"])`, `.prop("age", 0..=7)`. The block has a state for every combination of its
    /// properties; read and change them with `World::prop` / `prop_set`.
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

    /// Tools of `class` (`tools/<class>` items) mine it at their speed (tag `mineable/<class>`),
    /// and it drops nothing unless mined by one of at least `level` (`tool_tier`; `HAND`: drops
    /// anyway). The level is the `harvest_level` component, checked by `Cond::Harvested`.
    pub fn mined_by(mut self, class: &str, level: u32) -> Self {
        self.components
            .push(Pending::tag(&format!("mineable/{class}")));
        if level != abi::tool_tier::HAND {
            self.components.push(c::HARVEST_LEVEL.raw_value(&level));
        }
        self
    }

    /// Shades its textures with a shader handle (`Shader::named("stripes")`, the file
    /// `shaders/block/stripes.wgsl`): the shader goes
    /// with the textures, so blocks sharing one share it.
    pub fn shader(mut self, shader: &crate::Shader) -> Self {
        self.components
            .push(Pending::text(abi::comp::SHADER, shader.name()));
        self
    }

    /// Creative inventory tab of the block's item, see `Registry::creative_tab`.
    pub fn tab(self, tab: &CreativeTab) -> Self {
        self.tag(&tab_tag(tab))
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

    /// On an edit (`Registry::edit`) keeping the block's states: block texture `from` (another
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
    pub fn grows(mut self, mean_seconds: f32, min_light: u32) -> Self {
        self.each(|s| {
            s.set(&c::GROW_TIME, mean_seconds);
            s.set(&c::GROW_LIGHT, min_light);
        });
        self
    }

    /// What a growth step does instead of moving to the next state (growing a tree, a block on
    /// top); it keeps growing as long as the block stays.
    pub fn on_grow(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_grow = block_event_cb(f);
        self
    }

    /// Spreads onto blocks with `tag` near it (grass onto `#grass_spreads_to`): a target
    /// within 1 block sideways, 3 down and 1 up with `min_light` above it and nothing opaque on
    /// top turns into this block in `mean_seconds` on average (sooner with more of this around),
    /// also while nobody is near (caught up on loading).
    pub fn spreads(mut self, tag: &str, mean_seconds: f32, min_light: u32) -> Self {
        self.each(|s| {
            s.set_text(abi::comp::SPREADS_TO, "tag", tag);
            s.set(&c::SPREAD_TIME, mean_seconds);
            s.set(&c::SPREAD_LIGHT, min_light);
        });
        self
    }

    /// Where `spreads` reaches, from this block to the target: min and max offsets, at most 4
    /// each way (grass: `[-1, -3, -1]` to `[1, 1, 1]`, the default). With `covered`, targets
    /// with something opaque on top count too, lit by their brightest side (moss up a wall).
    pub fn spread_reach(mut self, min: impl Into<BlockPos>, max: impl Into<BlockPos>, covered: bool) -> Self {
        let min = min.into();
        let max = max.into();
        let v = [min.x, min.y, min.z, max.x, max.y, max.z];
        self.each(|s| {
            s.set(&c::SPREAD_REACH, v);
        });
        if covered { self.tag(abi::comp::SPREADS_COVERED) } else { self }
    }

    /// Turns into `block` in `mean_seconds` on average once something opaque covers it (grass
    /// into dirt).
    pub fn smothered_into(mut self, block: &str, mean_seconds: f32) -> Self {
        self.each(|s| {
            s.set_text(abi::comp::SMOTHERED_INTO, "block", block);
            s.set(&c::SMOTHER_TIME, mean_seconds);
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
        self.each(|s| s.set_text(abi::comp::AMBIENT_PARTICLE, "str", &kinds));
        self
    }

    pub fn on_use(mut self, f: impl Fn(&mut Use) -> bool + Send + Sync + 'static) -> Self {
        self.callbacks.on_use = callback(move |a: &mut abi::UseArgs| {
            let handled = f(Use::wrap(a));
            a.handled = handled as u32;
        });
        self
    }

    /// One of these was placed: `actions::PLACE_BLOCK` for this block, at the end of the tick.
    pub fn on_place(self, f: impl Fn(&World, &crate::BlockChange) + Send + Sync + 'static) -> Self {
        self.on_own(&crate::actions::PLACE_BLOCK, f)
    }

    /// One of these was broken, by a player (`player`) or by losing its support (0).
    pub fn on_break(self, f: impl Fn(&World, &crate::BlockChange) + Send + Sync + 'static) -> Self {
        self.on_own(&crate::actions::BREAK_BLOCK, f)
    }

    /// `action`'s changes of this block.
    fn on_own(
        self,
        action: &'static crate::Action<crate::BlockChange>,
        f: impl Fn(&World, &crate::BlockChange) + Send + Sync + 'static,
    ) -> Self {
        let Some(block) = self.handle else { return self };
        self.reg.on_event(action, move |b| {
            let w = b.world();
            for p in b.iter().filter(|p| block.is(p.state)) {
                f(&w, p);
            }
        });
        self
    }

    pub fn on_neighbor(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.neighbor_fns.push(Box::new(f));
        self
    }

    /// A `true`/`false` property for each of `sides` (`north`, `east` .. `up`, `down`) the engine
    /// keeps saying whether the block there is one it joins: blocks tagged `to` (`#fences`,
    /// another mod's pipes by a shared tag), named `to`, or with `"self"` its own kind. Fences,
    /// walls, panes, pipes and cables: `.states` shapes it by `p.is("north", "true")`.
    pub fn joins(mut self, sides: crate::Facings, to: &str) -> Self {
        for d in sides.dirs() {
            self = self.prop(d.name(), ["false", "true"]);
        }
        let to = to.to_string();
        self.derive_each(move |n, e| {
            let own = e.state();
            for d in sides.dirs() {
                let there = n.at(d);
                let joins = there != 0
                    && match to.as_str() {
                        "self" => crate::block_of(there) == crate::block_of(own),
                        t if t.starts_with('#') => crate::block_has_tag(there, &t[1..]),
                        name => crate::block_name(there).split('[').next() == Some(name),
                    };
                let want = if joins { "true" } else { "false" };
                if n.world().prop(n.pos(), d.name()) != Some(want) {
                    n.world().prop_set(n.pos(), d.name(), want);
                }
            }
        })
    }

    /// Property `prop` worked out from the blocks around, kept up to date by the engine as they
    /// change (a stair's corner shape). `f` gives the value; the property must have it.
    pub fn derived(self, prop: &'static str, f: impl Fn(&Neighbors) -> &'static str + Send + Sync + 'static) -> Self {
        self.derive_each(move |n, _| {
            let want = f(n);
            if n.world().prop(n.pos(), prop) != Some(want) {
                n.world().prop_set(n.pos(), prop, want);
            }
        })
    }

    fn derive_each(mut self, f: impl Fn(&Neighbors, &BlockEvent) + Send + Sync + 'static) -> Self {
        self.neighbor_fns.push(Box::new(move |e: &mut BlockEvent| {
            let n = Neighbors { world: e.world(), pos: e.pos() };
            f(&n, e)
        }));
        self
    }

    /// On a moving body, it came to another cell of the world: a drill, a plow. `pos` is that cell,
    /// in the world's coordinates; `body_pos` where this is on the body, `entity` the body.
    pub fn on_carried(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_carried = block_event_cb(f);
        self
    }
    pub fn on_scheduled(mut self, f: impl Fn(&mut BlockEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_scheduled = block_event_cb(f);
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
            crate::reg_mistake("two_part needs a block registered by its handle");
            return self;
        };
        let other = move |pos: BlockPos, state: u32| crate::other_part(pos, state, prop, parts[1], way);
        self.reg
            .before_event(&crate::actions::PLACE_BLOCK, move |q| {
                if !block.is(q.state()) {
                    return crate::Verdict::Abstain;
                }
                let there = q.world().block(other(q.pos(), q.state()));
                if there == 0 || crate::state_flags(there) & bf::REPLACEABLE != 0 { crate::Verdict::Abstain } else { crate::Verdict::Deny }
            });
        self.prop(prop, parts)
            .on_neighbor(|e| e.world().schedule_tick(e.pos(), 1))
            .on_scheduled(move |e| {
                let w = e.world();
                if block.is(e.state()) && !block.is(w.block(other(e.pos(), e.state()))) {
                    w.set_block(e.pos(), 0);
                }
            })
            .on_place(move |w, c| {
                if let Some(s) = crate::state_with(c.state, prop, parts[1]) {
                    w.set_block(other(c.pos, c.state), s);
                }
            })
            .on_break(move |w, c| {
                let there = other(c.pos, c.state);
                if block.is(w.block(there)) {
                    w.set_block(there, 0);
                }
            })
    }

    /// Falls like sand when nothing holds it up: a block next to it changing checks two ticks
    /// later whether below is free, and if so it drops as a moving block (`World::fall`).
    pub fn falling(self) -> Self {
        self.on_neighbor(|e| e.world().schedule_tick(e.pos(), 2))
            .on_scheduled(|e| {
                let (w, p) = (e.world(), e.pos());
                if w.block_flags([p.x, p.y - 1, p.z]) & abi::block_flags::REPLACEABLE != 0 {
                    w.fall_block(p);
                }
            })
    }

    /// Each placed block gets a block entity, an invisible entity of a kind named like the block
    /// that comes and goes with it: data of its own (`e.attach(..)`, ticked by a system), a
    /// model, items (`e.inventory(27)`). Doesn't make it usable: that is `opens` or `on_use`.
    /// `.entity(|e| e.attach(&SPEED))`. Its items drop when it breaks.
    pub fn entity(mut self, f: impl for<'r> FnOnce(crate::EntityBuilder<'r>) -> crate::EntityBuilder<'r> + 'static) -> Self {
        self.entity = Some(Box::new(f));
        self
    }

    /// Using it opens `menu`, on its block entity if it has one (a furnace), else on nothing (a
    /// crafting table). A menu with `.recipes(..)` makes the block a station of those recipes in
    /// the recipe viewer.
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
    pub fn default_facing(mut self, d: crate::Dir) -> Self {
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
        self.each(|s| {
            s.flags &= !(bf::SOLID | bf::OPAQUE);
            s.model = BlockModel::Wire;
            s.layer = RenderLayer::Cutout;
            s.set(&c::HARDNESS, 0.0);
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
    pub fn signal_constant(self) -> Self {
        self.signal_props(abi::signal_kind::SWITCH, 0, false, false)
    }
    /// A lever or button: goes on the clicked face; its `power` property (`off`, `on_event`) gives full
    /// power around it and strongly into the block it is on.
    pub fn signal_switch(self) -> Self {
        self.signal_props(abi::signal_kind::SWITCH, 2, true, true)
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
    pub fn signal_read(mut self, f: impl Fn(&World, BlockPos) -> u8 + Send + Sync + 'static) -> Self {
        self.callbacks.signal_read = callback(move |a: &mut abi::SignalReadArgs| a.output = f(&World::from_raw(a.world), a.pos) as u32);
        self
    }
    /// Goes along its `facing` (after `facing`), any of the six ways. Its front is what sticks: with
    /// power from any other side it carries itself, the block in front of it and what is stuck to
    /// that the way slime does, half a block a second per level of power. Circuits on it go on.
    /// Without power, or against what it can't go through, it all lands on a whole block.
    /// `power`: 0 idle, 1 going, 2 stuck. `breaks`: it breaks what is in its way instead.
    pub fn signal_floater(mut self, breaks: bool) -> Self {
        let flags = if breaks { abi::signal_flags::BREAKS } else { 0 };
        self.signal = Some(SignalBuild { flags, ..SignalBuild::default() });
        self.signal_props(abi::signal_kind::FLOATER, 3, false, false)
    }
    /// Pushes blocks at its `facing` with power from any other side, the `power` property saying
    /// it is out (after `facing`). `head`: the block it pushes out, with a `facing` property.
    /// `sticky` brings a block back.
    pub fn signal_piston(mut self, head: &'static str, sticky: bool) -> Self {
        let flags = if sticky { abi::signal_flags::STICKY } else { 0 };
        self.signal = Some(SignalBuild { head, flags, ..SignalBuild::default() });
        self.signal_props(abi::signal_kind::PISTON, 2, false, false)
    }
    /// The moving part of a piston (after `facing`): it goes when the piston does.
    pub fn signal_piston_head(mut self) -> Self {
        self.each(|s| s.flags |= bf::NOT_STOOD_ON);
        self.signal_props(abi::signal_kind::PISTON_HEAD, 0, false, false)
    }
    /// A gate with logic of your own, on the clicked face: what comes out of the front a
    /// `gate`'s delay after what goes in at its inputs calls for it. Runs only when something
    /// around it changes.
    pub fn signal_gate(mut self, gate: Gate, f: impl Fn(&GateInputs) -> u8 + Send + Sync + 'static) -> Self {
        self.callbacks.signal_gate = callback(move |a: &mut abi::GateArgs| {
            a.output = f(GateInputs::wrap(a)) as u32;
        });
        // In redstone ticks, like a repeater's: two game ticks each.
        self.signal = Some(SignalBuild { delay: Some(gate.delay * 2), inputs: gate.inputs, ..self.signal.unwrap_or_default() });
        self.signal_props(abi::signal_kind::GATE, 16, true, true)
    }
    /// Called when the power around a `signal_sink` changes.
    pub fn on_signal(mut self, f: impl Fn(&SignalEvent) + Send + Sync + 'static) -> Self {
        self.callbacks.on_signal = callback(move |a: &mut abi::SignalArgs| f(SignalEvent::wrap(a)));
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

    /// Its shape from model file `name` (`models/block/<path>.json` of that mod) instead of the one
    /// named like the block: several blocks, any mod's, on one model (#88).
    pub fn model_file(mut self, name: &str) -> Self {
        self.model_file = crate::full_name(name);
        self
    }

    /// The texture this block puts where the model file says `#name`: one model, each block
    /// its own pictures (a hollow log's inside, stripped). Relative names are under `block/`.
    pub fn model_texture(mut self, name: &str, texture: &str) -> Self {
        self.model_textures
            .push((name.trim_start_matches('#').to_string(), crate::asset("block", texture)));
        self
    }

    /// Not a block but a template: see `Registry::shapes_of`.
    pub(crate) fn shapes_of(mut self, sources: &str, suffix: &str) -> Self {
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
            self.states = state_combinations(&self.props)
                .iter()
                .map(|p| {
                    let mut d = template.clone();
                    if let Some(f) = &looks {
                        f(p, State { def: &mut d });
                    }
                    if let Some(b) = &signal {
                        b.apply(p, &mut d);
                    }
                    d
                })
                .collect();
        } else if let Some(b) = &signal {
            let none = Props { values: Vec::new() };
            for d in &mut self.states {
                b.apply(&none, d);
            }
        }
        let attaches = signal.is_some_and(|b| b.attaches);
        let switch = signal.is_some_and(|b| b.kind == abi::signal_kind::SWITCH);
        if self.facing.is_some() || self.placement.is_some() || attaches {
            let (props, facing, user) = (self.props.clone(), self.facing, self.placement.take());
            self.callbacks.placement = callback(move |a: &mut abi::PlaceArgs| {
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
                    let front =
                        if switch && attach.horizontal() { crate::Dir::Down } else { crate::signal::dir_along_face(attach, a.look) };
                    p.set("front", front.name());
                }
                let first = a.state;
                if let Some(f) = &user {
                    f(Place::wrap(a), &mut p);
                }
                a.state = first + state_index(&props, &p);
            });
        }
        if let Some(f) = self.merge.take() {
            let props = self.props.clone();
            self.callbacks.merge = callback(move |a: &mut abi::PlaceArgs| {
                let Some(mut p) = state_combinations(&props)
                    .into_iter()
                    .nth((a.state - a.first) as usize)
                else {
                    return;
                };
                f(Place::wrap(a), &mut p);
                a.state = a.first + state_index(&props, &p);
            });
        }
        let shares_cell: Vec<Str> = self.shares_cell.iter().map(|x| Str::new(x)).collect();
        let model_textures: Vec<Str> = self
            .model_textures
            .iter()
            .flat_map(|(a, b)| [Str::new(a), Str::new(b)])
            .collect();
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
                    .map(|b| abi::ModelBox {
                        turn: b.turns,
                        tilt: b.tilt,
                        min: b.min,
                        max: b.max,
                        textures: std::array::from_fn(|i| Str::new(&b.textures[i])),
                        uv: b.uv,
                    })
                    .collect()
            })
            .collect();
        let mut scratch: Vec<Vec<Vec<u8>>> = vec![Vec::new(); self.states.len()];
        // The engine takes boxes as min and max in one array.
        let flat = |list: &[([f32; 3], [f32; 3])]| -> Vec<[f32; 6]> {
            list.iter()
                .map(|(a, b)| [a[0], a[1], a[2], b[0], b[1], b[2]])
                .collect()
        };
        let hitboxes: Vec<Vec<[f32; 6]>> = self.states.iter().map(|s| flat(&s.hitbox)).collect();
        let outlines: Vec<Vec<[f32; 6]>> = self.states.iter().map(|s| flat(&s.outline)).collect();
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
            .zip(&hitboxes)
            .zip(&outlines)
            .map(|((((s, b), v), h), o)| abi::StateDesc {
                flags: s.flags,
                filter: s.filter,
                opacity: s
                    .opacity
                    .unwrap_or(if s.flags & bf::OPAQUE != 0 { 15 } else { 0 }),
                layer: s.layer,
                model: s.model,
                textures: std::array::from_fn(|i| Str::new(&s.textures[i])),
                turns: s.turns,
                boxes: Slice::new(b),
                hitbox: Slice::new(h),
                outline: Slice::new(o),
                components: Slice::new(v),
                signal: s.signal,
                liquid_walls: s.liquid_walls,
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
        if !self.neighbor_fns.is_empty() {
            let fns = std::mem::take(&mut self.neighbor_fns);
            self.callbacks.on_neighbor = block_event_cb(move |e| {
                for f in &fns {
                    f(e);
                }
            });
        }
        let cb = &self.callbacks;
        let desc = abi::BlockDesc {
            name: Str::new(&self.name),
            cell: cell(self.handle),
            states: Slice::new(&states),
            props: Slice::new(&props),
            components: Slice::new(&values),
            on_use: cb.on_use,
            on_neighbor: cb.on_neighbor,
            on_scheduled: cb.on_scheduled,
            on_carried: cb.on_carried,
            signal_read: cb.signal_read,
            merge: cb.merge,
            shapes_of: self.made_of.as_ref().map_or(Str::EMPTY, |m| Str::new(&m.0)),
            shapes_suffix: self.made_of.as_ref().map_or(Str::EMPTY, |m| Str::new(&m.1)),
            shares_cell: Slice::new(&shares_cell),
            model_file: Str::new(&self.model_file),
            model_textures: Slice::new(&model_textures),
            placement: cb.placement,
            edit: self.replace as u32,
            on_grow: cb.on_grow,
            on_signal: cb.on_signal,
            signal_gate: cb.signal_gate,
            own_states: (self.own_states || !self.replace) as u32,
            retexture: Slice::new(&retexture),
            opens: Str::new(&self.opens),
            ..abi::BlockDesc::DEFAULT
        };
        if let Some(h) = self.handle {
            h.set_count(self.states.len() as u32);
        }
        unsafe { (host().reg_block)(self.reg.raw, &desc) };
        if let Some(f) = self.entity.take() {
            // Its block entity is edited or replaced with it.
            let mode = match (self.replace, self.replacing) {
                (true, _) => abi::entry_mode::EDIT,
                (_, true) => abi::entry_mode::REPLACE,
                _ => abi::entry_mode::NEW,
            };
            self.reg
                .block_entity(Block::generated(self.name.clone()), mode, f);
        }

        let texture = std::mem::take(&mut self.item_texture);
        match (self.replace, self.item) {
            // An edit leaves the block's item as it is, but for what it says of it.
            (true, true) if !texture.is_empty() => drop(
                self.reg
                    .edit(Item::generated(self.name.clone()))
                    .set_text(&crate::comps::item::TEXTURE, &texture),
            ),
            (true, true) => {}
            (true, false) => self.reg.remove(Item::generated(self.name.clone())),
            (false, true) => {
                let mode = if self.replacing { abi::entry_mode::REPLACE } else { abi::entry_mode::NEW };
                drop(self.reg.block_item(&self.name, texture, mode))
            }
            (false, false) => {}
        }
        if let Some((item, count)) = self.drops.take() {
            // An edit's drops are the block's table from now on.
            let block = Block::generated(self.name.clone());
            let table = if self.replace { self.reg.replace(block.loot()) } else { self.reg.loot(block) };
            let table = table.set(&crate::comps::loot::POOLS, Vec::new());
            if !item.is_empty() {
                drop(
                    table.pool(
                        crate::extend::Pool::new()
                            .when(crate::extend::Cond::Harvested)
                            .items(&item, count, count),
                    ),
                );
            }
        }
    }
}

pub struct ItemBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

crate::entry::entry_builder!(
    Item => ItemBuilder, item;
    Biome => BiomeBuilder, biome;
    Dimension => DimensionBuilder, dimension;
);

impl ItemBuilder<'_> {
    /// How a stack of it looks by what is in it, decided once per kind of stack (its data):
    /// `.condition(|s, v| if s.wear() > 0 { v.texture("can_empty"); })`. Other players' hands
    /// show the usual look: their stacks aren't sent.
    pub fn condition(self, f: impl Fn(&abi::ItemStack, &mut Looks) + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::ItemModelArgs| {
            let mut v = Looks::default();
            f(&a.stack, &mut v);
            LOOKS.with(|l| {
                *l.borrow_mut() = v;
                let l = l.borrow();
                (a.model, a.texture) = (Str::new(&l.model), Str::new(&l.texture));
            });
        });
        self.set(&crate::comps::item::CONDITION, cb)
    }
    pub fn texture(self, t: &str) -> Self {
        let t = crate::asset("item", t);
        self.set_text(&crate::comps::item::TEXTURE, &t)
    }
    /// Stack size (`max_stack`).
    pub fn max_stack(self, n: u32) -> Self {
        self.set(&c::MAX_STACK, n)
    }
    /// Its clips, from `animations/<name>.animation.json`; a file named after the item is used
    /// even without this. They play on whoever holds it.
    pub fn animations(self, name: &str) -> Self {
        let name = crate::asset("", name);
        self.set_text(&c::ANIMATIONS, &name)
    }
    /// Needs both hands: while the other holds something it does nothing, and it can't go in
    /// the other hand. Its `hold` action shows the two.
    pub fn two_handed(self) -> Self {
        self.tag(abi::comp::TWO_HANDED)
    }
    /// A `two_handed` item may be held in one hand after all, playing this action. It goes in
    /// the other hand then too.
    pub fn one_handed_action(mut self, action: &str) -> Self {
        self.e
            .ops
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
    /// A tool of `class` (tag `tools/<class>`): it mines blocks `mined_by` that class, at its
    /// `mining_speed`, harvesting those needing up to `level` (`tool_level`). Held like one.
    pub fn tool(self, class: &str, level: u32) -> Self {
        let tag = format!("tools/{class}");
        self.tag(&tag).set(&c::TOOL_LEVEL, level).handheld()
    }
    /// Held like a tool in third person, by its end and pointing ahead (tag `handheld`, vanilla's
    /// `item/handheld` model), instead of flat in the hand. Tools have it already.
    pub fn handheld(self) -> Self {
        self.tag("handheld")
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
    /// both know, so nothing flickers. A rule that depends on more is `actions::BREAK_BLOCK`.
    pub fn no_creative_break(self) -> Self {
        self.tag(abi::comp::NO_CREATIVE_BREAK)
    }
    /// Takes effect after holding the use button this many ticks (food 32), slowing the player down.
    pub fn use_ticks(self, ticks: u32) -> Self {
        self.set(&c::USE_TICKS, ticks)
    }
    /// Full swings per second: hitting sooner does less (a sword 1.6, an empty hand 4).
    pub fn attack_speed(self, per_second: f32) -> Self {
        self.set(&c::ATTACK_SPEED, per_second)
    }
    pub fn attack_damage(self, damage: f32) -> Self {
        self.set(&c::ATTACK_DAMAGE, damage)
    }
    pub fn places(self, block: &Block) -> Self {
        self.set_text(&crate::comps::item::BLOCK, block.name())
    }
    /// Worn in equipment `slot`, where it shows as `model` on the wearer's bone `bone`, moved by
    /// what plays on the wearer, like `Registry::bone_model`: a tail only those who wear it have.
    pub fn wearable_model(self, slot: &EquipmentSlot, model: &str, bone: &str) -> Self {
        let model = format!("{}@{bone}", crate::asset("", model));
        self.tag(&format!("worn/{}", slot.name()))
            .set(&c::WORN, abi::Worn { inflate: 0.0 })
            .set_text(&c::WORN_MODEL, &model)
    }

    /// Creative inventory tab, see `Registry::creative_tab`.
    pub fn tab(self, tab: &CreativeTab) -> Self {
        self.tag(&tab_tag(tab))
    }
    /// Worn in equipment `slot` (any mod's), where it shows on the wearer's model over the parts
    /// the slot covers: `texture` (under `entity/`, like `armor/iron_1`) in the wearer's skin
    /// layout, `inflate` pixels out from the body.
    pub fn wearable(self, slot: &EquipmentSlot, inflate: f32, texture: &str) -> Self {
        let tex = crate::asset("entity", texture);
        self.tag(&format!("worn/{}", slot.name()))
            .set(&c::WORN, abi::Worn { inflate })
            .set_text(&c::WORN_TEXTURE, &tex)
    }

    /// Used on a block, puts `entity` on that side of it. One is used up, outside creative. Entities
    /// have an egg of their own without this; it is for something else that hatches one.
    pub fn spawns(mut self, entity: &'static crate::EntityKind) -> Self {
        self.e
            .ops
            .push(Pending::text(abi::comp::SPAWN_EGG, entity.name()));
        self.on_use(move |u| {
            let Some((p, face)) = u.target_block() else { return false };
            let (w, side) = (u.world(), crate::Dir::from_face(face).offset());
            w.spawn(entity, [(p.x + side.x) as f64 + 0.5, (p.y + side.y) as f64, (p.z + side.z) as f64 + 0.5]);
            if !w.creative(u.player()) {
                u.set_stack(crate::StackData::shrink(u.stack(), 1));
            }
            true
        })
    }

    pub fn on_use(self, f: impl Fn(&mut ItemUse) -> bool + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::ItemUseArgs| {
            let handled = f(ItemUse::wrap(a));
            a.handled = handled as u32;
        });
        self.set(&crate::comps::item::ON_USE, cb)
    }
}

/// What `ItemBuilder::condition` changes of a stack's look; what it doesn't set stays.
#[derive(Default)]
pub struct Looks {
    model: String,
    texture: String,
}

impl Looks {
    /// A texture from `item/` instead of its own.
    pub fn texture(&mut self, t: &str) -> &mut Self {
        self.texture = crate::asset("item", t);
        self
    }
    /// A model file instead of its own (`set_model`).
    pub fn model(&mut self, m: &str) -> &mut Self {
        self.model = crate::asset("", m);
        self
    }
}

thread_local! {
    static LOOKS: std::cell::RefCell<Looks> = std::cell::RefCell::new(Looks::default());
}

pub struct BiomeBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

/// A biome's climate and surface (`comps::biome`). Terrain rules placing it start from these
/// (`BiomeRule::new("desert")` needs nothing else) and may change them there.
impl BiomeBuilder<'_> {
    /// Where field `name` of the terrain fits it; given again, the new range replaces the old.
    fn range(self, name: &str, range: impl std::ops::RangeBounds<f64>) -> Self {
        let (min, max) = crate::terrain::range_f64(range);
        self.push(&crate::comps::biome::RANGES, abi::RuleRange { name: name.to_string(), min, max })
    }
    /// Where the climate fits it, like `0.35..` or `..-0.45` (from inclusive, to exclusive);
    /// the terrain's fields are about -1.4..1.4. Without one it fits everywhere.
    pub fn temperature(self, range: impl std::ops::RangeBounds<f64>) -> Self {
        self.range("temperature", range)
    }
    pub fn humidity(self, range: impl std::ops::RangeBounds<f64>) -> Self {
        self.range("humidity", range)
    }
    /// Top block, then `depth` blocks of filler, then the terrain's stone.
    pub fn surface(self, top: &str, filler: &str, depth: i32) -> Self {
        let (top, filler) = (qualify(self.reg.ns, top), qualify(self.reg.ns, filler));
        self.set_text(&crate::comps::biome::TOP, &top)
            .set_text(&crate::comps::biome::FILLER, &filler)
            .set(&crate::comps::biome::DEPTH, depth)
    }
    /// The lowest `layers` of the filler become `block` (sandstone under sand).
    pub fn under(self, block: &str, layers: i32) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::biome::UNDER, &block)
            .set(&crate::comps::biome::UNDER_LAYERS, layers)
    }
    /// Top block for columns lower than `below` (gravel on deep sea floors).
    pub fn deep_top(self, block: &str, below: i32) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::biome::DEEP_TOP, &block)
            .set(&crate::comps::biome::DEEP_BELOW, below)
    }
    /// Plain stone above this height (bare peaks).
    pub fn bare_above(self, y: i32) -> Self {
        self.set(&crate::comps::biome::BARE_ABOVE, y)
    }
    /// A frozen biome: the water surface freezes into `block`, and it has the tag `frozen`
    /// (decorations put no flowers there).
    pub fn frozen(self, block: &str) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::biome::ICE, &block)
            .tag("frozen")
    }
    /// Rain falls as snow here: `precipitation(&precipitations::SNOW)`.
    pub fn snows(self) -> Self {
        self.precipitation(&precipitations::SNOW)
    }
    /// What falls here when a weather brings rain: `precipitations::SNOW`, or a mod's own.
    pub fn precipitation(self, p: &'static Precipitation) -> Self {
        self.set(&crate::component::PRECIPITATION, crate::component::Text::new(p.name()))
    }
    /// The color (`0xRRGGBB`) blocks tinted with `kind` take here (`tints::GRASS`, or a mod's
    /// own kind). Kinds not given keep their default.
    pub fn tint(self, kind: &'static TintKind, color: u32) -> Self {
        self.push(&crate::comps::biome::TINTS, abi::BiomeTint { kind: kind.name().to_string(), color })
    }
}

pub struct DimensionBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl DimensionBuilder<'_> {
    /// Must be a pure function of its input: it runs in parallel and must give the same cube for the
    /// same seed and position on every machine.
    pub fn generator(self, f: impl Fn(&mut Gen) + Send + Sync + 'static) -> Self {
        self.set(&crate::comps::dimension::GENERATOR, callback(move |a: &mut abi::GenArgs| f(Gen::wrap(a))))
    }
    /// The block a player starts on top of in this dimension, for a seed. Without one the engine
    /// finds open ground near 0, 0 on the dimension's columns (biomes tagged `spawn_biome`
    /// first), out of caves and trees.
    pub fn spawn_point(self, f: impl Fn(u64) -> BlockPos + Send + Sync + 'static) -> Self {
        self.set(&crate::comps::dimension::SPAWN, callback(move |a: &mut abi::SpawnArgs| a.pos = f(a.seed)))
    }
    /// Players start here and respawn here. When several mods claim it, the one loaded last
    /// wins; without any, the first dimension registered is used.
    pub fn player_spawn(self) -> Self {
        self.tag(abi::comp::dimension::PLAYER_SPAWN)
    }
    /// Blocks never update here (no neighbor updates, block ticks or growth).
    pub fn no_block_updates(self) -> Self {
        self.tag(abi::comp::dimension::NO_BLOCK_UPDATES)
    }
    /// Only what names it: ores, scatters and decorations meant for every dimension stay out
    /// (a superflat test world).
    pub fn bare(self) -> Self {
        self.tag(abi::comp::dimension::BARE)
    }
    /// Surface height and biome id of column (x, z) for (seed, x, z), for a dimension without a
    /// terrain. The generator reads them from `g.columns()` instead of working them out again,
    /// and structures, scatters (trees) and the spawn search stand on them.
    pub fn column(self, f: impl Fn(u64, i32, i32) -> (i32, u32) + Send + Sync + 'static) -> Self {
        let cb = callback(move |a: &mut abi::ColumnArgs| (a.height, a.biome) = f(a.seed, a.x, a.z));
        self.set(&crate::comps::dimension::COLUMN, cb)
    }
    /// Inclusive limits in cubes (16 blocks).
    pub fn height_cubes(self, min_cube: i32, max_cube: i32) -> Self {
        self.set(&crate::comps::dimension::CUBES, [min_cube, max_cube])
    }
    /// Everything at or above this cube y is empty, so sky light there needs no generation.
    pub fn sky_above(self, cube_y: i32) -> Self {
        self.set(&crate::comps::dimension::SKY_ABOVE, cube_y)
    }
    /// The weathers that come here now and then after clear weather, each by its weight and
    /// duration (`&[&weathers::RAIN, &weathers::THUNDER]` is vanilla's). Without, always clear.
    pub fn weathers(self, list: &[&Weather]) -> Self {
        self.set(&crate::comps::dimension::WEATHERS, list.iter().map(|w| w.name().to_string()).collect())
    }
    pub fn no_sky_light(self) -> Self {
        self.tag(abi::comp::dimension::NO_SKY_LIGHT)
    }
    /// Blocks per second squared.
    pub fn gravity(self, g: f32) -> Self {
        self.set(&crate::comps::dimension::GRAVITY, g)
    }
    pub fn day_length(self, ticks: u32) -> Self {
        self.set(&crate::comps::dimension::DAY_LENGTH, ticks)
    }
    /// Daylight through the day as points (turn of the sky, level 0..1): the turn 0 is sunrise,
    /// 0.25 noon, 0.5 sunset, 0.75 midnight, with straight lines between. Endless dusk is
    /// `&[(0.0, 0.4)]`, a long polar day `&[(0.0, 1.0), (0.6, 1.0), (0.7, 0.0), (0.95, 0.0)]`.
    /// Spawning, growth, burning in the sun and the sky all follow it.
    pub fn daylight(self, points: &[(f32, f32)]) -> Self {
        let points = points
            .iter()
            .map(|&(turn, level)| abi::DaylightPoint { turn, level })
            .collect();
        self.set(&crate::comps::dimension::DAYLIGHT, points)
    }
    pub fn fixed_time(self, time: u32) -> Self {
        self.set(&crate::comps::dimension::DAY_LENGTH, 0)
            .set(&crate::comps::dimension::FIXED_TIME, time)
    }
    pub fn sky_color(self, sky: u32, fog: u32) -> Self {
        self.set(&crate::comps::dimension::SKY_COLOR, sky)
            .set(&crate::comps::dimension::FOG_COLOR, fog)
    }
    pub fn ambient_light(self, level: u32) -> Self {
        self.set(&crate::comps::dimension::AMBIENT, level)
    }
}

/// The same limits on everything placed in the world (`comps::common`): ores, structures,
/// scatters, decorations (dimensions only) and natural spawns.
pub trait Placed: WithComponents
where
    kind::Placement: kind::On<Self::Kind>,
{
    /// Only in this dimension (call again for more). Without, in every one but bare ones.
    fn dimension(self, dimension: &Dimension) -> Self {
        self.push(&crate::comps::common::DIMENSIONS, dimension.name().to_string())
    }
    /// Only where the biome has this tag (underground ones too: `#cave`).
    fn biome(self, tag: &str) -> Self {
        self.set_text(&crate::comps::common::BIOME, tag)
    }
    /// Heights, inclusive: where an ore's veins start, a scatter's site, a structure's ground,
    /// a spawn spot.
    fn y(self, min: i32, max: i32) -> Self {
        self.set(&crate::comps::common::Y, [min, max])
    }
}

impl<B: WithComponents> Placed for B where kind::Placement: kind::On<B::Kind> {}

/// A template placed now and then while generating (see `abi::comp::structure`):
///
/// ```ignore
/// r.template(&WELL).layer(&["ccc", "cwc", "ccc"]).key('c', "cobblestone").key('w', "water");
/// r.structure(&WELL).dimension(&OVERWORLD).spacing(64).chance(0.3).biome("#animals").buried(1);
/// ```
pub struct StructureBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl StructureBuilder<'_> {
    /// One chance per `blocks` x `blocks` cell (32 unless said).
    pub fn spacing(self, blocks: u32) -> Self {
        self.set(&crate::comps::structure::SPACING, blocks)
    }
    /// 0.5 unless said.
    pub fn chance(self, c: f32) -> Self {
        self.set(&crate::comps::structure::CHANCE, c)
    }
    /// How far into the ground its bottom layer goes.
    pub fn buried(self, blocks: i32) -> Self {
        self.set(&crate::comps::structure::BURIED, blocks)
    }
}

/// Layers from the bottom up, each a list of rows along z, each character one block along x:
/// a key says which block it means, a space leaves what is there, `air` clears. An edit adds its
/// blocks to the template's.
pub struct TemplateBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    layers: Vec<Vec<String>>,
    keys: Vec<(char, String)>,
    fills: Vec<(String, BlockPos, BlockPos)>,
}

crate::entry::entry_builder!(
    Ore => OreBuilder, ore_entry;
    Structure => StructureBuilder, structure_entry;
    Template => TemplateBuilder, template, finish;
    Liquid => LiquidBuilder, liquid_entry, finish;
    Reaction => ReactionBuilder, reaction_entry;
);

impl TemplateBuilder<'_> {
    pub fn layer(mut self, rows: &[&str]) -> Self {
        self.layers
            .push(rows.iter().map(|r| r.to_string()).collect());
        self
    }
    /// `block` may name a state (`furnace[lit=true]`) or `air` to clear.
    pub fn key(mut self, c: char, block: &str) -> Self {
        self.keys.push((c, qualify_block(self.reg.ns, block)));
        self
    }
    /// A box of `block` from `from` to `to`, inclusive (a moving body's deck).
    pub fn fill(mut self, block: &str, from: impl Into<BlockPos>, to: impl Into<BlockPos>) -> Self {
        let from = from.into();
        self.fills
            .push((qualify_block(self.reg.ns, block), from, to.into()));
        self
    }

    /// The layers and boxes as blocks: the whole list of a new template, more for an edit.
    fn finish(&mut self) {
        let mut blocks: Vec<abi::TemplateBlock> = Vec::new();
        for (y, layer) in self.layers.iter().enumerate() {
            for (z, row) in layer.iter().enumerate() {
                for (x, ch) in row.chars().enumerate() {
                    if let Some((_, b)) = self.keys.iter().find(|k| k.0 == ch) {
                        blocks.push(abi::TemplateBlock { pos: [x as i32, y as i32, z as i32], block: b.clone() });
                    }
                }
            }
        }
        for (b, f, t) in &self.fills {
            for y in f.y.min(t.y)..=f.y.max(t.y) {
                for z in f.z.min(t.z)..=f.z.max(t.z) {
                    for x in f.x.min(t.x)..=f.x.max(t.x) {
                        blocks.push(abi::TemplateBlock { pos: [x, y, z], block: b.clone() });
                    }
                }
            }
        }
        let list = &crate::comps::template::BLOCKS;
        if self.e.mode == abi::entry_mode::EDIT {
            let ops = blocks
                .iter()
                .map(|b| element(list, b, abi::value_op::PUSH, ""));
            self.e.ops.extend(ops);
        } else {
            self.e.ops.push(list.raw_value(&blocks));
        }
    }
}

fn qualify_block(ns: &str, block: &str) -> String {
    if block == "air" { "core:air".to_string() } else { qualify(ns, block) }
}

/// Heights ore veins start at (`OreBuilder::height`).
#[derive(Clone, Copy, Debug)]
pub struct Height {
    min: i32,
    max: i32,
    curve: u32,
    plateau: i32,
}

impl Height {
    pub fn uniform(min: i32, max: i32) -> Height {
        Height { min, max, curve: abi::height_curve::UNIFORM, plateau: 0 }
    }
    /// Most in the middle, fewer towards the ends, none past them.
    pub fn triangle(min: i32, max: i32) -> Height {
        Height { min, max, curve: abi::height_curve::TRIANGLE, plateau: 0 }
    }
    /// At its most over `plateau` blocks in the middle, then falling off.
    pub fn trapezoid(min: i32, max: i32, plateau: i32) -> Height {
        Height { min, max, curve: abi::height_curve::TRAPEZOID, plateau }
    }
}

/// What an ore vein looks like (`OreBuilder::shape`). Every shape stays within 16 blocks of
/// where it starts.
#[derive(Clone, Copy, Debug)]
pub struct Vein {
    kind: u32,
    radius: f32,
    thickness: f32,
    blocks: u32,
}

impl Vein {
    /// A random walk of `blocks` steps, the default.
    pub fn walk(blocks: u32) -> Vein {
        Vein { kind: abi::vein_shape::WALK, radius: 0.0, thickness: 0.0, blocks }
    }
    /// A lump of about `blocks`, like modern vanilla's.
    pub fn blob(blocks: u32) -> Vein {
        Vein { kind: abi::vein_shape::BLOB, radius: 0.0, thickness: 0.0, blocks }
    }
    pub fn sphere(radius: f32) -> Vein {
        Vein { kind: abi::vein_shape::SPHERE, radius, thickness: 0.0, blocks: 0 }
    }
    /// Flat and round (clay, gravel and sand under water).
    pub fn disk(radius: f32, thickness: f32) -> Vein {
        Vein { kind: abi::vein_shape::DISK, radius, thickness, blocks: 0 }
    }
}

/// An ore being registered or changed (`r.edit(IRON_ORE.ore())`: what it sets replaces the ore's,
/// what it `replaces` adds to its).
pub struct OreBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl OreBuilder<'_> {
    /// How the veins spread over heights: `Height::triangle(-24, 56)` puts most in the middle,
    /// like modern vanilla's iron.
    pub fn height(self, h: Height) -> Self {
        self.y(h.min, h.max)
            .set(&crate::comps::ore::HEIGHT, abi::OreHeight { curve: h.curve, plateau: h.plateau })
    }
    /// What a vein looks like (a random walk of `size` blocks unless this says otherwise).
    pub fn shape(self, v: Vein) -> Self {
        let b = self.set(&crate::comps::ore::SHAPE, abi::VeinShape { kind: v.kind, radius: v.radius, thickness: v.thickness });
        if v.blocks > 0 { b.vein_size(v.blocks) } else { b }
    }
    /// Veins started per cube (4 unless said).
    pub fn veins(self, n: u32) -> Self {
        self.set(&crate::comps::ore::VEINS, n)
    }
    /// Blocks per vein (8 unless said).
    pub fn vein_size(self, n: u32) -> Self {
        self.set(&crate::comps::ore::SIZE, n)
    }
    /// Blocks or `#tags` the ore may replace, space separated; again for more.
    pub fn replaces(mut self, blocks: &str) -> Self {
        for b in blocks.split_whitespace() {
            let name = qualify(self.reg.ns, b);
            self = self.push(&crate::comps::ore::REPLACES, name);
        }
        self
    }
    /// Where the vein meets `blocks` (names or `#tags`), `ore` goes there instead: one vein
    /// through stone and granite, iron ore in one and granite iron ore in the other.
    pub fn replaces_with(mut self, blocks: &str, ore: &Block) -> Self {
        for b in blocks.split_whitespace() {
            let variant = abi::OreVariant { name: qualify(self.reg.ns, b), ore: ore.name().to_string() };
            self = self.push(&crate::comps::ore::REPLACES_WITH, variant);
        }
        self
    }
    /// Share of its blocks left out where they would touch air, as vanilla's in caves (0.5 for
    /// most of them).
    pub fn exposed(self, share: f32) -> Self {
        self.set(&crate::comps::ore::EXPOSED, share)
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
    pub fn cube(&self) -> BlockPos {
        self.args.cube
    }
    pub fn origin(&self) -> BlockPos {
        let c = self.args.cube;
        [c.x * 16, c.y * 16, c.z * 16].into()
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
    flags: u32,
    inputs: u32,
    head: &'static str,
}

impl SignalBuild {
    fn apply(&self, p: &Props, d: &mut StateDef) {
        let dir = |name: &str| crate::Dir::from_name(p.get(name));
        let front = dir("front").or(dir("facing"));
        let mut s = abi::SignalDesc {
            kind: self.kind,
            flags: self.flags,
            inputs: self.inputs,
            head: Str::new(self.head),
            ..abi::SignalDesc::DEFAULT
        };
        if let Some(a) = dir("attach") {
            s.attach = a.face();
            if self.attaches {
                let _ = State { def: d }.needs(a, "");
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
            s.flags |= abi::signal_flags::SUBTRACT;
        }
        d.signal = s;
        if self.kind == abi::signal_kind::WIRE {
            let mask = p.int("faces");
            // A thin plate on each face it lies on.
            const PLATES: [([f32; 3], [f32; 3]); 6] = [
                ([0.0, 0.0, 0.0], [1.0, 16.0, 16.0]),
                ([15.0, 0.0, 0.0], [16.0, 16.0, 16.0]),
                ([0.0, 0.0, 0.0], [16.0, 1.0, 16.0]),
                ([0.0, 15.0, 0.0], [16.0, 16.0, 16.0]),
                ([0.0, 0.0, 0.0], [16.0, 16.0, 1.0]),
                ([0.0, 0.0, 15.0], [16.0, 16.0, 16.0]),
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
    pub fn pos(&self) -> BlockPos {
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
    pub fn level_before(&self) -> u8 {
        self.args.before as u8
    }
    /// It just got power.
    pub fn rising(&self) -> bool {
        self.args.before == 0 && self.args.level > 0
    }
}

/// A gate's faces, from where it looks (`Gate::inputs`, `GateInputs::side`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Front = 0,
    Rear = 1,
    Left = 2,
    Right = 3,
    /// Away from what it is on.
    Top = 4,
    Bottom = 5,
}

/// Which faces of a `BlockBuilder::signal_gate` read power, and its delay.
///
/// ```ignore
/// // a repeater-like AND of the sides, two redstone ticks late
/// .signal_gate(Gate::new().inputs(&[Side::Left, Side::Right]).delay(2), |g| if g.side(Side::Left) > 0 && g.side(Side::Right) > 0 { 15 } else { 0 })
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    inputs: u32,
    delay: u32,
}

impl Gate {
    /// Reads behind it and both sides, one redstone tick late.
    pub fn new() -> Gate {
        Gate { inputs: 1 << abi::gate_side::REAR | 1 << abi::gate_side::LEFT | 1 << abi::gate_side::RIGHT, delay: 1 }
    }
    /// The faces it reads. The front is its output, never an input.
    pub fn inputs(mut self, sides: &[Side]) -> Gate {
        self.inputs = sides
            .iter()
            .filter(|s| **s != Side::Front)
            .fold(0, |m, s| m | 1 << *s as u32);
        self
    }
    /// In redstone ticks, two game ticks each.
    pub fn delay(mut self, ticks: u32) -> Gate {
        self.delay = ticks;
        self
    }
}

impl Default for Gate {
    fn default() -> Gate {
        Gate::new()
    }
}

/// What comes in at a `BlockBuilder::signal_gate`'s faces.
pub struct GateInputs {
    args: abi::GateArgs,
}

impl GateInputs {
    fn wrap(a: &mut abi::GateArgs) -> &mut GateInputs {
        unsafe { &mut *(a as *mut abi::GateArgs as *mut GateInputs) }
    }
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    /// Power coming in at `side`, 0 for a face it doesn't read.
    pub fn side(&self, side: Side) -> u8 {
        self.args.inputs[side as usize] as u8
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
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    pub fn state(&self) -> u32 {
        self.args.state
    }
    /// The neighbor that changed, for `on_neighbor`.
    pub fn neighbor(&self) -> BlockPos {
        self.args.other
    }
    /// Where the block is on the body, for `on_carried`.
    pub fn body_pos(&self) -> BlockPos {
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
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    pub fn state(&self) -> u32 {
        self.args.state
    }
    /// Face of the block that was clicked.
    pub fn face(&self) -> crate::Dir {
        crate::Dir::from_face(self.args.face)
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
    pub fn pos(&self) -> BlockPos {
        self.args.pos
    }
    /// Face of the block that was clicked.
    pub fn face(&self) -> crate::Dir {
        crate::Dir::from_face(self.args.face)
    }
    pub fn look_dir(&self) -> crate::Vec3 {
        vec3(self.args.look)
    }
    /// Just outside where it was clicked, 0..1 from this cell's corner: for a slab, `hit().y > 0.5`
    /// is the upper half, on a floor, under a ceiling, against a wall or on another slab alike.
    pub fn hit(&self) -> crate::Vec3 {
        vec3(self.args.hit)
    }
    /// In `merge`: a property of how its item would go.
    pub fn placed(&self, prop: &str) -> Option<&'static str> {
        crate::world::state_prop(self.args.placed, prop)
    }
    /// Which way around the player looks.
    pub fn look_facing(&self) -> crate::Dir {
        crate::Dir::nearest(self.args.look, crate::Facings::HORIZONTAL)
    }
}

fn vec3(v: [f32; 3]) -> crate::Vec3 {
    crate::Vec3 { x: v[0] as f64, y: v[1] as f64, z: v[2] as f64 }
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
    pub fn target_block(&self) -> Option<(BlockPos, u32)> {
        (self.args.has_target != 0).then_some((self.args.pos, self.args.face))
    }
    pub fn stack(&self) -> crate::ItemStack {
        self.args.stack
    }
    /// The slot of the player's inventory the stack is in.
    pub fn slot(&self) -> u32 {
        self.args.slot
    }
    /// The used stack as a holder: what a backpack opens its menu on.
    pub fn holder(&self) -> crate::Holder {
        crate::Holder::held(self.args.player, self.args.slot)
    }
    /// Replaces the held stack (use count 0 to consume it entirely).
    pub fn set_stack(&mut self, s: crate::ItemStack) {
        self.args.stack = s;
    }
}

/// A liquid: its block (16 states) and flow rules, optionally a filled bucket item. Its own
/// components are its flow (`comps::liquid`); its block's (tags, particles) go in `.block(..)`,
/// and the block's tags are its item's too.
///
/// ```ignore
/// r.liquid(&OIL).block(|b| b.light_opacity(4).tag("oil")).flow_delay(15).drop_off(2).bucket(&OIL_BUCKET, &BUCKET, &TOOLS);
/// r.liquid_reaction("#hot", "#water").source_turns_into("obsidian").flowing_turns_into("cobblestone").below_turns_into("stone");
/// ```
pub struct LiquidBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
    block: &'static Block,
    look: Option<BlockLook>,
    bucket: Option<(&'static Item, String, String)>,
}

impl LiquidBuilder<'_> {
    /// Its block, on top of what makes it a liquid (texture `<name>_still`, drawn solid like
    /// lava): `.block(|b| b.tint(&tints::WATER).translucent().drag(0.4).light(15).tag("water"))`.
    /// In an edit, what it changes of the block (and the tags of its item).
    pub fn block(mut self, f: impl for<'r> FnOnce(BlockBuilder<'r>) -> BlockBuilder<'r> + 'static) -> Self {
        self.look = Some(Box::new(f));
        self
    }
    /// Ticks between flow steps (water 5, lava 30).
    pub fn flow_delay(self, ticks: u32) -> Self {
        self.set(&crate::comps::liquid::DELAY, ticks)
    }
    /// Level lost per block spread (water 1: 7 blocks, lava 2: 3 blocks).
    pub fn drop_off(self, n: u32) -> Self {
        self.set(&crate::comps::liquid::DROP_OFF, n)
    }
    /// How far it looks for a way down (water 4, lava 2).
    pub fn slope_find(self, n: u32) -> Self {
        self.set(&crate::comps::liquid::SLOPE_FIND, n)
    }
    /// New sources form where at least `sources` sources are beside a spot over solid ground
    /// (vanilla water 2). 0: always, one bucket fills everything up to its level; more than the
    /// sides there are: never, as without this call.
    pub fn renewable(self, sources: u32) -> Self {
        self.set(&crate::comps::liquid::RENEWABLE, sources)
    }
    /// Rises instead of falling.
    pub fn flows_up(self) -> Self {
        self.set(&crate::comps::liquid::DENSITY, -1000)
    }
    /// Roughly Kelvin; 1000 and more burns what touches it.
    pub fn heat(self, t: u32) -> Self {
        self.set(&crate::comps::liquid::HEAT, t)
    }
    /// The fog seen from inside: its color (0xRRGGBB) and how many blocks away it hides
    /// everything. By default the average color of its texture, 96 blocks.
    pub fn fog(self, rgb: u32, blocks: f32) -> Self {
        self.set(&crate::comps::liquid::FOG, abi::LiquidFog { color: rgb, distance: blocks })
    }
    /// The picture over the screen from inside, relative names are under `block/`, and how much
    /// of it shows. By default its own texture at 0.1; for an inside that looks unlike the surface, set
    /// this and `fog` apart from its texture.
    pub fn overlay(self, texture: &str, opacity: f32) -> Self {
        let texture = crate::asset("block", texture);
        self.set(&crate::comps::liquid::OVERLAY, abi::LiquidOverlay { texture, opacity })
    }
    /// Nothing over the screen from inside.
    pub fn no_overlay(self) -> Self {
        self.set(&crate::comps::liquid::OVERLAY, abi::LiquidOverlay { texture: String::new(), opacity: 0.0 })
    }
    /// Also registers `item`: `empty` (a container item, any mod's) filled with this liquid, in
    /// creative tab `tab` (any mod's). The engine fills and pours it; no code needed.
    pub fn bucket(mut self, item: &'static Item, empty: &Item, tab: &CreativeTab) -> Self {
        self.bucket = Some((item, empty.name().to_string(), tab_tag(tab)));
        self
    }

    /// Its block and item: registered with a new liquid, edited where `.block(..)` says so.
    fn finish(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let name = self.e.name.clone();
        let new = self.e.mode == abi::entry_mode::NEW;
        if new || self.look.is_some() {
            let look = self.look.take().unwrap_or_else(|| Box::new(|b| b));
            // The engine makes its 16 states liquid (flags, look, level) from its entry.
            let b = if new {
                let tex = format!("{}_still", self.block.path());
                look(
                    self.reg
                        .block(self.block)
                        .texture(&tex)
                        .prop("level", 0..=15)
                        .unbreakable()
                        .light_opacity(2)
                        .no_item()
                        .drops_nothing(),
                )
            } else {
                look(self.reg.edit(self.block))
            };
            let texture = new.then(|| b.states[0].textures[0].clone());
            // The same tags on its item: `#water` in a recipe is the liquid too.
            let tags: Vec<Pending> = b
                .components
                .iter()
                .filter(|p| p.is_tag())
                .cloned()
                .collect();
            drop(b);
            let item = Item::generated(name.clone());
            let mut it = match texture {
                Some(texture) => self
                    .reg
                    .item(item)
                    .set_text(&c::FLUID, &name)
                    .max_stack(u32::MAX)
                    .set_text(&crate::comps::item::TEXTURE, &texture),
                None => self.reg.edit(item),
            };
            it.e.ops.extend(tags);
        }
        if let Some((item, empty, tab)) = self.bucket.take() {
            self.reg
                .item(item)
                .max_stack(1)
                .set_text(&c::LIQUID, &name)
                .set_text(&c::REMAINDER, &empty)
                .tag(&tab);
        }
    }
}

/// What happens where two liquids meet, named `<mod>:<liquid>><touching>` (`base:hot>water`)
/// unless `named`.
pub struct ReactionBuilder<'a> {
    reg: &'a mut Registry,
    e: crate::entry::Entry,
}

impl ReactionBuilder<'_> {
    /// Its name instead of `<mod>:<liquid>><touching>`.
    pub fn named(mut self, name: &str) -> Self {
        let derived = std::mem::replace(&mut self.e.name, qualify(self.reg.ns, name));
        self.reg.derived.retain(|n| *n != derived);
        self
    }
    /// What a source of the liquid becomes.
    pub fn source_turns_into(self, block: &str) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::reaction::SOURCE, &block)
    }
    /// What flowing liquid becomes.
    pub fn flowing_turns_into(self, block: &str) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::reaction::FLOWING, &block)
    }
    /// What the touched liquid becomes when this one flows down into it.
    pub fn below_turns_into(self, block: &str) -> Self {
        let block = qualify(self.reg.ns, block);
        self.set_text(&crate::comps::reaction::FALLS_INTO, &block)
    }
}

impl Registry {
    /// A liquid the engine flows; see `LiquidBuilder`. Defaults are vanilla water's numbers, an
    /// average of its texture for the fog's color.
    pub fn liquid(&mut self, liquid: &'static Block) -> LiquidBuilder<'_> {
        LiquidBuilder { e: crate::entry::Entry::of(liquid.liquid()), reg: self, block: liquid, look: None, bucket: None }
    }

    fn liquid_entry(&mut self, liquid: &'static Liquid) -> LiquidBuilder<'_> {
        let block = Block::generated(liquid.name().to_string());
        LiquidBuilder { e: crate::entry::Entry::of(liquid), reg: self, block, look: None, bucket: None }
    }

    /// When `liquid` touches `touching` (names or `#tags` of liquid blocks), see `ReactionBuilder`.
    pub fn liquid_reaction(&mut self, liquid: &str, touching: &str) -> ReactionBuilder<'_> {
        let (liquid, touching) = (qualify(self.ns, liquid), qualify(self.ns, touching));
        let path = |n: &str| {
            let n = n.trim_start_matches('#');
            n.split_once(':').map_or(n, |x| x.1).to_string()
        };
        let name = self.derive_name(&format!("{}>{}", path(&liquid), path(&touching)));
        ReactionBuilder { reg: self, e: crate::entry::Entry::new(abi::entry_kind::REACTION, name) }
            .set_text(&crate::comps::reaction::LIQUID, &liquid)
            .set_text(&crate::comps::reaction::TOUCHING, &touching)
    }

    fn reaction_entry(&mut self, reaction: &'static Reaction) -> ReactionBuilder<'_> {
        ReactionBuilder { reg: self, e: crate::entry::Entry::of(reaction) }
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
        f(State { def: &mut d });
        d
    }

    #[test]
    fn what_comes_after_keeps_what_came_before() {
        let d = built(|s| s.holds_liquid().plant());
        assert_ne!(d.flags & bf::HOLDS_LIQUID, 0);
        assert_eq!(d.flags & bf::SOLID, 0);
        assert_eq!(built(|s| s.light_opacity(2).translucent()).opacity, Some(2));
        assert_eq!(built(|s| s.translucent().light_opacity(2)).opacity, Some(2));
        assert_eq!(built(|s| s.translucent().model_box([0.0; 3], [16.0; 3])).layer, RenderLayer::Translucent);
    }

    #[test]
    fn a_frame_turns_boxes_and_hitboxes() {
        let f = crate::signal::Orientation::new(crate::Dir::Down, crate::Dir::South);
        let d = built(|s| {
            s.oriented(f)
                .model_box([0.0; 3], [16.0, 16.0, 1.0])
                .hitbox([0.0; 3], [16.0, 16.0, 1.0])
        });
        // Its north end comes to the south.
        assert_eq!((d.boxes[0].min, d.boxes[0].max), ([0.0, 0.0, 15.0], [16.0, 16.0, 16.0]));
        assert_eq!(d.hitbox[0], ([0.0, 0.0, 15.0], [16.0, 16.0, 16.0]));
    }
}
