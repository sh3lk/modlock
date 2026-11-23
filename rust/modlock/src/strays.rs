//! Strays kills hero pawns that no player holds any more. A hero swap, a
//! move to the spectators or a reload can leave the old hero standing in the
//! world, and the game revives it with no player on its own respawn timer.
//! Removing a hero pawn leaves its abilities and items networked, which
//! closes clients, and killing one alone lets the game revive it. So a stray
//! is killed once and has its respawn held off for good; a dead hero draws
//! nothing and casts nothing. The kill is a forced death: the game drops a
//! damage hit on a disconnected player's hero. Each stray is tried once, and
//! one that still stands is reported once and left alone.
//!
//! A pawn is held when a connected player's hero is it, or when its own
//! controller's hero is it: a player still joining, or watching from the
//! spectators, has a controller that names their hero before the player
//! reports it. A disconnected controller's hero is held while its seat is
//! away, since the server gives that seat back to its player and a returning
//! client whose hero lies dead drops the connection. Only a pawn both checks
//! disown is a stray; an ended seat's hero Modlock kills itself.
//!
//! Stragglers clears players who hold the server without playing. A human
//! who stops loading keeps their slot as a connecting player until the
//! network gives up, and the game keeps a disconnected player's hero
//! standing in the world. Stragglers kicks a connection that has not
//! finished joining by a deadline and kills the heroes Strays finds no
//! player holding.
//!
//! It sees only the slots the game lists: an attempt the engine never put in
//! the server has no slot, so no mod can see or kick it.

use std::collections::{HashMap, HashSet};

use crate::bystander::hold_bystander;
use crate::generated::{
    find_entities, kill_entity, log_line, read_field, server_command, write_field, FieldType,
    FieldValue, KillOptions,
};
use crate::runtime::players;

/// STRAY_GRACE_SECONDS is how long a hero pawn may belong to no player
/// before Strays kills it.
pub const STRAY_GRACE_SECONDS: f64 = 3.0;

/// STRAGGLER_CONNECT_SECONDS is how long a human may take to finish joining,
/// unless the mod gives another time.
pub const STRAGGLER_CONNECT_SECONDS: f64 = 120.0;

/// STRAY_RESPAWN_TIME is the game time a stray's respawn is held at, past
/// any session.
const STRAY_RESPAWN_TIME: f64 = 1e9;

/// Strays finds and kills stray hero pawns. A mod that swaps heroes keeps
/// one and calls sweep each frame with the game time; it looks once a second.
#[derive(Debug)]
pub struct Strays {
    /// since maps each stray pawn to when it was first seen with no player.
    since: HashMap<u32, f64>,
    /// gone holds the controller handles strays name that no longer resolve.
    /// A handle carries its entity's serial, so it never resolves again, and
    /// each failed read logs on the host: a sweep reads it once.
    gone: HashSet<u32>,
    /// killed holds the strays already killed, or tried, for as long as they
    /// stay strays: each is tried once, then only has its respawn held.
    killed: HashSet<u32>,
    swept_at: f64,
}

impl Default for Strays {
    fn default() -> Self {
        Self {
            since: HashMap::new(),
            gone: HashSet::new(),
            killed: HashSet::new(),
            swept_at: f64::NEG_INFINITY,
        }
    }
}

impl Strays {
    /// new returns a Strays that has seen no pawn.
    pub fn new() -> Self {
        Self::default()
    }

    /// sweep kills each hero pawn that has belonged to no player for
    /// STRAY_GRACE_SECONDS, once per pawn, at most once a second. The game
    /// sets a dead pawn's respawn when it dies, so the hold repeats on each
    /// sweep. A pawn a player holds again is forgotten, and killed anew if it
    /// strays again.
    pub fn sweep(&mut self, now: f64) {
        // Look at most once a second.
        if now - self.swept_at < 1.0 {
            return;
        }
        self.swept_at = now;

        // A pawn is held when any connected player's hero is it. An away
        // seat holds its hero too, through own_hero.
        let mut held = HashSet::new();
        let mut away = HashSet::new();
        for connection in players() {
            if connection.away {
                away.insert(connection.player.slot());
                continue;
            }
            if let Some(pawn) = connection.player.pawn() {
                held.insert(pawn.entity);
            }
        }

        // Kill a pawn once it has belonged to no one for the grace period.
        let mut since = HashMap::new();
        let mut gone = HashSet::new();
        let mut killed = HashSet::new();
        for entity in find_entities("player") {
            if held.contains(&entity) || own_hero(entity, &away, &self.gone, &mut gone) {
                continue;
            }
            if self.killed.contains(&entity) {
                hold_respawn(entity);
                killed.insert(entity);
                continue;
            }
            let first = self.since.get(&entity).copied().unwrap_or(now);
            since.insert(entity, first);
            if now - first >= STRAY_GRACE_SECONDS {
                kill_stray(entity);
                killed.insert(entity);
            }
        }
        self.since = since;
        self.gone = gone;
        self.killed = killed;
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

/// own_hero reports whether the hero pawn entity is its controller's hero, for
/// a controller that is connected or whose slot is in away. A pawn whose
/// controller is gone, disconnected for good, or names another hero, is not.
/// It skips a controller in known as gone and adds each gone controller it
/// sees to seen.
fn own_hero(
    entity: u32,
    away: &HashSet<i32>,
    known: &HashSet<u32>,
    seen: &mut HashSet<u32>,
) -> bool {
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

    // Disconnecting and disconnected controllers may keep naming old heroes;
    // only an away seat's still holds one. A player controller's entity index
    // is its slot plus one.
    let class = "CBasePlayerController";
    let connected = number(controller, class, "m_iConnected", FieldType::Int32);
    if (connected == Some(3.0) || connected == Some(4.0))
        && !away.contains(&(((controller & 0x7fff) as i32) - 1))
    {
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

/// kill_stray kills a stray hero pawn if it stands and keeps its respawn out
/// of reach. Its abilities stay: killing ability entities on a live hero
/// pawn has ended the server process. A pawn the forced death leaves standing
/// is reported once; the sweep does not try it again.
fn kill_stray(entity: u32) {
    // Release any bystander hold, which makes the hero invulnerable.
    hold_bystander(entity, false);
    let health = number(entity, "CBaseEntity", "m_iHealth", FieldType::Int32);
    if !health.is_some_and(|health| health > 0.0) {
        hold_respawn(entity);
        return;
    }

    // Force the death, then hold the respawn the death set.
    kill_entity(&KillOptions {
        victim: entity,
        ..KillOptions::default()
    });
    hold_respawn(entity);
    let left = number(entity, "CBaseEntity", "m_iHealth", FieldType::Int32);
    if left.is_some_and(|left| left > 0.0) {
        log_line(format!(
            "stray hero pawn {entity} still stands after a forced death; left alone"
        ));
        return;
    }
    log_line(format!("killed a stray hero pawn {entity}"));
}

/// hold_respawn holds the dead hero pawn's respawn past any session.
fn hold_respawn(entity: u32) {
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

/// Stragglers kicks players stuck joining and kills heroes left without
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
    /// STRAGGLER_CONNECT_SECONDS and kills the heroes no player holds.
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
    /// Stragglers otherwise kills them as Strays does.
    pub fn leave_heroes(mut self) -> Self {
        self.strays = None;
        self
    }

    /// sweep kicks each human connecting for longer than the time allowed,
    /// once per connection, and kills heroes no player holds, at most once a
    /// second.
    pub fn sweep(&mut self, now: f64) {
        // Kill heroes left without a player; Strays paces itself.
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
