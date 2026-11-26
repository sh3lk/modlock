//! The tests of the generated module: its encoding against the Python
//! library's schema, and its calls and events against a host that records
//! them.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use crate::generated::round_trip;
use crate::host::{self, fake};
use crate::prelude::*;
use crate::wire::{self, Message};

/// hex returns the bytes text spells in hexadecimal.
pub(crate) fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

/// spell returns data in hexadecimal.
fn spell(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Calls holds each call the mod made: its method and its request in
/// hexadecimal.
type Calls = Rc<RefCell<Vec<(String, String)>>>;

/// answer makes the host record each call and answer it as reply does.
fn answer(reply: impl Fn(&str) -> Result<Vec<u8>, String> + 'static) -> Calls {
    let calls = Calls::default();
    let record = calls.clone();
    fake::answer(move |method, request| {
        record
            .borrow_mut()
            .push((method.to_owned(), spell(request)));
        reply(method)
    });
    calls
}

/// last returns the latest call the mod made.
fn last(calls: &Calls) -> (String, String) {
    calls.borrow().last().cloned().expect("the mod made a call")
}

/// Every encoding testdata/generate.py wrote decodes, and encodes back to
/// the same bytes.
#[test]
fn messages_round_trip() {
    let mut checked = 0;
    for line in include_str!("../testdata/messages.txt").lines() {
        let (name, encoded) = line.split_once(' ').unwrap();
        let data = hex(encoded);
        let again = round_trip(name, &data)
            .unwrap_or_else(|| panic!("the library declares no {name}"))
            .unwrap_or_else(|error| panic!("{name} {encoded}: {error}"));
        assert_eq!(again, data, "{name} {encoded}");
        checked += 1;
    }
    assert!(
        checked > 100,
        "testdata/messages.txt holds {checked} encodings"
    );
}

/// A call names its method and encodes its receiver and arguments as the
/// Python library encodes the same request.
#[test]
fn calls_encode_their_arguments() {
    let calls = answer(|_| Ok(Vec::new()));

    assert!(Player::of(3).chat("hi"));
    assert_eq!(last(&calls), ("Chat".to_owned(), "080312026869".to_owned()));

    // Slot 0 is the zero the receiver assumes.
    assert!(Player::of(0).announce("T", "x"));
    assert_eq!(
        last(&calls),
        ("Announce".to_owned(), "1201541a0178".to_owned())
    );

    // A oneof travels as the case the argument holds.
    Player::of(2).select_hero(&SelectHero::HeroName("hero_inferno".to_owned()), 3);
    assert_eq!(last(&calls).1, "0802120c6865726f5f696e6665726e6f2003");
    Player::of(2).select_hero(&SelectHero::HeroId(7), 0);
    assert_eq!(last(&calls).1, "08021807");

    // An optional argument travels when it is set, even to its zero.
    Player::of(3).replace_ability(1, "x", Some(false));
    assert_eq!(last(&calls).1, "080310011a01782000");
    Player::of(3).replace_ability(1, "x", None);
    assert_eq!(last(&calls).1, "080310011a0178");

    // An options call sends its receiver, then the options.
    let options = BotOptions {
        name: "Bot".to_owned(),
        team: 2,
        ..Default::default()
    };
    add_bot(&options);
    assert_eq!(
        last(&calls),
        ("AddBot".to_owned(), "0a03426f741002".to_owned())
    );
    let mut encoded = Vec::new();
    wire::int32(&mut encoded, 1, 4);
    let ability = AbilityOptions {
        charges: Some(2),
        ..Default::default()
    };
    ability.encode(&mut encoded);
    Player::of(4).set_ability(&ability);
    assert_eq!(last(&calls), ("SetAbility".to_owned(), spell(&encoded)));

    log!("a {}", 1);
    assert_eq!(last(&calls), ("Log".to_owned(), "0a03612031".to_owned()));
}

/// A call returns its response's field, and what stands for a failure when
/// the host answers with an error.
#[test]
fn calls_read_their_results() {
    let pawn = Pawn {
        health: 550,
        max_health: 700,
        stamina: Some(0.0),
        ..Default::default()
    };
    let sent = pawn.clone();
    answer(move |method| {
        let mut out = Vec::new();
        match method {
            "SelectHero" => wire::uint32(&mut out, 1, 77),
            "AddBot" => wire::int32(&mut out, 1, 5),
            "ModifierState" => wire::bool(&mut out, 1, true),
            "Pawn" => wire::message(&mut out, 1, &sent),
            "Abilities" => {
                wire::message(
                    &mut out,
                    1,
                    &Ability {
                        name: "a".to_owned(),
                        ..Default::default()
                    },
                );
                wire::message(
                    &mut out,
                    1,
                    &Ability {
                        name: "b".to_owned(),
                        slot: 1,
                        ..Default::default()
                    },
                );
            }
            _ => {}
        }
        Ok(out)
    });
    assert_eq!(
        Player::of(1).select_hero(&SelectHero::HeroId(7), 2),
        Some(77)
    );
    assert_eq!(add_bot(&BotOptions::default()), Some(Player::of(5)));
    assert!(modifier_state(9, "silenced"));
    assert_eq!(Player::of(1).pawn(), Some(pawn));
    let abilities = Player::of(1).abilities();
    assert_eq!(
        abilities
            .iter()
            .map(|ability| ability.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(abilities[1].slot, 1);

    // An empty response holds each field's zero, and no message.
    answer(|_| Ok(Vec::new()));
    assert_eq!(
        Player::of(1).select_hero(&SelectHero::HeroId(7), 2),
        Some(0)
    );
    assert_eq!(Player::of(1).pawn(), None);
    assert!(Player::of(1).abilities().is_empty());
    assert!(!modifier_state(9, "silenced"));

    // A failed call returns what stands for a failure.
    answer(|_| Err("no such player".to_owned()));
    assert!(!Player::of(1).chat("hi"));
    assert_eq!(Player::of(1).select_hero(&SelectHero::HeroId(7), 2), None);
    assert_eq!(Player::of(1).pawn(), None);
    assert!(Player::of(1).abilities().is_empty());
    assert!(!modifier_state(9, "silenced"));
}

/// A pawn read keeps what its event read until the game may change.
#[test]
fn a_pawn_read_keeps_what_its_event_read_until_the_game_may_change() {
    use std::collections::HashMap;

    /// Driver runs the body the event test gave it.
    #[derive(Default)]
    struct Driver;

    impl Mod for Driver {
        fn frame(&mut self, _event: &FrameEvent) {
            BODY.with(|body| {
                if let Some(mut body) = body.borrow_mut().take() {
                    body();
                }
            });
        }
    }

    thread_local! {
        static BODY: RefCell<Option<Box<dyn FnMut()>>> = RefCell::new(None);
    }

    // The stand-in game holds each slot's hero health and answers Pawn and
    // Pawns from it; SetStamina stands for any call that may write.
    let health = Rc::new(RefCell::new(HashMap::from([(1, 100), (2, 200)])));
    let fails = Rc::new(RefCell::new(false));
    let methods = Rc::new(RefCell::new(Vec::new()));
    let (held, failed, sent) = (health.clone(), fails.clone(), methods.clone());
    crate::host::fake::answer(move |method, data| {
        sent.borrow_mut().push(method.to_owned());
        let mut out = Vec::new();
        let pawn = |slot: i32| -> Option<Pawn> {
            held.borrow().get(&slot).map(|health| Pawn {
                entity: 10 + slot as u32,
                health: *health,
                ..Pawn::default()
            })
        };
        match method {
            "Pawn" => {
                let mut slot = 0;
                let mut r = wire::Reader::new(data);
                while let Some((number, kind)) = r.field().unwrap() {
                    if number == 1 {
                        slot = r.int32(kind).unwrap();
                    } else {
                        r.skip(kind).unwrap();
                    }
                }
                if let Some(pawn) = pawn(slot) {
                    wire::message(&mut out, 1, &pawn);
                }
            }
            "Pawns" => {
                if *failed.borrow() {
                    return Err("busy".to_owned());
                }
                for slot in held.borrow().keys().copied() {
                    let pawn = pawn(slot).unwrap();
                    wire::nested(&mut out, 1, |out| {
                        wire::int32(out, 1, slot);
                        wire::message(out, 2, &pawn);
                    });
                }
            }
            _ => {}
        }
        Ok(out)
    });
    let events = events!(Driver::default);
    let event = |body: Box<dyn FnMut()>| {
        BODY.with(|held| *held.borrow_mut() = Some(body));
        send(Driver::default, &events, "Frame", &at(0.0));
    };
    let one = Player::of(1);
    let two = Player::of(2);
    let three = Player::of(3);
    methods.borrow_mut().clear();

    // Outside an event every read reaches the host.
    one.pawn();
    one.pawn();
    assert_eq!(take_methods(&methods), ["Pawn", "Pawn"]);

    let calls = methods.clone();
    let seen = health.clone();
    event(Box::new(move || {
        // The first slot takes its own call and is answered again after it.
        let first = one.pawn();
        assert_eq!(one.pawn(), first);
        assert_eq!(take_methods(&calls), ["Pawn"]);

        // A second slot reads every slot at once, and the batch answers the
        // rest, a slot with no pawn included.
        assert_eq!(two.pawn().map(|pawn| pawn.health), Some(200));
        assert!(three.pawn().is_none());
        assert_eq!(one.pawn(), first);
        assert_eq!(take_methods(&calls), ["Pawns"]);

        // A call that may write drops the reads; unchanged bytes keep their
        // Pawn, and changed ones decode anew.
        seen.borrow_mut().insert(2, 150);
        one.set_stamina(50.0);
        assert_eq!(one.pawn(), first);
        assert_eq!(two.pawn().map(|pawn| pawn.health), Some(150));
        assert_eq!(take_methods(&calls), ["SetStamina", "Pawn", "Pawns"]);
    }));

    // The next event reads again, since the game ran between them, and the
    // bytes changed, so it decodes a new Pawn.
    health.borrow_mut().insert(1, 90);
    let calls = methods.clone();
    event(Box::new(move || {
        assert_eq!(one.pawn().map(|pawn| pawn.health), Some(90));
        assert_eq!(take_methods(&calls), ["Pawn"]);
    }));

    // A failed Pawns leaves the rest of its event to single reads, and the
    // next event batches again.
    *fails.borrow_mut() = true;
    let calls = methods.clone();
    event(Box::new(move || {
        one.pawn();
        two.pawn();
        one.set_stamina(50.0);
        one.pawn();
        two.pawn();
        assert_eq!(
            take_methods(&calls),
            ["Pawn", "Pawns", "Pawn", "SetStamina", "Pawn", "Pawn"]
        );
    }));
    *fails.borrow_mut() = false;
    let calls = methods.clone();
    event(Box::new(move || {
        one.pawn();
        two.pawn();
        assert_eq!(take_methods(&calls), ["Pawn", "Pawns"]);
    }));

    // With the cache off, every read in an event reaches the host.
    crate::set_pawn_cache(false);
    let calls = methods.clone();
    event(Box::new(move || {
        one.pawn();
        one.pawn();
        assert_eq!(take_methods(&calls), ["Pawn", "Pawn"]);
    }));
    crate::set_pawn_cache(true);
}

/// take_methods drains the methods the mod called since the last drain.
fn take_methods(calls: &Rc<RefCell<Vec<String>>>) -> Vec<String> {
    calls.borrow_mut().drain(..).collect()
}

/// Heard is a mod that keeps the chat lines it heard and blocks every hit.
struct Heard {
    lines: Rc<RefCell<Vec<String>>>,
}

impl Mod for Heard {
    fn start(&mut self, event: &StartEvent) {
        self.lines.borrow_mut().push(event.args.join(" "));
        command("say", Self::say);
        command("ping", Self::ping);
    }
}

impl OnChat for Heard {
    fn chat(&mut self, event: &ChatEvent) {
        self.lines
            .borrow_mut()
            .push(format!("{}: {}", event.player, event.text));
    }
}

impl OnDamage for Heard {
    fn damage(&mut self, event: &DamageEvent) -> DamageResult {
        DamageResult {
            block: Some(event.amount > 10.0),
            ..Default::default()
        }
    }
}

impl Heard {
    fn say(&mut self, player: Player, words: &str) {
        self.lines
            .borrow_mut()
            .push(format!("{player} says {words}"));
    }

    fn ping(&mut self, player: Player) {
        self.lines.borrow_mut().push(format!("{player} pings"));
        every(1.0, |heard: &mut Heard| {
            heard.lines.borrow_mut().push("tick".to_owned())
        });
        after(2.5, |heard: &mut Heard| {
            heard.lines.borrow_mut().push("late".to_owned())
        });
    }
}

/// deliver delivers one event to the mod of this thread and returns the
/// encoded Reply.
fn deliver(lines: &Rc<RefCell<Vec<String>>>, method: &str, event: &impl Message) -> String {
    let mut request = Vec::new();
    event.encode(&mut request);
    let mut call = Vec::new();
    wire::string(&mut call, 1, method);
    wire::bytes(&mut call, 2, &request);
    let mut reply = Vec::new();
    let make = || Heard {
        lines: lines.clone(),
    };
    let events = events!(make);
    host::serve(&call, &mut reply, make, &events);
    spell(&reply)
}

/// An event runs the mod's method with its decoded request and answers with
/// the method's encoded response; the mod lives from one event to the next.
#[test]
fn events_reach_the_mod() {
    let lines = Rc::new(RefCell::new(Vec::new()));

    let start = StartEvent {
        args: vec!["one".to_owned(), "two".to_owned()],
        ..Default::default()
    };
    assert_eq!(deliver(&lines, "Start", &start), "12080801100120012801");

    let chat = ChatEvent {
        player: Player::of(3),
        text: "hi".to_owned(),
        ..Default::default()
    };
    assert_eq!(deliver(&lines, "Chat", &chat), "");
    assert_eq!(
        deliver(
            &lines,
            "Chat",
            &ChatEvent {
                text: "again".to_owned(),
                ..chat
            }
        ),
        ""
    );

    // A chat line arrives under the same CPU budget as a command, so its
    // handler waits for the next frame.
    assert_eq!(*lines.borrow(), ["one two"]);
    let frame = FrameEvent {
        time_seconds: 1.0,
        ..Default::default()
    };
    assert_eq!(deliver(&lines, "Frame", &frame), "");
    assert_eq!(
        *lines.borrow(),
        ["one two", "player 3: hi", "player 3: again"]
    );

    assert_eq!(
        deliver(
            &lines,
            "Damage",
            &DamageEvent {
                amount: 25.0,
                ..Default::default()
            }
        ),
        "12020801"
    );
    assert_eq!(
        deliver(
            &lines,
            "Damage",
            &DamageEvent {
                amount: 5.0,
                ..Default::default()
            }
        ),
        ""
    );

    // An event the mod does not implement does nothing and answers nothing.
    assert_eq!(deliver(&lines, "Killed", &KillEvent::default()), "");

    // A method the library does not know answers with an error.
    let mut error = Vec::new();
    wire::string(&mut error, 1, "the mod has no method Nope");
    assert_eq!(
        deliver(&lines, "Nope", &KillEvent::default()),
        spell(&error)
    );
}

/// frame delivers the frame at seconds.
fn frame(lines: &Rc<RefCell<Vec<String>>>, seconds: f64) {
    let event = FrameEvent {
        time_seconds: seconds,
        ..Default::default()
    };
    assert_eq!(deliver(lines, "Frame", &event), "");
}

/// A command with a handler is claimed and runs with the next frame; any
/// other is the mod's to answer. Timers run from frames, on the mod.
#[test]
fn commands_and_timers_run_on_the_mod() {
    let lines = Rc::new(RefCell::new(Vec::new()));
    deliver(&lines, "Start", &StartEvent::default());
    lines.borrow_mut().clear();

    // The handler waits for a frame, and takes the trimmed arguments.
    let say = CommandEvent {
        player: Player::of(2),
        line: " say  hello there ".to_owned(),
    };
    assert_eq!(deliver(&lines, "Command", &say), "12020801");
    assert!(lines.borrow().is_empty());
    frame(&lines, 10.0);
    assert_eq!(*lines.borrow(), ["player 2 says hello there"]);

    // A command without a handler is not claimed.
    let other = CommandEvent {
        player: Player::of(2),
        line: "other".to_owned(),
    };
    assert_eq!(deliver(&lines, "Command", &other), "");

    // ping runs with the frame at 10.5, where it starts a timer every second
    // and one after two and a half. Each line a timer adds comes before the
    // mark of the frame that ran it.
    lines.borrow_mut().clear();
    let ping = CommandEvent {
        player: Player::of(2),
        line: "ping".to_owned(),
    };
    deliver(&lines, "Command", &ping);
    for seconds in [10.5, 11.0, 11.6, 12.4, 12.6, 13.1, 13.7] {
        frame(&lines, seconds);
        lines.borrow_mut().push(format!("@{seconds}"));
    }
    let expected = [
        "player 2 pings",
        "@10.5",
        "@11",
        "tick",
        "@11.6",
        "@12.4",
        "tick",
        "@12.6",
        "late",
        "@13.1",
        "tick",
        "@13.7",
    ];
    assert_eq!(*lines.borrow(), expected);
}

/// Command names ignore case while arguments keep their case, and a
/// different spelling replaces the same command registration.
#[test]
fn command_names_ignore_case() {
    #[derive(Default)]
    struct CaseMod {
        seen: Vec<String>,
    }
    impl Mod for CaseMod {
        fn start(&mut self, _event: &StartEvent) {
            command(
                "CaseFold",
                |m: &mut CaseMod, _player: Player, args: &str| {
                    m.seen.push(args.to_owned());
                },
            );
        }
        fn frame(&mut self, _event: &FrameEvent) {
            SEEN.with(|seen| seen.borrow_mut().clone_from(&self.seen));
        }
    }
    thread_local! {
        static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    let events = events!(CaseMod::default);
    send(CaseMod::default, &events, "Start", &StartEvent::default());
    let run = |line: &str| {
        let cmd = CommandEvent {
            player: Player::of(1),
            line: line.to_owned(),
        };
        send(CaseMod::default, &events, "Command", &cmd)
    };
    assert_eq!(run("casefold First"), "12020801");
    assert_eq!(run("CASEFOLD Second"), "12020801");
    assert_eq!(run("CaseFold ThIrD"), "12020801");
    send(CaseMod::default, &events, "Frame", &at(30.0));
    assert_eq!(
        SEEN.with(|seen| seen.borrow().clone()),
        ["First", "Second", "ThIrD"]
    );

    // A different spelling replaces the same command registration.
    command(
        "casefold",
        |m: &mut CaseMod, _player: Player, args: &str| {
            m.seen.push(format!("replacement {args}"));
        },
    );
    assert_eq!(run("CASEFOLD MiXeD"), "12020801");
    send(CaseMod::default, &events, "Frame", &at(30.016));
    assert_eq!(
        SEEN.with(|seen| seen.borrow().last().cloned()),
        Some("replacement MiXeD".to_owned())
    );
}

/// players answers from the list the game last reported, and a Player is
/// equal only to a Player of the same connection.
#[test]
fn players_follow_the_game() {
    let lines = Rc::new(RefCell::new(Vec::new()));

    // Before a report, the library asks the game.
    answer(|method| {
        let mut out = Vec::new();
        if method == "Players" {
            let asked = Connection {
                player: Player::of(1),
                name: "asked".to_owned(),
                ..Default::default()
            };
            wire::message(&mut out, 1, &asked);
        }
        Ok(out)
    });
    assert_eq!(
        players()
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["asked"]
    );

    let connection = |slot, generation, name: &str| Connection {
        player: Player::of(slot),
        generation,
        name: name.to_owned(),
        ..Default::default()
    };
    let report = |players: Vec<Connection>| PlayersChangedEvent { players };
    deliver(
        &lines,
        "PlayersChanged",
        &report(vec![connection(1, 7, "Ann"), connection(2, 8, "Bob")]),
    );
    assert_eq!(
        players()
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["Ann", "Bob"]
    );
    let (ann, bob) = (Player::of(1), Player::of(2));
    assert_eq!(bob.connection().map(|c| c.name), Some("Bob".to_owned()));
    assert_eq!(players()[1].player, bob);

    // Cid takes the slot Bob left: the slot is the same, the Player is not.
    deliver(
        &lines,
        "PlayersChanged",
        &report(vec![connection(1, 7, "Ann"), connection(2, 9, "Cid")]),
    );
    assert_eq!(Player::of(1), ann);
    assert_ne!(Player::of(2), bob);
    assert_eq!(Player::of(2).slot(), bob.slot());
    assert_eq!(players()[1].player, Player::of(2));
    assert_eq!(Player::default().connection(), None);
}

/// Quiet is a mod that handles no event the game sends on request, and
/// Joined one that handles a player joining.
struct Quiet;

impl Mod for Quiet {}

struct Joined;

impl Mod for Joined {}

impl OnJoin for Joined {
    fn player_joined(&mut self, event: &PlayerJoinedEvent) {
        event.connection.player.chat("Welcome!");
    }
}

/// wants returns what a mod asks the game for at its start.
fn wants<M: Mod + 'static>(make: impl FnOnce() -> M, events: &Events<M>) -> StartResult {
    let mut call = Vec::new();
    wire::string(&mut call, 1, "Start");
    let mut reply = Vec::new();
    host::serve(&call, &mut reply, make, events);
    let mut payload = wire::Reader::new(&reply);
    let (_, kind) = payload.field().unwrap().unwrap();
    StartResult::decode(payload.bytes(kind).unwrap()).unwrap()
}

/// A mod that implements none of the traits asks the game for frames, which
/// its timers run from, and for joins, which the library follows players by.
#[test]
fn a_mod_without_event_traits_asks_for_frames_and_joins() {
    let wanted = StartResult {
        frames: true,
        joins: true,
        ..Default::default()
    };
    assert_eq!(wants(|| Quiet, &events!(|| Quiet)), wanted);
}

/// A mod asks the game for the events whose traits it implements, and those
/// events reach it.
#[test]
fn a_mod_asks_for_the_events_it_implements() {
    // Joined implements one method of OnJoin; the other does nothing.
    let make = || Joined;
    let events = events!(make);
    assert!(events.player_joined.is_some() && events.player_left.is_some());
    assert!(events.killed.is_none() && events.damage.is_none());
    let joins = StartResult {
        frames: true,
        joins: true,
        ..Default::default()
    };
    assert_eq!(wants(make, &events), joins);

    // The event reaches the mod.
    let calls = answer(|_| Ok(Vec::new()));
    let joined = PlayerJoinedEvent {
        connection: Connection {
            player: Player::of(4),
            ..Default::default()
        },
    };
    let mut request = Vec::new();
    joined.encode(&mut request);
    let mut call = Vec::new();
    wire::string(&mut call, 1, "PlayerJoined");
    wire::bytes(&mut call, 2, &request);
    let mut reply = Vec::new();
    host::serve(&call, &mut reply, make, &events);
    assert_eq!(last(&calls).0, "Chat");

    // An event the mod has no trait for does nothing and answers nothing.
    let mut call = Vec::new();
    wire::string(&mut call, 1, "Killed");
    host::serve(&call, &mut reply, make, &events);
    assert!(reply.is_empty());
}

/// Lines holds what a mod under test wrote down.
type Lines = Rc<RefCell<Vec<String>>>;

/// send delivers one event to the mod of this thread, which make makes on the
/// first, and returns the encoded Reply in hexadecimal.
pub(crate) fn send<M: Mod + 'static>(
    make: impl FnOnce() -> M,
    events: &Events<M>,
    method: &str,
    event: &impl Message,
) -> String {
    let mut request = Vec::new();
    event.encode(&mut request);
    let mut call = Vec::new();
    wire::string(&mut call, 1, method);
    wire::bytes(&mut call, 2, &request);
    let mut reply = Vec::new();
    host::serve(&call, &mut reply, make, events);
    spell(&reply)
}

/// at returns the frame at seconds.
pub(crate) fn at(seconds: f64) -> FrameEvent {
    FrameEvent {
        time_seconds: seconds,
        ..Default::default()
    }
}

/// A message holds a player the game may leave out as an Option: None for
/// the slot -1, and the player of slot 0 for a slot the sender left out.
#[test]
fn a_missing_player_is_none() {
    let mut data = Vec::new();
    wire::int32(&mut data, 2, -1);
    wire::int32(&mut data, 4, 5);
    let kill = KillEvent::decode(&data).unwrap();
    assert_eq!(kill.victim_player, None);
    assert_eq!(kill.attacker_player, Some(Player::of(5)));
    let mut again = Vec::new();
    kill.encode(&mut again);
    assert_eq!(again, data);

    let kill = KillEvent::decode(&[]).unwrap();
    assert_eq!(kill.victim_player, Some(Player::of(0)));
    let mut again = Vec::new();
    kill.encode(&mut again);
    assert!(again.is_empty());
}

/// Watch is a mod that watches a modifier event and heroes entering play.
struct Watch {
    lines: Lines,
}

impl Mod for Watch {
    fn start(&mut self, _event: &StartEvent) {
        modifier_event("PARRY_SUCCESS", Self::parried);
        sequence(Self::count);
    }
}

impl OnSpawn for Watch {
    fn spawned(&mut self, player: Player, pawn: &Pawn) {
        self.lines
            .borrow_mut()
            .push(format!("{player} spawned as {}", pawn.hero));
    }
}

impl Watch {
    fn parried(&mut self, event: &ModifierFiredEvent) {
        let slot = event.player.map(Player::slot);
        self.lines.borrow_mut().push(format!("parry by {slot:?}"));
    }

    /// count runs three times, a second apart.
    fn count(&mut self, run: u32) -> Option<f64> {
        self.lines.borrow_mut().push(format!("run {run}"));
        (run < 3).then_some(1.0)
    }
}

/// A mod asks the game for the modifier events it registered, and for the
/// respawn event when it handles spawns; each reaches its handler.
#[test]
fn modifier_events_and_spawns_reach_the_mod() {
    let lines = Lines::default();
    let held = lines.clone();
    let make = move || Watch { lines: held };
    let events = events!(make);

    // The mod asks for both names, in order.
    let mut call = Vec::new();
    wire::string(&mut call, 1, "Start");
    let mut reply = Vec::new();
    host::serve(&call, &mut reply, make.clone(), &events);
    let mut payload = wire::Reader::new(&reply);
    let (_, kind) = payload.field().unwrap().unwrap();
    let wants = StartResult::decode(payload.bytes(kind).unwrap()).unwrap();
    assert_eq!(wants.modifier_events, ["PARRY_SUCCESS", "PLAYER_RESPAWNED"]);

    // A hero entering play is told once a frame, however often the game
    // repeats the event, and never for nobody or a placeholder pawn.
    let hero = Rc::new(RefCell::new(13));
    let shown = hero.clone();
    answer(move |method| {
        let mut out = Vec::new();
        if method == "Pawn" {
            let pawn = Pawn {
                hero: *shown.borrow(),
                health: 500,
                ..Default::default()
            };
            wire::message(&mut out, 1, &pawn);
        }
        Ok(out)
    });
    let fired = |name: &str, player| ModifierFiredEvent {
        name: name.to_owned(),
        player,
        ..Default::default()
    };
    let respawned = fired("PLAYER_RESPAWNED", Some(Player::of(2)));
    send(make.clone(), &events, "Frame", &at(1.0));
    send(make.clone(), &events, "ModifierFired", &respawned);
    send(make.clone(), &events, "ModifierFired", &respawned);
    send(
        make.clone(),
        &events,
        "ModifierFired",
        &fired("PLAYER_RESPAWNED", None),
    );
    send(make.clone(), &events, "Frame", &at(2.0));
    send(make.clone(), &events, "ModifierFired", &respawned);
    *hero.borrow_mut() = 0;
    send(make.clone(), &events, "Frame", &at(3.0));
    send(make.clone(), &events, "ModifierFired", &respawned);

    // The registered name runs its handler, with nobody as None.
    send(
        make.clone(),
        &events,
        "ModifierFired",
        &fired("PARRY_SUCCESS", None),
    );
    send(make.clone(), &events, "Frame", &at(9.0));
    assert_eq!(
        *lines.borrow(),
        [
            "run 1",
            "player 2 spawned as 13",
            "run 2",
            "player 2 spawned as 13",
            "run 3",
            "parry by None",
        ]
    );
}

/// Desk is a mod that serves one service and calls another.
struct Desk {
    lines: Lines,
}

impl Mod for Desk {
    fn start(&mut self, _event: &StartEvent) {
        serve("echo", Self::echo);
    }
}

impl Desk {
    fn echo(&mut self, method: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        match method {
            "back" => Ok(payload.iter().rev().copied().collect()),
            _ => Err(format!("echo has no method {method}")),
        }
    }

    fn done(&mut self, result: Result<Vec<u8>, ServiceError>) {
        self.lines.borrow_mut().push(match result {
            Ok(payload) => format!("answered {payload:?}"),
            Err(error) => format!("{:?}: {error}", error.failure),
        });
    }
}

/// A mod answers the calls to the service it serves, and hears the answer of
/// a call it started in a later event.
#[test]
fn services_run_on_the_mod() {
    let lines = Lines::default();
    let held = lines.clone();
    let make = move || Desk { lines: held };
    let events = events!(make);
    send(make.clone(), &events, "Start", &StartEvent::default());

    // The handler answers, refuses, or is not there.
    let call = |service: &str, method: &str| ServiceCall {
        service: service.to_owned(),
        method: method.to_owned(),
        payload: vec![1, 2, 3],
    };
    let mut answered = Vec::new();
    let mut body = Vec::new();
    ServiceReply {
        payload: vec![3, 2, 1],
    }
    .encode(&mut body);
    wire::bytes(&mut answered, 2, &body);
    assert_eq!(
        send(make.clone(), &events, "Serve", &call("echo", "back")),
        spell(&answered)
    );
    let refused = |why: &str| {
        let mut reply = Vec::new();
        wire::string(&mut reply, 1, why);
        spell(&reply)
    };
    assert_eq!(
        send(make.clone(), &events, "Serve", &call("echo", "nope")),
        refused("echo has no method nope")
    );
    assert_eq!(
        send(make.clone(), &events, "Serve", &call("other", "back")),
        refused("the mod serves no service other")
    );

    // A started call ends once, with the reply that names its number.
    let calls = answer(|method| {
        let mut out = Vec::new();
        if method == "StartServiceCall" {
            wire::uint64(&mut out, 1, 5);
        }
        Ok(out)
    });
    call_service_later("scores", "Read", &[9], Duration::from_secs(2), Desk::done);
    assert_eq!(last(&calls).0, "StartServiceCall");
    let replied = ServiceRepliedEvent {
        call: 5,
        payload: vec![7],
        ..Default::default()
    };
    send(make.clone(), &events, "ServiceReplied", &replied);
    send(make.clone(), &events, "ServiceReplied", &replied);
    assert_eq!(*lines.borrow(), ["answered [7]"]);

    // A failed call ends with the failure.
    call_service_later("scores", "Read", &[9], Duration::ZERO, Desk::done);
    let failed = ServiceRepliedEvent {
        call: 5,
        failure: ServiceFailure::Timeout,
        error: "too slow".to_owned(),
        ..Default::default()
    };
    send(make.clone(), &events, "ServiceReplied", &failed);
    assert_eq!(lines.borrow()[1], "Timeout: too slow");

    // A call the host does not start ends with the next frame.
    answer(|_| Err("no such service".to_owned()));
    call_service_later("scores", "Read", &[9], Duration::ZERO, Desk::done);
    assert_eq!(lines.borrow().len(), 2);
    send(make.clone(), &events, "Frame", &at(1.0));
    assert_eq!(
        lines.borrow()[2],
        "Unavailable: the host did not start the call to scores"
    );
}

/// A hero is found by its game name, that name without its prefix, or its
/// title however it is typed; a call that takes a hero sends the game's name.
#[test]
fn heroes_are_found_by_name() {
    assert_eq!(
        hero_named("Calico").map(|hero| hero.name),
        Some("hero_nano")
    );
    assert_eq!(hero_named("lady geist").map(|hero| hero.id), Some(4));
    assert_eq!(hero_named("hero_haze").map(|hero| hero.title), Some("Haze"));
    assert_eq!(hero_named("haze").map(|hero| hero.title), Some("Haze"));
    assert_eq!(hero_named("geist"), None);
    assert_eq!(
        find_hero("geist").map(|hero| hero.title),
        Some("Lady Geist")
    );
    assert_eq!(find_hero("mo").map(|hero| hero.title), Some("Mo & Krill"));
    assert_eq!(find_hero("!!"), None);

    let calls = answer(|_| Ok(Vec::new()));
    Player::of(2).select_hero(&SelectHero::HeroName("Calico".to_owned()), 3);
    assert_eq!(last(&calls).1, "080212096865726f5f6e616e6f2003");
}

/// Seats is a mod that seats each player who joins on Haze, on team 2.
struct Seats;

impl Mod for Seats {}

impl OnJoin for Seats {
    fn player_joined(&mut self, event: &PlayerJoinedEvent) {
        let seat = Seating {
            hero: "haze".into(),
            team: Some(2),
        };
        assert!(event.connection.player.seat(seat));
    }
}

/// A seated player is asked onto their hero until they have it, and a fallen
/// hero is asked back after the respawn delay.
#[test]
fn seating_asks_until_the_hero_shows() {
    let make = || Seats;
    let events = events!(make);
    send(make, &events, "Start", &StartEvent::default());

    // The game shows a pawn without a hero, then with one.
    let pawn = Rc::new(RefCell::new(Pawn {
        team: 2,
        ..Default::default()
    }));
    let shown = pawn.clone();
    let calls = answer(move |method| {
        let mut out = Vec::new();
        if method == "Pawn" {
            wire::message(&mut out, 1, &*shown.borrow());
        }
        Ok(out)
    });
    let count = |method: &str| {
        calls
            .borrow()
            .iter()
            .filter(|call| call.0 == method)
            .count()
    };
    let connection = Connection {
        player: Player::of(1),
        generation: 7,
        ready: true,
        ..Default::default()
    };
    let report = PlayersChangedEvent {
        players: vec![connection.clone()],
    };
    send(make, &events, "PlayersChanged", &report);
    send(
        make,
        &events,
        "PlayerJoined",
        &PlayerJoinedEvent { connection },
    );

    // The first frame asks, by the game's name; the next waits a second.
    send(make, &events, "Frame", &at(10.0));
    let asked = calls
        .borrow()
        .iter()
        .find(|call| call.0 == "SelectHero")
        .cloned();
    assert_eq!(
        asked.map(|call| call.1),
        Some("080112096865726f5f68617a652002".to_owned())
    );
    send(make, &events, "Frame", &at(10.5));
    assert_eq!(count("SelectHero"), 1);
    send(make, &events, "Frame", &at(11.0));
    assert_eq!(count("SelectHero"), 2);

    // Once the hero shows, nothing more is asked.
    pawn.borrow_mut().hero = 13;
    pawn.borrow_mut().health = 500;
    send(make, &events, "Frame", &at(12.5));
    send(make, &events, "Frame", &at(14.0));
    assert_eq!(count("SelectHero"), 2);

    // A fallen hero comes back after the delay, and not before.
    set_respawn_delay(Some(2.0));
    pawn.borrow_mut().health = 0;
    send(make, &events, "Frame", &at(15.0));
    send(make, &events, "Frame", &at(16.5));
    assert_eq!(count("Respawn"), 0);
    send(make, &events, "Frame", &at(17.0));
    assert_eq!(count("Respawn"), 1);
}

/// A setting reads as the switch or the number the manifest declares.
#[test]
fn settings_read_as_their_kind() {
    answer(|method| {
        let mut out = Vec::new();
        if method == "PlayerSetting" {
            wire::string(&mut out, 1, "true");
        }
        Ok(out)
    });
    assert_eq!(Player::of(1).setting_on("hints"), Some(true));
    assert_eq!(Player::of(1).setting_number("hints"), None);
    answer(|method| {
        let mut out = Vec::new();
        if method == "PlayerSetting" {
            wire::string(&mut out, 1, "0.5");
        }
        Ok(out)
    });
    assert_eq!(Player::of(1).setting_number("volume"), Some(0.5));
    assert_eq!(Player::of(1).setting_on("volume"), Some(false));
}

/// request returns what a call with these members encodes.
fn request(write: impl FnOnce(&mut Vec<u8>)) -> String {
    let mut out = Vec::new();
    write(&mut out);
    spell(&out)
}

/// value answers a field read with value.
fn value(value: FieldValue) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    wire::nested(&mut out, 1, |out| value.encode(out));
    Ok(out)
}

/// A class reads and writes its fields, and its bases', by the name of the
/// class that declares each.
#[test]
fn entity_fields_read_and_write() {
    use crate::entity::{CBaseEntity, CCitadelPlayerPawn, CEntityInstance, Class, INVALID_HANDLE};

    let hero = CCitadelPlayerPawn::of(77);
    let field = |class: &str, name: &str, kind: FieldType| {
        request(|out| {
            wire::uint32(out, 1, 77);
            wire::string(out, 2, class);
            wire::string(out, 3, name);
            wire::int32(out, 4, kind as i32);
        })
    };

    // A base's field is read from the base.
    let calls = answer(|_| value(FieldValue::Number(550.0)));
    assert_eq!(hero.m_iHealth().get(), Some(550));
    assert_eq!(
        last(&calls),
        (
            "ReadField".to_owned(),
            field("CBaseEntity", "m_iHealth", FieldType::Int32)
        )
    );

    // The class's own field is read from the class.
    answer(|_| {
        value(FieldValue::Vector(Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        }))
    });
    let calls = answer(|_| {
        value(FieldValue::Vector(Vector {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        }))
    });
    let angles = Angles {
        pitch: 1.0,
        yaw: 2.0,
        roll: 3.0,
    };
    assert_eq!(hero.m_angEyeAngles().get(), Some(angles));
    assert_eq!(
        last(&calls).1,
        field("CCitadelPlayerPawn", "m_angEyeAngles", FieldType::Vector)
    );

    // A write sends the value as the host takes it.
    let calls = answer(|_| Ok(Vec::new()));
    assert!(hero.m_iHealth().set(600));
    let mut written = hex(&field("CBaseEntity", "m_iHealth", FieldType::Int32));
    wire::nested(&mut written, 5, |out| FieldValue::Number(600.0).encode(out));
    assert_eq!(last(&calls), ("WriteField".to_owned(), spell(&written)));

    // A handle names an entity of its class, or none.
    answer(|_| value(FieldValue::Number(9.0)));
    assert_eq!(hero.m_hOwnerEntity().get(), Some(CBaseEntity::of(9)));
    answer(|_| value(FieldValue::Number(INVALID_HANDLE as f64)));
    assert_eq!(hero.m_hOwnerEntity().get(), None);

    // A failed read is None, and a class casts to any other.
    answer(|_| Err("no such entity".to_owned()));
    assert_eq!(hero.m_iHealth().get(), None);
    let base: CBaseEntity = hero.cast();
    assert_eq!(base.handle(), 77);
    assert_eq!(
        hero.cast::<CEntityInstance>(),
        CEntityInstance { handle: 77 }
    );
    assert_eq!(format!("{hero:?}"), "CCitadelPlayerPawn(0x4d)");

    // A class reads its own fields under its own name.
    let calls = answer(|_| value(FieldValue::Number(2.0)));
    let boss = crate::entity::CNPC_TrooperBoss::of(77);
    assert_eq!(boss.m_iLane().get(), Some(2));
    assert_eq!(
        last(&calls).1,
        field("CNPC_TrooperBoss", "m_iLane", FieldType::Int32)
    );
    assert_eq!(crate::entity::CNPC_TrooperBoss::NAME, "CNPC_TrooperBoss");
    assert_eq!(format!("{boss:?}"), "CNPC_TrooperBoss(0x4d)");
}

/// An input travels by its name with its value, and a designer creates its
/// entity with its subclass and the key values its struct sets.
#[test]
fn entities_take_inputs_and_are_created() {
    use crate::entity::{self, CBaseEntity, Class};

    let calls = answer(|method| {
        let mut out = Vec::new();
        match method {
            "FireInput" => wire::bool(&mut out, 1, true),
            "CreateEntity" => wire::uint32(&mut out, 1, 31),
            _ => {}
        }
        Ok(out)
    });
    let door = CBaseEntity::of(5);
    assert!(door.input_Kill());
    let killed = request(|out| {
        wire::uint32(out, 1, 5);
        wire::string(out, 2, "Kill");
    });
    assert_eq!(last(&calls), ("FireInput".to_owned(), killed));
    assert!(door.input_SetTeam(3));
    let teamed = request(|out| {
        wire::uint32(out, 1, 5);
        wire::string(out, 2, "SetTeam");
        wire::nested(out, 3, |out| EntityValue::Integer(3).encode(out));
    });
    assert_eq!(last(&calls).1, teamed);

    // The key values of a base come first, then the struct's own.
    let options = EntityOptions {
        team: 2,
        ..Default::default()
    };
    let boss = entity::npc_trooper_boss().create_with(&options, |keys| {
        keys.LaneNum = Some(2);
        keys.globalname = Some("boss".to_owned());
    });
    assert_eq!(boss.map(Class::handle), Some(31));
    let expected = EntityOptions {
        designer_name: "npc_trooper_boss".to_owned(),
        subclass: "npc_boss_tier1".to_owned(),
        team: 2,
        key_values: vec![
            KeyValue {
                key: "globalname".to_owned(),
                value: Some(EntityValue::Text("boss".to_owned())),
            },
            KeyValue {
                key: "LaneNum".to_owned(),
                value: Some(EntityValue::Integer(2)),
            },
        ],
        ..Default::default()
    };
    assert_eq!(
        last(&calls),
        (
            "CreateEntity".to_owned(),
            request(|out| expected.encode(out))
        )
    );
}

/// A console variable is set, and a command run, as one line at the server
/// console; apply names the settings the host refused.
#[test]
fn the_console_takes_lines() {
    use crate::console;

    let line = |text: &str| request(|out| wire::string(out, 1, text));
    let calls = answer(|_| Ok(Vec::new()));
    assert!(console::sv_cheats().set(true));
    assert_eq!(
        last(&calls),
        ("ServerCommand".to_owned(), line("sv_cheats 1"))
    );
    assert!(console::hostname().set("My server"));
    assert_eq!(last(&calls).1, line("hostname \"My server\""));
    assert!(console::changelevel().run(&["street_test"]));
    assert_eq!(last(&calls).1, line("changelevel street_test"));

    let mut settings = console::arena();
    settings.push(console::citadel_allow_duplicate_heroes().to(true));
    assert!(console::apply(&settings).is_empty());
    assert_eq!(last(&calls).1, line("citadel_allow_duplicate_heroes 1"));
    assert_eq!(calls.borrow().len(), 3 + settings.len());

    answer(|_| Err("refused".to_owned()));
    assert_eq!(
        console::apply(&settings[..2]),
        ["sv_hibernate_when_empty", "citadel_solo_bot_match"]
    );
}

/// An item is found by its class name, and its id is the hash the game makes
/// of that name.
#[test]
fn items_are_found_by_name() {
    let shard = item("upgrade_ability_power_shard").expect("the shop has the item");
    assert_eq!(
        (shard.slot, shard.tier, shard.active),
        (Slot::Spirit, 4, true)
    );
    assert_eq!(item("upgrade_nothing"), None);
    assert!(ITEMS.len() > 100);
    for item in &ITEMS {
        assert_eq!(item_id(item.name), item.id, "{}", item.name);
    }
}

/// Menu is a mod whose interface the tests draw.
struct Menu {
    lines: Lines,
}

impl Mod for Menu {}

impl Menu {
    fn ready(&mut self, player: Player) {
        self.lines.borrow_mut().push(format!("{player} ready"));
    }

    fn typed(&mut self, player: Player, text: &str) {
        self.lines
            .borrow_mut()
            .push(format!("{player} typed {text}"));
    }

    fn bar(&mut self, player: Player, slot: usize) {
        self.lines.borrow_mut().push(format!("{player} bar {slot}"));
    }
}

/// drawn returns the changes the mod sent since the last call, each with the
/// slot of its player.
fn drawn(calls: &Calls) -> Vec<(i32, Change)> {
    let sent = calls
        .borrow_mut()
        .drain(..)
        .filter(|(method, _)| method == "Ui")
        .map(|(_, request)| {
            let (mut slot, mut change) = (0, Change::default());
            let data = hex(&request);
            let mut r = wire::Reader::new(&data);
            while let Some((number, kind)) = r.field().unwrap() {
                match number {
                    1 => slot = r.int32(kind).unwrap(),
                    2 => change = Change::decode(r.bytes(kind).unwrap()).unwrap(),
                    _ => r.skip(kind).unwrap(),
                }
            }
            (slot, change)
        })
        .collect();
    sent
}

/// texts returns the text of each node of change that has one.
fn texts(change: &Change) -> Vec<&str> {
    change
        .set
        .iter()
        .filter(|node| !node.text.is_empty())
        .map(|node| node.text.as_str())
        .collect()
}

/// A first show sends the whole tree, and each later one only the nodes that
/// changed, under the ids their places had.
#[test]
fn a_show_sends_what_changed() {
    use crate::ui::*;

    let calls = answer(|_| Ok(Vec::new()));
    let player = Player::of(3);
    let screen = |round: u32, names: &[&str]| {
        let rows = names.iter().map(|name| text(*name).key(name));
        panel()
            .style(Style {
                flow: Flow::Down,
                padding: Some(16.into()),
                background: rgba(0x0000_00cc),
                ..Default::default()
            })
            .child(text(format!("Round {round}")))
            .child(rows)
            .child(button().on_press(Menu::ready).child(text("Ready")))
    };

    // The first screen resets the player's tree and holds every node,
    // children before parents and the root last.
    assert!(show(player, screen(1, &["Ann", "Bob"])));
    let sent = drawn(&calls);
    let (slot, first) = &sent[0];
    assert_eq!((*slot, sent.len(), first.reset), (3, 1, true));
    assert_eq!(texts(first), ["Round 1", "Ann", "Bob", "Ready"]);
    let root = first.set.last().unwrap();
    assert_eq!((root.id.as_str(), root.children.len()), ("", 1));
    let held = first
        .set
        .iter()
        .find(|node| node.kind == Kind::Panel)
        .unwrap();
    assert_eq!(held.children.len(), 4);
    assert_eq!(
        held.style.padding,
        Some(Edges {
            top: 16.0,
            right: 16.0,
            bottom: 16.0,
            left: 16.0
        })
    );

    // A style a mod left alone does not travel, so no node takes the pointer
    // for a hover style it does not have.
    let mut encoded = Vec::new();
    first.set[0].encode(&mut encoded);
    let label = Node::decode(&encoded).unwrap();
    assert_eq!(label.hover, None);
    assert_eq!(
        spell(&encoded),
        "0a01311001220752".to_owned() + "6f756e642031"
    );

    // The same screen sends nothing.
    assert!(show(player, screen(1, &["Ann", "Bob"])));
    assert!(drawn(&calls).is_empty());

    // A changed label sends itself alone.
    assert!(show(player, screen(2, &["Ann", "Bob"])));
    let sent = drawn(&calls);
    assert_eq!(
        (sent[0].1.reset, texts(&sent[0].1)),
        (false, vec!["Round 2"])
    );
    assert_eq!(sent[0].1.set[0].id, first.set[0].id);

    // A keyed row keeps its node as the list changes around it: Ann leaves,
    // and only her node and the panel that held it change.
    let ids = |change: &Change, name: &str| {
        change
            .set
            .iter()
            .find(|node| node.text == name)
            .map(|node| node.id.clone())
    };
    let bob = ids(first, "Bob");
    assert!(show(player, screen(2, &["Bob", "Cid"])));
    let sent = drawn(&calls);
    let change = &sent[0].1;
    assert_eq!(texts(change), ["Cid"]);
    assert_eq!(change.removed, [ids(first, "Ann").unwrap()]);
    let panel = change
        .set
        .iter()
        .find(|node| node.kind == Kind::Panel)
        .unwrap();
    assert_eq!(panel.children[1], bob.unwrap());

    // Hiding clears the tree, and the next show sends it whole again.
    assert!(hide(player));
    let sent = drawn(&calls);
    assert_eq!((sent[0].1.reset, sent[0].1.set.len()), (true, 0));
    assert!(show(player, screen(2, &["Bob"])));
    assert!(drawn(&calls)[0].1.reset);
}

/// A press runs its button's handler on the mod with the next frame, and an
/// input's change its handler with the value.
#[test]
fn presses_and_changes_reach_their_handlers() {
    use crate::ui::*;

    let lines = Lines::default();
    let held = lines.clone();
    let make = move || Menu { lines: held };
    let events = events!(make);
    send(make.clone(), &events, "Start", &StartEvent::default());
    let calls = answer(|_| Ok(Vec::new()));
    let player = Player::of(2);

    show(
        player,
        panel()
            .child(button().on_press(Menu::ready).child(text("Ready")))
            .child(
                button()
                    .disabled(true)
                    .on_press(Menu::ready)
                    .child(text("Later")),
            )
            .child(entry().value("gg").on_change(Menu::typed))
            .child(slider().range(0.0, 10.0).on_wheel(|_: &mut Menu, _, _| {})),
    );
    let first = drawn(&calls).remove(0).1;
    let buttons: Vec<&Node> = first
        .set
        .iter()
        .filter(|node| node.kind == Kind::Button)
        .collect();
    let inputs: Vec<&Node> = first
        .set
        .iter()
        .filter(|node| node.kind == Kind::Input)
        .collect();
    assert_eq!((buttons.len(), inputs.len()), (2, 2));
    assert!(
        matches!(&inputs[1].input, Some(NodeInput::Slider(slider)) if slider.wheel && slider.max == 10.0)
    );

    // The handler waits for a frame; a disabled button's never runs.
    let press = |node: &str| UiPressEvent {
        player,
        node: node.to_owned(),
        ..Default::default()
    };
    send(make.clone(), &events, "UiPress", &press(&buttons[0].id));
    send(make.clone(), &events, "UiPress", &press(&buttons[1].id));
    assert!(lines.borrow().is_empty());
    send(make.clone(), &events, "Frame", &at(1.0));
    assert_eq!(*lines.borrow(), ["player 2 ready"]);

    // An entry's text goes to its handler.
    let typed = UiInputEvent {
        player,
        node: inputs[0].id.clone(),
        value: Some(UiInputEventValue::Text("hello".to_owned())),
    };
    send(make.clone(), &events, "UiInput", &typed);
    send(make.clone(), &events, "Frame", &at(2.0));
    assert_eq!(lines.borrow()[1], "player 2 typed hello");

    // A click on the game's ability bar goes to the handler that asked.
    ability_bar_presses(player, Menu::bar);
    assert!(drawn(&calls)[0].1.set.last().unwrap().ability_bar_presses);
    send(make.clone(), &events, "UiPress", &press("ability:2"));
    send(make.clone(), &events, "Frame", &at(3.0));
    assert_eq!(lines.borrow()[2], "player 2 bar 2");

    // A player who takes the slot starts with nothing shown.
    let report = |generation| PlayersChangedEvent {
        players: vec![Connection {
            player,
            generation,
            ..Default::default()
        }],
    };
    send(make.clone(), &events, "PlayersChanged", &report(1));
    send(make.clone(), &events, "PlayersChanged", &report(2));
    send(make.clone(), &events, "UiPress", &press(&buttons[0].id));
    send(make.clone(), &events, "Frame", &at(4.0));
    assert_eq!(lines.borrow().len(), 3);
}

/// The root carries what the mod asked of the player's whole screen, and a
/// notice shows until its time passes.
#[test]
fn the_root_carries_the_screen() {
    use crate::ui::*;

    let make = || Quiet;
    let events = events!(make);
    send(make, &events, "Frame", &at(10.0));
    let calls = answer(|_| Ok(Vec::new()));
    let player = Player::of(1);
    let root = |calls: &Calls| drawn(calls).remove(0).1.set.pop().unwrap();

    assert!(hide_hud(player, &[HudPart::Minimap, HudPart::Souls]));
    assert_eq!(root(&calls).hide_hud, [HudPart::Minimap, HudPart::Souls]);
    assert!(free_cursor(player, true));
    let freed = root(&calls);
    assert_eq!((freed.free_cursor, freed.free_cursor_asks), (true, 1));
    assert!(sound(player, "UI.ItemDraft.Start", 0.0));
    assert_eq!(root(&calls).cues[0].number, 1);

    // A sheet and a route travel only when well formed.
    assert!(!sheet("../hud.css") && !sheet("hud.txt") && sheet("hud.css"));
    assert_eq!(root(&calls).sheets, ["hud.css"]);
    assert!(!route(player, "shop", "{not json") && !route(player, "sh op", "1"));
    assert!(route(
        player,
        "shop",
        r#"{"gold": [1, 2.5e3, "x\\n"], "open": true}"#
    ));
    let sent = drawn(&calls).remove(0).1;
    let routed = sent
        .set
        .iter()
        .find(|node| node.kind == Kind::Route)
        .unwrap();
    assert_eq!(
        (routed.id.as_str(), routed.route.as_str()),
        ("route:shop", "shop")
    );

    // A notice shows under the root, and leaves when its time passes.
    assert!(toast_for(player, "Saved", 2.0));
    assert_eq!(texts(&drawn(&calls)[0].1), ["Saved"]);
    send(make, &events, "Frame", &at(11.0));
    assert!(drawn(&calls).is_empty());
    send(make, &events, "Frame", &at(12.5));
    assert_eq!(drawn(&calls)[0].1.removed.len(), 2);
}

/// Tags build the tree the functions they name build, so showing one after
/// the other changes nothing.
#[test]
fn tags_build_what_their_functions_build() {
    use crate::ui::*;

    let calls = answer(|_| Ok(Vec::new()));
    let player = Player::of(4);
    let names = ["Ann", "Bob"];
    let count = format!("{} players", names.len());

    let built = panel()
        .style(Style {
            flow: Flow::Down,
            padding: Some(16.into()),
            margin: Some([4, 8].into()),
            background: 0x0000_00cc,
            color: rgb(0xffcc00),
            width: Some(px(200)),
            height: Some(FIT),
            opacity: Some(0.5),
            font_size: 24.0,
            ..Default::default()
        })
        .class("board wide")
        .child(text("Scores"))
        .child(label().text(count.as_str()).style(Style {
            bold: true,
            ..Default::default()
        }))
        .child(names.iter().map(|name| label().text(*name).key(name)))
        .child(
            button()
                .disabled(true)
                .on_press(Menu::ready)
                .child(text("Ready")),
        )
        .child(None::<Element>);
    assert!(show(player, built));
    assert_eq!(drawn(&calls)[0].1.set.len(), 8);

    assert!(show(
        player,
        ui! {
            <panel
                style={{
                    flow: Flow::Down,
                    padding: 16,
                    margin: [4, 8],
                    background: "#000000cc",
                    color: "#ffcc00",
                    width: 200,
                    height: FIT,
                    opacity: 0.5,
                    font_size: 24,
                }}
                class="board wide"
            >
                "Scores"
                <label text={count.as_str()} style={{ bold: true }} />
                {names.iter().map(|name| ui! { <label text={*name} key={name} /> })}
                <button disabled on_press={{ let _ = 0; Menu::ready }}>"Ready"</button>
                {None::<Element>}
            </panel>
        }
    ));
    assert!(drawn(&calls).is_empty());
}

/// A color reads as "#rrggbb" or "#rrggbbaa", and anything else as none.
#[test]
fn colors_read_from_text() {
    use crate::ui::color;

    assert_eq!(color("#ffcc00"), 0xffcc_00ff);
    assert_eq!(color("000000cc"), 0x0000_00cc);
    assert_eq!(color("#FFF"), 0);
    assert_eq!(color("#+12345"), 0);
    assert_eq!(color("red"), 0);
}

/// A gradient's colors arrive as stops spaced evenly along its shape.
#[test]
fn a_gradient_spaces_its_colors() {
    use crate::ui::{gradient, rgb};

    let stop = |at, color| GradientStop { at, color };
    assert_eq!(
        gradient(GradientShape::Right, &[rgb(0x000000), rgb(0xffffff)]),
        Gradient {
            shape: GradientShape::Right,
            stops: vec![stop(0.0, 0x0000_00ff), stop(1.0, 0xffff_ffff)],
        }
    );
    let places = |colors: &[u32]| -> Vec<f32> {
        let stops = gradient(GradientShape::Down, colors).stops;
        stops.iter().map(|stop| stop.at).collect()
    };
    assert_eq!(places(&[1, 2, 3]), [0.0, 0.5, 1.0]);
    assert_eq!(places(&[1]), [0.0]);
}

/// A later style overrides the members it sets and leaves the rest, and a
/// transform merges part by part.
#[test]
fn styles_merge() {
    let base = Style {
        flow: Flow::Down,
        font_size: 16.0,
        bold: true,
        background: 0x1122_33ff,
        transform: Some(Transform {
            translate_x: 16.0,
            ..Default::default()
        }),
        ..Default::default()
    };
    let over = Style {
        font_size: 24.0,
        opacity: Some(0.5),
        transform: Some(Transform {
            scale_x: Some(0.9),
            scale_y: Some(0.9),
            ..Default::default()
        }),
        ..Default::default()
    };
    let merged = base.merge(over);
    assert_eq!(
        (merged.flow, merged.font_size, merged.bold),
        (Flow::Down, 24.0, true)
    );
    assert_eq!(
        (merged.background, merged.opacity),
        (0x1122_33ff, Some(0.5))
    );
    let transform = merged.transform.unwrap();
    assert_eq!(
        (transform.translate_x, transform.scale_x),
        (16.0, Some(0.9))
    );
}

/// The HUD kit's components draw as elements in the stock HUD's look, and
/// their buttons reach the mod.
#[test]
fn the_hud_kit_draws() {
    use crate::ui::*;
    use crate::{hud, theme};

    let lines = Lines::default();
    let held = lines.clone();
    let make = move || Menu { lines: held };
    let events = events!(make);
    send(make.clone(), &events, "Start", &StartEvent::default());
    let calls = answer(|_| Ok(Vec::new()));
    let player = Player::of(1);

    assert!(show(
        player,
        ui! {
            <panel>
                <hud::top_strip heading="Round 2" line="Sudden death" line_tone={theme::DANGER}>
                    <hud::score near={2} far={1} />
                    <hud::pips filled={2} total={3} tone={theme::AMBER} />
                </hud::top_strip>
                <hud::banner title="Amber wins" detail="3 rounds to 1" tone={theme::AMBER} />
                <hud::meter value={1.5} tone={theme::GOLD} />
                <hud::tag text="Live" tone={theme::SUCCESS} filled />
                <hud::action text="Ready" primary on_press={Menu::ready} />
                <hud::action text="Leave" quiet disabled on_press={Menu::ready} />
                <hud::roster>
                    <hud::row name="Ann the very long named player of all" own hero={13} steam_id={111u64} />
                    <hud::row name="Bob" activity="Free roam" on_press={Menu::ready} />
                </hud::roster>
            </panel>
        }
    ));
    let screen = drawn(&calls).remove(0).1;
    let node = |text: &str| {
        let reads = |node: &&Node| node.text == text;
        let all: Vec<&str> = screen.set.iter().map(|node| node.text.as_str()).collect();
        screen
            .set
            .iter()
            .find(reads)
            .unwrap_or_else(|| panic!("no node reads {text}: {all:?}"))
    };
    let parent = |id: &str| {
        screen
            .set
            .iter()
            .find(|node| node.children.iter().any(|child| child == id))
            .unwrap()
    };

    // Text reads in its role's type and tone.
    let heading = node("Round 2");
    assert_eq!(
        (
            heading.style.face,
            heading.style.font_size,
            heading.style.uppercase
        ),
        (Face::Block, 15.0, true)
    );
    assert_eq!(
        (heading.style.color, heading.style.shadow),
        (theme::MUTED, true)
    );
    assert_eq!(node("Sudden death").style.color, theme::DANGER);
    assert_eq!(
        (node("2").style.color, node("1").style.color),
        (theme::ACCENT, theme::TEXT)
    );
    assert!(!node("2").style.shadow);
    assert_eq!(node("Amber wins").style.color, theme::AMBER);

    // Two of three pips are lit, and the meter is full, not over.
    let lit = screen
        .set
        .iter()
        .filter(|node| node.style.background == theme::AMBER)
        .count();
    let unlit = screen
        .set
        .iter()
        .filter(|node| node.style.border_color == theme::EMPTY_RING)
        .count();
    assert_eq!((lit, unlit), (2, 1));
    let fill = screen
        .set
        .iter()
        .find(|node| node.style.background == theme::GOLD)
        .unwrap();
    assert_eq!(fill.style.transform.as_ref().unwrap().scale_x, Some(1.0));

    // A filled tag reads dark on its tone.
    let live = node("Live");
    assert_eq!(
        (live.style.background, live.style.color),
        (theme::SUCCESS, theme::ON_ACCENT)
    );

    // An action lights and ticks under the pointer; a disabled one dims and
    // does neither.
    let ready = parent(&node("Ready").id);
    let hover = ready.hover.as_deref().expect("a hover style");
    assert_eq!(
        (ready.kind, ready.style.background, hover.background),
        (Kind::Button, theme::TEXT, theme::WHITE)
    );
    assert_eq!(
        (ready.sound.as_str(), ready.hover_sound.as_str()),
        (theme::PRESS_SOUND, theme::HOVER_SOUND)
    );
    let leave = parent(&node("Leave").id);
    assert_eq!(
        (
            leave.disabled,
            leave.style.opacity,
            leave.hover_sound.as_str()
        ),
        (true, Some(0.4), "")
    );
    assert_eq!(leave.hover, None);

    // The roster clips a long name at a word, edges the viewer's own portrait
    // in the accent, and makes a pressable row a button.
    assert_eq!(node("ANN THE VERY LONG…").style.text_align, Align::End);
    assert!(screen
        .set
        .iter()
        .any(|node| node.image.ends_with("haze_sm.psd")));
    assert_eq!(
        screen
            .set
            .iter()
            .filter(|node| node.style.background == theme::ACCENT
                && node
                    .style
                    .padding
                    .as_ref()
                    .is_some_and(|edges| edges.top == 1.0))
            .count(),
        1
    );
    let bob = parent(&parent(&node("BOB").id).id);
    assert_eq!(bob.kind, Kind::Button);

    // The roster's key follows a row's press and selection, and leaves out a
    // row's menu.
    let plain = hud::row().name("Bob").hero(13);
    let picked = plain.clone().selected(true).on_press(Menu::ready);
    assert_ne!(
        hud::roster_key(&[picked]),
        hud::roster_key(std::slice::from_ref(&plain))
    );
    let with_menu = plain
        .clone()
        .menu(ui! { <hud::text role={theme::Role::Label} text="menu" /> });
    assert_eq!(hud::roster_key(&[with_menu]), hud::roster_key(&[plain]));

    // Without rows the roster draws nothing.
    assert!(elements(hud::roster()).is_empty());

    // A row's menu hangs left of the column, its top level with its row's.
    let menu = ui! { <hud::text role={theme::Role::Label} text="menu" /> };
    let opened = show(
        player,
        ui! {
            <panel>
                <hud::roster>
                    <hud::row name="One" hero={13} />
                    <hud::row name="Two" hero={13} />
                    <hud::row name="Three" hero={13} />
                    <hud::row name="Four" hero={13} menu={menu} />
                    <hud::row name="Five" hero={13} menu={ui! { <hud::text role={theme::Role::Label} text="second" /> }} />
                </hud::roster>
            </panel>
        },
    );
    assert!(opened);
    let menu_screen = drawn(&calls).remove(0).1;
    let menu_label = menu_screen
        .set
        .iter()
        .find(|node| node.text == "menu")
        .unwrap();
    assert!(!menu_screen.set.iter().any(|node| node.text == "second"));
    let parent_of = |id: &str| {
        menu_screen
            .set
            .iter()
            .find(|node| node.children.iter().any(|child| child == id))
            .unwrap()
    };
    let hang = parent_of(&menu_label.id);
    let outer = parent_of(&hang.id);
    assert_eq!(outer.style.flow, Flow::Right);
    assert_eq!(outer.children.len(), 2);
    assert_eq!(
        hang.style.margin,
        Some(Edges {
            top: 3.0 * 52.0,
            right: 8.0,
            bottom: 0.0,
            left: 0.0,
        })
    );

    // The pressed action and row reach the mod; the disabled action does not.
    assert!(show(
        player,
        ui! {
            <panel>
                <hud::top_strip heading="Round 2" line="Sudden death" line_tone={theme::DANGER}>
                    <hud::score near={2} far={1} />
                    <hud::pips filled={2} total={3} tone={theme::AMBER} />
                </hud::top_strip>
                <hud::banner title="Amber wins" detail="3 rounds to 1" tone={theme::AMBER} />
                <hud::meter value={1.5} tone={theme::GOLD} />
                <hud::tag text="Live" tone={theme::SUCCESS} filled />
                <hud::action text="Ready" primary on_press={Menu::ready} />
                <hud::action text="Leave" quiet disabled on_press={Menu::ready} />
                <hud::roster>
                    <hud::row name="Ann the very long named player of all" own hero={13} steam_id={111u64} />
                    <hud::row name="Bob" activity="Free roam" on_press={Menu::ready} />
                </hud::roster>
            </panel>
        }
    ));
    let again = drawn(&calls).remove(0).1;
    let again_node = |text: &str| again.set.iter().find(|node| node.text == text).unwrap();
    let again_parent = |id: &str| {
        again
            .set
            .iter()
            .find(|node| node.children.iter().any(|child| child == id))
            .unwrap()
    };
    let ready = again_parent(&again_node("Ready").id);
    let leave = again_parent(&again_node("Leave").id);
    let bob = again_parent(&again_parent(&again_node("BOB").id).id);
    for id in [&ready.id, &leave.id, &bob.id] {
        let press = UiPressEvent {
            player,
            node: id.clone(),
            ..Default::default()
        };
        send(make.clone(), &events, "UiPress", &press);
    }
    send(make.clone(), &events, "Frame", &at(1.0));
    assert_eq!(*lines.borrow(), ["player 1 ready", "player 1 ready"]);
}

/// line is a component: a name beside its points, bold on the viewer's own
/// line.
#[crate::ui::component]
fn line(
    /// name is whose line it is.
    name: String,
    points: u32,
    own: bool,
    on_press: Option<crate::ui::Press>,
    children: Vec<crate::ui::Element>,
) -> crate::ui::Element {
    use crate::ui::*;
    let pressed = on_press.map(|handler| ui! { <button on_press={handler}>"+1"</button> });
    ui! {
        <panel style={{ flow: Flow::Right }}>
            <label text={name} style={{ bold: own }} />
            <label text={points.to_string()} />
            {pressed}
            {children}
        </panel>
    }
}

/// A function marked as a component is written as a tag, whose attributes
/// are the function's parameters, and as plain code too.
#[test]
fn a_component_is_a_tag() {
    use crate::ui::*;

    let lines = Lines::default();
    let held = lines.clone();
    let make = move || Menu { lines: held };
    let events = events!(make);
    send(make.clone(), &events, "Start", &StartEvent::default());
    let calls = answer(|_| Ok(Vec::new()));
    let player = Player::of(1);

    assert!(show(
        player,
        ui! {
            <panel>
                <line name="Ann" points={3} own key="ann" on_press={Menu::ready}>"extra"</line>
                <line name={format!("B{}", "ob")} />
            </panel>
        }
    ));
    let screen = drawn(&calls).remove(0).1;
    assert_eq!(texts(&screen), ["Ann", "3", "+1", "extra", "Bob", "0"]);
    let named = |text: &str| screen.set.iter().find(|node| node.text == text).unwrap();
    assert!(named("Ann").style.bold && !named("Bob").style.bold);

    // The tag and the plain code build the same tree.
    assert!(show(
        player,
        panel()
            .child(
                line()
                    .name("Ann")
                    .points(3)
                    .own(true)
                    .key("ann")
                    .on_press(Menu::ready)
                    .child(text("extra"))
            )
            .child(line().name("Bob"))
    ));
    assert!(drawn(&calls).is_empty());

    // The handler a component was given reaches the mod.
    let button = screen
        .set
        .iter()
        .find(|node| node.kind == Kind::Button)
        .unwrap();
    let press = UiPressEvent {
        player,
        node: button.id.clone(),
        ..Default::default()
    };
    send(make.clone(), &events, "UiPress", &press);
    send(make.clone(), &events, "Frame", &at(1.0));
    assert_eq!(*lines.borrow(), ["player 1 ready"]);
}
