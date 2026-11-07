mod attribute;
mod waypoints;

use weft_sdk::*;

pub fn register(r: &mut Registry) {
    attribute::register(r);
    waypoints::register(r);
}
