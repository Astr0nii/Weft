//! A camera of its own: around the player on O, the change into it eased.

use weft_sdk::*;

pub static ORBIT: CameraMode = CameraMode::named("orbit");
static ORBIT_KEY: Key = Key::named("orbit");

pub fn register(r: &mut Registry) {
    r.camera(&ORBIT)
        .shot(shot::orbit(6.0))
        .key(&ORBIT_KEY, "O")
        .transition(0.5, easing::SMOOTH);
}
