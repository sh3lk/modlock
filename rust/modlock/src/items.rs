//! items lists the shop's items, generated from the game's item data, and
//! finds one by the class name a mod gives give_item, such as
//! upgrade_ability_power_shard.

use std::collections::HashMap;
use std::sync::OnceLock;

#[rustfmt::skip]
#[path = "items.gen.rs"]
mod table;

pub use table::ITEMS;

/// Slot is the shop tab an item sits on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Weapon,
    Vitality,
    Spirit,
}

/// Item is one of the shop's items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Item {
    /// name is the item's class name, which give_item takes.
    pub name: &'static str,
    /// id is the game's id of the item.
    pub id: u32,
    /// slot is the shop tab the item sits on.
    pub slot: Slot,
    /// tier is the item's tier, 1 to 4.
    pub tier: u8,
    /// active is true for an item the player casts.
    pub active: bool,
    /// imbue is true for an item that strengthens an ability it is put on.
    pub imbue: bool,
    /// icon is the item's shop icon, as an image's src.
    pub icon: &'static str,
}

/// item returns the shop's item with class name, or None.
pub fn item(name: &str) -> Option<&'static Item> {
    static BY_NAME: OnceLock<HashMap<&'static str, &'static Item>> = OnceLock::new();
    BY_NAME
        .get_or_init(|| ITEMS.iter().map(|item| (item.name, item)).collect())
        .get(name)
        .copied()
}

/// item_id returns the game's id for an item or ability name, such as
/// upgrade_magic_shield: MurmurHash2 of the name's bytes, seeded with its
/// length, as the game hashes it.
pub fn item_id(name: &str) -> u32 {
    // Seed the hash with the name's length.
    const M: u32 = 0x5bd1_e995;
    let name = name.as_bytes();
    let mut h = 0x3141_5926 ^ name.len() as u32;

    // Mix in every whole four bytes.
    let mut words = name.chunks_exact(4);
    for word in &mut words {
        let mut k = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M) ^ k;
    }

    // Mix in the last one to three bytes, then finish.
    let rest = words.remainder();
    if !rest.is_empty() {
        for (at, byte) in rest.iter().enumerate().rev() {
            h ^= u32::from(*byte) << (8 * at);
        }
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ h >> 15
}
