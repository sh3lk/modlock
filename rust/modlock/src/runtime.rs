//! runtime is what the library keeps for the mod between events: its
//! commands, its timers, its handlers of modifier events and the list of
//! players. It stands between the game's events and the mod's methods: it
//! answers a command it has a handler for, runs the timers with each frame,
//! and follows the players as they change.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::generated::{
    serve_mod, ChatEvent, CommandEvent, CommandResult, Connection, DamageEvent, DamageResult,
    Events, FrameEvent, Mod, ModifierFiredEvent, Pawn, PlayersChangedEvent, PlayersResponse,
    RestoredEvent, ServiceCall, ServiceRepliedEvent, StartEvent, StartResult, UiInputEvent,
    UiPressEvent,
};
use crate::wire::{Error, Message};
use crate::{host, seating, services, Player};

/// RESPAWNED is the game's modifier event for a hero entering play.
const RESPAWNED: &str = "PLAYER_RESPAWNED";

/// Command runs one command's handler on the mod.
type Command = Rc<dyn Fn(&mut dyn Any, Player, &str)>;

/// Modifier runs one modifier event's handler on the mod.
type Modifier = Rc<dyn Fn(&mut dyn Any, &ModifierFiredEvent)>;

/// Run runs one timer on the mod and returns the wait before its next run in
/// seconds, or None after its last.
type Run = Box<dyn FnMut(&mut dyn Any) -> Option<f64>>;

/// Entry is one timer that has not finished.
struct Entry {
    id: u64,
    /// delay is the first wait, counted from the first frame when the timer
    /// came before one.
    delay: f64,
    started: bool,
    /// at is the game time of the next run, once started.
    at: f64,
    finished: Rc<Cell<bool>>,
    /// run is None while the timer runs.
    run: Option<Run>,
}

#[derive(Default)]
struct State {
    commands: HashMap<String, Command>,
    /// modifiers holds the handlers of each modifier event by its name, in
    /// the order the mod registered them.
    modifiers: HashMap<String, Vec<Modifier>>,
    timers: Vec<Entry>,
    next_timer: u64,
    /// now is the game time of the latest frame, and known whether one came.
    now: f64,
    known: bool,
    /// frames counts frames, so spawned_at can tell one frame's events apart.
    frames: u64,
    /// spawned_at maps each slot to the frame the mod last heard of its hero
    /// entering play; the game repeats the event as a player's hero is built.
    spawned_at: HashMap<i32, u64>,
    /// roster is the player list the game last reported, or None before one,
    /// and rosters counts the reports.
    roster: Option<Vec<Connection>>,
    rosters: u64,
    /// generations holds each slot's connection generation in the last
    /// player list, and tenures counts the connections each slot has lost
    /// since the mod started, so a Player tells a newcomer from the one who
    /// left.
    generations: HashMap<i32, u32>,
    tenures: HashMap<i32, u32>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();

    /// The interface library's part in the events, once the mod shows an
    /// interface.
    static INTERFACE: Cell<Option<Interface>> = const { Cell::new(None) };

    /// The parts the library's helpers take in the events, once the mod
    /// uses them.
    static HOOKS: Cell<Hooks> = const { Cell::new(Hooks { restored: None, puppets: None, chat: None }) };
}

/// Hooks are the parts the library's helpers take in the events. A helper
/// takes its part with its first use, so a mod that uses none carries none
/// of it.
#[derive(Clone, Copy)]
pub(crate) struct Hooks {
    /// restored settles a restore the restore helper started.
    pub restored: Option<fn(&mut dyn Any, &RestoredEvent)>,
    /// puppets moves the playing puppets with each frame.
    pub puppets: Option<fn(&FrameEvent)>,
    /// chat shows a chat line, once the mod shows chat lines; it runs on the
    /// mod, whose say of each speaker the lines may take.
    pub chat: Option<fn(&mut dyn Any, &ChatEvent)>,
}

/// hook changes the parts the library's helpers take in the events.
pub(crate) fn hook(change: impl FnOnce(&mut Hooks)) {
    let mut hooks = HOOKS.get();
    change(&mut hooks);
    HOOKS.set(hooks);
}

/// Interface is the interface library's part in the events: what it does
/// with a frame, a press, an input's change and a new player list. Each runs
/// on the mod: a frame and a new player list may lay the chat lines out
/// again, which may take the mod's say of each speaker. The library joins the
/// events with the first interface the mod shows, so a mod that shows none
/// carries none of the interface library.
#[derive(Clone, Copy)]
pub(crate) struct Interface {
    pub tick: fn(&mut dyn Any, f64),
    pub press: fn(&mut dyn Any, &UiPressEvent),
    pub changed: fn(&mut dyn Any, &UiInputEvent),
    pub forget: fn(&mut dyn Any, &[Connection]),
}

/// join gives the interface library its part in the events, and returns
/// whether it had none before.
pub(crate) fn join(interface: Interface) -> bool {
    INTERFACE.replace(Some(interface)).is_none()
}

/// Refusal is why an event has no answer, which the game logs.
pub(crate) struct Refusal(pub String);

impl From<Error> for Refusal {
    fn from(Error(why): Error) -> Self {
        Refusal(why.to_owned())
    }
}

/// CommandHandler is a function that handles a command: a method of the mod
/// that takes the player who typed it, and the text after the command's name
/// when it wants it.
pub trait CommandHandler<M, Arguments>: 'static {
    fn handle(&self, m: &mut M, player: Player, arguments: &str);
}

impl<M, F: Fn(&mut M, Player) + 'static> CommandHandler<M, (Player,)> for F {
    fn handle(&self, m: &mut M, player: Player, _arguments: &str) {
        self(m, player)
    }
}

impl<M, F: Fn(&mut M, Player, &str) + 'static> CommandHandler<M, (Player, String)> for F {
    fn handle(&self, m: &mut M, player: Player, arguments: &str) {
        self(m, player, arguments)
    }
}

/// command calls handler when a player types /name in chat or their client
/// sends the console command name to the server. The handler runs with the
/// next server frame and receives the text after the name, trimmed of
/// surrounding spaces. Names are lower case and match whatever case the
/// player types. Registering a name again replaces its handler.
pub fn command<M: 'static, Arguments>(name: &str, handler: impl CommandHandler<M, Arguments>) {
    let erased: Command = Rc::new(move |m, player, arguments| {
        if let Some(m) = m.downcast_mut::<M>() {
            handler.handle(m, player, arguments);
        }
    });
    STATE.with(|state| state.borrow_mut().commands.insert(name.to_owned(), erased));
}

/// modifier_event calls handler each time the game fires the modifier event
/// with this EModifierEvent name, such as ABILITY_CAST_STARTED or
/// PARRY_SUCCESS, before the next frame. Register in the mod's start: the
/// library asks the game for the names it holds when start returns. A name
/// the running game lacks is logged once and never fires.
pub fn modifier_event<M: 'static>(
    name: &str,
    handler: impl Fn(&mut M, &ModifierFiredEvent) + 'static,
) {
    let erased: Modifier = Rc::new(move |m, event| {
        if let Some(m) = m.downcast_mut::<M>() {
            handler(m, event);
        }
    });
    STATE.with(|state| {
        state
            .borrow_mut()
            .modifiers
            .entry(name.to_owned())
            .or_default()
            .push(erased)
    });
}

/// Timer is one scheduled call. Time is the game's, so a paused game holds
/// its timers.
#[derive(Clone)]
pub struct Timer {
    finished: Rc<Cell<bool>>,
}

impl Timer {
    /// finished reports whether the timer ran its last time or was cancelled.
    pub fn finished(&self) -> bool {
        self.finished.get()
    }

    /// cancel stops the timer. It does nothing once the timer is finished.
    pub fn cancel(&self) {
        self.finished.set(true);
    }
}

/// after runs run on the mod once, seconds from now.
pub fn after<M: 'static>(seconds: f64, run: impl FnOnce(&mut M) + 'static) -> Timer {
    let mut run = Some(run);
    schedule(seconds, move |m: &mut M| {
        if let Some(run) = run.take() {
            run(m);
        }
        None
    })
}

/// every runs run on the mod each seconds, keeping its cadence through a late
/// frame.
pub fn every<M: 'static>(seconds: f64, mut run: impl FnMut(&mut M) + 'static) -> Timer {
    schedule(seconds, move |m: &mut M| {
        run(m);
        Some(seconds)
    })
}

/// sequence runs run on the mod with the next frame, with the number of times
/// it has run, starting at 1. run returns the seconds to wait before it runs
/// again, or None to end the sequence.
pub fn sequence<M: 'static>(mut run: impl FnMut(&mut M, u32) -> Option<f64> + 'static) -> Timer {
    let mut count = 0;
    schedule(0.0, move |m: &mut M| {
        count += 1;
        run(m, count)
    })
}

/// schedule adds a timer whose first run is delay seconds away.
fn schedule<M: 'static>(delay: f64, mut run: impl FnMut(&mut M) -> Option<f64> + 'static) -> Timer {
    add(
        delay,
        Box::new(move |m| m.downcast_mut::<M>().and_then(&mut run)),
    )
}

/// repeat runs run each seconds for the library itself, whatever the mod is,
/// until run returns false.
pub(crate) fn repeat(seconds: f64, mut run: impl FnMut() -> bool + 'static) -> Timer {
    add(seconds, Box::new(move |_| run().then_some(seconds)))
}

/// soon runs run once with the next frame for the library itself, whatever
/// the mod is.
pub(crate) fn soon(run: impl FnOnce(&mut dyn Any) + 'static) {
    let mut run = Some(run);
    add(
        0.0,
        Box::new(move |m| {
            if let Some(run) = run.take() {
                run(m);
            }
            None
        }),
    );
}

/// add adds the timer that runs run, first delay seconds away.
fn add(delay: f64, run: Run) -> Timer {
    let finished = Rc::new(Cell::new(false));
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let id = state.next_timer;
        state.next_timer += 1;
        let (started, at) = (state.known, state.now + delay);
        state.timers.push(Entry {
            id,
            delay,
            started,
            at,
            finished: finished.clone(),
            run: Some(run),
        });
    });
    Timer { finished }
}

/// advance moves the clock to the frame at seconds and runs the timers that
/// came due, at most once each and in the order they came due. A clock that
/// runs backward, as a new map's does, keeps each timer's remaining delay.
fn advance(m: &mut dyn Any, seconds: f64) {
    // Start the timers made before the clock was known, and take the due
    // ones. A timer a call makes waits for the next frame.
    let mut due: Vec<(f64, u64)> = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let back = if state.known && seconds < state.now {
            seconds - state.now
        } else {
            0.0
        };
        state.now = seconds;
        state.known = true;
        state.frames += 1;
        let mut due = Vec::new();
        for timer in &mut state.timers {
            if !timer.started {
                timer.started = true;
                timer.at = seconds + timer.delay;
            } else {
                timer.at += back;
            }
            if !timer.finished.get() && timer.at <= seconds {
                due.push((timer.at, timer.id));
            }
        }
        due
    });
    due.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Run each with the state free, since its call may make a timer or a
    // command.
    for (_, id) in due {
        let taken = STATE.with(|state| {
            let mut state = state.borrow_mut();
            let timer = state.timers.iter_mut().find(|timer| timer.id == id)?;
            if timer.finished.get() {
                return None;
            }
            timer.run.take()
        });
        let Some(mut run) = taken else { continue };
        let wait = run(m);
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(timer) = state.timers.iter_mut().find(|timer| timer.id == id) else {
                return;
            };
            match wait {
                Some(wait) => {
                    timer.at += wait;
                    if timer.at <= seconds {
                        timer.at = seconds + wait;
                    }
                    timer.run = Some(run);
                }
                None => timer.finished.set(true),
            }
        });
    }
    STATE.with(|state| {
        state
            .borrow_mut()
            .timers
            .retain(|timer| !timer.finished.get())
    });
}

/// players lists the occupied player slots in slot order. It answers from the
/// list the game last reported, and asks the game until a report arrives.
pub fn players() -> Vec<Connection> {
    if let Some(roster) = STATE.with(|state| state.borrow().roster.clone()) {
        return roster;
    }
    host::call("Players", |_| {}, |r| PlayersResponse::decode(r.rest()))
        .map(|response| response.players)
        .unwrap_or_default()
}

/// connection_of_steam_id returns the connection of the account with a Steam
/// ID, or None when it is not connected.
pub fn connection_of_steam_id(steam_id: u64) -> Option<Connection> {
    players()
        .into_iter()
        .find(|connection| connection.steam_id == steam_id)
}

/// reported returns the player list the game last reported with the count of
/// its reports, which changes with each one, or None before the first.
pub(crate) fn reported() -> Option<(u64, Vec<Connection>)> {
    STATE.with(|state| {
        let state = state.borrow();
        state.roster.clone().map(|roster| (state.rosters, roster))
    })
}

/// now returns the game time of the latest frame, or zero before the first.
pub(crate) fn now() -> f64 {
    STATE.with(|state| state.borrow().now)
}

/// reports returns the count of player lists the game has reported.
pub(crate) fn reports() -> u64 {
    STATE.with(|state| state.borrow().rosters)
}

impl Player {
    /// of returns the Player of the connection that holds slot now, equal to
    /// every other Player made for that connection.
    pub fn of(slot: i32) -> Player {
        let tenure = STATE.with(|state| state.borrow().tenures.get(&slot).copied().unwrap_or(0));
        Player::held(slot, tenure + 1)
    }

    /// known returns the Player of the connection that holds slot now, or
    /// None for a negative slot, which the game reports where no player
    /// stands behind what an event tells of.
    pub fn known(slot: i32) -> Option<Player> {
        (slot >= 0).then(|| Player::of(slot))
    }

    /// connection returns the player's connection: their name, Steam ID,
    /// whether they are a bot and whether they finished joining. It returns
    /// None when nobody holds the slot.
    pub fn connection(self) -> Option<Connection> {
        players()
            .into_iter()
            .find(|connection| connection.player.slot() == self.slot())
    }

    /// setting_on returns the player's value of a switch setting the
    /// manifest declares, or None when the host refuses the read.
    pub fn setting_on(self, key: impl AsRef<str>) -> Option<bool> {
        self.setting(key).map(|value| value == "true")
    }

    /// setting_number returns the player's value of a number setting the
    /// manifest declares, or None when the host refuses the read or the
    /// value is no number.
    pub fn setting_number(self, key: impl AsRef<str>) -> Option<f64> {
        self.setting(key)?.parse().ok()
    }
}

/// follow keeps the player list event reports, ends the tenure of each slot
/// whose connection is gone, and gives each connection of event the Player of
/// its tenure.
fn follow(event: &mut PlayersChangedEvent) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let current: HashMap<i32, u32> = event
            .players
            .iter()
            .map(|connection| (connection.player.slot(), connection.generation))
            .collect();
        let ended: Vec<i32> = state
            .generations
            .iter()
            .filter(|(slot, generation)| current.get(slot) != Some(generation))
            .map(|(slot, _)| *slot)
            .collect();
        for slot in ended {
            *state.tenures.entry(slot).or_default() += 1;
        }
        state.generations = current;
        for connection in &mut event.players {
            let slot = connection.player.slot();
            connection.player =
                Player::held(slot, state.tenures.get(&slot).copied().unwrap_or(0) + 1);
        }
        state.roster = Some(event.players.clone());
        state.rosters += 1;
    });
}

/// respawned tells the mod, through handle, that the player's hero entered
/// play.
fn respawned<M>(m: &mut M, handle: fn(&mut M, Player, &Pawn), player: Player) {
    // Skip a placeholder pawn, which has hero 0, and a hero the mod already
    // heard of this frame.
    let Some(pawn) = player.pawn() else { return };
    if pawn.hero == 0 || pawn.health <= 0 {
        return;
    }
    let heard = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let frame = state.frames;
        state.spawned_at.insert(player.slot(), frame) == Some(frame)
    });
    if !heard {
        handle(m, player, &pawn);
    }
}

/// deliver runs the event method on the mod with its encoded request and
/// appends the encoded response to out. It returns false for a method the
/// library does not know.
pub(crate) fn deliver<M: Mod + 'static>(
    m: &mut M,
    events: &Events<M>,
    method: &str,
    request: &[u8],
    out: &mut Vec<u8>,
) -> Result<bool, Refusal> {
    match method {
        // Ask the game for the events the mod handles. Timers run from
        // frames, so the library always takes them, and it always takes the
        // joins too: what it keeps for each player follows who comes and
        // goes, whatever events the mod handles.
        "Start" => {
            m.start(&StartEvent::decode(request)?);
            let mut wants = StartResult::default();
            events.ask(&mut wants);
            wants.frames = true;
            wants.joins = true;
            wants.chat |= HOOKS.get().chat.is_some();
            wants.modifier_events =
                STATE.with(|state| state.borrow().modifiers.keys().cloned().collect());
            if events.spawned.is_some() && !wants.modifier_events.iter().any(|n| n == RESPAWNED) {
                wants.modifier_events.push(RESPAWNED.to_owned());
            }
            wants.modifier_events.sort();
            wants.encode(out);
        }
        "Frame" => {
            let event = FrameEvent::decode(request)?;
            advance(m, event.time_seconds);
            if let Some(interface) = INTERFACE.get() {
                (interface.tick)(m, event.time_seconds);
            }
            if let Some(puppets) = HOOKS.get().puppets {
                puppets(&event);
            }

            // Tell the mod, then seat the players: seating follows the
            // mod's frame, so it asks for what the mod wants as of this
            // frame.
            m.frame(&event);
            seating::frame(event.time_seconds);
        }
        // Claim a command that has a handler and run it with the next frame.
        // The server reads a client's commands under a CPU budget and drops
        // the client when one handler spends it; slow work in a frame only
        // delays that frame.
        "Command" => {
            let event = CommandEvent::decode(request)?;
            let line = event.line.trim();
            let (name, arguments) = line.split_once(' ').unwrap_or((line, ""));
            let handler = STATE.with(|state| state.borrow().commands.get(name).cloned());
            let Some(handler) = handler else {
                m.command(&event).encode(out);
                return Ok(true);
            };
            let (player, arguments) = (event.player, arguments.trim().to_owned());
            add(
                0.0,
                Box::new(move |m| {
                    handler(m, player, &arguments);
                    None
                }),
            );
            CommandResult { claimed: true }.encode(out);
        }
        // A chat line and a press arrive under the same CPU budget as a
        // command, so their handlers also run with the next frame.
        "Chat" => {
            let (heard, handle) = (HOOKS.get().chat, events.chat);
            if heard.is_some() || handle.is_some() {
                let event = ChatEvent::decode(request)?;
                after(0.0, move |m: &mut M| {
                    if let Some(heard) = heard {
                        heard(m, &event);
                    }
                    if let Some(handle) = handle {
                        handle(m, &event);
                    }
                });
            }
        }
        "UiPress" => {
            let event = UiPressEvent::decode(request)?;
            after(0.0, move |m: &mut M| {
                if let Some(interface) = INTERFACE.get() {
                    (interface.press)(m, &event);
                }
            });
        }
        "UiInput" => {
            let event = UiInputEvent::decode(request)?;
            after(0.0, move |m: &mut M| {
                if let Some(interface) = INTERFACE.get() {
                    (interface.changed)(m, &event);
                }
            });
        }
        // Tell the mod of a respawn, then run the handlers of the event's
        // name.
        "ModifierFired" => {
            let event = ModifierFiredEvent::decode(request)?;
            if event.name == RESPAWNED {
                if let (Some(handle), Some(player)) = (events.spawned, event.player) {
                    respawned(m, handle, player);
                }
            }
            let handlers = STATE.with(|state| state.borrow().modifiers.get(&event.name).cloned());
            for handler in handlers.unwrap_or_default() {
                handler(m, &event);
            }
        }
        // Answer a hit the mod lets land with its new amount alone, and with
        // nothing when the amount stands. A block goes as the mod gave it.
        "Damage" => {
            if let Some(handle) = events.damage {
                let event = DamageEvent::decode(request)?;
                let change = handle(m, &event);
                if change.block == Some(true) {
                    change.encode(out);
                } else {
                    let amount = change.amount.filter(|amount| *amount != event.amount);
                    DamageResult {
                        amount,
                        ..DamageResult::default()
                    }
                    .encode(out);
                }
            }
        }
        "Restored" => {
            let event = RestoredEvent::decode(request)?;
            if let Some(settle) = HOOKS.get().restored {
                settle(m, &event);
            }
            m.restored(&event);
        }
        "Serve" => services::answer(m, &ServiceCall::decode(request)?)?.encode(out),
        "ServiceReplied" => services::replied(m, ServiceRepliedEvent::decode(request)?),
        "PlayersChanged" => {
            let mut event = PlayersChangedEvent::decode(request)?;
            follow(&mut event);
            if let Some(interface) = INTERFACE.get() {
                (interface.forget)(m, &event.players);
            }
            m.players_changed(&event);
        }
        _ => {
            return Ok(events.serve(m, method, request, out)? || serve_mod(m, method, request, out)?)
        }
    }
    Ok(true)
}
