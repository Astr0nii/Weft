//! Things that only change how the game looks, all as plain mod code and assets:
//! - a glow crystal whose veins glow by a `_e` mask (LabPBR-style emission, no light source),
//!   and a glowing birch, leaves, moss and flower with masks of their own,
//! - sparks: a particle kind with its own shader, sent from the server for everyone nearby,
//! - wisps: floating entities drawn by an entity shader, their core glowing by a mask,
//! - a rune whose tooltip uses text effects (engine and own) and conditions (own and engine),
//!   and which shows the area it would affect as a see-through box while held,
//! - a screen effect toggled with a key, and machine screens drawn by a GUI shader (named in a `.gui`),
//! - an idol from a Blockbench model file (bones, a mesh pair of wings) that idles by keyframes
//!   and bows when used, and an orb whose model is built in code (a sphere mesh and a ring of
//!   stones on a child bone) but animated from a file,
//! - the player waves when using the rune: keyframes for the player's model, seen from anywhere.

use weft_sdk::*;

pub static SPARK: Particle = Particle::named("spark");
static WISP: Entity = Entity::named("wisp");
static IDOL: Entity = Entity::named("idol");
static ORB: Entity = Entity::named("orb");
static MOSAIC: Block = Block::named("mosaic");
static GLOW_MOSS: Block = Block::named("glow_moss");
static GLOW_LOG: Block = Block::named("glow_log");
static GLOW_LEAVES: Block = Block::named("glow_leaves");
static GLOW_FLOWER: Block = Block::named("glow_flower");
/// A thin layer that can lie over a plant: a cell holds them both.
static SNOW_LAYER: Block = Block::named("snow_layer");
static GLOW_CRYSTAL: Block = Block::named("glow_crystal");
static GLOW: TintKind = TintKind::named("glow");
static SPIRITS: EntityCategory = EntityCategory::named("spirits");
static HURT: TextCondition = TextCondition::named("hurt");
static DREAM_KEY: Key = Key::named("dream");
static RUNE: Item = Item::named("rune");
/// Drawn from `models/item/totem.json`: a stone base and a crystal, no icon of its own.
static TOTEM: Item = Item::named("totem");
/// Idols currently bowing.
static BOWED: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());
/// Settings players change in Settings > Mod Settings.
/// The generator's low hum (in `lib.rs`).
pub static GENERATOR_HUM: Config<bool> = Config::named("generator_hum");
static WISP_SPARKS: Config<i32> = Config::named("wisp_sparks");
static DREAM_STRENGTH: Config<f32> = Config::named("dream_strength");
static DREAM_STYLE: Config<u32> = Config::named("dream_style");

pub fn register(r: &mut Registry) {
    r.config(&GENERATOR_HUM).bool(true);
    // The server decides how much its wisps sparkle; joining players get its value.
    r.config(&WISP_SPARKS).int(2, 0, 10).server();
    r.config(&DREAM_STRENGTH).float(1.0, 0.1, 1.0).step(0.05);
    r.config(&DREAM_STYLE).choice(0, &["warm", "cold"]);
    r.particle(&SPARK)
        .textures(&["core:glint_3", "core:glint_2", "core:glint_1", "core:glint_0"])
        .life(0.6, 1.2)
        .size(0.12, 0.2)
        .gravity(-0.6)
        .drag(0.9)
        .color(0xFFFFFFFF, 0xFFFFFF60)
        .fullbright()
        .translucent()
        .shader("twinkle");

    // A glowing obelisk here and there in the deserts of another mod's world.
    static OBELISK: Template = Template::named("obelisk");
    let mut obelisk = r.template(&OBELISK).layer(&["sss", "sss", "sss"]);
    for _ in 0..4 {
        obelisk = obelisk.layer(&["   ", " g ", "   "]);
    }
    drop(obelisk.key('s', "base:sandstone").key('g', "glow_crystal"));
    r.structure(&crate::OVERWORLD, &OBELISK)
        .spacing(128)
        .chance(0.5)
        .biome("#dry")
        .buried(1);
    // Glow crystals grow only in the overworld's deep caves (an underground biome).
    r.ore(&GLOW_CRYSTAL)
        .veins(3)
        .size(6)
        .replace("#stone_ore_replaceables")
        .biome("#cave");
    // A 64x64 texture among 16x16 ones: every texture keeps its own size.
    r.block(&MOSAIC).hardness(1.5).mineable("pickaxe");
    // A tint kind of the mod's own: violet by default (and on the item), teal in plains.
    r.tint_kind(&GLOW, 0xc080ff);
    r.biome_tint(&Biome::named("base:plains"), &GLOW, 0x40e0c0);
    r.block(&GLOW_MOSS)
        .tint(&GLOW)
        .hardness(0.6)
        .mineable("shovel");
    // A glowing birch (a log's `tree` component grows it from a sapling or worldgen) and
    // flower; not generated in the world, only from the creative tabs.
    static TREE_LEAVES: Component<BlockRef> = Component::named("tree_leaves");
    r.block(&GLOW_LOG)
        .textures("glow_log_top", "glow_log", "glow_log_top")
        .hardness(2.0)
        .mineable("axe")
        .tag("logs")
        .set_text(&TREE_LEAVES, "glow_leaves");
    r.block(&GLOW_LEAVES)
        .cutout()
        .opacity(1)
        .tint(&GLOW)
        .hardness(0.2)
        .tag("leaves")
        .mineable("shears");
    r.block(&SNOW_LAYER)
        .texture("base:snow")
        .model_box([0.0, 0.0, 0.0], [16.0, 2.0, 16.0])
        .not_solid()
        .needs_below("")
        .shares_cell_with("#plantable")
        .hardness(0.1);
    r.block(&GLOW_FLOWER)
        .plant()
        .needs_below("#plant_soil")
        .tint(&GLOW);
    r.block(&GLOW_CRYSTAL)
        .hardness(1.5)
        .mineable("pickaxe")
        // Sparks now and then, shown by the client near the player.
        .ambient_particles(&["spark", "core:smoke"]);

    // Its own category (no spawning rule: wisps come only from commands and creative): they
    // drift off again when nobody is within 64 blocks.
    r.entity_category(&SPIRITS, 4, 64.0);
    r.entity(&WISP)
        .category(&SPIRITS)
        .size(0.4, 0.4)
        .health(4.0)
        .speed(1.5)
        .flags(entity_flags::NO_GRAVITY)
        .unpushable()
        .model(Model::from(vec![Part::new("wisp", [0.0, 3.0, 0.0]).cube([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0.0, 0.0])]).uv(32, 16))
        .shader("ghost")
        .on_tick(wisps);

    r.entity(&IDOL)
        .size(0.6, 1.1)
        .health(20.0)
        .animations("idol")
        .on_interact(|e| {
            let mut bowed = BOWED.lock().unwrap();
            let id = e.entity();
            if let Some(i) = bowed.iter().position(|&b| b == id) {
                bowed.swap_remove(i);
                e.world().play_action(id, "", false);
            } else {
                // Bows to whoever used it.
                let w = e.world();
                if let (Some(mut me), Some(them)) = (w.entity(id), w.entity(e.other())) {
                    me.yaw = yaw_toward(me.pos, them.pos);
                    w.set_entity(id, &me);
                }
                bowed.push(id);
                w.play_action(id, "bow", false);
            }
            true
        });
    let ring = Part::new("ring", [0.0, 8.0, 0.0]).child_of(0);
    let ring = [[6.0, -1.0, -1.0], [-8.0, -1.0, -1.0], [-1.0, -1.0, 6.0], [-1.0, -1.0, -8.0]]
        .into_iter()
        .fold(ring, |p, at| p.cube(at, [2.0, 2.0, 2.0], [16.0, 0.0]));
    r.entity(&ORB)
        .size(0.6, 0.9)
        .health(4.0)
        .flags(entity_flags::NO_GRAVITY)
        .model(
            Model::from(vec![Part::new("core", [0.0, 8.0, 0.0]).mesh(sphere([0.0, 8.0, 0.0], 4.0, 8, 12, [0.0, 0.0, 16.0, 16.0])), ring])
                .uv(32, 16),
        );

    r.set_animations(&PLAYER, "testmod:player");
    r.item(&TOTEM).max_stack(1);
    // Goes before what the other hand holds, and leaves it its turn with nothing to shoot.
    static LONG_BOW: Item = Item::named("long_bow");
    r.item(&LONG_BOW)
        .use_priority(10)
        .on_use(|u| u.stack().count > 1);
    // Both hands or nothing, swinging with its whole body.
    static GREATSWORD: Item = Item::named("greatsword");
    r.item(&GREATSWORD)
        .max_stack(1)
        .two_handed()
        .no_creative_break()
        .attack_damage(16f32)
        .attack_speed(0.7)
        .on_use(|_| true);
    // A metre of katana in the hand, on the ground and in a slot; much less of one once damaged.
    r.set_model(&GREATSWORD, "katana");
    r.item_model_for_stack(&GREATSWORD, |s| (s.damage() > 0).then_some("testmod:katana_broken"));
    // Food with models of its own, by one line and a file each: what has none stays as it was.
    r.set_model("#food", "food/{name}");
    // Its chain hangs and sways by itself: its bones are called `hanging_..`.
    static LANTERN: Item = Item::named("lantern");
    r.item(&LANTERN).max_stack(1);
    r.set_model(&LANTERN, "lantern");
    // Only who wears it has a tail, moved by what plays on them and swinging by itself.
    static FOX_TAIL: Item = Item::named("fox_tail");
    r.item(&FOX_TAIL)
        .max_stack(1)
        .wearable_model(&EquipmentSlot::named("base:legs"), "fox_tail", "body");
    r.bone_motion("fox_tail", "tail2", BoneMotion::SPRINGY);
    r.bone_motion("fox_tail", "tail3", BoneMotion::SPRINGY);
    // Both hands, or one if the other is busy.
    static MAP_SHEET: Item = Item::named("map_sheet");
    r.item(&MAP_SHEET)
        .max_stack(1)
        .one_handed_action("hold_one")
        .on_use(|_| true);
    // A model of bones in `models/item/`, its slide going back in the same clip as the arm.
    static BLASTER: Item = Item::named("blaster");
    r.item(&BLASTER).max_stack(1).handheld();
    r.item(&RUNE).max_stack(1).on_use(|u| {
        let w = u.world();
        w.play_action(u.player(), "wave", false);
        if let Some(st) = w.entity(u.player()) {
            w.spawn_particles(&SPARK, [st.pos[0], st.pos[1] + 1.2, st.pos[2]])
                .count(40)
                .spread(0.2)
                .speed(4.0);
        }
        true
    });
    r.text_condition(&HURT, |v| v.me().is_some_and(|m| m.health < m.max_health));

    r.key(&DREAM_KEY, "N").on_press(|c| {
        let on = DREAM.fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
        let effect = if DREAM_STYLE.get() == 1 { "dream_cold" } else { "dream" };
        c.post_effect("dream", 0.0);
        c.post_effect("dream_cold", 0.0);
        if !on {
            c.post_effect(effect, DREAM_STRENGTH.get());
        }
    });
    r.client_frame(|c| {
        // Holding the rune shows what it would reach: a see-through sheet over the ground around.
        let st = c.state();
        if c.inventory_slot(st.selected).item == RUNE.id() {
            let p = st.pos.map(f64::floor);
            c.draw_fill(None, [p[0] - 2.0, p[1], p[2] - 2.0], [p[0] + 3.0, p[1] + 0.06, p[2] + 3.0], 0x60D0FF50);
        }
    });
}

static DREAM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Wisps drift in a new direction every two seconds, bob up and down and shed sparks.
fn wisps(t: &EntityTick) {
    let w = t.world();
    let time = w.time();
    for &id in t.ids() {
        let Some(mut st) = w.entity(id) else { continue };
        let phase = (id % 40) as u64;
        if (time + phase) % 40 == 0 {
            let a = w.random_below(360) as f32 * std::f32::consts::PI / 180.0;
            st.yaw = a;
        }
        st.vel[1] = ((time as f64 + phase as f64) * 0.1).sin() * 0.8;
        w.set_entity(id, &st);
        w.walk(id, yaw_dir(st.yaw), 1.0, false);
        if (time + phase) % 20 == 0 {
            w.spawn_particles(&SPARK, [st.pos[0], st.pos[1] + 0.2, st.pos[2]])
                .count(WISP_SPARKS.get().max(0) as u32)
                .spread(0.15)
                .speed(0.3);
        }
    }
}
