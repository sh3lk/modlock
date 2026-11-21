//! hook is Bebop's hook for a mod that plays on it: the catch read, throw
//! detection, the cooldown length and the upgrade lock. A mod keeps one Hook
//! per thrower, calls update each frame the thrower's hero lives, and scores
//! what it reports.

use crate::generated::{read_field, Ability, AbilityOptions, FieldType, FieldValue};
use crate::Player;

/// HOOK_ABILITY is the designer name of Bebop's hook.
pub const HOOK_ABILITY: &str = "citadel_ability_hook";

/// HOOK_UPGRADES unlocks the hook with all three upgrades.
pub const HOOK_UPGRADES: u32 = 0b1111;

/// HookRead is what the hook did this frame: the hero it holds, if any, and
/// whether a new throw began since the last frame Hook reported one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookRead {
    pub victim: Option<u32>,
    pub thrown: bool,
}

/// Hook watches one thrower's hook. Its cooldown lasts its seconds from each
/// throw; zero ends it at once, so the hook is always ready.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hook {
    cooldown_seconds: f32,
    /// thrown_at is the start of the latest cooldown counted as a throw.
    thrown_at: f32,
}

impl Hook {
    /// new returns the Hook whose cooldown lasts cooldown_seconds from each
    /// throw.
    pub fn new(cooldown_seconds: f32) -> Self {
        Self {
            cooldown_seconds,
            thrown_at: 0.0,
        }
    }

    /// reset forgets the throws counted so far, for a thrower in a new
    /// world.
    pub fn reset(&mut self) {
        self.thrown_at = 0.0;
    }

    /// update reads the thrower's hook at game time now, keeps it fully
    /// upgraded and sets the length of any cooldown a throw started. It
    /// reports nothing for a hero without the hook.
    pub fn update(&mut self, player: Player, now: f64) -> Option<HookRead> {
        self.update_from(player, now, &player.abilities())
    }

    /// update_from is update for a caller that already read this frame's
    /// abilities, which saves another read.
    pub fn update_from(
        &mut self,
        player: Player,
        now: f64,
        abilities: &[Ability],
    ) -> Option<HookRead> {
        // Find the hook and read the hero it holds.
        let ability = abilities.iter().find(|held| held.name == HOOK_ABILITY)?;
        let victim = victim(ability.entity);

        // Keep the hook unlocked with every upgrade.
        if ability.upgrades & HOOK_UPGRADES != HOOK_UPGRADES {
            player.set_ability(&AbilityOptions {
                ability: HOOK_ABILITY.to_owned(),
                upgrades: Some(HOOK_UPGRADES),
                ..AbilityOptions::default()
            });
        }

        // A cooldown that started since the last one counted is a throw.
        if f64::from(ability.cooldown_end) <= now {
            let thrown = false;
            return Some(HookRead { victim, thrown });
        }
        let thrown = ability.cooldown_start > self.thrown_at;
        if thrown {
            self.thrown_at = ability.cooldown_start;
        }

        // Set the cooldown's length: none ends it, else it ends that long
        // after the throw.
        self.set_cooldown(player, ability);
        Some(HookRead { victim, thrown })
    }

    /// set_cooldown ends the cooldown the Hook's seconds after the hook's
    /// throw.
    fn set_cooldown(&self, player: Player, ability: &Ability) {
        if self.cooldown_seconds <= 0.0 {
            player.refresh_ability(HOOK_ABILITY);
            return;
        }
        let cooldown_end = ability.cooldown_start + self.cooldown_seconds;
        if cooldown_end != ability.cooldown_end {
            player.set_ability(&AbilityOptions {
                ability: HOOK_ABILITY.to_owned(),
                cooldown_end: Some(cooldown_end),
                ..AbilityOptions::default()
            });
        }
    }
}

/// victim is the hero the hook entity holds, or None while it holds none.
fn victim(hook: u32) -> Option<u32> {
    let class = "CCitadel_Ability_Hook";
    match read_field(hook, class, "m_hHookVictim", FieldType::Handle)? {
        FieldValue::Number(handle) => Some(handle as u32),
        FieldValue::Integer(handle) => Some(handle as u32),
        _ => None,
    }
}
