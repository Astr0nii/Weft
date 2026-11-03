//! What the server and a client both can read, so one check serves both sides:
//!
//! ```ignore
//! fn cold(v: &impl View, pos: [i32; 3]) -> bool { v.raining() && v.light(pos).unwrap_or(0) < 8 }
//! cold(&w, pos);          // in a world callback
//! cold(&c.view(), pos);   // on the client
//! ```
//!
//! A client knows less than the server: blocks and light only where it has the world loaded,
//! other entities' components only those synced to it (`SYNC_WATCHERS`), others' game modes
//! and attributes not at all. Those come back as `None`, survival, and NaN.

use crate::abi::{self, EntityState};
use crate::component::{Component, Value};
use crate::{ClientView, World};

pub trait View {
    fn raining(&self) -> bool;
    fn day_time(&self) -> u64;
    /// The block at `pos`, `None` where it isn't loaded.
    fn block(&self, pos: [i32; 3]) -> Option<u32>;
    /// Light as the world stores it (`abi::light`), `None` where it isn't loaded.
    fn light(&self, pos: [i32; 3]) -> Option<u32>;
    fn entity(&self, id: u64) -> Option<EntityState>;
    fn component_of<T: Value>(&self, id: u64, c: &Component<T>) -> Option<T>;
    /// `abi::game_mode`.
    fn game_mode_of(&self, id: u64) -> u32;
    fn attribute_of(&self, id: u64, a: &crate::Attribute) -> f64;
}

impl View for World {
    fn raining(&self) -> bool {
        World::raining(self)
    }
    fn day_time(&self) -> u64 {
        World::day_time(self)
    }
    fn block(&self, pos: [i32; 3]) -> Option<u32> {
        self.block_loaded(pos)
    }
    fn light(&self, pos: [i32; 3]) -> Option<u32> {
        self.block_loaded(pos).map(|_| World::light(self, pos))
    }
    fn entity(&self, id: u64) -> Option<EntityState> {
        World::entity(self, id)
    }
    fn component_of<T: Value>(&self, id: u64, c: &Component<T>) -> Option<T> {
        self.component(id, c).copied()
    }
    fn game_mode_of(&self, id: u64) -> u32 {
        self.game_mode(id)
    }
    fn attribute_of(&self, id: u64, a: &crate::Attribute) -> f64 {
        self.attribute(id, a)
    }
}

impl View for ClientView {
    fn raining(&self) -> bool {
        ClientView::raining(self)
    }
    fn day_time(&self) -> u64 {
        ClientView::day_time(self)
    }
    fn block(&self, pos: [i32; 3]) -> Option<u32> {
        self.client().block(pos)
    }
    fn light(&self, pos: [i32; 3]) -> Option<u32> {
        self.client().light(pos)
    }
    fn entity(&self, id: u64) -> Option<EntityState> {
        self.client().entity(id)
    }
    fn component_of<T: Value>(&self, id: u64, c: &Component<T>) -> Option<T> {
        if id == self.player() { self.component(c) } else { ClientView::component_of(self, id, c) }
    }
    fn game_mode_of(&self, id: u64) -> u32 {
        if id == self.player() { self.game_mode() } else { abi::game_mode::SURVIVAL }
    }
    fn attribute_of(&self, id: u64, a: &crate::Attribute) -> f64 {
        if id == self.player() { self.attribute(a) } else { f64::NAN }
    }
}
