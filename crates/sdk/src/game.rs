use crate::abi::{self, Holder, ItemStack, Slice, Str};
use crate::component::{Component, Value};
use crate::content::{
    Biome, Block, EntityCategory, EntityKind, EquipmentSlot, GizmoGroup, HudElement, Menu, Pending, RecipeKind, Screen, TintKind,
};
use crate::ui::{Ui, ui_cb};
use crate::{Id, Registry, World, callback, host, qualify};

pub use abi::{EYE_SHARE, Energy, EntityState, component_flags, damage_cause, damage_flags, entity_flags, slot_flags};

/// The engine's player kind, for hooks and components on players.
pub static PLAYER: EntityKind = EntityKind::named("core:player");
/// The engine's kind for dropped stacks (`World::spawn_item`).
pub static ITEM: EntityKind = EntityKind::named("core:item");

/// How a spawned entity starts (`World::spawn_with`); what isn't set is the kind's default.
#[derive(Clone, Copy, Debug)]
pub struct Spawn {
    desc: abi::SpawnEntityDesc,
}

impl Default for Spawn {
    fn default() -> Self {
        Spawn::new()
    }
}

impl Spawn {
    pub fn new() -> Spawn {
        Spawn {
            desc: abi::SpawnEntityDesc {
                kind: 0,
                pos: [0.0; 3],
                set: 0,
                stack: ItemStack::EMPTY,
                velocity: [0.0; 3],
                yaw: 0.0,
                pickup_delay: 0,
            },
        }
    }
    /// What an item entity is.
    pub fn stack(mut self, s: ItemStack) -> Self {
        self.desc.stack = s;
        self
    }
    /// Blocks per second.
    pub fn velocity(mut self, v: [f64; 3]) -> Self {
        self.desc.velocity = v;
        self.desc.set |= abi::spawn_set::VELOCITY;
        self
    }
    /// Radians, like `EntityState::yaw`.
    pub fn yaw(mut self, radians: f32) -> Self {
        self.desc.yaw = radians;
        self.desc.set |= abi::spawn_set::YAW;
        self
    }
    /// Ticks before an item can be picked up (10 by default, 40 for one a player threw).
    pub fn pickup_delay(mut self, ticks: u32) -> Self {
        self.desc.pickup_delay = ticks;
        self.desc.set |= abi::spawn_set::PICKUP_DELAY;
        self
    }
}

// ---------------------------------------------------------------- world helpers

impl World {
    /// Entity `id` plays keyframe animation `name` for everyone who sees it (looping until
    /// another starts); an empty name goes back to idle and walk.
    pub fn action_play(&self, id: u64, name: &str, looping: bool) {
        unsafe { (host().action_play)(self.raw(), id, Str::new(name), looping as u32) }
    }

    /// Stops the action `action_play` started.
    pub fn action_stop(&self, id: u64) {
        self.action_play(id, "", false);
    }

    /// While this holds, entity `id` plays action `to` wherever it would play `from`: a creeper
    /// running from a cat plays its own run instead of `walk`. Everyone who sees it sees it, and
    /// without a clip for `to` it goes on playing `from`.
    pub fn action_override(&self, id: u64, from: &str, to: &str) {
        unsafe { (host().action_override)(self.raw(), id, Str::new(from), Str::new(to)) }
    }

    /// Ends `action_override`.
    pub fn action_override_clear(&self, id: u64, from: &str) {
        unsafe { (host().action_override)(self.raw(), id, Str::new(from), Str::new("")) }
    }

    pub fn light(&self, pos: [i32; 3]) -> u32 {
        unsafe { (host().light)(self.raw(), &pos) }
    }
    /// Biome id at `pos` (`Biome::id`, `Tag::on_biome`); underground biomes differ from the
    /// surface's.
    pub fn biome(&self, pos: [i32; 3]) -> u32 {
        unsafe { (host().biome_at)(self.raw(), &pos) }
    }
    /// Sky light level at `pos` (0..15).
    pub fn sky_light(&self, pos: [i32; 3]) -> u32 {
        abi::light::sky(self.light(pos))
    }
    /// Strongest block light channel at `pos`.
    pub fn block_light(&self, pos: [i32; 3]) -> u32 {
        abi::light::block(self.light(pos))
    }
    pub fn daylight(&self) -> f32 {
        unsafe { (host().daylight)(self.raw()) }
    }
    /// An entity of a kind (a handle, or its id); 0 if there is no such kind.
    pub fn spawn(&self, kind: impl Into<Id<EntityKind>>, pos: [f64; 3]) -> u64 {
        self.spawn_with(kind, pos, &Spawn::new())
    }
    /// `spawn` with what it starts with: `Spawn::new().velocity(v).yaw(1.57)`, and for an item
    /// entity its stack.
    pub fn spawn_with(&self, kind: impl Into<Id<EntityKind>>, pos: [f64; 3], how: &Spawn) -> u64 {
        let desc = abi::SpawnEntityDesc { kind: kind.into().raw(), pos, ..how.desc };
        unsafe { (host().entity_spawn)(self.raw(), &desc) }
    }
    pub fn despawn(&self, id: u64) {
        unsafe { (host().entity_despawn)(self.raw(), id) }
    }
    pub fn entity(&self, id: u64) -> Option<EntityState> {
        let mut s = EntityState::default();
        (unsafe { (host().entity_get)(self.raw(), id, &mut s) } != 0).then_some(s)
    }
    /// Places `template` with its lowest corner at `pos`; false (and nothing placed) when part
    /// of it isn't loaded.
    pub fn place_template(&self, template: &crate::Template, pos: [i32; 3]) -> bool {
        unsafe { (host().place_template)(self.raw(), Str::new(template.name()), &pos) != 0 }
    }
    pub fn set_entity(&self, id: u64, s: &EntityState) {
        unsafe { (host().entity_set)(self.raw(), id, s) }
    }
    /// Adds `v` (blocks per second) to its velocity, players included: a trampoline, a blast.
    pub fn push(&self, id: u64, v: [f64; 3]) {
        unsafe { (host().entity_push)(self.raw(), id, &v) }
    }
    /// The entity's own box; `None` goes back to its kind's. Crouching still shrinks it.
    pub fn set_size(&self, id: u64, size: Option<abi::EntitySize>) {
        unsafe { (host().set_entity_size)(self.raw(), id, size.unwrap_or_default()) }
    }
    /// Draws the entity as another kind (model, texture, animations); `None` as itself. It stays
    /// the same entity with its own behavior.
    pub fn set_appearance(&self, id: u64, kind: Option<&str>) {
        unsafe { (host().set_entity_appearance)(self.raw(), id, Str::new(kind.unwrap_or(""))) }
    }
    /// The box entities of a kind have.
    pub fn kind_size(&self, kind: &str) -> Option<abi::EntitySize> {
        let mut out = abi::EntitySize::default();
        (unsafe { (host().entity_kind_size)(Str::new(kind), &mut out) } != 0).then_some(out)
    }
    /// `id` walks to `target` by itself, around obstacles, at `speed` times its walking speed
    /// (0 stops it). Call it again as the target moves. The path is found later in the tick;
    /// `navigating` turns false when the walk ends (arrived, or stuck for two seconds).
    pub fn navigate(&self, id: u64, target: [f64; 3], speed: f32) -> bool {
        unsafe { (host().navigate)(self.raw(), id, &target, speed) != 0 }
    }
    /// The world's seed, the one its terrain comes from. The same in every dimension.
    pub fn seed(&self) -> u64 {
        unsafe { (host().world_seed)(self.raw()) }
    }
    /// A random number that belongs to this block of this world and never changes: what a chest
    /// should hold when first opened, a villager's trades. Several from one spot:
    /// `noise::hash(w.seed_at(pos), 1, 0, 0)`, and so on. On a body `pos` is in the body's
    /// coordinates, like anything there.
    pub fn seed_at(&self, pos: [i32; 3]) -> u64 {
        crate::noise::hash(self.seed(), pos[0] as i64, pos[1] as i64, pos[2] as i64)
    }
    /// The same for the whole cube (16 blocks each way) `pos` is in.
    pub fn seed_of_cube(&self, pos: [i32; 3]) -> u64 {
        crate::noise::hash(self.seed() ^ 0x5EED_C0BE, (pos[0] >> 4) as i64, (pos[1] >> 4) as i64, (pos[2] >> 4) as i64)
    }
    /// Lowest corner of the nearest structure `name` (full name) within `radius` blocks of
    /// `pos`, generated yet or not.
    pub fn structure_near(&self, name: &str, pos: [i32; 3], radius: i32) -> Option<[i32; 3]> {
        let mut out = [0; 3];
        (unsafe { (host().structure_near)(self.raw(), Str::new(name), &pos, radius, &mut out) } != 0).then_some(out)
    }
    /// The block at `pos` falls as an entity (sand, gravel) and is placed again where it lands:
    /// `body_fall` of a body of that one block.
    pub fn fall_block(&self, pos: [i32; 3]) -> u64 {
        unsafe { (host().fall_block)(self.raw(), &pos) }
    }
    /// Whether `id` is still walking where `navigate` sent it.
    pub fn navigating(&self, id: u64) -> bool {
        unsafe { (host().navigating)(self.raw(), id) != 0 }
    }
    pub fn walk(&self, id: u64, dir: [f32; 2], speed: f32, jump: bool) {
        unsafe { (host().entity_walk)(self.raw(), id, &dir, speed, jump as u32) }
    }
    /// The first block a ray from `from` along `dir` hits within `max` blocks (liquids count if
    /// `liquids`): position, face hit (-X, +X, -Y, +Y, -Z, +Z) and state. A turret's line, a
    /// mob checking whether it sees a player, an arrow's next step.
    pub fn raycast(&self, from: [f64; 3], dir: [f64; 3], max: f64, liquids: bool) -> Option<LookAt> {
        let (mut pos, mut face, mut body) = ([0; 3], 0, 0);
        let s = unsafe { (host().raycast)(self.raw(), &from, &dir, max, liquids as u32, &mut pos, &mut face, &mut body) };
        (s != 0).then(|| LookAt { pos, face, state: s - 1, body })
    }
    /// The block `id` looks at within `max` blocks: `raycast` from its eyes along its look.
    pub fn target_block(&self, id: u64, max: f64, liquids: bool) -> Option<LookAt> {
        let s = self.entity(id)?;
        self.raycast(s.eye(), crate::look_dir(s.yaw, s.pitch), max, liquids)
    }
    /// Up to its maximum: `damage` with a negative amount.
    pub fn heal(&self, id: u64, amount: f32) -> bool {
        self.damage(id, -amount.max(0.0), 0, &crate::damage_causes::HEAL)
    }
    /// Hurts `id` (`source`: the entity that did it, 0 for none): the `HURT` question, the
    /// `DAMAGE` chain, the `actions::HURT` event, armor and death as the cause says. A negative
    /// `amount` heals instead, through the `HEAL` chain and the `ENTITY_HEALED` event, with none
    /// of damage's consequences. True if health changed.
    pub fn damage(&self, id: u64, amount: f32, source: u64, cause: impl Into<Id<crate::DamageCause>>) -> bool {
        unsafe { (host().entity_damage)(self.raw(), id, amount, source, cause.into().raw()) != 0 }
    }
    /// `id` hits `target` the way players do: other mods can forbid it (`question::ATTACK`) or
    /// change it (`chain::ATTACK_DAMAGE`). A player hits with what it holds, which wears; a mob
    /// with its kind's `.attack_damage(..)`, at its `.attack_speed(..)`. True if it landed.
    pub fn attack(&self, id: u64, target: u64) -> bool {
        unsafe { (host().entity_attack)(self.raw(), id, target) != 0 }
    }
    /// Whether `target` is within `id`'s reach (the `ENTITY_REACH` attribute, from its eyes to
    /// the nearest point of the target's box): the same test for players and mobs.
    pub fn in_reach(&self, id: u64, target: u64) -> bool {
        unsafe { (host().entity_in_reach)(self.raw(), id, target) != 0 }
    }

    /// Whether that point is under a liquid's surface, which a block that isn't full lies below
    /// its top: for what the block alone can't tell, like a head above shallow water.
    pub fn in_liquid(&self, pos: [f64; 3]) -> bool {
        unsafe { (host().in_liquid)(self.raw(), &pos) != 0 }
    }
    /// How charged a player's next swing is, 0..1, with what it holds now.
    pub fn swing_strength(&self, player: u64) -> f32 {
        unsafe { (host().entity_swing_strength)(self.raw(), player) }
    }
    /// The value of `c` on any holder: an entity, a block's entity, a stack in a slot.
    pub fn component<T: Value>(&self, h: impl Into<Holder>, c: &Component<T>) -> Option<T> {
        let mut buf = vec![0u8; size_of::<T>()];
        let n = unsafe { (host().component_get)(self.raw(), &h.into(), c.id(), buf.as_mut_ptr(), buf.len() as u32) };
        (n as usize == buf.len()).then(|| unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const T) })
    }
    /// Sets `c` on any holder. An entity has it only if its kind attaches it; a stack takes any.
    pub fn set_component<T: Value>(&self, h: impl Into<Holder>, c: &Component<T>, v: T) -> bool {
        let bytes = crate::component::value_bytes(&v);
        unsafe { (host().component_set)(self.raw(), &h.into(), c.id(), Slice::new(&bytes)) != 0 }
    }
    /// An entity's component changed in place (a stack's is a value: `set_component`); changes
    /// are saved and synced like any other.
    pub fn component_mut<T: Value>(&self, id: u64, c: &Component<T>) -> Option<&mut T> {
        let p = unsafe { (host().component)(self.raw(), id, c.id()) };
        (!p.is_null()).then(|| unsafe { &mut *(p as *mut T) })
    }
    /// Holds `id` in `pose` (a pet sitting, a zombie crawling, a player in a chair; its client
    /// follows), or with `None` gives it back to what its movement makes.
    /// With `facing` (degrees, feet that way when lying) it also stays still and turned so: in a bed.
    pub fn set_pose(&self, id: u64, pose: Option<&crate::Pose>, facing: Option<f32>) {
        let yaw = facing.map_or(f32::NAN, f32::to_radians);
        unsafe { (host().entity_set_pose)(self.raw(), id, pose.map_or(u32::MAX, |p| p.id()), yaw) }
    }
    /// A player in creative mode.
    pub fn creative(&self, id: u64) -> bool {
        self.game_mode(id) == abi::game_mode::CREATIVE
    }
    /// A player whose mode spares it damage, hunger and air (creative, spectator).
    pub fn outside_survival(&self, id: u64) -> bool {
        crate::GameMode::flags_of(self.game_mode(id)) & abi::game_mode_flags::INVULNERABLE != 0
    }
    /// The id of its game mode (`game_modes::CREATIVE.id()`): survival for what isn't a player.
    pub fn game_mode(&self, id: u64) -> u32 {
        unsafe { (host().game_mode)(self.raw(), id) }
    }
    /// Energy (the `ENERGY` component, read and set like any other) moves `amount` in (positive) or out (negative) within capacity and rate limits; returns
    /// what moved (or would, with `simulate`), with the same sign.
    pub fn energy_move(&self, h: impl Into<Holder>, amount: i64, simulate: bool) -> i64 {
        unsafe { (host().energy_move)(self.raw(), &h.into(), amount, simulate as u32) }
    }

    pub fn players(&self) -> Vec<u64> {
        self.players_iter().collect()
    }
    /// `players` without collecting them (mob AI asks every tick).
    pub fn players_iter(&self) -> impl Iterator<Item = u64> + '_ {
        let n = unsafe { (host().player_count)(self.raw()) };
        (0..n).map(move |i| unsafe { (host().player_at)(self.raw(), i) })
    }
    /// The inventory of an entity, a block (`[x, y, z]`, through its block entity) or a held
    /// stack (`Holder::held(player, slot)`, a backpack).
    pub fn inventory(&self, h: impl Into<Holder>) -> Inventory {
        Inventory { world: *self, holder: h.into() }
    }
    pub fn selected_slot(&self, player: u64) -> u32 {
        unsafe { (host().selected_slot)(self.raw(), player) }
    }
    /// A dropped stack, tossed a little; 0 for an empty one. `spawn_with(&ITEM, ..)` to throw
    /// it somewhere.
    pub fn spawn_item(&self, pos: [f64; 3], stack: ItemStack) -> u64 {
        self.spawn_with(&ITEM, pos, &Spawn::new().stack(stack))
    }
    /// Entity attached to the block at `pos` (furnace, chest), 0 if none.
    pub fn block_entity(&self, pos: [i32; 3]) -> u64 {
        unsafe { (host().block_entity)(self.raw(), &pos) }
    }
    /// Opens a menu (a handle, or its id) on a holder: an entity, a block, a held stack, or
    /// `Holder::NONE` for menus without one. False (and a logged reason) when the holder isn't
    /// there or holds no items.
    pub fn open_menu(&self, player: u64, menu: impl Into<Id<Menu>>, h: impl Into<Holder>) -> bool {
        unsafe { (host().open_menu)(self.raw(), player, menu.into().raw(), &h.into()) != 0 }
    }

    /// Whether a player here shows the gizmo group `group` (`/gizmo mymod:pipes`; the mod's
    /// namespace is added to bare names). Check before computing what to draw.
    pub fn gizmo_shown(&self, group: &GizmoGroup) -> bool {
        unsafe { (host().gizmo_shown)(self.raw(), Str::new(group.name())) != 0 }
    }
    /// A line seen by players showing `group`, color `0xRRGGBBAA`, for `ticks`.
    pub fn draw_line(&self, group: &GizmoGroup, a: [f64; 3], b: [f64; 3], color: u32, ticks: u32) {
        self.draw(group, abi::gizmo_kind::LINE, a, b, color, ticks, "", &[]);
    }
    pub fn draw_box(&self, group: &GizmoGroup, min: [f64; 3], max: [f64; 3], color: u32, ticks: u32) {
        self.draw(group, abi::gizmo_kind::BOX, min, max, color, ticks, "", &[]);
    }
    /// See-through by `color`'s alpha.
    pub fn draw_fill(&self, group: &GizmoGroup, min: [f64; 3], max: [f64; 3], color: u32, ticks: u32) {
        self.draw(group, abi::gizmo_kind::FILL, min, max, color, ticks, "", &[]);
    }
    /// The outline of a shape: where a fill or a search will reach, before it does.
    pub fn draw_shape(&self, group: &GizmoGroup, shape: &crate::Shape, color: u32, ticks: u32) {
        for (a, b) in shape.outline() {
            self.draw_line(group, a, b, color, ticks);
        }
    }
    /// The translation `key` with `args` at `pos`, in each viewer's language.
    /// `size` times as big as menu text.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text(&self, group: &GizmoGroup, pos: [f64; 3], key: &str, args: &[crate::Arg], color: u32, size: f32, ticks: u32) {
        self.draw(group, abi::gizmo_kind::TEXT, pos, [size as f64, 0.0, 0.0], color, ticks, key, args);
    }
    #[allow(clippy::too_many_arguments)]
    fn draw(&self, group: &GizmoGroup, kind: u32, a: [f64; 3], b: [f64; 3], color: u32, ticks: u32, key: &str, args: &[crate::Arg]) {
        let raw = crate::ui::raw_args(args);
        unsafe { (host().gizmo)(self.raw(), Str::new(group.name()), kind, &a, &b, color, ticks, Str::new(key), abi::Slice::new(&raw)) }
    }
}

#[derive(Clone, Copy)]
pub struct Inventory {
    world: World,
    holder: Holder,
}

impl Inventory {
    /// Slots, 0 when the holder has no inventory.
    pub fn len(&self) -> u32 {
        unsafe { (host().inventory_len)(self.world.raw(), &self.holder) }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get(&self, slot: u32) -> ItemStack {
        unsafe { (host().inventory_get)(self.world.raw(), &self.holder, slot) }
    }
    pub fn set(&self, slot: u32, stack: ItemStack) {
        unsafe { (host().inventory_set)(self.world.raw(), &self.holder, slot, stack) }
    }
    /// Returns what did not fit.
    pub fn insert(&self, stack: ItemStack) -> ItemStack {
        unsafe { (host().inventory_insert)(self.world.raw(), &self.holder, stack) }
    }
    /// A machine's slot group (`MachineBuilder::group`) in this inventory; a name it doesn't
    /// have is logged as a mistake and gives no slots.
    pub fn group(&self, name: &str) -> SlotGroup<'static> {
        self.find_group(name).unwrap_or_else(|| {
            crate::error!("the inventory of {:?} has no group {name}", self.holder);
            SlotGroup::empty(GroupIn::Inventory(*self))
        })
    }

    /// The group if this inventory has it: a player's equipment, which mobs don't have.
    pub fn find_group(&self, name: &str) -> Option<SlotGroup<'static>> {
        let mut out = abi::SlotGroup::default();
        let found = unsafe { (host().inventory_group)(self.world.raw(), &self.holder, Str::new(name), &mut out) } != 0;
        found.then(|| SlotGroup::of(GroupIn::Inventory(*self), out))
    }
}

#[derive(Clone, Copy)]
enum GroupIn {
    Inventory(Inventory),
    Menu(*mut abi::MenuCtx),
}

/// Slots of one group, numbered from 0: of an inventory (`w.inventory(h).group(..)`) or of an
/// open menu (`m.group(..)`, whoever's inventory they show), valid while the menu callback runs.
pub struct SlotGroup<'a> {
    of: GroupIn,
    start: u32,
    len: u32,
    cols: u32,
    _p: std::marker::PhantomData<&'a MenuCtx>,
}

impl SlotGroup<'_> {
    fn of(of: GroupIn, g: abi::SlotGroup) -> Self {
        SlotGroup { of, start: g.start, len: g.len, cols: g.cols, _p: std::marker::PhantomData }
    }
    fn empty(of: GroupIn) -> Self {
        SlotGroup { of, start: 0, len: 0, cols: 0, _p: std::marker::PhantomData }
    }
    pub fn len(&self) -> u32 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Its width as laid out (a 3 x 3 grid has 3).
    pub fn cols(&self) -> u32 {
        self.cols
    }
    pub fn rows(&self) -> u32 {
        self.len.div_ceil(self.cols.max(1))
    }
    pub fn get(&self, i: u32) -> ItemStack {
        if i >= self.len {
            return ItemStack::EMPTY;
        }
        match self.of {
            GroupIn::Inventory(inv) => inv.get(self.start + i),
            GroupIn::Menu(m) => unsafe { (host().menu_get)(m, self.start + i) },
        }
    }
    pub fn set(&self, i: u32, stack: ItemStack) {
        if i >= self.len {
            return;
        }
        match self.of {
            GroupIn::Inventory(inv) => inv.set(self.start + i, stack),
            GroupIn::Menu(m) => unsafe { (host().menu_set)(m, self.start + i, stack) },
        }
    }
}

// ---------------------------------------------------------------- recipes

pub struct PlayerInventoryBuilder<'a> {
    raw: *mut abi::Registrar,
    groups: GroupList,
    _p: std::marker::PhantomData<&'a mut Registry>,
}

impl PlayerInventoryBuilder<'_> {
    /// A group of the player's inventory: its size from `slots` (`Slots::row(9).hotbar()`).
    pub fn group(mut self, name: &str, slots: Slots) -> Self {
        self.groups.push("", name, slots, abi::slot_owner::PLAYER);
        self
    }
}

impl Drop for PlayerInventoryBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let descs = self.groups.descs();
        unsafe { (host().reg_player_inventory)(self.raw, Slice::new(&descs)) }
    }
}

pub fn recipe_grid_output(kind: &RecipeKind, grid: &[ItemStack], width: u32, height: u32) -> ItemStack {
    unsafe { (host().recipe_match)(Str::new(kind.name()), grid.as_ptr(), width, height) }
}

/// Output and time of a processing recipe (smelting) for `input`.
pub fn recipe_process(kind: &RecipeKind, input: u32) -> Option<(ItemStack, u32)> {
    let mut t = 0;
    let s = unsafe { (host().recipe_process)(Str::new(kind.name()), input, &mut t) };
    (!s.is_empty()).then_some((s, t))
}

// ---------------------------------------------------------------- registration

pub struct EntityTick {
    args: abi::EntityTickArgs,
}

impl EntityTick {
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn ids(&self) -> &[u64] {
        unsafe { std::slice::from_raw_parts(self.args.ids, self.args.count) }
    }
}

pub struct EntityEvent {
    args: abi::EntityEventArgs,
}

impl EntityEvent {
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn entity(&self) -> u64 {
        self.args.entity
    }
    pub fn other(&self) -> u64 {
        self.args.other
    }
}

fn tick_cb(f: impl Fn(&EntityTick) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::EntityTickArgs| f(unsafe { &*(a as *const abi::EntityTickArgs as *const EntityTick) }))
}

fn event_cb(f: impl Fn(&mut EntityEvent) -> bool + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::EntityEventArgs| {
        let handled = f(unsafe { &mut *(a as *mut abi::EntityEventArgs as *mut EntityEvent) });
        a.handled = handled as u32;
    })
}

/// A model part (a bone): its name, pivot, boxes, free-form triangles, and the part it hangs
/// from. The name says how it moves by itself, like in Blockbench's Bedrock models: `head` looks
/// around, `rightArm`, `leftArm`, `rightLeg`, `leftLeg` swing while walking (arms resting held
/// out, like a zombie's, only sway), `body` leans when sneaking, `leg0`..`leg3` are a
/// quadruped's and `leg0`..`leg7` a spider's sweeping legs; other names keep still. Keyframe
/// animations address bones by the same names.
pub struct ModelPart {
    pub pivot: [f32; 3],
    pub boxes: Vec<abi::ModelCuboid>,
    pub rest: [f32; 3],
    pub name: String,
    pub parent: i32,
    pub mesh: Vec<abi::MeshVertex>,
}

impl ModelPart {
    pub fn new(name: &str, pivot: [f32; 3]) -> ModelPart {
        ModelPart { pivot, boxes: Vec::new(), rest: [0.0; 3], name: name.to_string(), parent: -1, mesh: Vec::new() }
    }
    /// Resting rotation in degrees about x, y, z (applied in that order).
    pub fn rotation(mut self, x: f32, y: f32, z: f32) -> ModelPart {
        self.rest = [x, y, z].map(f32::to_radians);
        self
    }
    /// Box relative to the pivot in model pixels, `uv` = top-left texel of its unwrap.
    pub fn cuboid(mut self, origin: [f32; 3], size: [f32; 3], uv: [f32; 2]) -> ModelPart {
        self.boxes
            .push(abi::ModelCuboid { origin, extent: size, uv, ..abi::ModelCuboid::DEFAULT });
        self
    }
    /// The last box grown by this much on every side, its texture staying the same (wool, a skin's
    /// outer layer).
    pub fn inflated(mut self, by: f32) -> ModelPart {
        if let Some(b) = self.boxes.last_mut() {
            b.inflate = by;
        }
        self
    }
    /// The last box's unwrap mirrored left to right.
    pub fn mirrored(mut self) -> ModelPart {
        if let Some(b) = self.boxes.last_mut() {
            b.mirror = 1;
        }
        self
    }
    /// Hangs from the part at `index` in the same model: moves and turns with it.
    pub fn child_of(mut self, index: usize) -> ModelPart {
        self.parent = index as i32;
        self
    }
    /// Free-form triangles (three corners each), in model pixels like the pivot.
    pub fn mesh(mut self, triangles: Vec<abi::MeshVertex>) -> ModelPart {
        self.mesh.extend(triangles);
        self
    }
}

/// A sphere of triangles around `center` (model pixels), its texture stretched over the texel
/// rectangle `uv` (x, y, width, height) like a globe's map.
pub fn sphere_mesh(center: [f32; 3], radius: f32, rings: u32, segments: u32, uv: [f32; 4]) -> Vec<abi::MeshVertex> {
    use std::f32::consts::{PI, TAU};
    let (rings, segments) = (rings.max(2), segments.max(3));
    let at = |i: u32, j: u32| {
        let (theta, phi) = (PI * i as f32 / rings as f32, TAU * j as f32 / segments as f32);
        let pos =
            [center[0] + radius * theta.sin() * phi.cos(), center[1] + radius * theta.cos(), center[2] + radius * theta.sin() * phi.sin()];
        abi::MeshVertex { pos, uv: [uv[0] + uv[2] * j as f32 / segments as f32, uv[1] + uv[3] * i as f32 / rings as f32] }
    };
    let mut out = Vec::new();
    for i in 0..rings {
        for j in 0..segments {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            // Wound so the outside faces the camera (back faces are culled).
            out.extend([a, d, c, a, c, b]);
        }
    }
    out
}

/// Classic humanoid on a 64x32 skin; `arms_forward` for zombies.
/// A model built in code: its parts and the size of the skin its UVs are laid out on (the
/// picture itself may be bigger, a pack's high resolution skin keeps lining up).
pub struct Model {
    pub parts: Vec<ModelPart>,
    pub uv: [u32; 2],
}

impl Model {
    /// The UV size, if the layout isn't on a 64 x 32 skin.
    pub fn uv_size(mut self, width: u32, height: u32) -> Self {
        self.uv = [width, height];
        self
    }
}

impl From<Vec<ModelPart>> for Model {
    fn from(parts: Vec<ModelPart>) -> Model {
        Model { parts, uv: [64, 32] }
    }
}

/// Classic humanoid on a 64x32 skin, the player's too; `arms_forward` for zombies.
/// The limbs at -x have their unwrap mirrored: looking at the front +x is on the left, where the
/// limb drawn as it lies in the unwrap goes (`rightArm` is the one at -x all the same).
pub fn humanoid(arms_forward: bool) -> Model {
    let arms = if arms_forward { 90.0 } else { 0.0 };
    Model::from(vec![
        ModelPart::new("head", [0.0, 24.0, 0.0]).cuboid([-4.0, 0.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0]),
        ModelPart::new("body", [0.0, 24.0, 0.0]).cuboid([-4.0, -12.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0]),
        ModelPart::new("rightArm", [-6.0, 22.0, 0.0])
            .cuboid([-2.0, -10.0, -2.0], [4.0, 12.0, 4.0], [40.0, 16.0])
            .mirrored()
            .rotation(arms, 0.0, 0.0),
        ModelPart::new("leftArm", [6.0, 22.0, 0.0])
            .cuboid([-2.0, -10.0, -2.0], [4.0, 12.0, 4.0], [40.0, 16.0])
            .rotation(arms, 0.0, 0.0),
        ModelPart::new("rightLeg", [-2.0, 12.0, 0.0])
            .cuboid([-2.0, -12.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0])
            .mirrored(),
        ModelPart::new("leftLeg", [2.0, 12.0, 0.0]).cuboid([-2.0, -12.0, -2.0], [4.0, 12.0, 4.0], [0.0, 16.0]),
    ])
}

/// Vanilla's spider on a 64x32 skin: head, thorax, abdomen and eight splayed legs.
pub fn spider() -> Model {
    use std::f32::consts::FRAC_PI_4 as Q;
    let mut parts = vec![
        ModelPart::new("head", [0.0, 9.0, -3.0]).cuboid([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [32.0, 4.0]),
        ModelPart::new("neck", [0.0, 9.0, 0.0]).cuboid([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0.0, 0.0]),
        ModelPart::new("body", [0.0, 9.0, 9.0]).cuboid([-5.0, -4.0, -6.0], [10.0, 8.0, 12.0], [0.0, 12.0]),
    ];
    // (z, sideways splay, fan) back to front, a left leg then its right twin: leg0, leg1 ...
    for (i, (z, splay, fan)) in [(2.0, 1.0, 2.0), (1.0, 0.74, 1.0), (0.0, 0.74, -1.0), (-1.0, 1.0, -2.0)]
        .into_iter()
        .enumerate()
    {
        parts.push(
            ModelPart::new(&format!("leg{}", i * 2), [-4.0, 9.0, z])
                .cuboid([-15.0, -1.0, -1.0], [16.0, 2.0, 2.0], [18.0, 0.0])
                .rotation(0.0, (fan * Q / 2.0).to_degrees(), (splay * Q).to_degrees()),
        );
        parts.push(
            ModelPart::new(&format!("leg{}", i * 2 + 1), [4.0, 9.0, z])
                .cuboid([-1.0, -1.0, -1.0], [16.0, 2.0, 2.0], [18.0, 0.0])
                .rotation(0.0, (-fan * Q / 2.0).to_degrees(), (-splay * Q).to_degrees()),
        );
    }
    Model::from(parts)
}

/// Four-legged animal: body (width, height, length), leg height and head size in model pixels.
/// Skin is 64x64: head at (0,0), legs at (0,16), body at (0,32).
pub fn quadruped(body: [f32; 3], leg: f32, head: [f32; 3]) -> Model {
    let [bw, bh, bl] = body;
    let y = leg;
    // Bedrock's quadruped names: leg0 and leg1 at the back, leg2 and leg3 at the front.
    let legs = [
        ("leg0", [-bw / 2.0 + 2.0, y, bl / 2.0 - 2.0]),
        ("leg1", [bw / 2.0 - 2.0, y, bl / 2.0 - 2.0]),
        ("leg2", [bw / 2.0 - 2.0, y, -bl / 2.0 + 2.0]),
        ("leg3", [-bw / 2.0 + 2.0, y, -bl / 2.0 + 2.0]),
    ];
    let mut parts = vec![
        ModelPart::new("body", [0.0, y, 0.0]).cuboid([-bw / 2.0, 0.0, -bl / 2.0], [bw, bh, bl], [0.0, 32.0]),
        ModelPart::new("head", [0.0, y + bh * 0.75, -bl / 2.0]).cuboid([-head[0] / 2.0, -head[1] / 2.0, -head[2]], head, [0.0, 0.0]),
    ];
    for (name, pivot) in legs {
        parts.push(ModelPart::new(name, pivot).cuboid([-2.0, -leg, -2.0], [4.0, leg, 4.0], [0.0, 16.0]));
    }
    Model::from(parts).uv_size(64, 64)
}

pub struct EntityBuilder<'a> {
    no_egg: bool,
    reg: &'a mut Registry,
    name: String,
    handle: Option<&'static EntityKind>,
    components: Vec<Pending>,
    width: f32,
    height: f32,
    eye: Option<f32>,
    speed: f32,
    flags: u32,
    texture: String,
    texture_size: [u32; 2],
    parts: Vec<ModelPart>,
    on_tick: abi::Callback,
    on_interact: abi::Callback,
    replace: bool,
    carry_height: f32,
    category_name: String,
    template: String,
    slots: u32,
    /// Components (full name, starting value) every entity of this kind gets.
    attached: Vec<(&'static str, Vec<u8>)>,
}

/// Components of the kind (not per entity; see `Registry::entity_component` for that).
impl crate::WithComponents for EntityBuilder<'_> {
    fn pending(&mut self) -> &mut Vec<Pending> {
        &mut self.components
    }
}

impl EntityBuilder<'_> {
    /// Every entity of this kind carries `component` (declared with `r.entity_component`, by any
    /// mod), starting at its declared value. Not `.set`: that is one value of the kind itself,
    /// `.attach` gives each entity its own, which changes.
    pub fn attach<T: Value>(mut self, component: &'static Component<T>) -> Self {
        self.attached.push((component.name(), Vec::new()));
        self
    }
    /// `attach` with another starting value.
    pub fn attach_with<T: Value>(mut self, component: &'static Component<T>, value: T) -> Self {
        self.attached
            .push((component.name(), crate::component::value_bytes(&value)));
        self
    }
    /// A solid body: others stand on it and ride along, bump into it and get pushed by it
    /// (platforms, boats, lifts). Not pushed around itself.
    pub fn solid(mut self) -> Self {
        self.flags = (self.flags | entity_flags::SOLID) & !entity_flags::PUSHABLE;
        self
    }
    /// Takes no damage (no hurt flash, health stays); `/kill` and the void still remove it.
    pub fn invulnerable(mut self) -> Self {
        self.flags |= entity_flags::INVULNERABLE;
        self
    }
    /// Neither pushes nor is pushed by other entities (ghosts, projectiles).
    pub fn unpushable(mut self) -> Self {
        self.flags &= !entity_flags::PUSHABLE;
        self
    }
    /// Moved exactly by the velocity mod logic gives it (`set_entity`), through blocks, no
    /// gravity: lifts, airships, contraptions.
    pub fn kinematic(mut self) -> Self {
        self.flags |= entity_flags::KINEMATIC;
        self
    }
    /// A solid body that hurts what it squeezes into blocks instead of only pushing it.
    pub fn crushing(mut self) -> Self {
        self.flags |= entity_flags::CRUSHES;
        self
    }
    /// Only what it holds, components or an inventory: nothing sees, hits or saves it, and it stays
    /// until removed wherever it was put.
    pub fn not_in_world(mut self) -> Self {
        self.flags |= entity_flags::NOT_IN_WORLD;
        self
    }
    /// Riders up to `blocks` above the body (jumping, flying around inside it) keep moving with
    /// it and their own motion stays relative to it, so a jump on a fast ship lands on the ship.
    pub fn carry_height(mut self, blocks: f32) -> Self {
        self.carry_height = blocks;
        self
    }
    /// Replaces the entity kind with this name from a mod listed in `deps`.
    pub fn replacing(mut self) -> Self {
        self.replace = true;
        self
    }
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }
    /// Eye height in blocks, `EYE_SHARE` of its height when not said.
    pub fn eye_height(mut self, blocks: f32) -> Self {
        self.eye = Some(blocks);
        self
    }
    /// No spawn egg. Every entity has one otherwise, `<name>_spawn_egg`, called after it and looking
    /// generic until `item/<name>_spawn_egg.png` exists: for what isn't a creature (a projectile, a
    /// moving platform).
    pub fn no_egg(mut self) -> Self {
        self.no_egg = true;
        self
    }
    /// Damage of its melee hits (`World::attack`), like an item's.
    pub fn attack_damage(self, damage: f32) -> Self {
        crate::WithComponents::set(self, &crate::component::ATTACK_DAMAGE, damage)
    }
    /// Its melee hits per second (1), like an item's; `attribute::ATTACK_SPEED` multiplies it.
    pub fn attack_speed(self, per_second: f32) -> Self {
        crate::WithComponents::set(self, &crate::component::ATTACK_SPEED, per_second)
    }
    /// How far it hits (the `ENTITY_REACH` attribute, from its eyes to the target's box); a
    /// modifier changes it like any attribute.
    pub fn reach(mut self, blocks: f64) -> Self {
        self.components
            .retain(|p| p.name != abi::attribute::ENTITY_REACH);
        self.components
            .push(Pending::value(abi::attribute::ENTITY_REACH, "f64".into(), crate::component::value_bytes(&blocks)));
        self
    }
    pub fn max_health(mut self, h: f32) -> Self {
        self.components
            .retain(|p| p.name != abi::attribute::MAX_HEALTH);
        self.components
            .push(Pending::value(abi::attribute::MAX_HEALTH, "f64".into(), crate::component::value_bytes(&(h as f64))));
        self
    }
    pub fn walk_speed(mut self, s: f32) -> Self {
        self.speed = s;
        self
    }
    pub fn flags(mut self, f: u32) -> Self {
        self.flags |= f;
        self
    }
    /// Every entity of this kind carries an inventory of `slots` (`w.inventory(id)`).
    pub fn inventory(mut self, slots: u32) -> Self {
        self.slots = slots;
        self
    }
    /// A category registered with `Registry::entity_category` (any mod's), for spawn caps and
    /// despawning.
    pub fn category(mut self, category: &EntityCategory) -> Self {
        self.category_name = category.name().to_string();
        self
    }
    pub fn hostile(mut self) -> Self {
        self.category_name = "core:hostile".into();
        self
    }
    pub fn passive(mut self) -> Self {
        self.category_name = "core:passive".into();
        self
    }
    /// Skin texture under `entity/`; without this, the kind's name (`entity/zombie`).
    pub fn texture(mut self, t: &str) -> Self {
        self.texture = crate::asset("entity", t);
        self
    }
    /// A model built in code (`biped(..)`, `quadruped(..)`, `spider()`, or parts of your own).
    pub fn model(mut self, model: impl Into<Model>) -> Self {
        let m = model.into();
        (self.parts, self.texture_size) = (m.parts, m.uv);
        self
    }
    /// Sound event `event` (`hurt`, `death`, `ambient`, `step`): one of the files
    /// `sounds/entity/<kind>/<event>*.ogg|wav` at random.
    pub fn sound(self, event: &str) -> Self {
        let file = format!("entity/{}/{event}*", self.kind_path());
        self.sound_files(event, &[&file])
    }
    /// Sound event `event` from these files of `sounds/entity/<kind>/`, one at random.
    pub fn sound_variants(self, event: &str, files: &[&str]) -> Self {
        let path = self.kind_path();
        let files: Vec<String> = files.iter().map(|f| format!("entity/{path}/{f}")).collect();
        let files: Vec<&str> = files.iter().map(String::as_str).collect();
        self.sound_files(event, &files)
    }
    fn kind_path(&self) -> String {
        self.name
            .split_once(':')
            .map_or(self.name.as_str(), |x| x.1)
            .to_string()
    }
    fn sound_files(mut self, event: &str, files: &[&str]) -> Self {
        let sound = crate::Sound::generated(format!("{}.{event}", self.name));
        self.reg
            .sound(sound)
            .files(files)
            .category(&crate::sound_categories::CREATURES)
            .pitch(0.8, 1.2);
        self.components.retain(|p| p.name != abi::comp::SOUND);
        self.components
            .push(Pending::text(abi::comp::SOUND, &self.name));
        self
    }
    /// Sounds from `<ns>:entity.<group>` (`.hurt`, `.death`, `.ambient`, `.step` events); a name
    /// with a namespace is used as it is.
    pub fn sound_group(mut self, group: &str) -> Self {
        let prefix = if group.contains(':') { group.to_string() } else { format!("{}:entity.{group}", self.reg.ns) };
        self.components
            .push(Pending::text(abi::comp::SOUND, &prefix));
        self
    }

    /// Keyframe animations from `animations/<name>.animation.json`: `idle` and `walk` play by
    /// themselves, others with `World::action_play`. A file named like the kind is used even
    /// without this.
    pub fn animations(mut self, name: &str) -> Self {
        let name = crate::asset("", name);
        self.components
            .push(Pending::text(abi::comp::ANIMATIONS, &name));
        self
    }
    /// A moving body built of `template` (`Registry::template`; block (0, 0, 0) is
    /// centered on the entity's position, its box is what the blocks fill). Blocks are placed and
    /// broken on it like in the world, and it carries what stands on it; usually `.solid()`.
    pub fn body(mut self, template: &crate::Template) -> Self {
        self.template = template.name().to_string();
        self
    }
    /// Shades the model with `shaders/entity/<name>.wgsl` (namespace optional).
    pub fn shader(mut self, name: &str) -> Self {
        let name = crate::asset("", name);
        self.components
            .push(Pending::text(abi::comp::SHADER, &name));
        self
    }
    /// Called once per tick with every entity of this kind.
    pub fn on_tick(mut self, f: impl Fn(&EntityTick) + Send + Sync + 'static) -> Self {
        self.on_tick = tick_cb(f);
        self
    }
    /// Behavior from goals (`ai::Goals`), run every tick; replaces `on_tick`.
    pub fn goals(self, goals: crate::ai::Goals) -> Self {
        self.on_tick(move |t| goals.tick(t))
    }
    /// Leaves a player alone while `calm(world, the mob, the player)` holds, unless they hit it
    /// in the last 20 seconds: a spider by day, or one that only minds you within so many
    /// blocks. The kind's goals go by it through `ai::melee`, `ai::chase_target` and `ai::target`.
    pub fn neutral(self, calm: impl Fn(&World, &EntityState, &EntityState) -> bool + Send + Sync + 'static) -> Self {
        let Some(kind) = self.handle else { return self };
        if crate::ai::set_neutral(kind, Box::new(calm)) {
            self.reg
                .entity_component(&crate::ai::ANGER, crate::ai::Anger::default(), 0);
        }
        self.attach(&crate::ai::ANGER)
            .on_hurt(crate::ai::provoke_on_hurt)
    }
    /// One of these was hurt: `actions::HURT` for this kind, after the damage, with the other
    /// events at the end of the tick. Changing the damage is `r.modify(&chain::DAMAGE, ..)`,
    /// preventing it `r.intercept(&actions::HURT, ..)`.
    pub fn on_hurt(self, f: impl Fn(&World, &crate::Hurt) + Send + Sync + 'static) -> Self {
        let Some(kind) = self.handle else { return self };
        self.reg.on(&crate::actions::HURT, move |b| {
            let w = b.world();
            for h in b
                .iter()
                .filter(|h| w.entity(h.entity).is_some_and(|s| s.kind == kind.id()))
            {
                f(&w, h);
            }
        });
        self
    }
    /// One of these died: `actions::DIE` for this kind.
    pub fn on_died(self, f: impl Fn(&World, &crate::Death) + Send + Sync + 'static) -> Self {
        let Some(kind) = self.handle else { return self };
        self.reg.on(&crate::actions::DIE, move |b| {
            let w = b.world();
            for d in b.iter().filter(|d| d.kind == kind.id()) {
                f(&w, d);
            }
        });
        self
    }
    /// A player right clicked one, and `actions::INTERACT_ENTITY` let it; true if it did something,
    /// which swings the player's arm.
    pub fn on_interact(mut self, f: impl Fn(&mut EntityEvent) -> bool + Send + Sync + 'static) -> Self {
        self.on_interact = event_cb(f);
        self
    }
}

fn raw_part(p: &ModelPart) -> abi::ModelPart {
    abi::ModelPart {
        pivot: p.pivot,
        cuboids: Slice::new(&p.boxes),
        rest: p.rest,
        name: Str::new(&p.name),
        parent: p.parent,
        mesh: Slice::new(&p.mesh),
        ..abi::ModelPart::DEFAULT
    }
}

impl crate::content::ModelSource for Model {
    /// A kind's model, another mod's or the engine's (`core:player` is drawn with `biped(false)` by
    /// base); a later mod's wins.
    fn set_on(self, r: &mut Registry, what: crate::content::Target<'_>) {
        let parts: Vec<abi::ModelPart> = self.parts.iter().map(raw_part).collect();
        let kind = crate::qualify(r.ns, what.name());
        unsafe { (host().reg_entity_model)(r.raw, Str::new(&kind), Slice::new(&parts), self.uv[0], self.uv[1]) }
    }
}

impl Drop for EntityBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        if !self.no_egg && !self.replace {
            let mob = EntityKind::generated(self.name.clone());
            self.reg
                .item(crate::Item::generated(format!("{}_spawn_egg", self.name)))
                .spawns(mob)
                .tab(&crate::SPAWN_EGGS);
        }
        let parts: Vec<abi::ModelPart> = self.parts.iter().map(raw_part).collect();
        let mut scratch = Vec::new();
        let values = crate::content::raw_values(&self.components, &mut scratch);
        let desc = abi::EntityDesc {
            name: Str::new(&self.name),
            cell: self.handle.map_or(std::ptr::null(), |h| h.id_cell()),
            width: self.width,
            height: self.height,
            eye: self.eye.unwrap_or(self.height * EYE_SHARE),
            speed: self.speed,
            flags: self.flags,
            texture: Str::new(&self.texture),
            texture_size: self.texture_size,
            parts: Slice::new(&parts),
            on_tick: self.on_tick,
            on_interact: self.on_interact,
            replacing: self.replace as u32,
            components: Slice::new(&values),
            carry_height: self.carry_height,
            category_name: Str::new(&self.category_name),
            template: Str::new(&self.template),
            slots: self.slots,
            ..abi::EntityDesc::DEFAULT
        };
        unsafe { (host().reg_entity)(self.reg.raw, &desc) }
        for (comp, value) in &self.attached {
            unsafe { (host().reg_attach)(self.reg.raw, Str::new(&self.name), Str::new(comp), Slice::new(value)) }
        }
    }
}

pub struct MenuCtx {
    args: abi::MenuArgs,
}

impl MenuCtx {
    pub fn world(&self) -> World {
        World::from_raw(self.args.world)
    }
    pub fn player(&self) -> u64 {
        self.args.player
    }
    /// What the menu was opened on (a machine's block entity, a pouch in a slot); `None` for
    /// the player's own and a crafting table's.
    pub fn owner(&self) -> Option<Holder> {
        (self.args.owner.kind != abi::holder::NONE).then_some(self.args.owner)
    }
    /// Slot group `name` of this menu; asking for one it doesn't have is a mistake that gets
    /// logged, and the group is empty.
    pub fn group(&self, name: &str) -> SlotGroup<'_> {
        self.find_group(name).unwrap_or_else(|| {
            crate::reg_mistake(&format!("the menu has no group {name}"));
            SlotGroup::empty(GroupIn::Menu(self.args.menu))
        })
    }
    /// `group`, when not having it is fine.
    pub fn find_group(&self, name: &str) -> Option<SlotGroup<'_>> {
        let mut out = abi::SlotGroup::default();
        let found = unsafe { (host().menu_group)(self.args.menu, Str::new(name), &mut out) } != 0;
        found.then(|| SlotGroup::of(GroupIn::Menu(self.args.menu), out))
    }
}

/// The groups `player_inventory()` adds, for `Ui::group` and `MenuCtx::group`.
pub const INVENTORY_GROUP: &str = "core:inventory";
pub const HOTBAR_GROUP: &str = "core:hotbar";
/// Every equipment slot of a player's inventory (`w.inventory(player).group(EQUIPMENT_GROUP)`).
pub const EQUIPMENT_GROUP: &str = "core:equipment";

/// A group of slots: a grid, row or column of one inventory sharing its rules. Shown with
/// `ui.group(name)`, read and written with `MenuCtx::group(name)`.
///
/// ```ignore
/// r.machine(&ALLOY_SMELTER, &MENU)
///     .group("input", Slots::row(2))
///     .group("fuel", Slots::single().accepts("fuel").background("flame_slot"))
///     .group("output", Slots::single().output().large());
/// ```
#[derive(Clone)]
pub struct Slots {
    owner: Option<u32>,
    start: u32,
    cols: u32,
    rows: u32,
    flags: u32,
    accepts: String,
    limit: u32,
    size: f32,
    background: String,
    tooltip: String,
    priority: i32,
}

impl Slots {
    pub fn grid(cols: u32, rows: u32) -> Slots {
        Slots {
            owner: None,
            start: u32::MAX,
            cols,
            rows,
            flags: 0,
            accepts: String::new(),
            limit: 0,
            size: 0.0,
            background: String::new(),
            tooltip: String::new(),
            priority: 0,
        }
    }
    pub fn row(n: u32) -> Slots {
        Slots::grid(n, 1)
    }
    pub fn column(n: u32) -> Slots {
        Slots::grid(1, n)
    }
    pub fn single() -> Slots {
        Slots::grid(1, 1)
    }
    /// Rows of nine, like a chest.
    pub fn chest(rows: u32) -> Slots {
        Slots::grid(9, rows)
    }
    /// The menu's own slots, given back to the player when it closes (crafting grids). The
    /// default for `MenuBuilder::group`.
    pub fn menu(mut self) -> Self {
        self.owner = Some(abi::slot_owner::MENU);
        self
    }
    /// The inventory of the entity the menu opened on. The default for `MachineBuilder::group`.
    pub fn machine(mut self) -> Self {
        self.owner = Some(abi::slot_owner::MACHINE);
        self
    }
    /// The player's hotbar, in `Registry::player_inventory`.
    pub fn hotbar(mut self) -> Self {
        self.flags |= slot_flags::HOTBAR;
        self
    }
    /// From this index of the owner's inventory instead of right after the group before.
    pub fn at(mut self, start: u32) -> Self {
        self.start = start;
        self
    }
    /// Items only go in (a machine uses them up).
    pub fn input(mut self) -> Self {
        self.flags |= abi::slot_flags::INPUT;
        self
    }
    /// Holds `capacity` millibuckets of one liquid: its item (`Item::named` the liquid's
    /// name), poured in and taken out with buckets by hand, read and set like any slot.
    pub fn liquid(mut self, capacity: u32) -> Self {
        self.flags |= abi::slot_flags::LIQUID;
        self.limit = capacity;
        self
    }
    /// Items only come out (results).
    pub fn output(mut self) -> Self {
        self.flags |= abi::slot_flags::OUTPUT;
        self
    }
    /// Only items with this tag or component (its full name, like `fuel` or `armor_head`).
    pub fn accepts(mut self, tag: &str) -> Self {
        self.accepts = tag.to_string();
        self
    }
    /// At most this many items per slot.
    pub fn limit(mut self, n: u32) -> Self {
        self.limit = n;
        self
    }
    /// Side of a slot in GUI pixels (18 by default).
    pub fn size(mut self, px: f32) -> Self {
        self.size = px;
        self
    }
    /// The big slot vanilla uses for results.
    pub fn large(self) -> Self {
        self.size(26.0)
    }
    /// A texture from `gui/` shown while a slot is empty (the armor silhouettes).
    pub fn background(mut self, texture: &str) -> Self {
        self.background = crate::asset("gui", texture);
        self
    }
    /// A translation shown over an empty slot.
    pub fn tooltip(mut self, key: &str) -> Self {
        self.tooltip = key.to_string();
        self
    }
    /// Shift-clicks from the player's inventory fill groups with a higher priority first.
    pub fn priority(mut self, p: i32) -> Self {
        self.priority = p;
        self
    }
}

/// Owned strings of the groups while their descriptors are sent.
struct GroupList {
    names: Vec<String>,
    groups: Vec<Slots>,
    strings: Vec<[String; 3]>,
}

impl GroupList {
    fn new() -> GroupList {
        GroupList { names: Vec::new(), groups: Vec::new(), strings: Vec::new() }
    }
    fn push(&mut self, ns: &str, name: &str, s: Slots, default_owner: u32) {
        let s = Slots { owner: Some(s.owner.unwrap_or(default_owner)), ..s };
        let q = |t: &str| if t.is_empty() || t.contains(':') { t.to_string() } else { qualify(ns, t) };
        self.strings
            .push([s.accepts.clone(), q(&s.background), s.tooltip.clone()]);
        self.names.push(name.to_string());
        self.groups.push(s);
    }
    fn player_inventory(&mut self) {
        self.names.push(INVENTORY_GROUP.into());
        self.groups
            .push(Slots { owner: Some(abi::slot_owner::PLAYER_MAIN), ..Slots::single() });
        self.strings.push(Default::default());
        self.names.push(HOTBAR_GROUP.into());
        self.groups
            .push(Slots { owner: Some(abi::slot_owner::PLAYER_HOTBAR), ..Slots::single() });
        self.strings.push(Default::default());
    }
    fn descs(&self) -> Vec<abi::SlotGroupDesc> {
        self.groups
            .iter()
            .zip(&self.names)
            .zip(&self.strings)
            .map(|((g, name), [accepts, background, tooltip])| abi::SlotGroupDesc {
                name: Str::new(name),
                owner: g.owner.unwrap_or(0),
                start: g.start,
                cols: g.cols,
                rows: g.rows,
                flags: g.flags,
                accepts: Str::new(accepts),
                limit: g.limit,
                size: g.size,
                background: Str::new(background),
                tooltip: Str::new(tooltip),
                priority: g.priority,
            })
            .collect()
    }
}

fn menu_cb(f: impl Fn(&MenuCtx) + Send + Sync + 'static) -> abi::Callback {
    callback(move |a: &mut abi::MenuArgs| f(unsafe { &*(a as *const abi::MenuArgs as *const MenuCtx) }))
}

pub struct MenuBuilder<'a> {
    reg: &'a mut Registry,
    name: String,
    handle: Option<&'static Menu>,
    desc: abi::MenuDesc,
    groups: GroupList,
    entity: String,
    recipes: Vec<String>,
}

impl MenuBuilder<'_> {
    /// A group of slots, by default the menu's own (`Slots::menu`). Groups come in this order.
    pub fn group(mut self, name: &str, slots: Slots) -> Self {
        self.groups
            .push(self.reg.ns, name, slots, abi::slot_owner::MENU);
        self
    }
    /// The player's inventory and hotbar as two groups (`INVENTORY_GROUP`, `HOTBAR_GROUP`), shown
    /// with `ui.player_inventory(..)`.
    pub fn player_inventory(mut self) -> Self {
        self.groups.player_inventory();
        self
    }
    /// The player's equipment slots, here among the groups, each named like its slot
    /// (`ui.equipment()` draws them).
    pub fn equipment(mut self) -> Self {
        self.groups.push(
            "",
            EQUIPMENT_GROUP,
            Slots { owner: Some(abi::slot_owner::PLAYER_EQUIPMENT), ..Slots::single() },
            abi::slot_owner::PLAYER_EQUIPMENT,
        );
        self
    }
    /// Recipes of this kind are made here: clicking an `arrow` bar of the screen shows them.
    pub fn recipes(mut self, kind: &RecipeKind) -> Self {
        self.recipes.push(kind.name().to_string());
        self
    }
    pub fn on_change(mut self, f: impl Fn(&MenuCtx) + Send + Sync + 'static) -> Self {
        self.desc.on_change = menu_cb(f);
        self
    }
    pub fn on_take(mut self, f: impl Fn(&MenuCtx) + Send + Sync + 'static) -> Self {
        self.desc.on_take = menu_cb(f);
        self
    }
    pub fn on_tick(mut self, f: impl Fn(&MenuCtx) + Send + Sync + 'static) -> Self {
        self.desc.on_tick = menu_cb(f);
        self
    }
    pub fn screen(mut self, f: impl Fn(&mut Ui) + Send + Sync + 'static) -> Self {
        self.desc.screen = ui_cb(f);
        self
    }
}

impl Drop for MenuBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        self.desc.name = Str::new(&self.name);
        self.desc.cell = self.handle.map_or(std::ptr::null(), |h| h.id_cell());
        let groups = self.groups.descs();
        self.desc.groups = Slice::new(&groups);
        self.desc.entity = Str::new(&self.entity);
        let recipes = self.recipes.join(" ");
        self.desc.recipes = Str::new(&recipes);
        unsafe { (host().reg_menu)(self.reg.raw, &self.desc) }
    }
}

impl Registry {
    pub fn entity<'a>(&'a mut self, kind: &'static EntityKind) -> EntityBuilder<'a> {
        let d = abi::EntityDesc::DEFAULT;
        EntityBuilder {
            no_egg: false,
            name: kind.name().to_string(),
            reg: self,
            handle: Some(kind),
            components: Vec::new(),
            width: d.width,
            height: d.height,
            eye: None,
            speed: d.speed,
            flags: d.flags,
            texture: crate::asset("entity", kind.name()),
            texture_size: d.texture_size,
            parts: Vec::new(),
            on_tick: abi::Callback::NONE,
            on_interact: abi::Callback::NONE,
            replace: false,
            carry_height: 0.0,
            category_name: String::new(),
            template: String::new(),
            slots: d.slots,
            attached: Vec::new(),
        }
    }

    /// Declares `comp` for anything that holds a value of its own: entities (as
    /// `entity_component` does) and stacks (as `stack_component`). With
    /// `component_flags::CARRIED` the value goes from a broken machine onto its item and back
    /// when placed, as a renamed chest's name does.
    pub fn instance_component<T: Value>(&mut self, comp: &Component<T>, default: T, flags: u32) {
        self.entity_component(comp, default, flags);
        let no_save = if flags & component_flags::SAVE == 0 { abi::stack_flags::NO_SAVE } else { 0 };
        self.stack_component(comp, no_save);
    }

    /// Declares per-entity storage for `comp`: the value an entity starts with and the
    /// `component_flags` (saved, synced to whom). Kinds carry it with `.attach(&COMP)` (or
    /// `r.attach(&KIND, &COMP)` for another mod's kind).
    pub fn entity_component<T: Value>(&mut self, comp: &Component<T>, default: T, flags: u32) {
        let bytes = crate::component::value_bytes(&default);
        let schema = T::schema();
        let desc = abi::EntityComponentDesc {
            name: Str::new(comp.name()),
            schema: Str::new(&schema),
            value_size: size_of::<T>() as u32,
            flags,
            default: Slice::new(&bytes),
            ..abi::EntityComponentDesc::DEFAULT
        };
        unsafe { (host().reg_entity_component)(self.raw, &desc) }
    }

    /// Logic that is nobody's own (players' hunger, a mod's weather): once per tick in every
    /// region of every dimension, where `w.players()` are the players in that region. Another
    /// mod's entities are reached through events (`r.on(&actions::HURT, ..)`) and questions.
    pub fn world_tick(&mut self, f: impl Fn(&World) + Send + Sync + 'static) {
        let cb = crate::callback(move |a: &mut abi::WorldTickArgs| f(&World::from_raw(a.world)));
        unsafe { (host().reg_world_tick)(self.raw, cb) }
    }

    pub fn menu<'a>(&'a mut self, menu: &'static Menu) -> MenuBuilder<'a> {
        MenuBuilder {
            name: menu.name().to_string(),
            reg: self,
            handle: Some(menu),
            desc: abi::MenuDesc::DEFAULT,
            groups: GroupList::new(),
            entity: String::new(),
            recipes: Vec::new(),
        }
    }

    /// The player inventory as named groups, one of them `.hotbar()`:
    /// `r.player_inventory().group("hotbar", Slots::row(9).hotbar()).group("main", Slots::grid(9, 3))`.
    /// The engine lays them out, hotbar first, the equipment slots (`equipment_slot`) after
    /// everything; `w.inventory(player).group(name)` reads a group, `core:equipment` all of the
    /// equipment. The mod loaded last wins.
    pub fn player_inventory(&mut self) -> PlayerInventoryBuilder<'_> {
        PlayerInventoryBuilder { raw: self.raw, groups: GroupList::new(), _p: std::marker::PhantomData }
    }

    /// An equipment slot of players (head, chest...): what is worn there covers the bones named
    /// in `covers` (`worn::CHEST`, `&["leftWing", "rightWing"]`); a name with a role (`head`,
    /// `leftArm`, `leg0`) covers every bone of that role. `background` (from `gui/`) shows while
    /// it is empty. Items go there with `.wearable(&SLOT, ..)`; menus show the slots with
    /// `.equipment()`.
    pub fn equipment_slot(&mut self, slot: &EquipmentSlot, covers: &[&str], background: &str) {
        let bg = crate::asset("gui", background);
        let bones = covers.join(" ");
        unsafe { (host().reg_equipment_slot)(self.raw, Str::new(slot.name()), Str::new(&bones), Str::new(&bg)) }
    }

    /// The menu the inventory key opens.
    pub fn player_menu(&mut self, menu: &Menu) {
        unsafe { (host().set_player_menu)(self.raw, Str::new(menu.name())) }
    }

    /// Gives `block` a block entity of kind `entity` with an inventory of `slots` (what
    /// `BlockBuilder::inventory` and `machine` do).
    pub(crate) fn block_entity(&mut self, block: &Block, entity: &EntityKind, slots: u32) {
        unsafe { (host().set_block_entity)(self.raw, Str::new(block.name()), Str::new(entity.name()), slots) }
    }

    /// Every entity of `kind` (any mod's, `&PLAYER`) carries `component`, starting at its
    /// declared value.
    pub fn attach<T: Value>(&mut self, kind: &EntityKind, component: &Component<T>) {
        unsafe { (host().reg_attach)(self.raw, Str::new(kind.name()), Str::new(component.name()), Slice::EMPTY) }
    }

    /// `attach` with another starting value (a battery's capacity).
    pub fn attach_with<T: Value>(&mut self, kind: &EntityKind, component: &Component<T>, value: T) {
        let bytes = crate::component::value_bytes(&value);
        unsafe { (host().reg_attach)(self.raw, Str::new(kind.name()), Str::new(component.name()), Slice::new(&bytes)) }
    }

    /// Chat command `/name` run on the server; `usage` declares its arguments (`<name:type>`,
    /// `[name:type]`, literal words, `|` alternatives), which the engine parses, checks and
    /// completes. An engine command of that name (`/give`, `/tp` ...) is replaced by it.
    ///
    /// It asks for `permission::GAME_MASTER` unless told otherwise, and players below that are
    /// told so and not offered it: `r.command("rename", "<name:text>", f).permission(permission::EVERYONE)`.
    pub fn command(
        &mut self,
        name: &str,
        usage: &str,
        f: impl Fn(&mut crate::ServerCommand) + Send + Sync + 'static,
    ) -> crate::CommandOptions {
        let cb = crate::command::server_cb(f);
        unsafe { (host().reg_command)(self.raw, Str::new(name), Str::new(usage), 0, cb) };
        crate::CommandOptions::new(self.raw, name)
    }

    /// Like `command`, but runs on the client that types it (`cmd.client()`), never on the
    /// server: for what only the player's own game has (views, local settings, debug output).
    pub fn client_command(&mut self, name: &str, usage: &str, f: impl Fn(&mut crate::ClientCommand) + Send + Sync + 'static) {
        let cb = crate::command::client_cb(f);
        unsafe { (host().reg_command)(self.raw, Str::new(name), Str::new(usage), 1, cb) }
    }

    /// Adds words to a word set: command parameters `word(#set)` complete from it. The
    /// engine's are `core:rules` and `core:gizmos` (or see `gizmo_group`); a mod's command can
    /// declare its own set that other mods add to.
    pub fn add_words(&mut self, set: &str, words: &[&str]) {
        let words = words.join(",");
        unsafe { (host().reg_words)(self.raw, Str::new(set), Str::new(&words)) }
    }

    /// A gizmo group of this mod, offered by `/gizmo`'s completion.
    pub fn gizmo_group(&mut self, group: &GizmoGroup) {
        self.add_words("core:gizmos", &[group.name()]);
    }

    /// A new branch of an existing server command, handled here: `r.command_branch("time",
    /// "dawn", |cmd| ..)` makes `/time dawn` this mod's. Earlier alternatives win when both fit.
    pub fn command_branch(&mut self, command: &str, usage: &str, f: impl Fn(&mut crate::ServerCommand) + Send + Sync + 'static) {
        let cb = crate::command::server_cb(f);
        unsafe { (host().reg_command_branch)(self.raw, Str::new(command), Str::new(usage), 0, cb) }
    }

    /// The same for a client command (`/gizmo`, `/log`, a mod's `client_command`): `r.
    /// client_command_branch("gizmo", "clear", |cmd| ..)`.
    pub fn client_command_branch(&mut self, command: &str, usage: &str, f: impl Fn(&mut crate::ClientCommand) + Send + Sync + 'static) {
        let cb = crate::command::client_cb(f);
        unsafe { (host().reg_command_branch)(self.raw, Str::new(command), Str::new(usage), 1, cb) }
    }

    /// Adds to place `area` of `screen` without replacing the screen: a button in the main menu is
    /// `r.screen_part(&screens::TITLE, "buttons", 0, |ui| ..)`. Lower `order` comes first. The
    /// engine's places: `TITLE` `buttons` and `side`, `PAUSE` `buttons`, `SETTINGS` `buttons`.
    pub fn screen_part(&mut self, screen: &Screen, area: &str, order: i32, f: impl Fn(&mut Ui) + Send + Sync + 'static) {
        let cb = crate::ui::ui_cb(f);
        unsafe { (host().reg_screen_part)(self.raw, Str::new(screen.name()), Str::new(area), order, cb) }
    }

    /// A screen opened with `Client::screen_open` (or `--screen <name>`); registering an engine
    /// screen's handle (`screens::PAUSE`) replaces that one. How it behaves is declared on it,
    /// the same properties the engine's screens have: `r.screen(&screens::PAUSE, build).pauses()`.
    pub fn screen<'a>(&'a mut self, screen: &Screen, f: impl Fn(&mut Ui) + Send + Sync + 'static) -> ScreenBuilder<'a> {
        ScreenBuilder {
            raw: self.raw,
            name: screen.name().to_string(),
            desc: abi::ScreenDesc { build: ui_cb(f), ..abi::ScreenDesc::DEFAULT },
            _p: std::marker::PhantomData,
        }
    }

    /// HUD element at `anchor` (`hud_anchor::*`); `order` 0 sits nearest the anchor's edge.
    pub fn hud_element(&mut self, element: &HudElement, anchor: &str, order: i32, f: impl Fn(&mut Ui) + Send + Sync + 'static) {
        unsafe { (host().reg_hud_element)(self.raw, Str::new(element.name()), Str::new(anchor), order, ui_cb(f)) }
    }

    /// Shaped recipe: `pattern` rows use the characters in `keys`; ingredients are item names or `#tag`.
    /// The recipe viewer shows `item` (a machine or workstation) next to recipes of type `kind`.
    /// A tint kind: blocks with `.tint(&KIND)` take its color from the biome they stand in
    /// (`biome_tint`), `default` (0xRRGGBB) elsewhere and on items.
    pub fn tint_kind(&mut self, kind: &TintKind, default: u32) {
        unsafe { (host().reg_tint_kind)(self.raw, Str::new(kind.name()), default) }
    }

    /// The color of a tint kind in a biome; either may be another mod's.
    pub fn biome_tint(&mut self, biome: &Biome, kind: &TintKind, color: u32) {
        unsafe { (host().reg_biome_tint)(self.raw, Str::new(biome.name()), Str::new(kind.name()), color) }
    }

    /// An entity category: at most `cap` of it spawn naturally near a player; with `despawn` > 0
    /// they vanish when no player is within that many blocks.
    pub fn entity_category(&mut self, category: &EntityCategory, cap: u32, despawn: f32) -> CategoryBuilder<'_> {
        CategoryBuilder { raw: self.raw, name: category.name().to_string(), cap, despawn, cap_radius: 64.0, _p: std::marker::PhantomData }
    }

    /// Registers a recipe kind this mod owns, and how the recipe viewer shows it.
    pub fn recipe_kind<'a>(&'a mut self, kind: &RecipeKind) -> RecipeKindBuilder<'a> {
        RecipeKindBuilder {
            raw: self.raw,
            name: kind.name().to_string(),
            input_grid: [0; 2],
            output_grid: [0; 2],
            info: String::new(),
            info_values: Vec::new(),
            display: abi::Callback::NONE,
            _p: std::marker::PhantomData,
        }
    }

    /// Shaped recipe: `pattern` rows use the characters in `keys`; ingredients are item names or
    /// `#tag`. More results and data with the builder it returns.
    pub fn shaped<'a>(
        &'a mut self,
        kind: &RecipeKind,
        output: &str,
        count: u32,
        pattern: &[&str],
        keys: &[(char, &str)],
    ) -> RecipeBuilder<'a> {
        let keys = keys.iter().map(|(c, n)| (*c, self.ingredient(n))).collect();
        let output = self.ingredient(output);
        RecipeBuilder {
            kind: kind.name().to_string(),
            pattern: pattern.iter().map(|p| p.to_string()).collect(),
            keys,
            inputs: Vec::new(),
            counts: Vec::new(),
            output,
            count,
            time: 0,
            outputs: Vec::new(),
            values: Vec::new(),
            flags: 0,
            reg: self,
        }
    }

    /// Shapeless recipe or a processing recipe (inputs used up over `time` ticks). Inputs are
    /// items or `#tags`; a liquid is its item, named like it, counted in millibuckets.
    pub fn shapeless<'a>(&'a mut self, kind: &RecipeKind, output: &str, count: u32, inputs: &[&str], time: u32) -> RecipeBuilder<'a> {
        let inputs = inputs.iter().map(|n| self.ingredient(n)).collect();
        let output = self.ingredient(output);
        RecipeBuilder {
            kind: kind.name().to_string(),
            pattern: Vec::new(),
            keys: Vec::new(),
            inputs,
            counts: Vec::new(),
            output,
            count,
            time,
            outputs: Vec::new(),
            values: Vec::new(),
            flags: 0,
            reg: self,
        }
    }

    fn ingredient(&self, n: &str) -> String {
        if n.starts_with('#') { n.to_string() } else { qualify(self.ns, n) }
    }
}

/// A recipe being registered; sent when dropped.
pub struct RecipeBuilder<'a> {
    reg: &'a mut Registry,
    kind: String,
    pattern: Vec<String>,
    keys: Vec<(char, String)>,
    inputs: Vec<String>,
    counts: Vec<u32>,
    output: String,
    count: u32,
    time: u32,
    outputs: Vec<(String, u32, f32)>,
    values: Vec<Pending>,
    flags: u32,
}

impl RecipeBuilder<'_> {
    /// How many of each input it takes, in the order of the inputs (millibuckets for liquids).
    pub fn input_amounts(mut self, counts: &[u32]) -> Self {
        self.counts = counts.to_vec();
        self
    }
    /// Another result (an item, or a liquid by its name and millibuckets).
    pub fn output(self, item: &str, count: u32) -> Self {
        self.output_chance(item, count, 1.0)
    }
    /// A result that comes out with chance `chance` (0..1) each time.
    pub fn output_chance(mut self, item: &str, count: u32, chance: f32) -> Self {
        let item = self.reg.ingredient(item);
        self.outputs.push((item, count, chance));
        self
    }
    /// Data the recipe carries for the machine making it (energy per tick); the recipe
    /// viewer can show it in the kind's info line.
    pub fn value<T: Value>(mut self, comp: &Component<T>, v: T) -> Self {
        self.values.push(comp.raw_value(&v));
        self
    }
    /// Works, but the recipe viewer doesn't list it.
    pub fn hidden(mut self) -> Self {
        self.flags |= abi::recipe_flags::HIDDEN;
        self
    }
}

impl Drop for RecipeBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let pattern: Vec<Str> = self.pattern.iter().map(|p| Str::new(p)).collect();
        let keys: Vec<abi::RecipeKey> = self
            .keys
            .iter()
            .map(|(c, n)| abi::RecipeKey { ch: *c as u32, item: Str::new(n) })
            .collect();
        let inputs: Vec<Str> = self.inputs.iter().map(|n| Str::new(n)).collect();
        let outputs: Vec<abi::RecipeOutput> = self
            .outputs
            .iter()
            .map(|(n, c, ch)| abi::RecipeOutput { item: Str::new(n), count: *c, chance: *ch })
            .collect();
        let mut scratch = Vec::new();
        let values = crate::content::raw_values(&self.values, &mut scratch);
        let desc = abi::RecipeDesc {
            kind: Str::new(&self.kind),
            pattern: Slice::new(&pattern),
            keys: Slice::new(&keys),
            inputs: Slice::new(&inputs),
            input_counts: Slice::new(&self.counts),
            output: Str::new(&self.output),
            count: self.count,
            time: self.time,
            outputs: Slice::new(&outputs),
            values: Slice::new(&values),
            flags: self.flags,
        };
        unsafe { (host().reg_recipe)(self.reg.raw, &desc) }
    }
}

/// A recipe kind being registered; sent when dropped.
pub struct RecipeKindBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    input_grid: [u32; 2],
    output_grid: [u32; 2],
    info: String,
    info_values: Vec<String>,
    display: abi::Callback,
    _p: std::marker::PhantomData<&'a mut Registry>,
}

impl RecipeKindBuilder<'_> {
    /// The recipe viewer shows inputs as a grid this big, whatever a recipe uses (a crafting
    /// table's 3 x 3).
    pub fn inputs(mut self, cols: u32, rows: u32) -> Self {
        self.input_grid = [cols, rows];
        self
    }
    pub fn outputs(mut self, cols: u32, rows: u32) -> Self {
        self.output_grid = [cols, rows];
        self
    }
    /// A line under each recipe: translation `key`, its `{0}`.. filled with `values`: `time`
    /// (seconds) or components the recipe carries (`mymod:energy_per_tick`).
    pub fn info_line(mut self, key: &str, values: &[&str]) -> Self {
        self.info = key.to_string();
        self.info_values = values.iter().map(|v| v.to_string()).collect();
        self
    }
    /// Builds each recipe yourself instead of the usual inputs, arrow, outputs:
    /// `ui.recipe_inputs()`, `ui.recipe_outputs()` and `ui.recipe_input(i)` place its slots.
    pub fn layout(mut self, f: impl Fn(&mut Ui, &Recipe) + Send + Sync + 'static) -> Self {
        self.display = callback(move |a: &mut abi::RecipeUiArgs| {
            let mut ui = Ui::from_raw(a.ui, a.client);
            let r = Recipe { kind: unsafe { a.kind.as_str() }.to_string(), index: a.recipe };
            f(&mut ui, &r);
        });
        self
    }
}

impl Drop for RecipeKindBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let values: Vec<Str> = self.info_values.iter().map(|v| Str::new(v)).collect();
        let desc = abi::RecipeKindDesc {
            name: Str::new(&self.name),
            input_grid: self.input_grid,
            output_grid: self.output_grid,
            info: Str::new(&self.info),
            info_values: Slice::new(&values),
            layout: self.display,
        };
        unsafe { (host().reg_recipe_kind)(self.raw, &desc) }
    }
}

/// One recipe of a kind, found by `recipe_find` (or shown by a kind's `display`).
pub struct Recipe {
    kind: String,
    index: u32,
}

/// The first recipe of `kind` whose inputs, with their counts, are all among `stacks`.
pub fn recipe_find(kind: &RecipeKind, stacks: &[ItemStack]) -> Option<Recipe> {
    let n = unsafe { (host().recipe_find)(Str::new(kind.name()), stacks.as_ptr(), stacks.len() as u32) };
    (n > 0).then(|| Recipe { kind: kind.name().to_string(), index: n - 1 })
}

impl Recipe {
    fn info(&self) -> abi::RecipeInfo {
        let mut out = abi::RecipeInfo::default();
        unsafe { (host().recipe_info)(Str::new(&self.kind), self.index, &mut out) };
        out
    }
    /// Its time in ticks.
    pub fn time(&self) -> u32 {
        self.info().time
    }
    /// Its results with their chances; a liquid's is its item by the millibucket.
    pub fn outputs(&self) -> Vec<(ItemStack, f32)> {
        (0..self.info().outputs)
            .map(|i| {
                let (mut s, mut chance) = (ItemStack::EMPTY, 1.0);
                unsafe { (host().recipe_output)(Str::new(&self.kind), self.index, i, &mut s, &mut chance) };
                (s, chance)
            })
            .collect()
    }
    /// A value given with `RecipeBuilder::value`.
    pub fn value<T: Value>(&self, comp: &Component<T>) -> Option<T> {
        let mut buf = vec![0u8; std::mem::size_of::<T>()];
        let n =
            unsafe { (host().recipe_value)(Str::new(&self.kind), self.index, Str::new(comp.name()), buf.as_mut_ptr(), buf.len() as u32) };
        (n as usize == buf.len()).then(|| unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const T) })
    }
    /// Takes its inputs out of `stacks` (the ones `recipe_find` was given); false, and nothing
    /// taken, if they are not all there.
    pub fn consume(&self, stacks: &mut [ItemStack]) -> bool {
        unsafe { (host().recipe_consume)(Str::new(&self.kind), self.index, stacks.as_mut_ptr(), stacks.len() as u32) != 0 }
    }
}

/// An entity category being registered (`Registry::entity_category`).
pub struct CategoryBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    cap: u32,
    despawn: f32,
    cap_radius: f32,
    _p: std::marker::PhantomData<&'a mut Registry>,
}

impl CategoryBuilder<'_> {
    /// How close to a player its mobs count for the cap (64 by default).
    pub fn cap_radius(mut self, blocks: f32) -> Self {
        self.cap_radius = blocks;
        self
    }
}

impl Drop for CategoryBuilder<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            unsafe { (host().reg_entity_category)(self.raw, Str::new(&self.name), self.cap, self.despawn, self.cap_radius) }
        }
    }
}

/// What the base game's armor slots cover (`Registry::equipment_slot`): bones by role, so they fit
/// any model with bones of those names.
pub mod worn {
    pub const HEAD: &[&str] = &["head"];
    pub const CHEST: &[&str] = &["body", "leftArm", "rightArm"];
    pub const LEGS: &[&str] = &["body", "leftLeg", "rightLeg", "leg2", "leg3"];
    pub const FEET: &[&str] = &["leftLeg", "rightLeg", "leg2", "leg3"];
}

/// Where a natural spawn stands (`SpawnBuilder::place`).
#[derive(Clone, Copy, Debug)]
pub struct SpawnPlace {
    kind: u32,
    tag: Option<&'static str>,
}

impl SpawnPlace {
    /// On a solid block, in air.
    pub fn ground() -> SpawnPlace {
        SpawnPlace { kind: abi::spawn_place::GROUND, tag: None }
    }
    /// Inside a liquid with this tag (`"#water"`): fish, squid.
    pub fn in_liquid(tag: &'static str) -> SpawnPlace {
        SpawnPlace { kind: abi::spawn_place::IN_LIQUID, tag: Some(tag) }
    }
    /// Hanging under a solid block: bats.
    pub fn ceiling() -> SpawnPlace {
        SpawnPlace { kind: abi::spawn_place::CEILING, tag: None }
    }
}

/// A natural spawning rule (see `abi::SpawnDesc`):
/// `r.spawn("pig").biome("#animals").ground("#animal_spawn").light(9, 15).group(2, 3).chance(0.05)`.
pub struct SpawnBuilder<'a> {
    raw: *mut abi::Registrar,
    names: [String; 4],
    condition: String,
    desc: abi::SpawnDesc,
    _p: std::marker::PhantomData<&'a mut Registry>,
}

impl Registry {
    pub fn natural_spawn<'a>(&'a mut self, entity: &EntityKind) -> SpawnBuilder<'a> {
        let entity = entity.name().to_string();
        SpawnBuilder {
            raw: self.raw,
            names: [entity, String::new(), String::new(), String::new()],
            condition: String::new(),
            desc: abi::SpawnDesc::DEFAULT,
            _p: std::marker::PhantomData,
        }
    }
}

impl SpawnBuilder<'_> {
    /// Only in biomes with this tag.
    pub fn biome(mut self, tag: &str) -> Self {
        self.names[1] = tag.to_string();
        self
    }
    /// Only on blocks with this tag (under the spot; what `place` says it rests on).
    pub fn ground(mut self, tag: &str) -> Self {
        self.names[2] = tag.to_string();
        self
    }
    /// What kind of spot it takes: on the ground (the default), in a liquid, under a ceiling.
    /// The mob's height decides how much room it needs.
    pub fn place(mut self, place: SpawnPlace) -> Self {
        self.desc.place = place.kind;
        if let Some(t) = place.tag {
            self.names[2] = t.to_string();
        }
        self
    }
    /// Full dimension name.
    pub fn dimension(mut self, name: &str) -> Self {
        self.names[3] = name.to_string();
        self
    }
    /// Only where this condition holds too.
    pub fn when(mut self, condition: &crate::Condition) -> Self {
        self.condition = condition.name().to_string();
        self
    }
    /// Light at the spot, inclusive (0..15).
    pub fn light(mut self, min: u32, max: u32) -> Self {
        self.desc.light = [min, max];
        self
    }
    pub fn y(mut self, min: i32, max: i32) -> Self {
        self.desc.y = [min, max];
        self
    }
    pub fn pack_size(mut self, min: u32, max: u32) -> Self {
        self.desc.group = [min, max];
        self
    }
    pub fn weight(mut self, w: u32) -> Self {
        self.desc.weight = w;
        self
    }
    pub fn chance(mut self, c: f32) -> Self {
        self.desc.chance = c;
        self
    }
}

impl Drop for SpawnBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let [entity, biome, ground, dimension] = &self.names;
        let desc = abi::SpawnDesc {
            entity: Str::new(entity),
            biome: Str::new(biome),
            ground: Str::new(ground),
            dimension: Str::new(dimension),
            condition: Str::new(&self.condition),
            ..self.desc
        };
        unsafe { (host().reg_spawn)(self.raw, &desc) }
    }
}

type BlockLook = Box<dyn for<'r> FnOnce(crate::BlockBuilder<'r>) -> crate::BlockBuilder<'r>>;

/// A machine in one go: the block (its look and mining from `block`), an entity of the same
/// name holding its inventory and state (ticking with `on_tick`), components on that entity,
/// and the menu showing its slot groups above the player's inventory, opened by using the block.
///
/// ```ignore
/// r.machine(&FURNACE, &FURNACE_MENU)
///     .block(|b| b.textures(..).hardness(3.5))
///     .group("input", Slots::single())
///     .group("fuel", Slots::single().accepts("fuel"))
///     .group("output", Slots::single().output().large())
///     .attach(&SMELTING)
///     .screen(|ui| ..);
/// r.system(&SMELTING, tick_furnaces);   // the work: every furnace's state at once
/// ```
pub struct MachineBuilder<'a> {
    reg: &'a mut Registry,
    block_handle: &'static Block,
    menu: &'static Menu,
    block: Option<BlockLook>,
    groups: Vec<(String, Slots)>,
    recipes: Vec<String>,
    with: Vec<Box<dyn FnOnce(&mut Registry, &EntityKind)>>,
    screen: Option<Box<dyn Fn(&mut Ui) + Send + Sync>>,
    plain_screen: bool,
}

impl Registry {
    /// The machine's entity kind is named like its block: `EntityKind::named("furnace")`.
    pub fn machine<'a>(&'a mut self, block: &'static Block, menu: &'static Menu) -> MachineBuilder<'a> {
        MachineBuilder {
            block_handle: block,
            reg: self,
            menu,
            block: None,
            groups: Vec::new(),
            recipes: Vec::new(),
            with: Vec::new(),
            plain_screen: false,
            screen: None,
        }
    }
}

impl MachineBuilder<'_> {
    pub fn block(mut self, f: impl for<'r> FnOnce(crate::BlockBuilder<'r>) -> crate::BlockBuilder<'r> + 'static) -> Self {
        self.block = Some(Box::new(f));
        self
    }
    /// Recipes of this kind are made here: clicking an `arrow` bar of its screen shows them, and
    /// the recipe viewer shows the machine next to them.
    pub fn recipes(mut self, kind: &RecipeKind) -> Self {
        self.recipes.push(kind.name().to_string());
        self
    }
    /// A group of the machine's slots (its inventory holds them all, in this order). The
    /// machine's tick finds them with `w.inventory(id).group(name)`.
    pub fn group(mut self, name: &str, slots: Slots) -> Self {
        self.groups.push((name.to_string(), slots));
        self
    }
    /// Every machine of this kind carries `component` (declared with `r.entity_component`),
    /// starting at its declared value.
    pub fn attach<T: Value>(mut self, component: &'static Component<T>) -> Self {
        self.with
            .push(Box::new(move |r, kind| r.attach(kind, component)));
        self
    }
    /// `attach` with the value this machine starts with.
    pub fn attach_with<T: Value>(mut self, component: &'static Component<T>, value: T) -> Self {
        self.with
            .push(Box::new(move |r, kind| r.attach_with(kind, component, value)));
        self
    }
    /// The menu screen: the machine's groups by name (`ui.group`), the player's inventory with
    /// `ui.player_inventory(..)`.
    pub fn screen(mut self, f: impl Fn(&mut Ui) + Send + Sync + 'static) -> Self {
        self.screen = Some(Box::new(f));
        self
    }
    /// The usual menu screen: its title, its groups in order, the player's inventory.
    pub fn default_screen(mut self) -> Self {
        self.plain_screen = true;
        self
    }
}

impl Drop for MachineBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let (block, menu, reg) = (self.block_handle, self.menu, &mut *self.reg);
        let full = block.name().to_string();
        let kind = EntityKind::generated(full.clone());
        let look = self.block.take().unwrap_or_else(|| Box::new(|b| b));
        look(reg.block(block)).opens(menu);
        drop(
            reg.entity(kind)
                .no_egg()
                .flags(entity_flags::NO_GRAVITY | entity_flags::INVULNERABLE),
        );
        for f in self.with.drain(..) {
            f(reg, kind);
        }
        // The inventory holds every group of the machine's own, wherever they start.
        let mut size = 0;
        let mut next = 0;
        for (_, s) in &self.groups {
            if s.owner.is_none_or(|o| o == abi::slot_owner::MACHINE) {
                let start = if s.start == u32::MAX { next } else { s.start };
                next = start + s.cols * s.rows;
                size = size.max(next);
            }
        }
        reg.block_entity(block, kind, size);
        if self.plain_screen {
            let names: Vec<String> = self.groups.iter().map(|g| g.0.clone()).collect();
            self.screen = Some(Box::new(move |ui| {
                ui.panel("panel", crate::ui::S::column().pad(7.0).gap(4.0), |ui| {
                    ui.title();
                    for name in &names {
                        ui.group(name);
                    }
                    ui.player_inventory(true);
                });
            }));
        }
        let mut m = reg.menu(menu);
        for (g, s) in self.groups.drain(..) {
            let ns = m.reg.ns;
            m.groups.push(ns, &g, s, abi::slot_owner::MACHINE);
        }
        m.entity = full.clone();
        m.recipes = std::mem::take(&mut self.recipes);
        let m = m.player_inventory();
        match self.screen.take() {
            Some(s) => drop(m.screen(s)),
            None => drop(m),
        }
    }
}

/// The engine's entity categories (`.hostile()` and `.passive()` pick the first two).
pub mod entity_categories {
    use crate::EntityCategory;
    pub static HOSTILE: EntityCategory = EntityCategory::named("core:hostile");
    pub static PASSIVE: EntityCategory = EntityCategory::named("core:passive");
    pub static MISC: EntityCategory = EntityCategory::named("core:misc");
}

pub struct ScreenBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    desc: abi::ScreenDesc,
    _p: std::marker::PhantomData<&'a mut Registry>,
}

impl ScreenBuilder<'_> {
    /// Single player stops while it is open (the pause and settings screens).
    pub fn pauses(mut self) -> Self {
        self.desc.flags |= abi::screen_flags::PAUSES;
        self
    }
    /// Escape doesn't close it (the death screen).
    pub fn keep_on_escape(mut self) -> Self {
        self.desc.flags |= abi::screen_flags::KEEP_ON_ESCAPE;
        self
    }
    /// It closes when the player respawns.
    pub fn closes_on_respawn(mut self) -> Self {
        self.desc.flags |= abi::screen_flags::CLOSES_ON_RESPAWN;
        self
    }
    /// Like the chat screen: the HUD leaves the messages to it and Enter sends the text of its
    /// input of class `chat`.
    pub fn shows_chat(mut self) -> Self {
        self.desc.flags |= abi::screen_flags::SHOWS_CHAT;
        self
    }
    /// Keys pressed while it is on top, by their settings name (`Enter`, `KeyE`, `mouse3`), before
    /// Escape and the other global keys.
    pub fn on_key(mut self, f: impl Fn(&crate::ClientView, &str) + Send + Sync + 'static) -> Self {
        self.desc.on_key =
            crate::callback(move |a: &mut abi::ScreenKeyArgs| f(&crate::ClientView::from_raw(a.client), unsafe { a.key.as_str() }));
        self
    }
}

impl Drop for ScreenBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let desc = abi::ScreenDesc { name: Str::new(&self.name), ..self.desc };
        unsafe { (host().reg_screen)(self.raw, &desc) }
    }
}

/// What an entity looks at (`World::target_block`): a block of the world (`body` 0) or of a
/// moving body (`body` its entity, `pos` in its blocks).
#[derive(Clone, Copy, Debug)]
pub struct LookAt {
    pub pos: [i32; 3],
    pub face: u32,
    pub state: u32,
    pub body: u64,
}
