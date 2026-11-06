# Weft modding cheat sheet

## Contents
1. [Introduction](#introduction)
2. [Preparation](#preparation)
3. [Creating a mod in two minutes](#creating-a-mod-in-two-minutes)
4. [Names and paths](#names-and-paths)
   - [Names](#names), [References](#references), [Handles](#handles), [Textures](#textures-and-other-assets), [Translations](#translations)
5. [Components and tags as inter-mod communication](#components-and-tags-as-inter-mod-communication)
6. [Content builders](#content-builders)
   - [Blocks](#blocks), [Items](#items), [Entities](#entities), [Block entities](#block-entities), [Liquids](#liquids), [World](#world), [Recipes](#recipes), [Loot](#loot), [Replacing something of another mod](#replacing-something-of-another-mod)
7. [Hooking into the game](#hooking-into-the-game)
   - [Built-in events](#built-in-events), [Systems and energy](#systems-and-energy)
8. [Text on the screen](#text-on-the-screen)
9. [Commands](#commands)
10. [World API](#world-api)
11. [Client side](#client-side)
12. [Debugging](#debugging)
13. [Things that make your and other modders' life easier](#things-that-make-your-and-other-modders-life-easier)
14. [Guides for bigger systems](#guides-for-other-systems)

## Introduction
This is a guide describing how the base game itself is written.\
The game is a mod called `base`, which has no private hooks or priorities from the engine, so if base can do something,
your mod can too.\
When in doubt, check `mods/base` and `mods/testmod` and copy from there.\
testmod exists to prove you don't have to touch the engine.

## Preparation
1. Before starting working on Weft, you'll need workspace. The best recommendation I can give is using JetBrains RustRover, which is free and has everything needed.\
2. Clone the repo using built-in Clone Repository feature, where you'll put https://github.com/Weft-Rust/Weft as a link, which will create folder on your pc with the code.\
3. Then after finishing you need to configure Run Configuration. Find Run Configuration tab in top-right corner, and set the command to `run`, and call it whatever, after that the game will be launched from your local copy of the repo.

## Creating a mod in two minutes

Fastest way to begin is to copy `examples/examplemod`.\
It is a ready mod with one item, a setting and an event.\
Every kind of registration is written out there as a commented example with a pointer to where the base game uses it.\
Rename three things listed at the top of its `lib.rs` and you can begin creating your mod.\
The bare minimum looks like this:

```toml
# Cargo.toml
[package]
name = "mymod"            # your mod id: the library, the mod's folder and its lang keys go by it
version = "0.1.0"         # the version players see in the Mods screen
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
# you want to replace this path with the one to the SDK after copying this folder somewhere else
weft_sdk = { path = "../../crates/sdk" }

# only while the mod folder lies inside another cargo project (like this repository), see examples/examplemod/Cargo.toml
[workspace]
```

```rust
// lib.rs
use weft_sdk::*;

declare_mod!(deps: ["base"], init: init);

pub static RUBY: Item = Item::named("ruby");
pub static RUBY_ORE: Block = Block::named("ruby_ore");
static INGREDIENTS: CreativeTab = CreativeTab::named("base:ingredients");

fn init(r: &mut Registry) {
    r.block(&RUBY_ORE).hardness(3.0).mined_by("pickaxe", tool_tier::IRON).drops("ruby", 1);
    r.item(&RUBY).tag("gems/ruby").tab(&INGREDIENTS);
    r.ore(&RUBY_ORE).y(-64, 16).veins(2).vein_size(5).replaces("#stone_ore_replaceables");
}
```

Build with `cargo build --release`, then make a folder in the mods folder and copy the library from `target/release/` into it. That's `run/mods/` when you run the game from the repo; a released game keeps it in your user folders (see the README), and the Mods screen has a button that opens it.

```
run/mods/mymod/
    mymod.dll          (libmymod.so / libmymod.dylib elsewhere)
    assets/
        textures/block/ruby_ore.png
        textures/item/ruby.png
        lang/en_us.lang
```

`weft --pack-mod run/mods/mymod` makes a `mymod.zip` of the same thing, which is what you can send to anyone.\
The folder has to hold the library that's what gets packed.

The library is built for one system, so that zip only runs on yours, but you write your mod once and just build it three times. Put `mymod.dll`, `libmymod.so` and `libmymod.dylib` in the folder and `--pack-mod` takes them all, then the same zip works everywhere: the game picks the file of its system.\
`examples/examplemod/.github/workflows/release.yml` builds all three on GitHub and publishes the zip when you push a tag, the top of the file says what to set up.

The mod's id and version come from `Cargo.toml`, and its name from the lang file (`mod.mymod.name`), so `declare_mod!` only lists the mods yours needs.\
A folder or zip named differently from the crate stops the game at load, since the library inside wouldn't match.

A word on versioning: the mod interface is checked by a hash of its source. If your mod was built
using another engine version, it just refuses to load and tells you to rebuild.

## Names and paths
### Names
Everything a mod adds has a name `namespace:path`: blocks, items, entities, menus, biomes, dimensions,
settings, sounds, particles, events, keys, screens, HUD elements, rules, recipe kinds and so on.\
In your own mod you write just `ruby` and it becomes `mymod:ruby`, the same when you register it and when you read it.\
Other mods' things go by their full name: `base:furnace`. The engine's own are `core:..`.

### References
Wherever a block, item or biome is named as text (recipes, loot, ore and structure filters, `r.set`, support rules, spawn rules, `.ctm` files, `{block:..}` and `{item:..}`, commands), it is written the same way:
- `base:stone` is that thing, `base:furnace[lit=true]` the furnace's states with that value
- `#logs` is anything with the tag, `#ingots/*` anything with a tag starting with `ingots/`
- calls that only make sense with a tag (`needs_below`, `.biome(..)`, tool conditions) still take it with `#`, so it never looks like a name

A name nothing has, or a tag pattern no tag matches, stops the game at load with your mod's name and what was wrong.\
Where one item has to come out of a tag (a recipe output, a loot entry), it's the preferred one: the first registered in mod load order.\
Recipes have one more form, a variable: `#ores/{m}` in and `#ingots/{m}` out makes one recipe per material that has both.

### Handles
The name lives in a handle, and you register by the handle:

```rust
static RUBY: Item = Item::named("ruby");
static DASH: Key = Key::named("dash");
static STONE: Block = Block::named("base:stone");   // someone else's, only read

r.item(&RUBY).max_stack(16);
r.key(&DASH, "R").on_server(|p| dash(p.world(), p.player()));
if STONE.is(w.block(pos)) { .. }
```

Every kind of thing works like this, so there is nothing else to remember.\
Once the game is loaded a handle gives you the numeric id (`RUBY.id()`), and hot code works only with ids.\
Families made in a loop take `Item::generated(format!("{metal}_ingot"))`, a handle made from a name at runtime.

### Textures and other assets
Every call that takes a file knows its folder: blocks look in `block/`, items in `item/`, mobs and worn armor in `entity/`, particles in `particle/`, slot backgrounds and `{img:..}` in `gui/`, sounds in `sounds/`.\
So `.texture("ruby")` for a block means `mymod:block/ruby`, for an item `mymod:item/ruby`, and `"base:ruby"` is the same folder of the base game.\
Subfolders are fine (`.wearable(.., "armor/ruby_1")` is `entity/armor/ruby_1`), but a name can't leave its folder: `"block/ruby"` for an item, `..` or an absolute path stop the game at load. An item that wants to look like a block gets a model, not the block's texture path.\
Resource packs replace any of them by putting a file at the same place.\
Textures can also be [animated, glow and have overlays](textures.md), or [be connected](ctm.md)).

### Translations
Format: `type.modname.name = value`, e.g. `item.examplemod.example_item = Example Item`\
Any item can have a tooltip using `type.modname.name.tooltip = Some tooltip`, more in the [localization guide](localization.md).\
If the player's language doesn't have a key (or a whole file), English is shown, so `en_us.lang` is the one you always need.\
The Mods screen reads your mod's page from there too: `mod.mymod.name`, `mod.mymod.description` (as long as you like, with pictures), `mod.mymod.authors` and `mod.mymod.url`, and its icon is `assets/textures/gui/icon.png`.

Text can carry colors and effects:
- `{#ff8080}This text is red{/}`
- `{fx:core:wave}wavy{/fx}` (new fx can be added from your mod too)
- `{on_key:!core:sneak}This text will appear when the bound key is NOT pressed{/on_key}`
- `{on_key:core:sneak}This text will appear only when the bound key is pressed{/on_key}`
- `{if:creative}This text will appear if the player is in creative mode{/if}` (any game mode's name works, a mod's own as `{if:mymod:adventure}`)
- `{img:mymod:logo}` (from `gui/`), `{block:base:furnace}` and `{align:center}..{/align}` for pictures, blocks and centered lines
- `{name:item:base:coal}` for the name of anything players see named (items, blocks, liquids, tabs, mods, recipe kinds, settings, keys), the same name everywhere

All of it works in any text the game shows, chat and command replies included, so there is no second kind of text to learn.

## Components and tags as inter-mod communication

This is the most important part that you should remember to make life easier for you and other modders working with your mod.\
Properties are named values on content. Things like fuel, food, durability, etc etc, are components or tags.\
This is done so any mod reads and writes them without depending on whoever added a certain block.

```rust
// Declared where it's used, or taken from the mod that reads it (`base_api::FUEL`, see below)
static FUEL: Component<u32> = Component::named("fuel");
static HOT: Tag = Tag::named("hot");

r.item(&COAL_DUST).set(&FUEL, 800).tag("hot");      // on your own content
r.set("base:stick", &FUEL, 100);                    // on someone else's
r.set("#planks/*", &FUEL, 300);                     // on everything with a tag
r.tag("hot", &["base:lava", "base:magma"]);
r.set("base:stone", &HARDNESS, 3.0);                // the engine's own block properties too

// Reading
if let Some(ticks) = FUEL.on_item(stack.item) { /* burn it */ }
if HOT.on_block(state) { /* harm player */ }
```

Component and tag names are not things a mod owns, they are a vocabulary.\
Blocks and items belong to the mod that adds them, but `fuel`, `food`, `sand`, `conductor` or `ores/iron` belong to nobody
and mean the same in every mod, so they are written as they are, without a namespace.\
A term that only makes sense for your mod can carry its prefix, `mymod:heat`, so nobody else picks the same word by accident.

Block properties the engine reads are components as well: `HARDNESS`, `FRICTION`, `DRAG`, `LIGHT`, `TINT` (+ `TINT_FACES`), `SUPPORT_FACE` / `SUPPORT` / `SUPPORT_NEAR` and `HARVEST_LEVEL`.`.hardness(1.5)`, `.light(14)`, `.needs_below("#soil")`, `.tint(&tints::GRASS)` on a builder just set them, so another mod changes them with `r.set` and reads them with `HARDNESS.on_block(state)` like any other.

Your own data structures:

```rust
value! {
    #[derive(Default)]
    struct Burner { burn: u32, burn_max: u32 }
}
static BURNER: Component<Burner> = Component::named("mymod:burner");
```

A value that names blocks is a reference, not text: `Component<BlockRef>` for one block (or state) and `Component<BlockList>` for blocks with weights, set with `.set_text(&C, "oak_leaves")` or `.set_text(&C, "birch_log:1 oak_log:4")`. The engine turns the names into ids when the game loads, so a typo is a load error instead of a tree that never grows; reading gives `BlockRef(state)` or a `BlockList` with `pick(random)`. Base's trees work this way (`tree_leaves` on logs, `tree_kinds` and `flowers` on biomes).

Per-entity components like a furnace's state, hunger or anything else that changes in time go through
`r.entity_component(&COMP, default, flags)`, declared once (`r.edit(COMP.declaration())` changes another mod's), and then carried by the kinds that need it: `.attach(&COMP)` on an entity builder (a block entity's too), `r.attach(&PLAYER, &COMP)` for someone else's kind, `.attach_with(&COMP, value)` to start from another value.\
`.set(&C, v)` is different: one value of the kind itself (like a mob's attack damage), while `.attach` gives every entity its own value that changes.\
Flags from `component_flags` are `SAVE`, `SYNC_OWNER`, `SYNC_WATCHERS`, `SYNC_MENU`, `KEEP_ON_RESPAWN` (kept when a player respawns; everything else starts over from its default).\
You can change them through `w.component_mut(id, &COMP)` and the engine will figure out what to send.\
A `SAVE` component is saved as its bytes, so don't keep ids in one: ids of dimensions, items, blocks and the like are numbers that shift when a mod comes or goes, and yours would then point at something else. For a dimension save `dimension_key(id)` and read it back with `dimension_from_key(key)`.\
What the engine itself shows of an entity travels the same way, so a client can read it with `view.component_of(id, &C)`: `core:health` (`[f32; 2]`, health and, for players, max health) `core:burning` (`u8`) and `core:held` (`u32`, a player's item in hand, 0 for none). They are the engine's copies: writing them changes nothing.

### Sharing types with other mods
Events and components carry values, and two mods have to agree on their layout. Copying a struct into every mod works, but the mistake shows up only when the game loads.
So a mod can publish a small crate with just its types and handles, the way the base game does with `base_api` (`mods/base/api`): the `value!` structs of its events and components, and `Event`, `Component`, `Chain`, `RecipeKind`, `EquipmentSlot`, `Dimension` and tab handles with full names (`base:smelted`, since the statics compile into whoever uses them).
```toml
[dependencies]
weft_sdk = { path = "../../crates/sdk" }
base_api = { path = "../../mods/base/api" }
```
```rust
r.on_event(&base_api::SMELTED, |b| for s in b.iter() { /* s.furnace, s.input, s.output */ });
```
The mod itself uses the same crate, so there's one declaration of everything, and other mods depend on types only, never on the mod's code. The engine still compares the layouts at load.

## Content builders

All of them are `r.<thing>(&HANDLE)` followed by settings. The registration happens when the builder
is dropped.

### Blocks
`r.block(&BLOCK)` and then:
- `hardness`
- `mined_by("pickaxe", tier)`: tag `mineable/pickaxe` and the `harvest_level` component (`tool_tier::HAND` for none); loot pools with `Cond::Harvested` pass only for a fitting tool of that tier
- `drops(item, n)` or `drops_nothing()`: shorthand for its loot table (`drops` puts `Cond::Harvested` on the pool). Without either the block drops its own item the same way
- `light(15)` or `light_rgb(r, g, b)`
- `cutout()`, `translucent()`
- `front_faces(facing, side, top, front)` in `.states` for a furnace's kind of block
- `model_box(min, max)` (faces show the state's textures) or a `models/block/<name>.json` from Blockbench ([models guide](models.md#blocks-and-items)), `hitbox(min, max)` for its shape in the same 1/16 units, one box a call: what stops a walker, what the crosshair outlines and cracks, only the outline on a block that isn't solid. Without any: the model's boxes, else a plant's cross, else the whole block. Calling `frame(f)` first makes the boxes and hitboxes after it turn with the block, and `turned(..)` and `tilted(..)` change the last box. The order of the other calls doesn't matter (`cutout()` and `opacity(2)`, say)
- `needs_below("#soil")`, or `needs(Dir::West, "")` for another side
- `two_part("part", ["foot", "head"], |state| state_facing(state).opposite())` for a block two cells long: the second part goes where that says, needs room, and each goes with the other
- `grows(seconds, min_light)` + `on_grow(..)` for plants
- `spreads("#grass_spreads_to", seconds, min_light)` for what creeps onto neighbors (grass onto dirt), with `spread_reach(min, max, covered)` when it reaches otherwise than grass does (moss every way and up walls), `smothered_into("dirt", seconds)` for what turns when covered. There are no random ticks: these, growth and scheduled ticks all run on world time, so they catch up on time nobody was there for
- `ambient_particle("mymod:spark")` for particles the client shows around a block now and then (torch smoke), nothing on the server
- `on_use`, `on_neighbor`, `on_scheduled`, `on_carried` (on a moving body, it came to another cell of the world: a drill, a plow)
- `on_place(|w, change| ..)`, `on_break(..)`: `actions::PLACE_BLOCK` and `actions::BREAK_BLOCK` for this block only, at the end of the tick
- `prop("lit", ["false", "true"])`, `prop("age", 0..=7)` and `facing(Facings::HORIZONTAL, Toward::Player)` (or `Toward::Face`, out of what was clicked, or `Toward::Away`, where the player looks) for states, like blockstates in modern Minecraft: every combination of values is a state
- `states(|p, s| ..)` to make each state look different (`p.facing()`, `p.is("lit", "true")`), `placement(|place, p| p.set(..))` to pick values when placed
- `joins(Facings::HORIZONTAL, "#fences")` for what connects to its neighbors (fences, panes, pipes, cables): a `true`/`false` property per side the engine keeps right as blocks around change, so `.states` only draws `p.is("north", "true")`; joining by a shared tag connects other mods' blocks too, `"self"` only its own kind. `derived("shape", |n| ..)` is the same for any property worked out from `n.at(Dir::North)` and the rest (stairs' corners are one)

A block comes in stairs, slabs and vertical slabs by one tag, `.tag("has_shapes")`: base gives them to every block with it, whichever mod it is from, with its textures, its hardness and sounds, and recipes. To make such blocks yourself, `r.stairs("#my_tag")`, `r.slabs(..)` and `r.vertical_slabs(..)` take names and tags, and `.tab(..)` and the like go on what they return. `r.shapes_of("wall", "#my_tag", "_wall")` is the same for a shape of your own: `.prop(..)`, `.states(..)` and the rest as on any block, each made block being `<source>_wall`, and `shape.wall = {0} Wall` in your lang file naming them all. They are tagged `wall/<source>` and the source `wall_of/<source>`, so one recipe does for all: `r.shaped(C, "#wall/{m}", 6, &["###", "###"], &[('#', "#wall_of/{m}")])`. A block with its own name for one, `mymod:ruby_stairs` when there is a `ruby`, isn't made twice.

`merge(|place, p| ..)` is what its own item does placed in its cell: `p` is what is there, `place.placed("type")` how the item would go, and setting a property takes it in (two slabs of one block make it whole). `loot_rolls(2)` on a state rolls its loot twice. `place.hit()` is the point just outside what was clicked, from this cell's corner: `hit().y > 0.5` is the upper half, on a floor, under a ceiling, against a wall or on a slab alike.

A cell can hold any number of blocks. A block goes in with what is there when their hitboxes don't overlap, two slabs of different blocks say, with nothing to declare; `.shares_cell_with("#plantable")` lets it in though they do (snow over a plant). `.holds_liquid()` keeps the liquid source it is placed in and lets a bucket fill it: that is waterlogging, on slabs and stairs out of the box. The liquid doesn't pass the sides its block's hitbox covers whole (a stair's back holds it, a hollow log lets it out its ends), or the ones `.liquid_walls(&[Dir::North, Dir::South])` names (turned with `oriented`, `&[]` for none). From code: `w.parts(pos)` is all that is in a cell, `w.part_add(pos, state)` adds one whatever its shape, `w.part_remove(pos, state)` and `w.part_replace(pos, old, new)` act on one. `w.block` gives the one the cell stands for, and `w.set_block` replaces all of it, unless it is the same block, a property changing. `w.prop` and `w.prop_set` find the part that has the property. Inside a block's own callback `w.block(pos)` and `w.set_block(pos, ..)` mean that block, so it works the same in a cell with others. Only one block in a cell can have a block entity, a circuit, light of its own or growth, and that is the one the cell stands for; each part gets neighbor updates, scheduled ticks, its own support and its own loot, and the player breaks and uses the one aimed at.

Several blocks, any mod's, can share one model file: `.model_file("othermod:hollow_log")`, with `.model_texture("inside", "stripped_oak_log")` putting this block's own picture where the file says `#inside`. `.hitbox(..)` is what stops a walker and is outlined; `.outline(..)` outlines something else (a hollow log outlined whole). Both take several boxes, one a call.

Everything a builder set is readable from code, any mod's block or item: `block_info(state)` (its block, mod, `solid()`, `opaque()`, light, hardness, friction, model, textures, liquid, item), `item_info(item)` (mod, stack size, durability, attack, what it places, where it is worn), `components_of(entry_kind::BLOCK, state)` for every component and tag by name, values then through their handles, and `entries_with(entry_kind::BLOCK, "logs")` for every state with a tag. That's a WAILA, or hollow logs made for every mod's logs. `texture_exists("othermod:block/log_oak_inner")` says whether a mod has a picture (its own files, not resource packs), so you make one only where there is one; `asset_exists(name, "models", "json")` for any other file.

In code a state is read and changed by name, `w.prop(pos, "lit")` and `w.prop_set(pos, "lit", "true")`, the other properties stay as they are.
Anywhere a block is named as text, `base:furnace[lit=true]` picks a state: in structures, `r.set(..)`, `{block:..}` and commands (`/setblock ~ ~ ~ base:furnace[facing=east]`).

### Items
`r.item(&ITEM)` and then:
- `texture`
- `max_stack(N)`
- `durability`
- `tool("pickaxe", tier)`, `mining_speed(speed)`
- `attack_damage`, `attack_speed(per_second)` (hitting before it has recharged does less; a sword is 1.6, and without it an item swings at an empty hand's 4) and `sweeps()` (a full swing on the ground also does 1 damage to what stands next to the target)
- Advancements read tags, so yours count by tagging them: `cooked_meat` on food, `hostile` and `livestock` on mobs, `logs`, `torches`, `furnaces`, etc etc
- `no_creative_break()`: in creative it doesn't break blocks, like a sword. Both sides know, so nothing flickers; a rule that depends on more is `actions::BREAK_BLOCK`, which is asked when mining starts and tells the client no right away
- `spawns(&ENTITY)`: used on a block it puts the entity there. Every entity already has an egg of its own, see below
- `places(&BLOCK)`
- `handheld()`: held by its end and pointing ahead in third person, like a stick. `tool(..)` does it already, and so does the tag `handheld`
- `wearable(&HEAD, inflate, texture)` for armor and anything else worn in an equipment slot
- `two_handed()`, `one_handed_action(action)`, `use_priority(n)`: [what it does in the hands](models.md#items-in-the-hand)
- `on_use`: what using it does. Before it runs anyone can say no to `actions::USE_ITEM`, and after it the same action's event tells every mod (testmod heals with base's food through it). `base_api::eat` is the base game's eating: `.set(&FOOD, ..).use_ticks(32).on_use(base_api::eat)`

#### Stacks with their own data
A stack is an item, a count, and whatever values of components it carries of its own, like the damage a tool took. You never touch `data` yourself, it's only a handle to those values:
```rust
value! { pub struct Quality { pub grade: u32, pub maker: u32 } }
static QUALITY: Component<Quality> = Component::named("quality");

// While registering, once:
r.stack_component(&QUALITY, 0);

// Making one, a custom workbench's output for example:
let out = PICKAXE.stack(1)
    .with(&QUALITY, Quality { grade: 3, maker: 7 })
    .with_wear(10);
inventory.set(slot, out);

// Reading it:
QUALITY.on_stack(&stack);   // its own value, else what the item has
QUALITY.on_stack_own(&stack); // only its own
stack.wear();
let plain = stack.without(&QUALITY);
```
A stack's own value wins over its item's for everything the engine reads too: give one stack its own `DURABILITY`, `MINING_SPEED`, `TOOL_LEVEL`, `ATTACK_DAMAGE`, `ATTACK_SPEED`, `USE_TICKS` or a `tools/<class>` tag and it digs, wears and hits by those. Anything about a stack can be changed from outside: its item, count and any value, with `inventory.set(slot, ..)` and `.with(..)`.

A tool made of parts keeps them in a storage (`r.storage(&PARTS, 3)`, see [block entities](block_entities.md)): `stack.stacks(&PARTS)` and `stack.with_stacks(&PARTS, &[handle, head, binding])` read and write them right on the stack, no slot needed, and each part is a stack with its own values and parts, as deep as you go. Add the parts' values up when you make it and write the result onto the stack. `stack.with_layers(&[("rod", None), ("pick_head", Some(0xB0B0B0))])` makes it show its parts' pictures one over the other, tinted, in slots, in the hand and on the ground.

Stacks with different data don't merge. `r.stack_component` says a component exists for stacks, its flags are `abi::stack_flags`: `SERVER_ONLY` (clients get the stack without it), `NO_SAVE`, or 0 for neither. `.with` on a component nobody declared does nothing and says so in the log. Ids in a value shift when a mod comes or goes, the same as in a `SAVE` component (see [components](#components-and-tags-as-inter-mod-communication)).

### Entities
`r.entity(&ENTITY)` and then:
- every entity gets a spawn egg by itself, `<name>_spawn_egg`, called after it and in the creative tab of eggs. Draw `item/<name>_spawn_egg.png` or it looks generic; `no_egg()` for what isn't a creature
- `size(w, h)`, with `eye(height)` when its eyes aren't at 85% of that, `health`, `speed`
- `hostile()` or `passive()` or `category(..)`
- `texture` (by default `entity/<name>`)
- `model(humanoid(..))`, or `r.set_model("name", "thing")` for a Blockbench geo.json (any item's or entity's, also someone else's, `"#tag"` for many)
- `r.bone_model(..)`, `.wearable_model(..)` on an item: a model on a bone. `r.bone_motion`, `c.bone_turn`, `c.bone_position`: bones moving by themselves or by code (`docs/models.md`)
- `sound("hurt")`, `sound("death")`, `sound("ambient")`: files from `sounds/entity/<name>/`
- `animations(..)`
- `goals(..)` for AI (see `weft_sdk::ai`); `ai::melee()` with `.attack_damage(3.0)` and `.reach(1.3)` makes it hit like a player does (the reach is the `ENTITY_REACH` attribute, so a modifier lengthens a mob's arms the same as a player's; `w.in_reach(id, target)` is the test), so `question::ATTACK` and `chain::ATTACK_DAMAGE` (a strength effect, a peaceful zone) work on mobs too
- what sets a mob off is on its goals: every mob leaves everyone alone until a trigger goes off (`ai::Trigger::sight(range)`, `hurt()`, `gaze(range)` for an enderman, `when(range, |w, mob, them| ..)`, each with `.only_if(|w, mob| ..)`, `.of(&KIND)`, `.of_tag(&TAG)` or `.of_any(|w, id, st| ..)` for other mobs, `hurt().by_anything()`), and then a reaction does whatever it should: `ai::react::anger(secs, max)` (`ai::melee`, `ai::chase_target` and `ai::target` go after the nearest, `ai::targets` gives all of them, up to `max` at once: a charge through five players), `react::fear(secs, max)` (`ai::panic`, `ai::flee` away from all it fears), `react::alert(radius, ..)` for the pack around, or any `|w, mob, who| ..` of yours (points toward a grudge, a call for help). `Goals::new().aggressive(16.0)` is sight and hurt to anger, `.neutral()` hurt alone, `.timid(3.0)` an animal's panic; a spider is `.react(Trigger::sight(35.0).only_if(not_in_daylight), react::anger(2.0, 1)).neutral()`. Anger and fear last while the mod runs, they aren't saved with the world
- `on_tick`: every entity of the kind at once, once per tick
- `on_hurt(|w, hurt| ..)`, `on_died(|w, death| ..)`: `actions::HURT` and `actions::DIE` for this kind. The damage itself is the `chain::DAMAGE` chain and `actions::HURT` can stop it, for any kind, yours or not. Every hit has a cause (`damage_causes::FALL`, `LAVA` ...); your own is `r.damage_cause(&RADIATION).bypasses_armor().death_message("death.mymod.radiation")`, and a kind tagged `immune/fall` or `immune/mymod:radiation` takes none of it. Healing is the same call with a negative amount (`w.heal` is that), going through `chain::HEAL` and the `ENTITY_HEALED` event instead
- riding: `.seat(Seat::at([0.0, 1.0, 0.0]).steers())` is a place to sit, as many as you like, each with its own point in the mount's frame (+Z ahead) and `.exit(Exit::offset(..))`, `Exit::nearest(2.0)` (the default), `Exit::stay()` or `Exit::call(|w, mount, rider, nearest| ..)` for where its rider gets off. Riders ride riders, as deep as you like, never in a ring. Who sits in a seat that `steers` steers it: a mob with its own AI (a zombie jockey), a player with its keys through `.steering(..)`, which runs on that player's client and gives what the mount does; `steer::walk_turn`, `walk_look`, `strafe`, `fly`, `jump` and `charged_jump` are ready, `.and(..)` puts two together, `Steering::new(|keys, mount| ..)` is your own. Nobody steering, its own AI moves it. `.rider_rules(RiderRules::new().hits(|w, mount, rider| ..).builds(..).hurt_reaches_riders(0.5))` says what riders may do, by any condition; `.riders_fall()` passes its falls on; `.ride_camera(blocks)` how far back the third-person camera goes; `.rideable()` gets on with a right click. From code `w.mount(rider, mount)`, `w.mount_seat(..)`, `w.dismount(rider)`, `w.vehicle(id)`, `w.riders(mount)`, and `actions::MOUNT` / `DISMOUNT` to refuse or follow them. Players get off with `core:dismount`, Shift unless rebound. A teleport, to another dimension too, takes a mount's riders along and a rider off its mount. Riders sit by their model's `ride` clip (legs forward on any two-legged one) instead of walking. Testmod's steed shows it all
- camera modes: `r.camera(&ORBIT).shot(shot::orbit(6.0)).key(&ORBIT_KEY, "O").transition(0.5, easing::SMOOTH)` is a camera of your own. A shot runs every frame on the client and says where the camera is and looks, from the keys, the player's eyes and last frame's camera; `shot::eyes`, `behind`, `orbit`, `free`, `top_down`, `fixed` and `path` (a cutscene through points) are ready, `Shot::new(flags, |a| a.out = ..)` is yours. Flags say what goes with it: `BODY`, `HAND`, `HUD`, `EFFECTS` (bob, sprint FOV, hurt tilt), `CLIP` (pulled in where a block is in the way, as third person), `SOLID` (a small box that bumps into blocks and stays where the world is loaded), `DETACHED` (keys and mouse move the camera, the player stands), `AIM`, `NO_INTERACT`, `CYCLE` (F5 goes through it), `ASK` (only where the server lets it). The engine's first person, behind, front and free camera (P) are modes too: `r.edit(&cameras::FRONT).without(camera_flags::CYCLE)` takes the front view out of F5. The free camera asks `question::CAMERA` first, allowed in a game mode that flies; `Verdict::Allow` lets it anywhere, `Deny` nowhere (`a.is_camera(&cameras::FREE)`). The server puts a player in one with `w.force_camera(player, &MODE, Some((2.0, easing::IN_OUT)))`, held until `w.release_camera(..)`, or `w.set_camera(..)` it may leave. `ease(easing::BOUNCE_OUT, t)` is the same curves for your own moves. The world still loads around the player, not the camera.
- `on_interact`: a player right clicked one (after `actions::INTERACT_ENTITY`); true if it did something, which swings the player's arm

Moving platforms: `solid()`, `kinematic()`, `carry_height(..)`, and
`blocks(&DECK)` with a template (`r.template(&DECK).fill("base:oak_planks", from, to)`) makes a flying structure players can build on; its box is what the blocks fill, no `size` needed. `EntityState.size` is an entity's box and eye height as they are now (crouching and `w.set_size` included), so `st.pos.y + st.size.eye` is where its eyes are (or just `st.eye()`).

### Block entities
A block with `.entity(|e| e.attach(..).inventory(n))` (data, a tick, slots) and `.opens(&MENU)` (a window on use), each on its own; see the [block entities guide](block_entities.md).\
Look at testmod's generator for an example.

### Liquids
`r.liquid(&LIQUID)` and then:
- `block(|b| ..)`: its block like any other, on top of what makes it a liquid: `b.translucent()` to see through it (water, otherwise drawn solid like lava), `tint`, `light`, `light_opacity`, `drag` (how fast things move inside: water 1, lava 0.4), `texture` (`<name>_still` by default), and its tags (`tag("water")`), which its item gets too
- `flow_delay`, `drop_off`, `slope_find`, `flows_up()`
- `renewable(n)`: a new source where n sources meet (water 2)
- `heat` (roughly Kelvin, 1000 and more burns)
- what can swim in it isn't the liquid's say: an entity kind's `SWIMS_IN` (names and `#tags`, base gives players `#water`) and the same on anything it wears, so a suit with `.set_text(&SWIMS_IN, "#lava")` swims in lava. It changes nothing else: a burning liquid keeps burning unless `actions::HURT` says no
- `fog(rgb, blocks)` and `overlay(texture, opacity)` or `no_overlay()`: what you see from inside. By default both come from its texture; set them when the inside should look different from the surface: `r.liquid(&TAR).fog(0x1A1408, 6.0).overlay("tar_inside", 0.4)`
- `bucket(..)`

Another mod's liquid: `r.edit(WATER.liquid()).fog(0x204020, 12.0)`, any of the above; what you don't set stays, and `.block(..)` there edits its block.

Reactions: `r.liquid_reaction("#hot", "#water").source_turns_into("obsidian")`, named `base:hot>water` for others' `edit` and `remove` (`.named(..)` for one of your choice).\
Every liquid is also an item of its name and tags, its count in millibuckets: what liquid slots (`Slots::liquid(capacity)`) hold and recipes count, see the [block entities guide](block_entities.md).

### World
- `r.biome(..)` with its climate, surface and tints (`.temperature(0.35..).surface(..).tint(&tints::GRASS, ..)`); terrain rules inherit them
- `r.dimension(..)` (`spawn`, `height`, `gravity`, `fixed_time`, `sky`, `bare`, or `generator` + `column` to fill it in code)
- weather is a value too: `r.weather(&ACID_RAIN).darkness(0.5).gray(0.8, 0.6, 0.9).sound("mymod:weather.acid").duration(2400, 6000).weight(1)` (`falls`, `lightning`, `shelters_from_sun`, or the whole `look`), and a dimension lists what comes there after clear weather, `.weathers(&[&weathers::RAIN, &weathers::THUNDER, &ACID_RAIN])`. `w.weather()` is the id now (`w.raining()`, `w.thundering()` read its flags), `w.set_weather(&weathers::THUNDER, ticks)` changes it, and `r.on_event(&WEATHER_TICK, ..)` hears every tick of it for acid on whoever stands outside. What falls is the biome's: `.precipitation(&ASH)` after `r.precipitation(&ASH)`, `.snows()` for snow
- `r.sky_body(&DIM, &OBJECT)`, `r.sky_stars`, `r.sky_dome`, `r.sky_clouds` for what is in its sky, the sun and the moon included; `OVERWORLD.sky(&MOON)` is one of them for `r.edit` and `r.remove`
- `r.terrain(&DIM)` for your dimension's terrain, `r.edit(OVERWORLD.terrain())` for another mod's
- `r.ore(block)`, in every dimension whose stone it replaces (`.dimension(..)` limits it). `.replaces_with("#granite", &GRANITE_IRON_ORE)` makes the same vein go on as another block where it meets granite, so veins don't stop at stone borders; `.exposed(0.5)` leaves out half of what would touch a cave, as vanilla's gold and diamonds. `r.edit(IRON_ORE.ore())` changes another mod's ore: what you set, its `replaces` and `replaces_with` added to its own
- `r.scatter(&TREES)` for trees and the like, `r.decoration(f)` for plants (named `<mod>:decoration`, `/2`..), in every dimension
- `r.template(&T)` (layers and keys, or `fill` boxes) for a shape of blocks, then `r.structure(&T)` (`T.structure()` to edit it), `.body(&T)` on an entity or `w.place_template(&T, pos)`
- `r.natural_spawn(&ENTITY).biome("#animals").light(7, 15)`, named `<mod>:<entity>`, `/2`..

The terrain has presets like hills, canyons, dunes, caves, lava lakes if you don't want to write noise by hand.\
Mars in testmod is built from presets only.\
Your own shapes are formulas (`Expr`) the GPU runs as fast as the presets, and `.field(..)` gives biome rules any value you make, not just temperature and humidity.\
It works on other mods' dimensions too, so adding a biome to base's overworld is a `r.edit(OVERWORLD.terrain()).rule_before("base:desert", ..)` away.

### Recipes
- `r.shaped(&KIND, out, count, &["iii", " s ", " s "], &[('i', "#ingots/iron"), ('s', "#stick")])`
- `r.shapeless(..)`

Recipe kinds are registered too (`r.recipe_kind`), a furnace is just a kind with stations.\
A recipe is named after its kind and output in the namespace of the mod making it: base's crafting recipe for torches is `base:crafting/torch`, a second one of base's for torches `base:crafting/torch/2`, and so on in the order it makes them; `.named("torch_from_coal")` gives it one of your choice. By that name other mods change it like anything else: `r.remove(&Recipe::named("base:crafting/torch"))`, or `r.edit(&SMELTING_ALL).hidden()` with `static SMELTING_ALL: Recipe = Recipe::named("base:smelting/*")`.

### Loot
`r.loot(&RUBY_ORE).pool(Pool::new().when(Cond::Harvested).rolls(1, 2).item("ruby"))` (a block's or entity's table is what it drops, `LootTable::named(..)` is a table of its own)\
A block's table is all it drops, there is no tool check in front of it: put `Cond::Harvested` on the pools that need the right tool (broken by the world, like a torch losing its wall, always counts as harvested).\
and other mods' tables without replacing them: `r.edit(ZOMBIE.loot()).pool(..)` with `ZOMBIE = EntityKind::named("base:zombie")` adds a pool, `.replace_drop("rotten_flesh", "mymod:brain")` and `.remove_drop(..)` change what any pool of it gives, `.clear()` empties it, and the pools are a list like any (`.remove_item(&comps::loot::POOLS, "#0")` takes out the first).

### Changing something of another mod
- `r.edit(&base::TORCH).light(5)`: only what you set changes, the rest stays (testmod edits its own torch this way). Blocks, items, biomes, dimensions and menus by their handle, and the parts of a block by its: `IRON_ORE.ore()`, `WATER.liquid()`, `ZOMBIE.loot()`, `OVERWORLD.terrain()`, `WELL.structure()`. A block's edit says of its item only what it sets (`item_texture`, `no_item()`).
- `r.replace(&base::TORCH)...`: registers it from scratch, nothing of theirs stays; for every kind with a builder (blocks, items, entities, biomes, dimensions, menus, particles, sounds, poses, game modes, weathers, damage causes, text places, sky objects, HUD elements, recipes, loot tables...).
- `r.remove(&base::RUBY_ORE)`: as if nobody had registered it.

Their handle, item and recipes point at yours.

## Hooking into the game
There are four mechanisms, try them in this order:

```rust
// Events which are batched, delivered once per tick and nobody can cancel them.
static LEVEL_UP: Event<LevelUp> = Event::named("level_up");
r.on_event(&LEVEL_UP, |b| for e in b.iter() { /* do something */ });
w.emit(&MY_EVENT, value);

// Questions, anyone may say no, any deny wins.
r.before_event(&actions::BREAK_BLOCK, |q| if is_protected(q.pos()) { Verdict::Deny } else { Verdict::Abstain });

// Value chains for damage, attack damage, or your own. Stages order them, `w.chain_explain(..)` and
// `--log modifiers=debug` show what changed what.
r.modify(&chain::DAMAGE, stage::MULTIPLY, |m| m.value *= 0.5);

// Attributes with timed modifiers like speed, max health, your own that are saved with the entity.
static MANA: Attribute = Attribute::named("mana");
r.attribute(&MANA, 100.0, 0.0, 1000.0);
```

Two helpers go with them. `w.emit_after(entity, ticks, &EVENT, value)` fires an event later, saved with the entity (base's animals calm down 3 seconds after being hurt, players' hunger pulses every 4); one per entity and event, so setting it again moves it, instead of counters in components. `r.world_tick(|w| ..)` runs once per tick in every region for logic that is nobody's own, `w.players()` being the players there.

Keeping someone quiet is a question too: `r.before_event(&actions::CHAT, |q| if q.text().contains("spam") { Verdict::Deny } else { Verdict::Abstain })`. Reading what was said is `r.on_event(&actions::CHAT, ..)` (`e.text()`, `e.player`).

### Built-in events
Actions, each a question and an event under one name (`r.before_event(&actions::X, ..)`, `r.on_event(&actions::X, ..)`): `BREAK_BLOCK`, `PLACE_BLOCK`, `USE_BLOCK`, `USE_ITEM`, `INTERACT_ENTITY`, `HURT`, `DIE`, `PICKUP_ITEM`, `ENTER_DIMENSION`, `CHAT`. Events alone: `PLAYER_JOINED`, `PLAYER_RESPAWNED`, `ENTITY_HEALED`, `WEATHER_TICK`. A question alone: `question::ATTACK`.\
base adds `base:smelted`.

### Systems and energy
Systems run over every entity with a component all at once, and that is how block entities tick:
`r.system(&BURNER, |batch| for (id, m, ticks) in batch.iter_mut() { .. })`. `ticks` is 1 unless it is catching up on time it missed (unloaded, the time jumped): then it should do that many ticks of work at once, see [machines](machines.md#catching-up).

Energy is a built-in engine contract, you can give a block entity the `core:energy` component, tag
cables with `conductor`, and every mod's generators and machines will work together.\
Networks balance once per tick.

## Text on the screen
There is one way to put text on someone's screen, `show`. The other three are the same call with different defaults:

```rust
w.hint(player, "mymod.no_mana", &[]);                              // a short line above the hotbar
w.chat(Who::All, "mymod.boss_up", &[Arg::Text(name)]);             // a line in the chat
w.title(Who::Dimension(w.dimension()), "mymod.round", &[Arg::Int(3)]); // big, over the crosshair
w.subtitle(Who::Near(w.dimension(), pos, 32.0), "mymod.get_ready", &[]); // under the crosshair
w.show(player, "mymod.timer", &[Arg::Int(left)])                   // by itself, anywhere
    .id("mymod:timer")
    .at(0.98, 0.02)
    .scale(2.0);
```

The first argument is a player's id or a `Who`. The text is sent at the end of the statement, and everything can be changed on any of them:
- `.at(x, y)` in fractions of the window. That point of the text lies at the same point of the window, so `.at(1.0, 0.0)` is exactly the top right corner. A hint with `.at` is no longer in the HUD's flow.
- `.scale(s)`: 1 is menu text, a subtitle is 2, a title 4.
- `.times(fade_in, stay, fade_out)` in seconds.
- `.color(0xRRGGBB)` for what doesn't color itself with `{#color}`.
- `.id("mymod:timer")`: new text with the same place and id replaces the old one, so a timer counts down in one chat line instead of filling the chat. An empty key with no args takes it away.

Players move or hide hints and titles in the HUD editor (`core:hint`, `core:title`, `core:subtitle`), so use those for anything a player might want elsewhere, and `.at` when the position is the point. A place of your own works the same: `r.text_place(&QUEST).hud(hud_anchor::TOP_RIGHT, 0).stacks()` makes one more element of the HUD, and `w.show_at(player, &QUEST, key, args)` puts text there; the engine's are in `text_places`, and `hint`, `title` and the rest are `show_at` with one of them. The client has the same calls for its own screen: `c.show`, `c.show_at`, `c.hint`, `c.title`, `c.subtitle`, `c.chat` (`c.say` sends a line as if typed). A check that reads the world can serve both sides: written over `&impl View` (`raining`, `day_time`, `block`, `light`, `entity`, `component_of`), it takes the server's `w` and the client's `c.view()` alike. `c.draw_screen_text` is different: it draws for one frame, for what you animate yourself.

A player's message goes through one mod, the last loaded that asked. Pass what they typed as `Arg::Text`, or players get to format the line:
```rust
r.chat_format(|f| f.set("mymod.chat", &[Arg::Text(rank(f.player())), Arg::Text(f.text())]));
```
Commands: `/title <who> title|subtitle|hint <text>`, `/title <who> show <x> <y> <size> <text>`, `/title <who> times <in> <stay> <out>`, `/title <who> clear|reset`, `/say`, `/msg`, `/tellraw <who> <text>`. Text in `/title` and `/tellraw` can use `{#color}` and the rest; what `/say`, `/msg` and the chat get stays as typed.

## Commands
`r.command("heal", "[amount:int] [who:targets]", |cmd| ..)`\
The usage string is the whole spec: the engine parses it, checks types, completes it with Tab and shows it in `/help`.\
Reading values by name: `cmd.int("amount")`, `cmd.targets("who")`.\
Answering: `cmd.reply("mymod.healed", &[Arg::Int(amount)])` with `mymod.healed = healed by {0}` in your lang files. The reply travels as the key and its arguments and each player's client formats it in its own language, so a server never sends English text. The same goes for gizmo labels (`w.draw_text(&GROUP, pos, key, &args, ..)`) and the client's `draw_text`/`draw_screen_text`.\
The client draws the same shapes for one frame: `c.draw_line/box/fill/text(None, ..)`, or `Some(&GROUP)` to show them only while that group is on.\
Who may run it goes by permission levels, the same for the engine's commands and yours: 0 everyone, 1 moderator, 2 game master, 3 admin, 4 the host. A command asks for level 2 unless you say otherwise, `r.command(..).permission(permission::EVERYONE)`, and players below it are refused and not offered it. `/op <who> [level]` hands them out, and for anything else your mod does, `w.permission(player)` tells a player's level and `w.set_permission(player, level)` changes it.

The callback gets a `ServerCommand`: `cmd.world()` is where it runs, and what it changes settles when it returns (neighbors updated, events delivered), like `/setblock`.`r.client_command` is the same for commands that run on the client that typed them: a `ClientCommand` with `cmd.client()` and no world.`r.command_branch("time", "dawn", ..)` adds a branch to someone else's command (`r.client_command_branch` for client ones like `/gizmo`), and a `r.command` named like one of the engine's (`/give`, `/tp`...) replaces it.

## World API

Positions come back as `BlockPos` (a block, `x`, `y`, `z` whole numbers) and `Vec3` (a point or a speed, in blocks); `p.offset(0, 1, 0)`, `p.above()`, `p.center()` and `v.block()` move between them. Anything that takes a position takes a plain array too: `w.set_block([x, y + 1, z], s)`.

Inside any callback you get a `World` (`w`):
- the world's seed (`seed`, and `seed_at(pos)`, `seed_of_cube(pos)` for a number that belongs to a place: what a chest holds when first opened, a villager's trades)
- blocks (`block`, `set_block`, `schedule_tick`, `prop`, `prop_int`, `prop_set`, `prop_cycle` for a lever or a repeater's delay); for anything big, `w.fill(shape, block)` fills a whole shape in one call (a crater with air). Shapes are values: `Shape::ball`, `cuboid`, `cylinder`, `ellipsoid`, `noisy`, `blast` (a ragged crater like vanilla's explosions leave), combined with `.union`, `.intersect` and `.subtract` (a geode is a ball minus a smaller one); `.keep(Filter::unbreakable())`, `.keep(Filter::liquids())` and `.only(Filter::tag("#stone"))` choose what changes. The same shapes pick entities (`w.entities_in(&shape, EntityFilter::any().targets().touching())`, a conveyor's top, an arrow's tip), outline themselves with `w.draw_shape` and place particles (`.area(&shape)` on a burst). The engine does a fill at the end of the tick on every core, a cube at a time, lights each cube once, loads what it opens up in players' views right away and gives the rest the change when it loads. Players see it appear whole. Hundreds of blocks of radius take well under a second, where `set_block` per block costs about 1.6 µs each. Testmod's dynamite uses it
- entities (`spawn`, `spawn_with(&KIND, pos, &Spawn::new().velocity(v))` for one that starts moving, `spawn_item` for a dropped stack (`spawn_with(&ITEM, ..)` with `.stack(s)` throws it), `entity`, `set_entity`, `damage`, `heal`, `game_mode`, `creative`, `outside_survival` for a mode that spares damage and hunger, `set_game_mode(player, &game_modes::CREATIVE)`, `entities_near`, `targets_near` for what can be hit, `navigate`, `walk`, `push` for a trampoline or a blast, players too). Speed and jump are attributes (`attribute::MOVEMENT_SPEED`, `JUMP_STRENGTH`) for mobs and players alike: `w.attribute_add_modifier(player, &MOVEMENT_SPEED, ..)` is a speed effect, and `attribute::FOV` widens or narrows a player's view (a zoom, a speed rush). `w.entity_modify(id)` changes what an entity is now, only what you name, players' clients following: `.pos(..)`, `.velocity(..)`, `.yaw(..)`, `.pitch(..)`, `.health(..)`, `.fall_distance(0.0)` (a mace's smash, a parachute), `.flying(true)`, `.fire(ticks)`, `.selected(slot)`. Testmod's mace is a whole weapon on top of that and `chain::ATTACK_DAMAGE` Reach and mining speed are attributes too (`attribute::BLOCK_REACH`, `ENTITY_REACH`, `MINING_SPEED`), and the client reads them: a modifier is all a long tool or a haste effect takes. `w.in_liquid(pos)` knows a liquid's surface, which the block alone doesn't tell. `w.raycast(from, dir, max, liquids)` finds the first block along any line (a turret, whether a mob sees a player), and `w.target_block(id, ..)` is that from an entity's eyes.
- what happens in the world comes as actions, a question before and an event after under one handle: `r.before_event(&actions::DIE, ..)` is a totem (denied, it lives on with one health), `r.on_event(&actions::INTERACT_ENTITY, ..)` hears every right click on an entity. A mod's own is `static WRENCH: Action<Wrenched> = Action::named("wrench")`, and `w.act(&WRENCH, &mut query, value)` asks, then tells everyone if nobody refused
- conditions are one predicate for music, text, loot and spawning: `r.condition(&BLOOD_MOON, |c| ..)` answers on whichever side asks (`c.world()` on the server, `c.client()` on the client, with `c.pos()` and `c.entity()`), and then `.when(&BLOOD_MOON)` on music or a spawn rule, `Cond::Condition(&BLOOD_MOON)` in a loot pool and `{if:mymod:blood_moon}` in text all use it. The engine's are in `conditions` (`DAY`, `NIGHT`, `UNDERGROUND`, `SURFACE`), and every game mode is one by its name (`Condition::named("core:creative")`, `{if:creative}`)
- poses are values too: `r.pose(&SIT).height(0.7).eye(0.62).speed(0.0).action("sit")` registers one (`tilt(pose_tilt::FACE_DOWN)` lays the model down), and `w.set_pose(id, Some(&SIT), None)` holds a mob or a player in it until `None` (with a facing in degrees it also stays still and turned so, the way a bed lays a player down). The engine still picks standing, crouching, swimming and crawling (`keys::CRAWL` held, or no room for more) by itself while nothing holds the pose
- game modes are values with abilities, the engine's three as well as yours: `r.game_mode(&ADVENTURE).breaks(Breaking::None).no_placing()` is an adventure mode, with `no_costs`, `invulnerable`, `flight(Flight::Allowed)`, `noclip`, `hidden`, `no_interacting`, `no_hud`, `creative_inventory`, `with(flags)` and `.modifier(&attribute, op, amount)` for the rest. `/gamemode mymod:adventure` and `{if:mymod:adventure}` know it at once, and it is saved by name
- for stacks, `s.add_wear(1)` wears one down, and in a block entity's tick `output.room_for(result)` and `output.merged(result, n)` are what an output slot takes
- inventories (`inventory(h).get/set/insert`, `inventory(h).group("fuel")`), where `h` is an entity id, a block position or `Holder::held(player, slot)` for a backpack, or `STORAGE.of(h)` for a storage it carries (`r.storage(&S, slots)`, an ender chest's slots in every player); a player's by the groups its inventory was declared with (`r.player_slots().group("hotbar", Slots::row(9).hotbar())..`, the mod loaded last wins), `core:equipment` for all equipment slots
- components, energy, menus
- particles (`w.spawn_particles(&SPARK, pos).count(20).speed(2.0).color(0xFF4020)`, from the client too). Particles live on the GPU, so what happens to one is data on its kind, not code: `on_land`, `on_wall`, `on_ceiling`, `on_enter_liquid`, `on_leave_liquid` and `on_expire` each take `When::any()`, `When::liquid()` or `When::tag("water")` and `Then::die()`, `stop()`, `slide()`, `bounce(0.4)` or `turn_into(&OTHER)` / `turn_into_still(&OTHER)`; the first rule that holds wins. Rain that rings on water and is gone elsewhere is `.on_enter_liquid(When::tag("water"), Then::turn_into_still(&RIPPLE)).on_land(When::any(), Then::die())`, an ember that bounces and dies into smoke `.on_land(When::any(), Then::bounce(0.4)).on_expire(When::any(), Then::turn_into(&SMOKE))`. At most 5 different tags over all particle rules of the game
- sounds (`w.play_sound(&S, pos, volume)`, `.pitch(2.0)` on it for an exact playback speed; `r.sound(&S).falloff(4.0, Falloff::Late(2))` when a sound should stay loud further out)
- game rules: settings of the world, `r.config(&HARD_MODE).bool(false).world()`, read with `HARD_MODE.get()`, changed with `/rule hard_mode 1`
- gizmos for debugging (`w.draw_line/box/text/fill(&GROUP, ..)`)
- and a few plain helpers: `block_center(p)`, `block_floor(p)`, `block_pos_of(pos)`, `st.eye()`, `yaw_toward(from, to)`, `yaw_dir(yaw)`, `look_dir(yaw, pitch)`, `ratio(a, b)`. `EntityState`'s yaw and pitch are radians; what you pass to the API (`ModelPart::rotation`, `set_pose` facing, `Client::set_look`) is in degrees

## Client side

- UI is built in code: `r.menu(&ABOUT).screen(|ui| ui.panel(..)).pauses()`, the engine's screens are in `screens::*` and its pause, death and settings screens use the same parts (inputs, sliders, items, `S::mono()`, `.scroll()`...)
- HUD elements are built with `r.hud_element(&MANA_BAR, hud_anchor::HOTBAR_LEFT, 0, |ui| ..)`
- a bar of icons is `ui.icons("mana", 10, |c| value)` (`gui/mana_full`, `_half`, `_empty`; a negative value hides it, so it shows when you say), and it moves however you tell it, each by its own value read every frame: `.shaking_by(|c| ..)` (0 still, 2 hard), `.shaking_below(4.0)`, `.flash_on_gain(0.5)` / `.flash_on_loss(1.0)` (what changed blinks, `mana_flash` lights the bar), `.preview(1.6, |c| ..)` (what it would be, fading in and out), `.overlay(|c| ..)` (a second row in `mana_overlay_*` icons on top), `.wave_while(2.0, |c| ..)`. Base's hearts and food are made of these: food shakes with no saturation, shows the saturation outlined in gold and what the food in hand would fill
- F3 panels are at `hud_anchor::DEBUG_LEFT`
- Looks come from textures named after the class (`gui/button.png`, `_hover` ...) and named colors in the language files (`color.button`), so a resource pack can reskin screens ([UI guide](ui.md#looks))
- Bind values instead of copying them: `ui.progress_bind("energy", style, |c| ..)`, `ui.text_bind(..)`

Also:
- key bindings, one declaration each: `r.key(&DASH, "R").on_client(|c| ..)` on the client (a chord works too: `"ctrl+R"`, `"shift+alt+F6"`; up to two bindings, each held, `toggle` or a `double` tap: `"ctrl+R, double KeyR"`; the player rebinds them the same way in Controls), or `.on_server(|p| ..)` and the engine carries the press to the server for you. They only act while the world has the input, typing in the chat doesn't trigger them. `c.key_pressed(&DASH)` / `c.key_held(&DASH)` read them in a frame hook; the engine's keys are `keys::*`
- messages both ways on a typed channel: `static BOOST: Channel<Boost> = Channel::named("boost")` with a `value!` struct (or `String`, `Vec<u8>`, `()`), `r.packet(&BOOST, |p, b| ..)` on the server (`p.reply(&OTHER, &v)` answers), `r.client_packet` on the client, `w.send` / `client.send`. Both ends must declare the same type, the engine checks it like event values
- `client.run_command("/tp ~ ~5 ~")` runs a command as the player and `client.command_reply(id)` gives its answer to your code instead of the chat (testkit drives its server half like this)
- frame hooks (`r.client_frame`)
- cosmetics of your mobs: `r.client_entity_frame(&MOB, |c, e| ..)` every frame, with its position, distance walked, age and death; `.own_effects()` on the kind turns off the engine's steps, idle noises and death smoke
- particles and screen effects from shader snippets in `assets/shaders`
- sounds with `r.sound(..).category(..)` (every category gets its own volume slider), music
- settings that show up in Mod Settings by themselves (`r.config(&SPARKS).bool(true)`); the player's own by default, `.server()` for the server's value (sent to its players on every change), `.world()` for one saved with the world. The engine's settings are the same kind of thing (`settings::FOV`, `settings::LANGUAGE`, key bindings `core:key.<action>`, all in `config/core.txt`): set one and the client applies it

## Debugging

`--log mod:mymod=debug` (or `/log` in chat) turns on your `debug!` / `trace!` lines.\
`info!(target: "pipes", ..)` gives you `mod:mymod::pipes` to filter on.

F3 shows your mod's time per tick. `/gizmo` draws debug shapes, your own groups included (`r.gizmo_group(&PIPES)`).

A panic in your callback disables that callback and names your mod in the log instead of taking the game down.\
Native crashes write a report into `crash-reports/` of the game folder (`run/` from the repo) with the mod that was running.

F3+T reloads textures, lang, shaders and UI without restarting.

## Things that make your and other modders' life easier
Don't hardcode other mods' blocks, use a tag or a component.\
Tag `#sand` allows other mods to work with your mod, whereas `base:sand` does not.

Don't do per-object work across the mod boundary in hot paths if a batch exists (systems, events, entity ticks get all entities of a kind at once).

If you had to work around the engine to do something reasonable, that is an engine flaw.\
Tell me about this instead of building a hack, so I can adjust the engine for those cases.

You can contact me on Discord: `@vortex_so`

## Guides for other systems
- [Textures](textures.md): animation, glow, tinted overlays
- [CTM](ctm.md): connected and varied block textures (glass, logs, random stone)
- [Redstone](redstone.md): wire on any face, gates, pistons, and blocks of your own that join in
- [Localization](localization.md): all text, item tooltips, rich text and effects, icons and meters, lines by tag, adding them from other mods and packs
- [Worldgen](worldgen.md): ores, structures, biomes, spawns, new dimensions and terrain, also in other mods' dimensions
- [Sky](sky.md): suns, moons, stars and skyboxes in any dimension, changing or removing the ones there
- [Models and animations](models.md): mob models in code or Blockbench, keyframes and actions, items in the hands, armor
- [UI](ui.md): screens, HUD, menus, looks, GUI textures
- [Block entities](block_entities.md): blocks with data, inventories, menus, recipes, energy
- [Shaders](shaders.md): text effects, panels, mobs, particles, screen effects, shader packs
- [Resource packs](resourcepacks.md): what a pack can replace and how it is laid out
