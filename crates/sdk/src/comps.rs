//! The components registrations of each kind are made of, by kind: `comps::weather::DARKNESS`
//! is a weather's. The builders' methods set them; `r.edit(&X).set(&comps::..)` changes one of
//! another mod's, and the engine reads them when the game loads (absent: its default).

/// What a weather is made of: rain's look and flags where nothing is set.
pub mod weather {
    use crate::abi::comp::weather as w;
    use crate::kind::Weather;
    use crate::{Component, Text};
    pub static FALLS: Component<f32, Weather> = Component::named(w::FALLS);
    pub static SKY_GRAY: Component<f32, Weather> = Component::named(w::SKY_GRAY);
    pub static SKY_COVER: Component<f32, Weather> = Component::named(w::SKY_COVER);
    pub static FOG_GRAY: Component<f32, Weather> = Component::named(w::FOG_GRAY);
    pub static CLOUD_GRAY: Component<f32, Weather> = Component::named(w::CLOUD_GRAY);
    pub static CLOUD_DARK: Component<f32, Weather> = Component::named(w::CLOUD_DARK);
    pub static DARKNESS: Component<f32, Weather> = Component::named(w::DARKNESS);
    pub static SOUND: Component<Text, Weather> = Component::named(w::SOUND);
    pub static DURATION: Component<[u32; 2], Weather> = Component::named(w::DURATION);
    pub static WEIGHT: Component<u32, Weather> = Component::named(w::WEIGHT);
}

/// What a precipitation is made of.
pub mod precipitation {
    use crate::abi::comp::precipitation as p;
    use crate::kind::Precipitation;
    use crate::{Component, Text};
    pub static TEXTURE: Component<Text, Precipitation> = Component::named(p::TEXTURE);
    pub static SPEED: Component<f32, Precipitation> = Component::named(p::SPEED);
    pub static DENSITY: Component<f32, Precipitation> = Component::named(p::DENSITY);
}

/// What a pose is made of (shares of standing unless said).
pub mod pose {
    use crate::abi::comp::pose as p;
    use crate::kind::Pose;
    use crate::{Component, Text};
    pub static HEIGHT: Component<f32, Pose> = Component::named(p::HEIGHT);
    pub static EYE: Component<f32, Pose> = Component::named(p::EYE);
    pub static SPEED: Component<f32, Pose> = Component::named(p::SPEED);
    pub static TILT: Component<u32, Pose> = Component::named(p::TILT);
    pub static ACTION: Component<Text, Pose> = Component::named(p::ACTION);
    pub static PRIORITY: Component<i32, Pose> = Component::named(p::PRIORITY);
    pub static KEY: Component<Text, Pose> = Component::named(p::KEY);
    pub static FALLBACK: Component<Vec<String>, Pose> = Component::named(p::FALLBACK);
}

/// What a game mode is made of: a tag per ability (`abi::comp::game_mode::FLAGS`) and the
/// modifiers its players get.
pub mod game_mode {
    use crate::Component;
    use crate::kind::GameMode;
    pub static MODIFIERS: Component<Vec<crate::abi::ModeModifier>, GameMode> = Component::named(crate::abi::comp::game_mode::MODIFIERS);
}

/// What a damage cause is made of: tags for what it does (`abi::comp::damage_cause`) and its
/// death messages.
pub mod damage_cause {
    use crate::abi::comp::damage_cause as d;
    use crate::kind::DamageCause;
    use crate::{Component, Text};
    pub static DEATH_MESSAGE: Component<Text, DamageCause> = Component::named(d::DEATH_MESSAGE);
    pub static DEATH_MESSAGE_BY: Component<Text, DamageCause> = Component::named(d::DEATH_MESSAGE_BY);
}

/// What a text place is made of (`abi::comp::text_place`).
pub mod text_place {
    use crate::abi::comp::text_place as t;
    use crate::kind::TextPlace;
    use crate::{Component, Text};
    pub static ANCHOR: Component<Text, TextPlace> = Component::named(t::ANCHOR);
    pub static ORDER: Component<i32, TextPlace> = Component::named(t::ORDER);
    pub static AT: Component<[f32; 4], TextPlace> = Component::named(t::AT);
    pub static SCALE: Component<f32, TextPlace> = Component::named(t::SCALE);
    pub static TIMES: Component<[f32; 3], TextPlace> = Component::named(t::TIMES);
    pub static CLASS: Component<Text, TextPlace> = Component::named(t::CLASS);
    pub static SAMPLE: Component<Text, TextPlace> = Component::named(t::SAMPLE);
}

/// What an entity effect is made of.
pub mod entity_effect {
    use crate::kind::EntityEffect;
    use crate::{Component, Text};
    pub static SHADER: Component<Text, EntityEffect> = Component::named(crate::abi::comp::ENTITY_EFFECT_SHADER);
}

/// What a sound event is made of (`abi::comp::sound`).
pub mod sound {
    use crate::abi::comp::sound as s;
    use crate::kind::Sound;
    use crate::{Component, Text};
    pub static FILES: Component<Vec<String>, Sound> = Component::named(s::FILES);
    pub static VOLUME: Component<f32, Sound> = Component::named(s::VOLUME);
    pub static PITCH: Component<[f32; 2], Sound> = Component::named(s::PITCH);
    pub static RANGE: Component<f32, Sound> = Component::named(s::RANGE);
    pub static FULL_WITHIN: Component<f32, Sound> = Component::named(s::FULL_WITHIN);
    pub static FALLOFF: Component<i32, Sound> = Component::named(s::FALLOFF);
    pub static CATEGORY: Component<Text, Sound> = Component::named(s::CATEGORY);
}

/// What background music is made of.
pub mod music {
    use crate::abi::comp::music as m;
    use crate::kind::Music;
    use crate::{Component, Text};
    pub static TRACKS: Component<Vec<String>, Music> = Component::named(m::TRACKS);
    pub static DIMENSION: Component<Text, Music> = Component::named(m::DIMENSION);
    pub static WHEN: Component<Vec<String>, Music> = Component::named(m::WHEN);
}

/// What a camera mode is made of: a tag per flag (`abi::comp::camera::FLAGS`) and these.
pub mod camera {
    use crate::abi::comp::camera as c;
    use crate::kind::CameraMode;
    use crate::{Component, Text};
    pub static KEY: Component<Text, CameraMode> = Component::named(c::KEY);
    pub static CALL: Component<crate::abi::Callback, CameraMode> = Component::named(c::CALL);
    pub static TRANSITION: Component<f32, CameraMode> = Component::named(c::TRANSITION);
    pub static EASING: Component<u32, CameraMode> = Component::named(c::EASING);
}

/// What a setting is made of (`abi::comp::config`).
pub mod config {
    use crate::abi::comp::config as c;
    use crate::kind::Config;
    use crate::{Component, Text};
    pub static KIND: Component<u32, Config> = Component::named(c::KIND);
    pub static DEFAULT: Component<f64, Config> = Component::named(c::DEFAULT);
    pub static RANGE: Component<[f64; 2], Config> = Component::named(c::RANGE);
    pub static STEP: Component<f64, Config> = Component::named(c::STEP);
    pub static CHOICES: Component<Vec<String>, Config> = Component::named(c::CHOICES);
    pub static SCOPE: Component<u32, Config> = Component::named(c::SCOPE);
    pub static TEXT: Component<Text, Config> = Component::named(c::TEXT);
}

/// What an equipment slot is made of.
pub mod equipment_slot {
    use crate::abi::comp::equipment_slot as e;
    use crate::kind::EquipmentSlot;
    use crate::{Component, Text};
    pub static COVERS: Component<Vec<String>, EquipmentSlot> = Component::named(e::COVERS);
    pub static BACKGROUND: Component<Text, EquipmentSlot> = Component::named(e::BACKGROUND);
}

/// What an entity category is made of.
pub mod entity_category {
    use crate::Component;
    use crate::abi::comp::entity_category as c;
    use crate::kind::EntityCategory;
    pub static CAP: Component<u32, EntityCategory> = Component::named(c::CAP);
    pub static DESPAWN: Component<f32, EntityCategory> = Component::named(c::DESPAWN);
    pub static CAP_RADIUS: Component<f32, EntityCategory> = Component::named(c::CAP_RADIUS);
}

/// What a recipe kind is made of (`abi::comp::recipe_kind`).
pub mod recipe_kind {
    use crate::abi::comp::recipe_kind as r;
    use crate::kind::RecipeKind;
    use crate::{Component, Text};
    pub static INPUTS: Component<[u32; 2], RecipeKind> = Component::named(r::INPUTS);
    pub static OUTPUTS: Component<[u32; 2], RecipeKind> = Component::named(r::OUTPUTS);
    pub static INFO: Component<Text, RecipeKind> = Component::named(r::INFO);
    pub static INFO_VALUES: Component<Vec<String>, RecipeKind> = Component::named(r::INFO_VALUES);
    pub static LAYOUT: Component<crate::abi::Callback, RecipeKind> = Component::named(r::LAYOUT);
}

/// What an attribute is made of.
pub mod attribute {
    use crate::Component;
    use crate::kind::Attribute;
    pub static DEFAULT: Component<f64, Attribute> = Component::named(crate::abi::comp::attribute::DEFAULT);
    pub static RANGE: Component<[f64; 2], Attribute> = Component::named(crate::abi::comp::attribute::RANGE);
}

/// A creative tab's icon.
pub mod creative_tab {
    use crate::kind::CreativeTab;
    use crate::{Component, Text};
    pub static ICON: Component<Text, CreativeTab> = Component::named(crate::abi::comp::CREATIVE_TAB_ICON);
}

/// A tint kind's color where no biome gives one.
pub mod tint_kind {
    use crate::Component;
    use crate::kind::TintKind;
    pub static DEFAULT: Component<u32, TintKind> = Component::named(crate::abi::comp::TINT_KIND_DEFAULT);
}

/// A key's default binding.
pub mod key {
    use crate::kind::Key;
    use crate::{Component, Text};
    pub static DEFAULT: Component<Text, Key> = Component::named(crate::abi::comp::KEY_DEFAULT);
}
