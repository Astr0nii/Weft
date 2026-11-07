//! Loot tables: pools of weighted entries with counts and conditions, for blocks (the table named
//! like the block, by default its own item when harvested), entities (named like the kind) and anything a mod
//! names (chests). Each is an entry (`abi::comp::loot`) other mods edit like any.

use rustc_hash::FxHashMap;
use weft_abi::{self as abi, ItemStack, loot_cond as lc};

use crate::components::Components;

#[derive(Clone, Debug)]
pub struct Cond {
    /// `BLOCK_STATE`: the property and its value, `age=7`.
    pub prop: String,
    pub kind: u32,
    pub value: f32,
    /// Component id for tag conditions (`u32::MAX` = a tag nothing has).
    pub tag: u32,
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// 0 = nothing.
    pub item: u32,
    pub weight: u32,
    pub min: u32,
    pub max: u32,
    pub conds: Vec<Cond>,
    /// Recipe type that cooks it when the victim burns.
    pub smelt: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Pool {
    pub rolls: (u32, u32),
    pub conds: Vec<Cond>,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Default)]
pub struct Table {
    pub pools: Vec<Pool>,
}

/// What a roll knows about the situation.
#[derive(Clone, Default)]
pub struct Ctx {
    pub tool: ItemStack,
    pub by_player: bool,
    pub on_fire: bool,
    /// The block state mined with `tool` (0 = none, or broken by the world).
    pub mined: u32,
    /// The block state the table is rolled for, however it was broken (0 for none).
    pub state: u32,
    /// The conditions (`loot_cond::CONDITION`) that hold for this roll, by name.
    pub holds: Vec<String>,
}

/// The names of conditions a table asks, for whoever rolls it to answer first.
pub fn conditions_in(t: &Table) -> Vec<&str> {
    let mut out: Vec<&str> = t
        .pools
        .iter()
        .flat_map(|p| {
            p.conds
                .iter()
                .chain(p.entries.iter().flat_map(|e| &e.conds))
        })
        .filter(|cd| cd.kind == lc::CONDITION)
        .map(|cd| cd.prop.as_str())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

// ---------------------------------------------------------------- registration

/// What a block without a table of its own drops: its item, once, when harvested.
pub fn harvested(item: &str) -> abi::LootPool {
    let entry = abi::LootEntry { item: item.to_string(), weight: 1, min: 1, max: 1, ..Default::default() };
    let cond = abi::LootCond { kind: lc::HARVESTED, value: 1.0, text: String::new() };
    abi::LootPool { rolls_min: 1, rolls_max: 1, conditions: vec![cond], entries: vec![entry] }
}

pub struct Resolve<'a> {
    pub items: &'a FxHashMap<String, u32>,
    pub components: &'a Components,
}

impl Resolve<'_> {
    fn item(&self, s: &str, errors: &mut Vec<String>, table: &str) -> u32 {
        if s.is_empty() {
            return 0;
        }
        crate::refs::one(s, self.items, "item").unwrap_or_else(|e| {
            errors.push(format!("loot table {table}: {e}"));
            0
        })
    }

    /// A condition naming a tag nobody declares could never pass: an error, not a silent no.
    fn conds(&self, c: &[abi::LootCond], errors: &mut Vec<String>, table: &str) -> Vec<Cond> {
        c.iter()
            .map(|c| {
                let named = c.kind == lc::BLOCK_STATE || c.kind == lc::CONDITION;
                let tag = if c.text.is_empty() || named {
                    u32::MAX
                } else {
                    crate::refs::tag(self.components, &c.text).unwrap_or_else(|e| {
                        errors.push(format!("loot table {table}: condition: {e}"));
                        u32::MAX
                    })
                };
                let prop = if named { c.text.clone() } else { String::new() };
                Cond { kind: c.kind, value: c.value, tag, prop }
            })
            .collect()
    }

    fn pool(&self, p: &abi::LootPool, errors: &mut Vec<String>, table: &str) -> Pool {
        let conds = self.conds(&p.conditions, errors, table);
        let entries = p
            .entries
            .iter()
            .map(|e| Entry {
                item: self.item(&e.item, errors, table),
                weight: e.weight,
                min: e.min,
                max: e.max.max(e.min),
                conds: self.conds(&e.conditions, errors, table),
                smelt: (!e.smelt.is_empty()).then(|| e.smelt.clone()),
            })
            .collect();
        Pool { rolls: (p.rolls_min, p.rolls_max.max(p.rolls_min)), conds, entries }
    }

    /// A table from its entry: the pools, then the swaps in order.
    pub fn table(&self, e: &crate::entries::Entry, errors: &mut Vec<String>) -> Table {
        use abi::comp::loot as l;
        let mut pools: Vec<Pool> = e
            .or::<Vec<abi::LootPool>>(l::POOLS, Vec::new())
            .iter()
            .map(|p| self.pool(p, errors, &e.name))
            .collect();
        for s in e.or::<Vec<abi::LootSwap>>(l::SWAPS, Vec::new()) {
            let from = self.item(&s.name, errors, &e.name);
            let to = (!s.to.is_empty()).then(|| self.item(&s.to, errors, &e.name));
            for p in &mut pools {
                match to {
                    Some(to) => p
                        .entries
                        .iter_mut()
                        .filter(|x| x.item == from)
                        .for_each(|x| x.item = to),
                    None => p.entries.retain(|x| x.item != from),
                }
            }
        }
        Table { pools }
    }
}

// ---------------------------------------------------------------- rolling

fn passes(c: &crate::content::Content, conds: &[Cond], ctx: &Ctx, rng: &mut dyn FnMut() -> f32) -> bool {
    conds.iter().all(|cd| match cd.kind {
        lc::CHANCE => rng() < cd.value,
        lc::TOOL_TAG => !ctx.tool.is_empty() && c.stack_has(&ctx.tool, Some(cd.tag)),
        lc::NOT_TOOL_TAG => ctx.tool.is_empty() || !c.stack_has(&ctx.tool, Some(cd.tag)),
        lc::TOOL_LEVEL => {
            !ctx.tool.is_empty()
                && c.stack_value::<u32>(&ctx.tool, c.ids.tool_level)
                    .unwrap_or(0) as f32
                    >= cd.value
        }
        lc::KILLED_BY_PLAYER => ctx.by_player,
        lc::ON_FIRE => ctx.on_fire,
        lc::HARVESTED => ctx.mined == 0 || c.harvests(ctx.mined, &ctx.tool),
        lc::CONDITION => ctx.holds.iter().any(|h| *h == cd.prop),
        // Not per tick or per entity: only when a table is rolled for a block.
        lc::BLOCK_STATE => {
            let name = c.state_name(ctx.state);
            name.split_once('[')
                .is_some_and(|(_, props)| props.trim_end_matches(']').split(',').any(|p| p == cd.prop))
        }
        _ => false,
    })
}

/// Rolls a table; `rng` gives uniform numbers in 0..1 from the region's stream, so results are
/// deterministic.
pub fn roll(c: &crate::content::Content, t: &Table, ctx: &Ctx, rng: &mut dyn FnMut() -> f32) -> Vec<ItemStack> {
    let mut out: Vec<ItemStack> = Vec::new();
    for p in &t.pools {
        if !passes(c, &p.conds, ctx, rng) {
            continue;
        }
        let rolls = p.rolls.0 + ((rng() * (p.rolls.1 - p.rolls.0 + 1) as f32) as u32).min(p.rolls.1 - p.rolls.0);
        for _ in 0..rolls {
            let ok: Vec<&Entry> = p
                .entries
                .iter()
                .filter(|e| passes(c, &e.conds, ctx, rng))
                .collect();
            let total: u32 = ok.iter().map(|e| e.weight).sum();
            if total == 0 {
                continue;
            }
            let mut pick = (rng() * total as f32) as u32;
            let Some(e) = ok.iter().find(|e| {
                if pick < e.weight {
                    true
                } else {
                    pick -= e.weight;
                    false
                }
            }) else {
                continue;
            };
            let n = e.min + ((rng() * (e.max - e.min + 1) as f32) as u32).min(e.max - e.min);
            if e.item == 0 || n == 0 {
                continue;
            }
            let mut item = e.item;
            if let (true, Some(kind)) = (ctx.on_fire, &e.smelt) {
                if let Some(r) = c.game.process(kind, item, &c.components) {
                    let made = r.result();
                    if !made.is_empty() {
                        item = made.item;
                    }
                }
            }
            match out
                .iter_mut()
                .find(|s| s.item == item && s.count + n <= c.max_stack(item))
            {
                Some(s) => s.count += n,
                None => out.push(ItemStack::new(item, n)),
            }
        }
    }
    out
}
