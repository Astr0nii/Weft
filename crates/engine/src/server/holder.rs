//! What any `abi::Holder` holds, reached one way: an entity's inventory and components, a
//! block's through its block entity, a stack's through its `contents` and stack components,
//! written back into the slot the stack sits in.

use weft_abi::{self as abi, ItemStack};

use super::dim::Dim;
use crate::content::Content;
use crate::stack;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum At {
    Entity(u64),
    /// The stack in `slot` of `entity`'s inventory.
    Stack {
        entity: u64,
        slot: u32,
    },
}

pub fn resolve(d: &Dim, h: &abi::Holder) -> Option<At> {
    match h.kind {
        abi::holder::ENTITY => d
            .entities
            .get(h.entity)
            .filter(|e| !e.removed)
            .map(|_| At::Entity(h.entity)),
        abi::holder::BLOCK => d.anchored(crate::world::ivec(h.pos)).map(At::Entity),
        abi::holder::HELD => d
            .entities
            .inventories
            .get(&h.entity)
            .filter(|i| (h.slot as usize) < i.len())
            .map(|_| At::Stack { entity: h.entity, slot: h.slot }),
        _ => None,
    }
}

/// Slots each stack of `item` carries (its `inventory` component), 0 for most items.
pub fn item_slots(c: &Content, item: u32) -> u32 {
    c.item_value::<u32>(item, c.ids.inventory)
        .unwrap_or(0)
        .min(1 << 12)
}

fn read_u32(b: &[u8], i: usize) -> u32 {
    b.get(i..i + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// What `stack` holds, one entry per slot of its item.
pub fn contents(c: &Content, stack: &ItemStack) -> Vec<ItemStack> {
    let n = item_slots(c, stack.item) as usize;
    let bytes = c
        .ids
        .contents
        .and_then(|comp| stack::value(stack.data, comp))
        .unwrap_or(&[]);
    (0..n)
        .map(|i| {
            let s = ItemStack { item: read_u32(bytes, i * 12), count: read_u32(bytes, i * 12 + 4), data: read_u32(bytes, i * 12 + 8) };
            if s.is_empty() || s.item as usize >= c.items.len() { ItemStack::EMPTY } else { s }
        })
        .collect()
}

pub fn with_contents(c: &Content, stack: ItemStack, list: &[ItemStack]) -> ItemStack {
    let Some(comp) = c.ids.contents else { return stack };
    let empty = list.iter().all(ItemStack::is_empty);
    let bytes: Vec<u8> = list
        .iter()
        .flat_map(|s| {
            let s = if s.is_empty() { ItemStack::EMPTY } else { *s };
            [s.item, s.count, s.data]
        })
        .flat_map(u32::to_le_bytes)
        .collect();
    ItemStack { data: stack::with(stack.data, comp, (!empty).then_some(&bytes[..])), ..stack }
}

fn stack_at(d: &Dim, entity: u64, slot: u32) -> ItemStack {
    d.entities
        .inventories
        .get(&entity)
        .and_then(|i| i.get(slot as usize))
        .copied()
        .unwrap_or_default()
}

pub fn len(d: &Dim, c: &Content, at: At) -> u32 {
    match at {
        At::Entity(id) => d
            .entities
            .inventories
            .get(&id)
            .map_or(0, |i| i.len() as u32),
        At::Stack { entity, slot } => item_slots(c, stack_at(d, entity, slot).item),
    }
}

pub fn get(d: &Dim, c: &Content, at: At, i: u32) -> ItemStack {
    match at {
        At::Entity(id) => stack_at(d, id, i),
        At::Stack { entity, slot } => contents(c, &stack_at(d, entity, slot))
            .get(i as usize)
            .copied()
            .unwrap_or_default(),
    }
}

pub fn set(d: &mut Dim, c: &Content, at: At, i: u32, s: ItemStack) {
    match at {
        At::Entity(id) => d.entities.set_slot(id, i, s),
        At::Stack { entity, slot } => {
            let holder = stack_at(d, entity, slot);
            let mut list = contents(c, &holder);
            if let Some(e) = list.get_mut(i as usize) {
                *e = s;
                d.entities
                    .set_slot(entity, slot, with_contents(c, holder, &list));
            }
        }
    }
}

/// Merges into matching stacks, then empty slots. Returns the rest.
pub fn insert(d: &mut Dim, c: &Content, at: At, stack: ItemStack) -> ItemStack {
    match at {
        At::Entity(id) => d.entities.insert_item(c, id, stack),
        At::Stack { entity, slot } => {
            let holder = stack_at(d, entity, slot);
            let mut list = contents(c, &holder);
            let rest = merge_into(c, &mut list, stack, |_| true);
            if rest != stack {
                d.entities
                    .set_slot(entity, slot, with_contents(c, holder, &list));
            }
            rest
        }
    }
}

/// `stack` merged into `list` where `allowed` says, matching stacks first. Returns the rest.
pub fn merge_into(c: &Content, list: &mut [ItemStack], mut stack: ItemStack, allowed: impl Fn(usize) -> bool) -> ItemStack {
    let max = c.max_stack(stack.item);
    for (_, s) in list.iter_mut().enumerate().filter(|(i, _)| allowed(*i)) {
        if stack.count == 0 {
            break;
        }
        if s.stacks_with(&stack) && s.count < max && !s.is_empty() {
            let n = (max - s.count).min(stack.count);
            s.count += n;
            stack.count -= n;
        }
    }
    for (_, s) in list.iter_mut().enumerate().filter(|(i, _)| allowed(*i)) {
        if stack.count == 0 {
            break;
        }
        if s.is_empty() {
            let n = max.min(stack.count);
            *s = ItemStack { count: n, ..stack };
            stack.count -= n;
        }
    }
    if stack.count == 0 { ItemStack::EMPTY } else { stack }
}

/// The holder's `comp`, as bytes.
pub fn component(d: &Dim, at: At, comp: u32) -> Option<Box<[u8]>> {
    match at {
        At::Entity(id) => d.entities.comps.get(&(id, comp)).cloned(),
        At::Stack { entity, slot } => stack::value(stack_at(d, entity, slot).data, comp).map(Into::into),
    }
}

/// Sets the holder's `comp` (`None` takes it off a stack). False if it can't: an entity
/// without it, or bytes of another size than its kind's.
pub fn set_component(d: &mut Dim, c: &Content, at: At, comp: u32, v: Option<&[u8]>) -> bool {
    match at {
        At::Entity(id) => {
            let size = c
                .game
                .components
                .get(comp as usize)
                .map_or(0, |def| def.size as usize);
            match (d.entities.comps.get_mut(&(id, comp)), v) {
                (Some(old), Some(v)) if size == 0 || v.len() == size => {
                    *old = v.into();
                    true
                }
                _ => false,
            }
        }
        At::Stack { entity, slot } => {
            let s = stack_at(d, entity, slot);
            if s.is_empty() {
                return false;
            }
            d.entities
                .set_slot(entity, slot, ItemStack { data: stack::with(s.data, comp, v), ..s });
            true
        }
    }
}

/// What of `amount` (in positive, out negative) a store of `stored` out of `capacity` takes or
/// gives within its rate limits, with the same sign.
fn bounded(amount: i64, stored: i64, capacity: i64, max_in: i64, max_out: i64) -> i64 {
    if amount >= 0 {
        amount.min(max_in.max(0)).min((capacity - stored).max(0))
    } else {
        -(amount.saturating_neg())
            .min(max_out.max(0))
            .min(stored.max(0))
    }
}

fn read<T: Copy>(d: &Dim, at: At, comp: u32) -> Option<T> {
    let b = component(d, at, comp).filter(|b| b.len() >= size_of::<T>())?;
    Some(unsafe { std::ptr::read_unaligned(b.as_ptr() as *const T) })
}

fn write<T: Copy>(d: &mut Dim, c: &Content, at: At, comp: u32, v: &T) {
    let bytes = unsafe { std::slice::from_raw_parts(v as *const T as *const u8, size_of::<T>()) };
    set_component(d, c, at, comp, Some(bytes));
}

/// Moves `amount` in (positive) or out (negative) within the holder's limits; returns what
/// moved, with the same sign.
pub fn move_energy(d: &mut Dim, c: &Content, at: At, amount: i64, simulate: bool) -> i64 {
    let Some(comp) = c.ids.energy else { return 0 };
    let Some(mut v) = read::<abi::Energy>(d, at, comp) else { return 0 };
    let n = bounded(amount, v.stored, v.capacity, v.max_in, v.max_out);
    if !simulate && n != 0 {
        v.stored += n;
        write(d, c, at, comp, &v);
    }
    n
}
