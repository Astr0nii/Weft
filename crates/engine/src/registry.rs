//! Gameplay registries: entity kinds, components, menus, recipes, HUD parts. Frozen together with
//! blocks and items, ids again assigned by sorted name.

use std::sync::atomic::{AtomicU32, Ordering};

use rustc_hash::FxHashMap;
use weft_abi::{self as abi, Callback, ModId, entity_flags as ef};

use crate::components::{self as comps, Components, RawSet, RawValue};

pub const ENGINE: ModId = ModId(u32::MAX);
pub const PLAYER: &str = "core:player";

#[derive(Clone)]
pub struct HudElement {
    pub name: String,
    /// Translation key of its F3 panel title: the name with `.` for `:`.
    pub title: String,
    pub anchor: String,
    pub order: i32,
    pub owner: ModId,
    pub cb: Callback,
}
pub const ITEM: &str = "core:item";
/// A block falling as an entity (`Host::fall_block`), placed again where it lands.
pub const FALLING: &str = "core:falling_block";
pub const MOVING: &str = "core:moving_blocks";
/// What a floater carries, and what `World::lift` makes.
pub const PLATFORM: &str = "core:platform";

#[derive(Clone, Debug)]
pub struct Cuboid {
    /// Model pixels, relative to the part's pivot.
    pub origin: [f32; 3],
    pub size: [f32; 3],
    pub uv: [f32; 2],
    pub inflate: f32,
    pub mirror: bool,
    /// Own texel rectangle per face (-X, +X, -Y, +Y, -Z, +Z), instead of the box unwrap.
    pub faces: Option<[[f32; 4]; 6]>,
}

impl Cuboid {
    /// Texel rectangles of the faces (-X, +X, -Y, +Y, -Z, +Z): its own, or the classic box
    /// unwrap (mirrored: each face flipped and the side faces swapped).
    pub fn face_rects(&self) -> [[f32; 4]; 6] {
        if let Some(f) = self.faces {
            return f;
        }
        let [w, h, d] = self.size;
        let [u, v] = self.uv;
        // Wrapped around the box as they lie in the unwrap: seen from the front (-Z), what is left
        // of it there is on its left, which is +X. The top lies above the front, so it is turned
        // to meet it. As Blockbench does it.
        let mut r = [
            [u + d + w, v + d, d, h],
            [u, v + d, d, h],
            [u + d + w, v, w, d],
            [u + d + w, v + d, -w, -d],
            [u + d, v + d, w, h],
            [u + d * 2.0 + w, v + d, w, h],
        ];
        if self.mirror {
            r.swap(0, 1);
            for f in &mut r {
                f[0] += f[2];
                f[2] = -f[2];
            }
        }
        r
    }
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    /// Index of the part it hangs from.
    pub parent: Option<u32>,
    pub pivot: [f32; 3],
    pub role: u32,
    pub cuboids: Vec<Cuboid>,
    pub rest: [f32; 3],
    /// Free-form triangles: model pixels (absolute, not pivot relative) and texels.
    pub mesh: Vec<weft_abi::MeshVertex>,
}

pub struct EntityKind {
    pub name: String,
    pub owner: ModId,
    pub width: f32,
    pub height: f32,
    pub eye: f32,
    pub speed: f32,
    pub flags: u32,
    pub category: u32,
    /// Set by the mod instead of `category`; resolved at freeze.
    pub category_name: String,
    pub texture: String,
    pub texture_size: [u32; 2],
    pub parts: Vec<Part>,
    /// The kind's own callbacks (empty or one).
    pub on_tick: Vec<(ModId, Callback)>,
    pub on_interact: Vec<(ModId, Callback)>,
    /// Components attached at spawn.
    pub components: Vec<u32>,
    /// Starting values of attached components for this kind, where they differ from the
    /// component's default.
    pub values: Vec<(u32, Box<[u8]>)>,
    /// Size of the inventory created with it (block entities), 0 = none.
    pub slots: u32,
    /// Its equipment slots in that inventory (`entity_flags::WEARS`, the player's): never filled
    /// by pickups, worn on the model. Laid out at freeze.
    pub worn: std::ops::Range<u32>,
    /// Sound events hurt, death, ambient, step (`u32::MAX` none), from its `sound` component.
    pub sounds: [u32; 4],
    /// Solid bodies: riders up to this high above it move with it.
    pub carry_height: f32,
    /// A moving structure: its template (`Content::templates`), by name until the freeze.
    pub template_name: String,
    pub template: Option<u32>,
    /// Where riders sit (`abi::SeatDesc`).
    pub seats: Vec<Seat>,
    /// What the keys of a player steering it do (`abi::SteerArgs`), and where riders get off
    /// when a seat asks it (`abi::DismountArgs`).
    pub steer: Option<(ModId, Callback)>,
    pub dismount: Option<(ModId, Callback)>,
    /// Blocks the third-person camera goes further back on it, `NAN`: by its size.
    pub ride_camera: f32,
}

/// A place on a mount, in its frame: +Z ahead.
#[derive(Clone, Copy, Debug)]
pub struct Seat {
    pub pos: glam::DVec3,
    pub steers: bool,
    /// `abi::seat_exit`, and its offset or reach.
    pub exit: u32,
    pub exit_at: glam::DVec3,
}

impl EntityKind {
    /// Its box standing, as `Entity::dims` starts from.
    pub fn size(&self) -> weft_abi::EntitySize {
        weft_abi::EntitySize { width: self.width, height: self.height, eye: self.eye }
    }

    pub fn is(&self, f: u32) -> bool {
        self.flags & f != 0
    }
}

/// Per-entity storage of a component (indexed by component id; `runtime` false for components
/// that only exist on content entries).
#[derive(Clone, Default)]
pub struct ComponentDef {
    pub name: String,
    pub size: u32,
    pub flags: u32,
    pub default: Vec<u8>,
    pub runtime: bool,
}

pub struct MenuDef {
    pub name: String,
    pub owner: ModId,
    /// Slots of the menu's own inventory (its `slot_owner::MENU` groups).
    pub temp_slots: u32,
    pub groups: Vec<GroupDef>,
    /// Recipe kinds its `arrow` bars open.
    pub recipes: Vec<String>,
    pub on_change: Option<Callback>,
    pub on_take: Option<Callback>,
    pub on_tick: Option<Callback>,
    pub screen: Option<Callback>,
}

/// A slot group (`abi::SlotGroupDesc`), its start resolved.
#[derive(Clone, Debug)]
pub struct GroupDef {
    pub name: String,
    /// `abi::slot_owner`.
    pub owner: u32,
    pub start: u32,
    pub cols: u32,
    pub rows: u32,
    pub flags: u32,
    /// Tag or component name, empty for any item.
    pub accepts: String,
    pub limit: u32,
    pub size: f32,
    pub background: Option<String>,
    pub tooltip: Option<String>,
    pub priority: i32,
}

impl GroupDef {
    /// (owner inventory start, slot count, columns); the player's presets follow `layout`.
    pub fn span(&self, layout: &PlayerLayout) -> (u32, u32, u32) {
        use weft_abi::slot_owner as so;
        match self.owner {
            so::PLAYER_MAIN => (layout.hotbar, layout.main().count() as u32, layout.hotbar.max(1)),
            so::PLAYER_HOTBAR => (0, layout.hotbar, layout.hotbar.max(1)),
            _ => (self.start, self.cols * self.rows, self.cols.max(1)),
        }
    }

    /// Owner inventory index of the group's `i`-th slot (the main inventory skips worn slots).
    pub fn index(&self, layout: &PlayerLayout, i: u32) -> u32 {
        let (start, _, _) = self.span(layout);
        if self.owner == weft_abi::slot_owner::PLAYER_MAIN { layout.main().nth(i as usize).unwrap_or(u32::MAX) } else { start + i }
    }
}

impl MenuDef {
    /// (group, its first menu slot, slot count) in menu slot order.
    pub fn slot_groups(&self, layout: &PlayerLayout) -> impl Iterator<Item = (&GroupDef, u32, u32)> + '_ {
        let mut at = 0;
        let layout = layout.clone();
        self.groups.iter().map(move |g| {
            let n = g.span(&layout).1;
            at += n;
            (g, at - n, n)
        })
    }

    pub fn group(&self, layout: &PlayerLayout, name: &str) -> Option<(&GroupDef, u32, u32)> {
        self.slot_groups(layout).find(|g| g.0.name == name)
    }

    /// The group of menu slot `slot` and the slot's place in it.
    pub fn group_of(&self, layout: &PlayerLayout, slot: u32) -> Option<(&GroupDef, u32)> {
        self.slot_groups(layout)
            .find(|g| slot >= g.1 && slot < g.1 + g.2)
            .map(|g| (g.0, slot - g.1))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Ingredient {
    Empty,
    Item(u32),
    /// Component id of the tag.
    Tag(u32),
    /// Any of these items (a tag pattern, `#ingots/*`).
    Any(Vec<u32>),
}

impl Ingredient {
    #[inline]
    pub fn matches(&self, item: u32, c: &Components) -> bool {
        match self {
            Ingredient::Empty => item == 0,
            Ingredient::Item(i) => *i == item,
            Ingredient::Tag(t) => item != 0 && c.has(comps::ITEM, item, *t),
            Ingredient::Any(list) => list.contains(&item),
        }
    }
}

pub struct Recipe {
    /// Shaped: width, height, cells (row major).
    pub shaped: Option<(u32, u32, Vec<Ingredient>)>,
    pub inputs: Vec<Ingredient>,
    /// How many of each input (millibuckets for fluids).
    pub input_counts: Vec<u32>,
    /// The main result first.
    pub outputs: Vec<RecipeOutput>,
    pub time: u32,
    /// (component, value) the recipe carries.
    pub values: Vec<(u32, Vec<u8>)>,
    /// Not shown in the recipe viewer.
    pub hidden: bool,
}

#[derive(Clone, Debug)]
pub struct RecipeOutput {
    pub what: Ingredient,
    pub count: u32,
    pub chance: f32,
}

impl Recipe {
    /// The main result as an item stack (empty when it is a fluid).
    pub fn result(&self) -> weft_abi::ItemStack {
        match self.outputs.first() {
            Some(RecipeOutput { what: Ingredient::Item(i), count, .. }) => weft_abi::ItemStack::new(*i, *count),
            _ => weft_abi::ItemStack::EMPTY,
        }
    }

    pub fn value(&self, comp: u32) -> Option<&[u8]> {
        self.values
            .iter()
            .find(|v| v.0 == comp)
            .map(|v| v.1.as_slice())
    }
}

/// How the recipe viewer shows a kind of recipe (`abi::RecipeKindDesc`).
pub struct RecipeKindDef {
    pub owner: ModId,
    pub input_grid: [u32; 2],
    pub output_grid: [u32; 2],
    pub info: Option<String>,
    pub info_values: Vec<String>,
    pub display: Option<Callback>,
}

pub struct RawRecipeKind {
    pub name: String,
    pub def: RecipeKindDef,
}

#[derive(Default)]
pub struct Game {
    /// Camera modes by id (`abi::CameraDesc`): the engine's (`camera::*`) first.
    pub cameras: Vec<CameraMode>,
    pub camera_names: FxHashMap<String, u32>,
    pub entities: Vec<EntityKind>,
    pub entity_names: FxHashMap<String, u32>,
    pub components: Vec<ComponentDef>,
    pub component_names: FxHashMap<String, u32>,
    pub menus: Vec<MenuDef>,
    pub menu_names: FxHashMap<String, u32>,
    pub recipes: FxHashMap<String, Vec<Recipe>>,
    pub recipe_kinds: rustc_hash::FxHashSet<String>,
    pub recipe_kind_defs: FxHashMap<String, RecipeKindDef>,
    /// Entity categories by `EntityKind::category`: the engine's three first.
    pub categories: Vec<CategoryDef>,
    /// By id: the engine's (`abi::damage_cause`) first.
    pub damage_causes: Vec<DamageCauseDef>,
    pub damage_cause_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::entity_effect`) first.
    pub entity_effects: Vec<EntityEffectDef>,
    pub entity_effect_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::show_place`) first.
    pub text_places: Vec<crate::hud::Place>,
    pub text_place_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::game_mode`) first.
    pub game_modes: Vec<GameModeDef>,
    pub game_mode_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::pose`) first.
    pub poses: Vec<PoseDef>,
    /// Poses by priority, highest first.
    pub pose_order: Vec<crate::world::Pose>,
    pub pose_names: FxHashMap<String, u32>,
    /// By id: the engine's (`abi::weather`) first.
    pub weathers: Vec<WeatherDef>,
    pub weather_names: FxHashMap<String, u32>,
    /// By id: rain, snow, then mods'.
    pub precipitations: Vec<PrecipitationDef>,
    pub precipitation_names: FxHashMap<String, u32>,
    /// Sound categories by `SoundDef::category`: the engine's six first.
    pub sound_categories: Vec<SoundCategory>,
    pub huds: Vec<HudElement>,
    pub chat_format: Option<(ModId, Callback)>,
    pub player_menu: Option<u32>,
    pub player_layout: PlayerLayout,
    /// Mods' screens; of two with one name the mod loaded last comes last.
    pub screens: Vec<ScreenDef>,
    pub screen_parts: Vec<ScreenPart>,
    /// Block index -> (entity kind, inventory slots).
    pub block_entities: FxHashMap<u32, u32>,
    pub player_kind: u32,
    pub item_kind: u32,
    pub falling_kind: u32,
    /// Blocks a piston is moving, an entity while they do.
    pub moving_kind: u32,
    pub platform_kind: u32,
    pub energy: u32,
    /// (owner, hook, callback) in mod order, see `abi::client_hook`.
    pub client_hooks: Vec<(ModId, u32, Callback)>,
    /// Every frame on the client, for entities of a kind, as many as mods add.
    pub client_entity_watchers: FxHashMap<u32, Vec<(ModId, Callback)>>,
    /// (entity kind's name, model, bone it goes on).
    pub attached_models: Vec<(String, String, String)>,
    /// (model, bone, how it moves).
    pub bone_motions: Vec<(String, String, crate::model::BoneMotion)>,
    /// Who says which model a stack of an item shows as.
    pub item_model_for_stack: FxHashMap<u32, (ModId, Callback)>,
    /// Mod message channels by id (sorted by name, so both sides agree).
    pub packets: Vec<PacketDef>,
    pub packet_names: FxHashMap<String, u32>,
    /// Mod key bindings: (action, default key).
    pub keys: Vec<(String, String)>,
    pub events: Vec<EventDef>,
    pub event_names: FxHashMap<String, u32>,
    pub interceptors: FxHashMap<String, Vec<(ModId, Callback)>>,
    /// Steps per chain in run order: (stage, owner, modifier).
    pub modifiers: FxHashMap<String, Vec<(u32, ModId, Callback)>>,
    pub attributes: Vec<AttrDef>,
    pub attr_names: FxHashMap<String, u32>,
    /// The engine component holding entities' attribute modifiers.
    pub modifiers_comp: u32,
    /// `TIMERS` as an entity component.
    pub timers_comp: u32,
    /// (owner, callback) run once per tick in every region, in mod order.
    pub world_ticks: Vec<(ModId, Callback)>,
    /// `abi::comp::NAME` as an entity component.
    pub name_comp: u32,
    /// Components with `component_flags::CARRIED`: between a block's item and its block entity.
    pub carried: Vec<u32>,
    /// `HEALTH`, `BURNING`, `WORN`, `SHAPE`, `HELD`, `OFF_HELD`, `USING`, `SWAPS`, `LOOPING_ACTION`,
    /// `EFFECT`, `FORCED_POSE`, `RIDING`.
    pub field_comps: [u32; 12],
    /// `SIMULATED`.
    pub simulated_comp: u32,
    /// Entity components with `SYNC_WATCHERS` / `SYNC_MENU`.
    pub watched_comps: Vec<u32>,
    pub menu_comps: Vec<u32>,
    /// Per component: synced to anyone at all (its changes are looked for every tick).
    pub synced_comps: Vec<bool>,
    /// Config id of `core:mob_spawning`.
    pub mob_spawning: u32,
    /// Config id of `core:keep_inventory`.
    pub keep_inventory: u32,
    /// Config id of `core:simulation_distance`.
    pub simulation_distance: u32,
    /// (owner, component, system) in mod order.
    pub systems: Vec<(ModId, u32, Callback)>,
    /// `{if:name}` conditions mods answer.
    /// By id: the engine's (`abi::condition`, answered by the engine) first, then mods'.
    pub conditions: Vec<ConditionDef>,
    pub condition_names: FxHashMap<String, u32>,
    /// What is in the sky of each dimension, in drawing order.
    pub sky: Vec<Vec<SkyDef>>,
    /// Particle kinds by id (sorted by name).
    pub particles: Vec<ParticleDef>,
    pub particle_names: FxHashMap<String, u32>,
    /// Particle texture names while freezing; the content moves them into its one list.
    pub particle_textures: Vec<String>,
    /// Block tags particle rules ask about, in the order of their bits.
    pub particle_tags: Vec<String>,
    /// Items shown with a recipe type in the recipe viewer (the machines making them).
    pub recipe_stations: FxHashMap<String, Vec<u32>>,
    /// Sound events by id (sorted by name).
    pub sounds: Vec<SoundDef>,
    pub sound_names: FxHashMap<String, u32>,
    /// Background music, in mod load order.
    pub music: Vec<MusicDef>,
    /// Natural spawning rules, in mod load order.
    pub spawns: Vec<SpawnRule>,
    /// Settings by id (sorted by name), see `config.rs`.
    pub configs: Vec<crate::config::ConfigDef>,
    pub config_names: FxHashMap<String, u32>,
}

pub struct SoundDef {
    pub name: String,
    pub files: Vec<String>,
    pub category: u32,
    /// Set by mods naming their own category; resolved at freeze.
    pub category_name: String,
    pub volume: f32,
    pub pitch: [f32; 2],
    pub range: f32,
    pub full_within: f32,
    pub falloff: i32,
}

pub struct MusicDef {
    pub name: String,
    pub tracks: Vec<String>,
    pub dimension: Option<u32>,
    /// Conditions that all hold while it may play, and their names until the freeze.
    pub when: Vec<u32>,
    pub when_names: Vec<String>,
}

/// A condition and who answers it.
#[derive(Clone)]
pub struct ConditionDef {
    pub name: String,
    pub answer: Answer,
}

#[derive(Clone, Copy)]
pub enum Answer {
    /// One of `abi::condition`.
    Engine,
    /// Every game mode is a condition of its name: the entity, or the client's player, is in it.
    Mode(crate::world::GameMode),
    Mod(ModId, Callback),
}

pub struct ParticleDef {
    pub name: String,
    /// Frames over its life (ids into `Content::textures`); empty for block dust.
    pub frames: Vec<u32>,
    pub life_min: f32,
    pub life_max: f32,
    pub size_min: f32,
    pub size_max: f32,
    pub gravity: f32,
    pub drag: f32,
    pub color: u32,
    pub color_end: u32,
    pub flags: u32,
    pub shader: String,
    pub rules: Vec<ParticleRule>,
    /// Seconds one may live at most, the kinds it turns into included.
    pub longest: f32,
}

/// `abi::ParticleRule` with what it names as numbers.
#[derive(Clone, Copy, Debug)]
pub struct ParticleRule {
    pub event: u32,
    pub when: u32,
    /// `particle_when::TAG`: its index in `Game::particle_tags`.
    pub tag: u32,
    pub then: u32,
    pub amount: f32,
    /// `BECOME`: the kind.
    pub into: u32,
}

/// Tags particle rules ask about at most: the GPU keeps them in the bits a block's word has left.
pub const PARTICLE_TAGS: usize = 5;

/// A thing in a dimension's sky, every registration of it merged (`abi::SkyDesc`).
pub struct SkyDef {
    pub name: String,
    pub kind: u32,
    /// Asset name of the picture, empty for none.
    pub texture: String,
    pub shader: String,
    pub phase: f32,
    /// Ticks per turn, 0 still, infinite following the day.
    pub orbit: f32,
    pub tilt: f32,
    pub heading: f32,
    pub size: f32,
    pub spin: f32,
    pub opacity: [f32; 2],
    pub color: u32,
    pub frames: [u32; 2],
    /// 0: one frame per day of the dimension.
    pub frame_ticks: u32,
    pub height: f32,
    pub drift: f32,
    pub glow: u32,
    pub additive: bool,
    pub count: u32,
    pub seed: u32,
    pub order: i32,
}

pub struct RawSky {
    pub dimension: String,
    pub name: String,
    pub texture: String,
    pub shader: String,
    pub desc: abi::SkyDesc,
}

/// Each (dimension, name) with later registrations overriding what they set; defaults for what
/// nobody set come last, so a later mod can change one thing and keep the rest.
fn freeze_sky(g: &mut Game, raws: Vec<RawSky>, dims: &FxHashMap<String, u32>, errors: &mut Vec<String>) {
    fn over<T: Copy + PartialEq>(old: &mut T, new: T, unset: T) {
        if new != unset {
            *old = new;
        }
    }
    fn over_f(old: &mut f32, new: f32) {
        if !new.is_nan() {
            *old = new;
        }
    }
    let mut merged: Vec<RawSky> = Vec::new();
    for r in raws {
        if !dims.contains_key(&r.dimension) {
            errors.push(format!("sky object {}: unknown dimension {}", r.name, r.dimension));
            continue;
        }
        let at = merged
            .iter()
            .position(|m| m.dimension == r.dimension && m.name == r.name);
        if r.desc.remove != 0 {
            match at {
                Some(at) => drop(merged.remove(at)),
                None => log::warn!(
                    "removing sky object {} from {}: it isn't there (does the mod that adds it load first?)",
                    r.name,
                    r.dimension
                ),
            }
            continue;
        }
        let Some(at) = at else {
            merged.push(r);
            continue;
        };
        let (m, d) = (&mut merged[at], r.desc);
        if !r.texture.is_empty() {
            m.texture = r.texture;
        }
        if !r.shader.is_empty() {
            m.shader = r.shader;
        }
        // What it is stays what it was first registered as.
        let o = &mut m.desc;
        let [day, night] = &mut o.opacity;
        for (old, new) in [
            (&mut o.phase, d.phase),
            (&mut o.orbit, d.orbit),
            (&mut o.tilt, d.tilt),
            (&mut o.heading, d.heading),
            (&mut o.size, d.size),
            (&mut o.spin, d.spin),
            (day, d.opacity[0]),
            (night, d.opacity[1]),
            (&mut o.height, d.height),
            (&mut o.drift, d.drift),
        ] {
            over_f(old, new);
        }
        let [columns, rows] = &mut o.frames;
        for (old, new) in [
            (&mut o.color, d.color),
            (columns, d.frames[0]),
            (rows, d.frames[1]),
            (&mut o.frame_ticks, d.frame_ticks),
            (&mut o.glow, d.glow),
            (&mut o.additive, d.additive),
            (&mut o.count, d.count),
            (&mut o.seed, d.seed),
        ] {
            over(old, new, u32::MAX);
        }
        over(&mut o.order, d.order, i32::MIN);
    }
    g.sky = (0..dims.len()).map(|_| Vec::new()).collect();
    for r in merged {
        let d = r.desc;
        let kind = if d.kind == u32::MAX { abi::sky_kind::BODY } else { d.kind };
        let stars = kind == abi::sky_kind::STARS;
        let or = |v: f32, default: f32| if v.is_nan() { default } else { v };
        let or_u = |v: u32, default: u32| if v == u32::MAX { default } else { v };
        // Stars show at night only; behind them domes, in front of them bodies.
        let order = if d.order != i32::MIN {
            d.order
        } else {
            [0, -100, -200, 100]
                .get(kind as usize)
                .copied()
                .unwrap_or(0)
        };
        // Would be a white square across the sky.
        if kind != abi::sky_kind::STARS && r.texture.is_empty() && r.shader.is_empty() {
            errors.push(format!("sky object {} in {} has no texture or shader", r.name, r.dimension));
        }
        g.sky[dims[&r.dimension] as usize].push(SkyDef {
            name: r.name,
            kind,
            texture: r.texture,
            shader: r.shader,
            phase: or(d.phase, 0.0),
            // A dome stands still by default, what moves across the sky follows the day.
            orbit: or(d.orbit, if kind == abi::sky_kind::DOME { 0.0 } else { f32::INFINITY }),
            tilt: or(d.tilt, 0.0).to_radians(),
            heading: or(d.heading, 0.0).to_radians(),
            size: or(d.size, if stars { 0.25 } else { 20.0 }).to_radians(),
            spin: or(d.spin, 0.0),
            opacity: [or(d.opacity[0], if stars { 0.0 } else { 1.0 }), or(d.opacity[1], if stars { 0.22 } else { 1.0 })],
            color: or_u(d.color, 0xFFFFFF),
            frames: [or_u(d.frames[0], 1).max(1), or_u(d.frames[1], 1).max(1)],
            frame_ticks: or_u(d.frame_ticks, 0),
            height: or(d.height, 224.33),
            drift: or(d.drift, 0.03),
            glow: or_u(d.glow, 0),
            additive: or_u(d.additive, 0) != 0,
            count: or_u(d.count, 1500),
            seed: or_u(d.seed, 10842),
            order,
        });
    }
    for list in &mut g.sky {
        list.sort_by_key(|s| s.order);
    }
}

pub struct RawParticle {
    pub name: String,
    pub cell: usize,
    pub textures: Vec<String>,
    /// Numbers only: its `Str`s, `rules` and `cell` are copied out (they point into the mod).
    pub desc: abi::ParticleDesc,
    pub shader: String,
    pub rules: Vec<RawParticleRule>,
}

/// `abi::ParticleRule` with its names owned.
pub struct RawParticleRule {
    pub event: u32,
    pub when: u32,
    pub tag: String,
    pub then: u32,
    pub amount: f32,
    pub into: String,
}

// The descriptor's pointers are cleared when it is copied.
unsafe impl Send for RawParticle {}

#[derive(Default)]
pub struct PacketDef {
    pub server: Option<(ModId, Callback)>,
    pub client: Option<(ModId, Callback)>,
    /// The value schema both sides agree on; sends with another are refused.
    pub schema: String,
}

// ---------------------------------------------------------------- raw registration

pub struct RawEntity {
    pub name: String,
    pub cell: usize,
    pub desc: EntityKind,
    pub replace: bool,
    pub components: Vec<RawValue>,
}

pub struct RawComponent {
    pub owner: ModId,
    pub def: ComponentDef,
    pub schema: String,
}

#[derive(Default)]
pub struct RawRecipe {
    pub kind: String,
    pub pattern: Vec<String>,
    pub keys: Vec<(char, String)>,
    pub inputs: Vec<String>,
    pub input_counts: Vec<u32>,
    pub output: String,
    pub count: u32,
    pub time: u32,
    /// (item, count, chance) after the main output; a liquid is its item.
    pub outputs: Vec<(String, u32, f32)>,
    pub values: Vec<crate::components::RawValue>,
    pub flags: u32,
}

#[derive(Default)]
pub struct RawGame {
    pub cameras: Vec<RawCamera>,
    pub entities: Vec<RawEntity>,
    pub components: Vec<RawComponent>,
    /// (owner, kind, hook, callback); hook 0 is the tick.
    /// (owner, callback) run once per tick in every region (`Host::reg_world_tick`).
    pub world_ticks: Vec<(ModId, Callback)>,
    pub recipes: Vec<RawRecipe>,
    pub recipe_kinds: Vec<RawRecipeKind>,
    pub categories: Vec<CategoryDef>,
    pub damage_causes: Vec<(usize, DamageCauseDef)>,
    pub entity_effects: Vec<(usize, EntityEffectDef)>,
    pub text_places: Vec<(usize, crate::hud::Place)>,
    pub game_modes: Vec<(usize, GameModeDef)>,
    pub poses: Vec<(usize, PoseDef)>,
    pub weathers: Vec<(usize, WeatherDef)>,
    pub precipitations: Vec<(usize, PrecipitationDef)>,
    pub sound_categories: Vec<SoundCategory>,
    pub menus: Vec<(usize, MenuDef)>,
    pub huds: Vec<HudElement>,
    pub chat_format: Option<(ModId, Callback)>,
    /// (owner, name, usage, callback).
    pub commands: Vec<(ModId, String, String, Callback)>,
    /// Commands that run on the client: (owner, name, usage, callback).
    pub client_commands: Vec<(ModId, String, String, Callback)>,
    pub command_extensions: Vec<(ModId, CommandExtension)>,
    /// (mod, menu): of several, the mod loaded last wins.
    pub player_menu: Option<(ModId, String)>,
    /// (mod, groups (name, cols, rows, flags)): likewise.
    pub player_inventory: Option<(ModId, Vec<(String, u32, u32, u32)>)>,
    pub equipment: Vec<EquipmentSlot>,
    pub screens: Vec<ScreenDef>,
    pub screen_parts: Vec<ScreenPart>,
    /// (set, words) for `word(#set)` command parameters.
    pub words: Vec<(String, Vec<String>)>,
    pub block_entities: Vec<(String, String, u32)>,
    /// (kind, component, its starting value on that kind).
    pub attach: Vec<(String, String, Option<Vec<u8>>)>,
    pub ores: Vec<RawOre>,
    pub scatters: Vec<RawScatter>,
    /// (owner, dimensions (empty: every dimension but bare ones), callback).
    pub decorations: Vec<(ModId, RawWhere, Callback)>,
    pub terrain: Vec<crate::server::terrain::RawTerrain>,
    pub structures: Vec<RawStructure>,
    pub templates: Vec<RawTemplate>,
    /// (name, icon item) in registration order.
    pub creative_tabs: Vec<(String, String)>,
    /// Components and tags set on other mods' entries.
    pub sets: Vec<RawSet>,
    pub client_hooks: Vec<(ModId, u32, Callback)>,
    /// (owner, entity kind, callback).
    pub client_entity_watchers: Vec<(ModId, String, Callback)>,
    /// (entity kind, parts, skin size) set with `Host::reg_entity_model`.
    pub entity_models: Vec<(String, Vec<Part>, [u32; 2])>,
    pub attached_models: Vec<(String, String, String)>,
    pub bone_motions: Vec<(String, String, crate::model::BoneMotion)>,
    pub item_model_for_stack: Vec<(ModId, String, Callback)>,
    /// (owner, name, side, callback).
    /// (owner, channel, schema, side, callback).
    pub packets: Vec<(ModId, String, String, u32, Callback)>,
    pub keys: Vec<(String, String)>,
    /// (block name, flow rules).
    pub liquids: Vec<(String, String, weft_abi::LiquidDesc)>,
    /// (liquid, overlay, change), in mod order.
    pub liquid_changes: Vec<(String, String, weft_abi::LiquidEdit)>,
    /// (command, level), mods' say on what their commands ask for.
    pub command_permissions: Vec<(String, u32)>,
    /// liquid, touching, source result, flowing result, falls-into result.
    pub reactions: Vec<[String; 5]>,
    /// (owner, event, schema, handler).
    pub event_handlers: Vec<(ModId, String, String, Callback)>,
    pub interceptors: Vec<(ModId, String, Callback)>,
    /// (owner, chain, stage, modifier).
    pub modifiers: Vec<(ModId, String, u32, Callback)>,
    /// (name, default, min, max).
    pub attributes: Vec<(String, f64, f64, f64)>,
    pub loot: Vec<crate::loot::RawLoot>,
    pub loot_mods: Vec<crate::loot::RawLootMod>,
    /// (component, schema, `stack_flags`).
    pub stack_components: Vec<(String, String, u32)>,
    /// (owner, component, schema, system).
    pub systems: Vec<(ModId, String, String, Callback)>,
    pub conditions: Vec<(usize, ConditionDef)>,
    pub particles: Vec<RawParticle>,
    pub sky: Vec<RawSky>,
    /// (handle cell, setting).
    pub configs: Vec<(usize, crate::config::ConfigDef)>,
    /// (recipe type, item).
    /// (handle cell, event).
    pub sounds: Vec<(usize, SoundDef)>,
    /// (dimension name, music).
    pub music: Vec<(String, MusicDef)>,
    pub spawns: Vec<RawSpawn>,
}

/// A spawning rule as registered (names).
pub struct RawSpawn {
    pub entity: String,
    pub at: RawWhere,
    pub ground: String,
    pub place: u32,
    pub group: [u32; 2],
    pub weight: u32,
    pub chance: f32,
}

/// A spawning rule with its names resolved (see `abi::SpawnDesc`).
pub struct SpawnRule {
    pub entity: u32,
    pub category: u32,
    pub biome: Option<u32>,
    pub ground: Option<u32>,
    /// `abi::spawn_place`.
    pub place: u32,
    /// A condition the spot must meet.
    pub condition: Option<u32>,
    /// Blocks of room the mob needs up.
    pub height: i32,
    /// Dimensions it spawns in, empty: any.
    pub dims: Vec<u32>,
    pub light: [u32; 2],
    pub y: [i32; 2],
    pub group: [u32; 2],
    pub weight: u32,
    pub chance: f32,
}

/// A mod's addition to a command registered elsewhere.
pub enum CommandExtension {
    /// `client`: the branch was written for a client command (its callback wants a client).
    Branch { command: String, usage: String, client: bool, cb: Callback },
}

/// An event mods can subscribe to; `size` is learned from the first value emitted when the
/// engine does not know it.
pub struct EventDef {
    pub name: String,
    pub schema: String,
    pub size: std::sync::atomic::AtomicU32,
    pub handlers: Vec<(ModId, Callback)>,
}

/// A mod's screen.
/// Something a mod adds to a place of a screen.
#[derive(Clone)]
pub struct ScreenPart {
    pub screen: String,
    pub area: String,
    pub order: i32,
    pub owner: ModId,
    pub cb: Callback,
}

#[derive(Clone)]
pub struct ScreenDef {
    pub name: String,
    pub owner: ModId,
    pub cb: Callback,
    /// `abi::screen_flags`.
    pub flags: u32,
    pub on_key: Callback,
}

/// Slots of every player's inventory, laid out by the engine from the groups and equipment
/// slots mods declare.
#[derive(Clone, Debug)]
pub struct PlayerLayout {
    pub slots: u32,
    /// The first this many are the hotbar.
    pub hotbar: u32,
    /// The slot of the second hand, past the equipment, if there is one.
    pub off: Option<u32>,
    /// Equipment: never filled by pickups, worn on the model.
    pub worn: std::ops::Range<u32>,
    /// Named groups: the declared ones, each equipment slot, and `core:equipment` for all of them.
    pub groups: Vec<PlayerGroup>,
    pub equipment: Vec<EquipmentSlot>,
}

#[derive(Clone, Debug)]
pub struct PlayerGroup {
    pub name: String,
    pub start: u32,
    pub count: u32,
    pub cols: u32,
}

#[derive(Clone, Debug)]
pub struct EquipmentSlot {
    pub name: String,
    /// Bones it covers by name, and the roles (`1 << role`) of the names that have one.
    pub bones: Vec<String>,
    pub roles: u32,
    pub background: String,
}

/// The group holding every equipment slot.
pub const EQUIPMENT_GROUP: &str = "core:equipment";

impl Default for PlayerLayout {
    fn default() -> Self {
        PlayerLayout::new(&[("hotbar".into(), 9, 1, weft_abi::slot_flags::HOTBAR), ("main".into(), 9, 3, 0)], Vec::new())
    }
}

impl PlayerLayout {
    /// The hotbar group first, then the other groups in order, then the equipment slots.
    pub fn new(groups: &[(String, u32, u32, u32)], mut equipment: Vec<EquipmentSlot>) -> PlayerLayout {
        // The other hand shows among the equipment but isn't worn: last, past `worn`.
        equipment.sort_by_key(|e| e.name == weft_abi::OFF_HAND_SLOT);
        let off = equipment.iter().any(|e| e.name == weft_abi::OFF_HAND_SLOT) as u32;
        let mut ordered: Vec<&(String, u32, u32, u32)> = groups
            .iter()
            .filter(|g| g.3 & weft_abi::slot_flags::HOTBAR != 0)
            .take(1)
            .collect();
        ordered.extend(
            groups
                .iter()
                .filter(|g| g.3 & weft_abi::slot_flags::HOTBAR == 0),
        );
        let hotbar = ordered
            .first()
            .filter(|g| g.3 & weft_abi::slot_flags::HOTBAR != 0)
            .map_or(0, |g| g.1 * g.2);
        let mut at = 0;
        let mut out = Vec::new();
        for (name, cols, rows, _) in ordered {
            out.push(PlayerGroup { name: name.clone(), start: at, count: cols * rows, cols: *cols });
            at += cols * rows;
        }
        let worn = at..at + equipment.len() as u32 - off;
        for (i, e) in equipment.iter().enumerate() {
            out.push(PlayerGroup { name: e.name.clone(), start: worn.start + i as u32, count: 1, cols: 1 });
        }
        out.push(PlayerGroup { name: EQUIPMENT_GROUP.into(), start: worn.start, count: worn.len() as u32, cols: 1 });
        PlayerLayout { slots: worn.end + off, off: (off == 1).then_some(worn.end), hotbar, worn, groups: out, equipment }
    }

    pub fn group(&self, name: &str) -> Option<&PlayerGroup> {
        self.groups.iter().find(|g| g.name == name)
    }
}

impl PlayerLayout {
    /// Main inventory slots: neither hotbar nor equipment.
    pub fn main(&self) -> impl Iterator<Item = u32> + '_ {
        (self.hotbar..self.slots).filter(move |i| !self.worn.contains(i) && Some(*i) != self.off)
    }
}

/// A sound category: its volume setting and whether caves and water change it.
#[derive(Clone, Debug)]
pub struct SoundCategory {
    pub name: String,
    /// `<ns>:volume_<name>`, none for `core:ui`.
    pub setting: Option<String>,
    pub world: bool,
}

impl SoundCategory {
    pub fn new(name: &str, world: bool) -> SoundCategory {
        let (ns, short) = name.split_once(':').unwrap_or(("core", name));
        let setting = (name != "core:ui").then(|| format!("{ns}:volume_{short}"));
        SoundCategory { name: name.to_string(), setting, world }
    }
}

/// `SoundDef::category` of the engine's music track: the first category.
pub const MUSIC_CATEGORY: u32 = 0;

/// A damage cause: what the engine does differently for it (`abi::damage_flags`), and how a
/// death by it reads.
#[derive(Clone, Debug)]
pub struct DamageCauseDef {
    pub name: String,
    pub flags: u32,
    pub death_message: String,
    pub death_message_by: String,
}

impl DamageCauseDef {
    pub fn has(&self, flag: u32) -> bool {
        self.flags & flag != 0
    }
}

fn engine_damage_causes() -> Vec<(usize, DamageCauseDef)> {
    use abi::damage_flags as f;
    let flags = [
        0,
        f::BYPASSES_ARMOR,
        0,
        f::BURNING,
        f::BYPASSES_ARMOR,
        f::BYPASSES_ARMOR,
        f::BYPASSES_ARMOR | f::PIERCES_INVULNERABLE | f::IGNORES_COOLDOWN,
        f::BURNING,
        0,
        f::BYPASSES_ARMOR,
        0,
        0,
    ];
    let messages = ["generic", "fall", "generic", "fire", "starve", "drown", "void", "lava", "crush", "suffocate", "explosion", "generic"];
    abi::damage_cause::NAMES
        .iter()
        .zip(flags)
        .zip(messages)
        .map(|((name, flags), m)| {
            let by = if *name == "core:attack" { "death.attack".to_string() } else { String::new() };
            (0, DamageCauseDef { name: name.to_string(), flags, death_message: format!("death.{m}"), death_message_by: by })
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct WeatherDef {
    pub name: String,
    /// `abi::weather_flags`.
    pub flags: u32,
    pub look: abi::WeatherLook,
    pub sound_name: String,
    /// `sound_name` resolved at freeze.
    pub sound: Option<u32>,
    pub duration: [u64; 2],
    pub weight: u32,
}

impl WeatherDef {
    pub fn falls(&self) -> bool {
        self.flags & abi::weather_flags::FALLS != 0
    }
    pub fn lightning(&self) -> bool {
        self.flags & abi::weather_flags::LIGHTNING != 0
    }
    pub fn shelters(&self) -> bool {
        self.flags & abi::weather_flags::SHELTERS != 0
    }
}

fn engine_weathers() -> Vec<(usize, WeatherDef)> {
    let w = |name: &str, flags: u32, look: abi::WeatherLook, duration: [u64; 2], weight: u32| {
        (0, WeatherDef { name: format!("core:{name}"), flags, look, sound_name: String::new(), sound: None, duration, weight })
    };
    // Vanilla's timing: clear for 10 minutes to 2.5 hours, rain for 10 to 20 minutes, a quarter
    // of it with thunder.
    use abi::weather_flags as f;
    let rain = abi::WeatherDesc::DEFAULT.look;
    let thunder = abi::WeatherLook { cloud_dark: 0.95, ..rain };
    vec![
        w("clear", 0, abi::WeatherLook::default(), [12000, 180000], 0),
        w("rain", f::FALLS | f::SHELTERS, rain, [12000, 24000], 3),
        w("thunder", f::FALLS | f::SHELTERS | f::LIGHTNING, thunder, [12000, 24000], 1),
    ]
}

#[derive(Clone, Debug)]
pub struct PrecipitationDef {
    pub name: String,
    pub texture: String,
    pub speed: f32,
    pub density: f32,
}

/// A pose: its box and eyes as shares of standing, how fast it goes, how the model lies and
/// the clip it plays, and when a player takes it (`abi::PoseDesc`).
#[derive(Clone, Debug)]
pub struct PoseDef {
    pub name: String,
    pub height: f64,
    pub eye: f64,
    pub speed: f32,
    /// `abi::pose_tilt`.
    pub tilt: u32,
    pub action: String,
    /// The camera bobs while walking in it.
    pub bob: bool,
    pub priority: i32,
    /// The action held for it, empty for none.
    pub key: String,
    /// `abi::pose_when`.
    pub when: u32,
    /// Names as registered, then the poses they are (`fallback`).
    pub fallback_names: String,
    pub fallback: Vec<crate::world::Pose>,
}

pub fn engine_pose_defs() -> &'static [PoseDef] {
    static DEFS: std::sync::OnceLock<Vec<PoseDef>> = std::sync::OnceLock::new();
    DEFS.get_or_init(|| {
        let mut defs: Vec<PoseDef> = engine_poses().into_iter().map(|(_, d)| d).collect();
        let names: Vec<String> = defs.iter().map(|d| d.name.clone()).collect();
        for d in &mut defs {
            d.fallback = d
                .fallback_names
                .split_whitespace()
                .filter_map(|n| names.iter().position(|x| x == n))
                .map(|i| crate::world::Pose(i as u16))
                .collect();
        }
        defs
    })
}

fn engine_poses() -> Vec<(usize, PoseDef)> {
    use abi::{pose_tilt as t, pose_when as w};
    let pose = |name: &str, height: f64, eye: f64, speed: f32, tilt: u32, action: &str, bob: bool| PoseDef {
        name: format!("core:{name}"),
        height,
        eye,
        speed,
        tilt,
        action: action.into(),
        bob,
        priority: 0,
        key: String::new(),
        when: 0,
        fallback_names: String::new(),
        fallback: Vec::new(),
    };
    let taken = |p: PoseDef, priority: i32, key: &str, when: u32, fallback: &str| PoseDef {
        priority,
        key: key.into(),
        when,
        fallback_names: fallback.into(),
        ..p
    };
    // Modern vanilla's: the player's 1.8 and 1.62 standing. Under a low ceiling it crouches, and
    // where that has no room either it swims in a liquid and crawls out of one.
    let low = "core:swim core:crawl";
    vec![
        taken(pose("stand", 1.0, 1.0, 1.0, t::UPRIGHT, "", true), 0, "", 0, &format!("core:crouch {low}")),
        taken(pose("crouch", 1.5 / 1.8, 1.27 / 1.62, 0.3, t::UPRIGHT, "sneak", true), 10, "core:sneak", w::NOT_FLYING, low),
        taken(
            pose("swim", 0.6 / 1.8, 0.4 / 1.62, 1.0, t::ALONG_LOOK, "swim", false),
            30,
            "",
            w::SPRINTING | w::SWIM_LIQUID | w::NOT_FLYING,
            "core:crouch core:crawl",
        ),
        taken(pose("crawl", 0.6 / 1.8, 0.4 / 1.62, 0.3, t::FACE_DOWN, "crawl", true), 20, "core:crawl", w::NOT_FLYING, ""),
        // Only where a mod holds it.
        taken(pose("sleep", 0.2 / 1.8, 0.45 / 1.62, 0.0, t::ON_BACK, "", false), i32::MIN, "", 0, ""),
    ]
    .into_iter()
    .map(|p| (0, p))
    .collect()
}

/// A game mode: what its players may do (`abi::game_mode_flags`) and the attribute modifiers
/// they get, (attribute, op, amount).
#[derive(Clone, Debug)]
pub struct GameModeDef {
    pub name: String,
    pub flags: u32,
    pub modifiers: Vec<(String, u32, f64)>,
}

fn engine_game_modes() -> Vec<(usize, GameModeDef)> {
    use abi::game_mode_flags as f;
    // Unseen by mobs outside survival; creative reaches further.
    let unseen = (abi::attribute::VISIBILITY.to_string(), abi::attr_op::MULTIPLY_TOTAL, -1.0);
    let creative = f::INSTANT_BREAK | f::NO_COSTS | f::INVULNERABLE | f::CAN_FLY | f::CREATIVE_INVENTORY;
    let spectator = f::INVULNERABLE | f::ALWAYS_FLY | f::NOCLIP | f::NO_INTERACT | f::HIDDEN | f::NO_HUD | f::NO_COSTS;
    vec![
        (0, GameModeDef { name: "core:survival".into(), flags: 0, modifiers: Vec::new() }),
        (
            0,
            GameModeDef {
                name: "core:creative".into(),
                flags: creative,
                modifiers: vec![
                    (abi::attribute::BLOCK_REACH.into(), abi::attr_op::ADD, 0.5),
                    (abi::attribute::ENTITY_REACH.into(), abi::attr_op::ADD, 2.0),
                    unseen.clone(),
                ],
            },
        ),
        (0, GameModeDef { name: "core:spectator".into(), flags: spectator, modifiers: vec![unseen] }),
    ]
}

/// A shader effect on entities: the snippet that draws it, `ns:name`.
#[derive(Clone, Debug)]
pub struct EntityEffectDef {
    pub name: String,
    pub shader: String,
}

/// What natural spawning and despawning know of entities by category.
#[derive(Clone, Debug)]
pub struct CategoryDef {
    pub name: String,
    /// At most this many spawn naturally within 64 blocks of a player.
    pub cap: u32,
    /// Further than this from every player they vanish (checked every 5 s); 0 = never.
    pub despawn: f64,
    /// The cap counts them this close to a player.
    pub cap_radius: f64,
}

pub struct AttrDef {
    pub name: String,
    pub default: f64,
    pub min: f64,
    pub max: f64,
    /// Stable key in saved modifiers.
    pub hash: u64,
    /// The entity kind component of the same name holding a kind's own value.
    pub comp: Option<u32>,
}

/// Events the engine emits, with the schemas of their `abi` structs.
pub const ENGINE_EVENTS: &[(&str, &str, u32)] = {
    use abi::Schema;
    &[
        (abi::action::BREAK_BLOCK, abi::BlockChange::SCHEMA, size_of::<abi::BlockChange>() as u32),
        (abi::action::PLACE_BLOCK, abi::BlockChange::SCHEMA, size_of::<abi::BlockChange>() as u32),
        (abi::action::DIE, abi::Death::SCHEMA, size_of::<abi::Death>() as u32),
        (abi::event::PLAYER_JOINED, abi::PlayerEvent::SCHEMA, size_of::<abi::PlayerEvent>() as u32),
        (abi::event::PLAYER_RESPAWNED, abi::PlayerEvent::SCHEMA, size_of::<abi::PlayerEvent>() as u32),
        (abi::action::PICKUP_ITEM, abi::Pickup::SCHEMA, size_of::<abi::Pickup>() as u32),
        (abi::action::CHAT, abi::ChatMessage::SCHEMA, size_of::<abi::ChatMessage>() as u32),
        (abi::action::HURT, abi::Hurt::SCHEMA, size_of::<abi::Hurt>() as u32),
        (abi::action::USE_ITEM, abi::ItemUsed::SCHEMA, size_of::<abi::ItemUsed>() as u32),
        (abi::event::ENTITY_HEALED, abi::Hurt::SCHEMA, size_of::<abi::Hurt>() as u32),
        (abi::event::WEATHER_TICK, abi::WeatherTick::SCHEMA, size_of::<abi::WeatherTick>() as u32),
        (abi::action::USE_BLOCK, abi::BlockChange::SCHEMA, size_of::<abi::BlockChange>() as u32),
        (abi::action::INTERACT_ENTITY, abi::Interaction::SCHEMA, size_of::<abi::Interaction>() as u32),
        (abi::action::MOUNT, abi::Mounting::SCHEMA, size_of::<abi::Mounting>() as u32),
        (abi::action::DISMOUNT, abi::Mounting::SCHEMA, size_of::<abi::Mounting>() as u32),
        (abi::action::ENTER_DIMENSION, abi::DimensionChange::SCHEMA, size_of::<abi::DimensionChange>() as u32),
    ]
};

/// FNV-1a, for names stored in saves.
pub fn name_hash(s: &str) -> u64 {
    weft_noise::fnv(s.as_bytes())
}

pub struct RawOre {
    /// Empty: every dimension but bare ones.
    pub at: RawWhere,
    pub block: String,
    pub replace: Vec<String>,
    pub veins: u32,
    pub size: u32,
    pub shape: u32,
    pub shape_size: [f32; 2],
    pub height: u32,
    pub plateau: i32,
    /// (block or `#tag` met, ore block put there).
    pub replace_with: Vec<(String, String)>,
    pub exposed: f32,
    pub edit: bool,
}

pub struct RawScatter {
    pub owner: ModId,
    pub name: String,
    pub at: RawWhere,
    pub cell: i32,
    pub reach: i32,
    /// Down and up.
    pub reach_y: [i32; 2],
    pub density: String,
    pub ground: Vec<String>,
    pub shape: Callback,
}

pub struct RawTemplate {
    pub name: String,
    pub blocks: Vec<([i32; 3], String)>,
}

pub struct RawStructure {
    pub template: String,
    pub at: RawWhere,
    pub spacing: u32,
    pub chance: f32,
    pub sink: i32,
}

/// `abi::Where` as registered (names).
#[derive(Clone)]
pub struct RawWhere {
    pub dims: Vec<String>,
    pub biome: String,
    pub y: [i32; 2],
    pub light: [u32; 2],
    pub condition: String,
}

impl RawWhere {
    pub unsafe fn copy(w: &abi::Where) -> RawWhere {
        unsafe {
            RawWhere {
                dims: w
                    .dimensions
                    .as_str()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                biome: w.biome.as_str().to_string(),
                y: w.y,
                light: w.light,
                condition: w.condition.as_str().to_string(),
            }
        }
    }
}

fn opt(c: Callback) -> Option<Callback> {
    c.is_some().then_some(c)
}

/// # Safety
/// The parts' pointers must be valid for this call.
pub(crate) unsafe fn copy_parts(list: abi::Slice<abi::ModelPart>) -> Vec<Part> {
    unsafe {
        let mut parts = Vec::new();
        list.as_slice().iter().copied().for_each(|p| {
            let mut cuboids = Vec::new();
            p.cuboids.as_slice().iter().copied().for_each(|c| {
                cuboids.push(Cuboid {
                    origin: c.origin,
                    size: c.extent,
                    uv: c.uv,
                    inflate: c.inflate,
                    mirror: c.mirror != 0,
                    faces: (c.per_face != 0).then_some(c.faces),
                })
            });
            let n = parts.len() as i32;
            let parent = (p.parent >= 0 && p.parent < n).then_some(p.parent as u32);
            parts.push(Part {
                name: p.name.as_str().to_string(),
                parent,
                pivot: p.pivot,
                role: 0,
                rest: p.rest,
                cuboids,
                mesh: p.mesh.as_slice().to_vec(),
            });
        });
        crate::model::assign_roles(&mut parts);
        parts
    }
}

pub(crate) unsafe fn copy_entity(owner: ModId, d: &abi::EntityDesc) -> RawEntity {
    unsafe {
        let parts = copy_parts(d.parts);
        let one = |c: Callback| opt(c).map(|c| vec![(owner, c)]).unwrap_or_default();
        RawEntity {
            name: d.name.as_str().to_string(),
            cell: d.cell as usize,
            replace: d.replacing != 0,
            components: RawValue::list(d.components),
            desc: EntityKind {
                name: d.name.as_str().to_string(),
                owner,
                width: d.width,
                height: d.height,
                eye: d.eye,
                speed: d.speed,
                flags: d.flags,
                category: 0,
                category_name: d.category_name.as_str().to_string(),
                texture: d.texture.as_str().to_string(),
                texture_size: d.texture_size,
                parts,
                on_tick: one(d.on_tick),
                on_interact: one(d.on_interact),
                components: Vec::new(),
                values: Vec::new(),
                slots: d.slots.min(1 << 16),
                worn: 0..0,
                sounds: [u32::MAX; 4],
                carry_height: d.carry_height,
                template_name: d.template.as_str().to_string(),
                template: None,
                seats: d
                    .seats
                    .as_slice()
                    .iter()
                    .take(255)
                    .map(|s| Seat {
                        pos: crate::world::dvec(s.pos),
                        steers: s.steers != 0,
                        exit: s.exit,
                        exit_at: crate::world::dvec(s.exit_at),
                    })
                    .collect(),
                steer: opt(d.steer).map(|c| (owner, c)),
                dismount: opt(d.dismount).map(|c| (owner, c)),
                ride_camera: d.ride_camera,
            },
        }
    }
}

pub(crate) unsafe fn copy_menu(owner: ModId, d: &abi::MenuDesc) -> (usize, MenuDef) {
    unsafe {
        // Groups without a start take the slots right after the previous one of their owner.
        let mut next = [0u32; 5];
        let text = |s: abi::Str| (s.len > 0).then(|| s.as_str().to_string());
        let groups: Vec<GroupDef> = d
            .groups
            .as_slice()
            .iter()
            .map(|g| {
                let o = (g.owner as usize).min(4);
                let start = if g.start == u32::MAX { next[o] } else { g.start };
                next[o] = next[o].max(start + g.cols * g.rows);
                GroupDef {
                    name: g.name.as_str().to_string(),
                    owner: g.owner,
                    start,
                    cols: g.cols.max(1),
                    rows: g.rows.max(1),
                    flags: g.flags,
                    accepts: g.accepts.as_str().to_string(),
                    limit: g.limit,
                    size: if g.size > 0.0 { g.size } else { 18.0 },
                    background: text(g.background),
                    tooltip: text(g.tooltip),
                    priority: g.priority,
                }
            })
            .collect();
        (
            d.cell as usize,
            MenuDef {
                name: d.name.as_str().to_string(),
                owner,
                temp_slots: groups
                    .iter()
                    .filter(|g| g.owner == weft_abi::slot_owner::MENU)
                    .map(|g| g.start + g.cols * g.rows)
                    .max()
                    .unwrap_or(0),
                groups,
                recipes: d
                    .recipes
                    .as_str()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect(),
                on_change: opt(d.on_change),
                on_take: opt(d.on_take),
                on_tick: opt(d.on_tick),
                screen: opt(d.screen),
            },
        )
    }
}

pub(crate) unsafe fn copy_recipe(d: &abi::RecipeDesc) -> RawRecipe {
    unsafe {
        RawRecipe {
            kind: d.kind.as_str().to_string(),
            pattern: d
                .pattern
                .as_slice()
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            keys: d
                .keys
                .as_slice()
                .iter()
                .map(|k| (char::from_u32(k.ch).unwrap_or(' '), k.item.as_str().to_string()))
                .collect(),
            inputs: d
                .inputs
                .as_slice()
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            input_counts: d.input_counts.as_slice().to_vec(),
            output: d.output.as_str().to_string(),
            count: d.count.max(1),
            time: d.time,
            outputs: d
                .outputs
                .as_slice()
                .iter()
                .map(|o| (o.item.as_str().to_string(), o.count.max(1), o.chance))
                .collect(),
            values: crate::components::RawValue::list(d.values),
            flags: d.flags,
        }
    }
}

fn write_cell(cell: usize, id: u32) {
    if cell != 0 {
        unsafe { (*(cell as *const AtomicU32)).store(id, Ordering::Relaxed) }
    }
}

pub use weft_abi::EYE_SHARE as EYE;

/// What an entity kind's `sounds` hold.
pub mod entity_sound {
    pub const HURT: usize = 0;
    pub const DEATH: usize = 1;
    pub const AMBIENT: usize = 2;
    pub const STEP: usize = 3;
}

fn builtin(name: &str, width: f32, height: f32, flags: u32) -> EntityKind {
    EntityKind {
        name: name.into(),
        owner: ENGINE,
        width,
        height,
        eye: height * EYE,
        speed: 4.3,
        flags,
        category: 0,
        category_name: String::new(),
        texture: String::new(),
        texture_size: [64, 32],
        parts: Vec::new(),
        on_tick: Vec::new(),
        on_interact: Vec::new(),
        components: Vec::new(),
        values: Vec::new(),
        slots: 0,
        worn: 0..0,
        sounds: [u32::MAX; 4],
        carry_height: 0.0,
        template_name: String::new(),
        template: None,
        seats: Vec::new(),
        steer: None,
        dismount: None,
        ride_camera: f32::NAN,
    }
}

/// Entity component with attribute modifiers.
pub const MODIFIERS: &str = "core:modifiers";
/// Timers set with `Host::entity_after`, saved with their entity.
pub const TIMERS: &str = "core:timers";
/// Engine fields clients show, kept as synced components (`Dim::mirror_fields`): health and
/// max health (`[f32; 2]`, max only for players), on fire (`u8`), worn items (`u32` each), own
/// size and look (`[f32; 3]` with width 0 for the kind's, `u32` kind with `u32::MAX` for its own),
/// a player's item in hand (`u32`, 0 for none).
pub const HEALTH: &str = "core:health";
pub const BURNING: &str = "core:burning";
pub const WORN: &str = "core:worn";
pub const SHAPE: &str = "core:shape";
pub const HELD: &str = "core:held";
pub const OFF_HELD: &str = "core:off_held";
pub const USING: &str = "core:using";
pub const SWAPS: &str = "core:swaps";
pub const LOOPING_ACTION: &str = "core:looping_action";
/// The shader effect on it: effect id, strength (`f32`), the tick it ends (`u64`).
pub const EFFECT: &str = "core:effect";
/// A pose a mod holds it in (`u32` pose id, `f32` yaw or NaN), empty: as it moves.
pub const FORCED_POSE: &str = "core:forced_pose";
/// What it rides (`u64` id) and its seat (`u32`), empty: nothing. Saved, and seen by everyone.
pub const RIDING: &str = "core:riding";
/// Entity component (`u64`, saved): the world time a system last simulated it to, so it can
/// catch up on what it missed (`Dim::run_systems`).
pub const SIMULATED: &str = "core:simulated";

/// Entity kinds, frozen before components (which attach to them).
pub struct Entities {
    pub kinds: Vec<EntityKind>,
    pub names: FxHashMap<String, u32>,
    /// Each kind's own components and its owner.
    pub values: Vec<(ModId, Vec<RawValue>)>,
}

pub fn freeze_entities(raws: &mut [RawGame], errors: &mut Vec<String>) -> Entities {
    let mut entities: Vec<RawEntity> = raws
        .iter_mut()
        .flat_map(|r| std::mem::take(&mut r.entities))
        .collect();
    let models: Vec<(String, Vec<Part>, [u32; 2])> = raws
        .iter_mut()
        .flat_map(|r| std::mem::take(&mut r.entity_models))
        .collect();
    let mut player = builtin(PLAYER, 0.6, 1.8, ef::FALL_DAMAGE | ef::PUSHABLE);
    player.eye = 1.62;
    player.texture = "core:entity/player".into();
    entities.push(RawEntity { name: PLAYER.into(), cell: 0, desc: player, replace: false, components: Vec::new() });
    entities.push(RawEntity {
        name: ITEM.into(),
        cell: 0,
        desc: builtin(ITEM, 0.25, 0.25, ef::INVULNERABLE),
        replace: false,
        components: Vec::new(),
    });
    entities.push(RawEntity {
        name: FALLING.into(),
        cell: 0,
        desc: builtin(FALLING, 0.98, 1.0, ef::INVULNERABLE | ef::SOLID),
        replace: false,
        components: Vec::new(),
    });
    let mut moving = builtin(MOVING, 1.0, 1.0, ef::INVULNERABLE | ef::SOLID | ef::KINEMATIC);
    moving.carry_height = 1.0;
    entities.push(RawEntity { name: MOVING.into(), cell: 0, desc: moving, replace: false, components: Vec::new() });
    let mut platform = builtin(PLATFORM, 1.0, 1.0, ef::INVULNERABLE | ef::SOLID | ef::KINEMATIC);
    platform.carry_height = 3.0;
    entities.push(RawEntity { name: PLATFORM.into(), cell: 0, desc: platform, replace: false, components: Vec::new() });
    entities.sort_by(|a, b| a.name.cmp(&b.name).then(a.replace.cmp(&b.replace)));
    let mut kept: Vec<RawEntity> = Vec::with_capacity(entities.len());
    let mut replaced_cells: Vec<(usize, String)> = Vec::new();
    for e in entities {
        match kept.last() {
            Some(prev) if prev.name == e.name && e.replace => {
                // The replaced kind's handle must resolve to the replacement too.
                if prev.cell != 0 {
                    replaced_cells.push((prev.cell, e.name.clone()));
                }
                kept.pop();
                kept.push(e);
            }
            Some(prev) if prev.name == e.name => {
                errors.push(format!("entity {} registered twice", e.name));
            }
            _ => kept.push(e),
        }
    }
    let mut out = Entities { kinds: Vec::new(), names: FxHashMap::default(), values: Vec::new() };
    for (i, e) in kept.into_iter().enumerate() {
        write_cell(e.cell, i as u32);
        out.names.insert(e.name.clone(), i as u32);
        out.values.push((e.desc.owner, e.components));
        out.kinds.push(e.desc);
    }
    for (cell, name) in replaced_cells {
        write_cell(cell, out.names[&name]);
    }
    // Models set on kinds of others (the player's from base), in load order: the last wins.
    for (kind, parts, uv) in models {
        match out.names.get(&kind) {
            Some(&k) => (out.kinds[k as usize].parts, out.kinds[k as usize].texture_size) = (parts, uv),
            None => errors.push(format!("model for unknown entity {kind}")),
        }
    }
    out
}

pub fn freeze(
    raws: Vec<RawGame>,
    entities: Entities,
    block_names: &FxHashMap<String, u32>,
    item_names: &FxHashMap<String, u32>,
    items: &[crate::content::ItemDef],
    dim_names: &FxHashMap<String, u32>,
    c: &Components,
    tag_items: &FxHashMap<u32, Vec<u32>>,
    order: &dyn Fn(ModId) -> u32,
    errors: &mut Vec<String>,
) -> Game {
    let mut g = Game::default();
    let mut components: Vec<RawComponent> = Vec::new();
    let mut world_ticks = Vec::new();
    let mut recipes = Vec::new();
    let mut menus = Vec::new();
    let mut player_menu = None;
    let mut player_inventory: Option<(ModId, Vec<(String, u32, u32, u32)>)> = None;
    let mut equipment: Vec<EquipmentSlot> = Vec::new();
    let mut block_entities = Vec::new();
    let mut attach: Vec<(String, String, Option<Vec<u8>>)> = Vec::new();
    let mut packets: Vec<(ModId, String, String, u32, Callback)> = Vec::new();
    let mut event_handlers: Vec<(ModId, String, String, Callback)> = Vec::new();
    let mut systems: Vec<(ModId, String, String, Callback)> = Vec::new();
    let mut interceptors: Vec<(ModId, String, Callback)> = Vec::new();
    let mut modifiers: Vec<(ModId, String, u32, Callback)> = Vec::new();
    let mut attributes: Vec<(String, f64, f64, f64)> = vec![
        (abi::attribute::MAX_HEALTH.into(), 20.0, 1.0, 1024.0),
        (abi::attribute::MOVEMENT_SPEED.into(), 1.0, 0.0, 16.0),
        (abi::attribute::VISIBILITY.into(), 1.0, 0.0, 16.0),
        (abi::attribute::JUMP_STRENGTH.into(), 1.0, 0.0, 16.0),
        (abi::attribute::ATTACK_SPEED.into(), 1.0, 0.0, 16.0),
        (abi::attribute::CAN_SPRINT.into(), 1.0, 0.0, 1.0),
        (abi::attribute::BLOCK_REACH.into(), 4.5, 0.0, 1024.0),
        (abi::attribute::ENTITY_REACH.into(), 3.0, 0.0, 1024.0),
        (abi::attribute::MINING_SPEED.into(), 1.0, 0.0, 1024.0),
        (abi::attribute::FOV.into(), 1.0, 0.05, 8.0),
    ];
    let mut particles: Vec<RawParticle> = engine_particles();
    let mut sky: Vec<RawSky> = Vec::new();
    let mut configs: Vec<(usize, crate::config::ConfigDef)> = Vec::new();
    let mut sounds: Vec<(usize, SoundDef)> = engine_sounds();
    let mut music: Vec<(String, MusicDef)> = Vec::new();
    let mut spawns: Vec<RawSpawn> = Vec::new();
    // Music first (`MUSIC_CATEGORY`); the order is also that of the volume settings.
    let mut sound_categories: Vec<SoundCategory> = ["music", "blocks", "creatures", "players", "ambient", "ui"]
        .map(|n| SoundCategory::new(&format!("core:{n}"), n != "music" && n != "ui"))
        .to_vec();
    let mut categories: Vec<CategoryDef> = vec![
        CategoryDef { name: "core:misc".into(), cap: 6, despawn: 0.0, cap_radius: 64.0 },
        CategoryDef { name: "core:hostile".into(), cap: 8, despawn: 128.0, cap_radius: 64.0 },
        CategoryDef { name: "core:passive".into(), cap: 6, despawn: 0.0, cap_radius: 64.0 },
    ];
    let mut damage_causes = engine_damage_causes();
    let mut game_modes = engine_game_modes();
    let mut poses = engine_poses();
    let mut conditions: Vec<(usize, ConditionDef)> = abi::condition::NAMES
        .iter()
        .map(|n| (0, ConditionDef { name: n.to_string(), answer: Answer::Engine }))
        .collect();
    let mut weathers = engine_weathers();
    let mut precipitations: Vec<(usize, PrecipitationDef)> = vec![
        (0, PrecipitationDef { name: "core:rain".into(), texture: "core:environment/rain".into(), speed: 1.0, density: 0.1 }),
        (0, PrecipitationDef { name: "core:snow".into(), texture: "core:environment/snow".into(), speed: 0.1, density: 0.5 }),
    ];
    let mut text_places: Vec<(usize, crate::hud::Place)> = crate::hud::engine_places()
        .into_iter()
        .map(|p| (0, p))
        .collect();
    let mut entity_effects: Vec<(usize, EntityEffectDef)> = abi::entity_effect::NAMES
        .iter()
        .map(|n| (0, EntityEffectDef { name: n.to_string(), shader: n.to_string() }))
        .collect();
    let mut entity_watchers: Vec<(ModId, String, Callback)> = Vec::new();
    let mut attached_models: Vec<(String, String, String)> = Vec::new();
    let mut item_models: Vec<(ModId, String, Callback)> = Vec::new();
    let mut cameras: Vec<RawCamera> = Vec::new();
    for r in raws {
        entity_watchers.extend(r.client_entity_watchers);
        attached_models.extend(r.attached_models);
        g.bone_motions.extend(r.bone_motions);
        item_models.extend(r.item_model_for_stack);
        sounds.extend(r.sounds);
        music.extend(r.music);
        particles.extend(r.particles);
        sky.extend(r.sky);
        configs.extend(r.configs);
        conditions.extend(r.conditions);
        g.client_hooks.extend(r.client_hooks);
        packets.extend(r.packets);
        g.keys.extend(r.keys);
        cameras.extend(r.cameras);
        spawns.extend(r.spawns);
        event_handlers.extend(r.event_handlers);
        systems.extend(r.systems);
        interceptors.extend(r.interceptors);
        modifiers.extend(r.modifiers);
        attributes.extend(r.attributes);
        components.extend(r.components);
        world_ticks.extend(r.world_ticks);
        recipes.extend(r.recipes);
        for k in r.recipe_kinds {
            g.recipe_kinds.insert(k.name.clone());
            g.recipe_kind_defs.insert(k.name, k.def);
        }
        categories.extend(r.categories);
        damage_causes.extend(r.damage_causes);
        entity_effects.extend(r.entity_effects);
        text_places.extend(r.text_places);
        game_modes.extend(r.game_modes);
        poses.extend(r.poses);
        weathers.extend(r.weathers);
        precipitations.extend(r.precipitations);
        sound_categories.extend(r.sound_categories);
        menus.extend(r.menus);
        g.huds.extend(r.huds);
        g.chat_format = r.chat_format.or(g.chat_format);
        if let Some((owner, name)) = r.player_menu {
            if player_menu
                .as_ref()
                .is_none_or(|(o, _): &(ModId, String)| order(owner) >= order(*o))
            {
                player_menu = Some((owner, name));
            }
        }
        g.screens.extend(r.screens);
        g.screen_parts.extend(r.screen_parts);
        if let Some((owner, l)) = r.player_inventory {
            if player_inventory
                .as_ref()
                .is_none_or(|(o, _)| order(owner) >= order(*o))
            {
                player_inventory = Some((owner, l));
            }
        }
        equipment.extend(r.equipment);
        block_entities.extend(r.block_entities);
        attach.extend(r.attach);
    }
    g.entities = entities.kinds;
    g.entity_names = entities.names;
    g.player_kind = g.entity_names[PLAYER];
    g.item_kind = g.entity_names[ITEM];
    g.falling_kind = g.entity_names[FALLING];
    g.moving_kind = g.entity_names[MOVING];
    g.platform_kind = g.entity_names[PLATFORM];
    g.client_hooks.sort_by_key(|h| order(h.0));
    entity_watchers.sort_by_key(|t| order(t.0));
    for (owner, kind, cb) in entity_watchers {
        match g.entity_names.get(&kind) {
            Some(&k) => g
                .client_entity_watchers
                .entry(k)
                .or_default()
                .push((owner, cb)),
            None => errors.push(format!("on_entity_frame for unknown entity {kind}")),
        }
    }
    for a in attached_models {
        if g.entity_names.contains_key(&a.0) {
            g.attached_models.push(a);
        } else {
            errors.push(format!("model {} attached to unknown entity {}", a.1, a.0));
        }
    }
    item_models.sort_by_key(|t| order(t.0));
    for (owner, item, cb) in item_models {
        match item_names.get(&item) {
            Some(&i) => {
                g.item_model_for_stack.insert(i, (owner, cb));
            }
            None => errors.push(format!("item_model_for_stack for unknown item {item}")),
        }
    }
    freeze_particles(&mut g, particles, errors);
    freeze_sky(&mut g, sky, dim_names, errors);
    let volumes: Vec<(usize, crate::config::ConfigDef)> = sound_categories
        .iter()
        .filter_map(|k| k.setting.as_deref())
        .map(|s| (0, crate::config::volume(s)))
        .collect();
    configs.extend(volumes);
    sounds.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    for (cell, mut d) in sounds {
        let category = if d.category_name.is_empty() { "core:blocks" } else { &d.category_name };
        match sound_categories.iter().position(|k| k.name == category) {
            Some(i) => d.category = i as u32,
            None => errors.push(format!("sound {}: no category {category}", d.name)),
        }
        if g.sound_names.contains_key(&d.name) {
            errors.push(format!("sound {} registered twice", d.name));
            continue;
        }
        let id = g.sounds.len() as u32;
        write_cell(cell, id);
        g.sound_names.insert(d.name.clone(), id);
        g.sounds.push(d);
    }
    for (dim, mut m) in music {
        if !dim.is_empty() {
            match dim_names.get(&dim) {
                Some(&d) => m.dimension = Some(d),
                None => {
                    errors.push(format!("music {} plays in unknown dimension {dim}", m.name));
                    continue;
                }
            }
        }
        g.music.push(m);
    }
    freeze_cameras(&mut g, cameras);
    // The engine's settings, with a binding per key: the engine's and the mods'.
    let mut keys: Vec<(String, String)> = crate::input::ENGINE_KEYS
        .iter()
        .map(|(a, k)| (a.to_string(), k.to_string()))
        .collect();
    for (action, key) in &g.keys {
        match crate::input::Binding::parse_slots(key) {
            Some(k) => keys.push((action.clone(), crate::input::Binding::slots_id(&k))),
            None => errors.push(format!("key {action}: unknown key {key}")),
        }
    }
    configs.extend(
        crate::config::engine_configs(&keys)
            .into_iter()
            .map(|d| (0, d)),
    );
    configs.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    for (cell, d) in configs {
        if g.config_names.contains_key(&d.name) {
            errors.push(format!("setting {} registered twice", d.name));
            continue;
        }
        let id = g.configs.len() as u32;
        write_cell(cell, id);
        g.config_names.insert(d.name.clone(), id);
        g.configs.push(d);
    }
    g.mob_spawning = g.config_names["core:mob_spawning"];
    g.keep_inventory = g.config_names["core:keep_inventory"];
    g.simulation_distance = g.config_names["core:simulation_distance"];
    for d in &categories {
        if categories.iter().filter(|o| o.name == d.name).count() > 1 && !errors.iter().any(|e| e.contains(&d.name)) {
            errors.push(format!("entity category {} registered twice", d.name));
        }
    }
    for k in g
        .entities
        .iter_mut()
        .filter(|k| !k.category_name.is_empty())
    {
        match categories.iter().position(|d| d.name == k.category_name) {
            Some(i) => k.category = i as u32,
            None => errors.push(format!("entity {}: no category {}", k.name, k.category_name)),
        }
    }
    g.categories = categories;
    g.sound_categories = sound_categories;
    for (cell, d) in damage_causes {
        if g.damage_cause_names.contains_key(&d.name) {
            errors.push(format!("damage cause {} registered twice", d.name));
            continue;
        }
        let id = g.damage_causes.len() as u32;
        write_cell(cell, id);
        g.damage_cause_names.insert(d.name.clone(), id);
        g.damage_causes.push(d);
    }
    for (cell, d) in game_modes {
        if g.game_mode_names.contains_key(&d.name) {
            errors.push(format!("game mode {} registered twice", d.name));
            continue;
        }
        let id = g.game_modes.len() as u32;
        write_cell(cell, id);
        g.game_mode_names.insert(d.name.clone(), id);
        conditions.push((0, ConditionDef { name: d.name.clone(), answer: Answer::Mode(crate::world::GameMode(id as u16)) }));
        g.game_modes.push(d);
    }
    for (cell, d) in conditions {
        if g.condition_names.contains_key(&d.name) {
            errors.push(format!("condition {} registered twice", d.name));
            continue;
        }
        let id = g.conditions.len() as u32;
        write_cell(cell, id);
        g.condition_names.insert(d.name.clone(), id);
        g.conditions.push(d);
    }
    for m in g.music.iter_mut() {
        for n in std::mem::take(&mut m.when_names) {
            match g.condition_names.get(&n) {
                Some(&id) => m.when.push(id),
                None => errors.push(format!("music {}: no condition {n}", m.name)),
            }
        }
    }
    for (cell, mut d) in weathers {
        if g.weather_names.contains_key(&d.name) {
            errors.push(format!("weather {} registered twice", d.name));
            continue;
        }
        if !d.sound_name.is_empty() {
            d.sound = g.sound_names.get(&d.sound_name).copied();
            if d.sound.is_none() {
                errors.push(format!("weather {}: no sound {}", d.name, d.sound_name));
            }
        }
        let id = g.weathers.len() as u32;
        write_cell(cell, id);
        g.weather_names.insert(d.name.clone(), id);
        g.weathers.push(d);
    }
    for (cell, d) in precipitations {
        if g.precipitation_names.contains_key(&d.name) {
            errors.push(format!("precipitation {} registered twice", d.name));
            continue;
        }
        let id = g.precipitations.len() as u32;
        write_cell(cell, id);
        g.precipitation_names.insert(d.name.clone(), id);
        g.precipitations.push(d);
    }
    for (cell, d) in poses {
        if g.pose_names.contains_key(&d.name) {
            errors.push(format!("pose {} registered twice", d.name));
            continue;
        }
        let id = g.poses.len() as u32;
        write_cell(cell, id);
        g.pose_names.insert(d.name.clone(), id);
        g.poses.push(d);
    }
    for i in 0..g.poses.len() {
        let names = g.poses[i].fallback_names.clone();
        g.poses[i].fallback = names
            .split_whitespace()
            .filter_map(|n| match g.pose_names.get(n) {
                Some(&p) => Some(crate::world::Pose(p as u16)),
                None => {
                    errors.push(format!("pose {}: no pose {n} to fall back to", g.poses[i].name));
                    None
                }
            })
            .collect();
    }
    // Tried highest first, the engine's before a mod's of the same priority.
    g.pose_order = (0..g.poses.len() as u16).map(crate::world::Pose).collect();
    g.pose_order
        .sort_by_key(|p| std::cmp::Reverse(g.poses[p.0 as usize].priority));
    for (cell, d) in text_places {
        if g.text_place_names.contains_key(&d.name) {
            errors.push(format!("text place {} registered twice", d.name));
            continue;
        }
        let id = g.text_places.len() as u32;
        write_cell(cell, id);
        g.text_place_names.insert(d.name.clone(), id);
        g.text_places.push(d);
    }
    for (cell, d) in entity_effects {
        if g.entity_effect_names.contains_key(&d.name) {
            errors.push(format!("entity effect {} registered twice", d.name));
            continue;
        }
        let id = g.entity_effects.len() as u32;
        write_cell(cell, id);
        g.entity_effect_names.insert(d.name.clone(), id);
        g.entity_effects.push(d);
    }
    for s in spawns {
        let Some(&entity) = g.entity_names.get(&s.entity) else {
            errors.push(format!("spawn rule for unknown entity {}", s.entity));
            continue;
        };
        let tag = |name: &str, what: &str, errors: &mut Vec<String>| -> Result<Option<u32>, ()> {
            if name.is_empty() {
                return Ok(None);
            }
            crate::refs::tag(c, name)
                .map(Some)
                .map_err(|e| errors.push(format!("spawn rule for {}: {what}: {e}", s.entity)))
        };
        let what = format!("spawn rule for {}", s.entity);
        let (Ok(biome), Ok(ground)) = (crate::content::biome_tag(c, &what, &s.at, errors), tag(&s.ground, "block", errors)) else {
            continue;
        };
        let dims: Vec<u32> =
            s.at.dims
                .iter()
                .filter_map(|d| dim_names.get(d).copied())
                .collect();
        if dims.len() != s.at.dims.len() {
            errors.push(format!("{what}: unknown dimension in {:?}", s.at.dims));
            continue;
        }
        let category = g.entities[entity as usize].category;
        let height = (g.entities[entity as usize].height.ceil() as i32).max(1);
        let condition = match s.at.condition.as_str() {
            "" => None,
            n => match g.condition_names.get(n) {
                Some(&id) => Some(id),
                None => {
                    errors.push(format!("spawn rule for {}: no condition {n}", s.entity));
                    continue;
                }
            },
        };
        g.spawns.push(SpawnRule {
            entity,
            category,
            biome,
            ground,
            place: s.place,
            height,
            condition,
            dims,
            light: s.at.light,
            y: s.at.y,
            group: s.group,
            weight: s.weight,
            chance: s.chance,
        });
    }
    // Natural spawning (`spawns`) runs while this is on.

    let mut events: Vec<EventDef> = ENGINE_EVENTS
        .iter()
        .map(|(n, s, size)| EventDef { name: n.to_string(), schema: s.to_string(), size: (*size).into(), handlers: Vec::new() })
        .collect();
    event_handlers.sort_by_key(|h| order(h.0));
    for (owner, name, schema, cb) in event_handlers {
        match events.iter_mut().find(|e| e.name == name) {
            Some(e) if e.schema != schema => errors.push(format!("event {name} is '{}' but a handler takes '{schema}'", e.schema)),
            Some(e) => e.handlers.push((owner, cb)),
            None => events.push(EventDef { name, schema, size: 0.into(), handlers: vec![(owner, cb)] }),
        }
    }
    events.sort_by(|a, b| a.name.cmp(&b.name));
    g.event_names = events
        .iter()
        .enumerate()
        .map(|(i, e)| (e.name.clone(), i as u32))
        .collect();
    g.events = events;
    interceptors.sort_by_key(|h| order(h.0));
    for (owner, name, cb) in interceptors {
        g.interceptors.entry(name).or_default().push((owner, cb));
    }
    modifiers.sort_by_key(|m| (m.2, order(m.0)));
    for (owner, name, stage, cb) in modifiers {
        g.modifiers
            .entry(name)
            .or_default()
            .push((stage, owner, cb));
    }
    for (name, default, min, max) in attributes {
        if let Some(&a) = g.attr_names.get(&name) {
            // Declared again the same way is fine (mods sharing it); differently is not.
            let d = &g.attributes[a as usize];
            if (d.default, d.min, d.max) != (default, min.min(max), max) {
                errors.push(format!("attribute {name} is declared twice with different values"));
            }
            continue;
        }
        g.attr_names.insert(name.clone(), g.attributes.len() as u32);
        g.attributes
            .push(AttrDef { hash: name_hash(&name), comp: c.id(&name), name, default, min: min.min(max), max });
    }
    packets.sort_by(|a, b| a.1.cmp(&b.1).then(a.3.cmp(&b.3)));
    for (owner, name, schema, side, cb) in packets {
        let id = *g.packet_names.entry(name.clone()).or_insert_with(|| {
            g.packets
                .push(PacketDef { schema: schema.clone(), ..Default::default() });
            g.packets.len() as u32 - 1
        });
        let def = &mut g.packets[id as usize];
        if def.schema != schema {
            errors.push(format!("channel {name} carries '{}' on one side and '{schema}' on the other", def.schema));
        }
        let slot = if side == 0 { &mut def.server } else { &mut def.client };
        if slot.is_some() {
            errors.push(format!("packet channel {name} has two {} handlers", if side == 0 { "server" } else { "client" }));
        }
        *slot = Some((owner, cb));
    }

    // Entity storage, indexed by component id like everything else.
    g.components = c
        .names
        .iter()
        .map(|n| ComponentDef { name: n.clone(), ..Default::default() })
        .collect();
    let energy = size_of::<weft_abi::Energy>() as u32;
    components.push(RawComponent {
        owner: ENGINE,
        def: ComponentDef {
            name: abi::comp::ENERGY.into(),
            size: energy,
            flags: weft_abi::component_flags::SAVE | weft_abi::component_flags::SYNC_MENU,
            default: vec![0; energy as usize],
            runtime: true,
        },
        schema: String::new(),
    });
    // Variable length (size 0): attribute modifiers, added on demand.
    components.push(RawComponent {
        owner: ENGINE,
        def: ComponentDef { name: MODIFIERS.into(), size: 0, flags: weft_abi::component_flags::SAVE, default: Vec::new(), runtime: true },
        schema: String::new(),
    });
    components.push(RawComponent {
        owner: ENGINE,
        def: ComponentDef { name: TIMERS.into(), size: 0, flags: weft_abi::component_flags::SAVE, default: Vec::new(), runtime: true },
        schema: String::new(),
    });
    // A block entity's own name (a renamed chest).
    components.push(RawComponent {
        owner: ENGINE,
        def: ComponentDef {
            name: abi::comp::NAME.into(),
            size: 0,
            flags: weft_abi::component_flags::SAVE | weft_abi::component_flags::CARRIED,
            default: Vec::new(),
            runtime: true,
        },
        schema: String::new(),
    });
    let shown = weft_abi::component_flags::SYNC_WATCHERS;
    let own = weft_abi::component_flags::SYNC_OWNER | shown;
    let mut shape = vec![0u8; 12];
    shape.extend(u32::MAX.to_le_bytes());
    let save = weft_abi::component_flags::SAVE;
    for (name, size, flags, default) in [
        (HEALTH, 8, own, vec![0; 8]),
        (BURNING, 1, shown, vec![0]),
        (WORN, 0, shown, Vec::new()),
        (SHAPE, 16, own, shape),
        (HELD, 4, shown, vec![0; 4]),
        (OFF_HELD, 4, shown, vec![0; 4]),
        (USING, 4, shown, vec![0; 4]),
        (SWAPS, 0, own | shown, Vec::new()),
        (LOOPING_ACTION, 0, shown, Vec::new()),
        (EFFECT, 0, shown, Vec::new()),
        (FORCED_POSE, 0, own | shown, Vec::new()),
        (RIDING, 0, own | shown | save, Vec::new()),
        (SIMULATED, 8, save, vec![0; 8]),
    ] {
        components.push(RawComponent {
            owner: ENGINE,
            def: ComponentDef { name: name.into(), size, flags, default, runtime: true },
            schema: String::new(),
        });
    }
    for rc in components {
        let Some(id) = c.id(&rc.def.name) else { continue };
        g.components[id as usize] = ComponentDef { runtime: true, ..rc.def };
    }
    g.component_names = g
        .components
        .iter()
        .enumerate()
        .filter(|(_, d)| d.runtime)
        .map(|(i, d)| (d.name.clone(), i as u32))
        .collect();
    g.energy = g.component_names[abi::comp::ENERGY];
    g.modifiers_comp = g.component_names[MODIFIERS];
    g.timers_comp = g.component_names[TIMERS];
    g.name_comp = g.component_names[abi::comp::NAME];
    g.carried = g
        .components
        .iter()
        .enumerate()
        .filter(|(_, d)| d.runtime && d.flags & weft_abi::component_flags::CARRIED != 0)
        .map(|(i, _)| i as u32)
        .collect();
    g.field_comps = [HEALTH, BURNING, WORN, SHAPE, HELD, OFF_HELD, USING, SWAPS, LOOPING_ACTION, EFFECT, FORCED_POSE, RIDING]
        .map(|n| g.component_names[n]);
    g.simulated_comp = g.component_names[SIMULATED];
    let with_flag = |f: u32| {
        g.components
            .iter()
            .enumerate()
            .filter(|(_, d)| d.runtime && d.flags & f != 0)
            .map(|(i, _)| i as u32)
            .collect::<Vec<u32>>()
    };
    g.watched_comps = with_flag(abi::component_flags::SYNC_WATCHERS);
    g.menu_comps = with_flag(abi::component_flags::SYNC_MENU);
    let any_sync = abi::component_flags::SYNC_OWNER | abi::component_flags::SYNC_WATCHERS | abi::component_flags::SYNC_MENU;
    g.synced_comps = g
        .components
        .iter()
        .map(|d| d.runtime && d.flags & any_sync != 0)
        .collect();
    systems.sort_by_key(|s| order(s.0));
    for (owner, name, schema, cb) in systems {
        match g.component_names.get(&name) {
            Some(&id) if c.schemas[id as usize] == schema || schema.is_empty() => g.systems.push((owner, id, cb)),
            Some(&id) => errors.push(format!("system over {name}: it is '{}', the system takes '{schema}'", c.schemas[id as usize])),
            None => errors.push(format!("system over {name}: no entity component of that name")),
        }
    }
    for (kind, comp, value) in attach {
        match (g.entity_names.get(&kind), g.component_names.get(&comp)) {
            (Some(&k), Some(&cid)) => {
                let size = g.components[cid as usize].size as usize;
                match value {
                    Some(v) if v.len() != size => {
                        errors.push(format!("attaching {comp} to {kind}: a value of {} bytes, the component has {size}", v.len()))
                    }
                    Some(v) => g.entities[k as usize]
                        .values
                        .push((cid, v.into_boxed_slice())),
                    None => {}
                }
                g.entities[k as usize].components.push(cid);
            }
            _ => errors.push(format!("cannot attach {comp} to {kind}: unknown entity or component")),
        }
    }

    world_ticks.sort_by_key(|h: &(ModId, Callback)| order(h.0));
    g.world_ticks = world_ticks;

    menus.sort_by(|a, b| a.1.name.cmp(&b.1.name));
    for (i, (cell, m)) in menus.into_iter().enumerate() {
        write_cell(cell, i as u32);
        g.menu_names.insert(m.name.clone(), i as u32);
        g.menus.push(m);
    }
    g.screens.sort_by_key(|s| order(s.owner));
    g.screen_parts.sort_by_key(|p| (p.order, order(p.owner)));
    g.player_layout = match player_inventory {
        Some((_, groups)) => {
            if !groups
                .iter()
                .any(|g| g.3 & weft_abi::slot_flags::HOTBAR != 0)
            {
                errors.push("player inventory: no group is the hotbar (`.hotbar()`)".into());
            }
            PlayerLayout::new(&groups, equipment)
        }
        None => PlayerLayout {
            ..PlayerLayout::new(&[("hotbar".into(), 9, 1, weft_abi::slot_flags::HOTBAR), ("main".into(), 9, 3, 0)], equipment)
        },
    };
    // Kinds that wear get the equipment slots after their own, the player where its layout says.
    for k in &mut g.entities {
        if k.name == PLAYER {
            k.worn = g.player_layout.worn.clone();
        } else if k.flags & weft_abi::entity_flags::WEARS != 0 {
            k.worn = k.slots..k.slots + g.player_layout.worn.len() as u32;
            k.slots = k.worn.end;
        }
    }
    // Menus showing the player's equipment get a group per slot, taking what is worn there.
    let layout = g.player_layout.clone();
    for m in &mut g.menus {
        let Some(at) = m
            .groups
            .iter()
            .position(|g| g.owner == weft_abi::slot_owner::PLAYER_EQUIPMENT)
        else {
            continue;
        };
        m.groups.remove(at);
        for (i, e) in layout.equipment.iter().enumerate().rev() {
            let off = e.name == weft_abi::OFF_HAND_SLOT;
            // The other hand shows with the equipment but is numbered last, as in vanilla.
            let at = if off { m.groups.len() } else { at };
            m.groups.insert(
                at,
                GroupDef {
                    name: e.name.clone(),
                    owner: weft_abi::slot_owner::PLAYER,
                    start: layout.worn.start + i as u32,
                    cols: 1,
                    rows: 1,
                    flags: if off { weft_abi::slot_flags::OFF_HAND } else { 0 },
                    accepts: if off { String::new() } else { format!("worn/{}", e.name) },
                    limit: if off { 0 } else { 1 },
                    size: 18.0,
                    background: (!e.background.is_empty()).then(|| e.background.clone()),
                    tooltip: None,
                    priority: 1,
                },
            );
        }
    }
    g.player_menu = player_menu.and_then(|(_, n)| {
        let m = g.menu_names.get(&n).copied();
        if m.is_none() {
            errors.push(format!("player menu {n} is not registered"));
        }
        m
    });

    for (block, entity, slots) in block_entities {
        match (block_names.get(&block), g.entity_names.get(&entity)) {
            (Some(&b), Some(&e)) => {
                if g.block_entities.insert(b, e).is_some_and(|old| old != e) {
                    errors.push(format!("block {block} has two block entities"));
                }
                g.entities[e as usize].slots = slots;
            }
            _ => errors.push(format!("block entity {block} -> {entity}: unknown block or entity")),
        }
    }

    let _ = items;
    let tag_names: FxHashMap<&str, u32> = tag_items
        .keys()
        .map(|&t| (c.names[t as usize].as_str(), t))
        .collect();
    let mut expanded = Vec::new();
    for r in recipes {
        match expand_variables(r, &tag_names) {
            Ok(list) => expanded.extend(list),
            Err(e) => errors.push(e),
        }
    }
    let ing = |s: &str, errors: &mut Vec<String>| -> Ingredient {
        if s.is_empty() {
            return Ingredient::Empty;
        }
        let r = match crate::refs::Ref::parse(s) {
            Ok(r) => r,
            Err(e) => {
                errors.push(format!("recipe: {e}"));
                return Ingredient::Empty;
            }
        };
        let found = match &r {
            crate::refs::Ref::Tags(p) if !p.contains('*') => {
                crate::refs::items(&r, item_names, c, tag_items).map(|_| Ingredient::Tag(c.id(p).unwrap_or(u32::MAX)))
            }
            crate::refs::Ref::Tags(_) => crate::refs::items(&r, item_names, c, tag_items).map(Ingredient::Any),
            _ => crate::refs::item(&r, item_names, c, tag_items).map(Ingredient::Item),
        };
        found.unwrap_or_else(|e| {
            errors.push(format!("recipe: {e}"));
            Ingredient::Empty
        })
    };
    let one = |s: &str| crate::refs::Ref::parse(s).and_then(|r| crate::refs::item(&r, item_names, c, tag_items));
    let recipes = expanded;
    for r in recipes {
        if !g.recipe_kinds.contains(&r.kind) {
            errors.push(format!("recipe for {}: no recipe kind {}", r.output, r.kind));
            continue;
        }
        let output = match one(&r.output) {
            Ok(o) => o,
            Err(e) => {
                errors.push(format!("recipe for {}: {e}", r.output));
                continue;
            }
        };
        let shaped = if r.pattern.is_empty() {
            None
        } else {
            let h = r.pattern.len() as u32;
            let w = r
                .pattern
                .iter()
                .map(|p| p.chars().count())
                .max()
                .unwrap_or(0) as u32;
            let mut cells = Vec::new();
            for row in &r.pattern {
                let chars: Vec<char> = row.chars().collect();
                for x in 0..w as usize {
                    let ch = chars.get(x).copied().unwrap_or(' ');
                    let cell = if ch == ' ' {
                        Ingredient::Empty
                    } else {
                        match r.keys.iter().find(|k| k.0 == ch) {
                            Some((_, item)) => ing(item, errors),
                            None => {
                                errors.push(format!("recipe for {}: key '{ch}' not defined", r.output));
                                Ingredient::Empty
                            }
                        }
                    };
                    cells.push(cell);
                }
            }
            Some((w, h, cells))
        };
        let inputs: Vec<Ingredient> = r.inputs.iter().map(|s| ing(s, errors)).collect();
        let input_counts = (0..inputs.len())
            .map(|i| r.input_counts.get(i).copied().unwrap_or(1).max(1))
            .collect();
        let mut outputs = vec![RecipeOutput { what: Ingredient::Item(output), count: r.count, chance: 1.0 }];
        for (name, count, chance) in &r.outputs {
            match one(name).map(Ingredient::Item) {
                Ok(w) => outputs.push(RecipeOutput { what: w, count: *count, chance: chance.clamp(0.0, 1.0) }),
                Err(e) => errors.push(format!("recipe for {}: output: {e}", r.output)),
            }
        }
        let values = r
            .values
            .iter()
            .filter_map(|v| c.id(&v.name).map(|id| (id, v.bytes.clone())))
            .collect();
        let hidden = r.flags & weft_abi::recipe_flags::HIDDEN != 0;
        g.recipes
            .entry(r.kind)
            .or_default()
            .push(Recipe { shaped, inputs, input_counts, outputs, time: r.time, values, hidden });
    }
    g
}

impl Game {
    /// Entities of `kind` belong to a block (machines, chests): they come and go with it.
    pub fn is_block_entity(&self, kind: u32) -> bool {
        self.block_entities.values().any(|&k| k == kind)
    }

    pub fn component(&self, name: &str) -> Option<u32> {
        self.component_names.get(name).copied()
    }

    /// Bytes read back for a component, cut or filled to its size as it is now (0: variable, like
    /// attribute modifiers).
    pub fn fit_component(&self, id: u32, mut bytes: Vec<u8>) -> Box<[u8]> {
        let size = self.components[id as usize].size as usize;
        if size > 0 {
            bytes.resize(size, 0);
        }
        bytes.into_boxed_slice()
    }

    /// Finds the recipe whose pattern fits the grid (any position, mirrored allowed) or whose
    /// shapeless inputs match the grid contents exactly.
    pub fn match_grid(&self, kind: &str, grid: &[u32], w: u32, h: u32, tags: &Components) -> Option<&Recipe> {
        let list = self.recipes.get(kind)?;
        // Trim the grid to the bounding box of its items.
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if grid[(y * w + x) as usize] != 0 {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
            }
        }
        if x1 == 0 {
            return None;
        }
        let (bw, bh) = (x1 - x0, y1 - y0);
        let at = |x: u32, y: u32| grid[((y0 + y) * w + x0 + x) as usize];
        let items: Vec<u32> = grid.iter().copied().filter(|&i| i != 0).collect();
        list.iter().find(|r| match &r.shaped {
            Some((rw, rh, cells)) => {
                if *rw != bw || *rh != bh {
                    return false;
                }
                let fits = |mirror: bool| {
                    (0..bh).all(|y| {
                        (0..bw).all(|x| {
                            let cx = if mirror { bw - 1 - x } else { x };
                            cells[(y * rw + cx) as usize].matches(at(x, y), tags)
                        })
                    })
                };
                fits(false) || fits(true)
            }
            None => {
                if r.inputs.len() != items.len() {
                    return false;
                }
                let mut used = vec![false; items.len()];
                r.inputs.iter().all(|ing| {
                    let found = items
                        .iter()
                        .enumerate()
                        .find(|(i, it)| !used[*i] && ing.matches(**it, tags));
                    match found {
                        Some((i, _)) => {
                            used[i] = true;
                            true
                        }
                        None => false,
                    }
                })
            }
        })
    }

    pub fn process(&self, kind: &str, input: u32, tags: &Components) -> Option<&Recipe> {
        self.recipes
            .get(kind)?
            .iter()
            .find(|r| r.inputs.first().is_some_and(|i| i.matches(input, tags)))
    }

    /// For each input of `r`, the stack in `stacks` that holds enough of it (each stack used
    /// once); `None` if one is missing. Fluids are not in stacks and never match.
    pub fn find_inputs(&self, r: &Recipe, stacks: &[weft_abi::ItemStack], tags: &Components) -> Option<Vec<usize>> {
        let mut used = vec![false; stacks.len()];
        let mut out = Vec::with_capacity(r.inputs.len());
        for (ing, &n) in r.inputs.iter().zip(&r.input_counts) {
            if matches!(ing, Ingredient::Empty) {
                continue;
            }
            let i = stacks
                .iter()
                .enumerate()
                .position(|(i, s)| !used[i] && !s.is_empty() && s.count >= n && ing.matches(s.item, tags))?;
            used[i] = true;
            out.push(i);
        }
        Some(out)
    }
}

/// Sounds the engine plays itself (files in its own assets, packs may replace them).
fn engine_sounds() -> Vec<(usize, SoundDef)> {
    let one = |name: &str, file: &str, category: &str, volume: f32, pitch: [f32; 2]| {
        (
            0,
            SoundDef {
                name: name.into(),
                files: vec![file.into()],
                category: 0,
                category_name: category.into(),
                volume,
                pitch,
                range: 16.0,
                full_within: 1.0,
                falloff: 0,
            },
        )
    };
    vec![
        one("core:ui.click", "core:ui/click", "core:ui", 0.6, [1.0, 1.0]),
        one("core:item.pickup", "core:item/pickup", "core:players", 0.4, [1.4, 2.2]),
    ]
}

/// Kinds the engine spawns itself; mods may replace their look by registering the same name.
fn engine_particles() -> Vec<RawParticle> {
    use abi::{particle_event as pe, particle_flags as pf, particle_then as pt, particle_when as pw};
    let kind = |name: &str, textures: Vec<String>, desc: abi::ParticleDesc, rules: Vec<RawParticleRule>| RawParticle {
        name: name.into(),
        cell: 0,
        textures,
        desc,
        shader: String::new(),
        rules,
    };
    let rule = |event, then, into: &str| RawParticleRule { event, when: pw::ANY, tag: String::new(), then, amount: 0.0, into: into.into() };
    let generic: Vec<String> = (0..8)
        .map(|i| format!("core:particle/generic_{}", 7 - i))
        .collect();
    let one = |t: &str| vec![format!("core:particle/{t}")];
    vec![
        kind(
            abi::particle::BLOCK,
            Vec::new(),
            abi::ParticleDesc {
                life_min: 0.2,
                life_max: 1.0,
                size_min: 0.05,
                size_max: 0.1,
                gravity: 16.0,
                color: 0x999999FF,
                color_end: 0x999999FF,
                flags: pf::COLLIDE,
                ..abi::ParticleDesc::DEFAULT
            },
            Vec::new(),
        ),
        kind(
            abi::particle::POOF,
            generic.clone(),
            abi::ParticleDesc {
                life_min: 0.4,
                life_max: 1.1,
                size_min: 0.15,
                size_max: 0.3,
                gravity: -2.0,
                drag: 0.9,
                color: 0xD0D0D0FF,
                color_end: 0x909090FF,
                ..abi::ParticleDesc::DEFAULT
            },
            Vec::new(),
        ),
        kind(
            abi::particle::CRIT,
            generic.clone(),
            abi::ParticleDesc {
                life_min: 0.3,
                life_max: 0.7,
                size_min: 0.1,
                size_max: 0.2,
                gravity: 8.0,
                drag: 0.8,
                color: 0xFFE9A0FF,
                color_end: 0xC08030FF,
                flags: pf::FULLBRIGHT,
                ..abi::ParticleDesc::DEFAULT
            },
            Vec::new(),
        ),
        kind(
            abi::particle::FLAME,
            one("flame"),
            abi::ParticleDesc {
                life_min: 0.3,
                life_max: 0.8,
                size_min: 0.08,
                size_max: 0.14,
                gravity: 0.0,
                drag: 0.96,
                flags: pf::FULLBRIGHT | pf::SHRINK,
                ..abi::ParticleDesc::DEFAULT
            },
            Vec::new(),
        ),
        kind(
            abi::particle::SMOKE,
            generic,
            abi::ParticleDesc {
                life_min: 0.6,
                life_max: 1.4,
                size_min: 0.08,
                size_max: 0.16,
                gravity: -1.5,
                drag: 0.96,
                color: 0x505050FF,
                color_end: 0x303030FF,
                ..abi::ParticleDesc::DEFAULT
            },
            Vec::new(),
        ),
        // Pops at the surface, as vanilla's does.
        kind(
            abi::particle::BUBBLE,
            one("bubble"),
            abi::ParticleDesc {
                life_min: 0.5,
                life_max: 1.5,
                size_min: 0.05,
                size_max: 0.1,
                gravity: -4.0,
                drag: 0.85,
                ..abi::ParticleDesc::DEFAULT
            },
            vec![rule(pe::LEAVE_LIQUID, pt::DIE, "")],
        ),
        kind(
            abi::particle::SPLASH,
            one("splash"),
            abi::ParticleDesc {
                life_min: 0.2,
                life_max: 0.5,
                size_min: 0.08,
                size_max: 0.14,
                gravity: 16.0,
                flags: pf::COLLIDE,
                ..abi::ParticleDesc::DEFAULT
            },
            Vec::new(),
        ),
        // Splashes where it lands, as vanilla's falling drip turns into a landing one.
        kind(
            abi::particle::DRIP,
            one("drip"),
            abi::ParticleDesc { life_min: 0.8, life_max: 1.2, size_min: 0.04, size_max: 0.06, gravity: 16.0, ..abi::ParticleDesc::DEFAULT },
            vec![rule(pe::LAND, pt::BECOME_STILL, abi::particle::SPLASH)],
        ),
    ]
}

/// Ids by sorted name; a later registration of a name (a mod after the engine) replaces it.
fn freeze_particles(g: &mut Game, raws: Vec<RawParticle>, errors: &mut Vec<String>) {
    use abi::{particle_event as pe, particle_then as pt, particle_when as pw};
    let mut by_name: std::collections::BTreeMap<String, RawParticle> = Default::default();
    for p in raws {
        if p.name.is_empty() {
            errors.push("particle without a name".into());
            continue;
        }
        by_name.insert(p.name.clone(), p);
    }
    let ids: FxHashMap<String, u32> = by_name
        .keys()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as u32))
        .collect();
    let mut tex_ids: FxHashMap<String, u32> = FxHashMap::default();
    for (i, (name, p)) in by_name.into_iter().enumerate() {
        let frames = p
            .textures
            .iter()
            .map(|t| {
                *tex_ids.entry(t.clone()).or_insert_with(|| {
                    g.particle_textures.push(t.clone());
                    g.particle_textures.len() as u32 - 1
                })
            })
            .collect();
        let mut rules = Vec::new();
        for r in &p.rules {
            let into = match r.then {
                pt::BECOME | pt::BECOME_STILL => match ids.get(&r.into) {
                    Some(&id) => id,
                    None => {
                        errors.push(format!("particle {name} turns into {}, which no one registers", r.into));
                        continue;
                    }
                },
                _ => 0,
            };
            let tag = if r.when == pw::TAG {
                match g.particle_tags.iter().position(|t| *t == r.tag) {
                    Some(i) => i as u32,
                    None if g.particle_tags.len() < PARTICLE_TAGS => {
                        g.particle_tags.push(r.tag.clone());
                        g.particle_tags.len() as u32 - 1
                    }
                    None => {
                        errors.push(format!(
                            "particle {name} asks about tag {}: particle rules ask about {} tags at most ({})",
                            r.tag,
                            PARTICLE_TAGS,
                            g.particle_tags.join(", ")
                        ));
                        continue;
                    }
                }
            } else {
                0
            };
            if r.event > pe::EXPIRE || r.when > pw::TAG || r.then > pt::BECOME_STILL {
                errors.push(format!("particle {name}: a rule with an event, place or action that doesn't exist"));
                continue;
            }
            rules.push(ParticleRule { event: r.event, when: r.when, tag, then: r.then, amount: r.amount, into });
        }
        let d = p.desc;
        write_cell(p.cell, i as u32);
        let hits = rules
            .iter()
            .any(|r| matches!(r.event, pe::LAND | pe::WALL | pe::CEILING));
        let flags = if hits { d.flags | abi::particle_flags::COLLIDE } else { d.flags };
        g.particle_names.insert(name.clone(), i as u32);
        g.particles.push(ParticleDef {
            name,
            frames,
            life_min: d.life_min.min(d.life_max).max(0.01),
            life_max: d.life_max.max(d.life_min).max(0.01),
            size_min: d.size_min.min(d.size_max),
            size_max: d.size_max.max(d.size_min),
            gravity: d.gravity,
            drag: d.drag.clamp(0.0, 1.0),
            color: d.color,
            color_end: d.color_end,
            flags,
            shader: p.shader,
            rules,
            longest: 0.0,
        });
    }
    let translucent = |k: &ParticleDef| k.flags & abi::particle_flags::TRANSLUCENT != 0;
    for k in &g.particles {
        for r in k
            .rules
            .iter()
            .filter(|r| matches!(r.then, pt::BECOME | pt::BECOME_STILL))
        {
            let into = &g.particles[r.into as usize];
            if translucent(k) != translucent(into) {
                errors.push(format!("particle {} turns into {}, which is drawn the other way (translucent or not)", k.name, into.name));
            }
        }
    }
    // Turning into each other in a loop lives on; a chain is counted this many kinds deep.
    fn longest(all: &[ParticleDef], k: usize, depth: u32) -> f32 {
        let next = all[k]
            .rules
            .iter()
            .filter(|r| depth > 0 && matches!(r.then, pt::BECOME | pt::BECOME_STILL))
            .map(|r| longest(all, r.into as usize, depth - 1))
            .fold(0.0, f32::max);
        all[k].life_max + next
    }
    let lives: Vec<f32> = (0..g.particles.len())
        .map(|k| longest(&g.particles, k, 8))
        .collect();
    for (k, l) in g.particles.iter_mut().zip(lives) {
        k.longest = l;
    }
}

/// A recipe naming tags with a variable, `#ores/{m}` -> `#ingots/{m}`, stands for one recipe per
/// value: the first tag with the variable gives the values, and the copies whose other tags no
/// item carries are left out. So every mod's ores smelt into the matching ingots. (`*` is "any",
/// not a variable.) A variable with no value at all is an error.
fn expand_variables(r: RawRecipe, tags: &FxHashMap<&str, u32>) -> Result<Vec<RawRecipe>, String> {
    let names: Vec<&String> = r
        .keys
        .iter()
        .map(|k| &k.1)
        .chain(&r.inputs)
        .chain(std::iter::once(&r.output))
        .chain(r.outputs.iter().map(|o| &o.0))
        .collect();
    let Some(pattern) = names
        .iter()
        .find_map(|n| n.strip_prefix('#').filter(|t| t.contains('{')))
    else {
        return Ok(vec![r]);
    };
    let (pre, rest) = pattern.split_once('{').unwrap();
    let Some((var, post)) = rest.split_once('}') else { return Err(format!("recipe for {}: #{pattern} has no closing }}", r.output)) };
    let var = format!("{{{var}}}");
    let mut values: Vec<&str> = tags
        .keys()
        .filter_map(|t| t.strip_prefix(pre).and_then(|rest| rest.strip_suffix(post)))
        .filter(|v| !v.is_empty())
        .collect();
    values.sort_unstable();
    if values.is_empty() {
        return Err(format!("recipe for {}: no item has a tag like #{pattern}", r.output));
    }
    let mut out = Vec::new();
    for v in values {
        let sub = |s: &String| s.replace(&var, v);
        let usable = names.iter().all(|n| {
            n.strip_prefix('#')
                .is_none_or(|t| tags.contains_key(t.replace(&var, v).as_str()) || t.contains('*'))
        });
        if usable {
            out.push(RawRecipe {
                kind: r.kind.clone(),
                pattern: r.pattern.clone(),
                keys: r.keys.iter().map(|(c, n)| (*c, sub(n))).collect(),
                inputs: r.inputs.iter().map(sub).collect(),
                input_counts: r.input_counts.clone(),
                output: sub(&r.output),
                count: r.count,
                time: r.time,
                outputs: r.outputs.iter().map(|o| (sub(&o.0), o.1, o.2)).collect(),
                values: r.values.clone(),
                flags: r.flags,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod box_unwrap {
    use super::Cuboid;

    fn arm(mirror: bool) -> [[f32; 4]; 6] {
        Cuboid { origin: [0.0; 3], size: [4.0, 12.0, 4.0], uv: [40.0, 16.0], inflate: 0.0, mirror, faces: None }.face_rects()
    }

    #[test]
    fn wraps_around_the_box() {
        let r = arm(false);
        // Each side's edge meets the same edge of the strip beside it in the unwrap.
        assert_eq!(r[1], [40.0, 20.0, 4.0, 12.0], "+X, left of the front");
        assert_eq!(r[4], [44.0, 20.0, 4.0, 12.0], "-Z, the front");
        assert_eq!(r[0], [48.0, 20.0, 4.0, 12.0], "-X, right of the front");
        assert_eq!(r[5], [52.0, 20.0, 4.0, 12.0], "+Z, the back");
        assert_eq!(r[3], [48.0, 20.0, -4.0, -4.0], "the top, meeting the front");
    }

    #[test]
    fn mirrored_is_the_other_side_read_backwards() {
        let r = arm(true);
        assert_eq!(r[0], [44.0, 20.0, -4.0, 12.0], "-X has what +X had");
        assert_eq!(r[4], [48.0, 20.0, -4.0, 12.0], "the front flipped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variable_recipes_expand_per_material() {
        let tags: FxHashMap<&str, u32> = [("ores/iron", 1), ("ingots/iron", 2), ("ores/tin", 3), ("ingots/tin", 4), ("ores/ruby", 5)]
            .into_iter()
            .collect();
        let r = RawRecipe {
            kind: "smelting".into(),
            inputs: vec!["#ores/{m}".into()],
            output: "#ingots/{m}".into(),
            count: 1,
            time: 200,
            ..Default::default()
        };
        let out: Vec<(String, String)> = expand_variables(r, &tags)
            .unwrap()
            .into_iter()
            .map(|r| (r.inputs[0].clone(), r.output))
            .collect();
        // Ruby has no ingot, so it is left out.
        assert_eq!(out, vec![("#ores/iron".into(), "#ingots/iron".into()), ("#ores/tin".into(), "#ingots/tin".into())]);
    }
}

#[cfg(test)]
mod sky_tests {
    use super::*;

    fn raw(name: &str, set: impl Fn(&mut abi::SkyDesc)) -> RawSky {
        let mut desc = abi::SkyDesc::DEFAULT;
        set(&mut desc);
        RawSky { dimension: "a:d".into(), name: name.into(), texture: String::new(), shader: String::new(), desc }
    }

    /// A later registration changes what it sets and keeps the rest; `remove` takes it out.
    #[test]
    fn later_registrations_merge() {
        let mut g = Game::default();
        let dims: FxHashMap<String, u32> = [("a:d".to_string(), 0)].into_iter().collect();
        let raws = vec![
            raw("moon", |d| {
                d.size = 23.0;
                d.phase = 0.5;
            }),
            raw("moon", |d| d.size = 40.0),
            raw("gone", |d| d.size = 1.0),
            raw("gone", |d| d.remove = 1),
        ];
        let raws = raws
            .into_iter()
            .map(|mut r| {
                r.texture = "a:sky/x".into();
                r
            })
            .collect();
        let mut errors = Vec::new();
        freeze_sky(&mut g, raws, &dims, &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(g.sky[0].len(), 1);
        let m = &g.sky[0][0];
        assert_eq!((m.name.as_str(), m.size, m.phase), ("moon", 40f32.to_radians(), 0.5));
        assert!(m.orbit.is_infinite(), "nobody set an orbit, so it follows the day");
    }
}

/// A camera mode as frozen; `call` none for the engine's own, which it moves itself.
#[derive(Clone)]
pub struct CameraMode {
    pub name: String,
    pub flags: u32,
    pub key: Option<String>,
    pub call: Option<(ModId, Callback)>,
    pub transition: f32,
    pub easing: u32,
}

impl CameraMode {
    pub fn has(&self, f: u32) -> bool {
        self.flags & f != 0
    }
}

/// A `Host::reg_camera` call: a new mode or edits to one.
pub struct RawCamera {
    pub owner: ModId,
    pub name: String,
    pub flags: u32,
    pub flags_off: u32,
    pub key: String,
    pub call: Callback,
    pub transition: f32,
    pub easing: u32,
}

/// The engine's camera modes, in id order.
pub mod camera {
    pub const FIRST_PERSON: u32 = 0;
    pub const BEHIND: u32 = 1;
    pub const FRONT: u32 = 2;
    pub const FREE: u32 = 3;
}

fn freeze_cameras(g: &mut Game, raws: Vec<RawCamera>) {
    use abi::camera_flags as cf;
    let builtin = |name: &str, flags: u32, key: Option<&str>| CameraMode {
        name: name.to_string(),
        flags,
        key: key.map(str::to_string),
        call: None,
        transition: 0.0,
        easing: abi::easing::IN_OUT,
    };
    g.cameras = vec![
        builtin("core:first_person", cf::HAND | cf::HUD | cf::EFFECTS | cf::CYCLE, None),
        builtin("core:behind", cf::BODY | cf::HUD | cf::CLIP | cf::CYCLE, None),
        builtin("core:front", cf::BODY | cf::HUD | cf::CLIP | cf::CYCLE, None),
        builtin("core:free", cf::BODY | cf::SOLID | cf::DETACHED | cf::NO_INTERACT | cf::ASK, Some("core:freecam")),
    ];
    for r in raws {
        let i = match g.cameras.iter().position(|m| m.name == r.name) {
            Some(i) => i,
            None => {
                g.cameras
                    .push(CameraMode { name: r.name.clone(), flags: 0, key: None, call: None, transition: 0.0, easing: r.easing });
                g.cameras.len() - 1
            }
        };
        let m = &mut g.cameras[i];
        m.flags = (m.flags | r.flags) & !r.flags_off;
        if !r.key.is_empty() {
            m.key = Some(r.key);
        }
        if r.call.func.is_some() {
            m.call = Some((r.owner, r.call));
        }
        if r.transition.is_finite() {
            (m.transition, m.easing) = (r.transition.max(0.0), r.easing);
        }
    }
    g.camera_names = g
        .cameras
        .iter()
        .enumerate()
        .map(|(i, m)| (m.name.clone(), i as u32))
        .collect();
}
