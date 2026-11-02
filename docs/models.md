# Models and animations guide

How things are shaped: blocks and items from boxes, entities from bones, written in code or made in Blockbench, plus keyframe animations and actions, items in the hands, and armor drawn on the wearer.

## Note
Examples to copy from:
- `mods/testmod/assets/models/idol.geo.json` and `animations/idol.animation.json`: a model with mesh wings, an idle and a bow
- `mods/testmod/assets/animations/player.animation.json`: a wave for the player
- `mods/testmod/assets/animations/greatsword.animation.json`, `blaster.animation.json`: items held in two hands, one swinging with the whole body, one with a bone of its own
- `mods/testmod/src/looks.rs`: entities with models from code (`wisp`, `orb`) and from a file (`idol`)
- `mods/base/src/mobs.rs`: the `biped`, `quadruped` and `spider` mobs
- `mods/testmod/assets/models/block/pedestal.json` and `models/item/totem.json`: a block and an item shaped by files

## Blocks and items
A block or item made of boxes takes Blockbench's **Java Block/Item** model: export it as `models/block/<name>.json` or `models/item/<name>.json` in your mod and it is picked up by the name, no code needed. Entities take the **Bedrock Entity** format instead (below). Two formats because the two want different things: a block is boxes whose faces each show some texture and never moves, a mob is bones that turn, swing and play keyframes over one skin. Each format is the one Blockbench made for that job.

From a Java model the engine reads `elements` (`from`, `to`, `rotation`, and `faces` with their `texture`, `uv` and `rotation`), `textures` with `#name` references and `parent`, another model file or one of vanilla's cube parents (`block/cube_all`, `block/cube`, `block/cube_column`, `block/cube_bottom_top`) which give a plain cube its face textures. A face whose `#name` no texture sets shows the block's own texture of that face, so a file can bring only the shape and leave the textures to `.texture(..)` / `.textures(..)` in code. Faces the file leaves out aren't drawn. `uv` is 0 to 16 across a picture of any size, fractions included, so a face can be a part of an unwrap.

In code the same is `.model_box(min, max)` (pixels, 0..16), whose faces show the state's textures. A block's model decides its shape for everyone, collision and selection included (box by box, so a stair is one slab and a quarter, unless `.hitbox(min, max)`, in the same pixels, says otherwise).

For items, `models/item/<name>.json` with `"parent": "item/generated"` and a `layer0` texture is a flat item showing that texture; with `elements` the item is drawn as those boxes in the hand, on the ground and in slots. A face without a texture shows the item's own icon.

A resource pack replaces these files like any other: its `models/block/stone.json` in `base/` redraws stone, and the block's item with it unless the pack has a `models/item/stone.json`. Only the drawing changes; what a block collides with, how it is selected and which neighbors it hides stay what the mod gave every player, since a server knows nothing of packs. A `.ctm` can pick among models by position or neighbors, and let a block stop hiding its neighbors: [models as tiles](ctm.md#models-as-tiles).

## Quick links
[the basics](#the-basics) | [model in code](#model-in-code) | [model from Blockbench](#model-from-blockbench) | [roles](#roles) | [animations](#animations) | [items in the hand](#items-in-the-hand) | [armor](#armor-and-other-worn-items)

## The basics
An entity has a size, which is its hitbox, and a model, which is what gets drawn. The two are independent: `.size(0.6, 1.8)` is in blocks, and the model can be bigger or smaller than the box it lives in.

A model is a list of bones. Each one has a pivot (the point it turns around), boxes or loose triangles drawn around that point, and optionally a parent bone it moves with. Everything is measured in model pixels, 16 to a block, with y up from the feet and the model looking along -Z (north).

The skin lives in `textures/entity/<name>.png`, named like the entity, and so do its sounds in `sounds/entity/<name>/`:
```rust
static GHOST: Entity = Entity::named("ghost");
r.entity(&GHOST).size(0.6, 1.8).health(10.0).model(biped(false)).sound("hurt").sound("death");
```
`.texture("other")` picks another file under `entity/` (`"base:zombie"` for someone else's). The model says how big a skin its UVs are laid out on, so a resource pack can swap in a skin of twice the resolution and everything still lines up.\
`.sound("hurt")` plays one of `hurt*.ogg` (or `.wav`) from the entity's sound folder at random, `.sound_variants("hurt", &["groan", "growl"])` names the files. The events the engine plays are `hurt`, `death`, `ambient` and `step`.

## Model in code
`.model(..)` takes a `Model`, or a `Vec<Part>` laid out on a 64x32 skin (`Model::from(parts).uv(64, 64)` for another size). Three are ready made: `biped(arms_forward)` is the classic humanoid on a 64x32 skin, the player's own model too (`true` holds the arms out like a zombie), `quadruped(body, leg, head)` is an animal on a 64x64 skin with sizes in model pixels, and `spider()` is head, two body parts and eight legs.

For your own, a `Part` starts from a name (it decides the [role](#roles) and is what animations address) and a pivot, and collects boxes with `cube`:
```rust
Part::new("wisp", [0.0, 3.0, 0.0])
    .cube([-3.0, -3.0, -3.0], [6.0, 6.0, 6.0], [0.0, 0.0])
```
Everything you can do with a part:
- `cube(origin, size, uv)`: a box. `origin` is relative to the pivot, `uv` the top-left corner of its unwrap on the skin
- `mirrored()`: mirrors the last box's unwrap, handy for an arm that shares its texture with the other one
- `child_of(index)`: hangs it from the part at that index in the same list
- `rotation(x, y, z)`: a resting rotation in degrees
- `mesh(triangles)`: loose triangles in model pixels, three corners each. `sphere(center, radius, rings, segments, uv)` builds a ball

This is the orb from testmod, a ball with a ring of stones hanging off it:
```rust
let ring = Part::new("ring", [0.0, 8.0, 0.0]).child_of(0);
r.entity(&ORB).texture("orb").model(Model::from(vec![
    Part::new("core", [0.0, 8.0, 0.0]).mesh(sphere([0.0, 8.0, 0.0], 4.0, 8, 12, [0.0, 0.0, 16.0, 16.0])),
    ring.cube([6.0, -1.0, -1.0], [2.0, 2.0, 2.0], [16.0, 0.0]),
]).uv(32, 16));
```

## Model from Blockbench
Make the model as a Bedrock Entity, export it as `<name>.geo.json` and drop it into `assets/models/`. A file named like the entity is picked up on its own (`idol.geo.json` for `idol`), and `.model_file("name")` points at another one. Either way the file replaces any model written in code. Animations work the same way, from `animations/<name>.animation.json`.

The engine reads bones with their `pivot`, `parent` and resting `rotation`, boxes with `origin` (absolute, the way Blockbench writes it), `size`, `uv` (a pair or per face), `mirror` and `inflate`, and `poly_mesh` bones, triangles and quads alike, with normalized uvs if you use them. The geometry's `texture_width` and `texture_height` override the numbers you gave to `Model::uv`, so the skin can be any size.

Two things trip people up. A rotated box is drawn unrotated (the log warns about it), so put it in its own bone and rotate the bone. And list parent bones before their children, as Blockbench does. Molang expressions aren't evaluated, only plain numbers.

## Roles
A role tells the engine what a bone is: which stock clips move it, what armor covers it, where a hand is. It comes from the bone's name, in code and in Blockbench files alike (case and `_` don't matter), so the ready made models can be animated and worn on by the same names:

| bone name | role |
|---|---|
| `head` | follows where the entity looks |
| `leftArm`, `rightArm` (also `mainArm`, `offArm`) | swing while walking; resting held out (rotated about 90 degrees on x, a zombie's) they only sway |
| `body` | leans forward when sneaking |
| `leftLeg`, `rightLeg` (also `leg0`, `leg1`) | swing while walking |
| `leg2`, `leg3` | the front legs of a four-legged animal, opposite to the back ones |
| `leg0` .. `leg7` in a model with more than four legs | a spider's legs, sweeping sideways (left and right twins `leg0`/`leg1`, `leg2`/`leg3` ...) |
| anything else | stays still |

The player's model is `biped(false)` set by base (`r.entity_model(&Entity::named("core:player"), ..)` sets any kind's model, the engine's included); its sneaking pose comes from `body`, `head` and the arms and legs.

## Animations
One file holds any number of animations, and an animation's name is whatever follows the last dot, so `animation.idol.bow` is just `bow`:
```json
{
  "format_version": "1.8.0",
  "animations": {
    "animation.idol.bow": {
      "loop": "hold_on_last_frame",
      "animation_length": 0.8,
      "bones": {
        "head": { "rotation": { "0": [0, 0, 0], "0.4": [40, 0, 0] } }
      }
    }
  }
}
```
Each bone can carry `rotation` (degrees), `position` (model pixels) and `scale`, as keyframes at times in seconds. A keyframe is a plain `[x, y, z]` or an object with `pre`, `post` and `"lerp_mode": "catmullrom"` when you want a jump or a smooth curve. Between keys the value is linear. `loop` is `true` to repeat, `false` to play once and go back to normal, or `"hold_on_last_frame"` to stay in the last pose.

Keyframes are added on top of what is below them, and of a bone's resting angle, so even a walking leg can be animated. Set `"override_previous_animation": true` (Blockbench's Override box) and they go over what is below on the bones they name instead: `blend_weight` then says how much of that it hides, and its own angles stay what the file has. So 1 replaces it, and 0.9 on a greatsword's `hold` leaves a tenth of the walk on the arms, so they stir without dangling. A clip only ever touches the bones it names: arms only and the walk goes on, the whole body for a heavy swing and it doesn't. The angles follow Blockbench's convention: a Bedrock arm turned -90 degrees on x points forward.

Without override `blend_weight` scales the clip: a number, or `query.modified_move_speed` for how fast it walks. `anim_time_update` says what moves the clip's time: `query.modified_distance_moved` for steps, or anything with `progress` in it to stretch the clip over however long the action takes. Other Molang isn't evaluated.

### Actions
What plays goes by the name of an action, and that name is all there is to it. The engine looks for these by itself:

| action | when | time |
|---|---|---|
| `idle` | always | clock |
| `walk` | walking, more the faster | distance walked |
| `sneak`, `swim`, `crawl` | in that pose (`crawl` falls back on `swim`) | clock, distance |
| `hold` | something in hand (`hold_tool` for tools that don't have their own) | clock |
| `hold_empty` | nothing in hand, for `.first_person` only by default | clock |
| `equip` | the hand going down and up as it changes items or recharges | progress |
| `swing`, `use` | swinging (`swing_held` with an item that has no `swing` of its own), and using what it holds | progress |
| `death` | dying | clock |

The engine has clips for all of these (`assets/animations/stock_*.animation.json`), picked by the shape of the model: walking on two legs, four, eight. So any model with the right [bone names](#roles) walks without a single keyframe. Your own clip of the same name takes over on the bones it names, and the stock one keeps moving the others.

`mainArm` and `offArm` in a clip mean the arm with the item and the other one, whatever the model calls them. A clip for the item in the other hand plays mirrored, so you write each one once, for the main hand. There is also a bone no model needs: `root` moves the whole model, and `camera` moves the view of a player seeing it through their own eyes.

Anything else is yours to name. Started from code, everyone who sees the entity sees it:
```rust
world.play_action(entity_id, "bow", false);    // false plays once, true repeats
world.play_action(entity_id, "", false);       // back to normal
world.swap_action(creeper, "walk", "flee_cat"); // plays flee_cat wherever it would walk
world.unswap_action(creeper, "walk");
```
`swap_action` is the one for a mob's goal, and `ai::playing("walk", "flee_cat", goal)` does it for as long as the goal runs. Without a `flee_cat` clip it just goes on walking.

The file is found by the entity's name, or by `.animations("name")` when it is called something else.

### Animations for something that already exists
A file can also be attached to an entity that belongs to someone else, the player for example:
```rust
r.set_text_on(Target::Entity("core:player"), &ANIMATIONS, "mymod:player");
```
After that the player has the animations of your `animations/player.animation.json`, and `play_action(player, "wave", false)` plays one.

## Items in the hand
An item brings its own clips: `animations/<item>.animation.json`, or `.animations("name")` on the item. They are the same clips with the same action names, for whoever holds it. `hold` with `mainArm` and `offArm` both in it is a two-handed grip, and that's all a second arm takes: it comes up because the clip names it.

A `position` on an arm pulls it off its shoulder. A `hold` clip has its real angles, so what you see in Blockbench is what plays. The stock one has override and `blend_weight` 0.5: an arm holding something walks at half its swing, as in vanilla. Arms named by what is held follow the view: from the eyes they stay where they are on screen, and from outside they go half way with the look. The same clip and the same item size play seen from anywhere. With the First Person Model setting on, you see your real arms from your own eyes, exactly as others see them. With it off there is no body for them to hang from: an arm a held clip names leaves out the walking under it, stays put on screen and is seen from a little above and behind the eyes. If that view does need something else, `swing.first_person` plays there instead of `swing`, and so on for any action.

What an item may do in the hands is a property of the item:
```rust
r.item(&GREATSWORD).two_handed();                     // does nothing while the other hand holds something
r.item(&MAP).one_handed_as("hold_one");               // two hands, or one with this action
r.item(&BOW).use_priority(10);                        // goes before the other hand on a right click
```
A right click goes to the block first (not while sneaking), then to the hands by `use_priority`, the main hand if they are equal. The next hand gets its turn when the item does nothing, which `on_use` says by returning `false`: a bow with no arrows lets the torches in the other hand go down. Hits are for the main hand. The second hand itself is an equipment slot, `r.equipment_slot(&OFF_HAND, 0, "slot_offhand")`, and base registers it. A menu with `.equipment()` has it; its screen places it with `ui.group(OFF_HAND.name())`, as `ui.equipment()` draws only what is worn.

Where the item sits in the hand comes from `display.thirdperson_righthand` of `models/item/<name>.json`, which is Blockbench's Display tab, the same from the eyes and from outside; the other hand mirrors it. And `models/item/<name>.geo.json` makes the item out of bones (its texture is `textures/item/<name>.png`). Its origin is the palm with -Z along the arm, and its bones go in the same clips as the arms, so a gun's slide is one more bone in `swing`. On the ground and in the inventory it still shows its sprite.

`examples/examplemod/assets/models/player_template.geo.json` is the player's skeleton to open in Blockbench and animate on. Arms from the eyes are seen from 1.62 blocks up at the front of the head, plus 5 pixels up, 10 back and 3 to the left.

## Armor and other worn items
An item shows up on the wearer when it sits in an equipment slot. The slots are declared once, with the bones what is worn there covers and the silhouette of the empty slot, and an item only names its slot:
```rust
static HEAD: EquipmentSlot = EquipmentSlot::named("base:head");    // the base game's, or your own
r.equipment_slot(&BACK, worn_parts::BODY, "slot_back");             // a slot of your mod
r.item(&RUBY_HELMET).wearable(&HEAD, 1.0, "armor/ruby_1")
```
`worn_parts` are `HEAD`, `BODY`, `ARMS`, `LEGS`, combined with `|`. `wearable` takes how far the piece sticks out from the body in pixels (vanilla armor uses 1.0, and 0.5 for the leggings layer) and its texture.
The slot then takes the item in the inventory, shift-clicks put it there, and a new slot of any mod shows up in the inventory screen by itself (`.equipment()` on the menu, `ui.equipment()` on its screen). That texture is drawn on the same boxes as the wearer's, so it follows the wearer's skin layout, and the 64x32 files in `mods/base/assets/textures/entity/armor/` show how (the name is under `entity/`). It works on any model with the right roles, mobs made with `biped` included.

Items in the world and in the hand are flat sprites with some thickness, the boxes of a block, or [a model of their own](#blocks-and-items). `F3+T` reloads models and animations along with textures while the game runs.
