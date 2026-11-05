//! Engine side of the mod ABI: the function table mods call, and the wrapper the engine uses to
//! call mod callbacks (profiling and fault isolation live there).

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use glam::{DVec3, IVec3};
use weft_abi::{self as abi, Callback, Host, ItemStack, ModId, Str};

use crate::content::{self, RawRegistrar, content, try_content};
use crate::registry;
use crate::server::Dim;
use crate::server::menu::MenuCall;

pub static HOST: Host = Host {
    log: host_log,
    log_at,
    log_enabled,
    reg_block,
    reg_item,
    reg_biome,
    reg_dimension,
    reg_entity,
    reg_entity_model,
    reg_entity_component,
    reg_world_tick,
    emit_after,
    reg_recipe,
    reg_menu,
    set_player_menu,
    set_block_entity,
    reg_hud_element,
    reg_command,
    reg_ore,
    reg_scatter,
    reg_decoration,
    reg_creative_tab,
    reg_set,
    find,
    block_of,
    state_prop,
    state_with,
    state_cycle,
    state_flags,
    find_component,
    component_table,
    stack_get,
    stack_with,
    storage_slots,
    component_info,
    block_info,
    item_info,
    block_get,
    world_dimension,
    block_set,
    part_list,
    part_add,
    part_replace,
    schedule_tick,
    game_time,
    random,
    light,
    daylight,
    entity_spawn,
    entity_despawn,
    entity_mount,
    entity_dismount,
    entity_vehicle,
    entity_riders,
    entity_get,
    entity_push,
    entity_set,
    entity_walk,
    entity_damage,
    entity_attack,
    entity_in_reach,
    raycast,
    entities_in,
    in_liquid,
    entity_swing_strength,
    component,
    player_count,
    player_at,
    game_mode,
    inventory_len,
    inventory_get,
    inventory_set,
    inventory_insert,
    selected_slot,

    block_entity,
    open_menu,
    menu_group,
    inventory_group,
    menu_get,
    menu_set,
    recipe_match,
    recipe_process,
    recipe_find,
    recipe_info,
    recipe_output,
    recipe_value,
    recipe_consume,
    item_block,
    ui_open: crate::client::modui::ui_open,
    ui_close: crate::client::modui::ui_close,
    ui_text: crate::client::modui::ui_text,
    client_player: crate::client::modui::client_player,
    client_attribute: crate::client::modui::client_attribute,
    client_swing_strength: crate::client::modui::client_swing_strength,
    client_use_ticks_left: crate::client::modui::client_using,
    client_day_time: crate::client::modui::client_day_time,
    client_weather: crate::client::modui::client_weather,
    client_permission: crate::client::modui::client_permission,
    client_component: crate::client::modui::client_component,
    client_entity: crate::client::modui::client_entity,
    client_game_mode: crate::client::modui::client_game_mode,
    energy_move,
    component_get,
    component_set,
    command_reply,
    show,
    client_show: crate::app::api::client_show,
    reg_chat_format,
    chat_format_set,
    gizmo_shown,
    gizmo,
    gizmo_text,
    reg_client_hook,
    reg_client_entity_cosmetics,
    reg_client_entity_frame,
    reg_bone_model,
    reg_bone_motion,
    reg_item_model_for_stack,
    client_pose_bone: crate::app::api::client_pose_bone,
    client_bone_position: crate::app::api::client_bone_position,
    client_action_current: crate::app::api::client_entity_action,
    client_clip_length: crate::app::api::client_clip_length,
    client_held_item: crate::app::api::client_entity_held,
    client_engine_effects: crate::app::api::client_entity_defaults,
    reg_packet,
    reg_key,
    reg_mistake,
    launch_option: crate::app::api::launch_option,
    send_packet,
    entity_teleport,
    set_day_time,
    day_time,
    skip_ticks,
    fill,
    shape_contains,
    shape_bounds,
    set_game_mode,
    client_state: crate::app::api::client_state,
    client_input: crate::app::api::client_input,
    client_key_state: crate::app::api::client_pressed,
    client_control: crate::app::api::client_set_player,
    client_op: crate::app::api::client_command,
    client_text: crate::app::api::client_text,
    client_command_reply: crate::app::api::client_command_reply,
    client_block_in: crate::app::api::client_block_in,
    client_cube_shown: crate::app::api::client_cube_shown,
    client_light: crate::app::api::client_light,
    client_slot: crate::app::api::client_slot,
    client_menu: crate::app::api::client_menu,
    client_entities: crate::app::api::client_entities,
    client_ui_rect: crate::app::api::client_ui_rect,
    client_draw: crate::app::api::client_draw,
    client_draw_text: crate::app::api::client_draw_text,
    client_screen_text: crate::app::api::client_screen_text,
    client_send_packet: crate::app::api::client_send_packet,
    image_pixels: crate::app::api::image_get,
    image_load: crate::app::api::image_load,
    image_save: crate::app::api::image_save,
    image_unload: crate::app::api::image_free,
    name_of,
    reg_liquid,
    reg_liquid_edit,
    reg_liquid_reaction,
    liquid_of,
    use_container,
    reg_event_handler,
    find_event,
    emit,
    reg_interceptor,
    ask,
    reg_chain_modifier,
    chain_compute,
    explanation,
    reg_attribute,
    attribute,
    attribute_add_modifier,
    attribute_remove_modifier,
    attribute_explain,
    reg_loot,
    reg_loot_edit,
    reg_override,
    roll_loot,
    break_block,
    reg_stack_component,
    reg_storage,
    client_menu_owner: crate::client::modui::client_menu_owner,
    reg_system,
    set_weather,
    weather_now,
    reg_weather,
    reg_precipitation,
    weather_flags,
    entity_effect,
    reg_entity_effect,
    reg_text_place,
    reg_game_mode,
    reg_pose,
    entity_set_pose,
    game_mode_flags,
    reg_command_permission,
    player_permission,
    set_player_permission,
    reg_command_branch,
    reg_condition,
    reg_particle,
    reg_sky,
    particle_burst,
    signal_at,
    client_particle_burst: crate::app::api::client_particle_burst,
    reg_config,
    config_value,
    config_set,
    config_text,
    config_set_text,
    reg_sound,
    reg_music,
    action_play,
    action_override,
    play_sound,
    client_play_sound: crate::app::api::client_play_sound,
    reg_spawn,
    biome_at,
    navigate,
    navigating,
    reg_structure,
    reg_template,
    block_list,
    place_template,
    world_seed,
    structure_near,
    fall_block,
    reg_attach,
    reg_recipe_kind,
    reg_entity_category,
    reg_damage_cause,
    damage_cause_flags,
    reg_sound_category,
    reg_tint_kind,
    reg_biome_tint,
    reg_player_inventory,
    reg_equipment_slot,
    player_inventory_group,
    reg_words,
    set_entity_size,
    set_entity_appearance,
    entity_kind_size,
    body_from_blocks,
    body_to_blocks,
    body_fall,
    push_blocks,
    world_enter_body,
    world_leave_body,
    body_move_to,
    signal_read_changed,
    reg_terrain,
    reg_screen_part,
    dim_column,
    dim_surface_carved,
    dim_surface_block,
    reg_camera,
    camera_id,
    ease,
    force_load,
    asset_exists,
    player_camera,
};

unsafe extern "C" fn fall_block(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u64 {
    unsafe { dim(w).fall_block(content(), ip(pos)) }
}

unsafe extern "C" fn reg_template(r: *mut abi::Registrar, d: *const abi::TemplateDesc) {
    unsafe {
        let d = &*d;
        reg(r).game.templates.push(registry::RawTemplate {
            name: d.name.as_str().to_string(),
            blocks: d
                .blocks
                .as_slice()
                .iter()
                .map(|b| (b.pos.to_array(), b.block.as_str().to_string()))
                .collect(),
        });
    }
}

unsafe extern "C" fn block_list(index: u32) -> abi::Slice<[u32; 2]> {
    try_content()
        .and_then(|c| c.ref_lists.get(index as usize))
        .map_or(abi::Slice::EMPTY, |l| abi::Slice::new(l))
}

unsafe extern "C" fn place_template(w: *mut abi::WorldCtx, name: Str, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        let c = content();
        let Some(&t) = c.template_names.get(name.as_str()) else {
            misuse("place", || format!("no template {}", name.as_str()));
            return 0;
        };
        let d = dim(w);
        let at = ip(pos);
        let blocks = &c.templates[t as usize].blocks;
        if blocks.iter().any(|(p, _)| d.get(at + *p).is_none()) {
            return 0;
        }
        for &(p, state) in blocks {
            d.set(c, at + p, state);
        }
        1
    }
}

unsafe extern "C" fn reg_structure(r: *mut abi::Registrar, d: *const abi::StructureDesc) {
    unsafe {
        let d = *d;
        reg(r).game.structures.push(registry::RawStructure {
            template: d.template.as_str().to_string(),
            dimension: d.dimension.as_str().to_string(),
            spacing: d.spacing,
            chance: d.chance,
            biome: d.biome.as_str().to_string(),
            sink: d.buried,
        });
    }
}

unsafe extern "C" fn world_seed(w: *mut abi::WorldCtx) -> u64 {
    unsafe { dim(w).seed }
}

unsafe extern "C" fn structure_near(
    w: *mut abi::WorldCtx,
    name: Str,
    pos: *const abi::BlockPos,
    radius: i32,
    out: *mut abi::BlockPos,
) -> u32 {
    unsafe {
        let d = dim(w);
        match crate::server::structure_near(content(), d.id, d.seed, name.as_str(), ip(pos), radius) {
            Some(p) => {
                *out = p.to_array().into();
                1
            }
            None => 0,
        }
    }
}

unsafe extern "C" fn navigate(w: *mut abi::WorldCtx, id: u64, target: *const abi::Vec3, speed: f32) -> u32 {
    unsafe {
        let c = content();
        let d = dim(w);
        let target = d.place.to_world(crate::world::dvec(*target));
        let mul = d.attribute(c, id, crate::server::MOVEMENT_SPEED, false) as f32;
        let jump_mul = d.attribute(c, id, crate::server::JUMP_STRENGTH, false) as f32;
        if let Some(e) = d.entities.get_mut(id) {
            e.jump_mul = jump_mul;
        }
        d.navigate(id, target, speed * mul) as u32
    }
}

unsafe extern "C" fn navigating(w: *mut abi::WorldCtx, id: u64) -> u32 {
    unsafe { dim(w).entities.get(id).is_some_and(|e| e.nav.is_some()) as u32 }
}

unsafe extern "C" fn biome_at(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        let p = ip(pos);
        dim(w)
            .space
            .cubes
            .get(&crate::world::cube_of(p))
            .map_or(0, |c| c.biome((p.x & 15) as usize, (p.z & 15) as usize) as u32)
    }
}

unsafe extern "C" fn reg_spawn(r: *mut abi::Registrar, d: *const abi::SpawnDesc) {
    unsafe {
        let d = *d;
        let tag = |s: Str| s.as_str().to_string();
        reg(r).game.spawns.push(registry::RawSpawn {
            entity: d.entity.as_str().to_string(),
            biome: tag(d.biome),
            ground: tag(d.ground),
            place: d.place.min(abi::spawn_place::CEILING),
            condition: d.condition.as_str().to_string(),
            dimension: d.dimension.as_str().to_string(),
            light: d.light,
            y: d.y,
            group: [d.group[0].max(1), d.group[1].max(d.group[0]).max(1)],
            weight: d.weight,
            chance: d.chance,
        });
    }
}

unsafe extern "C" fn reg_sound(r: *mut abi::Registrar, d: *const abi::SoundDesc) {
    unsafe {
        let d = *d;
        let def = registry::SoundDef {
            name: d.name.as_str().to_string(),
            files: d
                .files
                .as_slice()
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            category: 0,
            category_name: d.category_name.as_str().to_string(),
            volume: d.volume,
            pitch: [d.pitch[0].min(d.pitch[1]).max(0.01), d.pitch[0].max(d.pitch[1]).max(0.01)],
            range: d.range.max(1.0),
            full_within: d.full_within.max(0.0),
            falloff: d.falloff.clamp(-8, 8),
        };
        reg(r).game.sounds.push((d.cell as usize, def));
    }
}

unsafe extern "C" fn reg_music(r: *mut abi::Registrar, d: *const abi::MusicDesc) {
    unsafe {
        let d = *d;
        let m = registry::MusicDef {
            name: d.name.as_str().to_string(),
            tracks: d
                .tracks
                .as_slice()
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            dimension: None,
            when: Vec::new(),
            when_names: d
                .when
                .as_str()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        };
        reg(r)
            .game
            .music
            .push((d.dimension.as_str().to_string(), m));
    }
}

unsafe extern "C" fn action_override(w: *mut abi::WorldCtx, id: u64, from: Str, to: Str) {
    unsafe {
        let d = dim(w);
        if d.entities.get(id).is_none() {
            return;
        }
        let comp = content().game.field_comps[7];
        let (from, to) = (from.as_str(), to.as_str());
        let now = d
            .entities
            .comps
            .get(&(id, comp))
            .map_or(&[][..], |b| &b[..]);
        let text = String::from_utf8_lossy(now);
        let mut pairs = text.lines().filter_map(|l| l.split_once('='));
        // A goal says it every tick: the same as before costs one look.
        if pairs.clone().any(|p| p == (from, to)) || (to.is_empty() && !pairs.any(|p| p.0 == from)) {
            return;
        }
        let mut next = String::new();
        for (a, b) in text
            .lines()
            .filter_map(|l| l.split_once('='))
            .filter(|p| p.0 != from)
        {
            next.push_str(&format!("{a}={b}\n"));
        }
        if !to.is_empty() {
            next.push_str(&format!("{from}={to}\n"));
        }
        d.entities
            .comps
            .insert((id, comp), next.into_bytes().into_boxed_slice());
    }
}

unsafe extern "C" fn action_play(w: *mut abi::WorldCtx, id: u64, name: Str, looping: u32) {
    unsafe {
        let d = dim(w);
        // What loops goes on for whoever comes along later too, as anything else they should see.
        let comp = content().game.field_comps[8];
        let now = if looping != 0 { name.as_str().as_bytes() } else { &[] };
        if d.entities.get(id).is_some()
            && d.entities
                .comps
                .get(&(id, comp))
                .map_or(&[][..], |b| &b[..])
                != now
        {
            d.entities
                .comps
                .insert((id, comp), now.to_vec().into_boxed_slice());
        }
        d.events
            .push(crate::server::DimEvent::PlayAction { id, name: name.as_str().to_string(), looping: looping != 0 });
    }
}

unsafe extern "C" fn play_sound(w: *mut abi::WorldCtx, sound: u32, pos: *const abi::Vec3, volume: f32, pitch: f32) {
    unsafe {
        if (sound as usize) < content().game.sounds.len() {
            dim(w).sound(sound, crate::world::dvec(*pos), volume, pitch);
        } else {
            misuse("play_sound", || format!("no sound {sound}"));
        }
    }
}

// ---------------------------------------------------------------- profiling per mod

pub const MAX_MODS: usize = 256;

pub struct ModStats {
    pub nanos: AtomicU64,
    pub calls: AtomicU64,
    pub faults: AtomicU32,
}

static STATS: [ModStats; MAX_MODS] =
    [const { ModStats { nanos: AtomicU64::new(0), calls: AtomicU64::new(0), faults: AtomicU32::new(0) } }; MAX_MODS];

pub fn stats(m: ModId) -> &'static ModStats {
    &STATS[(m.0 as usize).min(MAX_MODS - 1)]
}

/// Callback nanoseconds and calls of a mod since start; readers diff two looks.
pub fn mod_totals(m: ModId) -> (u64, u64) {
    let s = stats(m);
    (s.nanos.load(Ordering::Relaxed), s.calls.load(Ordering::Relaxed))
}

/// Calls a mod callback. Time is charged to `owner`; a failing callback is logged with the mod's
/// name and the engine carries on.
pub fn invoke<A>(owner: ModId, cb: Callback, args: &mut A) -> bool {
    let Some(f) = cb.func else { return true };
    let start = Instant::now();
    let prev = crate::diag::enter_mod(owner);
    let r = unsafe { f(cb.data, args as *mut A as *mut c_void) };
    crate::diag::leave_mod(prev);
    let s = stats(owner);
    s.nanos
        .fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
    s.calls.fetch_add(1, Ordering::Relaxed);
    if r != 0 {
        let n = s.faults.fetch_add(1, Ordering::Relaxed);
        if n < 10 {
            let name = try_content().map_or("?".to_string(), |c| c.mod_name(owner).to_string());
            log::error!("callback of mod '{name}' failed (see its panic above), continuing");
        }
        return false;
    }
    true
}

// ---------------------------------------------------------------- world access from callbacks

/// `WorldCtx` pointers handed to mods point at one of these. Only valid during the callback.
pub struct CallCtx {
    pub dim: *mut Dim,
}

impl CallCtx {
    pub fn raw(&mut self) -> *mut abi::WorldCtx {
        self as *mut CallCtx as *mut abi::WorldCtx
    }
}

unsafe fn dim<'a>(w: *mut abi::WorldCtx) -> &'a mut Dim {
    unsafe { &mut *(*(w as *mut CallCtx)).dim }
}

fn ip(p: *const abi::BlockPos) -> IVec3 {
    crate::world::ivec(unsafe { *p })
}

unsafe extern "C" fn block_get(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe { dim(w).seen(ip(pos)).unwrap_or(u32::MAX) }
}

unsafe extern "C" fn part_list(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, out: *mut u32, cap: u32) -> u32 {
    unsafe {
        let parts = dim(w).parts(ip(pos));
        std::ptr::copy_nonoverlapping(parts.as_ptr(), out, parts.len().min(cap as usize));
        parts.len() as u32
    }
}

unsafe extern "C" fn part_add(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, state: u32) -> u32 {
    if state as usize >= content().states.len() {
        misuse("part_add", || format!("no state {state}"));
        return 0;
    }
    unsafe { dim(w).add_part(content(), ip(pos), state) as u32 }
}

unsafe extern "C" fn part_replace(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, old: u32, new: u32) -> u32 {
    if new as usize >= content().states.len() {
        misuse("part_replace", || format!("no state {new}"));
        return 0;
    }
    unsafe { dim(w).replace_part(content(), ip(pos), old, new) as u32 }
}

unsafe extern "C" fn world_dimension(w: *mut abi::WorldCtx) -> u32 {
    unsafe { dim(w).id }
}

/// A mod called a host function with something that does not exist: logged, the first 20 times
/// per function (a mod doing it every tick would flood the log).
pub(crate) fn misuse(func: &'static str, detail: impl FnOnce() -> String) {
    static SEEN: std::sync::Mutex<Vec<(&'static str, u32)>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let n = match seen.iter_mut().find(|s| s.0 == func) {
        Some(s) => {
            s.1 += 1;
            s.1
        }
        None => {
            seen.push((func, 1));
            1
        }
    };
    if n <= 20 {
        log::error!("{func}: {}{}", detail(), if n == 20 { " (not reported again)" } else { "" });
    }
}

unsafe extern "C" fn block_set(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, state: u32) -> u32 {
    if state as usize >= content().states.len() {
        log::error!("mod tried to set invalid state {state}");
        return 0;
    }
    unsafe { dim(w).set_seen(content(), ip(pos), state).is_some() as u32 }
}

unsafe extern "C" fn body_from_blocks(
    w: *mut abi::WorldCtx,
    center: *const abi::BlockPos,
    cells: *const abi::BlockPos,
    count: usize,
) -> u64 {
    unsafe {
        let cells: Vec<IVec3> = std::slice::from_raw_parts(cells, count)
            .iter()
            .map(|p| crate::world::ivec(*p))
            .collect();
        dim(w).lift_for_mod(content(), ip(center), &cells)
    }
}

unsafe extern "C" fn body_to_blocks(w: *mut abi::WorldCtx, entity: u64) {
    unsafe { dim(w).ask_put_down(entity) }
}

unsafe extern "C" fn world_enter_body(w: *mut abi::WorldCtx, entity: u64) -> u32 {
    unsafe { dim(w).enter_frame(entity) as u32 }
}

unsafe extern "C" fn world_leave_body(w: *mut abi::WorldCtx) {
    unsafe { dim(w).leave_frame() }
}

unsafe extern "C" fn push_blocks(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, dir: u32) -> u32 {
    unsafe { dim(w).push_blocks(content(), ip(pos), dir.min(6) as u8) as u32 }
}

unsafe extern "C" fn body_fall(w: *mut abi::WorldCtx, entity: u64) {
    unsafe { dim(w).body_fall(entity) }
}

unsafe extern "C" fn body_move_to(w: *mut abi::WorldCtx, entity: u64, pos: *const abi::Vec3, ticks: u32) {
    unsafe {
        let d = dim(w);
        let to = d.place.to_world(crate::world::dvec(*pos));
        if let Some(e) = d
            .entities
            .get_mut(entity)
            .filter(|e| e.motion != crate::server::entity::Motion::Parked)
        {
            e.motion = crate::server::entity::Motion::To { to, left: ticks.clamp(1, u16::MAX as u32) as u16 };
        }
    }
}

unsafe extern "C" fn signal_read_changed(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) {
    unsafe { dim(w).space.signals.read_changed(content(), ip(pos)) }
}

unsafe extern "C" fn schedule_tick(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, delay: u32) {
    unsafe { dim(w).schedule(ip(pos), delay) }
}

unsafe extern "C" fn game_time(w: *mut abi::WorldCtx) -> u64 {
    unsafe { dim(w).time }
}

unsafe extern "C" fn random(w: *mut abi::WorldCtx) -> u64 {
    unsafe { dim(w).random() }
}

unsafe extern "C" fn light(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe { dim(w).light(ip(pos)) }
}

unsafe extern "C" fn daylight(w: *mut abi::WorldCtx) -> f32 {
    unsafe { dim(w).daylight(content()) }
}

unsafe extern "C" fn entity_spawn(w: *mut abi::WorldCtx, desc: *const abi::SpawnEntityDesc) -> u64 {
    let c = content();
    let d = unsafe { &*desc };
    let kind = d.kind;
    if kind as usize >= c.game.entities.len() {
        misuse("entity_spawn", || format!("no entity kind {kind}"));
        return 0;
    }
    if c.game.is_block_entity(kind) {
        misuse("entity_spawn", || format!("{} belongs to a block, set the block instead", c.game.entities[kind as usize].name));
        return 0;
    }
    let is_item = kind == c.game.item_kind;
    if !is_item && !d.stack.is_empty() {
        misuse("entity_spawn", || format!("{} carries no stack", c.game.entities[kind as usize].name));
    }
    if is_item && !known_item("entity_spawn", &d.stack) {
        return 0;
    }
    let dim = unsafe { dim(w) };
    let pos = crate::world::dvec(d.pos);
    // Items are their stack: nothing to spawn for an empty one.
    let id = if is_item { dim.spawn_item(c, pos, d.stack) } else { dim.spawn(c, kind, pos) };
    if let Some(e) = dim.entities.get_mut(id) {
        use abi::spawn_set as s;
        if d.set & s::VELOCITY != 0 && crate::world::dvec(d.velocity).is_finite() {
            e.vel = crate::world::dvec(d.velocity);
        }
        if d.set & s::YAW != 0 && d.yaw.is_finite() {
            e.yaw = d.yaw;
        }
        if d.set & s::PICKUP_DELAY != 0 {
            e.pickup_delay = d.pickup_delay;
        }
    }
    id
}

unsafe extern "C" fn entity_despawn(w: *mut abi::WorldCtx, id: u64) {
    unsafe {
        let d = dim(w);
        if d.entities.get(id).is_some_and(|e| e.player) {
            misuse("entity_remove", || "a player can't be removed; kill it or teleport it".to_string());
            return;
        }
        d.entities.remove(id)
    }
}

unsafe extern "C" fn entity_mount(w: *mut abi::WorldCtx, rider: u64, mount: u64, seat: u32) -> u32 {
    unsafe { dim(w).mount(content(), rider, mount, (seat != u32::MAX).then_some(seat.min(255) as u8)) as u32 }
}

unsafe extern "C" fn entity_dismount(w: *mut abi::WorldCtx, rider: u64) {
    unsafe {
        dim(w).dismount(content(), rider);
    }
}

unsafe extern "C" fn entity_vehicle(w: *mut abi::WorldCtx, rider: u64, seat: *mut u32) -> u64 {
    unsafe {
        let Some((mount, s)) = dim(w).entities.get(rider).and_then(|e| e.vehicle) else { return 0 };
        if !seat.is_null() {
            *seat = s as u32;
        }
        mount
    }
}

unsafe extern "C" fn entity_riders(w: *mut abi::WorldCtx, mount: u64, out: *mut u64, cap: u32) -> u32 {
    unsafe {
        let d = dim(w);
        let Some(m) = d.entities.get(mount) else { return 0 };
        let seats = content().game.entities[m.kind as usize].seats.len() as u32;
        for s in 0..seats.min(cap) {
            *out.add(s as usize) = d.rider_in(mount, s as u8).unwrap_or(0);
        }
        seats
    }
}

unsafe extern "C" fn entity_get(w: *mut abi::WorldCtx, id: u64, out: *mut abi::EntityState) -> u32 {
    unsafe {
        let d = dim(w);
        let c = content();
        match d.entities.get(id) {
            Some(e) => {
                *out = e.state(c, d.max_health(c, id));
                (*out).pos = (d.place.to_local(e.pos).to_array()).into();
                1
            }
            None => 0,
        }
    }
}

unsafe extern "C" fn entity_set(w: *mut abi::WorldCtx, id: u64, set: u32, s: *const abi::EntityState) {
    use crate::server::entity::Told;
    use abi::entity_set as es;
    unsafe {
        let s = &*s;
        let d = dim(w);
        let c = content();
        let max_health = d.max_health(c, id);
        let Some(e) = d.entities.get_mut(id) else { return };
        let player = e.player;
        let mut told = Vec::new();
        let pos = crate::world::dvec(s.pos);
        let teleport = set & es::POS != 0 && pos.is_finite();
        if teleport && !player {
            e.pos = d.place.to_world(pos);
        }
        let vel = crate::world::dvec(s.vel);
        if set & es::VELOCITY != 0 && vel.is_finite() {
            e.vel = vel;
            told.push(Told::Velocity((vel / crate::world::TPS as f64).as_vec3().to_array()));
        }
        if set & es::YAW != 0 && s.yaw.is_finite() {
            e.yaw = s.yaw;
        }
        if set & es::PITCH != 0 && s.pitch.is_finite() {
            e.pitch = s.pitch;
        }
        if set & (es::YAW | es::PITCH) != 0 {
            told.push(Told::Look([e.yaw, e.pitch]));
        }
        if set & es::FALL_DISTANCE != 0 && s.fall_distance.is_finite() {
            e.fall = s.fall_distance.max(0.0);
        }
        if set & es::FLYING != 0 {
            e.flying = s.flying != 0;
            told.push(Told::Flying(e.flying));
        }
        if set & es::FIRE != 0 {
            e.fire = s.fire;
        }
        if set & es::SELECTED != 0 && player {
            e.selected = s
                .selected
                .min(c.game.player_layout.hotbar.saturating_sub(1));
            told.push(Told::Selected(e.selected));
        }
        // Only a respawn brings back the dead, and only damage kills: everything that follows
        // a death goes one way.
        let dies = set & es::HEALTH != 0 && e.alive() && {
            let health = s.health.min(max_health);
            if health > 0.0 {
                e.health = health;
            }
            !(health > 0.0)
        };
        if player {
            d.entities.told.extend(told.into_iter().map(|t| (id, t)));
        }
        if teleport && player {
            d.events
                .push(crate::server::DimEvent::Teleport { from: d.id, id, dim: d.id, pos: d.place.to_world(pos) });
        }
        if dies {
            d.damage(c, id, f32::MAX, 0, weft_abi::damage_cause::VOID);
        }
    }
}

unsafe extern "C" fn entity_push(w: *mut abi::WorldCtx, id: u64, v: *const abi::Vec3) {
    unsafe {
        let v = crate::world::dvec(*v);
        if !v.is_finite() {
            return;
        }
        dim(w).push(id, v);
    }
}

unsafe extern "C" fn entity_walk(w: *mut abi::WorldCtx, id: u64, dir: *const [f32; 2], speed: f32, jump: u32) {
    unsafe {
        let d = dim(w);
        let mul = d.attribute(content(), id, crate::server::MOVEMENT_SPEED, false) as f32;
        let jump_mul = d.attribute(content(), id, crate::server::JUMP_STRENGTH, false) as f32;
        if let Some(e) = d.entities.get_mut(id) {
            e.walk = Some((*dir, speed * mul, jump != 0));
            e.jump_mul = jump_mul;
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn raycast(
    w: *mut abi::WorldCtx,
    from: *const abi::Vec3,
    dir: *const abi::Vec3,
    max: f64,
    liquids: u32,
    pos: *mut abi::BlockPos,
    face: *mut u32,
    body: *mut u64,
) -> u32 {
    unsafe {
        let dir = crate::world::dvec(*dir).normalize_or_zero();
        if dir == DVec3::ZERO || !max.is_finite() {
            return 0;
        }
        let Some(hit) = dim(w).ray(content(), crate::world::dvec(*from), dir, max.min(1024.0), liquids != 0) else { return 0 };
        *pos = hit.block.to_array().into();
        *face = hit.face as u32;
        *body = hit.frame;
        hit.state + 1
    }
}

unsafe extern "C" fn entity_damage(w: *mut abi::WorldCtx, id: u64, amount: f32, source: u64, cause: u32) -> u32 {
    unsafe { dim(w).damage(content(), id, amount, source, cause) as u32 }
}

unsafe extern "C" fn entity_in_reach(w: *mut abi::WorldCtx, id: u64, target: u64) -> u32 {
    unsafe {
        let d = dim(w);
        match (d.entities.get(id), d.entities.get(target)) {
            (Some(me), Some(t)) => crate::server::actions::in_reach(content(), d, me, t) as u32,
            _ => 0,
        }
    }
}

unsafe extern "C" fn entity_attack(w: *mut abi::WorldCtx, attacker: u64, target: u64) -> u32 {
    unsafe { dim(w).melee(content(), attacker, target) as u32 }
}

unsafe extern "C" fn in_liquid(w: *mut abi::WorldCtx, pos: *const abi::Vec3) -> u32 {
    unsafe {
        let d = dim(w);
        crate::physics::liquid_at(content(), &*d.space, crate::world::dvec(*pos)).is_some() as u32
    }
}

unsafe extern "C" fn entity_swing_strength(w: *mut abi::WorldCtx, id: u64) -> f32 {
    unsafe {
        let d = dim(w);
        d.swing_strength(content(), id, d.entities.held(id))
    }
}

unsafe extern "C" fn entities_in(
    w: *mut abi::WorldCtx,
    shape: abi::Slice<abi::ShapeStep>,
    kind: u32,
    flags: u32,
    out: *mut u64,
    max: usize,
) -> usize {
    unsafe {
        let Some(shape) = crate::shape::Shape::from_abi(shape.as_slice()) else {
            misuse("entities_in", || "the shape's steps don't make one shape".to_string());
            return 0;
        };
        let d = dim(w);
        let ids = d.entities.within(content(), &d.place, &shape, kind, flags);
        for (i, id) in ids.iter().take(max).enumerate() {
            *out.add(i) = *id;
        }
        ids.len()
    }
}

unsafe extern "C" fn component(w: *mut abi::WorldCtx, id: u64, comp: u32) -> *mut u8 {
    unsafe {
        let d = dim(w);
        // Writes are found by comparing at sync time (`Entities::find_changes`).
        d.entities
            .comps
            .get_mut(&(id, comp))
            .map_or(std::ptr::null_mut(), |b| b.as_mut_ptr())
    }
}

unsafe extern "C" fn player_count(w: *mut abi::WorldCtx) -> u32 {
    unsafe { dim(w).visible_players().len() as u32 }
}

unsafe extern "C" fn player_at(w: *mut abi::WorldCtx, index: u32) -> u64 {
    unsafe {
        dim(w)
            .visible_players()
            .get(index as usize)
            .copied()
            .unwrap_or(0)
    }
}

/// What `holder` points at here, `None` (and nothing done) when it isn't.
unsafe fn held(w: *mut abi::WorldCtx, h: *const abi::Holder) -> Option<crate::server::holder::At> {
    unsafe { crate::server::holder::resolve(dim(w), &*h) }
}

/// The inventory `holder` points at here, `None` (and nothing done) when there is none.
unsafe fn inventory(w: *mut abi::WorldCtx, h: *const abi::Holder) -> Option<crate::server::holder::Inv> {
    unsafe { crate::server::holder::inventory(dim(w), content(), &*h) }
}

unsafe extern "C" fn inventory_len(w: *mut abi::WorldCtx, h: *const abi::Holder) -> u32 {
    unsafe { inventory(w, h).map_or(0, |inv| crate::server::holder::len(dim(w), content(), inv)) }
}

unsafe extern "C" fn inventory_get(w: *mut abi::WorldCtx, h: *const abi::Holder, slot: u32) -> ItemStack {
    unsafe { inventory(w, h).map_or(ItemStack::EMPTY, |inv| crate::server::holder::get(dim(w), content(), inv, slot)) }
}

/// A stack from a mod: anything it says goes into saves and on screen, where a wrong id would
/// bring down the game instead of naming the mod.
pub(crate) fn known_item(what: &'static str, stack: &ItemStack) -> bool {
    let known = stack.is_empty() || (stack.item as usize) < content().items.len();
    if !known {
        misuse(what, || format!("no item with id {}", stack.item));
    }
    known
}

unsafe extern "C" fn inventory_set(w: *mut abi::WorldCtx, h: *const abi::Holder, slot: u32, stack: ItemStack) {
    if known_item("inventory_set", &stack) {
        unsafe {
            if let Some(inv) = inventory(w, h) {
                crate::server::holder::set(dim(w), content(), inv, slot, stack)
            }
        }
    }
}

unsafe extern "C" fn inventory_insert(w: *mut abi::WorldCtx, h: *const abi::Holder, stack: ItemStack) -> ItemStack {
    if !known_item("inventory_insert", &stack) {
        return ItemStack::EMPTY;
    }
    unsafe {
        match inventory(w, h) {
            Some(inv) => crate::server::holder::insert(dim(w), content(), inv, stack),
            None => stack,
        }
    }
}

unsafe extern "C" fn selected_slot(w: *mut abi::WorldCtx, player: u64) -> u32 {
    unsafe { dim(w).entities.get(player).map_or(0, |e| e.selected) }
}

unsafe extern "C" fn block_entity(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u64 {
    unsafe { dim(w).anchored(ip(pos)).unwrap_or(0) }
}

unsafe extern "C" fn open_menu(w: *mut abi::WorldCtx, player: u64, menu: u32, h: *const abi::Holder) -> u32 {
    use crate::server::holder::{self, At};
    unsafe {
        let d = dim(w);
        let c = content();
        let Some(def) = c.game.menus.get(menu as usize) else {
            log::error!("open_menu: there is no menu {menu}");
            return 0;
        };
        let h = &*h;
        let on = match holder::inventory(d, c, h) {
            _ if h.kind == abi::holder::NONE => None,
            Some(inv) if matches!(inv.at, At::Entity(_)) || holder::len(d, c, inv) > 0 => Some(inv),
            found => {
                let what = match h.kind {
                    _ if h.storage != 0 && holder::resolve(d, h).is_some() => {
                        let name = c
                            .game
                            .components
                            .get(h.storage as usize)
                            .map_or("?", |d| d.name.as_str());
                        format!("storage {name}, which isn't one or isn't carried there")
                    }
                    abi::holder::BLOCK => {
                        let p = crate::world::ivec(h.pos);
                        d.get(p)
                            .map_or("an unloaded block".to_string(), |s| format!("{} at {p}, which has no block entity", c.state_name(s)))
                    }
                    abi::holder::HELD if found.is_some() => {
                        format!("the stack in slot {} of entity {}, which holds no items", h.slot, h.entity)
                    }
                    abi::holder::HELD => format!("slot {} of entity {}, which isn't here", h.slot, h.entity),
                    _ => format!("entity {} (removed, or in another region)", h.entity),
                };
                log::error!("cannot open menu {} on {what}", def.name);
                return 0;
            }
        };
        d.events
            .push(crate::server::DimEvent::OpenMenu { player, menu, on, block: 0 });
        1
    }
}

unsafe extern "C" fn day_time(w: *mut abi::WorldCtx) -> u64 {
    unsafe {
        let d = dim(w);
        d.time.saturating_add_signed(d.day_offset)
    }
}

unsafe extern "C" fn skip_ticks(w: *mut abi::WorldCtx, ticks: u64) {
    unsafe {
        let d = dim(w);
        d.events
            .push(crate::server::DimEvent::SkipTicks { dim: d.id, ticks })
    }
}

unsafe extern "C" fn entity_teleport(w: *mut abi::WorldCtx, id: u64, to: u32, pos: *const abi::Vec3) {
    unsafe {
        let d = dim(w);
        d.events
            .push(crate::server::DimEvent::Teleport { from: d.id, id, dim: to, pos: d.place.to_world(crate::world::dvec(*pos)) })
    }
}

unsafe extern "C" fn set_day_time(w: *mut abi::WorldCtx, time: u64) {
    unsafe {
        let d = dim(w);
        d.events
            .push(crate::server::DimEvent::SetTime { dim: d.id, time })
    }
}

unsafe extern "C" fn fill(w: *mut abi::WorldCtx, desc: *const abi::FillDesc) {
    use crate::server::area::Filter;
    unsafe {
        let d = dim(w);
        let a = &*desc;
        let c = content();
        if d.frame != 0 {
            misuse("fill", || "fills are for the world, not a body's blocks".to_string());
            return;
        }
        let Some(shape) = crate::shape::Shape::from_abi(a.shape.as_slice()) else {
            misuse("fill", || "the shape's steps don't make one shape".to_string());
            return;
        };
        if a.state as usize >= c.states.len() {
            misuse("fill", || format!("no block state {}", a.state));
            return;
        }
        let filters = |l: abi::Slice<abi::BlockFilter>| -> Option<Vec<Filter>> {
            l.as_slice()
                .iter()
                .map(|f| Filter::from_abi(c, f))
                .collect()
        };
        let (Some(keep), Some(only)) = (filters(a.keep), filters(a.only)) else {
            misuse("fill", || "a filter names no tag or block".to_string());
            return;
        };
        let fill = crate::server::area::AreaFill { id: 0, shape, state: a.state, keep, only };
        d.events
            .push(crate::server::DimEvent::AreaFill { dim: d.id, fill });
    }
}

unsafe extern "C" fn shape_contains(shape: abi::Slice<abi::ShapeStep>, p: *const abi::Vec3) -> u32 {
    unsafe { crate::shape::Shape::from_abi(shape.as_slice()).is_some_and(|s| s.contains(crate::world::dvec(*p))) as u32 }
}

unsafe extern "C" fn shape_bounds(shape: abi::Slice<abi::ShapeStep>, min: *mut abi::Vec3, max: *mut abi::Vec3) -> u32 {
    unsafe {
        let Some(s) = crate::shape::Shape::from_abi(shape.as_slice()) else { return 0 };
        let (lo, hi) = s.bounds();
        (*min, *max) = (lo.to_array().into(), hi.to_array().into());
        1
    }
}

unsafe extern "C" fn reg_camera(r: *mut abi::Registrar, d: *const abi::CameraDesc) {
    unsafe {
        let r = reg(r);
        let d = &*d;
        r.game.cameras.push(registry::RawCamera {
            owner: r.owner,
            name: d.name.as_str().to_string(),
            flags: d.flags,
            flags_off: d.flags_off,
            key: d.key.as_str().to_string(),
            call: d.call,
            transition: d.transition,
            easing: d.easing,
        });
    }
}

unsafe extern "C" fn force_load(
    w: *mut abi::WorldCtx,
    key: u64,
    from: *const abi::BlockPos,
    to: *const abi::BlockPos,
    mode: u32,
    ticks: u32,
) -> u32 {
    unsafe {
        let d = dim(w);
        let (a, b) = (crate::world::ivec(*from), crate::world::ivec(*to));
        if mode != abi::force_load::OFF {
            let mut q = abi::Query { other: key, pos: *from, amount: mode as f64, ..abi::Query::DEFAULT };
            if !d.ask(content(), abi::action::FORCE_LOAD, &mut q) {
                return 0;
            }
        }
        let (a, b) = (d.place.to_world(a.as_dvec3()).floor().as_ivec3(), d.place.to_world(b.as_dvec3()).floor().as_ivec3());
        d.events.push(crate::server::DimEvent::ForceLoad {
            dim: d.id,
            key,
            min: crate::world::cube_of(a.min(b)),
            max: crate::world::cube_of(a.max(b)),
            mode,
            ticks,
        });
        1
    }
}

unsafe extern "C" fn asset_exists(name: Str, kind: Str, ext: Str) -> u32 {
    unsafe { crate::modloader::asset_exists(name.as_str(), kind.as_str(), ext.as_str()) as u32 }
}

unsafe extern "C" fn ease(kind: u32, t: f32) -> f32 {
    crate::client::camera::ease(kind, t)
}

unsafe extern "C" fn camera_id(name: Str) -> u32 {
    unsafe {
        content()
            .game
            .camera_names
            .get(name.as_str())
            .copied()
            .unwrap_or(u32::MAX)
    }
}

unsafe extern "C" fn player_camera(w: *mut abi::WorldCtx, player: u64, mode: u32, locked: u32, transition: f32, easing: u32) {
    unsafe {
        dim(w)
            .events
            .push(crate::server::DimEvent::Camera { player, mode, locked: locked != 0, transition, easing })
    }
}

unsafe extern "C" fn set_game_mode(w: *mut abi::WorldCtx, player: u64, mode: u32) {
    let mode = crate::world::GameMode::from_id(mode);
    unsafe {
        dim(w)
            .events
            .push(crate::server::DimEvent::SetMode { player, mode })
    }
}

unsafe extern "C" fn send_packet(w: *mut abi::WorldCtx, player: u64, name: Str, schema: Str, data: abi::Slice<u8>) {
    unsafe {
        if let Some(channel) = channel_for(name.as_str(), schema.as_str()) {
            dim(w)
                .events
                .push(crate::server::DimEvent::Packet { player, channel, data: data.as_slice().to_vec() });
        }
    }
}

/// The channel's id if it is registered and carries `schema`; a reason in the log otherwise.
pub fn channel_for(name: &str, schema: &str) -> Option<u32> {
    let g = &content().game;
    let Some(&channel) = g.packet_names.get(name) else {
        misuse("send", || format!("no channel {name} is registered"));
        return None;
    };
    let have = &g.packets[channel as usize].schema;
    if have != schema {
        misuse("send", || format!("channel {name} carries '{have}', not '{schema}'"));
        return None;
    }
    Some(channel)
}

unsafe extern "C" fn find_component(name: Str, schema: Str) -> u32 {
    let (name, schema) = unsafe { (name.as_str(), schema.as_str()) };
    let Some(c) = try_content() else { return u32::MAX };
    let Some(id) = c.components.id(name) else { return u32::MAX };
    let have = &c.components.schemas[id as usize];
    if !schema.is_empty() && have != schema {
        log::error!("component {name} is '{have}' but a mod reads it as '{schema}'");
        return u32::MAX;
    }
    id
}

unsafe extern "C" fn component_table() -> *const abi::ComponentTable {
    try_content().map_or(std::ptr::null(), |c| c.components.table())
}

unsafe extern "C" fn stack_get(data: u32, comp: u32, len: *mut usize) -> *const u8 {
    match crate::stack::value(data, comp) {
        Some(b) => {
            if !len.is_null() {
                unsafe { *len = b.len() }
            }
            b.as_ptr()
        }
        None => std::ptr::null(),
    }
}

unsafe extern "C" fn stack_with(data: u32, comp: u32, value: abi::Slice<u8>, remove: u32) -> u32 {
    if try_content().is_none_or(|c| comp >= c.components.count()) {
        misuse("stack_with", || "a component nobody declared: r.stack_component(&COMP, 0) makes it one".to_string());
        return data;
    }
    let bytes = unsafe { value.as_slice() };
    crate::stack::with(data, comp, (remove == 0).then_some(bytes))
}

unsafe extern "C" fn storage_slots(comp: u32) -> u32 {
    try_content()
        .and_then(|c| crate::server::holder::storage_slots(c, comp))
        .unwrap_or(0)
}

unsafe extern "C" fn component_info(comp: u32, name: *mut Str, schema: *mut Str) -> u32 {
    let Some(c) = try_content() else { return 0 };
    let Some(n) = c.components.names.get(comp as usize) else { return 0 };
    let s = c
        .components
        .schemas
        .get(comp as usize)
        .map_or("", String::as_str);
    unsafe {
        *name = Str::new(n);
        *schema = Str::new(s);
    }
    1
}

unsafe extern "C" fn block_info(state: u32, out: *mut abi::BlockInfo) -> u32 {
    let Some(c) = try_content() else { return 0 };
    let Some(s) = c.states.get(state as usize) else { return 0 };
    let b = &c.blocks[s.block as usize];
    let tex = |i: u32| {
        c.textures
            .get(i as usize)
            .map_or(Str::EMPTY, |t| Str::new(t))
    };
    let item = c
        .items
        .iter()
        .position(|i| i.block == Some(b.first_state))
        .map_or(u32::MAX, |i| i as u32);
    unsafe {
        *out = abi::BlockInfo {
            block: s.block,
            first_state: b.first_state,
            states: b.states,
            name: Str::new(&b.name),
            owner: Str::new(c.mod_name(b.owner)),
            flags: s.flags,
            opacity: s.opacity as u32,
            light: s.light,
            absorb: s.absorb,
            layer: s.layer,
            model: s.model,
            hardness: s.hardness,
            friction: s.friction,
            drag: s.drag,
            textures: s.textures.map(tex),
            liquid: s.liquid.map_or(u32::MAX, |l| l as u32),
            item,
        };
    }
    1
}

unsafe extern "C" fn item_info(item: u32, out: *mut abi::ItemInfo) -> u32 {
    let Some(c) = try_content() else { return 0 };
    let Some(d) = c.items.get(item as usize) else { return 0 };
    let worn = d
        .equip
        .and_then(|e| c.game.player_layout.equipment.get(e as usize))
        .map_or(Str::EMPTY, |e| Str::new(&e.name));
    unsafe {
        *out = abi::ItemInfo {
            name: Str::new(&d.name),
            owner: Str::new(c.mod_name(d.owner)),
            max_stack: d.max_stack,
            durability: d.durability,
            attack: d.attack,
            block: d.block.unwrap_or(u32::MAX),
            worn,
            handheld: d.handheld as u32,
        };
    }
    1
}

unsafe extern "C" fn energy_move(w: *mut abi::WorldCtx, h: *const abi::Holder, amount: i64, simulate: u32) -> i64 {
    unsafe { held(w, h).map_or(0, |at| crate::server::holder::move_energy(dim(w), content(), at, amount, simulate != 0)) }
}

unsafe extern "C" fn component_get(w: *mut abi::WorldCtx, h: *const abi::Holder, comp: u32, out: *mut u8, cap: u32) -> u32 {
    unsafe {
        let Some(v) = held(w, h).and_then(|at| crate::server::holder::component(dim(w), at, comp)) else { return 0 };
        std::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len().min(cap as usize));
        v.len() as u32
    }
}

unsafe extern "C" fn component_set(w: *mut abi::WorldCtx, h: *const abi::Holder, comp: u32, value: abi::Slice<u8>) -> u32 {
    unsafe {
        let v = value.as_slice();
        held(w, h).is_some_and(|at| crate::server::holder::set_component(dim(w), content(), at, comp, (!v.is_empty()).then_some(v))) as u32
    }
}

unsafe extern "C" fn gizmo_shown(w: *mut abi::WorldCtx, group: Str) -> u32 {
    unsafe { dim(w).gizmo_groups.iter().any(|g| g == group.as_str()) as u32 }
}

unsafe extern "C" fn gizmo(w: *mut abi::WorldCtx, group: Str, kind: u32, a: *const abi::Vec3, b: *const abi::Vec3, color: u32, ticks: u32) {
    unsafe {
        let d = dim(w);
        let group = group.as_str();
        if kind > abi::gizmo_kind::FILL || !d.gizmo_groups.iter().any(|g| g == group) {
            return;
        }
        let (a, b) = (d.place.to_world(crate::world::dvec(*a)), d.place.to_world(crate::world::dvec(*b)));
        d.gizmos
            .push(crate::net::GizmoShape::shape(group, kind as u8, a, b, color, ticks.max(1)));
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn gizmo_text(
    w: *mut abi::WorldCtx,
    group: Str,
    pos: *const abi::Vec3,
    size: f32,
    color: u32,
    ticks: u32,
    key: Str,
    args: abi::Slice<abi::UiArg>,
) {
    unsafe {
        let d = dim(w);
        let group = group.as_str();
        if !d.gizmo_groups.iter().any(|g| g == group) {
            return;
        }
        let at = d.place.to_world(crate::world::dvec(*pos));
        let text = crate::ui::text::Msg::new(key.as_str(), crate::client::modui::args(args));
        d.gizmos
            .push(crate::net::GizmoShape::text(group, at, size, color, ticks.max(1), text));
    }
}

// ---------------------------------------------------------------- menus

unsafe fn menu<'a>(m: *mut abi::MenuCtx) -> &'a mut MenuCall {
    unsafe { &mut *(m as *mut MenuCall) }
}

unsafe extern "C" fn menu_group(m: *mut abi::MenuCtx, name: Str, out: *mut abi::SlotGroup) -> u32 {
    unsafe {
        match menu(m).group(content(), name.as_str()) {
            Some((start, len, cols)) => {
                *out = abi::SlotGroup { start, len, cols };
                1
            }
            None => 0,
        }
    }
}

/// A machine's group in its entity's inventory: the menu built for its kind says where.
unsafe extern "C" fn inventory_group(w: *mut abi::WorldCtx, h: *const abi::Holder, name: Str, out: *mut abi::SlotGroup) -> u32 {
    unsafe {
        let c = content();
        let entity = match held(w, h) {
            Some(crate::server::holder::At::Entity(id)) if (*h).storage == 0 => id,
            _ => return 0,
        };
        let Some(e) = dim(w).entities.get(entity) else { return 0 };
        let layout = &c.game.player_layout;
        // A player's groups are the player inventory's.
        if e.player {
            return match layout.group(name.as_str()) {
                Some(g) => {
                    *out = abi::SlotGroup { start: g.start, len: g.count, cols: g.cols };
                    1
                }
                None => 0,
            };
        }
        // A wearing kind's equipment, each slot named like it.
        let worn = &c.game.entities[e.kind as usize].worn;
        let slot = layout
            .equipment
            .iter()
            .position(|q| q.name == name.as_str())
            .map(|i| i as u32);
        let (start, len) = match slot {
            _ if name.as_str() == registry::EQUIPMENT_GROUP => (worn.start, worn.len() as u32),
            Some(i) if i < worn.len() as u32 => (worn.start + i, 1),
            _ => (0, 0),
        };
        if len > 0 {
            *out = abi::SlotGroup { start, len, cols: 1 };
            return 1;
        }
        // A block entity's are those of the menu its block opens.
        let found = c
            .game
            .block_entities
            .iter()
            .find(|&(_, &k)| k == e.kind)
            .and_then(|(&b, _)| c.blocks[b as usize].opens)
            .and_then(|m| c.game.menus[m as usize].group(layout, name.as_str()))
            .filter(|g| g.0.owner == abi::slot_owner::HOLDER);
        match found {
            Some((g, _, n)) => {
                *out = abi::SlotGroup { start: g.start, len: n, cols: g.cols };
                1
            }
            None => 0,
        }
    }
}

unsafe extern "C" fn reg_player_inventory(r: *mut abi::Registrar, groups: abi::Slice<abi::SlotGroupDesc>) {
    unsafe {
        let r = reg(r);
        let groups = groups
            .as_slice()
            .iter()
            .map(|g| (g.name.as_str().to_string(), g.cols.max(1), g.rows.max(1), g.flags))
            .collect();
        r.game.player_inventory = Some((r.owner, groups));
    }
}

unsafe extern "C" fn player_inventory_group(name: Str, out: *mut abi::SlotGroup) -> u32 {
    unsafe {
        match content().game.player_layout.group(name.as_str()) {
            Some(g) => {
                *out = abi::SlotGroup { start: g.start, len: g.count, cols: g.cols };
                1
            }
            None => 0,
        }
    }
}

unsafe extern "C" fn reg_equipment_slot(r: *mut abi::Registrar, name: Str, bones: Str, background: Str) {
    unsafe {
        let r = reg(r);
        let bones: Vec<String> = bones
            .as_str()
            .split_whitespace()
            .map(str::to_string)
            .collect();
        r.game.equipment.push(registry::EquipmentSlot {
            name: name.as_str().to_string(),
            roles: crate::model::roles_of(&bones),
            bones,
            background: background.as_str().to_string(),
        });
    }
}

unsafe extern "C" fn menu_get(m: *mut abi::MenuCtx, slot: u32) -> ItemStack {
    unsafe { menu(m).get(slot) }
}

unsafe extern "C" fn menu_set(m: *mut abi::MenuCtx, slot: u32, stack: ItemStack) {
    if known_item("menu_set", &stack) {
        unsafe { menu(m).set(slot, stack) }
    }
}

// ---------------------------------------------------------------- lookups

unsafe extern "C" fn recipe_match(kind: Str, grid: *const ItemStack, width: u32, height: u32) -> ItemStack {
    let kind = unsafe { kind.as_str() };
    let cells: Vec<u32> = (0..(width * height) as usize)
        .map(|i| unsafe { (*grid.add(i)).item })
        .collect();
    content()
        .game
        .match_grid(kind, &cells, width, height, &content().components)
        .map_or(ItemStack::EMPTY, |r| r.result())
}

fn recipe_at(kind: Str, recipe: u32) -> Option<&'static registry::Recipe> {
    content()
        .game
        .recipes
        .get(unsafe { kind.as_str() })?
        .get(recipe as usize)
}

unsafe extern "C" fn recipe_find(kind: Str, stacks: *const ItemStack, count: u32) -> u32 {
    let c = content();
    let stacks = unsafe { std::slice::from_raw_parts(stacks, count as usize) };
    let Some(list) = c.game.recipes.get(unsafe { kind.as_str() }) else { return 0 };
    list.iter()
        .position(|r| c.game.find_inputs(r, stacks, &c.components).is_some())
        .map_or(0, |i| i as u32 + 1)
}

unsafe extern "C" fn recipe_info(kind: Str, recipe: u32, out: *mut abi::RecipeInfo) -> u32 {
    let Some(r) = recipe_at(kind, recipe) else { return 0 };
    let flags = if r.hidden { abi::recipe_flags::HIDDEN } else { 0 };
    unsafe { *out = abi::RecipeInfo { time: r.time, inputs: r.inputs.len() as u32, outputs: r.outputs.len() as u32, flags } };
    1
}

unsafe extern "C" fn recipe_output(kind: Str, recipe: u32, index: u32, stack: *mut ItemStack, chance: *mut f32) -> u32 {
    let Some(o) = recipe_at(kind, recipe).and_then(|r| r.outputs.get(index as usize)) else { return 0 };
    unsafe {
        *stack = match o.what {
            registry::Ingredient::Item(i) => ItemStack::new(i, o.count),
            _ => ItemStack::EMPTY,
        };
        *chance = o.chance;
    }
    1
}

unsafe extern "C" fn recipe_value(kind: Str, recipe: u32, component: Str, out: *mut u8, cap: u32) -> u32 {
    let c = content();
    let Some(id) = c.components.id(unsafe { component.as_str() }) else { return 0 };
    let Some(v) = recipe_at(kind, recipe).and_then(|r| r.value(id)) else { return 0 };
    let n = v.len().min(cap as usize);
    unsafe { std::ptr::copy_nonoverlapping(v.as_ptr(), out, n) };
    v.len() as u32
}

unsafe extern "C" fn recipe_consume(kind: Str, recipe: u32, stacks: *mut ItemStack, count: u32) -> u32 {
    let c = content();
    let Some(r) = recipe_at(kind, recipe) else { return 0 };
    let stacks = unsafe { std::slice::from_raw_parts_mut(stacks, count as usize) };
    let Some(found) = c.game.find_inputs(r, stacks, &c.components) else { return 0 };
    let counts = r
        .inputs
        .iter()
        .zip(&r.input_counts)
        .filter(|(i, _)| !matches!(i, registry::Ingredient::Empty))
        .map(|(_, &n)| n);
    for (i, n) in found.into_iter().zip(counts) {
        stacks[i].count -= n;
        if stacks[i].count == 0 {
            stacks[i] = ItemStack::EMPTY;
        }
    }
    1
}

unsafe extern "C" fn recipe_process(kind: Str, input: u32, time_out: *mut u32) -> ItemStack {
    let kind = unsafe { kind.as_str() };
    match content().game.process(kind, input, &content().components) {
        Some(r) => {
            if !time_out.is_null() {
                unsafe { *time_out = r.time }
            }
            r.result()
        }
        None => ItemStack::EMPTY,
    }
}

unsafe extern "C" fn item_block(item: u32) -> u32 {
    try_content()
        .and_then(|c| c.items.get(item as usize))
        .and_then(|d| d.block)
        .unwrap_or(0)
}

unsafe extern "C" fn find(kind: u32, name: Str, count: *mut u32) -> u32 {
    let name = unsafe { name.as_str() };
    let Some(c) = try_content() else { return u32::MAX };
    let mut n = 1;
    let id = match kind {
        // `#tag`: the first block carrying it, in mod load order (biome surfaces by tag).
        // `#tag`: its preferred block (mod load order) or item.
        abi::entry_kind::BLOCK if name.starts_with('#') => crate::refs::Ref::parse(name)
            .ok()
            .and_then(|r| crate::refs::state(&r, &c.block_names, &c.blocks, &c.components).ok())
            .inspect(|&s| n = c.block_of(s).states),
        abi::entry_kind::ITEM if name.starts_with('#') => crate::refs::Ref::parse(name)
            .ok()
            .and_then(|r| crate::refs::item(&r, &c.item_names, &c.components, &c.tag_items).ok()),
        abi::entry_kind::BLOCK if name.contains('[') => c.state_by_name(name),
        abi::entry_kind::BLOCK => c.block_names.get(name).map(|&b| {
            n = c.blocks[b as usize].states;
            c.blocks[b as usize].first_state
        }),
        abi::entry_kind::ITEM => c.item_names.get(name).copied(),
        abi::entry_kind::ENTITY => c.game.entity_names.get(name).copied(),
        abi::entry_kind::BIOME => c.biome_names.get(name).copied(),
        abi::entry_kind::DIMENSION => c.dim_names.get(name).copied(),
        abi::entry_kind::MENU => c.game.menu_names.get(name).copied(),
        abi::entry_kind::PARTICLE => c.game.particle_names.get(name).copied(),
        abi::entry_kind::CONFIG => c.game.config_names.get(name).copied(),
        abi::entry_kind::SOUND => c.game.sound_names.get(name).copied(),
        abi::entry_kind::DAMAGE_CAUSE => c.game.damage_cause_names.get(name).copied(),
        abi::entry_kind::ENTITY_EFFECT => c.game.entity_effect_names.get(name).copied(),
        abi::entry_kind::TEXT_PLACE => c.game.text_place_names.get(name).copied(),
        abi::entry_kind::GAME_MODE => c.game.game_mode_names.get(name).copied(),
        abi::entry_kind::POSE => c.game.pose_names.get(name).copied(),
        abi::entry_kind::WEATHER => c.game.weather_names.get(name).copied(),
        abi::entry_kind::PRECIPITATION => c.game.precipitation_names.get(name).copied(),
        abi::entry_kind::CONDITION => c.game.condition_names.get(name).copied(),
        _ => None,
    };
    if !count.is_null() {
        unsafe { *count = if id.is_some() { n } else { 0 } }
    }
    id.unwrap_or(u32::MAX)
}

unsafe extern "C" fn name_of(kind: u32, id: u32) -> Str {
    static STATE_NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let Some(c) = try_content() else { return Str::EMPTY };
    let name: Option<&str> = match kind {
        abi::entry_kind::BLOCK => {
            let names = STATE_NAMES.get_or_init(|| {
                (0..c.states.len() as u32)
                    .map(|s| c.state_name(s))
                    .collect()
            });
            names.get(id as usize).map(|s| s.as_str())
        }
        abi::entry_kind::ITEM => c.items.get(id as usize).map(|i| i.name.as_str()),
        abi::entry_kind::ENTITY => c.game.entities.get(id as usize).map(|e| e.name.as_str()),
        abi::entry_kind::BIOME => c.biomes.get(id as usize).map(|b| b.name.as_str()),
        abi::entry_kind::DIMENSION => c.dims.get(id as usize).map(|d| d.name.as_str()),
        abi::entry_kind::MENU => c.game.menus.get(id as usize).map(|m| m.name.as_str()),
        abi::entry_kind::PARTICLE => c.game.particles.get(id as usize).map(|p| p.name.as_str()),
        abi::entry_kind::CONFIG => c.game.configs.get(id as usize).map(|d| d.name.as_str()),
        abi::entry_kind::SOUND => c.game.sounds.get(id as usize).map(|d| d.name.as_str()),
        _ => None,
    };
    name.map_or(Str::EMPTY, Str::new)
}

unsafe extern "C" fn state_prop(state: u32, name: Str) -> Str {
    let c = content();
    if state as usize >= c.states.len() {
        return Str::EMPTY;
    }
    c.state_prop(state, unsafe { name.as_str() })
        .map_or(Str::EMPTY, Str::new)
}

unsafe extern "C" fn state_with(state: u32, name: Str, value: Str) -> u32 {
    let c = content();
    if state as usize >= c.states.len() {
        return u32::MAX;
    }
    unsafe { c.state_with(state, name.as_str(), value.as_str()) }.unwrap_or(u32::MAX)
}

unsafe extern "C" fn state_cycle(state: u32, name: Str) -> u32 {
    let c = content();
    let Some(s) = c.states.get(state as usize) else { return u32::MAX };
    let name = unsafe { name.as_str() };
    let Some(prop) = c.blocks[s.block as usize]
        .props
        .iter()
        .find(|p| p.name == name)
    else {
        return u32::MAX;
    };
    let at = (state - c.blocks[s.block as usize].first_state) / prop.stride.max(1);
    let next = (at as usize % prop.values.len().max(1) + 1) % prop.values.len().max(1);
    c.state_with(state, name, &prop.values[next])
        .unwrap_or(u32::MAX)
}

unsafe extern "C" fn block_of(state: u32) -> u32 {
    let c = content();
    c.states
        .get(state as usize)
        .map_or(0, |s| c.blocks[s.block as usize].first_state)
}

unsafe extern "C" fn state_flags(state: u32) -> u32 {
    content().states.get(state as usize).map_or(0, |s| s.flags)
}

// ---------------------------------------------------------------- registration

fn level(level: u32) -> log::Level {
    match level {
        abi::log_level::ERROR => log::Level::Error,
        abi::log_level::WARN => log::Level::Warn,
        abi::log_level::DEBUG => log::Level::Debug,
        abi::log_level::TRACE => log::Level::Trace,
        _ => log::Level::Info,
    }
}

fn mod_target(me: ModId, sub: &str) -> String {
    let name = crate::modloader::mod_id_name(me);
    if sub.is_empty() { format!("mod:{name}") } else { format!("mod:{name}::{sub}") }
}

unsafe extern "C" fn host_log(me: ModId, lvl: u32, msg: Str) {
    unsafe { log_at(me, lvl, Str::EMPTY, msg) }
}

unsafe extern "C" fn log_at(me: ModId, lvl: u32, target: Str, msg: Str) {
    let (target, msg) = unsafe { (mod_target(me, target.as_str()), msg.as_str()) };
    log::log!(target: &target, level(lvl), "{msg}");
}

unsafe extern "C" fn log_enabled(me: ModId, lvl: u32, target: Str) -> u32 {
    let lvl = level(lvl);
    if lvl > log::max_level() {
        return 0;
    }
    crate::log::enabled(&mod_target(me, unsafe { target.as_str() }), lvl) as u32
}

unsafe fn reg<'a>(r: *mut abi::Registrar) -> &'a mut RawRegistrar {
    unsafe { &mut *(r as *mut RawRegistrar) }
}

unsafe extern "C" fn reg_block(r: *mut abi::Registrar, d: *const abi::BlockDesc) {
    unsafe {
        let b = content::copy_block(&*d);
        let r = reg(r);
        if b.states.is_empty() {
            r.errors.push(format!("block {} has no states", b.name));
            return;
        }
        r.blocks.push(b);
    }
}

unsafe extern "C" fn reg_item(r: *mut abi::Registrar, d: *const abi::ItemDesc) {
    unsafe {
        let r = reg(r);
        let mut item = content::copy_item(&*d);
        item.seq = r.items.len() as u32;
        r.items.push(item);
    }
}

unsafe extern "C" fn reg_biome(r: *mut abi::Registrar, d: *const abi::BiomeDesc) {
    unsafe {
        let d = &*d;
        reg(r).biomes.push((
            content::BiomeDef { name: d.name.as_str().to_string(), tints: Vec::new() },
            d.cell as usize,
            crate::components::RawValue::list(d.components),
            crate::server::terrain::copy_rule(&d.rule),
        ));
    }
}

unsafe extern "C" fn reg_dimension(r: *mut abi::Registrar, d: *const abi::DimensionDesc) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.dims.push(content::copy_dimension(owner, &*d));
    }
}

unsafe extern "C" fn reg_entity(r: *mut abi::Registrar, d: *const abi::EntityDesc) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game.entities.push(registry::copy_entity(owner, &*d));
    }
}

unsafe extern "C" fn reg_entity_model(r: *mut abi::Registrar, kind: Str, parts: abi::Slice<abi::ModelPart>, uv_width: u32, uv_height: u32) {
    unsafe {
        let parts = registry::copy_parts(parts);
        reg(r)
            .game
            .entity_models
            .push((kind.as_str().to_string(), parts, [uv_width, uv_height]));
    }
}

unsafe extern "C" fn reg_entity_component(r: *mut abi::Registrar, d: *const abi::EntityComponentDesc) {
    unsafe {
        let d = &*d;
        let mut default = d.default.as_slice().to_vec();
        default.resize(d.value_size as usize, 0);
        let r = reg(r);
        r.game.components.push(registry::RawComponent {
            owner: r.owner,
            def: registry::ComponentDef { name: d.name.as_str().to_string(), size: d.value_size, flags: d.flags, default, runtime: true },
            schema: d.schema.as_str().to_string(),
        });
    }
}

unsafe extern "C" fn reg_world_tick(r: *mut abi::Registrar, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game.world_ticks.push((owner, cb));
    }
}

unsafe extern "C" fn emit_after(w: *mut abi::WorldCtx, entity: u64, ticks: u32, event: u32, data: abi::Slice<u8>) {
    unsafe {
        let c = content();
        let Some(def) = c.game.events.get(event as usize) else {
            misuse("after", || format!("no event {event}"));
            return;
        };
        let d = dim(w);
        if d.entities.get(entity).is_none() {
            return;
        }
        let until = d.time + ticks.max(1) as u64;
        let list = d
            .entities
            .comps
            .entry((entity, c.game.timers_comp))
            .or_insert_with(|| Box::new([]));
        // One timer per entity and event: setting it again moves it.
        let mut v = Vec::new();
        for (t, name, value) in crate::server::dim::read_timers(list) {
            if name != def.name {
                crate::server::dim::push_timer(&mut v, t, &name, &value);
            }
        }
        crate::server::dim::push_timer(&mut v, until, &def.name, data.as_slice());
        *list = v.into_boxed_slice();
    }
}

unsafe extern "C" fn reg_recipe(r: *mut abi::Registrar, d: *const abi::RecipeDesc) {
    unsafe { reg(r).game.recipes.push(registry::copy_recipe(&*d)) }
}

unsafe extern "C" fn reg_menu(r: *mut abi::Registrar, d: *const abi::MenuDesc) {
    unsafe {
        let r = reg(r);
        let (owner, d) = (r.owner, &*d);
        // Its screen opens on the client alone too (`client_op::OPEN_SCREEN`), with nothing to sync.
        if d.screen.is_some() {
            let name = d.name.as_str().to_string();
            r.game
                .screens
                .push(registry::ScreenDef { name, owner, cb: d.screen, flags: d.flags, on_key: d.on_key });
        }
        r.game.menus.push(registry::copy_menu(owner, d));
    }
}

unsafe extern "C" fn reg_set(r: *mut abi::Registrar, d: *const abi::SetDesc) {
    use crate::components::Target;
    unsafe {
        let d = *d;
        let r = reg(r);
        let name = d.target.as_str().to_string();
        let target = match d.target_kind {
            // A reference: `#pattern` for everything tagged, else a name.
            abi::set_target::NAME if name.starts_with('#') => Target::Tagged(name[1..].to_string()),
            abi::set_target::NAME => Target::Name(name),
            abi::set_target::TAGGED => Target::Tagged(name),
            abi::set_target::ENTITY => Target::Entity(name),
            abi::set_target::BIOME => Target::Biome(name),
            abi::set_target::DIMENSION => Target::Dimension(name),
            k => {
                r.errors
                    .push(format!("set {}: unknown target kind {k}", d.value.name.as_str()));
                return;
            }
        };
        let value = crate::components::RawValue::copy(&d.value);
        r.game
            .sets
            .push(crate::components::RawSet { owner: r.owner, target, value, remove: d.remove != 0 });
    }
}

unsafe extern "C" fn reg_creative_tab(r: *mut abi::Registrar, name: Str, icon: Str) {
    unsafe {
        let r = reg(r);
        r.game
            .creative_tabs
            .push((name.as_str().to_string(), icon.as_str().to_string()));
    }
}

unsafe extern "C" fn reg_screen_part(r: *mut abi::Registrar, screen: Str, area: Str, order: i32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game.screen_parts.push(registry::ScreenPart {
            screen: screen.as_str().to_string(),
            area: area.as_str().to_string(),
            order,
            owner,
            cb,
        });
    }
}

unsafe extern "C" fn reg_scatter(r: *mut abi::Registrar, d: *const abi::ScatterDesc) {
    unsafe {
        let r = reg(r);
        let d = &*d;
        r.game.scatters.push(registry::RawScatter {
            owner: r.owner,
            name: d.name.as_str().to_string(),
            dimensions: d
                .dimensions
                .as_str()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            cell: d.cell,
            reach: d.reach,
            reach_y: [d.reach_down.clamp(0, 256), d.reach_up.clamp(0, 256)],
            density: d.density.as_str().to_string(),
            ground: d
                .ground
                .as_str()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            shape: d.shape,
        });
    }
}

unsafe extern "C" fn reg_decoration(r: *mut abi::Registrar, dimensions: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        r.game.decorations.push((
            r.owner,
            dimensions
                .as_str()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            cb,
        ));
    }
}

unsafe extern "C" fn reg_terrain(r: *mut abi::Registrar, d: *const abi::TerrainDesc) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .terrain
            .push(crate::server::terrain::copy(owner, &*d));
    }
}

unsafe extern "C" fn dim_column(dim: u32, seed: u64, x: i32, z: i32, out: *mut abi::TerrainColumn) -> u32 {
    let Some(col) = content()
        .dims
        .get(dim as usize)
        .and_then(|d| d.column(seed, x, z))
    else {
        return 0;
    };
    unsafe { *out = col };
    1
}

unsafe extern "C" fn dim_surface_carved(dim: u32, seed: u64, x: i32, z: i32) -> u32 {
    content()
        .dims
        .get(dim as usize)
        .is_some_and(|d| d.carved(seed, x, z)) as u32
}

unsafe extern "C" fn dim_surface_block(dim: u32, seed: u64, x: i32, z: i32) -> u32 {
    content()
        .dims
        .get(dim as usize)
        .map_or(0, |d| d.surface_block(seed, x, z))
}

unsafe extern "C" fn reg_ore(r: *mut abi::Registrar, d: *const abi::OreDesc) {
    unsafe {
        let r = reg(r);
        let d = &*d;
        r.game.ores.push(registry::RawOre {
            dimensions: d
                .dimensions
                .as_str()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            block: d.block.as_str().to_string(),
            replace: d
                .replace
                .as_str()
                .split_whitespace()
                .map(str::to_string)
                .collect(),
            min_y: d.min_y,
            max_y: d.max_y,
            veins: d.veins,
            size: d.blocks,
            biome: d.biome.as_str().to_string(),
            shape: d.shape,
            shape_size: d
                .shape_size
                .map(|v| if v.is_finite() { v.clamp(0.0, 15.0) } else { 0.0 }),
            height: d.height,
            replace_with: d
                .replace_with
                .as_str()
                .split_whitespace()
                .filter_map(|p| p.split_once('>'))
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
            exposed: d.exposed,
            edit: d.edit != 0,
            plateau: d.plateau.max(0),
        });
    }
}

unsafe extern "C" fn reg_command(r: *mut abi::Registrar, name: Str, usage: Str, client: u32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        let list = if client != 0 { &mut r.game.client_commands } else { &mut r.game.commands };
        list.push((owner, name.as_str().to_string(), usage.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn command_reply(args: *mut abi::CommandArgs, key: Str, list: abi::Slice<abi::UiArg>) {
    unsafe {
        let reply = &mut *((*args).reply as *mut Vec<crate::ui::text::Msg>);
        reply.push(crate::ui::text::Msg::new(key.as_str(), crate::client::modui::args(list)));
    }
}

/// The text of a `ShowDesc`, to whoever it goes.
pub(crate) unsafe fn shown(d: &abi::ShowDesc) -> crate::net::Shown {
    unsafe {
        crate::net::Shown {
            id: d.id.as_str().to_string(),
            times: d.times,
            scale: d.scale,
            at: (d.at[0] >= 0.0 && d.at[1] >= 0.0).then_some(d.at),
            color: (d.color >= 0).then_some(d.color as u32),
            ..crate::net::Shown::new(d.place, crate::ui::text::Msg::new(d.key.as_str(), crate::client::modui::args(d.args)))
        }
    }
}

unsafe extern "C" fn show(w: *mut abi::WorldCtx, who: abi::Who, d: *const abi::ShowDesc) {
    unsafe {
        dim(w)
            .events
            .push(crate::server::DimEvent::Show { who, shown: shown(&*d) })
    }
}

unsafe extern "C" fn reg_chat_format(r: *mut abi::Registrar, cb: Callback) {
    unsafe {
        let r = reg(r);
        r.game.chat_format = Some((r.owner, cb));
    }
}

unsafe extern "C" fn chat_format_set(args: *mut abi::ChatFormatArgs, key: Str, list: abi::Slice<abi::UiArg>) {
    unsafe {
        let out = &mut *((*args).out as *mut Option<crate::ui::text::Msg>);
        *out = Some(crate::ui::text::Msg::new(key.as_str(), crate::client::modui::args(list)));
    }
}

unsafe extern "C" fn game_mode(w: *mut abi::WorldCtx, entity: u64) -> u32 {
    unsafe { dim(w).entities.get(entity).map_or(0, |e| e.mode.0 as u32) }
}

unsafe extern "C" fn reg_hud_element(r: *mut abi::Registrar, name: Str, anchor: Str, order: i32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game.huds.push(registry::HudElement {
            name: name.as_str().to_string(),
            title: crate::ui::text::dotted(name.as_str()),
            anchor: anchor.as_str().to_string(),
            order,
            owner,
            cb,
        });
    }
}

unsafe extern "C" fn set_player_menu(r: *mut abi::Registrar, menu: Str) {
    unsafe {
        let r = reg(r);
        r.game.player_menu = Some((r.owner, menu.as_str().to_string()));
    }
}

unsafe extern "C" fn reg_tint_kind(r: *mut abi::Registrar, name: Str, default: u32) {
    unsafe { reg(r).tint_kinds.push((name.as_str().to_string(), default)) }
}

unsafe extern "C" fn reg_biome_tint(r: *mut abi::Registrar, biome: Str, kind: Str, color: u32) {
    unsafe {
        reg(r)
            .biome_tints
            .push((biome.as_str().to_string(), kind.as_str().to_string(), color))
    }
}

unsafe extern "C" fn reg_sound_category(r: *mut abi::Registrar, name: Str, world: u32) {
    unsafe {
        reg(r)
            .game
            .sound_categories
            .push(registry::SoundCategory::new(name.as_str(), world != 0))
    }
}

unsafe extern "C" fn reg_damage_cause(r: *mut abi::Registrar, d: *const abi::DamageCauseDesc) {
    unsafe {
        let d = *d;
        let def = registry::DamageCauseDef {
            name: d.name.as_str().to_string(),
            flags: d.flags,
            death_message: d.death_message.as_str().to_string(),
            death_message_by: d.death_message_by.as_str().to_string(),
        };
        reg(r).game.damage_causes.push((d.cell as usize, def));
    }
}

unsafe extern "C" fn damage_cause_flags(cause: u32) -> u32 {
    try_content()
        .and_then(|c| c.game.damage_causes.get(cause as usize))
        .map_or(0, |d| d.flags)
}

unsafe extern "C" fn reg_entity_category(r: *mut abi::Registrar, name: Str, cap: u32, despawn: f32, cap_radius: f32) {
    unsafe {
        reg(r).game.categories.push(crate::registry::CategoryDef {
            name: name.as_str().to_string(),
            cap,
            despawn: despawn as f64,
            cap_radius: if cap_radius > 0.0 { cap_radius as f64 } else { 64.0 },
        })
    }
}

unsafe extern "C" fn reg_recipe_kind(r: *mut abi::Registrar, d: *const abi::RecipeKindDesc) {
    unsafe {
        let d = &*d;
        let r = reg(r);
        let def = registry::RecipeKindDef {
            owner: r.owner,
            input_grid: d.input_grid,
            output_grid: d.output_grid,
            info: (d.info.len > 0).then(|| d.info.as_str().to_string()),
            info_values: d
                .info_values
                .as_slice()
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            display: d.layout.is_some().then_some(d.layout),
        };
        r.game
            .recipe_kinds
            .push(registry::RawRecipeKind { name: d.name.as_str().to_string(), def });
    }
}

unsafe extern "C" fn reg_attach(r: *mut abi::Registrar, kind: Str, component: Str, value: abi::Slice<u8>) {
    let value = unsafe { value.as_slice() };
    unsafe {
        reg(r)
            .game
            .attach
            .push((kind.as_str().to_string(), component.as_str().to_string(), (!value.is_empty()).then(|| value.to_vec())))
    }
}

unsafe extern "C" fn reg_liquid(r: *mut abi::Registrar, d: *const abi::LiquidDesc) {
    unsafe {
        let d = *d;
        reg(r).game.liquids.push((
            d.name.as_str().to_string(),
            d.overlay.as_str().to_string(),
            abi::LiquidDesc { overlay: Str::EMPTY, ..d },
        ));
    }
}

unsafe extern "C" fn reg_liquid_edit(r: *mut abi::Registrar, d: *const abi::LiquidEdit) {
    unsafe {
        let d = *d;
        reg(r).game.liquid_changes.push((
            d.name.as_str().to_string(),
            d.overlay.as_str().to_string(),
            abi::LiquidEdit { name: Str::EMPTY, overlay: Str::EMPTY, ..d },
        ));
    }
}

unsafe extern "C" fn reg_liquid_reaction(r: *mut abi::Registrar, d: *const abi::ReactionDesc) {
    unsafe {
        let d = *d;
        reg(r)
            .game
            .reactions
            .push([d.liquid, d.touching, d.source, d.flowing, d.falls_into].map(|s| s.as_str().to_string()));
    }
}

unsafe extern "C" fn liquid_of(state: u32) -> u32 {
    try_content()
        .and_then(|c| c.states.get(state as usize))
        .and_then(|s| s.liquid)
        .map_or(u32::MAX, |l| l as u32)
}

unsafe extern "C" fn use_container(w: *mut abi::WorldCtx, player: u64, stack: ItemStack, handled: *mut u32) -> ItemStack {
    unsafe {
        let r = dim(w).use_container(content(), player, stack);
        if !handled.is_null() {
            *handled = r.is_some() as u32;
        }
        r.unwrap_or(stack)
    }
}

unsafe extern "C" fn reg_event_handler(r: *mut abi::Registrar, name: Str, schema: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .event_handlers
            .push((owner, name.as_str().to_string(), schema.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn find_event(name: Str, schema: Str) -> u32 {
    let (name, schema) = unsafe { (name.as_str(), schema.as_str()) };
    let Some(c) = try_content() else { return u32::MAX };
    let Some(&id) = c.game.event_names.get(name) else { return u32::MAX };
    let def = &c.game.events[id as usize];
    if !schema.is_empty() && def.schema != schema {
        log::error!("event {name} is '{}' but a mod emits '{schema}'", def.schema);
        return u32::MAX;
    }
    if def.handlers.is_empty() { u32::MAX } else { id }
}

unsafe extern "C" fn emit(w: *mut abi::WorldCtx, event: u32, value: abi::Slice<u8>) {
    unsafe { dim(w).emit(content(), event, value.as_slice()) }
}

unsafe extern "C" fn reg_interceptor(r: *mut abi::Registrar, name: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .interceptors
            .push((owner, name.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn ask(w: *mut abi::WorldCtx, name: Str, query: *mut abi::Query) -> u32 {
    unsafe {
        let mut q = *query;
        let ok = dim(w).ask(content(), name.as_str(), &mut q);
        *query = q;
        ok as u32
    }
}

unsafe extern "C" fn reg_chain_modifier(r: *mut abi::Registrar, name: Str, stage: u32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .modifiers
            .push((owner, name.as_str().to_string(), stage, cb));
    }
}

unsafe extern "C" fn chain_compute(w: *mut abi::WorldCtx, name: Str, base: f64, ctx: *const abi::Query, explain: u32) -> f64 {
    unsafe {
        let q = if ctx.is_null() { abi::Query::DEFAULT } else { *ctx };
        dim(w).compute(content(), name.as_str(), base, &q, explain != 0)
    }
}

unsafe extern "C" fn explanation() -> Str {
    thread_local! {
        static TEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    TEXT.with(|t| {
        let mut t = t.borrow_mut();
        *t = crate::server::explanation();
        Str { ptr: t.as_ptr(), len: t.len() }
    })
}

unsafe extern "C" fn reg_attribute(r: *mut abi::Registrar, name: Str, default: f64, min: f64, max: f64) {
    unsafe {
        reg(r)
            .game
            .attributes
            .push((name.as_str().to_string(), default, min, max))
    }
}

unsafe extern "C" fn attribute(w: *mut abi::WorldCtx, entity: u64, name: Str) -> f64 {
    unsafe { dim(w).attribute_named(content(), entity, name.as_str(), false) }
}

unsafe extern "C" fn attribute_explain(w: *mut abi::WorldCtx, entity: u64, name: Str) -> f64 {
    unsafe { dim(w).attribute_named(content(), entity, name.as_str(), true) }
}

unsafe extern "C" fn attribute_add_modifier(w: *mut abi::WorldCtx, entity: u64, name: Str, id: Str, op: u32, amount: f64, ticks: u64) {
    unsafe {
        let c = content();
        match c.game.attr_names.get(name.as_str()) {
            Some(&a) => dim(w).set_attr_modifier(c, entity, a, id.as_str(), op, amount, ticks),
            None => log::error!("no attribute {}", name.as_str()),
        }
    }
}

unsafe extern "C" fn attribute_remove_modifier(w: *mut abi::WorldCtx, entity: u64, name: Str, id: Str) {
    unsafe {
        let c = content();
        match c.game.attr_names.get(name.as_str()) {
            Some(&a) => dim(w).remove_attr_modifier(c, entity, a, id.as_str()),
            None => misuse("attribute_remove", || format!("no attribute {}", name.as_str())),
        }
    }
}

unsafe extern "C" fn reg_loot(r: *mut abi::Registrar, d: *const abi::LootDesc) {
    unsafe {
        let d = *d;
        let r = reg(r);
        let mut pools = Vec::new();
        d.pools
            .as_slice()
            .iter()
            .copied()
            .for_each(|p| pools.push(crate::loot::copy_pool(&p)));
        r.game
            .loot
            .push(crate::loot::RawLoot { owner: r.owner, name: d.name.as_str().to_string(), pools });
    }
}

unsafe extern "C" fn reg_override(r: *mut abi::Registrar, kind: u32, name: Str, removed: u32) {
    unsafe {
        reg(r)
            .overrides
            .push((kind, name.as_str().to_string(), removed != 0))
    }
}

unsafe extern "C" fn reg_loot_edit(r: *mut abi::Registrar, d: *const abi::LootEditDesc) {
    unsafe {
        let d = *d;
        let r = reg(r);
        let pool = crate::loot::copy_pool(&d.pool);
        r.game.loot_mods.push(crate::loot::RawLootMod {
            owner: r.owner,
            table: d.table.as_str().to_string(),
            op: d.op,
            pool,
            from: d.from.as_str().to_string(),
            to: d.to.as_str().to_string(),
        });
    }
}

unsafe extern "C" fn roll_loot(w: *mut abi::WorldCtx, name: Str, ctx: *const abi::LootCtx, out: *mut ItemStack, max: usize) -> usize {
    unsafe {
        let x = if ctx.is_null() { abi::LootCtx::DEFAULT } else { *ctx };
        let ctx = crate::loot::Ctx {
            tool: x.tool,
            by_player: x.by_player != 0,
            on_fire: x.on_fire != 0,
            mined: x.mined,
            state: x.mined,
            holds: Vec::new(),
        };
        let stacks = dim(w).roll_loot(content(), name.as_str(), &ctx, DVec3::ZERO, 0);
        for (i, s) in stacks.iter().take(max).enumerate() {
            *out.add(i) = *s;
        }
        stacks.len()
    }
}

unsafe extern "C" fn break_block(w: *mut abi::WorldCtx, pos: *const abi::BlockPos, tool: ItemStack, player: u64, drops: u32) -> u32 {
    unsafe { dim(w).break_block(content(), ip(pos), tool, player, drops != 0) as u32 }
}

unsafe extern "C" fn set_entity_size(w: *mut abi::WorldCtx, id: u64, size: abi::EntitySize) {
    unsafe {
        let d = dim(w);
        if let Some(e) = d.entities.get_mut(id) {
            let height = size.height.max(0.0);
            e.size = (size.width > 0.0).then_some(abi::EntitySize { width: size.width, height, eye: size.eye.clamp(0.0, height) });
        }
    }
}

unsafe extern "C" fn set_entity_appearance(w: *mut abi::WorldCtx, id: u64, kind: Str) {
    unsafe {
        let name = kind.as_str();
        let look = match name {
            "" => None,
            n => match content().game.entity_names.get(n) {
                Some(&k) => Some(k),
                None => return misuse("set_entity_look", || format!("no entity kind {n}")),
            },
        };
        let d = dim(w);
        if let Some(e) = d.entities.get_mut(id) {
            e.look = look;
        }
    }
}

unsafe extern "C" fn entity_kind_size(kind: Str, out: *mut abi::EntitySize) -> u32 {
    unsafe {
        let g = &content().game;
        let Some(&k) = g.entity_names.get(kind.as_str()) else { return 0 };
        let e = &g.entities[k as usize];
        *out = e.size();
        1
    }
}

unsafe extern "C" fn reg_words(r: *mut abi::Registrar, set: Str, words: Str) {
    unsafe {
        let words = words
            .as_str()
            .split(',')
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(String::from)
            .collect();
        reg(r).game.words.push((set.as_str().to_string(), words));
    }
}

unsafe extern "C" fn set_weather(w: *mut abi::WorldCtx, weather: u32, ticks: u32) {
    unsafe {
        let d = dim(w);
        if weather as usize >= content().game.weathers.len() {
            misuse("set_weather", || format!("no weather {weather}"));
            return;
        }
        d.events
            .push(crate::server::DimEvent::SetWeather { dim: d.id, weather: weather as u16, ticks });
    }
}

unsafe extern "C" fn weather_now(w: *mut abi::WorldCtx) -> u32 {
    unsafe { dim(w).weather as u32 }
}

unsafe extern "C" fn reg_weather(r: *mut abi::Registrar, d: *const abi::WeatherDesc) {
    unsafe {
        let d = &*d;
        let def = registry::WeatherDef {
            name: d.name.as_str().to_string(),
            flags: d.flags,
            look: abi::WeatherLook {
                falls: d.look.falls.clamp(0.0, 1.0),
                sky_gray: d.look.sky_gray.clamp(0.0, 1.0),
                sky_cover: d.look.sky_cover.clamp(0.0, 1.0),
                fog_gray: d.look.fog_gray.clamp(0.0, 1.0),
                cloud_gray: d.look.cloud_gray.clamp(0.0, 1.0),
                cloud_dark: d.look.cloud_dark.clamp(0.0, 1.0),
                darkness: d.look.darkness.clamp(0.0, 1.0),
            },
            sound_name: d.sound.as_str().to_string(),
            sound: None,
            duration: [d.duration[0].max(1) as u64, d.duration[1].max(d.duration[0]).max(1) as u64],
            weight: d.weight,
        };
        reg(r).game.weathers.push((d.cell as usize, def));
    }
}

unsafe extern "C" fn reg_precipitation(r: *mut abi::Registrar, d: *const abi::PrecipitationDesc) {
    unsafe {
        let d = &*d;
        let def = registry::PrecipitationDef {
            name: d.name.as_str().to_string(),
            texture: d.texture.as_str().to_string(),
            speed: d.speed.max(0.0),
            density: d.density.clamp(0.0, 1.0),
        };
        reg(r).game.precipitations.push((d.cell as usize, def));
    }
}

extern "C" fn weather_flags(weather: u32) -> u32 {
    content()
        .game
        .weathers
        .get(weather as usize)
        .map_or(0, |d| d.flags)
}

unsafe extern "C" fn entity_effect(w: *mut abi::WorldCtx, id: u64, effect: u32, strength: f32, ticks: u32) {
    unsafe {
        let d = dim(w);
        let c = content();
        if effect as usize >= c.game.entity_effects.len() {
            misuse("entity_effect", || format!("no entity effect {effect}"));
            return;
        }
        if d.entities.get(id).is_none() {
            return;
        }
        let comp = c.game.field_comps[9];
        let bytes: Vec<u8> = if ticks == 0 {
            Vec::new()
        } else {
            let until = d.time + ticks as u64;
            [effect.to_le_bytes(), strength.clamp(0.0, 1.0).to_le_bytes()]
                .concat()
                .into_iter()
                .chain(until.to_le_bytes())
                .collect()
        };
        d.entities
            .comps
            .insert((id, comp), bytes.into_boxed_slice());
    }
}

unsafe extern "C" fn reg_pose(r: *mut abi::Registrar, d: *const abi::PoseDesc) {
    unsafe {
        let d = &*d;
        let share = |v: f32| if v.is_finite() && v > 0.0 { v as f64 } else { 1.0 };
        let def = registry::PoseDef {
            name: d.name.as_str().to_string(),
            height: share(d.height),
            eye: share(d.eye),
            speed: d.speed.max(0.0),
            tilt: d.tilt.min(abi::pose_tilt::ON_BACK),
            action: d.action.as_str().to_string(),
            bob: d.bob != 0,
            priority: d.priority,
            key: d.key.as_str().to_string(),
            when: d.when,
            fallback_names: d.fallback.as_str().to_string(),
            fallback: Vec::new(),
        };
        reg(r).game.poses.push((d.cell as usize, def));
    }
}

unsafe extern "C" fn entity_set_pose(w: *mut abi::WorldCtx, id: u64, pose: u32, yaw: f32) {
    unsafe {
        let d = dim(w);
        let c = content();
        let comp = c.game.field_comps[10];
        let Some(e) = d.entities.get_mut(id) else { return };
        let held = (pose as usize) < c.game.poses.len();
        e.facing = (held && !yaw.is_nan()).then_some(yaw);
        let bytes: Vec<u8> = if held {
            e.pose = crate::world::Pose(pose as u16);
            if let Some(y) = e.facing {
                e.yaw = y;
            }
            [pose.to_le_bytes(), yaw.to_le_bytes()].concat()
        } else {
            Vec::new()
        };
        d.entities
            .comps
            .insert((id, comp), bytes.into_boxed_slice());
    }
}

unsafe extern "C" fn reg_game_mode(r: *mut abi::Registrar, d: *const abi::GameModeDesc) {
    unsafe {
        let d = &*d;
        let def = registry::GameModeDef {
            name: d.name.as_str().to_string(),
            flags: d.flags,
            modifiers: d
                .modifiers
                .as_slice()
                .iter()
                .map(|m| (m.attribute.as_str().to_string(), m.op, m.amount))
                .collect(),
        };
        reg(r).game.game_modes.push((d.cell as usize, def));
    }
}

unsafe extern "C" fn game_mode_flags(mode: u32) -> u32 {
    try_content()
        .and_then(|c| c.game.game_modes.get(mode as usize))
        .map_or(0, |m| m.flags)
}

unsafe extern "C" fn reg_text_place(r: *mut abi::Registrar, d: *const abi::TextPlaceDesc) {
    unsafe {
        let d = &*d;
        reg(r)
            .game
            .text_places
            .push((d.cell as usize, crate::hud::Place::from_abi(d)));
    }
}

unsafe extern "C" fn reg_entity_effect(r: *mut abi::Registrar, name: Str, cell: *const AtomicU32, shader: Str) {
    unsafe {
        let name = name.as_str().to_string();
        let shader = if shader.as_str().is_empty() { name.clone() } else { shader.as_str().to_string() };
        reg(r)
            .game
            .entity_effects
            .push((cell as usize, registry::EntityEffectDef { name, shader }));
    }
}

unsafe extern "C" fn reg_command_permission(r: *mut abi::Registrar, command: Str, level: u32) {
    unsafe {
        reg(r)
            .game
            .command_permissions
            .push((command.as_str().to_string(), level));
    }
}

unsafe extern "C" fn player_permission(w: *mut abi::WorldCtx, player: u64) -> u32 {
    unsafe { dim(w).entities.get(player).map_or(0, |e| e.permission) }
}

unsafe extern "C" fn set_player_permission(w: *mut abi::WorldCtx, player: u64, level: u32) {
    unsafe {
        dim(w)
            .events
            .push(crate::server::DimEvent::SetPermission { player, level: level.min(abi::permission::OWNER) });
    }
}

unsafe extern "C" fn reg_command_branch(r: *mut abi::Registrar, command: Str, usage: Str, client: u32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let ext = registry::CommandExtension::Branch {
            command: command.as_str().to_string(),
            usage: usage.as_str().to_string(),
            client: client != 0,
            cb,
        };
        let owner = r.owner;
        r.game.command_extensions.push((owner, ext));
    }
}

unsafe extern "C" fn reg_system(r: *mut abi::Registrar, name: Str, schema: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .systems
            .push((owner, name.as_str().to_string(), schema.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn reg_stack_component(r: *mut abi::Registrar, name: Str, schema: Str, flags: u32) {
    unsafe {
        reg(r)
            .game
            .stack_components
            .push((name.as_str().to_string(), schema.as_str().to_string(), flags))
    }
}

unsafe extern "C" fn reg_storage(r: *mut abi::Registrar, name: Str, slots: u32) {
    use abi::component_flags as cf;
    unsafe {
        let r = reg(r);
        let name = name.as_str().to_string();
        // One `ItemStack` per slot, empty ones all zero.
        let size = slots.min(1 << 12) * size_of::<ItemStack>() as u32;
        r.game.components.push(registry::RawComponent {
            owner: r.owner,
            def: registry::ComponentDef {
                name: name.clone(),
                size,
                flags: cf::SAVE | cf::KEEP_ON_RESPAWN | cf::STORAGE,
                default: vec![0; size as usize],
                runtime: true,
            },
            schema: STORAGE_SCHEMA.into(),
        });
        r.game
            .stack_components
            .push((name, STORAGE_SCHEMA.into(), 0));
    }
}

/// What storages say they hold, for mods that look one up by name.
pub const STORAGE_SCHEMA: &str = "storage";

unsafe extern "C" fn reg_client_hook(r: *mut abi::Registrar, hook: u32, cb: Callback) {
    unsafe {
        let r = reg(r);
        if hook > abi::client_hook::SCREEN {
            r.errors.push(format!("client hook {hook} does not exist"));
            return;
        }
        let owner = r.owner;
        r.game.client_hooks.push((owner, hook, cb));
    }
}

unsafe extern "C" fn reg_client_entity_frame(r: *mut abi::Registrar, kind: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .client_entity_watchers
            .push((owner, kind.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn reg_bone_model(r: *mut abi::Registrar, kind: Str, model: Str, bone: Str) {
    unsafe {
        let (kind, model, bone) = (kind.as_str().to_string(), model.as_str().to_string(), bone.as_str().to_string());
        reg(r).game.attached_models.push((kind, model, bone));
    }
}

unsafe extern "C" fn reg_bone_motion(r: *mut abi::Registrar, model: Str, bone: Str, gravity: f32, spring: f32, drag: f32) {
    unsafe {
        let d = crate::model::BoneMotion { gravity, spring, drag };
        let (model, bone) = (model.as_str().to_string(), bone.as_str().to_string());
        reg(r).game.bone_motions.push((model, bone, d));
    }
}

unsafe extern "C" fn reg_item_model_for_stack(r: *mut abi::Registrar, item: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .item_model_for_stack
            .push((owner, item.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn reg_client_entity_cosmetics(r: *mut abi::Registrar, kind: Str, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .client_entity_ticks
            .push((owner, kind.as_str().to_string(), cb));
    }
}

unsafe extern "C" fn reg_packet(r: *mut abi::Registrar, name: Str, schema: Str, side: u32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .packets
            .push((owner, name.as_str().to_string(), schema.as_str().to_string(), side.min(1), cb));
    }
}

unsafe extern "C" fn reg_mistake(r: *mut abi::Registrar, message: Str) {
    unsafe { reg(r).errors.push(message.as_str().to_string()) }
}

unsafe extern "C" fn reg_key(r: *mut abi::Registrar, name: Str, key: Str) {
    unsafe {
        let r = reg(r);
        if crate::input::Chord::parse(key.as_str()).is_none() {
            r.errors
                .push(format!("key {}: unknown key name {:?}", name.as_str(), key.as_str()));
        }
        r.game
            .keys
            .push((name.as_str().to_string(), key.as_str().to_string()))
    }
}

unsafe extern "C" fn set_block_entity(r: *mut abi::Registrar, block: Str, entity: Str, slots: u32) {
    unsafe {
        reg(r)
            .game
            .block_entities
            .push((block.as_str().to_string(), entity.as_str().to_string(), slots))
    }
}

unsafe extern "C" fn reg_condition(r: *mut abi::Registrar, name: Str, cell: *const AtomicU32, cb: Callback) {
    unsafe {
        let r = reg(r);
        let owner = r.owner;
        r.game
            .conditions
            .push((cell as usize, registry::ConditionDef { name: name.as_str().to_string(), answer: registry::Answer::Mod(owner, cb) }));
    }
}

unsafe extern "C" fn reg_particle(r: *mut abi::Registrar, d: *const abi::ParticleDesc) {
    unsafe {
        let d = *d;
        let textures = d
            .textures
            .as_slice()
            .iter()
            .map(|t| t.as_str().to_string())
            .collect();
        let rules = d
            .rules
            .as_slice()
            .iter()
            .map(|r| registry::RawParticleRule {
                event: r.event,
                when: r.when,
                tag: r.tag.as_str().to_string(),
                then: r.then,
                amount: r.amount,
                into: r.into.as_str().to_string(),
            })
            .collect();
        let p = registry::RawParticle {
            name: d.name.as_str().to_string(),
            cell: d.cell as usize,
            textures,
            shader: d.shader.as_str().to_string(),
            rules,
            desc: abi::ParticleDesc {
                name: Str::EMPTY,
                cell: std::ptr::null(),
                textures: abi::Slice::EMPTY,
                shader: Str::EMPTY,
                rules: abi::Slice::EMPTY,
                ..d
            },
        };
        reg(r).game.particles.push(p);
    }
}

unsafe extern "C" fn reg_sky(r: *mut abi::Registrar, d: *const abi::SkyDesc) {
    unsafe {
        let d = *d;
        let s = registry::RawSky {
            dimension: d.dimension.as_str().to_string(),
            name: d.name.as_str().to_string(),
            texture: d.texture.as_str().to_string(),
            shader: d.shader.as_str().to_string(),
            desc: abi::SkyDesc { dimension: Str::EMPTY, name: Str::EMPTY, texture: Str::EMPTY, shader: Str::EMPTY, ..d },
        };
        reg(r).game.sky.push(s);
    }
}

unsafe extern "C" fn particle_burst(w: *mut abi::WorldCtx, b: *const abi::ParticleBurst) {
    unsafe {
        let mut b = *b;
        if (b.kind as usize) < content().game.particles.len() {
            let d = dim(w);
            b.pos = d
                .place
                .to_world(crate::world::dvec(b.pos))
                .to_array()
                .into();
            d.events
                .push(crate::server::DimEvent::Particles { dim: d.id, burst: b });
        } else {
            misuse("particles", || format!("no particle {}", b.kind));
        }
    }
}

unsafe extern "C" fn signal_at(w: *mut abi::WorldCtx, pos: *const abi::BlockPos) -> u32 {
    unsafe {
        let d = dim(w);
        d.space
            .signals
            .power_at(content(), d.signal_view(), crate::world::ivec(*pos)) as u32
    }
}

unsafe extern "C" fn reg_config(r: *mut abi::Registrar, d: *const abi::ConfigDesc) {
    unsafe {
        let d = *d;
        let def = crate::config::ConfigDef {
            name: d.name.as_str().to_string(),
            kind: d.kind,
            default: d.default,
            min: d.min,
            max: d.max,
            step: d.step,
            choices: d
                .choices
                .as_slice()
                .iter()
                .map(|s| s.as_str().to_string())
                .collect(),
            flags: d.flags,
            scope: d.scope,
            text: d.text.as_str().to_string(),
        };
        if def.kind == abi::config_kind::TEXT && def.scope != abi::config_scope::CLIENT {
            reg(r)
                .errors
                .push(format!("setting {}: text settings are the client's own", def.name));
            return;
        }
        let def = crate::config::ConfigDef { default: def.fit(d.default), ..def };
        reg(r).game.configs.push((d.cell as usize, def));
    }
}

unsafe extern "C" fn config_value(id: u32) -> f64 {
    crate::config::get(id)
}

unsafe extern "C" fn config_set(id: u32, value: f64) {
    crate::config::set(id, value);
}

unsafe extern "C" fn config_text(id: u32) -> Str {
    thread_local! {
        static TEXT: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    TEXT.with(|t| {
        *t.borrow_mut() = crate::config::text(id);
        let t = t.borrow();
        Str { ptr: t.as_ptr(), len: t.len() }
    })
}

unsafe extern "C" fn config_set_text(id: u32, value: Str) {
    unsafe { crate::config::set_text(id, value.as_str()) }
}
