//! Registration as entries: every kind is a named entry made of components (`abi::EntryDesc`).
//! Mods' entries of a kind are folded in load order (new, edits, replacements, removals) into
//! one `Entry` per name, which the kind's freeze then bakes into what the game reads.

use rustc_hash::FxHashMap;
use weft_abi::{self as abi, ModId, data, entry_mode as em, value_op as vo};

use crate::components::RawValue;

#[derive(Clone, Debug)]
pub struct RawOp {
    pub op: u32,
    pub value: RawValue,
    pub anchor: String,
}

/// One entry as a mod registered it.
#[derive(Clone, Debug)]
pub struct RawEntry {
    pub owner: ModId,
    pub kind: u32,
    pub name: String,
    pub cell: usize,
    pub mode: u32,
    pub ops: Vec<RawOp>,
    pub parts: Vec<(String, Vec<RawOp>)>,
}

impl RawEntry {
    /// # Safety
    /// The descriptor's pointers must be valid for this call.
    pub unsafe fn copy(owner: ModId, d: &abi::EntryDesc) -> RawEntry {
        unsafe {
            let ops = |s: abi::Slice<abi::ValueOp>| -> Vec<RawOp> {
                s.as_slice()
                    .iter()
                    .map(|o| RawOp { op: o.op, value: RawValue::copy(&o.value), anchor: o.anchor.as_str().to_string() })
                    .collect()
            };
            RawEntry {
                owner,
                kind: d.kind,
                name: d.name.as_str().to_string(),
                cell: d.cell as usize,
                mode: d.mode,
                ops: ops(d.ops),
                parts: d
                    .parts
                    .as_slice()
                    .iter()
                    .map(|p| (p.key.as_str().to_string(), ops(p.ops)))
                    .collect(),
            }
        }
    }
}

/// One entry of a kind after every mod had its say.
#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub name: String,
    /// The mod whose registration stands (the first, or the last replacement).
    pub owner: ModId,
    /// Handle cells to fill with its id, every mod's.
    pub cells: Vec<usize>,
    pub values: Vec<RawValue>,
    /// Blocks: operations on some states, by key, in the order they came.
    pub parts: Vec<(String, Vec<RawOp>)>,
}

impl Entry {
    pub fn value(&self, name: &str) -> Option<&RawValue> {
        self.values.iter().find(|v| v.name == name)
    }

    pub fn has(&self, name: &str) -> bool {
        self.value(name).is_some()
    }

    /// The value of `name` as `T`, if it is there with `T`'s schema.
    pub fn get<T: data::Data>(&self, name: &str) -> Option<T> {
        self.value(name)
            .filter(|v| v.schema == T::schema())
            .and_then(|v| data::from_bytes(&v.bytes))
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        self.value(name).and_then(|v| v.text.as_deref())
    }

    pub fn or<T: data::Data>(&self, name: &str, default: T) -> T {
        self.get(name).unwrap_or(default)
    }

    pub fn callback(&self, name: &str) -> Option<abi::Callback> {
        self.get::<abi::Callback>(name).filter(|c| c.is_some())
    }

    /// Applies one operation; `errors` gets what made no sense.
    pub fn apply(&mut self, op: &RawOp, errors: &mut Vec<String>) {
        apply(&mut self.values, op, &self.name, errors);
    }
}

/// Applies `op` to a list of values.
pub fn apply(values: &mut Vec<RawValue>, op: &RawOp, what: &str, errors: &mut Vec<String>) {
    let at = values.iter().position(|v| v.name == op.value.name);
    match op.op {
        vo::SET => match at {
            Some(i) => values[i] = op.value.clone(),
            None => values.push(op.value.clone()),
        },
        vo::UNSET => {
            if let Some(i) = at {
                values.remove(i);
            }
        }
        _ => {
            let name = &op.value.name;
            let list_schema = format!("[{}]", op.value.schema);
            let i = match at {
                Some(i) => i,
                None => {
                    values.push(RawValue { name: name.clone(), schema: list_schema.clone(), bytes: data::list(&[]), text: None });
                    values.len() - 1
                }
            };
            let v = &mut values[i];
            if v.schema != list_schema {
                errors.push(format!("{what}: {name} is {}, not a list of {}", v.schema, op.value.schema));
                return;
            }
            let Some(mut items) = data::items(&v.schema, &v.bytes) else {
                errors.push(format!("{what}: {name}: bad list"));
                return;
            };
            let elem = op.value.schema.as_str();
            let found = || -> Option<usize> {
                match op
                    .anchor
                    .strip_prefix('#')
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    Some(n) => (n < items.len()).then_some(n),
                    None => items
                        .iter()
                        .position(|it| data::item_name(elem, it) == Some(op.anchor.as_str())),
                }
            };
            let new = op.value.bytes.as_slice();
            match op.op {
                vo::PUSH => items.push(new),
                vo::INSERT_BEFORE | vo::INSERT_AFTER | vo::REMOVE_ITEM | vo::REPLACE_ITEM => {
                    let Some(n) = found() else {
                        errors.push(format!("{what}: {name} has no element {}", op.anchor));
                        return;
                    };
                    match op.op {
                        vo::INSERT_BEFORE => items.insert(n, new),
                        vo::INSERT_AFTER => items.insert(n + 1, new),
                        vo::REMOVE_ITEM => drop(items.remove(n)),
                        _ => items[n] = new,
                    }
                }
                n => errors.push(format!("{what}: {name}: unknown operation {n}")),
            }
            let bytes = data::list(&items);
            values[i].bytes = bytes;
        }
    }
}

/// Whether an EDIT's name is a pattern, not one entry.
pub fn is_pattern(name: &str) -> bool {
    name.starts_with('#') || name.contains('*') || name.contains('[')
}

/// Folds one kind's entries, in load order, into one per name (sorted by name). `kind` names it
/// in errors. Edits naming a pattern are left in `patterns` for the kind to apply, as only it
/// knows its tags and states.
pub fn fold(kind: &str, raw: Vec<RawEntry>, mod_name: &dyn Fn(ModId) -> String, errors: &mut Vec<String>) -> Folded {
    let mut by: FxHashMap<String, Entry> = FxHashMap::default();
    let mut late: Vec<RawEntry> = Vec::new();
    let mut patterns: Vec<RawEntry> = Vec::new();
    let mut gone_cells: Vec<usize> = Vec::new();
    for e in raw {
        match e.mode {
            em::NEW | em::REPLACE => {
                if e.mode == em::NEW {
                    if let Some(old) = by.get(&e.name) {
                        errors.push(format!("{kind} {} registered by both {} and {}", e.name, mod_name(old.owner), mod_name(e.owner)));
                        continue;
                    }
                }
                let mut cells = by.remove(&e.name).map_or(Vec::new(), |o| o.cells);
                cells.push(e.cell);
                let mut entry = Entry { name: e.name.clone(), owner: e.owner, cells, values: Vec::new(), parts: e.parts };
                for op in &e.ops {
                    entry.apply(op, errors);
                }
                by.insert(e.name, entry);
            }
            em::REMOVE => {
                if let Some(old) = by.remove(&e.name) {
                    gone_cells.extend(old.cells);
                }
            }
            em::EDIT if is_pattern(&e.name) => patterns.push(e),
            em::EDIT => match by.get_mut(&e.name) {
                Some(entry) => edit(entry, e, errors),
                // Its mod may come later in the load order.
                None => late.push(e),
            },
            m => errors.push(format!("{kind} {}: unknown mode {m}", e.name)),
        }
    }
    for e in late {
        match by.get_mut(&e.name) {
            Some(entry) => edit(entry, e, errors),
            None => errors.push(format!("{}: edit of {kind} {}: no such {kind}", mod_name(e.owner), e.name)),
        }
    }
    let mut entries: Vec<Entry> = by.into_values().collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Folded { entries, patterns, gone_cells }
}

fn edit(entry: &mut Entry, e: RawEntry, errors: &mut Vec<String>) {
    entry.cells.push(e.cell);
    for op in &e.ops {
        entry.apply(op, errors);
    }
    entry.parts.extend(e.parts);
}

pub struct Folded {
    pub entries: Vec<Entry>,
    /// Edits naming a pattern, in load order.
    pub patterns: Vec<RawEntry>,
    /// Cells of removed entries: their handles stay unknown.
    pub gone_cells: Vec<usize>,
}

impl Folded {
    /// Applies the pattern edits matching by name (`*`) or by a tag value (`#tag`): what fits
    /// every kind. Blocks handle `[prop=value]` themselves.
    pub fn apply_patterns(&mut self, errors: &mut Vec<String>) {
        for p in std::mem::take(&mut self.patterns) {
            let tag = p.name.strip_prefix('#');
            for e in &mut self.entries {
                let hit = match tag {
                    Some(t) => e
                        .values
                        .iter()
                        .any(|v| v.schema.is_empty() && crate::components::matches(t, &v.name)),
                    None => crate::components::matches(&p.name, &e.name),
                };
                if hit {
                    for op in &p.ops {
                        e.apply(op, errors);
                    }
                }
            }
        }
    }

    /// Writes every entry's id (its place, by name) into its handles' cells.
    pub fn write_cells(&self) {
        for (i, e) in self.entries.iter().enumerate() {
            for &c in &e.cells {
                crate::registry::write_cell(c, i as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(name: &str, v: u32) -> RawOp {
        RawOp {
            op: vo::SET,
            value: RawValue { name: name.into(), schema: "u32".into(), bytes: v.to_le_bytes().to_vec(), text: None },
            anchor: String::new(),
        }
    }

    fn entry(owner: u32, name: &str, mode: u32, ops: Vec<RawOp>) -> RawEntry {
        RawEntry { owner: ModId(owner), kind: 0, name: name.into(), cell: 0, mode, ops, parts: Vec::new() }
    }

    #[test]
    fn modes_in_load_order() {
        let mut errors = Vec::new();
        let raw = vec![
            entry(2, "a:x", em::EDIT, vec![set("speed", 9)]),
            entry(1, "a:x", em::NEW, vec![set("speed", 1), set("size", 2)]),
            entry(3, "a:y", em::NEW, vec![set("speed", 1)]),
            entry(4, "a:y", em::REMOVE, vec![]),
            entry(5, "a:z", em::NEW, vec![set("speed", 1)]),
            entry(6, "a:z", em::REPLACE, vec![set("size", 7)]),
            entry(7, "a:z", em::NEW, vec![]),
        ];
        let f = fold("thing", raw, &|m| format!("mod{}", m.0), &mut errors);
        let names: Vec<&str> = f.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a:x", "a:z"]);
        assert_eq!(f.entries[0].get::<u32>("speed"), Some(9), "an edit before its entry still applies");
        assert_eq!(f.entries[0].get::<u32>("size"), Some(2));
        assert_eq!((f.entries[1].get::<u32>("speed"), f.entries[1].get::<u32>("size")), (None, Some(7)));
        assert_eq!(errors.len(), 1, "{errors:?}");
    }

    #[test]
    fn list_operations() {
        let item = |name: &str, n: u32| {
            let mut b = Vec::new();
            data::write_text(name, &mut b);
            b.extend(n.to_le_bytes());
            RawValue { name: "rules".into(), schema: "<name:str,n:u32>".into(), bytes: b, text: None }
        };
        let op = |op: u32, v: RawValue, anchor: &str| RawOp { op, value: v, anchor: anchor.into() };
        let mut e = Entry::default();
        let mut errors = Vec::new();
        e.apply(&op(vo::PUSH, item("a", 1), ""), &mut errors);
        e.apply(&op(vo::PUSH, item("c", 3), ""), &mut errors);
        e.apply(&op(vo::INSERT_BEFORE, item("b", 2), "c"), &mut errors);
        e.apply(&op(vo::REMOVE_ITEM, item("", 0), "a"), &mut errors);
        e.apply(&op(vo::REPLACE_ITEM, item("c", 30), "#1"), &mut errors);
        assert!(errors.is_empty(), "{errors:?}");
        let v = e.value("rules").unwrap();
        let names: Vec<_> = data::items(&v.schema, &v.bytes)
            .unwrap()
            .into_iter()
            .map(|it| {
                (data::item_name("<name:str,n:u32>", it).unwrap().to_string(), u32::from_le_bytes(it[it.len() - 4..].try_into().unwrap()))
            })
            .collect();
        assert_eq!(names, [("b".to_string(), 2), ("c".to_string(), 30)]);
    }
}
