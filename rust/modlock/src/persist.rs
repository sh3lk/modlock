//! persist keeps records for each player across sessions, such as a player's
//! stats or unlocks, in the record types a mod declares in mod.json under
//! persist, over the host's persist service. A read starts with the host and
//! answers Loading until the record arrives a frame or more later; an update
//! sends field changes the host applies whole, so two servers that update
//! one player at once both land.
//!
//! ```ignore
//! if let PersistRead::Loaded(stats) = persist::get(player, "stats") {
//!     log!("rounds won {}", stats.number("roundsWon"));
//! }
//! let won = [("roundsWon", PersistChange::Add(1.0))];
//! persist::update(player, "stats", won, PersistOptions::keyed(round_id));
//! ```

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use crate::generated::log_line;
use crate::runtime;
use crate::services::{self, ServiceError};
use crate::wire::{self, Error, Reader};
use crate::Player;

/// SERVICE names the host service that keeps records.
const SERVICE: &str = "persist";

/// LEFT is why a Player kept after its player left is refused.
const LEFT: &str = "the player left; their Player no longer names a connection, and the slot may hold another player";

/// NO_KEY is why an update that adds without a key is refused.
const NO_KEY: &str = "a persist update that adds needs an update key, so a resend counts once";

/// NO_ANSWER is why an update the host answered nothing to is refused.
const NO_ANSWER: &str = "the host answered nothing";

/// PersistValue is one field's value: a number, a text or a tags field's
/// members.
#[derive(Clone, Debug, PartialEq)]
pub enum PersistValue {
    Number(f64),
    Text(String),
    Tags(Vec<String>),
}

/// PersistRecord is one player's record: each field written so far, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PersistRecord {
    pub fields: HashMap<String, PersistValue>,
}

impl PersistRecord {
    /// number returns the number field name holds, or zero for a field never
    /// written.
    pub fn number(&self, name: &str) -> f64 {
        match self.fields.get(name) {
            Some(PersistValue::Number(number)) => *number,
            _ => 0.0,
        }
    }

    /// text returns the text field name holds, or nothing for a field never
    /// written.
    pub fn text(&self, name: &str) -> &str {
        match self.fields.get(name) {
            Some(PersistValue::Text(text)) => text,
            _ => "",
        }
    }

    /// tags returns the members of the tags field name, or none for a field
    /// never written.
    pub fn tags(&self, name: &str) -> &[String] {
        match self.fields.get(name) {
            Some(PersistValue::Tags(members)) => members,
            _ => &[],
        }
    }
}

/// PersistRead is what get knows of a record: Loading until the host
/// answers, then Loaded, or Unavailable with why, such as a host that keeps
/// no records.
#[derive(Clone, Debug, PartialEq)]
pub enum PersistRead {
    Loading,
    Loaded(PersistRecord),
    Unavailable(String),
}

/// PersistChange is one change to one field: add to a count or number, max
/// or min a number, set a count or number or a text, insert or remove a tag.
#[derive(Clone, Debug, PartialEq)]
pub enum PersistChange {
    Add(f64),
    Max(f64),
    Min(f64),
    Set(f64),
    SetText(String),
    Insert(String),
    Remove(String),
}

/// PersistOptions describe one update.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PersistOptions {
    /// key identifies the change, such as a round, so a resent update counts
    /// once. An update that adds needs one.
    pub key: Option<String>,
    /// record selects the record; leave it None, as hosts keep one record of
    /// each type for now.
    pub record: Option<String>,
}

impl PersistOptions {
    /// keyed returns the options of the update key identifies.
    pub fn keyed(key: impl Into<String>) -> Self {
        Self {
            key: Some(key.into()),
            record: None,
        }
    }
}

/// Updated is how an update ended: the record after it, and whether its key
/// made it a repeat the host left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Updated {
    pub record: PersistRecord,
    pub repeated: bool,
}

/// Done ends one update on the mod: with how it ended, or with the error
/// that refused it.
type Done = Box<dyn FnOnce(&mut dyn Any, Result<Updated, String>)>;

/// Entry is what the mod knows of one player's record of one type.
struct Entry {
    read: PersistRead,
    /// started numbers the last request started for the record.
    started: u64,
    /// applied numbers the request whose answer read holds.
    applied: u64,
}

/// Held holds each player's records by type, and numbers the requests.
#[derive(Default)]
struct Held {
    /// entries are keyed by the Player, which is another for a new
    /// connection in the slot, so a record never passes to the next player
    /// in a slot.
    entries: HashMap<(Player, String), Entry>,
    /// requests numbers the requests the mod started, in order.
    requests: u64,
}

thread_local! {
    static HELD: RefCell<Held> = RefCell::default();
}

/// start numbers a new request for player's record of kind, made empty when
/// the mod knows none.
fn start(player: Player, kind: &str) -> u64 {
    HELD.with(|held| {
        let held = &mut *held.borrow_mut();
        held.requests += 1;
        let entry = held.entries.entry((player, kind.to_owned()));
        let entry = entry.or_insert(Entry {
            read: PersistRead::Loading,
            started: 0,
            applied: 0,
        });
        entry.started = held.requests;
        held.requests
    })
}

/// settle keeps an answer to the request numbered request, unless the answer
/// to a later one already arrived.
fn settle(player: Player, kind: &str, request: u64, read: PersistRead) {
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        let entry = held.entries.get_mut(&(player, kind.to_owned()));
        if let Some(entry) = entry.filter(|entry| request >= entry.applied) {
            entry.applied = request;
            entry.read = read;
        }
    });
}

/// generation returns the connection generation player was made for, or None
/// when player no longer names the connection in its slot, as when the mod
/// kept it after the player left. The host checks it again, so a request
/// started just before the player left still cannot reach the next player in
/// the slot.
fn generation(player: Player) -> Option<u32> {
    let held = player.connection()?;
    (held.player == player && held.generation != 0).then_some(held.generation)
}

/// unavailable is the read of a record the host did not answer, logged once.
fn unavailable(kind: &str, error: &str) -> PersistRead {
    let why = if error.is_empty() { NO_ANSWER } else { error };
    log_line(format!("persist {kind}: {why}"));
    PersistRead::Unavailable(why.to_owned())
}

/// answer returns the reply of a call, or why the call has none.
fn answer(reply: Result<Vec<u8>, ServiceError>) -> Result<Vec<u8>, String> {
    reply.map_err(|error| error.message)
}

/// call starts a call of the persist service that ends on the mod.
fn call(
    method: &str,
    payload: &[u8],
    done: impl FnOnce(&mut dyn Any, Result<Vec<u8>, String>) + 'static,
) {
    let ended = Box::new(move |m: &mut dyn Any, reply| done(m, answer(reply)));
    services::call_later(SERVICE, method, payload, Duration::ZERO, ended);
}

/// load asks the host for player's record of kind.
fn load(player: Player, kind: &str) {
    let request = start(player, kind);
    let Some(live) = generation(player) else {
        settle(player, kind, request, unavailable(kind, LEFT));
        return;
    };
    let mut payload = Vec::new();
    slot(&mut payload, player);
    wire::string(&mut payload, 2, kind);
    wire::uint32(&mut payload, 16, live);
    let kind = kind.to_owned();
    call("Get", &payload, move |_, reply| {
        let read = match reply.and_then(|reply| record_of(&reply, 1)) {
            Ok(record) => PersistRead::Loaded(record),
            Err(error) => unavailable(&kind, &error),
        };
        settle(player, &kind, request, read);
    });
}

/// slot writes player's slot as a request's first field.
fn slot(out: &mut Vec<u8>, player: Player) {
    if player.slot() != 0 {
        wire::int32(out, 1, player.slot());
    }
}

/// get returns player's record of kind, a record type the manifest declares.
/// The first get starts the read and answers Loading; a later get answers
/// what the host sent. A record never written loads empty.
pub fn get(player: Player, kind: &str) -> PersistRead {
    let read = HELD.with(|held| {
        let held = held.borrow();
        let known = held.entries.get(&(player, kind.to_owned()));
        known
            .filter(|entry| entry.started != 0)
            .map(|entry| entry.read.clone())
    });
    read.unwrap_or_else(|| {
        load(player, kind);
        get(player, kind)
    })
}

/// get_many starts reading the record of kind for each of players, up to 16,
/// in one call, such as everyone in a lobby at join. get then answers each;
/// a Player kept after its player left answers Unavailable.
pub fn get_many(players: &[Player], kind: &str) {
    // Start each live player's read; a left player's is unavailable at once.
    let mut pending = Vec::new();
    for player in players {
        let request = start(*player, kind);
        match generation(*player) {
            Some(live) => pending.push((*player, request, live)),
            None => settle(*player, kind, request, unavailable(kind, LEFT)),
        }
    }
    if pending.is_empty() {
        return;
    }
    let slots: Vec<i32> = pending.iter().map(|(player, ..)| player.slot()).collect();
    let generations: Vec<u32> = pending.iter().map(|(.., live)| *live).collect();
    let mut payload = Vec::new();
    wire::packed(&mut payload, 1, &slots, |out, slot| {
        wire::raw::int32(out, *slot)
    });
    wire::string(&mut payload, 2, kind);
    wire::packed(&mut payload, 16, &generations, |out, live| {
        wire::raw::uint32(out, *live)
    });
    let kind = kind.to_owned();
    call("GetMany", &payload, move |_, reply| {
        let records = match reply.and_then(|reply| records_of(&reply)) {
            Ok(records) => records,
            Err(error) => {
                let read = unavailable(&kind, &error);
                for (player, request, _) in pending {
                    settle(player, &kind, request, read.clone());
                }
                return;
            }
        };
        for (player, request, _) in pending {
            let record = records.get(&player.slot()).cloned().unwrap_or_default();
            settle(player, &kind, request, PersistRead::Loaded(record));
        }
    });
}

/// update changes player's record of kind by changes, each a field's name
/// and a change, which the host applies all or none. With a key, an update
/// the host already took is left out, so resending after a lost answer
/// counts once. An update with an add and no key is refused without being
/// sent, since a resend would count it twice. An update the host answers as
/// taking too long may still have been kept: send it again with the same
/// key, and do not act as if nothing was written. get answers the changed
/// record once the host has answered. An update for a Player kept after its
/// player left is refused without being sent.
pub fn update<'a>(
    player: Player,
    kind: &str,
    changes: impl IntoIterator<Item = (&'a str, PersistChange)>,
    options: PersistOptions,
) {
    send(player, kind, changes, options, Box::new(|_, _| {}));
}

/// update_then changes player's record as update does, and runs done on the
/// mod once the host has answered: with the record after the update, or with
/// the error that refused it.
pub fn update_then<'a, M: 'static>(
    player: Player,
    kind: &str,
    changes: impl IntoIterator<Item = (&'a str, PersistChange)>,
    options: PersistOptions,
    done: impl FnOnce(&mut M, Result<Updated, String>) + 'static,
) {
    let done: Done = Box::new(move |m, result| {
        if let Some(m) = m.downcast_mut::<M>() {
            done(m, result);
        }
    });
    send(player, kind, changes, options, done);
}

/// send sends one update, and ends it with done.
fn send<'a>(
    player: Player,
    kind: &str,
    changes: impl IntoIterator<Item = (&'a str, PersistChange)>,
    options: PersistOptions,
    done: Done,
) {
    // An add needs a key, as the host requires.
    let changes: Vec<(&str, PersistChange)> = changes.into_iter().collect();
    let adds = |(_, change): &(&str, PersistChange)| matches!(change, PersistChange::Add(_));
    let keyed = options.key.as_ref().is_some_and(|key| !key.is_empty());
    let refused = if !keyed && changes.iter().any(adds) {
        Some(NO_KEY)
    } else if generation(player).is_none() {
        Some(LEFT)
    } else {
        None
    };
    if let Some(why) = refused {
        log_line(format!("persist {kind} update: {why}"));
        runtime::soon(move |m| done(m, Err(why.to_owned())));
        return;
    }

    // Write the request: who, which record, the operations and the key.
    let request = start(player, kind);
    let mut payload = Vec::new();
    slot(&mut payload, player);
    wire::string(&mut payload, 2, kind);
    if let Some(record) = options.record.filter(|record| !record.is_empty()) {
        wire::string(&mut payload, 3, &record);
    }
    for (field, change) in &changes {
        wire::nested(&mut payload, 4, |out| operation(out, field, change));
    }
    if let Some(key) = options.key.filter(|key| !key.is_empty()) {
        wire::string(&mut payload, 5, &key);
    }
    wire::uint32(&mut payload, 16, generation(player).unwrap_or(0));

    // A refused update leaves the record as get last read it.
    let kind = kind.to_owned();
    call("Update", &payload, move |m, reply| {
        let updated = reply.and_then(|reply| updated_of(&reply));
        match &updated {
            Ok(updated) => {
                let read = PersistRead::Loaded(updated.record.clone());
                settle(player, &kind, request, read);
            }
            Err(why) => {
                let why = if why.is_empty() { NO_ANSWER } else { why };
                log_line(format!("persist {kind} update: {why}"));
            }
        }
        done(
            m,
            updated.map_err(|why| {
                if why.is_empty() {
                    NO_ANSWER.to_owned()
                } else {
                    why
                }
            }),
        );
    });
}

/// refresh reads player's record of kind again, such as after Unavailable.
pub fn refresh(player: Player, kind: &str) {
    load(player, kind);
}

/// operation writes one change of field as the host's operation.
fn operation(out: &mut Vec<u8>, field: &str, change: &PersistChange) {
    if !field.is_empty() {
        wire::string(out, 1, field);
    }
    match change {
        PersistChange::Add(number) => wire::double(out, 2, *number),
        PersistChange::Max(number) => wire::double(out, 3, *number),
        PersistChange::Min(number) => wire::double(out, 4, *number),
        PersistChange::Set(number) => wire::nested(out, 5, |out| wire::double(out, 1, *number)),
        PersistChange::SetText(text) => wire::nested(out, 5, |out| wire::string(out, 2, text)),
        PersistChange::Insert(tag) => wire::string(out, 6, tag),
        PersistChange::Remove(tag) => wire::string(out, 7, tag),
    }
}

/// failed turns a reply the library cannot read into the error of its call.
fn failed(Error(why): Error) -> String {
    why.to_owned()
}

/// record_of reads the record a reply holds in the field numbered number.
fn record_of(reply: &[u8], number: u32) -> Result<PersistRecord, String> {
    let mut record = PersistRecord::default();
    let mut r = Reader::new(reply);
    while let Some((held, kind)) = r.field().map_err(failed)? {
        if held == number {
            record = decode(r.bytes(kind).map_err(failed)?).map_err(failed)?;
        } else {
            r.skip(kind).map_err(failed)?;
        }
    }
    Ok(record)
}

/// records_of reads the records of a GetMany reply by slot.
fn records_of(reply: &[u8]) -> Result<HashMap<i32, PersistRecord>, String> {
    let mut records = HashMap::new();
    let mut r = Reader::new(reply);
    while let Some((number, kind)) = r.field().map_err(failed)? {
        if number != 1 {
            r.skip(kind).map_err(failed)?;
            continue;
        }

        // Each entry names a player and holds their record.
        let entry = r.bytes(kind).map_err(failed)?;
        let mut slot = 0;
        let mut inner = Reader::new(entry);
        while let Some((number, kind)) = inner.field().map_err(failed)? {
            match number {
                1 => slot = inner.int32(kind).map_err(failed)?,
                _ => inner.skip(kind).map_err(failed)?,
            }
        }
        records.insert(slot, record_of(entry, 2)?);
    }
    Ok(records)
}

/// updated_of reads an Update reply.
fn updated_of(reply: &[u8]) -> Result<Updated, String> {
    let mut repeated = false;
    let mut r = Reader::new(reply);
    while let Some((number, kind)) = r.field().map_err(failed)? {
        match number {
            2 => repeated = r.bool(kind).map_err(failed)?,
            _ => r.skip(kind).map_err(failed)?,
        }
    }
    let record = record_of(reply, 1)?;
    Ok(Updated { record, repeated })
}

/// decode reads a record the host sent: its fields by name.
fn decode(data: &[u8]) -> Result<PersistRecord, Error> {
    let mut record = PersistRecord::default();
    let mut r = Reader::new(data);
    while let Some((number, kind)) = r.field()? {
        if number != 1 {
            r.skip(kind)?;
            continue;
        }

        // Each entry of the fields is a name and a value.
        let (mut name, mut value) = (String::new(), None);
        let mut entry = Reader::new(r.bytes(kind)?);
        while let Some((number, kind)) = entry.field()? {
            match number {
                1 => name = entry.string(kind)?,
                2 => value = value_of(entry.bytes(kind)?)?,
                _ => entry.skip(kind)?,
            }
        }
        if let Some(value) = value {
            record.fields.insert(name, value);
        }
    }
    Ok(record)
}

/// value_of reads one field's value, or None for a value of no known case.
fn value_of(data: &[u8]) -> Result<Option<PersistValue>, Error> {
    let mut value = None;
    let mut r = Reader::new(data);
    while let Some((number, kind)) = r.field()? {
        match number {
            1 => value = Some(PersistValue::Number(r.double(kind)?)),
            2 => value = Some(PersistValue::Text(r.string(kind)?)),
            3 => {
                let mut members = Vec::new();
                let mut tags = Reader::new(r.bytes(kind)?);
                while let Some((number, kind)) = tags.field()? {
                    match number {
                        1 => members.push(tags.string(kind)?),
                        _ => tags.skip(kind)?,
                    }
                }
                value = Some(PersistValue::Tags(members));
            }
            _ => r.skip(kind)?,
        }
    }
    Ok(value)
}
