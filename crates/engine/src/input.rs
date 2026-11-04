//! Raw input plus named actions. Every action (engine or mod) is registered here with its default
//! bindings, so rebinding and conflict detection work on one table. An action has up to `SLOTS`
//! bindings, each a key held, switched on and off, or tapped twice (`Trigger`); `held` and
//! `pressed` answer for all of them, so no caller knows which.

use std::time::{Duration, Instant};

use glam::Vec2;
use rustc_hash::{FxHashMap, FxHashSet};
use winit::keyboard::KeyCode;

/// The hotbar slot actions, first to ninth.
pub const HOTBAR: [&str; 9] = [
    "core:hotbar1",
    "core:hotbar2",
    "core:hotbar3",
    "core:hotbar4",
    "core:hotbar5",
    "core:hotbar6",
    "core:hotbar7",
    "core:hotbar8",
    "core:hotbar9",
];

/// The engine's actions and their default bindings (`Binding::parse_slots`).
pub const ENGINE_KEYS: &[(&str, &str)] = &[
    ("core:forward", "KeyW"),
    ("core:back", "KeyS"),
    ("core:left", "KeyA"),
    ("core:right", "KeyD"),
    ("core:jump", "Space"),
    ("core:sneak", "ShiftLeft"),
    ("core:dismount", "ShiftLeft"),
    // Vanilla's double tap of forward is a second way to sprint.
    ("core:sprint", "ControlLeft, double KeyW"),
    ("core:crawl", "KeyC"),
    // And of jump to fly.
    ("core:fly", "KeyF, double Space"),
    ("core:inventory", "KeyE"),
    ("core:drop", "KeyQ"),
    ("core:command", "Slash"),
    ("core:chat", "KeyT"),
    ("core:debug", "F3"),
    ("core:screenshot", "F2"),
    ("core:perspective", "F5"),
    ("core:fullscreen", "F11"),
    ("core:player_list", "Tab"),
    ("core:hide_hud", "F1"),
    ("core:swap_hands", "KeyG"),
    // Also F3+T, like vanilla.
    ("core:reload", "F8"),
    ("core:recipes", "KeyR"),
    ("core:uses", "KeyU"),
    ("core:attack", "mouse0"),
    ("core:use", "mouse1"),
    ("core:pick", "mouse2"),
    ("core:hotbar1", "Digit1"),
    ("core:hotbar2", "Digit2"),
    ("core:hotbar3", "Digit3"),
    ("core:hotbar4", "Digit4"),
    ("core:hotbar5", "Digit5"),
    ("core:hotbar6", "Digit6"),
    ("core:hotbar7", "Digit7"),
    ("core:hotbar8", "Digit8"),
    ("core:hotbar9", "Digit9"),
];

/// Two presses closer than this are a double tap (vanilla's 7 ticks).
pub const DOUBLE_TAP: Duration = Duration::from_millis(350);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Code(KeyCode),
    Mouse(u8),
}

/// Modifier bits of a `Chord`.
pub const CTRL: u8 = 1;
pub const SHIFT: u8 = 2;
pub const ALT: u8 = 4;

/// A key and the modifiers held with it (`ctrl+T`). A modifier key is also a key of its own
/// (sneak on Shift).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: Key,
    pub mods: u8,
}

impl From<Key> for Chord {
    fn from(key: Key) -> Chord {
        Chord { key, mods: 0 }
    }
}

/// The modifiers: its `Chord` bit, what it is called, and the two keys that are it.
pub const MODIFIERS: [(u8, &str, KeyCode, KeyCode); 3] = [
    (CTRL, "ctrl", KeyCode::ControlLeft, KeyCode::ControlRight),
    (SHIFT, "shift", KeyCode::ShiftLeft, KeyCode::ShiftRight),
    (ALT, "alt", KeyCode::AltLeft, KeyCode::AltRight),
];

/// The `Chord` bit of a modifier key, 0 for others.
pub fn modifier(k: Key) -> u8 {
    MODIFIERS
        .iter()
        .find(|m| k == Key::Code(m.2) || k == Key::Code(m.3))
        .map_or(0, |m| m.0)
}

/// How a binding's key sets its action off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Trigger {
    /// While the key is down.
    #[default]
    Hold,
    /// From one press to the next.
    Toggle,
    /// From a second press in quick succession (`DOUBLE_TAP`) for as long as the key stays down.
    Double,
}

/// One way to set an action off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    pub chord: Chord,
    pub trigger: Trigger,
}

/// How many bindings an action has.
pub const SLOTS: usize = 2;

pub type Slots = [Option<Binding>; SLOTS];

impl Binding {
    /// `ctrl+KeyT`, `toggle KeyW`, `double Space`.
    pub fn id(self) -> String {
        let chord = self.chord.id();
        match self.trigger {
            Trigger::Hold => chord,
            Trigger::Toggle => format!("toggle {chord}"),
            Trigger::Double => format!("double {chord}"),
        }
    }

    pub fn parse(s: &str) -> Option<Binding> {
        let s = s.trim();
        let (trigger, chord) = match s.split_once(' ') {
            Some(("toggle", c)) => (Trigger::Toggle, c),
            Some(("double", c)) => (Trigger::Double, c),
            _ => (Trigger::Hold, s),
        };
        Some(Binding { chord: Chord::parse(chord)?, trigger })
    }

    /// The bindings of one action, by slot, `none` for an empty one: `ControlLeft, double KeyW`.
    pub fn parse_slots(s: &str) -> Option<Slots> {
        let mut out = [None; SLOTS];
        for (slot, part) in s.split(',').map(str::trim).enumerate() {
            match part {
                "" | "none" => {}
                _ if slot >= SLOTS => return None,
                p => out[slot] = Some(Binding::parse(p)?),
            }
        }
        Some(out)
    }

    pub fn slots_id(slots: &Slots) -> String {
        let last = slots.iter().rposition(Option::is_some).map_or(0, |i| i + 1);
        slots[..last]
            .iter()
            .map(|b| b.map_or("none".into(), Binding::id))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub struct Bind {
    pub action: String,
    pub default: Slots,
    pub keys: Slots,
    /// Per slot: switched on (a toggle, or a double tap while its key stays down), set off this
    /// frame, and a double tap's first press.
    on: [bool; SLOTS],
    fired: [bool; SLOTS],
    tapped: [Option<Instant>; SLOTS],
}

impl Bind {
    fn new(action: &str, default: Slots) -> Bind {
        Bind { action: action.to_string(), default, keys: default, on: [false; SLOTS], fired: [false; SLOTS], tapped: [None; SLOTS] }
    }
}

pub struct Input {
    pub captured: bool,
    pub mouse_delta: Vec2,
    pub cursor: Vec2,
    pub scroll: f32,
    pub sensitivity: f32,
    pub invert_y: bool,
    pub typed: String,
    held: FxHashSet<Key>,
    pressed: FxHashSet<Key>,
    released: FxHashSet<Key>,
    repeated: FxHashSet<Key>,
    pub binds: Vec<Bind>,
    lookup: FxHashMap<String, usize>,
}

impl Default for Input {
    fn default() -> Self {
        let mut i = Input {
            captured: false,
            mouse_delta: Vec2::ZERO,
            cursor: Vec2::ZERO,
            scroll: 0.0,
            sensitivity: 1.0,
            invert_y: false,
            typed: String::new(),
            held: FxHashSet::default(),
            pressed: FxHashSet::default(),
            released: FxHashSet::default(),
            repeated: FxHashSet::default(),
            binds: Vec::new(),
            lookup: FxHashMap::default(),
        };
        for &(name, keys) in ENGINE_KEYS {
            i.register(name, Binding::parse_slots(keys).expect("engine key"));
        }
        i
    }
}

impl Input {
    pub fn register(&mut self, action: &str, default: Slots) {
        if self.lookup.contains_key(action) {
            return;
        }
        self.lookup.insert(action.to_string(), self.binds.len());
        self.binds.push(Bind::new(action, default));
    }

    pub fn rebind(&mut self, action: &str, keys: Slots) {
        if let Some(&i) = self.lookup.get(action) {
            self.binds[i] = Bind { keys, ..Bind::new(action, self.binds[i].default) };
        }
    }

    /// Actions with the same binding as another action.
    pub fn conflicts(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (i, a) in self.binds.iter().enumerate() {
            for b in &self.binds[i + 1..] {
                if a.keys.iter().flatten().any(|k| b.keys.contains(&Some(*k))) {
                    out.push((a.action.clone(), b.action.clone()));
                }
            }
        }
        out
    }

    /// The first binding of an action, for showing it and for pressing it on a player's behalf.
    pub fn binding_of(&self, action: &str) -> Option<Binding> {
        self.lookup
            .get(action)
            .and_then(|&i| self.binds[i].keys.iter().flatten().next().copied())
    }

    pub fn held(&self, action: &str) -> bool {
        self.lookup.get(action).is_some_and(|&i| {
            let b = &self.binds[i];
            (0..SLOTS).any(|s| match b.keys[s] {
                Some(Binding { chord, trigger: Trigger::Hold }) => self.active(chord, &self.held),
                Some(_) => b.on[s],
                None => false,
            })
        })
    }

    pub fn pressed(&self, action: &str) -> bool {
        self.lookup.get(action).is_some_and(|&i| {
            let b = &self.binds[i];
            (0..SLOTS).any(|s| match b.keys[s] {
                Some(Binding { chord, trigger: Trigger::Hold }) => self.active(chord, &self.pressed),
                Some(_) => b.fired[s],
                None => false,
            })
        })
    }

    /// Switches the action off until its key is pressed again (a toggled sprint the server
    /// ended); held keys still count.
    pub fn stop(&mut self, action: &str) {
        if let Some(&i) = self.lookup.get(action) {
            (self.binds[i].on, self.binds[i].tapped) = ([false; SLOTS], [None; SLOTS]);
        }
    }

    /// Modifiers held now (`Chord` bits).
    pub fn mods(&self) -> u8 {
        self.held.iter().fold(0, |m, &k| m | modifier(k))
    }

    /// The chord is down: its key in `keys`, its modifiers held, and no binding of the same key
    /// with more modifiers, all held, takes it over (`ctrl+T` rather than `T`).
    fn active(&self, c: Chord, keys: &FxHashSet<Key>) -> bool {
        if !keys.contains(&c.key) {
            return false;
        }
        let held = self.mods();
        held & c.mods == c.mods
            && !self
                .binds
                .iter()
                .flat_map(|b| b.keys.iter().flatten())
                .any(|b| {
                    b.chord.key == c.key && b.chord.mods != c.mods && b.chord.mods & c.mods == c.mods && held & b.chord.mods == b.chord.mods
                })
    }

    /// Toggles and double taps see a key go down. They switch only while the game has the
    /// mouse, so typing in a screen leaves them be.
    fn switch(&mut self, k: Key) {
        if !self.captured {
            return;
        }
        let now = Instant::now();
        for i in 0..self.binds.len() {
            for s in 0..SLOTS {
                let Some(b) = self.binds[i].keys[s].filter(|b| b.chord.key == k && b.trigger != Trigger::Hold) else { continue };
                if !self.active(b.chord, &self.held) {
                    continue;
                }
                let bind = &mut self.binds[i];
                match b.trigger {
                    Trigger::Toggle => {
                        bind.on[s] = !bind.on[s];
                        bind.fired[s] = bind.on[s];
                    }
                    _ => match bind.tapped[s].take() {
                        Some(t) if now.duration_since(t) < DOUBLE_TAP => (bind.on[s], bind.fired[s]) = (true, true),
                        _ => bind.tapped[s] = Some(now),
                    },
                }
            }
        }
    }

    /// A double tap lasts while its key is down.
    fn let_go(&mut self, k: Key) {
        for b in &mut self.binds {
            for s in 0..SLOTS {
                if b.keys[s].is_some_and(|b| b.chord.key == k && b.trigger == Trigger::Double) {
                    b.on[s] = false;
                }
            }
        }
    }

    pub fn held_key(&self, k: Key) -> bool {
        self.held.contains(&k)
    }

    pub fn pressed_key(&self, k: Key) -> bool {
        self.pressed.contains(&k)
    }

    pub fn pressed_list(&self) -> Vec<Key> {
        self.pressed.iter().copied().collect()
    }

    /// Keys let go of this frame.
    pub fn released_list(&self) -> Vec<Key> {
        self.released.iter().copied().collect()
    }

    pub fn held_button(&self, b: u8) -> bool {
        self.held.contains(&Key::Mouse(b))
    }

    pub fn pressed_button(&self, b: u8) -> bool {
        self.pressed.contains(&Key::Mouse(b))
    }

    /// Pressed this frame, or held long enough for the system's key repeat to fire.
    pub fn pressed_or_repeated(&self, k: Key) -> bool {
        self.pressed.contains(&k) || self.repeated.contains(&k)
    }

    pub fn key_repeat(&mut self, k: Key) {
        self.repeated.insert(k);
    }

    pub fn key_down(&mut self, k: Key) {
        if self.held.insert(k) {
            self.pressed.insert(k);
            self.switch(k);
        }
    }

    pub fn key_up(&mut self, k: Key) {
        if self.held.remove(&k) {
            self.released.insert(k);
            self.let_go(k);
        }
    }

    pub fn release_all(&mut self) {
        for k in std::mem::take(&mut self.held) {
            self.let_go(k);
        }
    }

    /// Clears per-frame state after the frame consumed it.
    pub fn end_frame(&mut self) {
        for b in &mut self.binds {
            b.fired = [false; SLOTS];
        }
        self.pressed.clear();
        self.released.clear();
        self.repeated.clear();
        self.mouse_delta = Vec2::ZERO;
        self.scroll = 0.0;
        self.typed.clear();
    }
}

impl Key {
    /// What it is called in settings and to mods: `KeyR`, `Enter`, `mouse0`. `key_name` is what a
    /// player reads.
    pub fn id(self) -> String {
        match self {
            Key::Mouse(b) => format!("mouse{b}"),
            Key::Code(c) => format!("{c:?}"),
        }
    }

    pub fn parse(s: &str) -> Option<Key> {
        if let Some(b) = s.strip_prefix("mouse") {
            return b.parse().ok().map(Key::Mouse);
        }
        use winit::keyboard::KeyCode::*;
        const ALL: &[winit::keyboard::KeyCode] = &[
            KeyA,
            KeyB,
            KeyC,
            KeyD,
            KeyE,
            KeyF,
            KeyG,
            KeyH,
            KeyI,
            KeyJ,
            KeyK,
            KeyL,
            KeyM,
            KeyN,
            KeyO,
            KeyP,
            KeyQ,
            KeyR,
            KeyS,
            KeyT,
            KeyU,
            KeyV,
            KeyW,
            KeyX,
            KeyY,
            KeyZ,
            Digit0,
            Digit1,
            Digit2,
            Digit3,
            Digit4,
            Digit5,
            Digit6,
            Digit7,
            Digit8,
            Digit9,
            F1,
            F2,
            F3,
            F4,
            F5,
            F6,
            F7,
            F8,
            F9,
            F10,
            F11,
            F12,
            Space,
            Enter,
            Tab,
            Backspace,
            Escape,
            ShiftLeft,
            ShiftRight,
            ControlLeft,
            ControlRight,
            AltLeft,
            AltRight,
            ArrowUp,
            ArrowDown,
            ArrowLeft,
            ArrowRight,
            Minus,
            Equal,
            BracketLeft,
            BracketRight,
            Backslash,
            Semicolon,
            Quote,
            Comma,
            Period,
            Slash,
            Backquote,
            CapsLock,
            Insert,
            Delete,
            Home,
            End,
            PageUp,
            PageDown,
            Numpad0,
            Numpad1,
            Numpad2,
            Numpad3,
            Numpad4,
            Numpad5,
            Numpad6,
            Numpad7,
            Numpad8,
            Numpad9,
            NumpadAdd,
            NumpadSubtract,
            NumpadMultiply,
            NumpadDivide,
            NumpadDecimal,
            NumpadEnter,
            NumpadEqual,
            NumLock,
            ScrollLock,
            PrintScreen,
            Pause,
            ContextMenu,
            IntlBackslash,
            F13,
            F14,
            F15,
            F16,
            F17,
            F18,
            F19,
            F20,
            F21,
            F22,
            F23,
            F24,
            SuperLeft,
            SuperRight,
        ];
        // Short names ("R", "5", "space", "lshift") as well as winit's ("KeyR", "Digit5", "ShiftLeft").
        let short = match s.to_ascii_lowercase().as_str() {
            l if l.len() == 1 && l.as_bytes()[0].is_ascii_alphabetic() => format!("Key{}", l.to_ascii_uppercase()),
            l if l.len() == 1 && l.as_bytes()[0].is_ascii_digit() => format!("Digit{l}"),
            "lshift" | "shift" => "ShiftLeft".into(),
            "rshift" => "ShiftRight".into(),
            "lctrl" | "ctrl" | "control" => "ControlLeft".into(),
            "rctrl" => "ControlRight".into(),
            "lalt" | "alt" => "AltLeft".into(),
            "ralt" => "AltRight".into(),
            "esc" => "Escape".into(),
            "up" => "ArrowUp".into(),
            "down" => "ArrowDown".into(),
            "left" => "ArrowLeft".into(),
            "right" => "ArrowRight".into(),
            _ => s.into(),
        };
        ALL.iter()
            .find(|c| format!("{c:?}").eq_ignore_ascii_case(&short))
            .map(|c| Key::Code(*c))
    }
}

impl Chord {
    /// `ctrl+shift+KeyT`: the modifiers, then the key.
    pub fn id(self) -> String {
        let mut out = String::new();
        for (bit, name, ..) in MODIFIERS {
            if self.mods & bit != 0 {
                out += name;
                out.push('+');
            }
        }
        out + &self.key.id()
    }

    /// A key with modifiers in front (`ctrl+T`, `shift+alt+F6`), or a key alone.
    pub fn parse(s: &str) -> Option<Chord> {
        let mut parts: Vec<&str> = s.split('+').map(str::trim).collect();
        let key = Key::parse(parts.pop()?)?;
        let mut mods = 0;
        for p in parts {
            mods |= MODIFIERS
                .iter()
                .find(|m| m.1.eq_ignore_ascii_case(p) || (m.1 == "ctrl" && p.eq_ignore_ascii_case("control")))?
                .0;
        }
        Some(Chord { key, mods })
    }
}

pub fn key_name(k: Key) -> String {
    match k {
        Key::Mouse(0) => "LMB".into(),
        Key::Mouse(1) => "RMB".into(),
        Key::Mouse(2) => "MMB".into(),
        Key::Mouse(n) => format!("Mouse{}", n + 1),
        Key::Code(c) => {
            let s = format!("{c:?}");
            s.strip_prefix("Key")
                .or_else(|| s.strip_prefix("Digit"))
                .unwrap_or(&s)
                .to_string()
        }
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;

    fn input(action: &str, keys: &str) -> Input {
        let mut i = Input { captured: true, ..Input::default() };
        i.rebind(action, Binding::parse_slots(keys).unwrap());
        i
    }

    fn tap(i: &mut Input, k: KeyCode) {
        i.key_down(Key::Code(k));
        i.key_up(Key::Code(k));
        i.end_frame();
    }

    #[test]
    fn bindings_read_back() {
        for s in ["ControlLeft, double KeyW", "none, toggle ctrl+KeyT", "F3"] {
            assert_eq!(Binding::slots_id(&Binding::parse_slots(s).unwrap()), s);
        }
        assert!(Binding::parse_slots("KeyA, KeyB, KeyC").is_none(), "more than two");
        assert!(Binding::parse_slots("triple KeyA").is_none());
    }

    #[test]
    fn a_toggle_stays_on_until_pressed_again() {
        let mut i = input("core:sneak", "toggle ShiftLeft");
        i.key_down(Key::Code(KeyCode::ShiftLeft));
        assert!(i.pressed("core:sneak"));
        i.key_up(Key::Code(KeyCode::ShiftLeft));
        i.end_frame();
        assert!(i.held("core:sneak") && !i.pressed("core:sneak"));
        tap(&mut i, KeyCode::ShiftLeft);
        assert!(!i.held("core:sneak"));
        // Not while typing in a screen.
        i.captured = false;
        tap(&mut i, KeyCode::ShiftLeft);
        assert!(!i.held("core:sneak"));
    }

    #[test]
    fn a_double_tap_lasts_while_held_and_either_slot_counts() {
        let mut i = input("core:sprint", "ControlLeft, double KeyW");
        tap(&mut i, KeyCode::KeyW);
        assert!(!i.held("core:sprint"), "one tap");
        i.key_down(Key::Code(KeyCode::KeyW));
        assert!(i.pressed("core:sprint") && i.held("core:sprint"));
        i.key_up(Key::Code(KeyCode::KeyW));
        assert!(!i.held("core:sprint"));
        i.key_down(Key::Code(KeyCode::ControlLeft));
        assert!(i.held("core:sprint"));
        // Stopped, a toggle or a double tap waits for its key again; a held key still counts.
        let mut i = input("core:sprint", "toggle ControlLeft");
        tap(&mut i, KeyCode::ControlLeft);
        i.stop("core:sprint");
        assert!(!i.held("core:sprint"));
    }

    #[test]
    fn key_names() {
        assert_eq!(Key::parse("R"), Some(Key::Code(KeyCode::KeyR)));
        assert_eq!(Key::parse("KeyR"), Some(Key::Code(KeyCode::KeyR)));
        assert_eq!(Key::parse("5"), Some(Key::Code(KeyCode::Digit5)));
        assert_eq!(Key::parse("space"), Some(Key::Code(KeyCode::Space)));
        assert_eq!(Key::parse("LShift"), Some(Key::Code(KeyCode::ShiftLeft)));
        assert_eq!(Key::parse("F6"), Some(Key::Code(KeyCode::F6)));
        assert_eq!(Key::parse("mouse3"), Some(Key::Mouse(3)));
        assert_eq!(Key::parse("Rr"), None);
        assert_eq!(Chord::parse("ctrl+T"), Some(Chord { key: Key::Code(KeyCode::KeyT), mods: crate::input::CTRL }));
        assert_eq!(
            Chord::parse("Shift + Alt + F6"),
            Some(Chord { key: Key::Code(KeyCode::F6), mods: crate::input::SHIFT | crate::input::ALT })
        );
        assert_eq!(Chord::parse("LShift"), Some(Chord { key: Key::Code(KeyCode::ShiftLeft), mods: 0 }));
        assert_eq!(Chord::parse("meta+T"), None);
        let c = Chord::parse("ctrl+shift+R").unwrap();
        assert_eq!(Chord::parse(&Chord::id(c)), Some(c));
    }

    #[test]
    fn what_a_key_is_called_reads_back() {
        for k in [Key::Mouse(0), Key::Mouse(3), Key::Code(KeyCode::KeyR), Key::Code(KeyCode::Digit5), Key::Code(KeyCode::Enter)] {
            assert_eq!(Key::parse(&k.id()), Some(k));
        }
    }
}
