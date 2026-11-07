//! What the engine reads of some tags all the time, as bits made once when content is frozen:
//! `TAGS` says which tag sets each. Mods only see the tags (`abi::comp`).

pub mod screen {
    pub const PAUSES: u32 = 1;
    pub const KEEP_ON_ESCAPE: u32 = 2;
    pub const CLOSES_ON_RESPAWN: u32 = 4;
    pub const KEYS_FIRST: u32 = 8;
    pub const SHOWS_CHAT: u32 = 16;
    pub const TAGS: [(&str, u32); 5] = [
        (weft_abi::comp::menu::PAUSES, PAUSES),
        (weft_abi::comp::menu::KEEP_ON_ESCAPE, KEEP_ON_ESCAPE),
        (weft_abi::comp::menu::CLOSES_ON_RESPAWN, CLOSES_ON_RESPAWN),
        (weft_abi::comp::menu::KEYS_FIRST, KEYS_FIRST),
        (weft_abi::comp::menu::SHOWS_CHAT, SHOWS_CHAT),
    ];
}

pub mod camera {
    pub const BODY: u32 = 1;
    pub const HAND: u32 = 2;
    pub const HUD: u32 = 4;
    pub const EFFECTS: u32 = 8;
    pub const CLIP: u32 = 16;
    pub const SOLID: u32 = 32;
    pub const CYCLE: u32 = 64;
    pub const DETACHED: u32 = 128;
    pub const AIM: u32 = 256;
    pub const NO_INTERACT: u32 = 512;
    pub const ASK: u32 = 1024;
    pub const TAGS: [(&str, u32); 11] = [
        (weft_abi::comp::camera::BODY, BODY),
        (weft_abi::comp::camera::HAND, HAND),
        (weft_abi::comp::camera::HUD, HUD),
        (weft_abi::comp::camera::EFFECTS, EFFECTS),
        (weft_abi::comp::camera::CLIP, CLIP),
        (weft_abi::comp::camera::SOLID, SOLID),
        (weft_abi::comp::camera::CYCLE, CYCLE),
        (weft_abi::comp::camera::DETACHED, DETACHED),
        (weft_abi::comp::camera::AIM, AIM),
        (weft_abi::comp::camera::NO_INTERACT, NO_INTERACT),
        (weft_abi::comp::camera::ASK, ASK),
    ];
}

pub mod damage {
    pub const BYPASSES_ARMOR: u32 = 1;
    pub const PIERCES_INVULNERABLE: u32 = 2;
    pub const IGNORES_COOLDOWN: u32 = 4;
    pub const BURNING: u32 = 8;
    pub const NO_KNOCKBACK: u32 = 16;
    pub const SILENT: u32 = 32;
    pub const TAGS: [(&str, u32); 6] = [
        (weft_abi::comp::damage_cause::BYPASSES_ARMOR, BYPASSES_ARMOR),
        (weft_abi::comp::damage_cause::PIERCES_INVULNERABLE, PIERCES_INVULNERABLE),
        (weft_abi::comp::damage_cause::IGNORES_COOLDOWN, IGNORES_COOLDOWN),
        (weft_abi::comp::damage_cause::BURNING, BURNING),
        (weft_abi::comp::damage_cause::NO_KNOCKBACK, NO_KNOCKBACK),
        (weft_abi::comp::damage_cause::SILENT, SILENT),
    ];
}

pub mod game_mode {
    pub const INSTANT_BREAK: u32 = 1;
    pub const NO_BREAK: u32 = 2;
    pub const NO_PLACE: u32 = 4;
    pub const NO_COSTS: u32 = 8;
    pub const INVULNERABLE: u32 = 16;
    pub const CAN_FLY: u32 = 32;
    pub const ALWAYS_FLY: u32 = 64;
    pub const NOCLIP: u32 = 128;
    pub const NO_INTERACT: u32 = 256;
    pub const HIDDEN: u32 = 512;
    pub const NO_HUD: u32 = 1024;
    pub const CREATIVE_INVENTORY: u32 = 2048;
    pub const TAGS: [(&str, u32); 12] = [
        (weft_abi::comp::game_mode::INSTANT_BREAK, INSTANT_BREAK),
        (weft_abi::comp::game_mode::NO_BREAK, NO_BREAK),
        (weft_abi::comp::game_mode::NO_PLACE, NO_PLACE),
        (weft_abi::comp::game_mode::NO_COSTS, NO_COSTS),
        (weft_abi::comp::game_mode::INVULNERABLE, INVULNERABLE),
        (weft_abi::comp::game_mode::CAN_FLY, CAN_FLY),
        (weft_abi::comp::game_mode::ALWAYS_FLY, ALWAYS_FLY),
        (weft_abi::comp::game_mode::NOCLIP, NOCLIP),
        (weft_abi::comp::game_mode::NO_INTERACT, NO_INTERACT),
        (weft_abi::comp::game_mode::HIDDEN, HIDDEN),
        (weft_abi::comp::game_mode::NO_HUD, NO_HUD),
        (weft_abi::comp::game_mode::CREATIVE_INVENTORY, CREATIVE_INVENTORY),
    ];
}

pub mod particle {
    pub const COLLIDE: u32 = 1;
    pub const FULLBRIGHT: u32 = 2;
    pub const TRANSLUCENT: u32 = 4;
    pub const SHRINK: u32 = 8;
    pub const TAGS: [(&str, u32); 4] = [
        (weft_abi::comp::particle::COLLIDE, COLLIDE),
        (weft_abi::comp::particle::FULLBRIGHT, FULLBRIGHT),
        (weft_abi::comp::particle::TRANSLUCENT, TRANSLUCENT),
        (weft_abi::comp::particle::SHRINK, SHRINK),
    ];
}

pub mod config {
    pub const RESTART_REQUIRED: u32 = 2;
    pub const TAGS: [(&str, u32); 1] = [(weft_abi::comp::config::RESTART_REQUIRED, RESTART_REQUIRED)];
}

/// Weather is read the other way round: what a weather doesn't say, rain does.
pub mod weather {
    pub const FALLS: u32 = 1;
    pub const LIGHTNING: u32 = 2;
    pub const SHELTERS: u32 = 4;
}

pub mod entity {
    pub const FALL_DAMAGE: u32 = 1;
    pub const PUSHABLE: u32 = 2;
    pub const NO_GRAVITY: u32 = 4;
    pub const INVULNERABLE: u32 = 8;
    pub const BURNS_IN_SUN: u32 = 16;
    pub const SWIMS: u32 = 32;
    pub const SOLID: u32 = 64;
    pub const KINEMATIC: u32 = 128;
    pub const CRUSHES: u32 = 256;
    pub const RIDERS_FALL: u32 = 512;
    pub const WEARS: u32 = 1024;
    pub const OWN_EFFECTS: u32 = 2048;
    pub const BLOCKED: u32 = weft_abi::entity_state::BLOCKED;
    /// The tags taking away what every kind has.
    pub const OFF: [(&str, u32); 2] =
        [(weft_abi::comp::entity::NO_FALL_DAMAGE, FALL_DAMAGE), (weft_abi::comp::entity::UNPUSHABLE, PUSHABLE)];
    pub const TAGS: [(&str, u32); 10] = [
        (weft_abi::comp::entity::NO_GRAVITY, NO_GRAVITY),
        (weft_abi::comp::entity::INVULNERABLE, INVULNERABLE),
        (weft_abi::comp::entity::BURNS_IN_SUN, BURNS_IN_SUN),
        (weft_abi::comp::entity::SWIMS, SWIMS),
        (weft_abi::comp::entity::SOLID, SOLID),
        (weft_abi::comp::entity::KINEMATIC, KINEMATIC),
        (weft_abi::comp::entity::CRUSHES, CRUSHES),
        (weft_abi::comp::entity::RIDERS_FALL, RIDERS_FALL),
        (weft_abi::comp::entity::WEARS, WEARS),
        (weft_abi::comp::entity::OWN_EFFECTS, OWN_EFFECTS),
    ];
}

pub mod block {
    use weft_abi::block_flags as f;
    pub const TAGS: [(&str, u32); 9] = [
        (weft_abi::comp::block::REPLACEABLE, f::REPLACEABLE),
        (weft_abi::comp::block::CLIMBABLE, f::CLIMBABLE),
        (weft_abi::comp::block::NOT_TARGETABLE, f::NOT_TARGETABLE),
        (weft_abi::comp::block::CULL_SAME, f::CULL_SAME),
        (weft_abi::comp::block::IMMOVABLE, f::IMMOVABLE),
        (weft_abi::comp::block::SIGNAL_BLOCKER, f::SIGNAL_BLOCKER),
        (weft_abi::comp::block::STICKY, f::STICKY),
        (weft_abi::comp::block::NOT_STOOD_ON, f::NOT_STOOD_ON),
        (weft_abi::comp::block::HOLDS_LIQUID, f::HOLDS_LIQUID),
    ];
}
