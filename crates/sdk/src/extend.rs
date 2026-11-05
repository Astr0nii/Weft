//! Hooking into what happens without replacing anything:
//!
//! ```ignore
//! // Notifications after the fact, batched once per tick:
//! pub static EATEN: Event<Eaten> = Event::named("eaten");
//! r.on_event(&EATEN, |b| for e in b.iter() { b.world().damage(..) });
//! w.emit(&EATEN, Eaten { .. });
//! // May it happen? Any deny wins:
//! r.before_event(&actions::BREAK_BLOCK, |q| if q.pos()[1] < 0 { Verdict::Deny } else { Verdict::Abstain });
//! // Computed values in stages, explainable:
//! r.modify(&chain::DAMAGE, stage::MULTIPLY, |m| m.value *= 0.5);
//! // Loot tables any mod can change:
//! r.loot(&OAK_LEAVES).pool(Pool::new().when(Cond::Chance(0.05)).item("sapling"));
//! r.loot_edit(&ZOMBIE).add_pool(Pool::new().item("mymod:brain"));
//! ```

use crate::abi::BlockPos;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::abi::{self, ItemStack, Slice, Str};
use crate::component::Value;
use crate::{Attribute, Chain, LootSource, Question, Registry, World, callback, host, qualify};

pub use abi::{
    BlockChange, ChatMessage, Death, DimensionChange, Hurt, Interaction, ItemUsed, Pickup, PlayerEvent, attr_op, permission, stage,
};

/// The engine's questions with no event after them (`Registry::intercept`, `World::ask`); the
/// others are `actions`.
pub mod question {
    use crate::Question;
    /// `entity` attacking `other` with `item`.
    pub static ATTACK: Question = Question::named(crate::abi::question::ATTACK);
    /// Player `entity` turning on a camera mode that asks (`camera_flags::ASK`), which one
    /// `Ask::is_camera` tells. `Allow` lets it where the game mode wouldn't.
    pub static CAMERA: Question = Question::named(crate::abi::action::CAMERA);
    /// Cubes kept loaded (`World::keep_loaded`) from block `pos`, `amount` the mode
    /// (`force_load::*`), `other` the ticket's key: deny one in someone else's claim.
    pub static FORCE_LOAD: Question = Question::named(crate::abi::action::FORCE_LOAD);
}

/// The engine's value chains (`Registry::modify`, `World::chain_compute`).
pub mod chain {
    use crate::Chain;
    /// Damage an entity takes (`entity` hurt by `other`, `cause`), after `on_hurt` hooks.
    pub static DAMAGE: Chain = Chain::named(crate::abi::chain::DAMAGE);
    /// Melee damage of `entity` hitting `other` holding `item`, from the item's `attack_damage`.
    pub static ATTACK_DAMAGE: Chain = Chain::named(crate::abi::chain::ATTACK_DAMAGE);
}

/// Attributes the engine reads.
pub mod attribute {
    use crate::Attribute;
    pub static MAX_HEALTH: Attribute = Attribute::named(crate::abi::attribute::MAX_HEALTH);
    /// Multiplier on walking speed (1 = the kind's speed).
    pub static MOVEMENT_SPEED: Attribute = Attribute::named(crate::abi::attribute::MOVEMENT_SPEED);
    /// Multiplier on how far mobs notice it (1; 0 in creative).
    pub static VISIBILITY: Attribute = Attribute::named(crate::abi::attribute::VISIBILITY);
    /// Multiplier on jump speed, players and mobs (1).
    pub static JUMP_STRENGTH: Attribute = Attribute::named(crate::abi::attribute::JUMP_STRENGTH);
    /// Multiplier on how fast a swing recharges (1).
    pub static ATTACK_SPEED: Attribute = Attribute::named(crate::abi::attribute::ATTACK_SPEED);
    /// Below 1: can't sprint (1).
    pub static CAN_SPRINT: Attribute = Attribute::named(crate::abi::attribute::CAN_SPRINT);
    /// How far a player reaches blocks (4.5; 5 in creative).
    pub static BLOCK_REACH: Attribute = Attribute::named(crate::abi::attribute::BLOCK_REACH);
    /// How far a player reaches entities (3; 5 in creative).
    pub static ENTITY_REACH: Attribute = Attribute::named(crate::abi::attribute::ENTITY_REACH);
    /// Multiplier on how fast a player mines (1).
    pub static MINING_SPEED: Attribute = Attribute::named(crate::abi::attribute::MINING_SPEED);
    /// Multiplier on a player's field of view (1): a zoom, a speed rush.
    pub static FOV: Attribute = Attribute::named(crate::abi::attribute::FOV);
}

const UNRESOLVED: u32 = u32::MAX - 1;

/// A typed event, named like any other thing a mod owns: `Event::named("eaten")` is this mod's,
/// `Event::named("base:eaten")` another's.
pub struct Event<T: Value> {
    name: &'static str,
    full: std::sync::OnceLock<String>,
    id: AtomicU32,
    _p: PhantomData<fn() -> T>,
}

impl<T: Value> Event<T> {
    pub const fn named(name: &'static str) -> Self {
        Event { name, full: std::sync::OnceLock::new(), id: AtomicU32::new(UNRESOLVED), _p: PhantomData }
    }
    pub fn name(&self) -> &str {
        self.full.get_or_init(|| crate::full_name(self.name))
    }
    /// `u32::MAX` when nobody listens.
    pub fn id(&self) -> u32 {
        let id = self.id.load(Ordering::Relaxed);
        if id != UNRESOLVED {
            return id;
        }
        if unsafe { (host().component_table)() }.is_null() {
            return u32::MAX;
        }
        let id = unsafe { (host().find_event)(Str::new(self.name()), Str::new(&T::schema())) };
        self.id.store(id, Ordering::Relaxed);
        id
    }
}

macro_rules! schema_values {
    ($($t:ty),*) => {$(
        unsafe impl Value for $t {
            fn schema() -> String {
                <$t as abi::Schema>::SCHEMA.into()
            }
        }
    )*};
}
schema_values!(
    ItemStack,
    abi::ChatMessage,
    abi::BlockChange,
    abi::Death,
    abi::Hurt,
    abi::PlayerEvent,
    abi::Pickup,
    abi::ItemUsed,
    abi::Interaction,
    abi::Mounting,
    abi::DimensionChange,
    abi::WeatherTick
);

/// The engine's events with no question before them; the others are `actions`.
pub static PLAYER_JOINED: Event<abi::PlayerEvent> = Event::named(abi::event::PLAYER_JOINED);
/// A dead player came back at the spawn point, components reset but the `KEEP_ON_RESPAWN` ones;
/// moving them on from here (a bed) happens before anyone saw them there.
pub static PLAYER_RESPAWNED: Event<abi::PlayerEvent> = Event::named(abi::event::PLAYER_RESPAWNED);
pub static ENTITY_HEALED: Event<abi::Hurt> = Event::named(abi::event::ENTITY_HEALED);
/// Every tick in each dimension with weather: which it is.
pub static WEATHER_TICK: Event<abi::WeatherTick> = Event::named(abi::event::WEATHER_TICK);

/// Something that happens, as one name: the question asked before it (`r.before_event(&A, ..)`,
/// any deny wins) and the event after (`r.on_event(&A, ..)`). The engine's are in `actions`; a mod's
/// own goes through `w.act(&WRENCH, &mut query, value)`.
///
/// ```ignore
/// static WRENCH: Action<Wrenched> = Action::named("wrench");
/// r.before_event(&actions::DIE, |a| if has_totem(a) { Verdict::Deny } else { Verdict::Abstain });
/// r.on_event(&actions::BREAK_BLOCK, |b| for c in b.iter() { /* .. */ });
/// ```
pub struct Action<T: Value> {
    ask: Question,
    done: Event<T>,
}

impl<T: Value> Action<T> {
    pub const fn named(name: &'static str) -> Self {
        Action { ask: Question::named(name), done: Event::named(name) }
    }
}

impl<T: Value> AsRef<Question> for Action<T> {
    fn as_ref(&self) -> &Question {
        &self.ask
    }
}

impl<T: Value> AsRef<Event<T>> for Action<T> {
    fn as_ref(&self) -> &Event<T> {
        &self.done
    }
}

impl AsRef<Question> for Question {
    fn as_ref(&self) -> &Question {
        self
    }
}

impl<T: Value> AsRef<Event<T>> for Event<T> {
    fn as_ref(&self) -> &Event<T> {
        self
    }
}

impl World {
    /// Asks `action`'s question with `q`, and when nobody refuses sends its event with `value`.
    /// True if it may happen.
    pub fn act<T: Value>(&self, action: &Action<T>, q: &mut abi::Query, value: T) -> bool {
        let ok = self.ask(action, q);
        if ok {
            self.emit(action, value);
        }
        ok
    }
}

/// The engine's actions (`abi::action`): what may be refused and what is told after, for each.
pub mod actions {
    use super::Action;
    use crate::abi::{self, action as a};
    pub static BREAK_BLOCK: Action<abi::BlockChange> = Action::named(a::BREAK_BLOCK);
    pub static PLACE_BLOCK: Action<abi::BlockChange> = Action::named(a::PLACE_BLOCK);
    pub static USE_BLOCK: Action<abi::BlockChange> = Action::named(a::USE_BLOCK);
    /// Sent when using the item did something (`stack` as it was before): eating, emptying a
    /// bucket. Another mod reacts to food here, by the item's components.
    pub static USE_ITEM: Action<abi::ItemUsed> = Action::named(a::USE_ITEM);
    pub static INTERACT_ENTITY: Action<abi::Interaction> = Action::named(a::INTERACT_ENTITY);
    /// Asked before the damage chain, sent after armor and every modifier.
    pub static HURT: Action<abi::Hurt> = Action::named(a::HURT);
    pub static DIE: Action<abi::Death> = Action::named(a::DIE);
    pub static PICKUP_ITEM: Action<abi::Pickup> = Action::named(a::PICKUP_ITEM);
    pub static ENTER_DIMENSION: Action<abi::DimensionChange> = Action::named(a::ENTER_DIMENSION);
    pub static CHAT: Action<abi::ChatMessage> = Action::named(a::CHAT);
    /// Asked with `entity` the rider, `other` the mount, `amount` the seat.
    pub static MOUNT: Action<abi::Mounting> = Action::named(a::MOUNT);
    pub static DISMOUNT: Action<abi::Mounting> = Action::named(a::DISMOUNT);
}

/// Every value of one event queued during a tick, in the order they happened.
pub struct EventBatch<T> {
    world: World,
    values: Vec<T>,
}

impl<T: Copy> EventBatch<T> {
    pub fn world(&self) -> World {
        self.world
    }
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.values.iter()
    }
    pub fn len(&self) -> usize {
        self.values.len()
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Abstain,
    Allow,
    Deny,
}

/// A question being asked (`Registry::intercept`).
pub struct Ask {
    world: World,
    question: &'static str,
    query: *mut abi::Query,
}

impl Ask {
    pub fn world(&self) -> World {
        self.world
    }
    pub fn question(&self) -> &str {
        self.question
    }
    pub fn query(&self) -> &abi::Query {
        unsafe { &*self.query }
    }
    pub fn entity(&self) -> u64 {
        self.query().entity
    }
    pub fn pos(&self) -> BlockPos {
        self.query().pos
    }
    pub fn state(&self) -> u32 {
        self.query().state
    }
    /// What a player is about to say (`actions::CHAT`).
    pub fn text(&self) -> &str {
        unsafe { self.query().text.as_str() }
    }
    pub fn item(&self) -> u32 {
        self.query().item
    }
    /// Whether `question::CAMERA` asks about `mode`.
    pub fn is_camera(&self, mode: &crate::CameraMode) -> bool {
        self.query().amount == mode.camera_id() as f64
    }
}

/// One step of a value chain (`Registry::modify`): read the context, change `value`.
pub struct Modify {
    world: World,
    query: abi::Query,
    pub base: f64,
    pub value: f64,
}

impl Modify {
    pub fn world(&self) -> World {
        self.world
    }
    pub fn query(&self) -> &abi::Query {
        &self.query
    }
}

/// What a loot roll knows.
#[derive(Clone, Copy, Default)]
pub struct LootCtx {
    pub tool: ItemStack,
    pub killer: u64,
    pub by_player: bool,
    pub on_fire: bool,
    /// The block state mined with `tool`, for `Cond::Harvested` (0 = none).
    pub mined: u32,
}

/// A loot condition.
#[derive(Clone)]
pub enum Cond {
    Chance(f32),
    /// The tool carries the tag (`tools/shears`).
    ToolTag(String),
    NotToolTag(String),
    ToolLevel(u32),
    KilledByPlayer,
    OnFire,
    /// The block was harvested: broken by the world, or mined with a tool its `mined_by` level
    /// accepts. `drops(..)` tables have it; write it on pools of your own tables that should.
    Harvested,
    /// The block's state has this property at this value, written as in `w.prop`:
    /// `BlockState("age=7")` for a ripe crop, `BlockState("lit=true")`.
    BlockState(&'static str),
    /// A condition (`Registry::condition`) holds there, asked about the killer or breaker.
    Condition(&'static crate::Condition),
}

impl Cond {
    pub fn tool_tag(t: &str) -> Cond {
        Cond::ToolTag(t.to_string())
    }
    fn raw(&self) -> (u32, f32, &str) {
        use abi::loot_cond as k;
        match self {
            Cond::Chance(p) => (k::CHANCE, *p, ""),
            Cond::ToolTag(t) => (k::TOOL_TAG, 1.0, t),
            Cond::NotToolTag(t) => (k::NOT_TOOL_TAG, 1.0, t),
            Cond::ToolLevel(l) => (k::TOOL_LEVEL, *l as f32, ""),
            Cond::KilledByPlayer => (k::KILLED_BY_PLAYER, 1.0, ""),
            Cond::OnFire => (k::ON_FIRE, 1.0, ""),
            Cond::BlockState(prop) => (k::BLOCK_STATE, 1.0, *prop),
            Cond::Harvested => (k::HARVESTED, 1.0, ""),
            Cond::Condition(c) => (k::CONDITION, 1.0, c.name()),
        }
    }
}

/// One choice of a pool.
#[derive(Clone)]
pub struct Entry {
    item: String,
    weight: u32,
    min: u32,
    max: u32,
    when: Vec<Cond>,
    smelt: String,
}

impl Entry {
    /// An item name (this mod's namespace by default) or `#tag`.
    pub fn item(name: &str) -> Entry {
        Entry { item: name.to_string(), weight: 1, min: 1, max: 1, when: Vec::new(), smelt: String::new() }
    }
    /// A choice that gives nothing (weight against the others).
    pub fn nothing() -> Entry {
        Entry::item("")
    }
    pub fn count(mut self, min: u32, max: u32) -> Entry {
        (self.min, self.max) = (min, max.max(min));
        self
    }
    pub fn weight(mut self, w: u32) -> Entry {
        self.weight = w;
        self
    }
    pub fn when(mut self, c: Cond) -> Entry {
        self.when.push(c);
        self
    }
    /// A burning victim drops it cooked by this recipe type (`base:smelting`).
    pub fn smelted(mut self, recipe_kind: &crate::RecipeKind) -> Entry {
        self.smelt = recipe_kind.name().to_string();
        self
    }
}

/// Rolls (1 by default) of one weighted choice each.
#[derive(Clone, Default)]
pub struct Pool {
    rolls: (u32, u32),
    when: Vec<Cond>,
    entries: Vec<Entry>,
}

impl Pool {
    pub fn new() -> Pool {
        Pool { rolls: (1, 1), when: Vec::new(), entries: Vec::new() }
    }
    pub fn rolls(mut self, min: u32, max: u32) -> Pool {
        self.rolls = (min, max.max(min));
        self
    }
    pub fn when(mut self, c: Cond) -> Pool {
        self.when.push(c);
        self
    }
    pub fn entry(mut self, e: Entry) -> Pool {
        self.entries.push(e);
        self
    }
    /// One of `name`.
    pub fn item(self, name: &str) -> Pool {
        self.entry(Entry::item(name))
    }
    /// `min..=max` of `name`.
    pub fn items(self, name: &str, min: u32, max: u32) -> Pool {
        self.entry(Entry::item(name).count(min, max))
    }
}

/// The ABI view of pools, kept alive while a descriptor points into it.
pub(crate) struct RawPools {
    conds: Vec<Vec<abi::LootCond>>,
    entries: Vec<Vec<abi::LootEntry>>,
    pub(crate) pools: Vec<abi::LootPool>,
    _names: Vec<String>,
}

pub(crate) fn raw_pools(ns: &str, pools: &[Pool]) -> RawPools {
    let mut names: Vec<String> = Vec::new();
    for p in pools {
        for e in &p.entries {
            names.push(if e.item.is_empty() || e.item.starts_with('#') { e.item.clone() } else { qualify(ns, &e.item) });
        }
    }
    let mut out = RawPools { conds: Vec::new(), entries: Vec::new(), pools: Vec::new(), _names: Vec::new() };
    let mut cond_index = Vec::new();
    for p in pools {
        let pc: Vec<abi::LootCond> = p
            .when
            .iter()
            .map(|c| {
                let (kind, value, text) = c.raw();
                abi::LootCond { kind, value, text: Str::new(text), ..abi::LootCond::DEFAULT }
            })
            .collect();
        cond_index.push(out.conds.len());
        out.conds.push(pc);
        for e in &p.entries {
            out.conds.push(
                e.when
                    .iter()
                    .map(|c| {
                        let (kind, value, text) = c.raw();
                        abi::LootCond { kind, value, text: Str::new(text), ..abi::LootCond::DEFAULT }
                    })
                    .collect(),
            );
        }
    }
    let mut n = 0;
    for (pi, p) in pools.iter().enumerate() {
        let first = cond_index[pi];
        let entries: Vec<abi::LootEntry> = p
            .entries
            .iter()
            .enumerate()
            .map(|(ei, e)| {
                let name = Str::new(&names[n + ei]);
                abi::LootEntry {
                    item: name,
                    weight: e.weight,
                    min: e.min,
                    max: e.max,
                    conditions: Slice::new(&out.conds[first + 1 + ei]),
                    smelt: Str::new(&e.smelt),
                    ..abi::LootEntry::DEFAULT
                }
            })
            .collect();
        n += p.entries.len();
        out.entries.push(entries);
    }
    for (pi, p) in pools.iter().enumerate() {
        out.pools.push(abi::LootPool {
            rolls_min: p.rolls.0,
            rolls_max: p.rolls.1,
            conditions: Slice::new(&out.conds[cond_index[pi]]),
            entries: Slice::new(&out.entries[pi]),
            ..abi::LootPool::DEFAULT
        });
    }
    out._names = names;
    out
}

/// A loot table being registered; sent when dropped.
pub struct LootBuilder<'a> {
    reg: &'a mut Registry,
    name: String,
    pools: Vec<Pool>,
}

impl LootBuilder<'_> {
    pub fn pool(mut self, p: Pool) -> Self {
        self.pools.push(p);
        self
    }
}

impl Drop for LootBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let raw = raw_pools(self.reg.ns, &self.pools);
        let desc = abi::LootDesc { name: Str::new(&self.name), pools: Slice::new(&raw.pools), ..abi::LootDesc::DEFAULT };
        unsafe { (host().reg_loot)(self.reg.raw, &desc) }
    }
}

/// Changes to tables of any mod (`table` may end in `*`).
pub struct LootModBuilder<'a> {
    reg: &'a mut Registry,
    table: String,
}

impl LootModBuilder<'_> {
    fn send(&mut self, op: u32, pool: Option<&Pool>, from: &str, to: &str) {
        let pools: Vec<Pool> = pool.into_iter().cloned().collect();
        let raw = raw_pools(self.reg.ns, &pools);
        let (from, to) = (
            if from.is_empty() { String::new() } else { qualify(self.reg.ns, from) },
            if to.is_empty() { String::new() } else { qualify(self.reg.ns, to) },
        );
        let desc = abi::LootEditDesc {
            table: Str::new(&self.table),
            op,
            pool: raw.pools.first().copied().unwrap_or(abi::LootPool::DEFAULT),
            from: Str::new(&from),
            to: Str::new(&to),
            ..abi::LootEditDesc::DEFAULT
        };
        unsafe { (host().reg_loot_edit)(self.reg.raw, &desc) }
    }
    pub fn add_pool(mut self, p: Pool) -> Self {
        self.send(abi::loot_op::ADD_POOL, Some(&p), "", "");
        self
    }
    pub fn replace_item(mut self, from: &str, to: &str) -> Self {
        self.send(abi::loot_op::REPLACE_ITEM, None, from, to);
        self
    }
    pub fn remove_item(mut self, item: &str) -> Self {
        self.send(abi::loot_op::REMOVE_ITEM, None, item, "");
        self
    }
    pub fn clear(mut self) -> Self {
        self.send(abi::loot_op::CLEAR, None, "", "");
        self
    }
}

/// Every entity of a region that has a component, with the values side by side, and the ticks
/// each call stands for: 1 normally, more while an entity catches up on time it missed (its
/// region was unloaded, the world's time jumped). A system worth catching up does the work of
/// `ticks` ticks at once (a furnace: so many items smelted), anything else may ignore it.
pub struct SystemBatch<'a, T> {
    world: World,
    ids: &'a [u64],
    values: &'a mut [T],
    ticks: &'a [u32],
}

impl<T> SystemBatch<'_, T> {
    pub fn world(&self) -> World {
        self.world
    }
    pub fn ids(&self) -> &[u64] {
        self.ids
    }
    pub fn values(&mut self) -> &mut [T] {
        self.values
    }
    pub fn ticks(&self) -> &[u32] {
        self.ticks
    }
    /// (entity, its value, ticks to do).
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (u64, &mut T, u32)> {
        self.ids
            .iter()
            .copied()
            .zip(self.values.iter_mut())
            .zip(self.ticks.iter().copied())
            .map(|((id, v), t)| (id, v, t))
    }
}

impl Registry {
    /// A system over a per-entity component (`Registry::entity_component`): once per tick and
    /// region with all values in one slice, no host call per entity.
    pub fn system<T: Value>(&mut self, comp: &crate::Component<T>, f: impl Fn(&mut SystemBatch<T>) + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::SystemArgs| {
            if a.stride as usize != size_of::<T>() || a.count == 0 {
                return;
            }
            let n = a.count as usize;
            let mut b = SystemBatch {
                world: World::from_raw(a.world),
                ids: unsafe { std::slice::from_raw_parts(a.ids, n) },
                values: unsafe { std::slice::from_raw_parts_mut(a.values as *mut T, n) },
                ticks: unsafe { std::slice::from_raw_parts(a.ticks, n) },
            };
            f(&mut b);
        });
        unsafe { (host().reg_system)(self.raw, Str::new(comp.name()), Str::new(&T::schema()), cb) }
    }

    /// Handles every value of `event` once per tick (and region), in one call.
    pub fn on_event<T: Value>(&mut self, event: &impl AsRef<Event<T>>, f: impl Fn(&EventBatch<T>) + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::EventArgs| {
            let stride = a.stride as usize;
            if stride != size_of::<T>() {
                return;
            }
            let values = (0..a.count as usize)
                .map(|i| unsafe { std::ptr::read_unaligned(a.data.add(i * stride) as *const T) })
                .collect();
            f(&EventBatch { world: World::from_raw(a.world), values });
        });
        unsafe { (host().reg_event_handler)(self.raw, Str::new(event.as_ref().name()), Str::new(&T::schema()), cb) }
    }

    /// Answers `question` (`question::*` or a mod's own) before it happens.
    pub fn before_event(&mut self, question: &impl AsRef<Question>, f: impl Fn(&Ask) -> Verdict + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::InterceptArgs| {
            let ask = Ask {
                world: World::from_raw(a.world),
                question: unsafe { std::mem::transmute::<&str, &'static str>(a.question.as_str()) },
                query: a.query,
            };
            let v = f(&ask);
            unsafe {
                (*a.query).verdict = match v {
                    Verdict::Abstain => abi::verdict::ABSTAIN,
                    Verdict::Allow => abi::verdict::ALLOW,
                    Verdict::Deny => abi::verdict::DENY,
                }
            }
        });
        unsafe { (host().reg_interceptor)(self.raw, Str::new(question.as_ref().name()), cb) }
    }

    /// A step on value `chain` (`chain::*` or a mod's own) at `stage` (`stage::*`).
    pub fn modify(&mut self, chain: &Chain, stage: u32, f: impl Fn(&mut Modify) + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::ModifyArgs| {
            let mut m = Modify { world: World::from_raw(a.world), query: unsafe { *a.ctx }, base: a.base, value: a.value };
            f(&mut m);
            a.value = m.value;
        });
        unsafe { (host().reg_chain_modifier)(self.raw, Str::new(chain.name()), stage, cb) }
    }

    /// An attribute of living entities (`attribute::*` are the engine's).
    pub fn attribute(&mut self, attribute: &Attribute, default: f64, min: f64, max: f64) {
        unsafe { (host().reg_attribute)(self.raw, Str::new(attribute.name()), default, min, max) }
    }

    /// A loot table: a block's or entity kind's is what it drops, a `LootTable` is one of its own.
    pub fn loot<'a>(&'a mut self, table: &dyn LootSource) -> LootBuilder<'a> {
        LootBuilder { reg: self, name: table.table_name().to_string(), pools: Vec::new() }
    }

    pub fn loot_edit<'a>(&'a mut self, table: &dyn LootSource) -> LootModBuilder<'a> {
        LootModBuilder { reg: self, table: table.table_name().to_string() }
    }

    /// Declares a component stacks carry a value of (`stack.with(&COMP, v)`), with its flags
    /// (`abi::stack_flags`: kept on the server, not saved), 0 for neither.
    pub fn stack_component<T: Value>(&mut self, c: &crate::Component<T>, flags: u32) {
        unsafe { (host().reg_stack_component)(self.raw, Str::new(c.name()), Str::new(&T::schema()), flags) }
    }
}

impl World {
    /// In `ticks` ticks `event` fires with `value` (a calm-down, a hunger pulse). The timer is
    /// saved with `entity` and goes with it when it is removed; one per entity and event, so
    /// setting it again moves it.
    pub fn emit_after<T: Value>(&self, entity: u64, ticks: u32, event: &Event<T>, value: T) {
        let id = event.id();
        if id == u32::MAX {
            return;
        }
        let b = unsafe { std::slice::from_raw_parts(&value as *const T as *const u8, size_of::<T>()) };
        unsafe { (host().emit_after)(self.raw(), entity, ticks, id, Slice::new(b)) }
    }

    pub fn emit<T: Value>(&self, event: &impl AsRef<Event<T>>, value: T) {
        let id = event.as_ref().id();
        if id == u32::MAX {
            return;
        }
        let b = unsafe { std::slice::from_raw_parts(&value as *const T as *const u8, size_of::<T>()) };
        unsafe { (host().emit)(self.raw(), id, Slice::new(b)) }
    }

    /// Asks the interceptors of `question`; true if allowed.
    pub fn ask(&self, question: &impl AsRef<Question>, q: &mut abi::Query) -> bool {
        unsafe { (host().ask)(self.raw(), Str::new(question.as_ref().name()), q) != 0 }
    }

    pub fn chain_compute(&self, chain: &Chain, base: f64, q: &abi::Query) -> f64 {
        unsafe { (host().chain_compute)(self.raw(), Str::new(chain.name()), base, q, 0) }
    }

    /// Like `chain_compute`, with the steps that led to the value ("4, base at stage 200: 4 -> 2 = 2").
    pub fn chain_explain(&self, chain: &Chain, base: f64, q: &abi::Query) -> (f64, String) {
        let v = unsafe { (host().chain_compute)(self.raw(), Str::new(chain.name()), base, q, 1) };
        (v, unsafe { (host().explanation)().as_str().to_string() })
    }

    pub fn attribute(&self, entity: u64, attribute: &Attribute) -> f64 {
        unsafe { (host().attribute)(self.raw(), entity, Str::new(attribute.name())) }
    }

    pub fn attribute_explain(&self, entity: u64, attribute: &Attribute) -> (f64, String) {
        let v = unsafe { (host().attribute_explain)(self.raw(), entity, Str::new(attribute.name())) };
        (v, unsafe { (host().explanation)().as_str().to_string() })
    }

    /// Adds or replaces modifier `id` on an attribute for `ticks` (0 = until removed).
    pub fn attribute_add_modifier(&self, entity: u64, attr: &Attribute, id: &str, op: u32, amount: f64, ticks: u64) {
        unsafe { (host().attribute_add_modifier)(self.raw(), entity, Str::new(attr.name()), Str::new(id), op, amount, ticks) }
    }

    pub fn attribute_remove_modifier(&self, entity: u64, attr: &Attribute, id: &str) {
        unsafe { (host().attribute_remove_modifier)(self.raw(), entity, Str::new(attr.name()), Str::new(id)) }
    }

    pub fn roll_loot(&self, table: &dyn LootSource, ctx: &LootCtx) -> Vec<ItemStack> {
        let table = table.table_name();
        let c = abi::LootCtx {
            tool: ctx.tool,
            killer: ctx.killer,
            by_player: ctx.by_player as u32,
            on_fire: ctx.on_fire as u32,
            mined: ctx.mined,
        };
        let mut buf = vec![ItemStack::EMPTY; 16];
        let n = unsafe { (host().roll_loot)(self.raw(), Str::new(table), &c, buf.as_mut_ptr(), buf.len()) };
        if n > buf.len() {
            buf.resize(n, ItemStack::EMPTY);
            unsafe { (host().roll_loot)(self.raw(), Str::new(table), &c, buf.as_mut_ptr(), buf.len()) };
        }
        buf.truncate(n);
        buf
    }

    /// Breaks a block as if mined with `tool` by `player` (0 = nobody), with its loot when
    /// `drops`. False if nothing broke or an interceptor said no.
    pub fn break_block(&self, pos: impl Into<BlockPos>, tool: ItemStack, player: u64, drops: bool) -> bool {
        let pos = pos.into();
        unsafe { (host().break_block)(self.raw(), &pos, tool, player, drops as u32) != 0 }
    }
}

/// A query for `World::ask` and value chains.
pub fn query() -> abi::Query {
    abi::Query::DEFAULT
}
