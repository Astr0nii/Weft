//! Mobs: zombies and spiders at night or in the dark, pigs and cows on grass by day.

use weft_sdk::ai;
use weft_sdk::*;

value! {
    #[derive(Default)]
    /// 1 while it runs about after being hurt.
    pub struct Brain { pub panicking: u32 }
}

pub static BRAIN: Component<Brain> = Component::named("base:brain");
/// Three seconds after being hurt an animal calms down.
static CALM: Event<u64> = Event::named("calm");
pub static ZOMBIE: Entity = Entity::named("zombie");
pub static SPIDER: Entity = Entity::named("spider");
pub static PIG: Entity = Entity::named("pig");
pub static CREEPER: Entity = Entity::named("creeper");
pub static SKELETON: Entity = Entity::named("skeleton");
pub static SHEEP: Entity = Entity::named("sheep");
pub static CHICKEN: Entity = Entity::named("chicken");
pub static ARROW: Entity = Entity::named("arrow");
static EGG: Item = Item::named("egg");
/// Ticks of whatever a mob counts: a creeper's fuse, a skeleton's reload, a chicken's next egg.
static FUSE: Component<u32> = Component::named("fuse");
static SHOOTER: Component<u64> = Component::named("shooter");

/// A part of a model written as vanilla's are, y down from 24, so their textures fit.
fn vanilla_part(name: &str, pivot: [f32; 3], boxes: &[([f32; 3], [f32; 3], [f32; 2])]) -> Part {
    let mut part = Part::new(name, [pivot[0], 24.0 - pivot[1], pivot[2]]);
    for &(at, size, uv) in boxes {
        part = part.cube([at[0], -(at[1] + size[1]), at[2]], size, uv);
    }
    part
}

/// Legs as vanilla's, hanging from `y` (y down), back ones at z 7, front ones at z -5.
fn vanilla_legs(x: f32, y: f32) -> [Part; 4] {
    let height = 24.0 - y;
    let leg = |name, px, pz| vanilla_part(name, [px, y, pz], &[([-2.0, 0.0, -2.0], [4.0, height, 4.0], [0.0, 16.0])]);
    [leg("leg0", -x, 7.0), leg("leg1", x, 7.0), leg("leg2", x, -5.0), leg("leg3", -x, -5.0)]
}

/// Vanilla's creeper (`CreeperModel`): 64 x 32.
fn creeper_model() -> Model {
    let leg = |name, x, z| vanilla_part(name, [x, 18.0, z], &[([-2.0, 0.0, -2.0], [4.0, 6.0, 4.0], [0.0, 16.0])]);
    Model::from(vec![
        vanilla_part("head", [0.0, 6.0, 0.0], &[([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
        vanilla_part("body", [0.0, 6.0, 0.0], &[([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0])]),
        leg("leg0", -2.0, 4.0),
        leg("leg1", 2.0, 4.0),
        leg("leg2", 2.0, -4.0),
        leg("leg3", -2.0, -4.0),
    ])
    .uv(64, 32)
}

/// Vanilla's skeleton (`SkeletonModel`): a thin biped, arms held out like a zombie's while aiming.
fn skeleton_model() -> Model {
    let forward = 90.0;
    Model::from(vec![
        vanilla_part("head", [0.0, 0.0, 0.0], &[([-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], [0.0, 0.0])]),
        vanilla_part("body", [0.0, 0.0, 0.0], &[([-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], [16.0, 16.0])]),
        vanilla_part("rightArm", [-5.0, 2.0, 0.0], &[([-1.0, -2.0, -1.0], [2.0, 12.0, 2.0], [40.0, 16.0])])
            .mirrored()
            .rotation(forward, 0.0, 0.0),
        vanilla_part("leftArm", [5.0, 2.0, 0.0], &[([-1.0, -2.0, -1.0], [2.0, 12.0, 2.0], [40.0, 16.0])]).rotation(forward, 0.0, 0.0),
        vanilla_part("rightLeg", [-2.0, 12.0, 0.0], &[([-1.0, 0.0, -1.0], [2.0, 12.0, 2.0], [0.0, 16.0])]).mirrored(),
        vanilla_part("leftLeg", [2.0, 12.0, 0.0], &[([-1.0, 0.0, -1.0], [2.0, 12.0, 2.0], [0.0, 16.0])]),
    ])
    .uv(64, 32)
}

/// Vanilla's sheep (`SheepModel` and `SheepFurModel`) in one texture: the sheep in the upper half,
/// its wool in the lower, on boxes grown over it.
fn sheep_model() -> Model {
    let wool = |p: [f32; 2]| [p[0], p[1] + 32.0];
    let head = vanilla_part("head", [0.0, 6.0, -8.0], &[([-3.0, -4.0, -6.0], [6.0, 6.0, 8.0], [0.0, 0.0])])
        .cube([-3.0, -2.0, -4.0], [6.0, 6.0, 6.0], wool([0.0, 0.0]))
        .inflated(0.6);
    let body = vanilla_part("body", [0.0, 5.0, 2.0], &[([-4.0, -10.0, -7.0], [8.0, 16.0, 6.0], [28.0, 8.0])])
        .cube([-4.0, -6.0, -7.0], [8.0, 16.0, 6.0], wool([28.0, 8.0]))
        .inflated(1.75)
        .rotation(-90.0, 0.0, 0.0);
    let mut parts = vec![head, body];
    parts.extend(vanilla_legs(3.0, 12.0).map(|l| {
        l.cube([-2.0, -6.0, -2.0], [4.0, 6.0, 4.0], wool([0.0, 16.0]))
            .inflated(0.5)
    }));
    Model::from(parts).uv(64, 64)
}

/// Vanilla's chicken: 64 x 32.
fn chicken_model() -> Model {
    let leg = |name, x| vanilla_part(name, [x, 19.0, 1.0], &[([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [26.0, 0.0])]);
    Model::from(vec![
        vanilla_part(
            "head",
            [0.0, 15.0, -4.0],
            &[
                ([-2.0, -6.0, -2.0], [4.0, 6.0, 3.0], [0.0, 0.0]),
                ([-2.0, -4.0, -4.0], [4.0, 2.0, 2.0], [14.0, 0.0]),
                ([-1.0, -2.0, -3.0], [2.0, 2.0, 2.0], [14.0, 4.0]),
            ],
        ),
        vanilla_part("body", [0.0, 16.0, 0.0], &[([-3.0, -4.0, -3.0], [6.0, 8.0, 6.0], [0.0, 9.0])]).rotation(-90.0, 0.0, 0.0),
        vanilla_part("wing0", [-4.0, 13.0, 0.0], &[([0.0, 0.0, -3.0], [1.0, 4.0, 6.0], [24.0, 13.0])]),
        vanilla_part("wing1", [4.0, 13.0, 0.0], &[([-1.0, 0.0, -3.0], [1.0, 4.0, 6.0], [24.0, 13.0])]),
        leg("rightLeg", -2.0),
        leg("leftLeg", 1.0),
    ])
    .uv(64, 32)
}

/// A shot arrow: a thin stick.
fn arrow_model() -> Model {
    Model::from(vec![Part::new("body", [0.0, 2.0, 0.0]).cube([-0.5, -0.5, -8.0], [1.0, 1.0, 16.0], [0.0, 0.0])]).uv(32, 32)
}

/// Vanilla's cow (`CowModel`): head with muzzle and horns, body lying along z with its udder.
fn cow_model() -> Model {
    let head = vanilla_part(
        "head",
        [0.0, 4.0, -8.0],
        &[
            ([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0.0, 0.0]),
            ([-3.0, 1.0, -7.0], [6.0, 3.0, 1.0], [1.0, 33.0]),
            ([-5.0, -5.0, -5.0], [1.0, 3.0, 1.0], [22.0, 0.0]),
            ([4.0, -5.0, -5.0], [1.0, 3.0, 1.0], [22.0, 0.0]),
        ],
    );
    let body = vanilla_part(
        "body",
        [0.0, 5.0, 2.0],
        &[([-6.0, -10.0, -7.0], [12.0, 18.0, 10.0], [18.0, 4.0]), ([-2.0, 2.0, -8.0], [4.0, 6.0, 1.0], [52.0, 0.0])],
    )
    .rotation(-90.0, 0.0, 0.0);
    let mut parts = vec![head, body];
    parts.extend(vanilla_legs(4.0, 12.0));
    Model::from(parts).uv(64, 64)
}

/// Vanilla's pig (`PigModel`).
fn pig_model() -> Model {
    let head = vanilla_part(
        "head",
        [0.0, 12.0, -6.0],
        &[([-4.0, -4.0, -8.0], [8.0, 8.0, 8.0], [0.0, 0.0]), ([-2.0, 0.0, -9.0], [4.0, 3.0, 1.0], [16.0, 16.0])],
    );
    let body = vanilla_part("body", [0.0, 11.0, 2.0], &[([-5.0, -10.0, -7.0], [10.0, 16.0, 8.0], [28.0, 8.0])]).rotation(-90.0, 0.0, 0.0);
    let mut parts = vec![head, body];
    parts.extend(vanilla_legs(3.0, 18.0));
    Model::from(parts).uv(64, 64)
}
pub static COW: Entity = Entity::named("cow");

pub fn register(r: &mut Registry) {
    // The engine's player kind, drawn as the same humanoid as the zombie.
    r.set_model(&PLAYER, biped(false));
    r.entity_component(&BRAIN, Brain::default(), 0);
    r.on(&CALM, |b| {
        let w = b.world();
        for &id in b.iter() {
            if let Some(brain) = w.component_mut(id, &BRAIN) {
                brain.panicking = 0;
            }
        }
    });
    r.entity(&ZOMBIE)
        .tag("hostile")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.6, 1.95)
        .health(20.0)
        .speed(2.6)
        .hostile()
        .flags(entity_flags::BURNS_IN_SUN)
        .model(biped(true))
        .attach(&BRAIN)
        .attack_damage(3.0)
        .goals(hostile(1.6));
    r.entity(&SPIDER)
        .tag("hostile")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(1.3, 0.9)
        .health(16.0)
        .speed(3.6)
        .hostile()
        .model(spider())
        .attach(&BRAIN)
        .attack_damage(2.0)
        // Vanilla's reach goes by a mob's width: a body this wide stops further off.
        .goals(hostile(2.7))
        .neutral(in_daylight);
    r.entity_component(&FUSE, 0u32, 0);
    r.entity_component(&SHOOTER, 0u64, 0);
    r.entity(&CREEPER)
        .tag("hostile")
        .size(0.6, 1.7)
        .health(20.0)
        .speed(2.5)
        .hostile()
        .model(creeper_model())
        .attach(&BRAIN)
        .attach(&FUSE)
        .goals(creeper());
    r.entity(&SKELETON)
        .tag("hostile")
        .size(0.6, 1.99)
        .health(20.0)
        .speed(2.5)
        .hostile()
        .flags(entity_flags::BURNS_IN_SUN)
        .model(skeleton_model())
        .attach(&BRAIN)
        .attach(&FUSE)
        .goals(skeleton());
    r.entity(&ARROW)
        .no_egg()
        .size(0.5, 0.5)
        .unpushable()
        .health(1.0)
        .speed(0.0)
        .model(arrow_model())
        .attach(&SHOOTER)
        .goals(ai::Goals::new().then(arrow_flies));
    r.entity(&SHEEP)
        .tag("livestock")
        .size(0.9, 1.3)
        .health(8.0)
        .speed(2.3)
        .passive()
        .model(sheep_model())
        .attach(&BRAIN)
        .goals(passive())
        .on_hurt(panic);
    r.entity(&CHICKEN)
        .tag("livestock")
        .size(0.4, 0.7)
        .health(4.0)
        .speed(2.5)
        .passive()
        .model(chicken_model())
        .attach(&BRAIN)
        .attach(&FUSE)
        .goals(passive_chicken())
        .on_hurt(panic);
    r.entity(&PIG)
        .tag("livestock")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.9, 0.9)
        .health(10.0)
        .speed(2.5)
        .passive()
        .model(pig_model())
        .attach(&BRAIN)
        .goals(passive())
        .on_hurt(panic);
    r.entity(&COW)
        .tag("livestock")
        .sound("ambient")
        .sound("hurt")
        .sound("death")
        .size(0.9, 1.3)
        .health(10.0)
        .speed(2.2)
        .passive()
        .model(cow_model())
        .attach(&BRAIN)
        .goals(passive())
        .on_hurt(panic);
    // Vanilla amounts; meat comes out cooked when the animal burns.
    r.loot(&ZOMBIE)
        .pool(Pool::new().items("rotten_flesh", 0, 2));
    r.loot(&SPIDER)
        .pool(Pool::new().items("string", 0, 2))
        .pool(
            Pool::new()
                .when(Cond::KilledByPlayer)
                .when(Cond::Chance(0.33))
                .item("spider_eye"),
        );
    r.loot(&CREEPER).pool(Pool::new().items("gunpowder", 0, 2));
    r.loot(&SKELETON)
        .pool(Pool::new().items("arrow", 0, 2))
        .pool(Pool::new().items("bone", 0, 2));
    r.loot(&SHEEP).pool(Pool::new().item("wool")).pool(
        Pool::new().entry(
            Entry::item("mutton")
                .count(1, 2)
                .smelted(&crate::recipes::SMELTING),
        ),
    );
    r.loot(&CHICKEN)
        .pool(Pool::new().items("feather", 0, 2))
        .pool(
            Pool::new().entry(
                Entry::item("chicken")
                    .count(1, 1)
                    .smelted(&crate::recipes::SMELTING),
            ),
        );
    r.loot(&PIG).pool(
        Pool::new().entry(
            Entry::item("porkchop")
                .count(1, 3)
                .smelted(&crate::recipes::SMELTING),
        ),
    );
    r.loot(&COW)
        .pool(
            Pool::new().entry(
                Entry::item("beef")
                    .count(1, 3)
                    .smelted(&crate::recipes::SMELTING),
            ),
        )
        .pool(Pool::new().items("leather", 0, 2));
    // Natural spawning, as rules the engine runs: monsters in the dark (spiders more in deep
    // caves), animals in small herds on grass by day, rarely once the world is loaded.
    r.spawn(&ZOMBIE).light(0, 6).weight(2);
    r.spawn(&CREEPER).light(0, 6).weight(2);
    r.spawn(&SKELETON).light(0, 6).weight(2);
    r.spawn(&SPIDER).light(0, 6);
    r.spawn(&SPIDER).light(0, 6).biome("#cave").weight(4);
    for animal in [&PIG, &COW, &SHEEP, &CHICKEN] {
        r.spawn(animal)
            .biome("#animals")
            .ground("#animal_spawn")
            .light(7, 15)
            .group(2, 3)
            .chance(0.05);
    }
}

/// Vanilla's creeper: comes for the player, and within 3 blocks starts its 30 tick fuse, which
/// runs back down once the player is 7 away. A radius 3 blast at the end.
fn creeper() -> ai::Goals {
    ai::Goals::new()
        .then(|w: &World, id: u64, st: &EntityState| {
            let near = ai::nearest_player(w, st, 7.0);
            let d2 = near
                .as_ref()
                .map_or(f64::MAX, |(_, p)| (0..3).map(|i| (p.pos[i] - st.pos[i]).powi(2)).sum());
            let Some(fuse) = w.component_mut(id, &FUSE) else { return false };
            if d2 < 9.0 || (*fuse > 0 && d2 < 49.0) {
                *fuse += 1;
            } else {
                *fuse = fuse.saturating_sub(1);
            }
            let lit = *fuse;
            // Faster as it nears the end.
            if lit > 0 && lit % (if lit > 20 { 3 } else { 8 }) == 0 {
                w.flash(id);
            }
            if lit >= 30 {
                blast(w, id, st.pos, 3.0);
                return true;
            }
            lit > 0
        })
        .then(ai::watch_player(8.0))
        .then(ai::follow_player(16.0, 1.0))
        .then(ai::wander(0.6, 10.0))
}

/// Vanilla's explosion in short: a hole, and damage falling off to nothing at twice the radius.
fn blast(w: &World, id: u64, at: [f64; 3], radius: f64) {
    for other in w.near(at, radius * 2.0, None) {
        let Some(s) = w.entity(other).filter(|_| other != id) else { continue };
        let d = (0..3)
            .map(|i| (s.pos[i] - at[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        let impact = (1.0 - d / (radius * 2.0)).max(0.0);
        let damage = (impact * impact + impact) / 2.0 * 7.0 * radius * 2.0 + 1.0;
        w.damage(other, damage as f32, id, damage::EXPLOSION);
    }
    // Vanilla's: one going off in a liquid breaks nothing, and liquids don't break at all.
    if w.flags(block_at(at)) & abi::block_flags::LIQUID == 0 {
        w.fill_sphere(at, radius, 0)
            .keep_unbreakable()
            .keep_liquids();
    }
    w.spawn_particles(&Particle::named("core:poof"), at)
        .count(40)
        .spread(radius as f32 * 0.5)
        .speed(2.0);
    w.remove(id);
}

/// Vanilla's skeleton: keeps to about 10 blocks and shoots once in two seconds.
fn skeleton() -> ai::Goals {
    ai::Goals::new()
        .then(ai::watch_player(16.0))
        .then(|w: &World, id: u64, st: &EntityState| {
            let Some((_, p)) = ai::nearest_player(w, st, 15.0) else { return false };
            let Some(reload) = w.component_mut(id, &FUSE) else { return false };
            *reload = (*reload + 1) % 40;
            if *reload == 0 {
                shoot(w, id, st, &p);
            }
            let d2 = (p.pos[0] - st.pos[0]).powi(2) + (p.pos[2] - st.pos[2]).powi(2);
            if d2 > 100.0 {
                w.navigate(id, p.pos, 1.0);
            }
            true
        })
        .then(ai::follow_player(16.0, 1.0))
        .then(ai::wander(0.6, 10.0))
}

fn shoot(w: &World, id: u64, from: &EntityState, at: &EntityState) {
    let eye = from.eye()[1];
    let d = [at.pos[0] - from.pos[0], at.pos[1] + at.size[2] as f64 * 0.7 - eye, at.pos[2] - from.pos[2]];
    let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    let len = (flat * flat + d[1] * d[1]).sqrt().max(0.1);
    // A little above the target for the drop on the way.
    const SPEED: f64 = 32.0;
    let arrow = w.spawn(&ARROW, [from.pos[0], eye, from.pos[2]]);
    if let Some(shooter) = w.component_mut(arrow, &SHOOTER) {
        *shooter = id;
    }
    if let Some(mut s) = w.entity(arrow) {
        s.vel = [d[0] / len * SPEED, d[1] / len * SPEED + flat * 0.4, d[2] / len * SPEED];
        s.yaw = yaw_toward([0.0; 3], d);
        w.set_entity(arrow, &s);
    }
}

/// An arrow in flight: gone when it hits something or has flown for five seconds.
fn arrow_flies(w: &World, id: u64, st: &EntityState) -> bool {
    let shooter = w.component(id, &SHOOTER).copied().unwrap_or(0);
    let hit = w
        .near_targets(st.pos, 1.0, None)
        .into_iter()
        .find(|&e| e != id && e != shooter && w.entity(e).is_some_and(|s| s.kind != st.kind));
    if let Some(target) = hit {
        w.damage(target, 3.0, shooter, damage::ATTACK);
        w.remove(id);
    } else if st.flags & entity_flags::BLOCKED != 0 || st.on_ground != 0 || st.age > 100 {
        w.remove(id);
    }
    true
}

/// A chicken: flutters down instead of falling, and lays an egg now and then.
fn passive_chicken() -> ai::Goals {
    let goals = ai::Goals::new().then(|w: &World, id: u64, st: &EntityState| {
        if st.on_ground == 0 && st.vel[1] < 0.0 {
            let mut now = *st;
            now.vel[1] *= 0.6;
            w.set_entity(id, &now);
        }
        if let Some(next) = w.component_mut(id, &FUSE) {
            if *next == 0 {
                *next = 6000 + w.random_below(6000) as u32;
            } else {
                *next -= 1;
                if *next == 0 {
                    w.spawn_item(st.pos, ItemStack::new(EGG.id(), 1));
                }
            }
        }
        false
    });
    with_passive(goals)
}

fn panic(w: &World, h: &Hurt) {
    if let Some(b) = w.component_mut(h.entity, &BRAIN) {
        b.panicking = 1;
        w.after(h.entity, 60, &CALM, h.entity);
    }
}

/// Monsters chase the player they are after and hit it when in reach (`ai::melee`, once a
/// second). Otherwise they wander.
fn hostile(reach: f64) -> ai::Goals {
    ai::Goals::new()
        .then(ai::watch_player(8.0))
        .then(ai::melee(reach))
        .then(ai::follow_player(35.0, 1.0))
        .then(ai::wander(0.6, 10.0))
}

/// A spider's peace: out in daylight.
fn in_daylight(w: &World, st: &EntityState, _player: &EntityState) -> bool {
    let feet = [st.pos[0].floor() as i32, (st.pos[1] + 0.5).floor() as i32, st.pos[2].floor() as i32];
    w.daylight() >= 0.5 && w.sky_light(feet) >= 10
}

/// Animals run about for three seconds after being hurt, otherwise wander.
fn passive() -> ai::Goals {
    with_passive(ai::Goals::new())
}

fn with_passive(goals: ai::Goals) -> ai::Goals {
    goals
        .then(ai::watch_player(6.0))
        .then(|w: &World, id: u64, st: &EntityState| {
            if w.component(id, &BRAIN).is_none_or(|b| b.panicking == 0) {
                return false;
            }
            // A new direction every second.
            if w.time() % 20 == id % 20 || !w.navigating(id) {
                let a = w.random_below(628) as f64 / 100.0;
                w.navigate(id, [st.pos[0] + a.cos() * 8.0, st.pos[1], st.pos[2] + a.sin() * 8.0], 1.6);
            }
            true
        })
        .then(ai::wander(0.5, 10.0))
}
