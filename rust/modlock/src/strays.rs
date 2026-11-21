//! Strays lays down hero pawns that no player holds any more. A hero swap, a
//! move to the spectators or a reload can leave the old hero standing in the
//! world, and the game revives it with no player on its own respawn timer.
//! Removing a hero pawn leaves its abilities and items networked, which
//! closes clients, and killing one alone lets the game revive it. So a stray
//! is killed once and has its respawn held off for good; a dead hero draws
//! nothing and casts nothing.
//!
//! A pawn is held when a connected player's hero is it, or when its own
//! controller's hero is it: a player still joining, or watching from the
//! spectators, has a controller that names their hero before the player
//! reports it. Only a pawn both checks disown is a stray.
//!
//! Stragglers clears players who hold the server without playing. A human
//! who stops loading keeps their slot as a connecting player until the
//! network gives up, and the game keeps a disconnected player's hero
//! standing in the world. Stragglers kicks a connection that has not
//! finished joining by a deadline and lays down the heroes Strays finds no
//! player holding.
//!
//! It sees only the slots the game lists: an attempt the engine never put in
//! the server has no slot, so no mod can see or kick it.

use std::collections::{HashMap, HashSet};

use crate::bystander::hold_bystander;
use crate::generated::{
    damage, find_entities, log_line, read_field, server_command, write_field, FieldType,
    FieldValue, HitOptions,
};
use crate::runtime::players;

/// STRAY_GRACE_SECONDS is how long a hero pawn may belong to no player
/// before Strays lays it down.
pub const STRAY_GRACE_SECONDS: f64 = 3.0;

/// STRAGGLER_CONNECT_SECONDS is how long a human may take to finish joining,
/// unless the mod gives another time.
pub const STRAGGLER_CONNECT_SECONDS: f64 = 120.0;

/// STRAY_RESPAWN_TIME is the game time a stray's respawn is held at, past
/// any session.
const STRAY_RESPAWN_TIME: f64 = 1e9;

/// STRAY_BLOW is a hit no hero's resistances survive.
const STRAY_BLOW: f32 = 1_000_000.0;

/// Strays finds and lays down stray hero pawns. A mod that swaps heroes
/// keeps one and calls sweep each frame with the game time; it looks once a
/// second.
#[derive(Debug)]
pub struct Strays {
    /// since maps each stray pawn to when it was first seen with no player.
    since: HashMap<u32, f64>,
    /// gone holds the controller handles strays name that no longer resolve.
    /// A handle carries its entity's serial, so it never resolves again, and
    /// each failed read logs on the host: a sweep reads it once.
    gone: HashSet<u32>,
    swept_at: f64,
}

impl Default for Strays {
    fn default() -> Self {
        Self {
            since: HashMap::new(),
            gone: HashSet::new(),
            swept_at: f64::NEG_INFINITY,
        }
    }
}

impl Strays {
    /// new returns a Strays that has seen no pawn.
    pub fn new() -> Self {
        Self::default()
    }

    /// sweep lays down each hero pawn that has belonged to no player for
    /// STRAY_GRACE_SECONDS, at most once a second. The game sets a dead
    /// pawn's respawn when it dies, so the hold repeats on each sweep.
    pub fn sweep(&mut self, now: f64) {
        // Look at most once a second.
        if now - self.swept_at < 1.0 {
            return;
        }
        self.swept_at = now;

        // A pawn is held when any connected player's hero is it.
        let pawns = players().into_iter().filter_map(|held| held.player.pawn());
        let held: HashSet<u32> = pawns.map(|pawn| pawn.entity).collect();

        // Lay a pawn down once it has belonged to no one for the grace
        // period.
        let mut since = HashMap::new();
        let mut gone = HashSet::new();
        for entity in find_entities("player") {
            if held.contains(&entity) || own_hero(entity, &self.gone, &mut gone) {
                continue;
            }
            let first = self.since.get(&entity).copied().unwrap_or(now);
            since.insert(entity, first);
            if now - first >= STRAY_GRACE_SECONDS {
                lay_down(entity);
            }
        }
        self.since = since;
        self.gone = gone;
    }
}

/// number reads a field of entity as a number: a value of 32 bits or less,
/// or a handle.
fn number(entity: u32, class: &str, field: &str, kind: FieldType) -> Option<f64> {
    match read_field(entity, class, field, kind)? {
        FieldValue::Number(number) => Some(number),
        FieldValue::Integer(integer) => Some(integer as f64),
        _ => None,
    }
}

/// own_hero reports whether the hero pawn entity is its controller's hero. A
/// pawn whose controller is gone, or names another hero, is not. It skips a
/// controller in known as gone and adds each gone controller it sees to
/// seen.
fn own_hero(entity: u32, known: &HashSet<u32>, seen: &mut HashSet<u32>) -> bool {
    // A controller must still hold this hero on behalf of a connection.
    let held = number(
        entity,
        "CBasePlayerPawn",
        "m_hController",
        FieldType::Handle,
    );
    let Some(controller) = held.map(|handle| handle as u32) else {
        return false;
    };
    if known.contains(&controller) {
        seen.insert(controller);
        return false;
    }

    // Disconnecting and disconnected controllers may keep naming old heroes.
    let class = "CBasePlayerController";
    let connected = number(controller, class, "m_iConnected", FieldType::Int32);
    if connected == Some(3.0) || connected == Some(4.0) {
        return false;
    }
    let class = "CCitadelPlayerController";
    let hero = number(controller, class, "m_hHeroPawn", FieldType::Handle);

    // A controller whose fields both fail to read no longer resolves.
    if connected.is_none() && hero.is_none() {
        seen.insert(controller);
    }
    hero == Some(f64::from(entity))
}

/// lay_down kills a stray hero pawn if it stands and keeps its respawn out
/// of reach. Its abilities stay: killing ability entities on a live hero
/// pawn has ended the server process.
fn lay_down(entity: u32) {
    // Kill it if it stands, then hold its respawn past any session.
    hold_bystander(entity, false);
    let health = number(entity, "CBaseEntity", "m_iHealth", FieldType::Int32);
    if health.is_some_and(|health| health > 0.0) {
        damage(&HitOptions {
            victim: entity,
            amount: STRAY_BLOW,
            ..HitOptions::default()
        });
        log_line(format!("laid down a stray hero pawn {entity}"));
    }
    write_field(
        entity,
        "CCitadelPlayerPawn",
        "m_flRespawnTime",
        FieldType::Float32,
        &FieldValue::Number(STRAY_RESPAWN_TIME),
    );
}

/// Joining is a human still connecting: the generation of their connection
/// and when Stragglers first saw it.
#[derive(Clone, Copy, Debug)]
struct Joining {
    generation: u32,
    since: f64,
}

/// Stragglers kicks players stuck joining and lays down heroes left without
/// a player. A mod keeps one and calls sweep each frame with the game time;
/// it looks once a second.
#[derive(Debug)]
pub struct Stragglers {
    connect_seconds: f64,
    strays: Option<Strays>,
    /// joining maps each connecting human's slot to its connection.
    joining: HashMap<i32, Joining>,
    /// kicked holds the generation each slot had when this kicked it.
    kicked: HashMap<i32, u32>,
    swept_at: f64,
}

impl Default for Stragglers {
    fn default() -> Self {
        Self {
            connect_seconds: STRAGGLER_CONNECT_SECONDS,
            strays: Some(Strays::new()),
            joining: HashMap::new(),
            kicked: HashMap::new(),
            swept_at: f64::NEG_INFINITY,
        }
    }
}

impl Stragglers {
    /// new returns a Stragglers that kicks a human still connecting after
    /// STRAGGLER_CONNECT_SECONDS and lays down the heroes no player holds.
    pub fn new() -> Self {
        Self::default()
    }

    /// connect_seconds is how long a human may stay connecting before the
    /// server kicks them. Zero or less never kicks.
    pub fn connect_seconds(mut self, seconds: f64) -> Self {
        self.connect_seconds = seconds;
        self
    }

    /// leave_heroes leaves the heroes no player holds standing, where
    /// Stragglers otherwise lays them down as Strays does.
    pub fn leave_heroes(mut self) -> Self {
        self.strays = None;
        self
    }

    /// sweep kicks each human connecting for longer than the time allowed,
    /// once per connection, and lays down heroes no player holds, at most
    /// once a second.
    pub fn sweep(&mut self, now: f64) {
        // Lay down heroes left without a player; Strays paces itself.
        if let Some(strays) = &mut self.strays {
            strays.sweep(now);
        }

        // Look for stuck joins at most once a second.
        if now - self.swept_at < 1.0 {
            return;
        }
        self.swept_at = now;
        if self.connect_seconds <= 0.0 {
            return;
        }

        // Time each human still joining from the first sweep that saw them.
        let mut joining = HashMap::new();
        for connection in players() {
            if connection.bot || connection.ready {
                continue;
            }
            let slot = connection.player.slot();
            let seen = self.joining.get(&slot);
            let known = seen.filter(|seen| seen.generation == connection.generation);
            let since = known.map_or(now, |seen| seen.since);
            let generation = connection.generation;
            joining.insert(slot, Joining { generation, since });
            if now - since < self.connect_seconds {
                continue;
            }

            // Kick each stuck connection once.
            if self.kicked.insert(slot, generation) == Some(generation) {
                continue;
            }
            let name = &connection.name;
            if name.is_empty() || name.contains('"') {
                log_line(format!(
                    "slot {slot} stuck joining, but its name cannot be kicked"
                ));
                continue;
            }
            let seconds = self.connect_seconds;
            log_line(format!("kicked {name}: still joining after {seconds} s"));
            server_command(format!("kick \"{name}\""));
        }
        self.joining = joining;
    }
}
