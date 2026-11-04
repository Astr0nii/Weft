//! `/attribute <attribute> <amount> [who]` adds value to any attribute, and `/attribute clear`
//! clears all set attributes

use weft_sdk::*;

const MODIFIER: &str = "base:attribute_command";

pub fn register(r: &mut Registry) {
    r.command("attribute", "<attribute:attribute> <amount:float> [who:targets]", |cmd| {
        let w = cmd.world();
        let Some(name) = cmd.text("attribute").map(String::from) else { return };
        let attr = Attribute::generated(name.clone());
        let targets = if cmd.given("who") { cmd.targets("who") } else { vec![cmd.player()] };
        let amount = cmd.float("amount").unwrap_or(0.0);
        for &id in &targets {
            w.attribute_add_modifier(id, &attr, MODIFIER, attr_op::ADD, amount, 0);
        }
        cmd.reply("command.base.attribute.done", &[Arg::Text(&name), Arg::Float(amount, 2), Arg::Int(targets.len() as i64)]);
    });
}
