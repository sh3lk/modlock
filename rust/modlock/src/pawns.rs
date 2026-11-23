//! pawns answers the per-event pawn read behind `Player::pawn`. A mod whose
//! systems each read every player's pawn on each frame would otherwise pay a
//! Pawn call and a full decode for every read, though the game cannot change
//! between two reads in one event unless the mod itself changed it. This
//! module answers those reads from what the event already read:
//!
//! - The host's revision moves with each event and with each call the schema
//!   does not mark NO_SIDE_EFFECTS. A pawn read at the current revision is
//!   answered again without a call.
//! - The first slot read at a revision takes its own Pawn call. A second slot
//!   read at the same revision reads every slot with one Pawns call, which
//!   answers the rest of the revision's reads.
//! - Each slot keeps its last decoded Pawn and the bytes it came from. A
//!   slot whose bytes have not changed gets the same Pawn back without a
//!   decode, as a hero standing still does frame after frame.
//!
//! A Pawn the cache answers with is a value of its own, but a mod treats a
//! Pawn as read-only. Outside an event, and while `set_pawn_cache(false)`
//! holds, every read is a Pawn call and a decode.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::generated::Pawn;
use crate::host;
use crate::wire::{self, Message, Reader};

/// Slot is what one slot's pawn read last found.
struct Slot {
    /// revision is the revision at which the slot was last read.
    revision: u64,
    /// bytes is the slot's encoded Pawn, or None when it had none.
    bytes: Option<Vec<u8>>,
    /// pawn is bytes decoded.
    pawn: Option<Pawn>,
}

/// Cache is the per-event reads.
struct Cache {
    /// on is `set_pawn_cache`'s switch.
    on: bool,
    /// slots holds each slot's last read, by slot.
    slots: HashMap<i32, Slot>,
    /// single_revision is the revision of the last Pawn call for one slot.
    single_revision: Option<u64>,
    /// batch_revision is the revision `batch` was read at.
    batch_revision: Option<u64>,
    /// batch holds each slot's encoded Pawn from the last Pawns call.
    batch: HashMap<i32, Vec<u8>>,
    /// batch_failed is the event in which the host last answered Pawns with
    /// an error. The rest of that event reads each slot alone, and the next
    /// event tries Pawns again.
    batch_failed: Option<u64>,
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            on: true,
            slots: HashMap::new(),
            single_revision: None,
            batch_revision: None,
            batch: HashMap::new(),
            batch_failed: None,
        }
    }
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// set_pawn_cache turns the per-event pawn read on, as it starts, or off,
/// when every `Player::pawn` makes its own Pawn call and decode, so a mod can
/// compare the two. Either way the next read starts afresh.
pub fn set_pawn_cache(on: bool) {
    CACHE.with(|cache| {
        *cache.borrow_mut() = Cache {
            on,
            ..Cache::default()
        }
    });
}

/// pawn_of answers `Player::pawn` for slot.
pub(crate) fn pawn_of(slot: i32) -> Option<Pawn> {
    // Outside an event, or with the cache off, every read reaches the host.
    let now = host::revision();
    let Some(now) = now.filter(|_| CACHE.with(|cache| cache.borrow().on)) else {
        return read_one(slot).and_then(|bytes| Pawn::decode(&bytes).ok());
    };

    // A slot read at this revision is answered again.
    if let Some(kept) = CACHE.with(|cache| {
        cache
            .borrow()
            .slots
            .get(&slot)
            .filter(|kept| kept.revision == now)
            .map(|kept| kept.pawn.clone())
    }) {
        return kept;
    }

    // Read the slot alone first, and every slot once a second is read.
    let read_batch_now = CACHE.with(|cache| {
        let cache = cache.borrow();
        cache.batch_revision != Some(now)
            && cache.single_revision == Some(now)
            && cache.batch_failed != Some(host::delivered())
    });
    if read_batch_now {
        read_batch(now);
    }
    let batch = CACHE.with(|cache| {
        let cache = cache.borrow();
        (cache.batch_revision == Some(now))
            .then(|| cache.batch.get(&slot).cloned())
            .flatten()
    });
    let in_batch = CACHE.with(|cache| cache.borrow().batch_revision == Some(now));
    let bytes = if in_batch {
        batch
    } else {
        CACHE.with(|cache| cache.borrow_mut().single_revision = Some(now));
        read_one(slot)
    };

    // Bytes the slot's last decode came from give back the same Pawn.
    let kept = CACHE.with(|cache| {
        cache.borrow().slots.get(&slot).map(|kept| {
            (
                kept.bytes.clone(),
                same_bytes(kept.bytes.as_deref(), bytes.as_deref()),
                kept.pawn.clone(),
            )
        })
    });
    let (bytes, pawn) = match kept {
        Some((kept_bytes, true, kept_pawn)) => (kept_bytes, kept_pawn),
        _ => {
            let pawn = bytes.as_deref().and_then(|bytes| Pawn::decode(bytes).ok());
            (bytes, pawn)
        }
    };
    CACHE.with(|cache| {
        cache.borrow_mut().slots.insert(
            slot,
            Slot {
                revision: now,
                bytes: bytes.clone(),
                pawn: pawn.clone(),
            },
        );
    });
    pawn
}

/// read_one returns slot's encoded Pawn from its own Pawn call, or None when
/// the slot has none, the call failed or the reply is unreadable.
fn read_one(slot: i32) -> Option<Vec<u8>> {
    host::read(
        "Pawn",
        |out| {
            if slot != 0 {
                wire::int32(out, 1, slot);
            }
        },
        |r| {
            let mut result: Option<Vec<u8>> = None;
            while let Some((number, kind)) = r.field()? {
                match number {
                    1 => result = Some(r.bytes(kind)?.to_vec()),
                    _ => r.skip(kind)?,
                }
            }
            Ok(result)
        },
    )
    .flatten()
}

/// read_batch fills the cache from one Pawns call at revision now. A host
/// that answers with an error, such as one older than Pawns, leaves the rest
/// of the event's reads to their own calls; a reply the walk cannot read
/// leaves this revision's.
fn read_batch(now: u64) {
    let found = host::read(
        "Pawns",
        |_| {},
        |r| {
            let mut entries = HashMap::new();
            while let Some((number, kind)) = r.field()? {
                if number != 1 {
                    r.skip(kind)?;
                    continue;
                }
                let entry = r.bytes(kind)?;
                let mut player = 0;
                let mut pawn = None;
                let mut entry = Reader::new(entry);
                while let Some((number, kind)) = entry.field()? {
                    match number {
                        1 => player = entry.int32(kind)?,
                        2 => pawn = Some(entry.bytes(kind)?.to_vec()),
                        _ => entry.skip(kind)?,
                    }
                }
                if let Some(pawn) = pawn {
                    entries.insert(player, pawn);
                }
            }
            Ok(entries)
        },
    );
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        match found {
            Some(entries) => {
                cache.batch = entries;
                cache.batch_revision = Some(now);
            }
            None => cache.batch_failed = Some(host::delivered()),
        }
    });
}

/// same_bytes reports whether a and b hold the same bytes, or are both absent.
fn same_bytes(a: Option<&[u8]>, b: Option<&[u8]>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a == b,
        (a, b) => a.is_none() && b.is_none(),
    }
}
