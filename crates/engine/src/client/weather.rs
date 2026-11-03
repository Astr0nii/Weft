//! Rain and snow the way vanilla draws them: a strip standing in each column around the camera,
//! turned toward it only about the vertical, with its picture scrolling down. Nothing is simulated:
//! the columns are looked up a few times a second, the strips are one instanced draw.

use glam::{DVec3, IVec3};
use weft_render::*;

use crate::assets::{Assets, decode_png};
use crate::content::content;
use crate::gpu_layout::GpuWeather;

use super::Camera;
use super::world::ClientWorld;

/// Columns this far from the camera have a strip (vanilla's fancy graphics).
const RADIUS: i32 = 10;
/// A strip goes this far up and down from the camera.
const REACH: i32 = 10;
/// Seconds between looking the columns up again, unless the camera moved to another column.
const REFRESH: f32 = 0.25;

/// A column with rain or snow in it.
#[derive(Clone, Copy)]
struct Column {
    x: i32,
    z: i32,
    bottom: i32,
    top: i32,
    /// What falls there (`registry::PrecipitationDef`).
    kind: u16,
    light: u32,
    /// A number of its own: which part of the picture, how fast.
    seed: u32,
}

pub struct Weather {
    pipe: Option<PipelineId>,
    /// Each precipitation's texture and its index.
    pictures: Vec<Option<(TextureId, u32)>>,
    columns: Vec<Column>,
    /// The camera's column and how long ago they were looked up.
    built: Option<(IVec3, f32)>,
    instances: Vec<(BufferId, u32)>,
    gpu: Vec<GpuWeather>,
}

impl Weather {
    pub fn new(gpu: &mut dyn Backend, assets: &Assets) -> Weather {
        let mut w = Weather { pipe: None, pictures: Vec::new(), columns: Vec::new(), built: None, instances: Vec::new(), gpu: Vec::new() };
        w.reload(gpu, assets);
        w
    }

    pub fn reload(&mut self, gpu: &mut dyn Backend, assets: &Assets) {
        let made = crate::packs::compose(assets, "weather.wgsl", &[])
            .map_err(|e| e.0)
            .and_then(|(shader, _)| {
                gpu.create_pipeline(&PipelineDesc {
                    label: "weather",
                    shader: &shader,
                    vertex: "vs",
                    fragment: Some("fs"),
                    color: Some(Format::Surface),
                    depth: Some(DepthState { write: false, compare: Compare::GreaterEqual }),
                    blend: Blend::Alpha,
                    cull_back: false,
                    topology: Topology::Triangles,
                })
                .map_err(|e| e.0)
            });
        match made {
            Ok(pipe) => {
                if let Some(old) = self.pipe.replace(pipe) {
                    gpu.destroy_pipeline(old);
                }
            }
            Err(e) => log::error!("weather shader: {e}"),
        }
        self.drop_pictures(gpu);
        let names: Vec<&str> = content()
            .game
            .precipitations
            .iter()
            .map(|p| p.texture.as_str())
            .collect();
        self.pictures = names
            .into_iter()
            .map(|name| {
                let img = assets
                    .read(name, "textures", "png")
                    .and_then(|b| decode_png(&b));
                if img.is_none() {
                    log::error!("weather: no picture {name}");
                }
                img.map(|img| {
                    let texture = gpu.create_texture(
                        &TextureDesc { width: img.width, height: img.height, mips: 1, format: Format::Rgba8Srgb, render_target: false },
                        name,
                    );
                    gpu.write_texture(texture, 0, &img.rgba);
                    (texture, gpu.texture_index(texture))
                })
            })
            .collect();
    }

    fn drop_pictures(&mut self, gpu: &mut dyn Backend) {
        for (t, _) in self.pictures.iter_mut().filter_map(|p| p.take()) {
            gpu.destroy_texture(t);
        }
    }

    pub fn destroy(&mut self, gpu: &mut dyn Backend) {
        if let Some(p) = self.pipe.take() {
            gpu.destroy_pipeline(p);
        }
        self.drop_pictures(gpu);
        for (b, _) in self.instances.drain(..) {
            gpu.destroy_buffer(b);
        }
    }

    /// The columns open to the sky around `eye`, each from what stops it to the top of its strip.
    fn look_up(&mut self, eye: IVec3, world: &ClientWorld) {
        let c = content();
        self.columns.clear();
        for dz in -RADIUS..=RADIUS {
            for dx in -RADIUS..=RADIUS {
                // Not right in front of the eyes, where a drop would be as wide as a block.
                if !(9..=RADIUS * RADIUS).contains(&(dx * dx + dz * dz)) {
                    continue;
                }
                let (x, z) = (eye.x + dx, eye.z + dz);
                let top = eye.y + REACH;
                // A roof over it, or nothing loaded there.
                if world
                    .light_loaded(IVec3::new(x, top, z))
                    .is_none_or(|l| weft_abi::light::sky(l) < 15)
                {
                    continue;
                }
                let floor = (eye.y - REACH..top).rev().find(|&y| {
                    let s = world
                        .block_or_air(IVec3::new(x, y, z))
                        .unwrap_or(crate::content::AIR);
                    c.states[s as usize].flags & (weft_abi::block_flags::SOLID | weft_abi::block_flags::LIQUID) != 0
                });
                let bottom = floor.map_or(eye.y - REACH, |y| y + 1);
                if bottom >= top {
                    continue;
                }
                let cell = IVec3::new(x, bottom, z);
                let biome = world
                    .cube(cell >> 4i32)
                    .map_or(0, |cube| cube.biome((x & 15) as usize, (z & 15) as usize));
                let kind = c
                    .biome_precipitation
                    .get(biome as usize)
                    .copied()
                    .unwrap_or(0);
                let seed = (x.wrapping_mul(3121) ^ z.wrapping_mul(45238971)) as u32;
                self.columns
                    .push(Column { x, z, bottom, top, kind, light: world.light(cell), seed: seed.wrapping_mul(0x9E37_79B1) >> 8 });
            }
        }
    }

    /// `level`: how much of it there is, 0..1. `ticks`: game ticks, with the fraction of one.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        gpu: &mut dyn Backend,
        list: &mut CommandList,
        frame: &FrameInfo,
        cam: &Camera,
        scene: BufferId,
        world: &ClientWorld,
        level: f32,
        ticks: f64,
        dt: f32,
    ) {
        let Some(pipe) = self.pipe else { return };
        if level <= 0.0 {
            self.built = None;
            return;
        }
        let eye = cam.pos.floor().as_ivec3();
        let at = eye * IVec3::new(1, 0, 1);
        match &mut self.built {
            Some((was, since)) if *was == at && *since + dt < REFRESH => *since += dt,
            _ => {
                self.look_up(eye, world);
                self.built = Some((at, 0.0));
            }
        }
        // Kept small for the float, not to one picture: the stretches that show go by its whole count.
        let scroll = |speed: f64| (ticks * speed).rem_euclid(1024.0) as f32;
        self.gpu.clear();
        for c in &self.columns {
            let Some(Some((_, picture))) = self.pictures.get(c.kind as usize) else { continue };
            let p = &content().game.precipitations[c.kind as usize];
            let random = (c.seed & 255) as f64 / 255.0;
            let rel = DVec3::new(c.x as f64 + 0.5, 0.0, c.z as f64 + 0.5) - DVec3::new(cam.pos.x, 0.0, cam.pos.z);
            let dist = (rel.length() as f32 / RADIUS as f32).min(1.0);
            // Rain falls quickly, three times vanilla's speed, each column a little differently;
            // what is much slower (snow) drifts sideways as it comes down.
            let speed = p.speed as f64;
            let (slide, drop) = if speed < 0.5 {
                ((ticks * 0.01 * (random - 0.5)).rem_euclid(1.0) as f32, scroll((1.0 / 512.0 + random / 2048.0) * speed / 0.1))
            } else {
                (0.0, scroll((3.0 + random) * 3.0 / 32.0 * speed))
            };
            let alpha = ((1.0 - dist * dist) * 0.5 + 0.5) * level;
            let (bottom, top) = ((c.bottom as f64 - cam.pos.y) as f32, (c.top as f64 - cam.pos.y) as f32);
            self.gpu.push(GpuWeather {
                place: [rel.x as f32, bottom, rel.z as f32, top],
                // A quarter of the picture across, four blocks of it up.
                uv: [((c.seed >> 8) & 3) as f32 * 0.25 + slide, c.bottom as f32 * 0.25 - drop, (c.top - c.bottom) as f32 * 0.25, alpha],
                data: [*picture, c.light, c.seed, (p.density * 255.0) as u32],
            });
        }
        if self.gpu.is_empty() {
            return;
        }
        let n = self.gpu.len() as u32;
        let stride = size_of::<GpuWeather>() as u64;
        let buf = crate::gpu_layout::frame_buffer(gpu, &mut self.instances, frame.slot, n, stride, usage::STORAGE, "weather");
        gpu.mapped(buf)[..self.gpu.len() * stride as usize].copy_from_slice(bytemuck::cast_slice(&self.gpu));
        list.pipeline(pipe);
        list.bind(crate::gpu_layout::bind::weather::COLUMNS, buf);
        list.bind(crate::gpu_layout::bind::weather::SCENE, scene);
        list.push(Cmd::Draw { vertices: 6, instances: n, first_vertex: 0, first_instance: 0 });
    }
}
