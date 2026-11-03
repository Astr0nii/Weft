//! The client's options as the engine reads them: typed views of its `core:` settings
//! (`config::engine_configs`), which live in `config/core.txt` like every mod's. Setting one
//! saves it; the client applies what changed by itself (`Running::apply_settings`).

use std::path::{Path, PathBuf};

use crate::config;
use crate::input::{Chord, Input};

#[derive(Clone)]
pub struct Settings {
    pub run_dir: PathBuf,
    /// An automated run: it saves nothing and never takes the real mouse.
    pub read_only: bool,
}

/// The largest GUI scale a window shows whole (vanilla's 320x240 per step); auto uses it.
pub fn max_gui_scale(window: (u32, u32)) -> u32 {
    (window.0 / 320).min(window.1 / 240).max(1)
}

fn id(name: &str) -> u32 {
    config::by_name(name).unwrap_or(u32::MAX)
}

fn number(name: &str) -> f64 {
    config::get(id(name))
}

/// How the chat is shown, as its settings have it.
#[derive(Clone, Copy, PartialEq)]
pub struct ChatLook {
    pub width: f32,
    /// The most it takes up open, and shown over the game.
    pub open_height: f32,
    pub closed_height: f32,
    /// Of the dark behind it, 0..255.
    pub dim: u8,
}

/// Which chat lines a player wants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChatVisibility {
    All,
    System,
    Hidden,
}

pub fn chat_visibility() -> ChatVisibility {
    match number("core:chat_visibility") as u32 {
        0 => ChatVisibility::All,
        1 => ChatVisibility::System,
        _ => ChatVisibility::Hidden,
    }
}

pub fn chat_look() -> ChatLook {
    ChatLook {
        width: number("core:chat_width") as f32,
        open_height: number("core:chat_height") as f32,
        closed_height: number("core:chat_height_closed") as f32,
        dim: (number("core:chat_opacity") * 2.55) as u8,
    }
}

fn list(name: &str) -> Vec<String> {
    config::text(id(name))
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

impl Settings {
    pub fn new(run_dir: &Path) -> Settings {
        Settings { run_dir: run_dir.to_path_buf(), read_only: false }
    }

    pub fn language(&self) -> String {
        config::text(id("core:language"))
    }
    /// 0 = automatic.
    pub fn gui_scale(&self) -> u32 {
        number("core:gui_scale") as u32
    }
    pub fn view_distance(&self) -> i32 {
        number("core:view_distance") as i32
    }
    pub fn vsync(&self) -> bool {
        number("core:vsync") != 0.0
    }
    pub fn fov(&self) -> f32 {
        number("core:fov") as f32
    }
    pub fn sensitivity(&self) -> f32 {
        number("core:sensitivity") as f32
    }
    /// 0 moody .. 1 bright.
    pub fn brightness(&self) -> f32 {
        number("core:brightness") as f32
    }
    /// Bit per debug overlay group shown (F3 + number).
    pub fn debug_groups(&self) -> u32 {
        number("core:debug_groups") as u32
    }
    /// Mouse up looks down instead.
    pub fn invert_y(&self) -> bool {
        number("core:invert_y") != 0.0
    }
    /// The sneak key switches sneaking on and off instead of being held.
    pub fn toggle_sneak(&self) -> bool {
        number("core:toggle_sneak") != 0.0
    }
    /// The sprint key switches sprinting on and off instead of being held.
    pub fn toggle_sprint(&self) -> bool {
        number("core:toggle_sprint") != 0.0
    }
    pub fn name(&self) -> String {
        config::text(id("core:name"))
    }
    pub fn server_port(&self) -> u16 {
        number("core:server_port") as u16
    }
    /// Extra iroh relay URLs; empty uses the public defaults.
    pub fn relays(&self) -> Vec<String> {
        list("core:relays")
    }
    /// Enabled resource packs, highest priority first.
    pub fn resource_packs(&self) -> Vec<String> {
        list("core:resource_packs")
    }
    pub fn shader_pack(&self) -> Option<String> {
        Some(config::text(id("core:shader_pack"))).filter(|p| !p.is_empty())
    }

    pub fn occlusion_culling(&self) -> bool {
        number("core:occlusion_culling") != 0.0
    }

    pub fn show_avatars(&self) -> bool {
        number("core:show_avatars") != 0.0
    }

    pub fn first_person_model(&self) -> bool {
        number("core:first_person_model") != 0.0
    }

    pub fn set(&self, name: &str, v: f64) {
        config::set(id(&format!("core:{name}")), v);
    }
    pub fn set_text(&self, name: &str, v: &str) {
        config::set_text(id(&format!("core:{name}")), v);
    }
    pub fn set_resource_packs(&self, packs: &[String]) {
        self.set_text("resource_packs", &packs.join(","));
    }

    pub fn saves_dir(&self) -> PathBuf {
        self.run_dir.join("saves")
    }

    /// Every key as its `core:key.<action>` setting says.
    pub fn apply_binds(&self, input: &mut Input) {
        let actions: Vec<String> = input.binds.iter().map(|b| b.action.clone()).collect();
        for action in actions {
            match Chord::parse(&config::text(id(&format!("core:key.{action}")))) {
                Some(k) => input.rebind(&action, k),
                None => log::warn!("key binding {action}: unknown key {}", config::text(id(&format!("core:key.{action}")))),
            }
        }
    }

    pub fn set_bind(&self, action: &str, key: Chord) {
        config::set_text(id(&format!("core:key.{action}")), &Chord::id(key));
    }
}

/// The old `config/settings.txt` joins `config/core.txt` once (lists were one line per entry).
pub fn migrate(run_dir: &Path) {
    let config = crate::paths::config(run_dir);
    let (old, new) = (config.join("settings.txt"), config.join("core.txt"));
    let Ok(src) = std::fs::read_to_string(&old) else { return };
    let mut out = std::fs::read_to_string(&new).unwrap_or_default();
    let (mut packs, mut relays) = (Vec::new(), Vec::new());
    for (_, k, v) in crate::kv::pairs(&src) {
        match k {
            "resource_pack" => packs.push(v),
            "relay" => relays.push(v),
            _ => out.push_str(&format!(
                "{k} = {v}
"
            )),
        }
    }
    out.push_str(&format!(
        "resource_packs = {}
relays = {}
",
        packs.join(","),
        relays.join(",")
    ));
    if std::fs::write(&new, out).is_ok() {
        let _ = std::fs::remove_file(&old);
    }
}

pub fn system_language() -> String {
    let lang = std::env::var("LANG").unwrap_or_default().to_lowercase();
    if lang.starts_with("ru") || sys_locale_ru() { "ru_ru".into() } else { "en_us".into() }
}

#[cfg(windows)]
fn sys_locale_ru() -> bool {
    unsafe extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
    }
    // Primary language id 0x19 is Russian.
    unsafe { GetUserDefaultUILanguage() & 0x3FF == 0x19 }
}

#[cfg(not(windows))]
fn sys_locale_ru() -> bool {
    false
}
