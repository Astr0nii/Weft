//! Conditions (`Host::reg_condition`): one predicate that music, `{if:..}` in text, loot tables
//! and natural spawning all take. The engine's own are answered here; a mod's is its callback,
//! asked with the side it runs on.

use glam::DVec3;
use weft_abi::{self as abi, condition as k};

use crate::content::Content;
use crate::server::Dim;

/// On the server, about `pos` and `entity` (0 for none).
pub fn on_server(d: &mut Dim, c: &'static Content, id: u32, pos: DVec3, entity: u64) -> bool {
    let Some(def) = c.game.conditions.get(id as usize) else { return false };
    let sky = || {
        let p = d.place.to_local(pos).floor().as_ivec3();
        abi::light::sky(d.light(p)) >= 6 || !c.dims[d.id as usize].sky_light
    };
    match (id, def.cb) {
        (k::DAY, None) => d.daylight(c) >= 0.5,
        (k::NIGHT, None) => d.daylight(c) < 0.5,
        (k::UNDERGROUND, None) => !sky(),
        (k::SURFACE, None) => sky(),
        (k::CREATIVE, None) => d
            .entities
            .get(entity)
            .is_some_and(|e| e.mode == crate::world::GameMode::CREATIVE),
        (_, Some((owner, cb))) => {
            d.call(owner, cb, |w| abi::ConditionArgs { world: w, client: std::ptr::null_mut(), pos: pos.to_array(), entity, result: 0 })
                .result
                != 0
        }
        _ => false,
    }
}

/// On a client, about its own player.
pub fn on_client(client: Option<&crate::client::Client>, id: u32) -> bool {
    let c = crate::content::content();
    let Some(def) = c.game.conditions.get(id as usize) else { return false };
    let enclosed = || client.is_some_and(|cl| cl.enclosed());
    match (id, def.cb) {
        (k::DAY, None) => client.is_some_and(|cl| cl.daylight() > 0.5),
        (k::NIGHT, None) => client.is_some_and(|cl| cl.daylight() <= 0.5),
        (k::UNDERGROUND, None) => enclosed(),
        (k::SURFACE, None) => client.is_some() && !enclosed(),
        (k::CREATIVE, None) => client.is_some_and(|cl| cl.mode == crate::world::GameMode::CREATIVE),
        (_, Some((owner, cb))) => {
            let mut v = crate::client::modui::ClientView::new(client);
            let pos = client.map_or([0.0; 3], |cl| cl.player.pos.to_array());
            let mut a = abi::ConditionArgs { world: std::ptr::null_mut(), client: v.raw(), pos, entity: 0, result: 0 };
            crate::host::invoke(owner, cb, &mut a);
            a.result != 0
        }
        _ => false,
    }
}
