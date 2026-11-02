//! Entity models on the GPU: every kind's boxes and triangles in two static tables; per frame
//! the bones of every visible entity are posed on the CPU (parents, keyframes, the look) and each
//! layer (skin or something worn) is one instance, drawn instanced per kind.
//! Models and animations come from code or from files (`model.rs`); files win, so resource packs
//! can change any entity's model.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rayon::prelude::*;
use rustc_hash::FxHashMap;
use weft_abi::comp;
use weft_render::*;

use crate::assets::Assets;
use crate::block_model::{self, Display};
use crate::components::{ENTITY, ITEM};
use crate::content::content;
pub use crate::gpu_layout::Instance;
use crate::gpu_layout::{GpuCorner, GpuCuboid, rows};
use crate::model::{self, Animation, Bound, Drive, Motion, Playing, role};
use crate::registry::Part;

/// A kind's model and animations.
pub struct KindModel {
    pub parts: Vec<Part>,
    cuboids: (u32, u32),
    corners: (u32, u32),
    pub anims: Vec<Bound>,
    /// Per action, its clips: this kind's own and what is left of the stock one under them.
    actions: FxHashMap<String, Vec<u32>>,
    /// The same for what an item in hand brings along.
    items: FxHashMap<u32, FxHashMap<String, Vec<u32>>>,
    /// The actions the engine plays, by `Act`, resolved once: mobs by the thousand go by these.
    engine: [Vec<u32>; ACTS.len()],
    /// Texture size the model file was made for (it overrides the kind's).
    pub texture: Option<[u32; 2]>,
}

/// The actions the engine asks for on every entity.
#[derive(Clone, Copy)]
pub enum Act {
    Idle,
    Walk,
    Sneak,
    Swim,
    Crawl,
    Swing,
    Death,
}

const ACTS: [&str; 7] = ["idle", "walk", "sneak", "swim", "crawl", "swing", "death"];

impl Act {
    pub fn name(self) -> &'static str {
        ACTS[self as usize]
    }
}

impl KindModel {
    /// What plays for an engine action, nothing in hand or swapped.
    pub fn act(&self, a: Act) -> &[u32] {
        &self.engine[a as usize]
    }

    /// What plays for action `name`: the item's clips if it has them, else the kind's.
    pub fn action(&self, item: u32, name: &str) -> &[u32] {
        if item != 0 {
            if let Some(c) = self.items.get(&item).and_then(|t| t.get(name)) {
                return c;
            }
        }
        self.actions.get(name).map_or(&[], Vec::as_slice)
    }

    /// The item brings clips of its own for this action.
    pub fn has_own(&self, item: u32, name: &str) -> bool {
        self.items.get(&item).is_some_and(|t| t.contains_key(name))
    }

    /// The swing of what is in the hand: from a held pose a whole arm's swing goes over the head.
    pub fn swing_of(&self, item: u32) -> &'static str {
        if item != 0 && !self.has_own(item, "swing") { "swing_held" } else { "swing" }
    }

    /// `name.first_person` from the first person, when it exists.
    pub fn action_seen(&self, item: u32, name: &str, first_person: bool) -> &[u32] {
        if first_person {
            let own = self.action(item, &format!("{name}.first_person"));
            if !own.is_empty() {
                return own;
            }
        }
        self.action(item, name)
    }

    /// Adds `clips` to what plays this frame.
    pub fn play(&self, out: &mut Clips, clips: &[u32], how: &Play) {
        for &i in clips {
            let Some(b) = self.anims.get(i as usize) else { continue };
            let t = match b.anim.drive.unwrap_or(how.drive) {
                Drive::Clock => b.at(how.clock, how.looping),
                Drive::Distance => b.at(how.distance, true),
                Drive::Progress => how.progress.clamp(0.0, 1.0) * b.anim.length,
            };
            let speed = if how.by_speed || b.anim.by_speed { how.speed } else { 1.0 };
            let w = how.weight * b.anim.weight * speed;
            if w > 0.0 {
                out.push(PlayClip { anim: i, t, w, mirror: how.mirror, aim: how.aim });
            }
        }
    }
}

/// How the clips of an action play this frame.
#[derive(Clone, Copy)]
pub struct Play {
    pub drive: Drive,
    pub clock: f32,
    pub distance: f32,
    pub progress: f32,
    pub looping: bool,
    pub weight: f32,
    pub speed: f32,
    pub by_speed: bool,
    pub mirror: bool,
    pub aim: bool,
}

impl Default for Play {
    fn default() -> Play {
        Play {
            drive: Drive::Clock,
            clock: 0.0,
            distance: 0.0,
            progress: 0.0,
            looping: false,
            weight: 1.0,
            speed: 1.0,
            by_speed: false,
            mirror: false,
            aim: false,
        }
    }
}

/// How many clips play on one entity at most.
pub const MAX_CLIPS: usize = 12;

#[derive(Clone, Copy, Default)]
pub struct PlayClip {
    pub anim: u32,
    pub t: f32,
    pub w: f32,
    pub mirror: bool,
    pub aim: bool,
}

#[derive(Clone, Copy, Default)]
pub struct Clips {
    pub list: [PlayClip; MAX_CLIPS],
    pub n: usize,
}

impl Clips {
    fn push(&mut self, c: PlayClip) {
        if self.n < MAX_CLIPS {
            self.list[self.n] = c;
            self.n += 1;
        }
    }

    pub fn all(&self) -> &[PlayClip] {
        &self.list[..self.n]
    }
}

/// Layers one entity draws at most: its skin and what it wears.
pub const MAX_LAYERS: usize = 8;

/// One entity to pose this frame: kind, root transform, its look, where in `Models::clips` what
/// plays on it is, and its layers.
#[derive(Clone, Copy)]
pub struct Posed {
    pub kind: u32,
    pub root: Mat4,
    pub pitch: f32,
    pub head_yaw: f32,
    pub clips: (u32, u32),
    pub layers: [Instance; MAX_LAYERS],
    pub layer_count: usize,
}

#[derive(Clone, Copy)]
pub enum Batch {
    World,
    Preview,
    /// The first person's arms, after everything else.
    Hand,
}

pub struct Models {
    pub kinds: Vec<KindModel>,
    /// Items with a model file of bones: their place in `kinds`, past the entities', and texture.
    pub item_kinds: FxHashMap<u32, (u32, String)>,
    /// What plays on the entities of this frame (`Posed::clips`).
    pub clips: Vec<PlayClip>,
    /// Items whose model file says where they go in the hand.
    pub displays: FxHashMap<u32, Display>,
    cuboids: BufferId,
    corners: BufferId,
    /// Per frame slot, one set per [`Batch`]: the GPU reads them only when the frame's list
    /// runs, so two batches drawn in one frame can't share them. Bones can where they are the
    /// same bones (`bone_set`), and `uploaded` says they are in place.
    instances: [Vec<(BufferId, u32)>; 3],
    bones: [Vec<(BufferId, u32)>; 2],
    uploaded: Option<BufferId>,
    /// This frame: (kind, instance) and bone rows.
    queued: Vec<(u32, Instance)>,
    rows: Vec<[[f32; 4]; 3]>,
    scratch: Vec<Mat4>,
    /// Bones per entity in `add_many`.
    counts: Vec<usize>,
}

/// The kind's model: the file named by its `model` component, else `<ns>:<name>` if a model
/// file of that name exists, else its parts from code.
fn load_kind(assets: &Assets, k: u32) -> (Vec<Part>, Option<[u32; 2]>, Vec<Animation>) {
    let c = content();
    let kind = &c.game.entities[k as usize];
    let named = |key: &str| {
        c.components
            .id(key)
            .and_then(|id| c.components.text(ENTITY, k, id))
            .map(str::to_string)
    };
    let (mut parts, mut texture) = (kind.parts.clone(), None);
    let model_name = named(comp::MODEL).unwrap_or_else(|| kind.name.clone());
    if let Some(src) = read(assets, &model_name, "models", "geo.json") {
        match model::load_geo(&src) {
            Ok(g) => (parts, texture) = (g.parts, g.texture),
            Err(e) => log::error!("model {model_name}: {e}"),
        }
    }
    let anim_name = named(comp::ANIMATIONS).unwrap_or_else(|| kind.name.clone());
    (parts, texture, animations(assets, &anim_name))
}

fn read(assets: &Assets, name: &str, kind: &str, ext: &str) -> Option<String> {
    assets
        .read(name, kind, ext)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn animations(assets: &Assets, name: &str) -> Vec<Animation> {
    match read(assets, name, "animations", "animation.json") {
        Some(src) => model::load_animations(&src).unwrap_or_else(|e| {
            log::error!("animations {name}: {e}");
            Vec::new()
        }),
        None => Vec::new(),
    }
}

/// The engine's own animations for a model of this shape, which any of its bones go by unless
/// the kind has clips of its own for them.
fn stock_file(parts: &[Part]) -> &'static str {
    let has = |r: u32| parts.iter().any(|p| p.role == r);
    let arms = parts
        .iter()
        .filter(|p| p.role == role::ARM_LEFT || p.role == role::ARM_RIGHT);
    if has(role::LEG_SWEEP) {
        "core:stock_spider"
    } else if has(role::LEG_FRONT_LEFT) || has(role::LEG_FRONT_RIGHT) {
        "core:stock_quadruped"
    } else if arms.clone().any(|p| p.rest[0].abs() > 1.0) {
        "core:stock_biped_arms_out"
    } else if arms.count() > 0 || has(role::LEG_LEFT) || has(role::LEG_RIGHT) {
        "core:stock_biped"
    } else {
        "core:stock_other"
    }
}

const STOCK_FILES: [&str; 5] =
    ["core:stock_biped", "core:stock_biped_arms_out", "core:stock_quadruped", "core:stock_spider", "core:stock_other"];

/// A model's boxes and triangles into the two tables.
fn add_geometry(parts: &[Part], cuboids: &mut Vec<GpuCuboid>, corners: &mut Vec<GpuCorner>) {
    for (b, p) in parts.iter().enumerate() {
        for cu in &p.cuboids {
            let origin = [p.pivot[0] + cu.origin[0], p.pivot[1] + cu.origin[1], p.pivot[2] + cu.origin[2]];
            cuboids.push(GpuCuboid {
                origin,
                bone: b as u32,
                size: cu.size,
                inflate: cu.inflate,
                faces: cu.face_rects(),
                role: p.role,
                pad0: 0,
                pad1: 0,
                pad2: 0,
            });
        }
        corners.extend(
            p.mesh
                .iter()
                .map(|v| GpuCorner { pos: v.pos, bone: b as u32, uv: v.uv, role: p.role, pad: 0 }),
        );
    }
}

/// Binds one action's clips: `over` wins on the bones it names, `under` plays the rest.
fn bind(anims: &mut Vec<Bound>, parts: &[Part], over: &Arc<Animation>, under: &[u32]) -> Vec<u32> {
    let top = Bound::new(over.clone(), parts);
    let mut out = Vec::new();
    for &i in under {
        let rest = anims[i as usize].without(&top);
        out.push(anims.len() as u32);
        anims.push(rest);
    }
    out.push(anims.len() as u32);
    anims.push(top);
    out
}

/// A kind's clips, by action, from its own file over the stock one, then what items bring.
fn bind_kind(
    parts: &[Part],
    own: &[Arc<Animation>],
    stock: &[Arc<Animation>],
    items: &[(u32, Vec<Arc<Animation>>)],
) -> (Vec<Bound>, FxHashMap<String, Vec<u32>>, FxHashMap<u32, FxHashMap<String, Vec<u32>>>) {
    let mut anims: Vec<Bound> = Vec::new();
    let mut actions: FxHashMap<String, Vec<u32>> = FxHashMap::default();
    for a in stock {
        actions.insert(a.name.clone(), vec![anims.len() as u32]);
        anims.push(Bound::new(a.clone(), parts));
    }
    for a in own {
        let under = actions.remove(&a.name).unwrap_or_default();
        let top = bind(&mut anims, parts, a, &under);
        actions.insert(a.name.clone(), top);
    }
    let mut by_item = FxHashMap::default();
    if parts.iter().any(|p| p.role == role::ARM_RIGHT) {
        for (item, clips) in items {
            let mut table: FxHashMap<String, Vec<u32>> = FxHashMap::default();
            for a in clips {
                let under = actions.get(&a.name).cloned().unwrap_or_default();
                table.insert(a.name.clone(), bind(&mut anims, parts, a, &under));
            }
            by_item.insert(*item, table);
        }
    }
    (anims, actions, by_item)
}

/// Adds a model to the tables and the list of kinds; its place there.
fn push_kind(
    kinds: &mut Vec<KindModel>,
    tables: &mut (Vec<GpuCuboid>, Vec<GpuCorner>),
    parts: Vec<Part>,
    texture: Option<[u32; 2]>,
    clips: (&[Arc<Animation>], &[Arc<Animation>], &[(u32, Vec<Arc<Animation>>)]),
) -> u32 {
    let (c0, v0) = (tables.0.len() as u32, tables.1.len() as u32);
    add_geometry(&parts, &mut tables.0, &mut tables.1);
    let (anims, actions, items) = bind_kind(&parts, clips.0, clips.1, clips.2);
    let engine = ACTS.map(|n| actions.get(n).cloned().unwrap_or_default());
    kinds.push(KindModel {
        cuboids: (c0, tables.0.len() as u32 - c0),
        corners: (v0, tables.1.len() as u32 - v0),
        parts,
        anims,
        actions,
        items,
        engine,
        texture,
    });
    kinds.len() as u32 - 1
}

/// Which set of bone buffers a batch uses.
fn bone_set(batch: Batch) -> usize {
    matches!(batch, Batch::Preview) as usize
}

/// The head of a model and where its eyes are in its pixels, looking ahead: `eye` above the
/// ground, at the front of the head.
fn eyes(m: &KindModel, eye: f32) -> Option<(usize, Vec3)> {
    let i = m.parts.iter().position(|p| p.role == role::HEAD)?;
    let head = &m.parts[i];
    let front = head
        .cuboids
        .iter()
        .map(|c| c.origin[2])
        .fold(0.0f32, f32::min);
    Some((i, Vec3::new(head.pivot[0], eye * 16.0, head.pivot[2] + front)))
}

fn playing_of<'a, 'b>(m: &'a KindModel, clips: &'b [PlayClip]) -> impl Iterator<Item = Playing<'a>> + use<'a, 'b> {
    clips.iter().filter_map(|c| {
        m.anims
            .get(c.anim as usize)
            .map(|b| Playing { bound: b, t: c.t, w: c.w, mirror: c.mirror, aim: c.aim })
    })
}

impl Models {
    pub fn new(gpu: &mut dyn Backend, assets: &Assets) -> Models {
        let mut m = Models {
            kinds: Vec::new(),
            item_kinds: FxHashMap::default(),
            displays: FxHashMap::default(),
            cuboids: gpu.create_buffer(16, usage::STORAGE, "model cuboids"),
            corners: gpu.create_buffer(16, usage::STORAGE, "model corners"),
            instances: Default::default(),
            bones: Default::default(),
            uploaded: None,
            clips: Vec::new(),
            queued: Vec::new(),
            rows: Vec::new(),
            scratch: Vec::new(),
            counts: Vec::new(),
        };
        m.reload(gpu, assets);
        m
    }

    /// Reads every kind's model and animations again (files may have changed).
    pub fn reload(&mut self, gpu: &mut dyn Backend, assets: &Assets) {
        let mut tables = (Vec::new(), Vec::new());
        self.kinds.clear();
        let c = content();
        let stock: FxHashMap<&'static str, Vec<Arc<Animation>>> = STOCK_FILES
            .into_iter()
            .map(|n| (n, animations(assets, n).into_iter().map(Arc::new).collect()))
            .collect();
        let anim_comp = c.components.id(comp::ANIMATIONS);
        // An item's own file: named by its `animations` component, else after the item.
        let items: Vec<(u32, Vec<Arc<Animation>>)> = c
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, d)| {
                let name = anim_comp
                    .and_then(|id| c.components.text(ITEM, i as u32, id))
                    .unwrap_or(&d.name);
                let clips = animations(assets, name);
                (!clips.is_empty()).then(|| (i as u32, clips.into_iter().map(Arc::new).collect()))
            })
            .collect();
        for k in 0..c.game.entities.len() as u32 {
            let (parts, texture, own) = load_kind(assets, k);
            let own: Vec<Arc<Animation>> = own.into_iter().map(Arc::new).collect();
            let stocked = stock.get(stock_file(&parts)).map_or(&[][..], Vec::as_slice);
            push_kind(&mut self.kinds, &mut tables, parts, texture, (&own, stocked, &items));
        }
        // Items: where they sit in the hand, and those made of bones like an entity. Such a model
        // is posed by the holder's clips of the same names (a gun's slide in its `swing`).
        self.item_kinds.clear();
        self.displays.clear();
        for (i, d) in c.items.iter().enumerate().skip(1) {
            let Some((ns, path)) = d.name.split_once(':') else { continue };
            let file = format!("{ns}:item/{path}");
            let source = |name: &str| read(assets, name, "models", "json");
            if let Some(display) = block_model::load(&file, &source)
                .ok()
                .flatten()
                .and_then(|m| m.display)
            {
                self.displays.insert(i as u32, display);
            }
            let Some(src) = read(assets, &file, "models", "geo.json") else { continue };
            let geo = match model::load_geo(&src) {
                Ok(g) => g,
                Err(e) => {
                    log::error!("model {file}: {e}");
                    continue;
                }
            };
            let own = items
                .iter()
                .find(|t| t.0 == i as u32)
                .map_or(&[][..], |t| t.1.as_slice());
            let kind = push_kind(&mut self.kinds, &mut tables, geo.parts, geo.texture, (own, &[], &[]));
            self.item_kinds.insert(i as u32, (kind, file));
        }
        // Buffers may not be empty.
        let (mut cuboids, mut corners) = tables;
        cuboids.push(GpuCuboid::default());
        corners.push(GpuCorner::default());
        gpu.destroy_buffer(self.cuboids);
        gpu.destroy_buffer(self.corners);
        self.cuboids = gpu.create_buffer((cuboids.len() * size_of::<GpuCuboid>()) as u64, usage::STORAGE, "model cuboids");
        gpu.write_buffer(self.cuboids, 0, bytemuck::cast_slice(&cuboids));
        self.corners = gpu.create_buffer((corners.len() * size_of::<GpuCorner>()) as u64, usage::STORAGE, "model corners");
        gpu.write_buffer(self.corners, 0, bytemuck::cast_slice(&corners));
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        for b in [self.cuboids, self.corners].into_iter().chain(
            self.instances
                .iter_mut()
                .chain(&mut self.bones)
                .flat_map(|v| v.drain(..))
                .map(|b| b.0),
        ) {
            gpu.destroy_buffer(b);
        }
    }

    pub fn begin(&mut self) {
        self.queued.clear();
        self.rows.clear();
        self.clips.clear();
        self.uploaded = None;
    }

    /// Poses a model of `kind` (model pixels to `root`) and queues its layers.
    pub fn add(&mut self, kind: u32, root: Mat4, motion: &Motion, layers: impl IntoIterator<Item = Instance>) {
        let first = self.rows.len() as u32;
        if let Some(m) = self.kinds.get(kind as usize) {
            model::pose(&m.parts, root, motion, &mut self.scratch);
            self.rows.extend(self.scratch.iter().map(|b| rows(*b)));
        }
        self.queue(kind, first, layers);
    }

    /// Poses a model by clips alone, looking ahead; where its bones start.
    pub fn pose_clips(&mut self, kind: u32, root: Mat4, clips: &[PlayClip]) -> u32 {
        let first = self.rows.len() as u32;
        if let Some(m) = self.kinds.get(kind as usize) {
            let playing: Vec<Playing> = playing_of(m, clips).collect();
            let motion = Motion { pitch: 0.0, head_yaw: 0.0, aim_root: None, clips: &playing };
            model::pose(&m.parts, root, &motion, &mut self.scratch);
            self.rows.extend(self.scratch.iter().map(|b| rows(*b)));
        }
        first
    }

    /// Where the eyes of a model are with these clips, looking straight ahead.
    pub fn eye_point(&mut self, kind: u32, root: Mat4, clips: &[PlayClip], eye: f32) -> Option<Vec3> {
        let m = self.kinds.get(kind as usize)?;
        let (i, at) = eyes(m, eye)?;
        let playing: Vec<Playing> = playing_of(m, clips).collect();
        let motion = Motion { pitch: 0.0, head_yaw: 0.0, aim_root: None, clips: &playing };
        model::pose(&m.parts, root, &motion, &mut self.scratch);
        Some(self.scratch[i].transform_point3(at))
    }

    /// Where a standing model's arms are seen from through its own eyes, in its pixels: above and
    /// behind the eyes, or the hands are under the view with a shoulder at its edge. Vanilla has
    /// its hand as far out for the same reason, with no shoulder to answer for.
    pub fn view_point(&self, kind: u32, eye: f32) -> Option<Vec3> {
        eyes(self.kinds.get(kind as usize)?, eye).map(|e| e.1 + Vec3::new(-3.0, 5.0, 10.0))
    }

    /// How many bones are posed so far: where what comes next starts in `bone`.
    pub fn bones_so_far(&self) -> usize {
        self.rows.len()
    }

    /// A posed bone's transform.
    pub fn bone(&self, at: usize) -> Mat4 {
        let [a, b, c] = self.rows[at];
        Mat4::from_cols_array_2d(&[a, b, c, [0.0, 0.0, 0.0, 1.0]]).transpose()
    }

    /// The clips of model `to` that go with what plays on `holder` for the hand it is in, by
    /// name (a first person version counts as the action): same place in them.
    pub fn clips_along(&self, to: u32, holder: u32, clips: &[PlayClip], clock: f32, off: bool) -> Clips {
        let mut out = Clips::default();
        let (Some(m), Some(h)) = (self.kinds.get(to as usize), self.kinds.get(holder as usize)) else { return out };
        m.play(&mut out, m.act(Act::Idle), &Play { clock, looping: true, ..Play::default() });
        for c in clips.iter().filter(|c| c.mirror == off) {
            let Some(b) = h.anims.get(c.anim as usize) else { continue };
            let name = b
                .anim
                .name
                .strip_suffix(".first_person")
                .unwrap_or(&b.anim.name);
            for &i in m.action(0, name) {
                let Some(own) = m.anims.get(i as usize) else { continue };
                out.push(PlayClip { anim: i, t: c.t / b.anim.length * own.anim.length, w: c.w, mirror: false, aim: false });
            }
        }
        out
    }

    /// Moves an already posed bone by `by`.
    pub fn move_bone(&mut self, at: usize, by: Mat4) {
        self.rows[at] = rows(by * self.bone(at));
    }

    /// Starts another batch over the bones posed so far.
    pub fn begin_batch(&mut self) {
        self.queued.clear();
    }

    /// Queues layers of a model whose bones start at `first`.
    pub fn queue(&mut self, kind: u32, first: u32, layers: impl IntoIterator<Item = Instance>) {
        let Some(m) = self.kinds.get(kind as usize) else { return };
        for mut inst in layers {
            (inst.bones, inst.cuboids, inst.cuboid_count, inst.vertices) = (first, m.cuboids.0, m.cuboids.1, m.corners.0);
            self.queued.push((kind, inst));
        }
    }

    /// Poses many models at once, in parallel, straight into the bone table: per entity its kind,
    /// root, look, clips and layers. Nothing is allocated per entity (a thousand mobs are a
    /// thousand of each every frame).
    /// `own`: the one entity whose arms are in the view (`Motion::aim_root`).
    pub fn add_many(&mut self, entities: &[Posed], own: Option<(usize, Mat4)>) {
        let kinds = &self.kinds;
        let clips = &self.clips;
        let base = self.rows.len();
        let mut counts = std::mem::take(&mut self.counts);
        counts.clear();
        counts.extend(
            entities
                .iter()
                .map(|e| kinds.get(e.kind as usize).map_or(0, |m| m.parts.len())),
        );
        self.rows
            .resize(base + counts.iter().sum::<usize>(), [[0.0; 4]; 3]);
        let mut slices: Vec<&mut [[[f32; 4]; 3]]> = Vec::with_capacity(entities.len());
        let mut rest = &mut self.rows[base..];
        for &n in &counts {
            let (a, b) = rest.split_at_mut(n);
            slices.push(a);
            rest = b;
        }
        entities
            .par_iter()
            .enumerate()
            .zip(slices)
            .with_min_len(64)
            .for_each_init(
                || (Vec::new(), Vec::new()),
                |(scratch, list), ((i, e), out)| {
                    let Some(m) = kinds.get(e.kind as usize) else { return };
                    list.clear();
                    list.extend(playing_of(m, &clips[e.clips.0 as usize..e.clips.1 as usize]));
                    let aim_root = own.filter(|o| o.0 == i).map(|o| o.1);
                    model::pose(&m.parts, e.root, &Motion { pitch: e.pitch, head_yaw: e.head_yaw, aim_root, clips: list }, scratch);
                    for (o, b) in out.iter_mut().zip(scratch.iter()) {
                        *o = rows(*b);
                    }
                },
            );
        let mut first = base as u32;
        for (e, &n) in entities.iter().zip(&counts) {
            self.queue(e.kind, first, e.layers[..e.layer_count].iter().copied());
            first += n as u32;
        }
        self.counts = counts;
    }

    /// Draws what was queued since `begin`, one instanced draw per kind, with `pipe` and the
    /// view in `view`.
    pub fn draw(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        frame: &FrameInfo,
        batch: Batch,
        pipe: PipelineId,
        view: BufferId,
    ) {
        if self.queued.is_empty() {
            return;
        }
        self.queued.sort_by_key(|q| q.0);
        let grow = crate::gpu_layout::frame_buffer;
        let ib = grow(
            gpu,
            &mut self.instances[batch as usize],
            frame.slot,
            self.queued.len() as u32,
            size_of::<Instance>() as u64,
            usage::STORAGE,
            "model instances",
        );
        let insts: Vec<Instance> = self.queued.iter().map(|q| q.1).collect();
        gpu.mapped(ib)[..insts.len() * size_of::<Instance>()].copy_from_slice(bytemuck::cast_slice(&insts));
        let bb = match self.uploaded {
            Some(b) => b,
            None => {
                let b = grow(
                    gpu,
                    &mut self.bones[bone_set(batch)],
                    frame.slot,
                    self.rows.len().max(1) as u32,
                    48,
                    usage::STORAGE,
                    "model bones",
                );
                gpu.mapped(b)[..self.rows.len() * 48].copy_from_slice(bytemuck::cast_slice(&self.rows));
                self.uploaded = Some(b);
                b
            }
        };
        list.pipeline(pipe);
        list.bind(crate::gpu_layout::bind::model::INSTANCES, ib);
        list.bind(crate::gpu_layout::bind::model::SCENE, view);
        list.bind(crate::gpu_layout::bind::model::CUBOIDS, self.cuboids);
        list.bind(crate::gpu_layout::bind::model::CORNERS, self.corners);
        list.bind(crate::gpu_layout::bind::model::BONES, bb);
        let mut i = 0;
        while i < self.queued.len() {
            let kind = self.queued[i].0;
            let end = i + self.queued[i..].iter().take_while(|q| q.0 == kind).count();
            let m = &self.kinds[kind as usize];
            let vertices = m.cuboids.1 * 36 + m.corners.1;
            list.push(Cmd::Draw { vertices, instances: (end - i) as u32, first_vertex: 0, first_instance: i as u32 });
            i = end;
        }
    }
}
