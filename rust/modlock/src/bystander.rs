//! A bystander is a hero kept out of play while its player waits: in the
//! stands, on a bench, or lined up for a photo. It cannot move, shoot, cast
//! or zoom, and nothing in play can hurt it or target it with an ability, so
//! it can stand beside a fight without touching it. Its player still turns,
//! so a mod that pins the camera on the fight (play_camera with curve::hold
//! tracks for target and distance) gives them a view that circles it.
//!
//! spectate_from parks a hidden bystander above the map and lets its player
//! fly the free camera, so a player who is out of the round watches it and
//! keeps their hero, where Player::spectate would take it away.

use crate::generated::{hold_modifier_state, Angles, Vector};
use crate::Player;

/// BYSTANDER_STATES are the modifier states that keep a hero out of play.
pub const BYSTANDER_STATES: [&str; 11] = [
    "MODIFIER_STATE_IMMOBILIZED",
    "MODIFIER_STATE_DISARMED",
    "MODIFIER_STATE_MELEE_DISABLED",
    "MODIFIER_STATE_SILENCED",
    "MODIFIER_STATE_INVULNERABLE",
    "MODIFIER_STATE_TECH_UNTARGETABLE_BY_ENEMIES",
    "MODIFIER_STATE_TECH_UNTARGETABLE_BY_TEAMMATES",
    "MODIFIER_STATE_DISPLACEMENT_IMMUNE",
    "MODIFIER_STATE_IGNORE_BULLETS",
    "MODIFIER_STATE_IGNORE_MELEE",
    "MODIFIER_STATE_IN_ABILITY_DISABLE_ZOOM",
];

/// hold_bystander keeps the hero pawn entity out of play, or with active
/// false gives it back. The states stay until released or the pawn is
/// removed, so a mod releases them before the hero plays again. It returns
/// false when a state did not take, such as on a pawn that is gone.
pub fn hold_bystander(entity: u32, active: bool) -> bool {
    let mut held = true;
    for state in BYSTANDER_STATES {
        held = hold_modifier_state(entity, state, Some(active)) && held;
    }
    held
}

/// PARKED_STATES hide a parked hero on top of BYSTANDER_STATES: no model, no
/// health bar or name, no minimap icon, and no out of play area check, so
/// the game leaves a hero hanging above the map alone.
pub const PARKED_STATES: [&str; 4] = [
    "MODIFIER_STATE_DO_NOT_DRAW_MODEL",
    "MODIFIER_STATE_UNIT_STATUS_HIDDEN",
    "MODIFIER_STATE_INVISIBLE_ON_MINIMAP",
    "MODIFIER_STATE_IGNORE_OUT_OF_PLAY_AREA_CHECK",
];

/// PARK_HEIGHT is how far above where it stood spectate_from parks a hero,
/// above the roofs of a map and out of every hero's reach.
pub const PARK_HEIGHT: f64 = 4000.0;

/// Spectating is a player watching through the free camera while their hero
/// is parked out of sight. end returns them to it.
#[must_use = "end returns the player to their hero"]
pub struct Spectating {
    player: Player,
    entity: u32,
    stood: Vector,
    faced: Angles,
}

impl Spectating {
    /// end returns the player to their hero, releases the hold and the
    /// freeze and, with restore, puts the hero back where it stood. A mod
    /// that respawns or moves the hero itself skips the move back.
    pub fn end(self, restore: bool) {
        self.player.unwatch();
        self.player.freeze(Some(false));
        if restore {
            let still = Vector::default();
            self.player.teleport(&self.stood, &self.faced, Some(&still));
        }
        hide_parked(self.entity, false);
    }
}

/// spectate_from takes the player's hero out of play and out of sight, and
/// gives the player the free camera from position. The hero is held as a
/// bystander, hidden by PARKED_STATES, moved PARK_HEIGHT above where it stood
/// (or to park) and frozen there, where gravity does not pull it, so it
/// works on any map. It returns the view, whose end undoes each step, or
/// None when the player has no hero.
pub fn spectate_from(
    player: Player,
    position: Vector,
    facing: Angles,
    park: Option<Vector>,
) -> Option<Spectating> {
    // Remember where the hero stood.
    let pawn = player.pawn().filter(|pawn| pawn.hero != 0)?;
    let (stood, faced) = (pawn.position, pawn.eye_angles);

    // Hide it, park it and freeze it there, then fly the camera.
    hide_parked(pawn.entity, true);
    let above = Vector {
        z: stood.z + PARK_HEIGHT,
        ..stood
    };
    player.teleport(&park.unwrap_or(above), &faced, Some(&Vector::default()));
    player.freeze(Some(true));
    player.free_camera(&position, &facing);
    Some(Spectating {
        player,
        entity: pawn.entity,
        stood,
        faced,
    })
}

/// hide_parked holds or releases BYSTANDER_STATES and PARKED_STATES
/// together.
fn hide_parked(entity: u32, active: bool) {
    hold_bystander(entity, active);
    for state in PARKED_STATES {
        hold_modifier_state(entity, state, Some(active));
    }
}
