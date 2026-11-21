//! Replayed damage applies a hit that was recorded elsewhere, so a hero
//! takes it as if the attacker had dealt it just now.

use crate::generated::{damage, HitOptions};
use crate::Player;

/// ReplayedHit is one recorded hit.
#[derive(Clone, Debug, PartialEq)]
pub struct ReplayedHit {
    /// victim is the player whose hero takes the hit.
    pub victim: Player,
    /// attacker is the player credited with the hit, usually a puppet.
    /// Without one the hit is a hazard that credits no one.
    pub attacker: Option<Player>,
    /// amount is the damage before resistances.
    pub amount: f32,
    /// headshot makes the hit land on the head.
    pub headshot: bool,
    /// ability names the attacker's ability that dealt the hit, such as
    /// citadel_ability_hornet_snipe. It is left out of the hit when the
    /// attacker does not own it.
    pub ability: Option<String>,
}

impl ReplayedHit {
    /// new returns a hit of amount on victim's hero that credits no one.
    pub fn new(victim: Player, amount: f32) -> Self {
        Self {
            victim,
            attacker: None,
            amount,
            headshot: false,
            ability: None,
        }
    }

    /// by credits the hit to attacker.
    pub fn by(mut self, attacker: Player) -> Self {
        self.attacker = Some(attacker);
        self
    }
}

/// replay_damage deals hit through the game's own damage, so resistances,
/// kill credit, hit effects and the damage events apply. It returns false
/// when either hero is not alive to take part; the game logs why.
pub fn replay_damage(hit: &ReplayedHit) -> bool {
    // Both heroes must be in the world.
    let Some(victim) = hit.victim.pawn() else {
        return false;
    };
    let attacker = hit.attacker.and_then(Player::pawn);
    if hit.attacker.is_some() && attacker.is_none() {
        return false;
    }

    // Credit the ability by its entity when the attacker owns it.
    let ability = hit
        .ability
        .as_ref()
        .zip(hit.attacker)
        .and_then(|(name, by)| {
            let mut owned = by.abilities().into_iter();
            owned.find(|owned| &owned.name == name)
        });
    damage(&HitOptions {
        victim: victim.entity,
        attacker: attacker.map(|attacker| attacker.entity),
        amount: hit.amount,
        hit_group: hit.headshot.then_some(1),
        ability: ability.map(|ability| ability.entity),
        ..HitOptions::default()
    })
}
