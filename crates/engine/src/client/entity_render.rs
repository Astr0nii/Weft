//! Entities: their models (`models.rs`, posed per frame and drawn instanced per kind), dropped
//! items, the held item, flames, the block selection outline, break progress and gizmo boxes
//! (textured boxes built on the CPU each frame, a few hundred at most).

use super::looks::ItemDraw;
use super::models::{MAX_LAYERS, Posed};
use crate::gpu_layout::box_mode;
use glam::{DVec3, IVec3, Mat4, Quat, Vec3, Vec4};
use rustc_hash::FxHashMap;

use super::models::{Act, Clips, Play};
use crate::block_model::Display;
use crate::gpu_layout::rows;
use crate::model::{Drive, role};
use weft_render::*;

use super::ClientEntity;
use super::models::{Instance, Models};
use super::textures::GameTextures;
use super::world_render::Camera;
use crate::assets::{Assets, decode_png};
use crate::content::content;
use crate::gpu_layout::{GpuBox, Scene};
use crate::physics::Hit;

/// What the first person view holds: the item (or the bare arm) and how it moves.
/// Cracks to draw: how far along, on `state`'s shape at `pos`, of what is at `at`.
#[derive(Clone, Copy)]
pub struct Crack {
    pub pos: IVec3,
    pub progress: f32,
    pub at: DVec3,
    pub state: u32,
}

pub struct Hand {
    /// How far it is lowered, 0..1: while changing items and just after a swing.
    pub lowered: f32,
    /// Radians the hand lags behind a turning camera: pitch, yaw.
    pub lag: (f32, f32),
    /// The body shows too, not only what the clips bring up.
    pub body: bool,
}

/// Opaque pixels of a sprite, for drawing it as a 1/16 thick solid like vanilla's held items.
struct Sprite {
    size: u32,
    /// Opaque pixels and which of their sides touch another one (left, right, below, above).
    pixels: Vec<(u8, u8, u32)>,
}

pub struct EntityRenderer {
    /// What the head's move in an action does to the camera: offset in view space, roll, pitch.
    pub camera_kick: (Vec3, f32, f32),
    pipeline: PipelineId,
    overlay: PipelineId,
    /// Model pipelines for the screen and for the inventory figure's image.
    model_pipe: PipelineId,
    /// The first person's arms: fixed field of view, depth squeezed so they never sink into walls.
    model_hand_pipe: PipelineId,
    preview_pipe: PipelineId,
    pub models: Models,
    buffers: Vec<(BufferId, u32)>,
    /// The held item's projection: a fixed field of view, like vanilla, so it keeps its shape
    /// whatever the player's setting.
    hand_views: Vec<BufferId>,
    /// The inventory figure's view; it is drawn into its own texture so the UI can layer it like
    /// any image.
    preview: Vec<BufferId>,
    /// Color, depth, size, bindless index.
    preview_target: Option<(TextureId, TextureId, [u32; 2], u32)>,
    /// Texture, bindless index and size, by name; and the glow masks of those that have one.
    skins: FxHashMap<String, Option<(TextureId, u32, [u32; 2])>>,
    /// Players' own skins by entity, replacing the player kind's texture on them.
    player_skins: FxHashMap<u64, (TextureId, u32, [u32; 2])>,
    /// Skins received and not uploaded yet.
    pub skin_pending: Vec<(u64, Vec<u8>)>,
    /// The local player's entity.
    pub local: u64,
    glows: FxHashMap<String, (TextureId, u32)>,
    /// Shader slot of each entity kind.
    kind_shaders: Vec<u32>,
    sprites: FxHashMap<String, Option<Sprite>>,
    /// Entities to pose this frame, kept between frames.
    posed: Vec<Posed>,
    /// `preload`'s kinds and items already looked at this frame.
    seen_kinds: Vec<bool>,
    seen_items: Vec<bool>,
    /// Item models drawn instanced: pipelines for the world and the hand, each item's boxes
    /// (first, count) in `item_boxes`, the GPU copy (buffer, capacity, boxes uploaded) and the
    /// texture generation they were made with.
    item_pipe: PipelineId,
    hand_item_pipe: PipelineId,
    item_models: FxHashMap<u32, (u32, u32)>,
    item_boxes: Vec<GpuBox>,
    item_buf: Option<(BufferId, u32, usize)>,
    item_generation: u64,
    /// This frame's item instances: (item, instance).
    instances: Vec<(u32, GpuBox)>,
}

fn shader(assets: &Assets) -> Result<Shader, ShaderError> {
    crate::packs::shader(assets, "entity.wgsl")
}

pub const HOOK: crate::shaders::Hook = crate::shaders::Hook {
    kind: "entity",
    entry: "shade",
    dispatch: "entity_shade",
    params: "p: EntityPixel",
    args: "p",
    ret: "vec4<f32>",
    fallback: "return vec4(p.albedo.rgb * p.light, p.albedo.a);",
};

/// Light on boxes and model layers drawn without any (the inventory figure, flames).
const FULL_LIGHT: u32 = u32::MAX;

/// Gives boxes pushed since `from` the light they stand in (the shader turns it into color).
fn light_boxes(boxes: &mut [GpuBox], from: usize, light: u32) {
    for b in &mut boxes[from..] {
        b.ids2[1] = light;
    }
}

impl EntityRenderer {
    pub fn new(gpu: &mut dyn Backend, assets: &Assets) -> Result<EntityRenderer, String> {
        let (pipeline, overlay) = Self::pipelines(gpu, assets)?;
        let ([model_pipe, preview_pipe, model_hand_pipe], kind_shaders) = Self::model_pipelines(gpu, assets)?;
        let models = Models::new(gpu, assets);
        let frames = gpu.frames_in_flight();
        let hand_views = (0..frames)
            .map(|_| gpu.create_buffer(size_of::<Scene>() as u64, usage::STORAGE | usage::HOST, "hand scene"))
            .collect();
        let preview = (0..frames)
            .map(|_| gpu.create_buffer(size_of::<Scene>() as u64, usage::STORAGE | usage::HOST, "preview scene"))
            .collect();
        let (item_pipe, hand_item_pipe) = Self::item_pipelines(gpu, assets)?;
        Ok(EntityRenderer {
            item_pipe,
            hand_item_pipe,
            item_models: FxHashMap::default(),
            item_boxes: Vec::new(),
            item_buf: None,
            item_generation: 0,
            instances: Vec::new(),
            seen_items: Vec::new(),
            pipeline,
            overlay,
            camera_kick: (Vec3::ZERO, 0.0, 0.0),
            model_pipe,
            model_hand_pipe,
            models,
            buffers: Vec::new(),
            hand_views,
            preview,
            preview_pipe,
            preview_target: None,
            skins: FxHashMap::default(),
            player_skins: FxHashMap::default(),
            skin_pending: Vec::new(),
            local: 0,
            glows: FxHashMap::default(),
            kind_shaders,
            sprites: FxHashMap::default(),
            posed: Vec::new(),
            seen_kinds: Vec::new(),
        })
    }

    /// Item models instanced in the world and in the hand.
    fn item_pipelines(gpu: &mut dyn Backend, assets: &Assets) -> Result<(PipelineId, PipelineId), String> {
        let s = shader(assets).map_err(|e| e.0)?;
        let mut pipe = |label, vertex| {
            gpu.create_pipeline(&PipelineDesc {
                label,
                shader: &s,
                vertex,
                fragment: Some("fs"),
                color: Some(Format::Surface),
                depth: Some(DepthState { write: true, compare: Compare::GreaterEqual }),
                blend: Blend::None,
                cull_back: true,
                topology: Topology::Triangles,
            })
            .map_err(|e| e.0)
        };
        Ok((pipe("items", "vs_item")?, pipe("hand item", "vs_hand_item")?))
    }

    fn pipelines(gpu: &mut dyn Backend, assets: &Assets) -> Result<(PipelineId, PipelineId), String> {
        let s = shader(assets).map_err(|e| e.0)?;
        let main = gpu
            .create_pipeline(&PipelineDesc {
                label: "entities",
                shader: &s,
                vertex: "vs",
                fragment: Some("fs"),
                color: Some(Format::Surface),
                depth: Some(DepthState { write: true, compare: Compare::GreaterEqual }),
                blend: Blend::None,
                cull_back: true,
                topology: Topology::Triangles,
            })
            .map_err(|e| e.0)?;
        let overlay = gpu
            .create_pipeline(&PipelineDesc {
                label: "overlay",
                shader: &s,
                vertex: "vs",
                fragment: Some("fs_overlay"),
                color: Some(Format::Surface),
                depth: Some(DepthState { write: false, compare: Compare::GreaterEqual }),
                blend: Blend::Alpha,
                cull_back: true,
                topology: Topology::Triangles,
            })
            .map_err(|e| e.0)?;
        Ok((main, overlay))
    }

    /// The model pipelines (screen, inventory figure image) with every entity shader snippet,
    /// and each kind's slot.
    fn model_pipelines(gpu: &mut dyn Backend, assets: &Assets) -> Result<([PipelineId; 3], Vec<u32>), String> {
        let snippets = crate::shaders::snippets(assets, &HOOK);
        let (s, mut slots) = crate::packs::compose(assets, "model.wgsl", &[(&HOOK, &snippets)]).map_err(|e| e.0)?;
        let slots = slots.pop().unwrap_or_default();
        let c = content();
        let comp = c.components.id(weft_abi::comp::SHADER);
        let kinds = (0..c.game.entities.len() as u32)
            .map(|k| {
                let name = comp.and_then(|id| c.components.text(crate::components::ENTITY, k, id));
                let slot = name.map_or(0, |n| slots.slot(n));
                if slot == 0 && name.is_some() {
                    log::warn!("entity {}: no entity shader {}", c.game.entities[k as usize].name, name.unwrap_or(""));
                }
                slot
            })
            .collect();
        let mut make = |label: &str, vertex: &str, color: Format| {
            gpu.create_pipeline(&PipelineDesc {
                label,
                shader: &s,
                vertex,
                fragment: Some("fs"),
                color: Some(color),
                depth: Some(DepthState { write: true, compare: Compare::GreaterEqual }),
                blend: Blend::None,
                cull_back: true,
                topology: Topology::Triangles,
            })
            .map_err(|e| e.0)
        };
        let (world, preview) = (make("models", "vs", Format::Surface)?, make("entity preview", "vs", Format::Rgba8Srgb)?);
        // A shader pack's older copy of the file has no `vs_hand`.
        let hand = make("model hand", "vs_hand", Format::Surface).or_else(|_| make("model hand", "vs", Format::Surface))?;
        Ok(([world, preview, hand], kinds))
    }

    pub fn reload(&mut self, gpu: &mut dyn Backend, assets: &Assets) {
        match Self::model_pipelines(gpu, assets) {
            Ok(([p, q, h], kinds)) => {
                gpu.destroy_pipeline(self.model_pipe);
                gpu.destroy_pipeline(self.model_hand_pipe);
                gpu.destroy_pipeline(self.preview_pipe);
                (self.model_pipe, self.preview_pipe, self.model_hand_pipe) = (p, q, h);
                self.kind_shaders = kinds;
            }
            Err(e) => log::error!("model shader: {e}"),
        }
        self.models.reload(gpu, assets);
        // Skins and masks are read again the next time they are used.
        for t in self
            .skins
            .drain()
            .filter_map(|(_, v)| v)
            .map(|v| v.0)
            .chain(self.glows.drain().map(|(_, g)| g.0))
        {
            gpu.destroy_texture(t);
        }
        if let Ok((a, b)) = Self::item_pipelines(gpu, assets) {
            gpu.destroy_pipeline(self.item_pipe);
            gpu.destroy_pipeline(self.hand_item_pipe);
            (self.item_pipe, self.hand_item_pipe) = (a, b);
        }
        // Sprites and the models made of them are read again.
        self.sprites.clear();
        self.item_models.clear();
        self.item_boxes.clear();
        match Self::pipelines(gpu, assets) {
            Ok((a, b)) => {
                gpu.destroy_pipeline(self.pipeline);
                gpu.destroy_pipeline(self.overlay);
                self.pipeline = a;
                self.overlay = b;
            }
            Err(e) => log::error!("entity shader: {e}"),
        }
    }

    /// Uploads player `entity`'s own skin.
    pub fn set_skin(&mut self, gpu: &mut dyn Backend, entity: u64, png: &[u8]) {
        let Some(img) = crate::assets::decode_png(png) else { return };
        let t = gpu.create_texture(
            &TextureDesc { width: img.width, height: img.height, mips: 1, format: Format::Rgba8Srgb, render_target: false },
            "player skin",
        );
        gpu.write_texture(t, 0, &img.rgba);
        if let Some(old) = self
            .player_skins
            .insert(entity, (t, gpu.texture_index(t), [img.width, img.height]))
        {
            gpu.destroy_texture(old.0);
        }
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        for (t, ..) in self.player_skins.drain().map(|(_, v)| v) {
            gpu.destroy_texture(t);
        }
        for (b, _) in self.buffers.drain(..) {
            gpu.destroy_buffer(b);
        }
        self.models.destroy(gpu);
        gpu.destroy_pipeline(self.model_pipe);
        gpu.destroy_pipeline(self.model_hand_pipe);
        gpu.destroy_pipeline(self.preview_pipe);
        if let Some((c, d, ..)) = self.preview_target.take() {
            gpu.destroy_texture(c);
            gpu.destroy_texture(d);
        }
        let preview: Vec<BufferId> = self.preview.drain(..).collect();
        for b in self.hand_views.drain(..).chain(preview) {
            gpu.destroy_buffer(b);
        }
        for t in self
            .skins
            .drain()
            .filter_map(|(_, v)| v)
            .map(|v| v.0)
            .chain(self.glows.drain().map(|(_, g)| g.0))
        {
            gpu.destroy_texture(t);
        }
        gpu.destroy_pipeline(self.pipeline);
        gpu.destroy_pipeline(self.overlay);
        gpu.destroy_pipeline(self.item_pipe);
        gpu.destroy_pipeline(self.hand_item_pipe);
        if let Some((b, ..)) = self.item_buf.take() {
            gpu.destroy_buffer(b);
        }
    }

    /// Loads skins and worn textures the first time something visible uses them; `worn` is what
    /// the local player wears.
    pub fn preload(&mut self, gpu: &mut dyn Backend, assets: &Assets, entities: &FxHashMap<u64, ClientEntity>, worn: &[u32], held: &[u32]) {
        let c = content();
        for &i in held {
            self.load_held(gpu, assets, i);
        }
        // Each kind and item looked at once per frame, without listing thousands of entities.
        let mut seen = std::mem::take(&mut self.seen_kinds);
        seen.clear();
        seen.resize(c.game.entities.len(), false);
        let mut seen_items = std::mem::take(&mut self.seen_items);
        seen_items.clear();
        seen_items.resize(c.items.len(), false);
        for e in entities.values() {
            if !e.item.is_empty() && seen_items.get(e.item.item as usize).is_some_and(|s| !s) {
                seen_items[e.item.item as usize] = true;
                self.load_sprite(assets, e.item.item);
            }
            if !std::mem::replace(&mut seen[e.look() as usize], true) {
                self.load(gpu, assets, &c.game.entities[e.look() as usize].texture);
            }
            for &i in &e.worn {
                if let Some((_, t)) = c.worn(i) {
                    self.load(gpu, assets, t);
                }
            }
            for i in [e.held, e.off_held] {
                if i != 0 && seen_items.get(i as usize).is_some_and(|s| !s) {
                    seen_items[i as usize] = true;
                    self.load_held(gpu, assets, i);
                }
            }
        }
        self.seen_kinds = seen;
        self.seen_items = seen_items;
        self.load(gpu, assets, &c.game.entities[c.game.player_kind as usize].texture);
        for &i in worn {
            if let Some((_, t)) = c.worn(i) {
                self.load(gpu, assets, t);
            }
        }
    }

    /// What drawing an item in a hand takes.
    fn load_held(&mut self, gpu: &mut dyn Backend, assets: &Assets, item: u32) {
        self.load_sprite(assets, item);
        if let Some(skin) = self.models.item_kinds.get(&item).map(|k| k.1.clone()) {
            self.load(gpu, assets, &skin);
        }
    }

    /// The opaque pixels of an item's sprite, read the first time it is held or dropped.
    fn load_sprite(&mut self, assets: &Assets, item: u32) {
        let Some(name) = sprite_name(item) else { return };
        if self.sprites.contains_key(&name) {
            return;
        }
        let img = assets
            .read(&name, "textures", "png")
            .and_then(|b| decode_png(&b));
        let sprite = img.map(|img| {
            let size = img.width;
            // Animated strips: the first frame.
            let solid = |x: i64, y: i64| {
                (0..size as i64).contains(&x)
                    && (0..size as i64).contains(&y)
                    && img
                        .rgba
                        .get((y as usize * size as usize + x as usize) * 4 + 3)
                        .is_some_and(|&a| a >= 128)
            };
            let pixels = (0..size * size)
                .map(|i| ((i % size) as i64, (i / size) as i64))
                .filter(|&(x, y)| solid(x, y))
                .map(|(x, y)| {
                    (
                        x as u8,
                        y as u8,
                        solid(x - 1, y) as u32
                            | (solid(x + 1, y) as u32) << 1
                            | (solid(x, y + 1) as u32) << 2
                            | (solid(x, y - 1) as u32) << 3,
                    )
                })
                .collect();
            Sprite { size, pixels }
        });
        self.sprites.insert(name, sprite);
    }

    /// An item's model as boxes in the unit cube, made once: a block item's shape, else its
    /// sprite one texel thick (flat when the sprite could not be read). (first box, count).
    fn item_model(&mut self, textures: &GameTextures, item: u32) -> Option<(u32, u32)> {
        if let Some(&m) = self.item_models.get(&item) {
            return Some(m);
        }
        let c = content();
        let first = self.item_boxes.len();
        let white = Vec4::new(1.0, 1.0, 1.0, 0.0);
        let looks = super::looks::current();
        let mut sprite_color = white;
        let sprite = match looks.item_draw(c, item) {
            ItemDraw::Model(model) => {
                boxes_shape(&mut self.item_boxes, textures, Mat4::IDENTITY, model, &c.states[0], white);
                None
            }
            ItemDraw::Icon(t) => Some(t),
            ItemDraw::Block(state) => {
                let s = &c.states[state as usize];
                let sprite = looks.block_sprite(state, s);
                if sprite.is_none() {
                    boxes_shape(&mut self.item_boxes, textures, Mat4::IDENTITY, &looks.block_shape(state, s), s, white);
                }
                if crate::content::face_paint(s, 0, false).base_tinted {
                    let [r, g, b, _] = (crate::content::item_tint(s.tint) << 8)
                        .to_be_bytes()
                        .map(|v| v as f32 / 255.0);
                    sprite_color = Vec4::new(r, g, b, 0.0);
                }
                sprite
            }
            ItemDraw::Nothing => return None,
        };
        if let Some(t) = sprite {
            let l = textures.texture(t);
            match sprite_name(item)
                .and_then(|n| self.sprites.get(&n))
                .and_then(|s| s.as_ref())
            {
                Some(sp) => {
                    let n = sp.size as f32;
                    // Only the outline: a side face against a neighboring pixel would fight
                    // with it. Mode 4: every face takes the color of this one texel.
                    for &(x, y, hidden) in &sp.pixels {
                        let m = Mat4::from_translation(Vec3::new(x as f32 / n, 1.0 - (y as f32 + 1.0) / n, 7.5 / 16.0))
                            * Mat4::from_scale(Vec3::new(1.0 / n, 1.0 / n, 1.0 / 16.0));
                        self.item_boxes.push(GpuBox {
                            rows: rows(m),
                            uvo: [(x as f32 + 0.5) / n, (y as f32 + 0.5) / n, 0.0, 0.0],
                            color: sprite_color.to_array(),
                            ids: [0, box_mode::TEXEL, l, hidden],
                            ..Default::default()
                        });
                    }
                }
                None => {
                    let m = Mat4::from_translation(Vec3::new(0.0, 0.0, 0.5)) * Mat4::from_scale(Vec3::new(1.0, 1.0, 0.0));
                    self.item_boxes.push(GpuBox {
                        rows: rows(m),
                        color: sprite_color.to_array(),
                        ids: [0, box_mode::FACES, l | l << 16, l | l << 16],
                        ids2: [l | l << 16, 0, 0, 0],
                        ..Default::default()
                    });
                }
            }
        }
        let m = (first as u32, (self.item_boxes.len() - first) as u32);
        self.item_models.insert(item, m);
        Some(m)
    }

    /// Item models made since the last upload go to the GPU; a new texture generation remakes
    /// them all.
    fn upload_items(&mut self, gpu: &mut dyn Backend) {
        let n = self.item_boxes.len();
        let (buf, cap, done) = match self.item_buf {
            Some((b, cap, done)) if (n as u32) <= cap => (b, cap, done.min(n)),
            old => {
                if let Some((b, ..)) = old {
                    gpu.destroy_buffer(b);
                }
                let cap = (n as u32).max(1024).next_power_of_two();
                (gpu.create_buffer(cap as u64 * 128, usage::STORAGE, "item models"), cap, 0)
            }
        };
        if done < n {
            gpu.write_buffer(buf, done as u64 * 128, bytemuck::cast_slice(&self.item_boxes[done..]));
        }
        self.item_buf = Some((buf, cap, n));
    }

    fn load(&mut self, gpu: &mut dyn Backend, assets: &Assets, name: &str) {
        if name.is_empty() || self.skins.contains_key(name) {
            return;
        }
        let loaded = assets
            .read(name, "textures", "png")
            .and_then(|b| decode_png(&b))
            .map(|img| {
                let t = gpu.create_texture(
                    &TextureDesc { width: img.width, height: img.height, mips: 1, format: Format::Rgba8Srgb, render_target: false },
                    name,
                );
                gpu.write_texture(t, 0, &img.rgba);
                (t, gpu.texture_index(t), [img.width, img.height])
            });
        if loaded.is_none() {
            log::warn!("missing entity texture {name}");
        }
        if let Some(mask) = crate::assets::emission_mask(assets, name) {
            let t = gpu.create_texture(
                &TextureDesc { width: mask.width, height: mask.height, mips: 1, format: Format::R8Unorm, render_target: false },
                name,
            );
            gpu.write_texture(t, 0, &mask.rgba);
            self.glows
                .insert(name.to_string(), (t, gpu.texture_index(t)));
        }
        self.skins.insert(name.to_string(), loaded);
    }

    /// Model instances of `entity`, of `kind`: its skin (a player's own if they sent one) and what
    /// it wears (how many in `.1`).
    fn layers(&self, kind: u32, entity: u64, worn: &[u32], light: u32, hurt: f32) -> ([Instance; MAX_LAYERS], usize) {
        let c = content();
        let k = &c.game.entities[kind as usize];
        let mut out = ([Instance::default(); MAX_LAYERS], 0);
        let Some(Some((_, skin, _))) = self.skins.get(&k.texture) else { return out };
        let glow = |name: &str| self.glows.get(name).map_or(u32::MAX, |g| g.1);
        let size = self
            .models
            .kinds
            .get(kind as usize)
            .and_then(|m| m.texture)
            .unwrap_or(k.texture_size);
        let own = self
            .player_skins
            .get(&entity)
            .filter(|_| kind == c.game.player_kind);
        let (skin, size) = own.map_or((skin, size), |o| (&o.1, o.2));
        let base = Instance {
            color: [1.0; 3],
            light,
            hurt,
            tex: *skin,
            tex_size: [size[0] as f32, size[1] as f32],
            roles: u32::MAX,
            shader: self.kind_shaders.get(kind as usize).copied().unwrap_or(0),
            glow: glow(&k.texture),
            ..Default::default()
        };
        out.0[0] = base;
        out.1 = 1;
        for &i in worn {
            if let Some((w, t)) = c.worn(i) {
                if let Some(Some((_, idx, size))) = self.skins.get(t).filter(|_| out.1 < MAX_LAYERS) {
                    out.0[out.1] = Instance {
                        tex: *idx,
                        tex_size: [size[0] as f32, size[1] as f32],
                        roles: crate::model::role_mask(w.roles),
                        inflate: w.inflate,
                        glow: glow(t),
                        ..base
                    };
                    out.1 += 1;
                }
            }
        }
        out
    }

    /// Renders the local player's model into a `size` texture (physical pixels), turned towards
    /// `look` (relative to the texture's top left) like the inventory figure. Returns the
    /// texture's bindless index for the UI to draw.
    pub fn draw_preview(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        frame: &FrameInfo,
        size: [u32; 2],
        look: glam::Vec2,
        worn: &[u32],
    ) -> Option<u32> {
        let c = content();
        if size[0] == 0 || size[1] == 0 {
            return None;
        }
        if self.preview_target.is_none_or(|t| t.2 != size) {
            if let Some((col, dep, ..)) = self.preview_target.take() {
                gpu.destroy_texture(col);
                gpu.destroy_texture(dep);
            }
            let desc = |format| TextureDesc { width: size[0], height: size[1], mips: 1, format, render_target: true };
            let col = gpu.create_texture(&desc(Format::Rgba8Srgb), "preview color");
            let dep = gpu.create_texture(&desc(Format::Depth32), "preview depth");
            self.preview_target = Some((col, dep, size, gpu.texture_index(col)));
        }
        let (col, dep, _, index) = self.preview_target?;
        let (x, y, w, h) = (0.0, 0.0, size[0] as f32, size[1] as f32);
        let (sw, sh) = (w, h);
        let s = h / 2.4;
        let (cx, feet) = (x + w / 2.0, y + h - h * 0.08);
        let head = feet - 1.5 * s;
        let yaw = std::f32::consts::PI + ((look.x - cx) / (s * 1.6)).atan() * 0.7;
        let pitch = ((head - look.y) / (s * 1.6)).atan() * 0.5;
        let proj = Mat4::from_cols(
            Vec4::new(2.0 * s / sw, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 2.0 * s / sh, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 0.1, 0.0),
            Vec4::new(2.0 * cx / sw - 1.0, 1.0 - 2.0 * feet / sh, 0.5, 1.0),
        );
        let layers = self.layers(c.game.player_kind, self.local, worn, FULL_LIGHT, 0.0);
        self.models.begin();
        let root = Mat4::from_rotation_y(yaw) * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
        self.models.add(
            c.game.player_kind,
            root,
            &crate::model::Motion { pitch, head_yaw: 0.0, aim_root: None, clips: &[] },
            layers.0[..layers.1].iter().copied(),
        );
        let vb = self.preview[frame.slot];
        // Full daylight, no fog.
        let v = Scene { view_proj: proj.to_cols_array_2d(), params: [1e9, 2e9, 1.0, crate::shaders::time()], ..Default::default() };
        gpu.mapped(vb)[..size_of::<Scene>()].copy_from_slice(bytemuck::bytes_of(&v));
        list.push(Cmd::BeginPass { color: Some((Target::Texture(col), Some([0.0; 4]))), depth: Some((dep, Some(0.0))) });
        self.models
            .draw(gpu, list, frame, super::models::Batch::Preview, self.preview_pipe, vb);
        list.push(Cmd::EndPass);
        Some(index)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        frame: &FrameInfo,
        cam: &Camera,
        entities: &FxHashMap<u64, ClientEntity>,
        me: Option<&ClientEntity>,
        hand: Option<Hand>,
        textures: &GameTextures,
        light: &impl Fn(IVec3) -> u32,
        scene: &Scene,
        scene_buf: BufferId,
        target: Option<(Hit, DVec3)>,
        breaking: &[Crack],
        gizmo_lines: &[super::gizmo::Line],
        gizmo_fills: &[super::gizmo::Fill],
        late: &mut CommandList,
    ) {
        let c = content();
        let aspect = frame.width as f32 / frame.height.max(1) as f32;
        let mut boxes: Vec<GpuBox> = Vec::new();
        if textures.generation() != self.item_generation {
            self.item_generation = textures.generation();
            self.item_models.clear();
            self.item_boxes.clear();
            if let Some(b) = &mut self.item_buf {
                b.2 = 0;
            }
        }
        let mut instances = std::mem::take(&mut self.instances);
        instances.clear();
        let rel = |p: DVec3| (p - cam.pos).as_vec3();
        let shade = |p: DVec3| light(p.floor().as_ivec3());

        // Models: bones posed here (in parallel when many), drawn instanced per kind.
        let mut posed = std::mem::take(&mut self.posed);
        posed.clear();
        self.models.begin();
        let steady = Camera { bob: (Vec3::ZERO, 0.0, 0.0), ..*cam }.view();
        let mut aim_root = None;
        if hand.is_none() {
            self.camera_kick = (Vec3::ZERO, 0.0, 0.0);
        }
        // Who holds something: its place in `posed`, the item, its light, in the other hand, its age.
        let mut holders: Vec<(usize, u32, u32, bool, f32)> = Vec::new();
        // The first person: its place in `posed`, its layers before the arms were left out of
        // them and which arms the clips bring up.
        let mut own_hand: Option<(usize, [Instance; super::models::MAX_LAYERS], usize, u32)> = None;
        for (id, e) in entities
            .iter()
            .map(|(i, e)| (*i, e))
            .chain(me.map(|m| (self.local, m)))
        {
            let base = rel(e.pos);
            if base.length_squared() > 160.0 * 160.0 {
                continue;
            }
            let lit = shade(e.pos + DVec3::Y * 0.5);
            // Below zero the model's shader flashes white instead of red.
            let hurt = if e.flash > 0.0 { -1.0 } else { (e.hurt / 0.5).clamp(0.0, 1.0) };
            if !e.item.is_empty() {
                if let Some(m) = self.item_model(textures, e.item.item) {
                    instances.push((e.item.item, item_instance(ground_transform(base, e.age, e.item.item), m.0, hurt, lit)));
                }
                continue;
            }
            let kind = e.look();
            let own_view = id == self.local && hand.is_some();
            // With its body showing the player sees its real arms, moving as anyone else sees them.
            // Without it there is nothing for them to be attached to: they are held in the view.
            let body = own_view && hand.as_ref().is_some_and(|h| h.body);
            let in_view = own_view && !body;
            let hurt = if own_view { 0.0 } else { hurt };
            let (mut layers, mut layer_count) = self.layers(kind, id, &e.worn, lit, hurt);
            if layer_count == 0 {
                continue;
            }
            if e.burning && !own_view {
                let (hw, h, _) = e.dims();
                let from = boxes.len();
                fire_boxes(&mut boxes, textures, base, hw as f32 * 2.0, h as f32, cam.yaw, e.age);
                light_boxes(&mut boxes, from, FULL_LIGHT);
            }
            // Lying down in its low box: head first where it looks when swimming (vanilla's
            // setupRotations), flat when crawling, turned around the middle of the body.
            let lying = match e.pose {
                crate::world::Pose::Swim => Some(e.pitch - std::f32::consts::FRAC_PI_2),
                crate::world::Pose::Crawl => Some(-std::f32::consts::FRAC_PI_2),
                // On its back, the head toward its back's side.
                crate::world::Pose::Sleep => Some(std::f32::consts::FRAC_PI_2),
                _ => None,
            };
            let sleeping = e.pose == crate::world::Pose::Sleep;
            let body_yaw = if sleeping { e.yaw } else { e.body_yaw };
            let swim = lying.map_or(Mat4::IDENTITY, |a| {
                Mat4::from_translation(Vec3::new(0.0, 0.3, 0.0))
                    * Mat4::from_rotation_x(a)
                    * Mat4::from_translation(Vec3::new(0.0, -0.9, 0.0))
            });
            let mut root =
                Mat4::from_translation(base) * Mat4::from_rotation_y(body_yaw) * swim * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
            // Its head up to look ahead when lying down.
            let (pitch, head_yaw) = if sleeping {
                (0.0, 0.0)
            } else if lying.is_some() {
                (std::f32::consts::FRAC_PI_4, 0.0)
            } else {
                (e.pitch, e.yaw - e.body_yaw)
            };
            let (mut clips, mut standing) = (Clips::default(), 0);
            if let Some(m) = self.models.kinds.get(kind as usize) {
                let how = Play { clock: e.age, distance: e.walk * 0.6, speed: e.swing, ..Play::default() };
                // Stands for what it says unless a mod swapped it for now, and has a clip for that.
                let swapped = |item: u32, name: &'static str| -> &str {
                    match e.swaps.iter().find(|s| s.0 == name) {
                        Some(s) if !m.action_seen(item, &s.1, in_view).is_empty() => &s.1,
                        _ => name,
                    }
                };
                let act = |a: Act| if e.swaps.is_empty() { m.act(a) } else { m.action(0, swapped(0, a.name())) };
                m.play(&mut clips, act(Act::Idle), &Play { looping: true, ..how });
                if lying.is_none() {
                    m.play(&mut clips, act(Act::Walk), &Play { drive: Drive::Distance, by_speed: true, ..how });
                }
                if e.crouched > 0.0 {
                    m.play(&mut clips, act(Act::Sneak), &Play { looping: true, weight: e.crouched, ..how });
                }
                if !sleeping && lying.is_some() {
                    let crawl = act(Act::Crawl);
                    let limbs = if e.pose == crate::world::Pose::Crawl && !crawl.is_empty() { crawl } else { act(Act::Swim) };
                    m.play(&mut clips, limbs, &Play { drive: Drive::Distance, ..how });
                }
                standing = clips.n;
                // From the eyes it stays in view whatever the body does.
                if lying.is_none() || (in_view && !sleeping) {
                    // What it holds, and what it does with that. An item for both hands in the
                    // other hand, or with something there, plays what it has for one.
                    let hold_in = |item: u32, busy: bool| -> &str {
                        match crate::content::one_handed_action(c, item).filter(|_| busy && crate::content::two_handed(c, item)) {
                            Some(one) => one,
                            None => swapped(item, hold_of(c, m, item)),
                        }
                    };
                    let held_how = Play { looping: true, aim: true, ..how };
                    if e.held != 0 {
                        m.play(&mut clips, m.action_seen(e.held, hold_in(e.held, e.off_held != 0), in_view), &held_how);
                    } else if in_view {
                        m.play(&mut clips, m.action_seen(0, "hold_empty", true), &held_how);
                    }
                    if e.off_held != 0 {
                        m.play(
                            &mut clips,
                            m.action_seen(e.off_held, hold_in(e.off_held, true), in_view),
                            &Play { mirror: true, ..held_how },
                        );
                    }
                    let progress = Play { drive: Drive::Progress, aim: in_view, ..how };
                    if let Some(h) = hand.as_ref().filter(|_| in_view) {
                        m.play(
                            &mut clips,
                            m.action_seen(e.held, swapped(e.held, "equip"), true),
                            &Play { progress: h.lowered, ..progress },
                        );
                    }
                    if let Some(p) = e.arm_swing {
                        let item = if e.swing_off { e.off_held } else { e.held };
                        let swing = if item == 0 && !in_view {
                            act(Act::Swing)
                        } else {
                            m.action_seen(item, swapped(item, m.swing_of(item)), in_view)
                        };
                        m.play(&mut clips, swing, &Play { progress: p, mirror: e.swing_off, ..progress });
                    }
                    if let Some((p, off)) = e.using {
                        let item = if off { e.off_held } else { e.held };
                        m.play(
                            &mut clips,
                            m.action_seen(item, swapped(item, "use"), in_view),
                            &Play { progress: p, mirror: off, ..progress },
                        );
                    }
                }
                if let Some(d) = e.dying {
                    m.play(&mut clips, act(Act::Death), &Play { clock: d, ..how });
                }
                if let Some((name, start, looping)) = &e.action {
                    let secs = e.age - start;
                    let list = m.action_seen(e.held, name, in_view);
                    let running = list
                        .last()
                        .and_then(|&i| m.anims.get(i as usize))
                        .is_some_and(|b| !b.finished(secs, *looping));
                    if running {
                        m.play(&mut clips, list, &Play { clock: secs, looping: *looping, aim: in_view, ..how });
                    }
                }
            }
            if own_view {
                // Its model goes where the camera is: the camera doesn't sit in the torso, whatever pose it's in.
                let eye = c.game.entities[kind as usize].eye;
                if let Some(at) = self
                    .models
                    .eye_point(kind, root, &clips.all()[..standing], eye)
                {
                    root = Mat4::from_translation(-at) * root;
                }
                aim_root = self
                    .models
                    .view_point(kind, eye)
                    .filter(|_| in_view)
                    .map(|p| {
                        let view_root = Mat4::from_rotation_y(e.yaw)
                            * Mat4::from_rotation_x(e.pitch)
                            * Mat4::from_scale(Vec3::splat(1.0 / 16.0))
                            * Mat4::from_translation(-p);
                        (posed.len(), view_root)
                    });
                let acting = &clips.all()[standing..];
                let mut arms_up = 0u32;
                let mut kick = (Vec3::ZERO, 0.0, 0.0);
                let mut head_moves = false;
                if let Some(m) = self.models.kinds.get(kind as usize) {
                    let names = |i: usize| {
                        acting.iter().any(|c| {
                            m.anims
                                .get(c.anim as usize)
                                .is_some_and(|b| b.names(i, c.mirror))
                        })
                    };
                    for (i, p) in m.parts.iter().enumerate() {
                        if in_view && (p.role == role::ARM_LEFT || p.role == role::ARM_RIGHT) && names(i) {
                            arms_up |= 1 << p.role;
                        }
                    }
                    for c in acting {
                        let Some((pos, rot)) = m
                            .anims
                            .get(c.anim as usize)
                            .and_then(|b| b.anim.free_bone("camera", c.t))
                        else {
                            continue;
                        };
                        kick.0 -= steady.transform_vector3(root.transform_vector3(pos * c.w));
                        kick.1 -= rot.z * c.w;
                        kick.2 -= rot.x * c.w;
                    }
                    head_moves = m
                        .parts
                        .iter()
                        .position(|p| p.role == role::HEAD)
                        .is_some_and(names);
                }
                if head_moves {
                    // The pose has its eyes at the camera; what the action does on top of it, the camera does too.
                    if let Some(at) = self.models.eye_point(kind, root, clips.all(), eye) {
                        kick.0 -= steady.transform_vector3(at);
                    }
                }
                self.camera_kick = kick;
                own_hand = in_view.then_some((posed.len(), layers, layer_count, arms_up));
                for l in &mut layers[..layer_count] {
                    l.roles &= !(1 << role::HEAD | arms_up);
                }
                if !body {
                    layer_count = 0;
                }
            }
            let from = self.models.clips.len() as u32;
            self.models.clips.extend_from_slice(clips.all());
            posed.push(Posed { kind, root, pitch, head_yaw, clips: (from, self.models.clips.len() as u32), layers, layer_count });
            if !sleeping {
                let age = e.age;
                if e.held != 0 {
                    holders.push((posed.len() - 1, e.held, lit, false, age));
                }
                if e.off_held != 0 {
                    holders.push((posed.len() - 1, e.off_held, lit, true, age));
                }
            }
        }
        let vb = scene_buf;
        let first_bone = self.models.bones_so_far();
        self.models.add_many(&posed, aim_root);
        // The first person's arms lag behind a turning camera. Its bones are the last ones.
        let mut own_first = 0;
        if let (Some((i, ..)), Some(h)) = (&own_hand, &hand) {
            let parts = self
                .models
                .kinds
                .get(posed[*i].kind as usize)
                .map_or(&[][..], |m| m.parts.as_slice());
            own_first = self.models.bones_so_far() - parts.len();
            let arms: Vec<usize> = parts
                .iter()
                .enumerate()
                .filter(|(_, p)| p.role == role::ARM_LEFT || p.role == role::ARM_RIGHT)
                .map(|(i, _)| i)
                .collect();
            let lag = steady.inverse() * Mat4::from_rotation_x(-h.lag.0 * 0.1) * Mat4::from_rotation_y(-h.lag.1 * 0.1) * steady;
            for a in arms {
                self.models.move_bone(own_first + a, lag);
            }
        }
        let mut hand_items: Vec<(GpuBox, u32)> = Vec::new();
        let mut own_items: Vec<(u32, u32, Instance)> = Vec::new();
        let (mut at, mut next) = (first_bone, 0);
        for (holder, item, lit, off, age) in holders {
            at += posed[next..holder]
                .iter()
                .filter_map(|p| self.models.kinds.get(p.kind as usize))
                .map(|m| m.parts.len())
                .sum::<usize>();
            next = holder;
            let parts = self
                .models
                .kinds
                .get(posed[holder].kind as usize)
                .map(|m| m.parts.as_slice())
                .unwrap_or_default();
            let side = if off { role::ARM_LEFT } else { role::ARM_RIGHT };
            let Some(i) = parts.iter().position(|p| p.role == side) else {
                continue;
            };
            let arm = self.models.bone(at + i) * Mat4::from_translation(Vec3::from_array(parts[i].pivot));
            let own = own_hand.as_ref().is_some_and(|o| o.0 == holder);
            // Made of bones: posed by the holder's clips of the same names, from the palm.
            if let Some(&(kind, ref skin)) = self.models.item_kinds.get(&item) {
                let layer = self
                    .skins
                    .get(skin)
                    .and_then(|s| s.as_ref())
                    .map(|(_, tex, size)| Instance {
                        color: [1.0; 3],
                        light: lit,
                        tex: *tex,
                        tex_size: [size[0] as f32, size[1] as f32],
                        roles: u32::MAX,
                        glow: u32::MAX,
                        ..Default::default()
                    });
                let root = palm(arm) * Mat4::from_scale(Vec3::splat(1.0 / 16.0));
                let (from, to) = posed[holder].clips;
                let clips = self
                    .models
                    .clips_along(kind, posed[holder].kind, &self.models.clips[from as usize..to as usize], age, off);
                let bones = self.models.pose_clips(kind, root, clips.all());
                if let Some(l) = layer {
                    if own {
                        own_items.push((kind, bones, l));
                    } else {
                        self.models.queue(kind, bones, [l]);
                    }
                }
            } else if let Some(m) = self.item_model(textures, item) {
                let display = self.models.displays.get(&item).copied();
                let inst = item_instance(held_transform(arm, item, off, display), m.0, 0.0, lit);
                if own {
                    hand_items.push((inst, m.1));
                } else {
                    instances.push((item, inst));
                }
            }
        }
        self.posed = posed;
        self.models
            .draw(gpu, list, frame, super::models::Batch::World, self.model_pipe, vb);

        // Selection outline: twelve thin dark boxes along the edges of each selection box.
        let outlined = target.map(|(hit, at)| (&c.states[hit.state as usize].selection, rel(hit.block.as_dvec3() + at)));
        for (sel, o) in outlined
            .iter()
            .flat_map(|(sel, o)| sel.iter().map(move |s| (s, *o)))
        {
            let (mn, mx) = (Vec3::new(sel[0], sel[1], sel[2]), Vec3::new(sel[3], sel[4], sel[5]));
            let t = 0.006;
            let (a, b) = (o + mn - Vec3::splat(t), o + mx + Vec3::splat(t));
            for axis in 0..3 {
                for corner in 0..4 {
                    let mut lo = a;
                    let mut size = Vec3::splat(t * 2.0);
                    size[axis] = b[axis] - a[axis];
                    let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                    lo[u] = if corner & 1 == 0 { a[u] } else { b[u] - t * 2.0 };
                    lo[v] = if corner & 2 == 0 { a[v] } else { b[v] - t * 2.0 };
                    let m = Mat4::from_translation(lo) * Mat4::from_scale(size);
                    boxes.push(GpuBox {
                        rows: rows(m),
                        color: [0.02, 0.02, 0.02, 0.0],
                        ids: [0, box_mode::SOLID, 0, 0],
                        ..Default::default()
                    });
                }
            }
        }
        // Gizmo lines: thin boxes like the outline, a little thicker so they read from afar.
        for &(a, b, color) in gizmo_lines {
            let (a, b) = (rel(a), rel(b));
            let d = b - a;
            let len = d.length();
            if len < 1e-4 || a.length_squared().min(b.length_squared()) > 160.0 * 160.0 {
                continue;
            }
            let t = 0.012 + 0.0012 * a.length().min(b.length());
            let rot = Quat::from_rotation_arc(Vec3::X, d / len);
            let m = Mat4::from_rotation_translation(rot, a)
                * Mat4::from_translation(Vec3::new(0.0, -t / 2.0, -t / 2.0))
                * Mat4::from_scale(Vec3::new(len, t, t));
            let rgb = |s: u32| ((color >> s) & 255) as f32 / 255.0;
            boxes.push(GpuBox {
                rows: rows(m),
                color: [rgb(24), rgb(16), rgb(8), 0.0],
                ids: [0, box_mode::SOLID, 0, 0],
                ..Default::default()
            });
        }

        let overlay_start = boxes.len();
        // Filled gizmo boxes: see-through by their own alpha (mode 5), slightly grown so they do
        // not fight with the faces of the blocks they cover.
        for &(a, b, color) in gizmo_fills {
            let (a, b) = (rel(a) - Vec3::splat(0.002), rel(b) + Vec3::splat(0.002));
            if a.length_squared().min(b.length_squared()) > 160.0 * 160.0 {
                continue;
            }
            let m = Mat4::from_translation(a) * Mat4::from_scale(b - a);
            let ch = |s: u32| ((color >> s) & 255) as f32 / 255.0;
            boxes.push(GpuBox {
                rows: rows(m),
                color: [ch(24), ch(16), ch(8), 0.0],
                ids: [0, box_mode::FILLED, ch(0).to_bits(), 0],
                ..Default::default()
            });
        }
        for &Crack { pos: p, progress, at, state } in breaking {
            if progress > 0.0 {
                // Over the faces of the block's model (a ladder's board, not its whole cube),
                // textured like the block's own faces there.
                let stage = ((progress * 10.0) as usize).min(9);
                let l = textures.texture(c.crack_textures[stage]);
                let lit = shade(p.as_dvec3() + at + DVec3::splat(0.5));
                let shape = c
                    .states
                    .get(state as usize)
                    .map_or(&[][..], |s| &s.selection[..]);
                for b in shape {
                    let (mn, mx) = (Vec3::new(b[0], b[1], b[2]), Vec3::new(b[3], b[4], b[5]));
                    let (a, b) = (mn - Vec3::splat(0.004), mx + Vec3::splat(0.004));
                    let m = Mat4::from_translation(rel(p.as_dvec3() + at) + a) * Mat4::from_scale(b - a);
                    boxes.push(GpuBox {
                        rows: rows(m),
                        dims: mn.extend(1.0).to_array(),
                        uvo: mx.extend(0.0).to_array(),
                        color: [1.0, 1.0, 1.0, 0.0],
                        ids: [0, box_mode::FACES, l | l << 16, l | l << 16],
                        ids2: [l | l << 16, lit, 0, 0],
                    });
                }
            }
        }
        // Item instances after the rest, grouped by item: one instanced draw each.
        let overlay_end = boxes.len();
        instances.sort_unstable_by_key(|i| i.0);
        boxes.extend(instances.iter().map(|i| i.1));
        boxes.extend(hand_items.iter().map(|h| h.0));
        // The buffer for them may not be empty, and arms alone have no boxes.
        if boxes.is_empty() {
            boxes.push(GpuBox::default());
        }
        if !instances.is_empty() || !hand_items.is_empty() {
            self.upload_items(gpu);
        }

        let buf =
            crate::gpu_layout::frame_buffer(gpu, &mut self.buffers, frame.slot, boxes.len() as u32, 128, usage::STORAGE, "entity boxes");
        gpu.mapped(buf)[..boxes.len() * 128].copy_from_slice(bytemuck::cast_slice(&boxes));
        let glow = textures.glow_buffer();

        if overlay_start > 0 {
            list.pipeline(self.pipeline);
            list.bind(crate::gpu_layout::bind::entity::BOXES, buf);
            list.bind(crate::gpu_layout::bind::entity::SCENE, vb);
            list.bind(crate::gpu_layout::bind::entity::GLOW_MAP, glow);
            list.draw(overlay_start as u32 * 36, 1);
        }
        let item_buf = self.item_buf.map(|b| b.0);
        if let (Some(ib), false) = (item_buf, instances.is_empty()) {
            list.pipeline(self.item_pipe);
            list.bind(crate::gpu_layout::bind::entity::BOXES, buf);
            list.bind(crate::gpu_layout::bind::entity::SCENE, vb);
            list.bind(crate::gpu_layout::bind::entity::GLOW_MAP, glow);
            list.bind(crate::gpu_layout::bind::entity::ITEM_BOXES, ib);
            let mut i = 0;
            while i < instances.len() {
                let item = instances[i].0;
                let run = instances[i..].iter().take_while(|x| x.0 == item).count();
                let boxes_per = self.item_models.get(&item).map_or(0, |m| m.1);
                list.push(Cmd::Draw {
                    vertices: boxes_per * 36,
                    instances: run as u32,
                    first_vertex: 0,
                    first_instance: (overlay_end + i) as u32,
                });
                i += run;
            }
        }
        // Cracks, filled gizmos and the own hands come after translucent blocks.
        let list = late;
        if overlay_end > overlay_start {
            list.pipeline(self.overlay);
            list.bind(crate::gpu_layout::bind::entity::BOXES, buf);
            list.bind(crate::gpu_layout::bind::entity::SCENE, vb);
            list.bind(crate::gpu_layout::bind::entity::GLOW_MAP, glow);
            list.push(Cmd::Draw {
                vertices: (overlay_end - overlay_start) as u32 * 36,
                instances: 1,
                first_vertex: overlay_start as u32 * 36,
                first_instance: 0,
            });
        }
        if let Some((i, own, count, arms_up)) = own_hand {
            let hand_cam = Camera { fov: 70.0, ..*cam };
            let hv = Scene { view_proj: (hand_cam.proj(aspect) * cam.view()).to_cols_array_2d(), ..*scene };
            let hb = self.hand_views[frame.slot];
            gpu.mapped(hb)[..size_of::<Scene>()].copy_from_slice(bytemuck::bytes_of(&hv));
            self.models.begin_batch();
            let kind = self.posed[i].kind;
            let arms = own[..count].iter().copied().map(|mut l| {
                l.roles &= arms_up;
                l
            });
            self.models.queue(kind, own_first as u32, arms);
            for (kind, first, l) in own_items {
                self.models.queue(kind, first, [l]);
            }
            self.models
                .draw(gpu, list, frame, super::models::Batch::Hand, self.model_hand_pipe, hb);
            if let Some(ib) = item_buf.filter(|_| !hand_items.is_empty()) {
                list.pipeline(self.hand_item_pipe);
                list.bind(crate::gpu_layout::bind::entity::BOXES, buf);
                list.bind(crate::gpu_layout::bind::entity::SCENE, hb);
                list.bind(crate::gpu_layout::bind::entity::GLOW_MAP, glow);
                list.bind(crate::gpu_layout::bind::entity::ITEM_BOXES, ib);
                let first = boxes.len() - hand_items.len();
                for (k, (_, n)) in hand_items.iter().enumerate() {
                    list.push(Cmd::Draw { vertices: n * 36, instances: 1, first_vertex: 0, first_instance: (first + k) as u32 });
                }
            }
        }
        self.instances = instances;
    }
}

/// Two layers of flames facing the camera, the inner one smaller, like vanilla's burning mobs.
fn fire_boxes(out: &mut Vec<GpuBox>, textures: &GameTextures, base: Vec3, width: f32, height: f32, cam_yaw: f32, age: f32) {
    let c = content();
    let frame = |offset: f32| textures.frame_now(c.fire_texture, age * crate::world::TPS as f32 + offset);
    for (i, (scale, back)) in [(1.4, 0.0), (1.0, 0.2)].into_iter().enumerate() {
        let l = frame(i as f32 * 11.0);
        let w = width * scale;
        let m = Mat4::from_translation(base)
            * Mat4::from_rotation_y(cam_yaw)
            * Mat4::from_translation(Vec3::new(-w / 2.0, 0.0, back * width))
            * Mat4::from_scale(Vec3::new(w, height * scale * 0.95, 0.0));
        out.push(GpuBox {
            rows: rows(m),
            color: [1.0, 1.0, 1.0, 0.0],
            ids: [0, box_mode::FLAMES, l | l << 16, l | l << 16],
            ids2: [l | l << 16, 0, 0, 0],
            ..Default::default()
        });
    }
}

/// Texture a held item is drawn from as a solid sprite: its icon, or the texture of a plant block.
fn sprite_name(item: u32) -> Option<String> {
    let c = content();
    let looks = super::looks::current();
    let t = match looks.item_draw(c, item) {
        ItemDraw::Icon(t) => t,
        ItemDraw::Block(state) => looks.block_sprite(state, &c.states[state as usize])?,
        _ => return None,
    };
    looks.texture_name(c, t).map(str::to_string)
}

/// Model boxes (a block's, or an item's from its file) in the unit cube `m` maps, tinted like
/// `s` (an item's own boxes pass air: no tint).
fn boxes_shape(
    out: &mut Vec<GpuBox>,
    textures: &GameTextures,
    m: Mat4,
    boxes: &[crate::content::ModelBox],
    s: &crate::content::StateInfo,
    color: Vec4,
) {
    let tint = crate::content::item_tint(s.tint) & 0xff_ffff;
    for b in boxes {
        for d in 0..6 {
            let Some((corners, uv)) = super::mesher::box_face(b, d) else { continue };
            let (n, shade) = super::mesher::face_look(&corners);
            let [p0, p1, _, p3] = corners.map(Vec3::from_array);
            let overlay = textures
                .overlays
                .get(b.textures[d] as usize)
                .filter(|&&o| o != 0)
                .map(|o| o - 1);
            let mut face = |texture: u32, tinted: bool, lift: f32| {
                let place = Mat4::from_cols((p1 - p0).extend(0.0), (p3 - p0).extend(0.0), Vec4::ZERO, (p0 + n * lift).extend(1.0));
                out.push(GpuBox {
                    rows: rows(m * place),
                    dims: [shade, 0.0, 0.0, 0.0],
                    uvo: uv.map(|v| v / 16.0),
                    color: color.to_array(),
                    ids: [0, box_mode::QUAD, textures.texture(texture), 0],
                    ids2: [0, 0, tinted as u32, tint],
                });
            };
            let paint = crate::content::face_paint(s, d, overlay.is_some());
            // Faces the block tints take the tint here, not from a tinted copy drawn on top (the
            // untinted faces would show through cutout holes and at the edges).
            face(b.textures[d], paint.base_tinted, 0.0);
            // The part of a face over it (a grass side's green), just outside it.
            if let Some(o) = overlay {
                face(o, paint.overlay_tinted, 0.002);
            }
        }
    }
}

/// An item model instance: `m` places the model's unit cube.
fn item_instance(m: Mat4, first: u32, hurt: f32, light: u32) -> GpuBox {
    GpuBox { rows: rows(m), color: [1.0, 1.0, 1.0, hurt], ids: [0, 0, first, 0], ids2: [0, light, 0, 0], ..Default::default() }
}

/// Whether the item shows as its block's shape (else as a sprite).
fn shows_block(item: u32) -> bool {
    let c = content();
    let looks = super::looks::current();
    match looks.item_draw(c, item) {
        ItemDraw::Model(_) => true,
        ItemDraw::Block(state) => looks
            .block_sprite(state, &c.states[state as usize])
            .is_none(),
        _ => false,
    }
}

/// Where an item goes in the hand of a model, from its arm at the shoulder (pixels): vanilla's
/// ItemInHandLayer, then the item model's third person display transform. Vanilla's model space is
/// ours turned around z, and it measures from a shoulder one pixel closer to the body.
fn held_transform(arm: Mat4, item: u32, off: bool, display: Option<Display>) -> Mat4 {
    use std::f32::consts::{FRAC_PI_2, PI};
    let deg = |d: f32| d.to_radians();
    let hand = arm
        * Mat4::from_scale(Vec3::splat(16.0))
        * Mat4::from_rotation_z(PI)
        * Mat4::from_rotation_x(-FRAC_PI_2)
        * Mat4::from_rotation_y(PI)
        * Mat4::from_translation(Vec3::new(0.0, 0.125, -0.625));
    // The model file's, as Blockbench's Display tab has it, else a block straight in the hand,
    // handheld.json or generated.json.
    let plain = |rotation, translation, scale| Display { rotation, translation, scale: [scale; 3] };
    let d = display.unwrap_or_else(|| {
        if shows_block(item) {
            plain([0.0, 0.0, 0.0], [0.0, 2.5, 0.0], 0.375)
        } else if content()
            .items
            .get(item as usize)
            .is_some_and(|d| d.handheld)
        {
            plain([0.0, -90.0, 55.0], [0.0, 4.0, 0.5], 0.85)
        } else {
            plain([0.0, 0.0, 0.0], [0.0, 3.0, 1.0], 0.55)
        }
    });
    // The other hand's display transforms are these turned the other way.
    let side = if off { -1.0 } else { 1.0 };
    let [rx, ry, rz] = d.rotation;
    let [tx, ty, tz] = d.translation;
    hand * Mat4::from_translation(Vec3::new(tx * side, ty, tz) / 16.0)
        * Mat4::from_rotation_x(deg(rx))
        * Mat4::from_rotation_y(deg(ry * side))
        * Mat4::from_rotation_z(deg(rz * side))
        * Mat4::from_scale(Vec3::from_array(d.scale))
        // Seen from behind, a flat item is its mirror image, without turning its faces inside out.
        * Mat4::from_rotation_y(if off { PI } else { 0.0 })
        * Mat4::from_translation(Vec3::splat(-0.5))
}

/// How an item is held unless it says: a long tool lower down.
fn hold_of(c: &crate::content::Content, m: &super::models::KindModel, item: u32) -> &'static str {
    let tool = c.items.get(item as usize).is_some_and(|d| d.handheld);
    if tool && !m.has_own(item, "hold") { "hold_tool" } else { "hold" }
}

/// Where a model of bones held in the hand has its origin: on the palm, its front (-Z) along the
/// arm, so an arm held out points it ahead with its top (+Y) up.
fn palm(arm: Mat4) -> Mat4 {
    arm * Mat4::from_scale(Vec3::splat(16.0))
        * Mat4::from_translation(Vec3::new(0.0, -10.0 / 16.0, 0.0))
        * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2)
}

/// Dropped items spin and bob: a block a quarter of a block across, a sprite half a block
/// (vanilla's ground transforms).
fn ground_transform(base: Vec3, age: f32, item: u32) -> Mat4 {
    let bob = (age * 2.5).sin() * 0.06 + 0.15;
    let spin = Mat4::from_translation(base + Vec3::Y * bob) * Mat4::from_rotation_y(age * 1.5);
    if shows_block(item) {
        spin * Mat4::from_translation(Vec3::splat(-0.125)) * Mat4::from_scale(Vec3::splat(0.25))
    } else {
        spin * Mat4::from_translation(Vec3::new(-0.25, 0.0, -0.25)) * Mat4::from_scale(Vec3::splat(0.5))
    }
}
