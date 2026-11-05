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
