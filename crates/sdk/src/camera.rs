//! Camera modes: where the camera is each frame and what goes with it (the body, the hand, the
//! HUD, blocks in the way). The engine's first person, behind, front and free camera are modes
//! too (`cameras::*`), edited the same way. `shot::*` are ready-made ways to move one; a mod's
//! server puts a player in a mode with `World::force_camera`.
//!
//! ```ignore
//! static ORBIT: CameraMode = CameraMode::named("orbit");
//! static ORBIT_KEY: Key = Key::named("orbit");
//! r.camera(&ORBIT).shot(shot::orbit(6.0)).key(&ORBIT_KEY, "O").transition(0.6, easing::SMOOTH);
//! r.camera(&cameras::FRONT).without(camera_flags::CYCLE); // F5 skips it
//! ```

use crate::abi::{self, CameraArgs, CameraPose, Str, Vec3, camera_flags as cf};
use crate::{CameraMode, Key, Registry, World, callback, host};

pub use abi::{camera_flags, easing};

/// The engine's camera modes.
pub mod cameras {
    use crate::CameraMode;
    pub static FIRST_PERSON: CameraMode = CameraMode::named("core:first_person");
    pub static BEHIND: CameraMode = CameraMode::named("core:behind");
    pub static FRONT: CameraMode = CameraMode::named("core:front");
    /// Detached, a small box that bumps into blocks; `core:freecam` (P), in a game mode that
    /// flies unless a mod says otherwise (`actions::CAMERA`).
    pub static FREE: CameraMode = CameraMode::named("core:free");
}

/// `t` (0..1) eased by `kind` (`easing::*`), the curve camera changes take: for a mod's own
/// moves too.
pub fn ease(kind: u32, t: f32) -> f32 {
    unsafe { (host().ease)(kind, t) }
}

impl CameraMode {
    /// Its id as `actions::CAMERA` asks about it (`Ask::amount`), `u32::MAX` if there's none.
    pub fn camera_id(&self) -> u32 {
        unsafe { (host().camera_id)(Str::new(self.name())) }
    }
}

/// How a camera moves: called each frame on the client with the keys, the player's eyes and
/// last frame's camera (`CameraArgs`), it sets `out`. It brings the flags it needs along
/// (a free camera is `DETACHED`), `CameraBuilder::shot` adds them.
pub struct Shot {
    f: Box<dyn Fn(&mut CameraArgs) + Send + Sync>,
    flags: u32,
}

impl Shot {
    /// Yours: `|a| a.out.pos = ..`, with these flags.
    pub fn new(flags: u32, f: impl Fn(&mut CameraArgs) + Send + Sync + 'static) -> Shot {
        Shot { f: Box::new(f), flags }
    }
}

fn v(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

fn at(p: [f64; 3]) -> Vec3 {
    Vec3 { x: p[0], y: p[1], z: p[2] }
}

/// Where yaw and pitch look, as the engine's.
fn look_dir(yaw: f32, pitch: f32) -> [f64; 3] {
    let (yaw, pitch) = (yaw as f64, pitch as f64);
    [-yaw.sin() * pitch.cos(), pitch.sin(), -yaw.cos() * pitch.cos()]
}

/// Yaw and pitch from `from` towards `to`.
pub fn look_at(from: Vec3, to: Vec3) -> (f32, f32) {
    let d = [to.x - from.x, to.y - from.y, to.z - from.z];
    let flat = (d[0] * d[0] + d[2] * d[2]).sqrt();
    ((-d[0]).atan2(-d[2]) as f32, d[1].atan2(flat) as f32)
}

/// Ready-made shots.
pub mod shot {
    use super::*;

    /// From the eyes, as the player looks.
    pub fn eyes() -> Shot {
        Shot::new(cf::HAND | cf::HUD | cf::EFFECTS, |a| {
            a.out = CameraPose { pos: a.eye, yaw: a.eye_yaw, pitch: a.eye_pitch, ..CameraPose::DEFAULT }
        })
    }

    /// `extra` blocks further back than third person (more on a mount), kept out of blocks.
    pub fn behind(extra: f32) -> Shot {
        Shot::new(cf::BODY | cf::HUD | cf::CLIP, move |a| {
            let d = look_dir(a.eye_yaw, a.eye_pitch);
            let back = (a.back + extra) as f64;
            let e = v(a.eye);
            a.out = CameraPose {
                pos: at([e[0] - d[0] * back, e[1] - d[1] * back, e[2] - d[2] * back]),
                yaw: a.eye_yaw,
                pitch: a.eye_pitch,
                ..CameraPose::DEFAULT
            };
        })
    }

    /// Around the player at `distance`, turned by the mouse, while the player stands.
    pub fn orbit(distance: f32) -> Shot {
        Shot::new(cf::BODY | cf::CLIP | cf::DETACHED | cf::NO_INTERACT, move |a| {
            let d = look_dir(a.keys.yaw, a.keys.pitch);
            let e = v(a.eye);
            let r = distance as f64;
            a.out = CameraPose {
                pos: at([e[0] - d[0] * r, e[1] - d[1] * r, e[2] - d[2] * r]),
                yaw: a.keys.yaw,
                pitch: a.keys.pitch,
                ..CameraPose::DEFAULT
            };
        })
    }

    /// Flies where it looks at `speed` blocks a second (twice sprinting), jump and sneak up and
    /// down, easing into a speed and out of it; the player stands meanwhile. A small box, it
    /// doesn't go through blocks.
    pub fn free(speed: f32) -> Shot {
        let vel = std::sync::Mutex::new([0.0f64; 3]);
        Shot::new(cf::BODY | cf::SOLID | cf::DETACHED | cf::NO_INTERACT, move |a| {
            let mut vel = vel.lock().unwrap();
            if a.first != 0 {
                *vel = [0.0; 3];
            }
            let k = &a.keys;
            let ahead = look_dir(k.yaw, k.pitch);
            let right = [k.yaw.cos() as f64, 0.0, -k.yaw.sin() as f64];
            let up = k.up as f64 - k.down as f64;
            let mut wish = [0.0; 3];
            for i in 0..3 {
                wish[i] = ahead[i] * k.forward as f64 + right[i] * k.strafe as f64 + if i == 1 { up } else { 0.0 };
            }
            let len = (wish[0] * wish[0] + wish[1] * wish[1] + wish[2] * wish[2])
                .sqrt()
                .max(1.0);
            let s = speed as f64 * if k.sprint != 0 { 2.0 } else { 1.0 } / len;
            let blend = (a.dt as f64 * 8.0).min(1.0);
            let mut p = v(a.out.pos);
            for i in 0..3 {
                vel[i] += (wish[i] * s - vel[i]) * blend;
                p[i] += vel[i] * a.dt as f64;
            }
            a.out = CameraPose { pos: at(p), yaw: k.yaw, pitch: k.pitch, ..CameraPose::DEFAULT };
        })
    }

    /// Straight down from `height` blocks over the player, north up (a map, a top-down game).
    pub fn top_down(height: f32) -> Shot {
        Shot::new(cf::BODY | cf::HUD, move |a| {
            let e = v(a.eye);
            a.out = CameraPose { pos: at([e[0], e[1] + height as f64, e[2]]), yaw: 0.0, pitch: -1.5707, ..CameraPose::DEFAULT };
        })
    }

    /// From `pos`, always looking at the player (a security camera).
    pub fn fixed(pos: impl Into<Vec3>) -> Shot {
        let pos = pos.into();
        Shot::new(cf::BODY | cf::HUD, move |a| {
            let (yaw, pitch) = look_at(pos, a.eye);
            a.out = CameraPose { pos, yaw, pitch, ..CameraPose::DEFAULT };
        })
    }

    /// Through `points` (position, yaw, pitch), `secs` from each to the next, eased by `easing`
    /// each way; stays on the last (a cutscene: force it, then release it after).
    pub fn path(points: Vec<(Vec3, f32, f32)>, secs: f32, easing: u32) -> Shot {
        Shot::new(cf::BODY, move |a| {
            let Some(&last) = points.last() else { return };
            let leg = (a.time / secs.max(1e-3)) as usize;
            let (from, to) = match (points.get(leg), points.get(leg + 1)) {
                (Some(&f), Some(&t)) => (f, t),
                _ => (last, last),
            };
            let k = super::ease(easing, a.time / secs.max(1e-3) - leg as f32) as f64;
            let (f, t) = (v(from.0), v(to.0));
            let turn = (to.1 - from.1 + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            a.out = CameraPose {
                pos: at([f[0] + (t[0] - f[0]) * k, f[1] + (t[1] - f[1]) * k, f[2] + (t[2] - f[2]) * k]),
                yaw: from.1 + turn * k as f32,
                pitch: from.2 + (to.2 - from.2) * k as f32,
                ..CameraPose::DEFAULT
            };
        })
    }
}

/// `Registry::camera`: registers when dropped.
pub struct CameraBuilder<'a> {
    reg: &'a mut Registry,
    mode: &'static CameraMode,
    flags: u32,
    off: u32,
    key: Option<&'static Key>,
    shot: Option<Shot>,
    transition: f32,
    easing: u32,
}

impl Registry {
    /// A camera mode, or the edits to one already there (the engine's `cameras::*`, another
    /// mod's): flags added and taken away, its shot, key and change replaced.
    pub fn camera(&mut self, mode: &'static CameraMode) -> CameraBuilder<'_> {
        CameraBuilder { reg: self, mode, flags: 0, off: 0, key: None, shot: None, transition: f32::NAN, easing: easing::IN_OUT }
    }
}

impl CameraBuilder<'_> {
    /// How it moves, with the flags that takes.
    pub fn shot(mut self, shot: Shot) -> Self {
        self.flags |= shot.flags;
        self.shot = Some(shot);
        self
    }
    /// These `camera_flags` on.
    pub fn with(mut self, flags: u32) -> Self {
        (self.flags, self.off) = (self.flags | flags, self.off & !flags);
        self
    }
    /// These off.
    pub fn without(mut self, flags: u32) -> Self {
        (self.flags, self.off) = (self.flags & !flags, self.off | flags);
        self
    }
    /// One F5 goes through.
    pub fn in_cycle(self) -> Self {
        self.with(cf::CYCLE)
    }
    /// Only where the server lets it (`actions::CAMERA`).
    pub fn ask(self) -> Self {
        self.with(cf::ASK)
    }
    /// A key that turns it on and back off, `default` as `Registry::key` takes it.
    pub fn key(mut self, key: &'static Key, default: &str) -> Self {
        unsafe { (host().reg_key)(self.reg.raw, Str::new(key.name()), Str::new(default)) }
        self.key = Some(key);
        self
    }
    /// The change into it takes `secs`, eased by `easing` (`easing::*`).
    pub fn transition(mut self, secs: f32, easing: u32) -> Self {
        (self.transition, self.easing) = (secs, easing);
        self
    }
}

impl Drop for CameraBuilder<'_> {
    fn drop(&mut self) {
        let call = self
            .shot
            .take()
            .map_or(abi::Callback::NONE, |s| callback(move |a: &mut CameraArgs| (s.f)(a)));
        let desc = abi::CameraDesc {
            name: Str::new(self.mode.name()),
            flags: self.flags,
            flags_off: self.off,
            key: self.key.map_or(Str::EMPTY, |k| Str::new(k.name())),
            call,
            transition: self.transition,
            easing: self.easing,
        };
        unsafe { (host().reg_camera)(self.reg.raw, &desc) }
    }
}

impl World {
    /// Puts `player` in camera `mode` and keeps it there until `release_camera`; the change
    /// over `over` (seconds, `easing::*`) or as the mode says.
    pub fn force_camera(&self, player: u64, mode: &CameraMode, over: Option<(f32, u32)>) {
        self.send_camera(player, mode.camera_id(), true, over)
    }
    /// Puts `player` in camera `mode`, which it may leave as usual.
    pub fn set_camera(&self, player: u64, mode: &CameraMode, over: Option<(f32, u32)>) {
        self.send_camera(player, mode.camera_id(), false, over)
    }
    /// Back to the camera the player had chosen, free to change it again.
    pub fn release_camera(&self, player: u64, over: Option<(f32, u32)>) {
        self.send_camera(player, u32::MAX, false, over)
    }
    fn send_camera(&self, player: u64, mode: u32, locked: bool, over: Option<(f32, u32)>) {
        let (secs, easing) = over.unwrap_or((f32::NAN, 0));
        unsafe { (host().player_camera)(self.raw(), player, mode, locked as u32, secs, easing) }
    }
}
