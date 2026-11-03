//! Particles, conditions, screen effects: things that only change what players see.
//!
//! ```ignore
//! static SPARK: Particle = Particle::named("spark");
//! r.particle(&SPARK).textures(&["glint_0", "glint_1"]).life(0.3, 0.6).fullbright();
//! w.spawn_particles(&SPARK, pos).count(12).speed(2.0);          // server: everyone nearby sees them
//! r.condition(&CHARGED, |c| c.client().is_some_and(|v| v.component(&CHARGE).is_some_and(|c| c > 0)));
//! c.post_effect("mymod:underwater", 1.0);                  // client: until turned off
//! ```
//! Shaders for text (`{fx:..}`), GUI panels (`shader` in a class's `.gui`), particles, entities and screen
//! effects are assets: `shaders/fx|gui|particle|entity|block|sky|post/<name>.wgsl` in the mod's folder.

use std::marker::PhantomData;

use crate::abi::{self, Slice, Str, particle_flags as pf};
use crate::{Client, ClientView, Dimension, Music, Particle, Registry, SkyObject, SoundCategory, World, callback, host, qualify};

pub struct ParticleBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    textures: Vec<String>,
    shader: String,
    handle: Option<&'static Particle>,
    desc: abi::ParticleDesc,
    _p: PhantomData<&'a mut Registry>,
}

impl ParticleBuilder<'_> {
    /// Frames over its life; relative names are under `particle/`. None: block dust (pieces of
    /// the burst's block).
    pub fn textures(mut self, names: &[&str]) -> Self {
        self.textures = names.iter().map(|t| crate::asset("particle", t)).collect();
        self
    }
    /// Seconds, picked between the two.
    pub fn life(mut self, min: f32, max: f32) -> Self {
        self.desc.life = [min, max];
        self
    }
    /// Edge length in blocks, picked between the two.
    pub fn size(mut self, min: f32, max: f32) -> Self {
        self.desc.scale = [min, max];
        self
    }
    /// Blocks per second squared, pulling down (negative rises).
    pub fn gravity(mut self, g: f32) -> Self {
        self.desc.gravity = g;
        self
    }
    /// Fraction of speed kept per tick (vanilla 0.98).
    pub fn drag(mut self, d: f32) -> Self {
        self.desc.drag = d;
        self
    }
    /// 0xRRGGBBAA at birth and at the end of its life.
    pub fn color(mut self, start: u32, end: u32) -> Self {
        (self.desc.color, self.desc.color_end) = (start, end);
        self
    }
    pub fn colliding(mut self) -> Self {
        self.desc.flags |= pf::COLLIDE;
        self
    }
    pub fn fullbright(mut self) -> Self {
        self.desc.flags |= pf::FULLBRIGHT;
        self
    }
    pub fn translucent(mut self) -> Self {
        self.desc.flags |= pf::TRANSLUCENT;
        self
    }
    pub fn shrinking(mut self) -> Self {
        self.desc.flags |= pf::SHRINK;
        self
    }
    /// Colors it with `shaders/particle/<name>.wgsl` (`fn shade(p: ParticleFrag) -> vec4<f32>`).
    pub fn shader(mut self, name: &str) -> Self {
        self.shader = crate::asset("", name);
        self
    }
}

impl Drop for ParticleBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let textures: Vec<Str> = self.textures.iter().map(|t| Str::new(t)).collect();
        let desc = abi::ParticleDesc {
            name: Str::new(&self.name),
            cell: self.handle.map_or(std::ptr::null(), |h| h.id_cell()),
            textures: Slice::new(&textures),
            shader: Str::new(&self.shader),
            ..self.desc
        };
        unsafe { (host().reg_particle)(self.raw, &desc) }
    }
}

/// A thing in a dimension's sky, see `Registry::sky_body`, `sky_stars`, `sky_dome`.
pub struct SkyBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    dimension: String,
    texture: String,
    shader: String,
    desc: abi::SkyDesc,
    _p: PhantomData<&'a mut Registry>,
}

impl SkyBuilder<'_> {
    /// Picture, relative names are under `textures/sky/`. A dome tells its layout by its shape:
    /// twice as wide as high is a panorama, three by two is the six faces of a box.
    pub fn texture(mut self, name: &str) -> Self {
        self.texture = crate::asset("sky", name);
        self
    }
    /// Colors it with `shaders/sky/<name>.wgsl` instead of the plain picture.
    pub fn shader(mut self, name: &str) -> Self {
        self.shader = crate::asset("", name);
        self
    }
    /// Ticks per turn around the sky, negative the other way. Without it, it follows the
    /// dimension's day like the sun does.
    pub fn orbit(mut self, ticks: f32) -> Self {
        self.desc.orbit = ticks;
        self
    }
    /// Stays where its phase, tilt and heading put it.
    pub fn orbit_still(self) -> Self {
        self.orbit(0.0)
    }
    /// Back to following the dimension's day, for changing another mod's object.
    pub fn orbit_day(self) -> Self {
        self.orbit(f32::INFINITY)
    }
    /// Where on its orbit it starts, in turns: 0 rising in the east, 0.25 overhead, 0.5 setting.
    /// A moon opposite the sun is 0.5.
    pub fn phase(mut self, turns: f32) -> Self {
        self.desc.phase = turns;
        self
    }
    /// Degrees its orbit leans away from overhead, to the south.
    pub fn tilt(mut self, degrees: f32) -> Self {
        self.desc.tilt = degrees;
        self
    }
    /// Degrees its orbit turns around the vertical: 90 rises in the north instead of the east.
    pub fn heading(mut self, degrees: f32) -> Self {
        self.desc.heading = degrees;
        self
    }
    /// Degrees across the sky (vanilla's sun 33, moon 23); of each star, for stars.
    pub fn size(mut self, degrees: f32) -> Self {
        self.desc.size = degrees;
        self
    }
    /// Ticks per turn around its own middle.
    pub fn spin(mut self, ticks: f32) -> Self {
        self.desc.spin = ticks;
        self
    }
    /// How much of it shows by full day and by deep night, 0..1 (stars: 0 and 0.22).
    pub fn opacity(mut self, day: f32, night: f32) -> Self {
        self.desc.opacity = [day, night];
        self
    }
    /// 0xRRGGBB multiplied in.
    pub fn color(mut self, rgb: u32) -> Self {
        self.desc.color = rgb;
        self
    }
    /// The picture is a grid of frames, shown one after another: a moon's phases.
    pub fn frames(mut self, columns: u32, rows: u32) -> Self {
        self.desc.frames = [columns, rows];
        self
    }
    /// Ticks each frame lasts; by default a day of the dimension.
    pub fn frame_ticks(mut self, ticks: u32) -> Self {
        self.desc.frame_ticks = ticks;
        self
    }
    /// Lights the horizon while rising and setting, a sunset in this color (0xRRGGBB).
    pub fn glow(mut self, rgb: u32) -> Self {
        self.desc.glow = rgb;
        self
    }
    /// Adds its light to what is behind it. Without it, it covers what is behind it, so a moon
    /// with an opaque picture hides any other that passes behind it.
    pub fn blend_add(mut self) -> Self {
        self.desc.additive = 1;
        self
    }
    /// Covers what is behind it, for an object someone made `blend_add`.
    pub fn blend_cover(mut self) -> Self {
        self.desc.additive = 0;
        self
    }
    /// Drawn low to high, one over the other; registration order among equals. Bodies start at
    /// 0, stars at -100, domes at -200.
    pub fn order(mut self, order: i32) -> Self {
        self.desc.order = order;
        self
    }
    /// The height of clouds' underside (only clouds use it, like `drift`).
    pub fn height(mut self, y: f32) -> Self {
        self.desc.height = y;
        self
    }
    /// Blocks per tick clouds drift along x, negative the other way.
    pub fn drift(mut self, blocks_per_tick: f32) -> Self {
        self.desc.drift = blocks_per_tick;
        self
    }
    /// How many stars (only stars use it, like `seed`).
    pub fn count(mut self, n: u32) -> Self {
        self.desc.count = n;
        self
    }
    /// What scatters the stars: another seed, another sky.
    pub fn seed(mut self, seed: u32) -> Self {
        self.desc.seed = seed;
        self
    }
}

impl Drop for SkyBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let desc = abi::SkyDesc {
            dimension: Str::new(&self.dimension),
            name: Str::new(&self.name),
            texture: Str::new(&self.texture),
            shader: Str::new(&self.shader),
            ..self.desc
        };
        unsafe { (host().reg_sky)(self.raw, &desc) }
    }
}

impl Registry {
    fn sky_object<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject, kind: u32) -> SkyBuilder<'a> {
        SkyBuilder {
            raw: self.raw,
            name: object.name().to_string(),
            dimension: dimension.name().to_string(),
            texture: String::new(),
            shader: String::new(),
            desc: abi::SkyDesc { kind, ..abi::SkyDesc::DEFAULT },
            _p: PhantomData,
        }
    }

    /// A picture on an orbit, a sun or a moon, independent of everything else in the sky:
    ///
    /// ```ignore
    /// static PLANET: SkyObject = SkyObject::named("planet");
    /// r.sky_body(&OVERWORLD, &PLANET).texture("planet").size(6.0).orbit(96000.0).phase(0.1).tilt(20.0);
    /// ```
    ///
    /// Naming another mod's object changes only what you set: `r.sky_body(&OVERWORLD, &MOON).size(40.0)`.
    pub fn sky_body<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a> {
        self.sky_object(dimension, object, abi::sky_kind::BODY)
    }

    /// Points scattered over the sky and turning together, seen at night unless `opacity` says
    /// otherwise.
    pub fn sky_stars<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a> {
        self.sky_object(dimension, object, abi::sky_kind::STARS)
    }

    /// The whole sky at once, a skybox picture or a shader, standing still unless given an orbit.
    /// It starts out still, and over the day's colors; `opacity(0.0, 1.0)` keeps it to the night.
    pub fn sky_dome<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a> {
        self.sky_object(dimension, object, abi::sky_kind::DOME)
    }

    /// A layer of boxes in the air, 12 blocks each, wherever `texture`'s pixels are filled:
    /// `r.sky_clouds(&OVERWORLD, &CLOUDS).texture("clouds").height(300.0)`.
    pub fn sky_clouds<'a>(&'a mut self, dimension: &Dimension, object: &SkyObject) -> SkyBuilder<'a> {
        self.sky_object(dimension, object, abi::sky_kind::CLOUDS)
    }

    /// Takes something out of one dimension's sky, your own or another mod's (that mod has to load
    /// first: depend on it). The base game puts its sun, moon and stars in every dimension of its
    /// own, each to be taken out separately.
    pub fn sky_remove(&mut self, dimension: &Dimension, object: &SkyObject) {
        let mut b = self.sky_object(dimension, object, u32::MAX);
        b.desc.remove = 1;
    }

    /// A particle kind, see `ParticleBuilder`. Registering `core:block` or `core:poof` changes how
    /// the engine's own block dust and smoke look.
    pub fn particle<'a>(&'a mut self, particle: &'static Particle) -> ParticleBuilder<'a> {
        ParticleBuilder {
            raw: self.raw,

            name: particle.name().to_string(),
            textures: Vec::new(),
            shader: String::new(),
            handle: Some(particle),
            desc: abi::ParticleDesc::DEFAULT,
            _p: PhantomData,
        }
    }

    /// A condition music (`.when`), text (`{if:<ns>:<name>}`), loot (`Cond::Condition`) and
    /// spawning (`.when`) can ask. `f` is answered on whichever side asks: on the server
    /// (`c.world()`) for loot and spawning, on the client (`c.client()`) for music and text, every
    /// frame such a text is on screen. Every game mode is a condition too, by its name.
    pub fn condition(&mut self, condition: &'static crate::Condition, f: impl Fn(&ConditionCtx) -> bool + Send + Sync + 'static) {
        let cb = callback(move |a: &mut abi::ConditionArgs| a.result = f(&ConditionCtx(*a)) as u32);
        unsafe { (host().reg_condition)(self.raw, Str::new(condition.name()), condition.id_cell(), cb) }
    }
}

/// What a condition is asked about.
pub struct ConditionCtx(abi::ConditionArgs);

impl ConditionCtx {
    /// The server's world, when the server asks (loot, spawning).
    pub fn world(&self) -> Option<World> {
        (!self.0.world.is_null()).then(|| World::from_raw(self.0.world))
    }
    /// The client, when it asks (music, text): its own player.
    pub fn client(&self) -> Option<ClientView> {
        (!self.0.client.is_null()).then(|| ClientView::from_raw(self.0.client))
    }
    /// The spot, the block, the player.
    pub fn pos(&self) -> [f64; 3] {
        self.0.pos
    }
    /// Who it is about (the killer or breaker for loot), 0 for nobody.
    pub fn entity(&self) -> u64 {
        self.0.entity
    }
}

enum Target {
    World(World),
    Client(Client),
}

/// Particles sent when this is dropped: `w.spawn_particles(&SPARK, pos).count(10).speed(1.5)`.
pub struct Burst {
    target: Target,
    burst: abi::ParticleBurst,
    area: Option<crate::Shape>,
}

impl Burst {
    /// The particles start anywhere inside `shape` (a ring, a shell, a magic circle) instead
    /// of a box around the position; `outward` sends them away from the position.
    pub fn area(mut self, shape: &crate::Shape) -> Self {
        self.area = Some(shape.clone());
        self
    }
    pub fn count(mut self, n: u32) -> Self {
        self.burst.count = n;
        self
    }
    /// Each particle starts up to this far from the position on every axis.
    pub fn spread(mut self, s: f32) -> Self {
        self.burst.spread = [s; 3];
        self
    }
    pub fn spread_xyz(mut self, s: [f32; 3]) -> Self {
        self.burst.spread = s;
        self
    }
    /// Starting velocity, blocks per second.
    pub fn velocity(mut self, v: [f32; 3]) -> Self {
        self.burst.velocity = v;
        self
    }
    /// Random extra velocity up to this, in any direction.
    pub fn random_speed(mut self, s: f32) -> Self {
        self.burst.speed = s;
        self
    }
    /// Each flies away from the middle of the burst, more or less, like the pieces of a broken block.
    pub fn outward(mut self) -> Self {
        self.burst.outward = 1;
        self
    }
    /// Block state the textureless kinds (block dust) take their texture from.
    pub fn block(mut self, state: u32) -> Self {
        self.burst.block = state;
        self
    }
}

impl Drop for Burst {
    fn drop(&mut self) {
        if std::thread::panicking() || self.burst.kind == u32::MAX {
            return;
        }
        let send = |b: &abi::ParticleBurst| match &self.target {
            Target::World(w) => unsafe { (host().particle_burst)(w.raw(), b) },
            Target::Client(c) => unsafe { (host().client_particle_burst)(c.raw(), b) },
        };
        let Some(area) = &self.area else { return send(&self.burst) };
        // One particle per point inside, found by trying points in the box around the shape.
        let (lo, hi) = area.bounds();
        let mut seed =
            self.burst.pos[0].to_bits() ^ self.burst.pos[1].to_bits().rotate_left(21) ^ self.burst.pos[2].to_bits().rotate_left(42);
        let mut next = || {
            seed = abi::fnv(&seed.to_le_bytes());
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let c = self.burst.pos;
        let speed = (self.burst.velocity.iter().map(|v| v * v).sum::<f32>())
            .sqrt()
            .max(1.0);
        let mut placed = 0;
        for _ in 0..self.burst.count.saturating_mul(20).min(20_000) {
            if placed >= self.burst.count {
                break;
            }
            let p = [0, 1, 2].map(|a| lo[a] + (hi[a] - lo[a]) * next());
            if !area.contains(p) {
                continue;
            }
            let mut one = abi::ParticleBurst { pos: p, count: 1, spread: [0.0; 3], outward: 0, ..self.burst };
            if self.burst.outward != 0 {
                let d = [0, 1, 2].map(|a| (p[a] - c[a]) as f32);
                let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-6);
                one.velocity = d.map(|x| x / len * speed);
            }
            send(&one);
            placed += 1;
        }
    }
}

fn burst(target: Target, kind: &Particle, pos: [f64; 3]) -> Burst {
    Burst { target, burst: abi::ParticleBurst { kind: kind.id(), pos, ..abi::ParticleBurst::DEFAULT }, area: None }
}

impl World {
    /// Particles every player near `pos` sees (one by default).
    pub fn spawn_particles(&self, kind: &Particle, pos: [f64; 3]) -> Burst {
        burst(Target::World(*self), kind, pos)
    }
}

impl Client {
    /// Particles only this client shows.
    pub fn spawn_particles(&self, kind: &Particle, pos: [f64; 3]) -> Burst {
        burst(Target::Client(*self), kind, pos)
    }

    /// Turns screen effect `name` (`shaders/post/<name>.wgsl`) on at `strength` (0 turns it off).
    pub fn post_effect(&self, name: &str, strength: f32) -> bool {
        let name = qualify(crate::mod_id(), name);
        unsafe { (host().client_op)(self.raw(), abi::client_op::POST_EFFECT, Str::new(&name), strength as f64) != 0 }
    }
}

pub struct SoundBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    files: Vec<String>,
    handle: Option<&'static crate::Sound>,
    desc: abi::SoundDesc,
    category_name: String,
    _p: PhantomData<&'a mut Registry>,
}

impl SoundBuilder<'_> {
    /// Variants picked at random: `sounds/<name>.ogg` or `.wav`, relative to this mod.
    pub fn files(mut self, names: &[&str]) -> Self {
        self.files = names.iter().map(|n| crate::asset("", n)).collect();
        self
    }
    /// Category for its volume setting: the engine's (`core:creatures` ...) or one registered
    /// with `Registry::sound_category` (any mod's).
    pub fn category(mut self, category: &SoundCategory) -> Self {
        self.category_name = category.name().to_string();
        self
    }
    pub fn volume(mut self, v: f32) -> Self {
        self.desc.volume = v;
        self
    }
    /// Playback speed picked between the two.
    pub fn pitch(mut self, min: f32, max: f32) -> Self {
        self.desc.pitch = [min, max];
        self
    }
    /// Blocks away it is still heard.
    pub fn range(mut self, blocks: f32) -> Self {
        self.desc.range = blocks;
        self
    }
    /// Full volume within `full_within` blocks, then fading to nothing at `range` along `curve`
    /// (by default within 1 block, evenly).
    pub fn falloff(mut self, full_within: f32, curve: Falloff) -> Self {
        self.desc.full_within = full_within;
        self.desc.falloff = match curve {
            Falloff::Even => 0,
            Falloff::Late(n) => n.clamp(1, 8) as i32,
            Falloff::Early(n) => -(n.clamp(1, 8) as i32),
        };
        self
    }
}

/// How a sound fades with distance (`SoundBuilder::falloff`).
#[derive(Clone, Copy, Debug)]
pub enum Falloff {
    Even,
    /// Stays loud longer, then drops near the edge; the number is how sharply (1..8).
    Late(u8),
    /// Drops soon after `full_within`, then trails off.
    Early(u8),
}

impl Drop for SoundBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let files: Vec<Str> = self.files.iter().map(|f| Str::new(f)).collect();
        let desc = abi::SoundDesc {
            name: Str::new(&self.name),
            cell: self.handle.map_or(std::ptr::null(), |h| h.id_cell()),
            files: Slice::new(&files),
            category_name: Str::new(&self.category_name),
            ..self.desc
        };
        unsafe { (host().reg_sound)(self.raw, &desc) }
    }
}

pub struct MusicBuilder<'a> {
    raw: *mut abi::Registrar,
    name: String,
    tracks: Vec<String>,
    dimension: String,
    when: String,
    _p: PhantomData<&'a mut Registry>,
}

impl MusicBuilder<'_> {
    /// Files streamed (`sounds/<name>.ogg|.wav`), relative to this mod.
    pub fn tracks(mut self, names: &[&str]) -> Self {
        self.tracks = names.iter().map(|n| crate::asset("", n)).collect();
        self
    }
    pub fn dimension(mut self, dimension: &crate::Dimension) -> Self {
        self.dimension = dimension.name().to_string();
        self
    }
    /// Plays only while this condition holds (`conditions::NIGHT`, a mod's own); call again
    /// for more, all of which have to hold.
    pub fn when(mut self, condition: &crate::Condition) -> Self {
        self.when = format!("{} {}", self.when, condition.name());
        self
    }
}

impl Drop for MusicBuilder<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            return;
        }
        let tracks: Vec<Str> = self.tracks.iter().map(|f| Str::new(f)).collect();
        let desc = abi::MusicDesc {
            name: Str::new(&self.name),
            tracks: Slice::new(&tracks),
            dimension: Str::new(&self.dimension),
            when: Str::new(&self.when),
            ..abi::MusicDesc::DEFAULT
        };
        unsafe { (host().reg_music)(self.raw, &desc) }
    }
}

impl Registry {
    /// A sound event, see `SoundBuilder`; blocks and entities name groups of them with `.sound_group()`.
    pub fn sound<'a>(&'a mut self, sound: &'static crate::Sound) -> SoundBuilder<'a> {
        SoundBuilder {
            raw: self.raw,
            name: sound.name().to_string(),
            files: Vec::new(),
            handle: Some(sound),
            desc: abi::SoundDesc::DEFAULT,
            category_name: String::new(),
            _p: PhantomData,
        }
    }

    /// A sound category with its own volume setting (`<ns>:volume_<name>`, in the mod's
    /// settings); `world`: heard through caves and water like block sounds.
    pub fn sound_category(&mut self, category: &SoundCategory, world: bool) {
        unsafe { (host().reg_sound_category)(self.raw, Str::new(category.name()), world as u32) }
    }

    /// Background music, see `MusicBuilder`.
    pub fn music<'a>(&'a mut self, music: &Music) -> MusicBuilder<'a> {
        MusicBuilder {
            raw: self.raw,
            name: music.name().to_string(),
            tracks: Vec::new(),
            dimension: String::new(),
            when: String::new(),
            _p: PhantomData,
        }
    }
}

impl World {
    /// Sound event `sound` at `pos` for the players in its range, sent at the end of the
    /// statement; `.pitch(p)` sets the playback speed.
    pub fn play_sound(&self, sound: &crate::Sound, pos: [f64; 3], volume: f32) -> PlaySound {
        PlaySound { to: Hearer::World(*self), sound: sound.id(), pos: Some(pos), volume, pitch: 0.0 }
    }
}

impl Client {
    /// Only this client hears it; `None` = not positioned (UI sounds).
    pub fn play_sound(&self, sound: &crate::Sound, pos: Option<[f64; 3]>, volume: f32) -> PlaySound {
        PlaySound { to: Hearer::Client(*self), sound: sound.id(), pos, volume, pitch: 0.0 }
    }
}

enum Hearer {
    World(World),
    Client(Client),
}

/// A sound about to play (`World::play_sound`, `Client::play_sound`).
pub struct PlaySound {
    to: Hearer,
    sound: u32,
    pos: Option<[f64; 3]>,
    volume: f32,
    pitch: f32,
}

impl PlaySound {
    /// Playback speed (1 = as recorded, 2 an octave up) instead of one picked from the sound's
    /// range: a note block's `2f32.powf((key - 12) as f32 / 12.0)`.
    pub fn pitch(mut self, pitch: f32) -> Self {
        self.pitch = pitch.max(0.01);
        self
    }
}

impl Drop for PlaySound {
    fn drop(&mut self) {
        let p = self
            .pos
            .as_ref()
            .map_or(std::ptr::null(), |p| p as *const [f64; 3]);
        unsafe {
            match self.to {
                Hearer::World(w) => (host().play_sound)(w.raw(), self.sound, p, self.volume, self.pitch),
                Hearer::Client(c) => (host().client_play_sound)(c.raw(), self.sound, p, self.volume, self.pitch),
            }
        }
    }
}

/// The engine's sound categories, each with its volume setting.
pub mod sound_categories {
    use crate::SoundCategory;
    pub static MUSIC: SoundCategory = SoundCategory::named("core:music");
    pub static BLOCKS: SoundCategory = SoundCategory::named("core:blocks");
    pub static CREATURES: SoundCategory = SoundCategory::named("core:creatures");
    pub static PLAYERS: SoundCategory = SoundCategory::named("core:players");
    pub static AMBIENT: SoundCategory = SoundCategory::named("core:ambient");
    pub static UI: SoundCategory = SoundCategory::named("core:ui");
}
