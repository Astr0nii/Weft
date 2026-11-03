//! Text on players' screens. `show` is the one way to put it there; `chat`, `hint`, `title` and
//! `subtitle` are `show` with a place's defaults, and every setting below works on all of them.
//!
//! ```ignore
//! w.hint(player, "bed.base.occupied", &[]);
//! w.title(Who::All, "mymod.round", &[Arg::Int(3)]).times(0.2, 2.0, 0.5);
//! w.show(player, "mymod.timer", &[Arg::Int(left)]).id("mymod:timer").at(0.98, 0.02).scale(2.0);
//! ```

use std::marker::PhantomData;

use crate::abi::{self, Slice, Str, Who, show_place as place};
use crate::ui::raw_args;
use crate::{Arg, Client, Registry, World, callback, host};

#[derive(Clone, Copy)]
enum Target {
    World(World, Who),
    Client(Client),
}

/// Text on its way to the screen: it goes when this is dropped, at the end of the statement.
pub struct Show<'a> {
    target: Target,
    desc: abi::ShowDesc,
    args: Vec<abi::UiArg>,
    text: PhantomData<&'a str>,
}

impl<'a> Show<'a> {
    fn new(target: Target, place: u32, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        let desc = abi::ShowDesc { place, key: Str::new(key), ..abi::ShowDesc::DEFAULT };
        Show { target, desc, args: raw_args(args), text: PhantomData }
    }

    /// New text with the same place and id replaces this one instead of adding another: a timer
    /// counting in one chat line, several free texts side by side. Empty key and no args takes it
    /// away.
    pub fn id(mut self, id: &'a str) -> Self {
        self.desc.id = Str::new(id);
        self
    }

    /// This point of the text at the same point of the window, in fractions of either: (0, 0) the
    /// top left corner of both, (0.5, 0.5) centered, (1, 1) the bottom right corner of both. It
    /// stands by itself then, whatever its place.
    pub fn at(mut self, x: f32, y: f32) -> Self {
        self.desc.at = [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)];
        self
    }

    /// How big, 1 being menu text. A title is 4, a subtitle 2.
    pub fn scale(mut self, scale: f32) -> Self {
        self.desc.scale = scale;
        self
    }

    /// Seconds of fading in, staying and fading out.
    pub fn times(mut self, fade_in: f32, stay: f32, fade_out: f32) -> Self {
        self.desc.times = [fade_in.max(0.0), stay.max(0.0), fade_out.max(0.0)];
        self
    }

    /// `0xRRGGBB`, for what doesn't color itself.
    pub fn color(mut self, rgb: u32) -> Self {
        self.desc.color = (rgb & 0xFFFFFF) as i64;
        self
    }
}

impl Drop for Show<'_> {
    fn drop(&mut self) {
        self.desc.args = Slice::new(&self.args);
        unsafe {
            match self.target {
                Target::World(w, who) => (host().show)(w.raw(), who, &self.desc),
                Target::Client(c) => (host().client_show)(c.ctx(), &self.desc),
            }
        }
    }
}

impl World {
    /// Text by itself, in the middle of the screen unless `at` says otherwise.
    pub fn show<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::FREE, key, args)
    }

    /// A line in the chat, for the player to scroll back to.
    pub fn chat<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::CHAT, key, args)
    }

    /// A short line above the hotbar. A player can move or hide `core:hint` in the HUD editor.
    pub fn hint<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::HINT, key, args)
    }

    /// Big text over the crosshair. A player can move or hide `core:title` in the HUD editor.
    pub fn title<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::TITLE, key, args)
    }

    /// Smaller text under the crosshair. A player can move or hide `core:subtitle` likewise.
    pub fn subtitle<'a>(&self, who: impl Into<Who>, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::World(*self, who.into()), place::SUBTITLE, key, args)
    }
}

impl Client {
    /// `World::show`, for this client alone.
    pub fn show<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::FREE, key, args)
    }

    /// `World::chat`, for this client alone. Not `chat`, which says something as if typed.
    pub fn chat_local<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::CHAT, key, args)
    }

    /// `World::hint`, for this client alone.
    pub fn hint<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::HINT, key, args)
    }

    /// `World::title`, for this client alone.
    pub fn title<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::TITLE, key, args)
    }

    /// `World::subtitle`, for this client alone.
    pub fn subtitle<'a>(&self, key: &'a str, args: &'a [Arg<'a>]) -> Show<'a> {
        Show::new(Target::Client(*self), place::SUBTITLE, key, args)
    }
}

/// A player's message on its way to the chat (`Registry::chat_format`).
pub struct ChatFormat(*mut abi::ChatFormatArgs);

impl ChatFormat {
    pub fn world(&self) -> World {
        unsafe { World::from_raw((*self.0).world) }
    }

    pub fn player(&self) -> u64 {
        unsafe { (*self.0).player }
    }

    /// What they typed. Pass it as `Arg::Text`, or a player gets to format the line.
    pub fn text(&self) -> &str {
        unsafe { (*self.0).text.as_str() }
    }

    /// The line everyone gets. Not called, it stays the engine's `<name> text`.
    pub fn set(&self, key: &str, args: &[Arg]) {
        let raw = raw_args(args);
        unsafe { (host().chat_format_set)(self.0, Str::new(key), Slice::new(&raw)) }
    }
}

impl Registry {
    /// Writes players' chat lines. There is one line, so one writer: the last mod loaded wins. To
    /// keep a message out of the chat use `question::CHAT`, to read them `CHAT_MESSAGE`.
    pub fn chat_format(&mut self, f: impl Fn(&mut ChatFormat) + Send + Sync + 'static) {
        let cb = callback(move |args: &mut abi::ChatFormatArgs| f(&mut ChatFormat(args)));
        unsafe { (host().reg_chat_format)(self.raw, cb) }
    }
}
