//! restore sets a hero to a snapshot of another hero's state for a fresh
//! start: where it stood, its health, level, souls, build and cooldowns.
//!
//! ```ignore
//! let snapshot = HeroSnapshot {
//!     health: 520,
//!     max_health: 760,
//!     souls: Some(4200),
//!     ..HeroSnapshot::at(position, facing)
//! };
//! restore_hero(player, snapshot, |duel: &mut Duel, error| duel.restored(error));
//! ```

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

use crate::generated::{Angles, EquipmentTarget, HeroTarget, RestoredEvent, TimerTarget, Vector};
use crate::runtime;
use crate::Player;

/// HeroSnapshot is the state restore_hero gives a hero. A part left None
/// keeps the hero's own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeroSnapshot {
    /// position is the hero's origin.
    pub position: Vector,
    /// facing is where the hero and its camera face.
    pub facing: Angles,
    /// health and max_health are the hero's health and its base maximum.
    pub health: i32,
    pub max_health: i32,
    /// level is the hero's level. The restore also sets the hero's souls to
    /// what the level needs, since the game raises a level its souls pass
    /// and never lowers one: a richer hero loses souls, from its wallet
    /// first, and a poorer one gains them.
    pub level: Option<i32>,
    /// souls is the souls the hero holds unspent, once the build is in
    /// place. The build a snapshot lists is not paid for from them. They are
    /// set after the level, so souls past the level's band raise the level
    /// again.
    pub souls: Option<i32>,
    /// upgrade_bonuses are the three stat bonuses purchases add.
    pub upgrade_bonuses: Option<Vec<f32>>,
    /// abilities are the hero's abilities in their slots, with their bought
    /// tiers.
    pub abilities: Option<Vec<EquipmentTarget>>,
    /// items replace every item the hero owns; an empty list removes them.
    pub items: Option<Vec<EquipmentTarget>>,
    /// timers are the charges and cooldowns of owned abilities and items.
    pub timers: Option<Vec<TimerTarget>>,
}

impl HeroSnapshot {
    /// at returns the snapshot of a hero that stands at position and faces
    /// facing, with every other part left for the mod to fill.
    pub fn at(position: Vector, facing: Angles) -> Self {
        Self {
            position,
            facing,
            ..Self::default()
        }
    }
}

/// Done runs on the mod when a restore ends, with why it failed.
type Done = Box<dyn FnOnce(&mut dyn Any, Option<&str>)>;

/// Waiting is one slot's running restore.
struct Waiting {
    souls: Option<i32>,
    done: Done,
}

thread_local! {
    /// WAITING holds each slot's running restore.
    static WAITING: RefCell<HashMap<i32, Waiting>> = RefCell::new(HashMap::new());
}

/// restore_hero gives player's live hero the snapshot over the next few
/// seconds, and runs done on the mod with None once the state has held, or
/// with the reason it did not. A new restore for the player replaces the
/// running one, which never runs done. It returns false when the restore
/// could not start. Shields are not restored.
pub fn restore_hero<M: 'static>(
    player: Player,
    snapshot: HeroSnapshot,
    done: impl FnOnce(&mut M, Option<&str>) + 'static,
) -> bool {
    // The hero spawns its souls with the build, so they are set first and
    // the unspent balance is corrected when the restore ends.
    let target = HeroTarget {
        position: snapshot.position,
        facing: snapshot.facing,
        health: Some(snapshot.health),
        max_health: Some(snapshot.max_health),
        level: snapshot.level,
        upgrade_bonuses: snapshot.upgrade_bonuses.unwrap_or_default(),
        abilities: snapshot.abilities.unwrap_or_default(),
        replace_items: Some(snapshot.items.is_some()),
        items: snapshot.items.unwrap_or_default(),
        timers: snapshot.timers.unwrap_or_default(),
        ..HeroTarget::default()
    };
    if let Some(souls) = snapshot.souls {
        player.starting_souls(souls);
    }
    let done: Done = Box::new(move |m, error| {
        if let Some(m) = m.downcast_mut::<M>() {
            done(m, error);
        }
    });
    let souls = snapshot.souls;
    runtime::hook(|hooks| hooks.restored = Some(settle));
    WAITING.with(|waiting| {
        let waiting = &mut waiting.borrow_mut();
        waiting.insert(player.slot(), Waiting { souls, done })
    });
    if player.restore_hero(&target) {
        return true;
    }
    WAITING.with(|waiting| waiting.borrow_mut().remove(&player.slot()));
    false
}

/// settle finishes a player's restore when the game reports its end: it
/// sets the souls the snapshot holds and runs the restore's function.
fn settle(m: &mut dyn Any, event: &RestoredEvent) {
    // Only a restore this module started has souls to settle.
    let player = event.player;
    let restore = WAITING.with(|waiting| waiting.borrow_mut().remove(&player.slot()));
    let Some(restore) = restore else {
        return;
    };

    // Pay or refund the difference between the souls held and the
    // snapshot's.
    let held = player.pawn().map(|pawn| pawn.souls);
    if let (Some(souls), Some(held)) = (restore.souls, held) {
        if held != souls {
            player.adjust_souls(souls - held, Some(true));
        }
    }
    let error = Some(event.error.as_str()).filter(|error| !error.is_empty());
    (restore.done)(m, error);
}
