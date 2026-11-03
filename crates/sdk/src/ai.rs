//! Goal-driven mob behavior: goals in priority order, and each tick every entity runs the first
//! goal that wants to act. Movement goes through the engine's navigation (`World::navigate`),
//! so mobs walk around walls instead of into them.
//!
//! ```ignore
//! r.entity(&ZOMBIE).goals(Goals::new().then(attack).then(ai::chase_target(18.0, 1.0)).then(ai::wander(0.6, 10.0)));
//! ```
//!
//! A kind made `.neutral(..)` leaves players alone while its condition holds, and goes for one who
//! hit it for a while regardless. `melee` and `chase_target` go by that, as does `target`, for
//! goals of your own.

use std::sync::RwLock;

use crate::{Component, EntityKind, EntityState, EntityTick, World};

crate::value! {
    #[derive(Default)]
    /// Who a neutral mob goes for whatever its condition says, and until which tick.
    pub struct Anger { pub at: u64, pub until: u64 }
}

pub(crate) static ANGER: Component<Anger> = Component::named("anger");
/// How long a neutral mob stays after who hit it: 20 seconds.
const ANGER_TICKS: u64 = 400;

/// Is it calm towards this player? (world, the mob, the player)
type Calm = Box<dyn Fn(&World, &EntityState, &EntityState) -> bool + Send + Sync>;

/// The neutral kinds of this mod.
static NEUTRAL: RwLock<Vec<(&'static EntityKind, Calm)>> = RwLock::new(Vec::new());

/// Makes `kind` neutral; true if it is the first of this mod.
pub(crate) fn set_neutral(kind: &'static EntityKind, calm: Calm) -> bool {
    let mut all = NEUTRAL.write().unwrap();
    all.push((kind, calm));
    all.len() == 1
}

/// A neutral mob was hurt: by a player, it goes after them.
pub(crate) fn provoke_on_hurt(w: &World, h: &crate::Hurt) {
    if w.players_iter().any(|p| p == h.source) {
        if let Some(a) = w.component_mut(h.entity, &ANGER) {
            *a = Anger { at: h.source, until: w.game_time() + ANGER_TICKS };
        }
    }
}

/// One behavior: acts this tick and returns true, or returns false to let a lower one act.
pub trait Goal: Send + Sync + 'static {
    fn run(&self, w: &World, id: u64, st: &EntityState) -> bool;
    /// A goal above it acted this tick, so this one didn't get to run.
    fn passed_over(&self, _w: &World, _id: u64) {}
}

impl<F: Fn(&World, u64, &EntityState) -> bool + Send + Sync + 'static> Goal for F {
    fn run(&self, w: &World, id: u64, st: &EntityState) -> bool {
        self(w, id, st)
    }
}

#[derive(Default)]
pub struct Goals(Vec<Box<dyn Goal>>);

impl Goals {
    pub fn new() -> Goals {
        Goals(Vec::new())
    }
    /// Adds a goal below the ones added before.
    pub fn then(mut self, g: impl Goal) -> Goals {
        self.0.push(Box::new(g));
        self
    }
    /// Runs the goals for every entity of the tick.
    pub fn tick(&self, t: &EntityTick) {
        let w = t.world();
        for &id in t.ids() {
            let Some(st) = w.entity(id) else { continue };
            let mut goals = self.0.iter();
            for g in goals.by_ref() {
                if g.run(&w, id, &st) {
                    break;
                }
            }
            for g in goals {
                g.passed_over(&w, id);
            }
        }
    }
}

struct Playing<G> {
    from: &'static str,
    to: &'static str,
    goal: G,
}

impl<G: Goal> Goal for Playing<G> {
    fn run(&self, w: &World, id: u64, st: &EntityState) -> bool {
        let on = self.goal.run(w, id, st);
        if on {
            w.action_override(id, self.from, self.to);
        } else {
            w.action_override_clear(id, self.from);
        }
        on
    }

    fn passed_over(&self, w: &World, id: u64) {
        w.action_override_clear(id, self.from);
        self.goal.passed_over(w, id);
    }
}

/// `goal`, and while it is what the mob does, it plays action `to` wherever it would play `from`.
pub fn with_action_override(from: &'static str, to: &'static str, goal: impl Goal) -> impl Goal {
    Playing { from, to, goal }
}

/// Now and then walks somewhere within `radius` blocks at `speed`; idle in between.
pub fn wander(speed: f32, radius: f64) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        if w.navigating(id) {
            return true;
        }
        if w.random_below(100) != 0 {
            return false;
        }
        let a = w.random_below(628) as f64 / 100.0;
        let r = radius * (0.3 + w.random_below(70) as f64 / 100.0);
        w.navigate(id, [st.pos[0] + a.cos() * r, st.pos[1], st.pos[2] + a.sin() * r], speed);
        true
    }
}

/// The nearest living player within `range` and 8 blocks up or down, seen as if its
/// `visibility` attribute times nearer (0 in creative): its id and state.
pub fn nearest_player(w: &World, st: &EntityState, range: f64) -> Option<(u64, EntityState)> {
    nearest_player_by(w, st, &around(st, range), |_, _| true)
}

/// The usual zone a mob looks for players in: `range` around, 8 up and down.
fn around(st: &EntityState, range: f64) -> crate::Shape {
    crate::Shape::cylinder([st.pos[0], st.pos[1] - 8.0, st.pos[2]], range, 16.0, crate::Dir::Up)
}

/// The nearest living player in `area` that `wanted` accepts, by horizontal distance. A player
/// counts as if its `visibility` attribute times as close to the mob (0: never): a flyer can
/// look far down with a tall area, a mob of your own can have any condition.
pub fn nearest_player_by(
    w: &World,
    st: &EntityState,
    area: &crate::Shape,
    wanted: impl Fn(u64, &EntityState) -> bool,
) -> Option<(u64, EntityState)> {
    let d2 = |s: &EntityState| (s.pos[0] - st.pos[0]).powi(2) + (s.pos[2] - st.pos[2]).powi(2);
    w.players_iter()
        .filter_map(|p| w.entity(p).map(|s| (p, s)))
        .filter(|(p, s)| {
            let seen = w.attribute(*p, &crate::attribute::VISIBILITY);
            s.health > 0.0 && seen > 0.0 && area.contains([0, 1, 2].map(|a| st.pos[a] + (s.pos[a] - st.pos[a]) / seen)) && wanted(*p, s)
        })
        .min_by(|a, b| d2(&a.1).total_cmp(&d2(&b.1)))
}

/// Who this mob goes for within `range`: `nearest_player`, but for a neutral kind only one who hit
/// it lately or towards whom its condition doesn't hold.
pub fn find_target(w: &World, id: u64, st: &EntityState, range: f64) -> Option<(u64, EntityState)> {
    let neutral = NEUTRAL.read().unwrap();
    let Some((_, calm)) = neutral.iter().find(|(kind, _)| kind.id() == st.kind) else {
        return nearest_player(w, st, range);
    };
    let angry = w
        .component(id, &ANGER)
        .filter(|a| a.until > w.game_time())
        .map(|a| a.at);
    nearest_player_by(w, st, &around(st, range), |p, ps| Some(p) == angry || !calm(w, st, ps))
}

/// Looks at the nearest player within `range` while standing around (vanilla's
/// LookAtPlayerGoal). The body stays where it is until the head has turned too far. Never takes
/// over: what comes after it runs. Like everything aimed at players it goes by `visibility`, so
/// mobs don't look at someone in creative.
pub fn watch_player(range: f64) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        if !w.navigating(id) {
            if let Some((_, ps)) = nearest_player(w, st, range) {
                let (dx, dz) = (ps.pos[0] - st.pos[0], ps.pos[2] - st.pos[2]);
                let dy = ps.eye()[1] - st.eye()[1];
                let mut now = *st;
                now.yaw = crate::yaw_toward(st.pos, ps.pos);
                now.pitch = dy.atan2((dx * dx + dz * dz).sqrt()) as f32;
                w.set_entity(id, &now);
            }
        }
        false
    }
}

/// Hits its `target` when it is in reach (`World::in_reach`: the kind's `.reach(..)`, an
/// attribute modifiers change) with `World::attack`, which keeps the pace. Never takes over:
/// chasing goes on.
pub fn melee() -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        let reach = w.attribute(id, &crate::attribute::ENTITY_REACH);
        if let Some((p, _)) = find_target(w, id, st, reach + st.size[0] as f64 + 2.0) {
            if w.in_reach(id, p) {
                w.attack(id, p);
            }
        }
        false
    }
}

/// `melee` with a reach of its own instead of the attribute, from its eyes to the target's box.
pub fn melee_within(reach: f64) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| {
        if let Some((p, ps)) = find_target(w, id, st, reach + 2.0) {
            let eye = st.eye();
            let half = ps.size[0] as f64 / 2.0;
            let near = [
                eye[0].clamp(ps.pos[0] - half, ps.pos[0] + half),
                eye[1].clamp(ps.pos[1], ps.pos[1] + ps.size[1] as f64),
                eye[2].clamp(ps.pos[2] - half, ps.pos[2] + half),
            ];
            let d2: f64 = (0..3).map(|a| (near[a] - eye[a]).powi(2)).sum();
            if d2 <= reach * reach {
                w.attack(id, p);
            }
        }
        false
    }
}

/// Walks to its `target` within `range` at `speed`.
pub fn chase_target(range: f64, speed: f32) -> impl Goal {
    move |w: &World, id: u64, st: &EntityState| match find_target(w, id, st, range) {
        Some((_, p)) => {
            w.navigate(id, p.pos, speed);
            true
        }
        None => false,
    }
}
