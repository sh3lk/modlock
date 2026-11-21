//! Tests of the helpers that call nothing in the game.

use crate::format;
use crate::rotation::{
    after_match, leaderboard, least_used, next_match, settle, Standings, RATING_STEP,
    STARTING_RATING,
};
use crate::{Angles, Buttons, Layers, Vector};

/// near reports whether two numbers agree to a millionth.
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// A vector adds, scales and measures, and a view turns into a direction and
/// back.
#[test]
fn vectors_measure_and_aim() {
    let (a, b) = (Vector::new(1.0, 2.0, 3.0), Vector::new(4.0, 6.0, 3.0));
    assert_eq!(a + b, Vector::new(5.0, 8.0, 6.0));
    assert_eq!(b - a, Vector::new(3.0, 4.0, 0.0));
    assert_eq!(a * 2.0, Vector::new(2.0, 4.0, 6.0));
    assert_eq!(-a, Vector::new(-1.0, -2.0, -3.0));
    assert_eq!(a.dot(b), 25.0);
    assert!(near(a.distance(b), 5.0) && near(a.flat_distance(b), 5.0));
    assert_eq!(a.lerp(b, 0.5), Vector::new(2.5, 4.0, 3.0));
    let step = a.toward(b);
    assert!(near(step.x, 0.6) && near(step.y, 0.8) && step.z == 0.0);
    assert_eq!(a.toward(a), Vector::default());
    assert_eq!(Vector::default().normalize(), Vector::new(1.0, 0.0, 0.0));
    assert!(near((b - a).normalize().length(), 1.0));

    // Yaw runs counterclockwise from +x, and pitch is positive looking down.
    let origin = Vector::default();
    assert!(near(origin.yaw_toward(Vector::new(0.0, 5.0, 0.0)), 90.0));
    let down = origin.aim_at(Vector::new(10.0, 0.0, -10.0));
    assert!(near(down.pitch, 45.0) && near(down.yaw, 0.0) && down.roll == 0.0);
    let ahead = down.forward();
    assert!(near(ahead.x, ahead.z.abs()) && ahead.z < 0.0 && near(ahead.y, 0.0));
    let left = Angles::level(90.0).forward();
    assert!(near(left.x, 0.0) && near(left.y, 1.0) && near(left.z, 0.0));
}

/// The HUD's numbers read the way players read them.
#[test]
fn numbers_format_for_the_hud() {
    assert_eq!(format::clock(247.0), "4:07");
    assert_eq!(format::clock(0.2), "0:01");
    assert_eq!(format::clock(-3.0), "0:00");
    assert_eq!(format::stopwatch(64.379), "1:04.37");
    assert_eq!(format::stopwatch(-1.0), "0:00.00");
    assert_eq!(format::percent(21.0, 50.0), "42%");
    assert_eq!(format::percent(9.0, 3.0), "100%");
    assert_eq!(format::percent(1.0, 0.0), "-");
    assert_eq!(format::grouped(4210.4), "4,210");
    assert_eq!(format::grouped(1_234_567.0), "1,234,567");
    assert_eq!(format::grouped(-1500.0), "-1,500");
    assert_eq!(format::grouped(999.0), "999");
    let places = [1, 2, 3, 4, 11, 12, 13, 21, 23, 111].map(format::ordinal);
    assert_eq!(
        places,
        ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "23rd", "111th"]
    );
    assert_eq!(format::count(1, "kill"), "1 kill");
    assert_eq!(format::count(3000, "kill"), "3,000 kills");
    assert_eq!(format::count_of(2, "enemy", "enemies"), "2 enemies");
    assert_eq!(format::clip("short", 10), "short");
    assert_eq!(format::clip("Ann the very long named", 12), "Ann the…");
    assert_eq!(format::clip("Abcdefghijklmnop", 6), "Abcde…");
}

/// The button and layer bits join into one mask.
#[test]
fn bits_join_into_masks() {
    assert_eq!(Buttons::ATTACK | Buttons::CROUCH, 0b101);
    assert_eq!(Buttons::JUMP, 1 << 48);
    assert_eq!(Layers::SOLID | Layers::TRIGGER, 0b101);
}

/// A session runs through join, two seatings, a result and the leaderboard,
/// the way a rotation mod calls these each match.
#[test]
fn a_rotation_session_seats_evenly_and_keeps_standings() {
    let mut standings: Standings<[u32; 2]> = Standings::new();
    let ids = [1u64, 2, 3, 4, 5];
    for id in &ids[..4] {
        standings.join(*id, []);
    }

    // Four even players: the snake deal puts the first and fourth together.
    for (id, rating) in [(1, 1100.0), (2, 1050.0), (3, 950.0), (4, 900.0)] {
        standings.get_mut(id).unwrap().standing.rating = rating;
    }
    let mut line = ids[..4].to_vec();
    let (a, b) = next_match(&line, 2, |id| standings.standing(*id)).unwrap();
    assert_eq!((a.as_slice(), b.as_slice()), (&[1, 4][..], &[2, 3][..]));

    // The winners rise by the upset step and the losers fall by it.
    assert_eq!(standings.settle(&a, &b, RATING_STEP), 16.0);
    let first = standings.standing(1);
    assert_eq!((first.rating, first.wins, first.streak), (1116.0, 1, 1));
    let second = standings.standing(2);
    assert_eq!(
        (second.rating, second.losses, second.streak),
        (1034.0, 1, 0)
    );
    for id in a.iter().chain(&b) {
        standings.get_mut(*id).unwrap().standing.played += 1;
    }

    // A late joiner starts level with the fewest played and keeps the extra.
    let late = standings.join(5, line.iter().copied());
    assert_eq!(
        (late.standing.played, late.standing.rating),
        (1, STARTING_RATING)
    );
    assert_eq!(late.extra, [0, 0]);

    // A rejoin keeps the record rather than resetting it.
    assert_eq!(standings.join(1, line.iter().copied()).standing.wins, 1);

    // The fighters go to the back; the next one-a-side match takes the front.
    line.push(5);
    line = after_match(&line, &[1, 2]);
    assert_eq!(line, [3, 4, 5, 1, 2]);
    standings.get_mut(3).unwrap().standing.played = 0;
    let (a, b) = next_match(&line, 1, |id| standings.standing(*id)).unwrap();
    assert_eq!([a, b].concat(), [3, 4]);
    assert!(next_match(&line[..3], 2, |id| standings.standing(*id)).is_none());

    // The viewer below the cut keeps the last row.
    let standing = |id: &u64| standings.standing(*id);
    assert_eq!(leaderboard(&line, standing, 2, Some(&2)), [1, 2]);
    assert_eq!(leaderboard(&line, standing, 2, Some(&5)), [1, 5]);

    // An even hero share prefers the default, else the one played least.
    assert_eq!(least_used(&[1, 1], 1), 1);
    assert_eq!(least_used(&[2, 1], 0), 1);

    // The function settles standings a mod keeps itself.
    let (mut won, mut lost) = (standings.standing(3), standings.standing(4));
    assert_eq!(settle([&mut won], [&mut lost], RATING_STEP), 15.0);
    assert_eq!((won.wins, lost.losses), (1, 1));
}

/// Idle is a mod that does nothing, for the frames that run the timers.
#[derive(Default)]
struct Idle;

impl crate::Mod for Idle {}

/// Sent holds the camera calls the host received: each call's method and,
/// for a move, its options.
type Sent = std::rc::Rc<std::cell::RefCell<Vec<(String, Option<crate::CameraOptions>)>>>;

/// camera_host makes the host record the camera calls, and fail the call
/// whose method refuse names.
fn camera_host(refuse: std::rc::Rc<std::cell::Cell<&'static str>>) -> Sent {
    use crate::wire::Message;
    let sent = Sent::default();
    let record = sent.clone();
    crate::host::fake::answer(move |method, request| {
        let options = (method == "MoveCamera")
            .then(|| crate::CameraOptions::decode(request).expect("the options of a move"));
        record.borrow_mut().push((method.to_owned(), options));
        if method == refuse.get() {
            return Err("no hero".to_owned());
        }
        Ok(Vec::new())
    });
    sent
}

/// lerps returns each sent lerp of a number as its start, end and duration.
fn lerps(sent: &Sent) -> Vec<[f32; 3]> {
    let scalar = |value: &Option<crate::CameraValue>| match value {
        Some(crate::CameraValue::Number(number)) => *number,
        _ => f32::NAN,
    };
    let sent = sent.borrow();
    let moves = sent.iter().filter_map(|(_, options)| options.as_ref());
    moves
        .filter_map(|options| match &options.motion {
            Some(crate::CameraMotion::Lerp(lerp)) => Some([
                scalar(&lerp.start),
                scalar(&lerp.end),
                lerp.duration.unwrap_or(0.0),
            ]),
            _ => None,
        })
        .collect()
}

/// Curves compose, and a move plays them as lerps queued in one context.
#[test]
fn curves_compose_and_play_as_queued_lerps() {
    use crate::camera::{Play, Pulse, Shake};
    use crate::curve::{self, Ease, Key, Orbit, Through};
    use crate::{camera_pulse, camera_shake, play_camera, CameraParam, CameraValue, Player};

    let number = |value: Option<CameraValue>| match value {
        Some(CameraValue::Number(number)) => f64::from(number),
        other => panic!("{other:?} is no number"),
    };

    // A straight or bias and gain line is one of the game's lerps, and a
    // reversed bias is not.
    let straight = curve::line(0, 10, 2.0, Ease::Linear);
    assert!(straight.spans.iter().all(|span| span.lerp.is_some()));
    let biased = curve::line(0, 10, 2.0, Ease::Bias(0.25));
    let shapes = biased
        .spans
        .iter()
        .map(|span| span.lerp.and_then(|shape| shape.bias));
    assert!(shapes.eq([Some(0.25)]));
    assert_eq!(number(biased.value_at(1.0)), 2.5);
    assert!(biased
        .reverse()
        .spans
        .iter()
        .all(|span| span.lerp.is_none()));

    // Any other ease is sampled at the rate, and passes through its ends.
    let eased = curve::line(0, 10, 2.0, Ease::Out);
    assert!(eased.spans.iter().all(|span| span.lerp.is_none()));
    assert_eq!([0.0, 2.0].map(|t| number(eased.value_at(t))), [0.0, 10.0]);
    assert!(number(eased.value_at(1.0)) > 5.0);

    // Keyframes wait at a late first key, jump where two keys share a time,
    // and a smooth curve still passes through every key.
    let keys = [
        Key::new(1.0, 90),
        Key::new(1.0, 60),
        Key::new(2.0, 70),
        Key::new(4.0, 40),
    ];
    assert_eq!(curve::keyframes(&keys, false).duration(), 4.0);
    let smooth = curve::keyframes(&keys, true);
    for (at, value) in [(2.0, 70.0), (4.0, 40.0)] {
        assert!((number(smooth.value_at(at)) - value).abs() < 1e-3);
    }

    // Through times each move by its distance; reverse and ping_pong mirror.
    let points = [0, 300, 200].map(CameraValue::from);
    let paced = curve::through(
        &points,
        Through {
            speed: 100.0,
            ..Through::default()
        },
    );
    assert_eq!(paced.duration(), 4.0);
    assert_eq!(number(paced.reverse().value_at(1.0)), 300.0);
    assert_eq!(number(paced.ping_pong().value_at(8.0)), 0.0);
    assert_eq!(paced.repeat(3).duration(), 12.0);

    // An orbit keeps its radius and climbs by its rise.
    let ring = curve::orbit(Orbit {
        center: Vector::new(100.0, 200.0, 50.0),
        radius: 40.0,
        duration: 2.0,
        rise: 10.0,
        ..Orbit::default()
    });
    let Some(CameraValue::Position(quarter)) = ring.value_at(0.5) else {
        panic!("an orbit moves a position");
    };
    assert!(near((quarter.x - 100.0).hypot(quarter.y - 200.0), 40.0));
    assert_eq!(quarter.z, 52.5);

    // A pulse sends one lerp out, one hold and one back, all relative and in
    // one context of its own.
    let refuse = std::rc::Rc::new(std::cell::Cell::new(""));
    let sent = camera_host(refuse.clone());
    let player = Player::of(3);
    let pulse = camera_pulse(
        player,
        Pulse::new(CameraParam::Distance, 300, 0.5, 2.0, 1.0),
    );
    let pulse = pulse.expect("the pulse started");
    assert_eq!(pulse.duration(), 3.5);
    assert_eq!(
        lerps(&sent),
        [[0.0, 300.0, 0.5], [300.0, 300.0, 2.0], [300.0, 0.0, 1.0]]
    );
    let options = |sent: &Sent| -> Vec<crate::CameraOptions> {
        let sent = sent.borrow();
        sent.iter()
            .filter_map(|(_, options)| options.clone())
            .collect()
    };
    assert!(options(&sent)
        .iter()
        .all(|options| options.relative == Some(true)));
    let named =
        |options: &crate::CameraOptions| options.context.as_deref() == Some(pulse.context());
    assert!(options(&sent).iter().all(named));

    // Two tracks share a timeline, each in order; the delay holds only each
    // track's first lerp, and cancel clears only this move's context.
    sent.borrow_mut().clear();
    let play = Play::new()
        .delay(1.0)
        .rate(10.0)
        .track(
            CameraParam::Distance,
            curve::line(150, 400, 1.0, Ease::Linear),
        )
        .track(
            CameraParam::FieldOfView,
            curve::line(90, 70, 1.0, Ease::InOut),
        );
    let both = play_camera(player, play).expect("the move started");
    assert_ne!(both.context(), pulse.context());
    assert_eq!(sent.borrow().len(), 11);
    let delayed = options(&sent)
        .iter()
        .filter(|options| options.delay == Some(1.0))
        .count();
    assert_eq!(delayed, 2);
    sent.borrow_mut().clear();
    assert!(both.cancel());
    let methods: Vec<String> = sent
        .borrow()
        .iter()
        .map(|(method, _)| method.clone())
        .collect();
    assert_eq!(methods, ["ClearCamera"]);

    // A bias or gain line goes out as one lerp carrying its shape.
    sent.borrow_mut().clear();
    let gained = curve::line(0, 300, 1.0, Ease::Gain(0.75));
    play_camera(player, Play::new().track(CameraParam::Distance, gained));
    let shaped = options(&sent);
    assert_eq!(shaped.len(), 1);
    assert!(matches!(
        &shaped[0].motion,
        Some(crate::CameraMotion::Lerp(lerp)) if lerp.gain == Some(0.75)
    ));

    // A shake settles at zero.
    sent.borrow_mut().clear();
    let shake = Shake {
        amplitude: 20.0,
        duration: 0.5,
        ..Shake::default()
    };
    camera_shake(player, shake);
    assert!(lerps(&sent).last().is_some_and(|last| last[1].abs() < 1e-3));

    // A loop queues two rounds ahead.
    let events = events!(Idle::default);
    let frame = |seconds: f64| {
        crate::tests::send(Idle::default, &events, "Frame", &crate::tests::at(seconds));
    };
    frame(0.0);
    sent.borrow_mut().clear();
    let bob = curve::line(0, 50, 1.0, Ease::Linear).ping_pong();
    let play = Play::new()
        .looping()
        .track(CameraParam::VerticalOffset, bob);
    let looped = play_camera(player, play).expect("the loop started");
    assert_eq!(looped.duration(), f64::INFINITY);
    assert_eq!(lerps(&sent).len(), 4);

    // Each finished round queues the next until the loop is cancelled.
    frame(2.0);
    assert_eq!(lerps(&sent).len(), 6);
    looped.cancel();
    frame(4.0);
    assert_eq!(lerps(&sent).len(), 6);

    // A move the host refuses is cleared and reports nothing.
    refuse.set("MoveCamera");
    sent.borrow_mut().clear();
    assert!(camera_pulse(player, Pulse::new(CameraParam::Distance, 1, 1.0, 1.0, 1.0)).is_none());
    assert_eq!(
        sent.borrow().last().map(|(method, _)| method.as_str()),
        Some("ClearCamera")
    );
}

/// A mode's Cast makes a member for each human who finished joining, ends it
/// when they leave, and seats each on the hero and team it wants.
#[test]
fn a_cast_keeps_a_modes_members() {
    use crate::wire;
    use crate::{Bots, Cast, Connection, HeroPick, Pawn, Player, PlayersChangedEvent, Want};
    use std::cell::RefCell;
    use std::rc::Rc;

    // The game shows each player a hero on team 2, and records what it is
    // asked.
    let asked = Rc::new(RefCell::new(Vec::new()));
    let record = asked.clone();
    crate::host::fake::answer(move |method, _| {
        record.borrow_mut().push(method.to_owned());
        let mut out = Vec::new();
        if method == "Pawn" {
            let pawn = Pawn {
                hero: 13,
                team: 2,
                health: 500,
                ..Pawn::default()
            };
            wire::message(&mut out, 1, &pawn);
        }
        Ok(out)
    });
    let events = events!(Idle::default);
    let connection = |slot: i32, generation: u32, bot: bool| Connection {
        player: Player::of(slot),
        generation,
        ready: true,
        bot,
        name: format!("Player {slot}"),
        ..Connection::default()
    };
    let report = |players: Vec<Connection>| {
        let report = PlayersChangedEvent { players };
        crate::tests::send(Idle::default, &events, "PlayersChanged", &report);
    };

    // Two humans join; the bot is left out.
    let mut cast: Cast<String> = Cast::new(Bots::Skip);
    let left = Rc::new(RefCell::new(Vec::new()));
    let frame = |cast: &mut Cast<String>, seconds: f64, want: Want| {
        cast.frame(
            seconds,
            |connection| connection.name.clone(),
            |member, _| left.borrow_mut().push(member),
            |_| want.clone(),
        );
    };
    report(vec![
        connection(1, 1, false),
        connection(2, 1, false),
        connection(3, 1, true),
    ]);
    frame(&mut cast, 1.0, Want::default());
    assert_eq!(cast.len(), 2);
    assert!(cast.members().eq(["Player 1", "Player 2"]));
    assert_eq!(
        cast.get(Player::of(2)).map(String::as_str),
        Some("Player 2")
    );
    assert!(cast.get(Player::of(3)).is_none());
    assert_eq!(cast.pawn(Player::of(1)).map(|pawn| pawn.hero), Some(13));

    // A member's hero is theirs to change.
    if let Some(member) = cast.get_mut(Player::of(1)) {
        member.push('!');
    }
    assert_eq!(
        cast.get(Player::of(1)).map(String::as_str),
        Some("Player 1!")
    );

    // A want the hero does not show is asked for, and one it shows is not.
    asked.borrow_mut().clear();
    let other = Want {
        hero: Some(HeroPick::Named("hero_wraith".to_owned())),
        ..Want::default()
    };
    frame(&mut cast, 5.0, other);
    assert_eq!(
        asked
            .borrow()
            .iter()
            .filter(|method| *method == "SelectHero")
            .count(),
        2
    );
    asked.borrow_mut().clear();
    let shown = Want {
        hero: Some(HeroPick::Id(13)),
        team: Some(2),
        ..Want::default()
    };
    frame(&mut cast, 9.0, shown);
    assert!(!asked.borrow().iter().any(|method| method == "SelectHero"));

    // A reconnect is a leave and a join, and a player who left is ended.
    report(vec![connection(1, 2, false)]);
    frame(&mut cast, 10.0, Want::default());
    assert_eq!(*left.borrow(), ["Player 1!", "Player 2"]);
    assert!(cast.members().eq(["Player 1"]));
    assert!(!cast.is_empty());
}

/// request reads the entity and the texts of a call's request: field 1 as a
/// number, and each text field by its number.
fn request(data: &[u8]) -> (u32, std::collections::HashMap<u32, String>) {
    let (mut entity, mut texts) = (0, std::collections::HashMap::new());
    let mut r = crate::wire::Reader::new(data);
    while let Some((number, kind)) = r.field().unwrap() {
        match (number, kind) {
            (1, crate::wire::VARINT) => entity = r.uint32(kind).unwrap(),
            (_, crate::wire::LEN) => {
                let text = String::from_utf8_lossy(r.bytes(kind).unwrap()).into_owned();
                texts.insert(number, text);
            }
            _ => r.skip(kind).unwrap(),
        }
    }
    (entity, texts)
}

/// Disconnected controllers do not keep abandoned heroes, while the heroes
/// of players still joining stay protected.
#[test]
fn strays_lay_down_abandoned_heroes() {
    use crate::wire::{self, Message};
    use crate::{FieldValue, HitOptions, Strays};
    use std::cell::RefCell;
    use std::rc::Rc;

    // Six controllers keep their hero handle across their connection states.
    let states = [Some(0.0), Some(1.0), Some(2.0), Some(3.0), Some(4.0), None];
    let hits = Rc::new(RefCell::new(Vec::new()));
    let released = Rc::new(RefCell::new(Vec::new()));
    let respawns = Rc::new(RefCell::new(Vec::new()));
    let (hit, release, respawn) = (hits.clone(), released.clone(), respawns.clone());
    crate::host::fake::answer(move |method, data| {
        let mut out = Vec::new();
        let (entity, texts) = request(data);
        match method {
            "FindEntities" => {
                let entities: Vec<u32> = (1..=6).collect();
                wire::packed(&mut out, 1, &entities, |out, v| wire::raw::uint32(out, *v));
            }
            "ReadField" => {
                let value = match texts[&3].as_str() {
                    "m_hController" => Some(f64::from(entity + 100)),
                    "m_hHeroPawn" => Some(f64::from(entity - 100)),
                    "m_iConnected" => states[entity as usize - 101],
                    "m_iHealth" => Some(100.0),
                    _ => None,
                };
                if let Some(value) = value {
                    wire::nested(&mut out, 1, |out| FieldValue::Number(value).encode(out));
                }
            }
            "Damage" => hit
                .borrow_mut()
                .push(HitOptions::decode(data).unwrap().victim),
            "HoldModifierState" if texts[&2] == "MODIFIER_STATE_MELEE_DISABLED" => {
                release.borrow_mut().push(entity);
            }
            "WriteField" => respawn.borrow_mut().push(entity),
            _ => {}
        }
        Ok(out)
    });

    // Nothing is removed during the grace period, then only the heroes of
    // the disconnected.
    let mut strays = Strays::new();
    strays.sweep(0.0);
    strays.sweep(2.0);
    assert!(hits.borrow().is_empty());
    strays.sweep(3.0);
    assert_eq!(*hits.borrow(), [4, 5]);
    assert_eq!(*respawns.borrow(), [4, 5]);
    assert_eq!(*released.borrow(), [4, 5]);
}

/// A human still connecting past the deadline is kicked once, by name.
#[test]
fn stragglers_kick_a_stuck_join() {
    use crate::{Connection, Player, PlayersChangedEvent, Stragglers};
    use std::cell::RefCell;
    use std::rc::Rc;

    let commands = Rc::new(RefCell::new(Vec::new()));
    let record = commands.clone();
    crate::host::fake::answer(move |method, data| {
        if method == "ServerCommand" {
            let mut r = crate::wire::Reader::new(data);
            while let Some((_, kind)) = r.field().unwrap() {
                record.borrow_mut().push(r.string(kind).unwrap());
            }
        }
        Ok(Vec::new())
    });
    let events = events!(Idle::default);
    let connection = |slot: i32, name: &str, ready: bool| Connection {
        player: Player::of(slot),
        generation: 1,
        ready,
        name: name.to_owned(),
        ..Connection::default()
    };
    let report = PlayersChangedEvent {
        players: vec![connection(1, "Ann", true), connection(2, "Bob", false)],
    };
    crate::tests::send(Idle::default, &events, "PlayersChanged", &report);

    // Bob is still joining after ten seconds, and is kicked only once.
    let mut stragglers = Stragglers::new().connect_seconds(10.0).leave_heroes();
    stragglers.sweep(0.0);
    stragglers.sweep(9.0);
    assert!(commands.borrow().is_empty());
    stragglers.sweep(10.0);
    stragglers.sweep(20.0);
    assert_eq!(*commands.borrow(), ["kick \"Bob\""]);
}

/// A restore sets the snapshot's souls once the game reports its end, and
/// tells the mod how it went.
#[test]
fn a_restore_settles_souls_and_reports() {
    use crate::wire::{self, Message};
    use crate::{restore_hero, HeroSnapshot, HeroTarget, Mod, Pawn, Player, RestoredEvent};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Duel keeps how its restores ended.
    #[derive(Default)]
    struct Duel {
        ended: Vec<String>,
    }
    impl Mod for Duel {
        fn frame(&mut self, _event: &crate::FrameEvent) {
            SEEN.with(|seen| seen.borrow_mut().clone_from(&self.ended));
        }
    }
    thread_local! {
        static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    // The game shows a hero with 5000 souls, and records what it is asked.
    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    crate::host::fake::answer(move |method, data| {
        record.borrow_mut().push((method.to_owned(), data.to_vec()));
        let mut out = Vec::new();
        if method == "Pawn" {
            let pawn = Pawn {
                hero: 13,
                souls: 5000,
                ..Pawn::default()
            };
            wire::message(&mut out, 1, &pawn);
        }
        Ok(out)
    });
    let events = events!(Duel::default);
    let player = Player::of(2);
    let snapshot = HeroSnapshot {
        health: 520,
        max_health: 760,
        souls: Some(4200),
        items: Some(Vec::new()),
        ..HeroSnapshot::default()
    };
    crate::tests::send(
        Duel::default,
        &events,
        "Start",
        &crate::StartEvent::default(),
    );
    assert!(restore_hero(player, snapshot, |duel: &mut Duel, error| {
        duel.ended.push(error.unwrap_or("held").to_owned());
    }));

    // The restore replaces the items and starts from the snapshot's souls.
    let sent = |method: &str| -> Option<Vec<u8>> {
        let calls = calls.borrow();
        let mut sent = calls.iter().filter(|call| call.0 == method);
        sent.next_back().map(|call| call.1.clone())
    };
    let asked = sent("RestoreHero").unwrap();
    let mut r = wire::Reader::new(&asked);
    let mut target = HeroTarget::default();
    while let Some((number, kind)) = r.field().unwrap() {
        match number {
            2 => target = HeroTarget::decode(r.bytes(kind).unwrap()).unwrap(),
            _ => r.skip(kind).unwrap(),
        }
    }
    assert_eq!((target.health, target.max_health), (Some(520), Some(760)));
    assert_eq!(target.replace_items, Some(true));
    assert!(sent("StartingSouls").is_some());

    // Its end refunds the difference and reaches the mod.
    let ended = RestoredEvent {
        player,
        ..RestoredEvent::default()
    };
    crate::tests::send(Duel::default, &events, "Restored", &ended);
    assert!(sent("AdjustSouls").is_some());
    crate::tests::send(Duel::default, &events, "Frame", &crate::tests::at(1.0));
    assert_eq!(SEEN.with(|seen| seen.borrow().clone()), ["held"]);
}

/// A track reads between its samples, turns the short way and clamps at its
/// ends.
#[test]
fn a_track_blends_its_samples() {
    use crate::Track;

    // Two samples one second apart, the facing crossing the 360 degree seam.
    let track = Track {
        step_ticks: 64.0,
        x: vec![0.0, 100.0],
        y: vec![0.0, 0.0],
        z: vec![10.0, 10.0],
        yaw: vec![350.0, 10.0],
        pitch: None,
        health: Some(vec![100.0, 50.0]),
    };
    assert_eq!(track.ticks(), 64.0);

    // Halfway is the midpoint, and the velocity is the segment's slope.
    let mid = track.pose_at(32.0).unwrap();
    assert_eq!(mid.position.x, 50.0);
    assert_eq!(mid.facing.yaw, 360.0);
    assert_eq!(mid.health, Some(75.0));
    assert_eq!(mid.velocity.x, 100.0);

    // A tick past either end reads that end.
    assert_eq!(track.pose_at(500.0).unwrap().position.x, 100.0);
    assert_eq!(track.pose_at(-5.0).unwrap().position.x, 0.0);
    assert!(Track::default().pose_at(0.0).is_none());
}

/// A playing puppet moves its hero with the frames, and a paused one is
/// placed once.
#[test]
fn a_puppet_follows_its_track() {
    use crate::wire;
    use crate::{Pawn, Player, Puppet, PuppetOptions, PuppetState, Track};
    use std::cell::RefCell;
    use std::rc::Rc;

    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    crate::host::fake::answer(move |method, _| {
        record.borrow_mut().push(method.to_owned());
        let mut out = Vec::new();
        if method == "Pawn" {
            let pawn = Pawn {
                entity: 40,
                hero: 13,
                ..Pawn::default()
            };
            wire::message(&mut out, 1, &pawn);
        }
        Ok(out)
    });
    let events = events!(Idle::default);
    let frame = |seconds: f64| {
        crate::tests::send(Idle::default, &events, "Frame", &crate::tests::at(seconds));
    };
    let count = |method: &str| calls.borrow().iter().filter(|call| *call == method).count();

    // A paused puppet is placed once, by a teleport.
    let track = Track {
        step_ticks: 64.0,
        x: vec![0.0, 100.0, 200.0],
        y: vec![0.0; 3],
        z: vec![0.0; 3],
        yaw: vec![0.0; 3],
        ..Track::default()
    };
    let puppet = Puppet::new(Player::of(5), track, PuppetOptions::default());
    frame(0.0);
    frame(0.1);
    assert_eq!((count("Teleport"), count("MovePlayer")), (1, 0));

    // A playing one glides each frame, and ends at the track's last tick.
    puppet.play();
    frame(0.2);
    frame(0.3);
    assert_eq!(count("MovePlayer"), 2);
    assert_eq!(puppet.state(), PuppetState::Playing);
    assert!((puppet.tick() - 12.8).abs() < 1e-9);
    puppet.set_rate(100.0);
    frame(0.4);
    assert_eq!(puppet.state(), PuppetState::Ended);
    assert_eq!(puppet.tick(), 128.0);

    // A stopped puppet moves no more.
    puppet.stop();
    let moved = count("MovePlayer") + count("Teleport");
    puppet.seek(0.0);
    frame(0.5);
    assert_eq!(count("MovePlayer") + count("Teleport"), moved);
}

/// A spot's document is the bytes the Go library writes for it, and reads
/// back as the spot.
#[test]
fn a_spot_survives_its_document() {
    use crate::{decode_spot, encode_spot, Angles, Spot, SpotObject};

    let crate_model = "models/props/crate.vmdl";
    let spot = Spot {
        map: "dl_midtown".to_owned(),
        objects: vec![
            SpotObject {
                model: crate_model.to_owned(),
                position: Vector::new(8.0, -16.0, 24.5),
                facing: Angles::level(90.0),
                placed_by: 7,
                ..SpotObject::default()
            },
            SpotObject {
                model: crate_model.to_owned(),
                position: Vector::new(1.0, 2.0, 3.0),
                scale: 2.0,
                bounce: 900.0,
                placed_by: 7,
                ..SpotObject::default()
            },
            SpotObject {
                model: "models/props/ramp.vmdl".to_owned(),
                facing: Angles::new(0.0, 0.0, 15.0),
                ..SpotObject::default()
            },
        ],
    };
    let written = concat!(
        "0a0a646c5f6d6964746f776e12176d6f64656c732f70726f70732f63726174652e766d646c",
        "12166d6f64656c732f70726f70732f72616d702e766d646c1a080700000000000000",
        "221615000000411d000080c1250000c441350000b4425001",
        "221b150000803f1d00000040250000404045000000404d000061445001",
        "220708013d00007041",
    );
    let encoded = encode_spot(&spot);
    assert_eq!(encoded, crate::tests::hex(written));
    assert_eq!(decode_spot(&encoded).unwrap(), spot);
}

/// A loaded spot keeps its objects by entity, and bounces a hero who stands
/// on a bouncing one.
#[test]
fn a_loaded_spot_places_and_bounces() {
    use crate::wire;
    use crate::{load_spot, MovementSample, Player, Spot, SpotObject, TraceHit};
    use std::cell::RefCell;
    use std::rc::Rc;

    // The game makes world objects 1, 2, ... with entities 101, 102, ...,
    // and a trace hits the second.
    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    let made = Rc::new(std::cell::Cell::new(0u32));
    crate::host::fake::answer(move |method, data| {
        record.borrow_mut().push(method.to_owned());
        let mut out = Vec::new();
        match method {
            "CreateModel" => {
                made.set(made.get() + 1);
                wire::uint32(&mut out, 1, made.get());
            }
            "WorldObjectEntity" | "ObjectEntity" | "Entity" => {
                let (id, _) = request(data);
                wire::uint32(&mut out, 1, id + 100);
            }
            "Trace" => {
                let hit = TraceHit {
                    entity: 102,
                    ..TraceHit::default()
                };
                wire::message(&mut out, 1, &hit);
            }
            _ => {}
        }
        Ok(out)
    });
    let object = |bounce: f32| SpotObject {
        model: "models/props/crate.vmdl".to_owned(),
        bounce,
        ..SpotObject::default()
    };
    let spot = Spot {
        map: "dl_midtown".to_owned(),
        objects: vec![object(0.0), object(900.0)],
    };
    let mut loaded = load_spot(&spot);
    let entities: Vec<u32> = loaded
        .objects()
        .iter()
        .map(|placed| placed.entity)
        .collect();
    assert_eq!(entities, [101, 102]);
    assert_eq!(loaded.spot(), spot);
    assert_eq!(
        loaded.at(102).map(|placed| placed.object.bounce),
        Some(900.0)
    );

    // A hero standing on the bouncing object is launched; one in the air is
    // not.
    let sample = |grounded: bool| MovementSample {
        player: Player::of(3),
        pawn: 40,
        grounded,
        ..MovementSample::default()
    };
    loaded.bounce(&[sample(false)]);
    assert!(!calls.borrow().iter().any(|call| call == "SetVelocity"));
    loaded.bounce(&[sample(true)]);
    assert!(calls.borrow().iter().any(|call| call == "SetVelocity"));

    // Removing the first keeps the second findable, and clear empties it.
    assert!(loaded.remove(101));
    assert!(!loaded.remove(101));
    assert_eq!(loaded.at(102).map(|placed| placed.entity), Some(102));
    assert_eq!(loaded.objects().len(), 1);
    loaded.clear();
    assert!(loaded.objects().is_empty() && loaded.at(102).is_none());
}

/// A hit is replayed through the game's damage, on the head when it was a
/// headshot.
#[test]
fn a_hit_is_replayed() {
    use crate::wire::{self, Message};
    use crate::{replay_damage, HitOptions, Pawn, Player, ReplayedHit};
    use std::cell::RefCell;
    use std::rc::Rc;

    let hits = Rc::new(RefCell::new(Vec::new()));
    let record = hits.clone();
    crate::host::fake::answer(move |method, data| {
        let mut out = Vec::new();
        match method {
            "Pawn" => {
                let (slot, _) = request(data);
                let pawn = Pawn {
                    entity: 100 + slot,
                    hero: 13,
                    ..Pawn::default()
                };
                wire::message(&mut out, 1, &pawn);
            }
            "Damage" => record.borrow_mut().push(HitOptions::decode(data).unwrap()),
            _ => {}
        }
        Ok(out)
    });
    let hit = ReplayedHit {
        headshot: true,
        ..ReplayedHit::new(Player::of(1), 50.0).by(Player::of(2))
    };
    assert!(replay_damage(&hit));
    let sent = hits.borrow();
    assert_eq!((sent[0].victim, sent[0].attacker), (101, Some(102)));
    assert_eq!((sent[0].amount, sent[0].hit_group), (50.0, Some(1)));
}

/// A hook reports a new throw once, and its cooldown ends when the mod says.
#[test]
fn a_hook_counts_its_throws() {
    use crate::wire;
    use crate::{Ability, FieldValue, Hook, HookRead, Player, HOOK_ABILITY, HOOK_UPGRADES};
    use std::cell::RefCell;
    use std::rc::Rc;

    let calls = Rc::new(RefCell::new(Vec::new()));
    let record = calls.clone();
    crate::host::fake::answer(move |method, _| {
        record.borrow_mut().push(method.to_owned());
        let mut out = Vec::new();
        if method == "ReadField" {
            wire::nested(&mut out, 1, |out| FieldValue::Number(77.0).encode(out));
        }
        Ok(out)
    });
    let ability = |start: f32, end: f32, upgrades: u32| Ability {
        name: HOOK_ABILITY.to_owned(),
        entity: 9,
        upgrades,
        cooldown_start: start,
        cooldown_end: end,
        ..Ability::default()
    };
    let player = Player::of(1);
    let mut hook = Hook::new(0.0);
    let count = |method: &str| calls.borrow().iter().filter(|call| *call == method).count();

    // A hero without the hook reports nothing.
    assert_eq!(hook.update_from(player, 10.0, &[]), None);

    // A hook off cooldown holds its victim, and is given its upgrades.
    let idle = hook.update_from(player, 10.0, &[ability(0.0, 0.0, 1)]);
    let victim = Some(77);
    assert_eq!(
        idle,
        Some(HookRead {
            victim,
            thrown: false
        })
    );
    assert_eq!(count("SetAbility"), 1);

    // A new cooldown is a throw, counted once, and a zero cooldown ends it.
    let thrown = hook.update_from(player, 10.0, &[ability(9.0, 20.0, HOOK_UPGRADES)]);
    assert_eq!(
        thrown,
        Some(HookRead {
            victim,
            thrown: true
        })
    );
    let again = hook.update_from(player, 11.0, &[ability(9.0, 20.0, HOOK_UPGRADES)]);
    assert_eq!(
        again,
        Some(HookRead {
            victim,
            thrown: false
        })
    );
    assert_eq!(count("RefreshAbility"), 2);
}

/// The floor under a point is where a trace straight down lands.
#[test]
fn the_ground_is_under_a_point() {
    use crate::wire;
    use crate::{ground, Ground, TraceHit};
    use std::cell::Cell;
    use std::rc::Rc;

    let solid = Rc::new(Cell::new(false));
    let inside = solid.clone();
    crate::host::fake::answer(move |_, _| {
        let hit = TraceHit {
            position: Vector::new(5.0, 6.0, 1.0),
            start_solid: inside.get(),
            ..TraceHit::default()
        };
        let mut out = Vec::new();
        wire::message(&mut out, 1, &hit);
        Ok(out)
    });
    let point = Vector::new(5.0, 6.0, 40.0);
    assert_eq!(
        ground(point, Ground::default()),
        Some(Vector::new(5.0, 6.0, 1.0))
    );
    solid.set(true);
    assert_eq!(ground(point, Ground::default()), None);
}

/// A builder aims a preview, drops it as a solid object, and takes it back.
#[test]
fn a_builder_drops_and_undoes() {
    use crate::dropper::{CatalogEntry, Dropper, DROPPER_BUTTONS};
    use crate::wire::{self, Message};
    use crate::{
        Angles, Buttons, Connection, LoadedSpot, ModelOptions, Pawn, Player, PlayersChangedEvent,
        TraceHit,
    };
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    // The game shows a hero looking along +x, a floor where they look, and
    // makes world objects 1, 2, ... with entities 101, 102, ...
    let calls = Rc::new(RefCell::new(Vec::new()));
    let models = Rc::new(RefCell::new(Vec::new()));
    let (record, made, count) = (calls.clone(), models.clone(), Rc::new(Cell::new(0u32)));
    crate::host::fake::answer(move |method, data| {
        record.borrow_mut().push(method.to_owned());
        let mut out = Vec::new();
        match method {
            "Pawn" => {
                let pawn = Pawn {
                    entity: 40,
                    hero: 13,
                    eye_position: Vector::new(0.0, 0.0, 72.0),
                    eye_angles: Angles::level(47.0),
                    ..Pawn::default()
                };
                wire::message(&mut out, 1, &pawn);
            }
            "Trace" => {
                let hit = TraceHit {
                    position: Vector::new(101.0, 203.0, 5.0),
                    normal: Vector::new(0.0, 0.0, 1.0),
                    ..TraceHit::default()
                };
                wire::message(&mut out, 1, &hit);
            }
            "CreateModel" => {
                made.borrow_mut().push(ModelOptions::decode(data).unwrap());
                count.set(count.get() + 1);
                wire::uint32(&mut out, 1, count.get());
            }
            "ObjectEntity" => wire::uint32(&mut out, 1, request(data).0 + 100),
            _ => {}
        }
        Ok(out)
    });
    let events = events!(Idle::default);
    let player = Player::of(1);
    let report = PlayersChangedEvent {
        players: vec![Connection {
            player,
            steam_id: 7,
            ready: true,
            ..Connection::default()
        }],
    };
    crate::tests::send(Idle::default, &events, "PlayersChanged", &report);
    let called = |method: &str| calls.borrow().iter().filter(|call| *call == method).count();

    // Building takes the build keys from the hero and shows the catalog.
    let catalog = vec![
        CatalogEntry::new("Crates", "Crate", "models/crate.vmdl"),
        CatalogEntry {
            bounce: 900.0,
            ..CatalogEntry::new("Pads", "Pad", "models/pad.vmdl")
        },
    ];
    let spot = LoadedSpot::new("dl_midtown");
    let mut dropper: Dropper<Idle> = Dropper::new(catalog, spot, |_| None);
    dropper.toggle(player);
    assert!(dropper.building(player));
    assert_eq!((called("BlockPlayerInput"), called("Ui")), (1, 1));
    assert_ne!(DROPPER_BUTTONS & Buttons::ATTACK, 0);

    // The preview follows the aim: on the floor's grid, facing the hero's
    // yaw snapped to a turn step.
    dropper.frame(&[]);
    let preview = models.borrow()[0].clone();
    assert_eq!(preview.position, Vector::new(104.0, 200.0, 5.0));
    assert_eq!(preview.facing.map(|facing| facing.yaw), Some(45.0));
    assert_eq!((preview.solid, preview.glow), (None, Some(true)));
    dropper.frame(&[]);
    assert_eq!((called("CreateModel"), called("MoveObject")), (1, 1));

    // Fire drops the object as a solid model, and the mode hears of it once.
    dropper.select(player, 1);
    dropper.frame(&[]);
    dropper.input(player, Buttons::ATTACK);
    let dropped = models.borrow().last().cloned().unwrap();
    assert_eq!(
        (dropped.resource.as_str(), dropped.solid),
        ("models/pad.vmdl", Some(true))
    );
    let saved = dropper.take_changed().expect("the spot changed");
    assert_eq!((saved.objects.len(), saved.objects[0].bounce), (1, 900.0));
    assert_eq!(saved.objects[0].placed_by, 7);
    assert!(dropper.take_changed().is_none());

    // Undo takes the drop back, and a second toggle ends the build.
    dropper.undo(player);
    assert!(dropper.spot.objects().is_empty());
    assert!(dropper
        .take_changed()
        .is_some_and(|spot| spot.objects.is_empty()));
    dropper.toggle(player);
    assert!(!dropper.building(player));
    assert_eq!(called("BlockPlayerInput"), 2);
}

/// A gate stands its window and its band, shows them to its viewer, and
/// takes every part down.
#[test]
fn a_race_gate_stands_and_comes_down() {
    use crate::racegate::{RaceGate, RaceGateOptions};
    use crate::wire::{self, Message};
    use crate::TextOptions;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    // The game makes every part it is asked for.
    let calls = Rc::new(RefCell::new(Vec::new()));
    let texts = Rc::new(RefCell::new(Vec::new()));
    let (record, written, made) = (calls.clone(), texts.clone(), Rc::new(Cell::new(0u32)));
    crate::host::fake::answer(move |method, data| {
        record.borrow_mut().push(method.to_owned());
        let mut out = Vec::new();
        match method {
            "CreateText" => {
                written
                    .borrow_mut()
                    .push(TextOptions::decode(data).unwrap());
                made.set(made.get() + 1);
                wire::uint32(&mut out, 1, made.get());
            }
            "CreateParticle" | "CreateEntity" => {
                made.set(made.get() + 1);
                wire::uint32(&mut out, 1, made.get());
            }
            "ObjectEntity" => wire::uint32(&mut out, 1, request(data).0 + 100),
            _ => {}
        }
        Ok(out)
    });
    let called = |method: &str| calls.borrow().iter().filter(|call| *call == method).count();

    // Two outlines of four beams between four named points each, and a band
    // of six columns and three rows, each drawn four times.
    let options = RaceGateOptions {
        viewer: Some(2),
        ..RaceGateOptions::at(Vector::new(100.0, 0.0, 10.0), 90.0)
    };
    let mut gate = RaceGate::stand(&options);
    assert_eq!(called("CreateParticle"), 8);
    assert_eq!(called("CreateEntity"), 16);
    assert_eq!(called("FireInput"), 8);
    assert_eq!(called("CreateText"), 36);
    let first = texts.borrow()[0].clone();
    assert_eq!(first.text.chars().filter(|glyph| *glyph == '■').count(), 10);
    assert_eq!(first.color, Some(0x1470_7aff));

    // Every part is shown to the one viewer, and removed with the gate.
    assert_eq!(called("ShowObjectOnlyTo") + called("ShowOnlyTo"), 44);
    assert_eq!(called("ShowEntityOnlyTo"), 16);
    gate.remove();
    assert_eq!(called("RemoveEntity"), 16);
    assert_eq!(called("RemoveObject") + called("Remove"), 44);
}

/// Started holds the service calls the mod started: each call's method and
/// payload, numbered from 1.
type Started = std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<u8>)>>>;

/// service_host makes the host start every service call the mod asks for,
/// and answer the Bind of the score service with a proof.
fn service_host() -> Started {
    use crate::wire::{self, Message};
    let started = Started::default();
    let record = started.clone();
    crate::host::fake::answer(move |method, data| {
        let mut out = Vec::new();
        let (mut name, mut payload) = (String::new(), Vec::new());
        let mut r = wire::Reader::new(data);
        while let Some((number, kind)) = r.field().unwrap() {
            match (number, kind) {
                (2, wire::LEN) => name = r.string(kind).unwrap(),
                (3, wire::LEN) => payload = r.bytes(kind).unwrap().to_vec(),
                _ => r.skip(kind).unwrap(),
            }
        }
        match method {
            "StartServiceCall" => {
                record.borrow_mut().push((name, payload));
                wire::uint64(&mut out, 1, record.borrow().len() as u64);
            }
            "CallService" => {
                let mut bound = crate::ScoreConnection::decode(&payload).unwrap();
                bound.proof = "proof".to_owned();
                let mut reply = Vec::new();
                bound.encode(&mut reply);
                wire::bytes(&mut out, 1, &reply);
            }
            _ => {}
        }
        Ok(out)
    });
    started
}

/// A record loads a frame or more after the first read, and an update
/// changes it once the host has answered.
#[test]
fn a_record_loads_and_updates() {
    use crate::persist::{self, PersistChange, PersistOptions, PersistRead, PersistValue};
    use crate::wire;
    use crate::{Connection, Player, PlayersChangedEvent, ServiceRepliedEvent};

    let started = service_host();
    let events = events!(Idle::default);
    let report = PlayersChangedEvent {
        players: vec![Connection {
            player: Player::of(1),
            generation: 4,
            ready: true,
            ..Connection::default()
        }],
    };
    crate::tests::send(Idle::default, &events, "PlayersChanged", &report);
    let player = Player::of(1);
    let reply = |call: u64, payload: Vec<u8>| {
        let replied = ServiceRepliedEvent {
            call,
            payload,
            ..ServiceRepliedEvent::default()
        };
        crate::tests::send(Idle::default, &events, "ServiceReplied", &replied);
    };

    // A record the host sends: roundsWon 2 and the tag cone.
    let record = |rounds: f64| {
        let mut record = Vec::new();
        wire::nested(&mut record, 1, |out| {
            wire::string(out, 1, "roundsWon");
            wire::nested(out, 2, |out| wire::double(out, 1, rounds));
        });
        wire::nested(&mut record, 1, |out| {
            wire::string(out, 1, "unlocks");
            wire::nested(out, 2, |out| {
                wire::nested(out, 3, |out| wire::string(out, 1, "cone"));
            });
        });
        let mut reply = Vec::new();
        wire::bytes(&mut reply, 1, &record);
        reply
    };

    // The first get starts one read and answers Loading until it ends.
    assert_eq!(persist::get(player, "stats"), PersistRead::Loading);
    assert_eq!(persist::get(player, "stats"), PersistRead::Loading);
    assert_eq!(started.borrow().len(), 1);
    assert_eq!(started.borrow()[0].0, "Get");
    reply(1, record(2.0));
    let PersistRead::Loaded(stats) = persist::get(player, "stats") else {
        panic!("the record loaded");
    };
    assert_eq!(stats.number("roundsWon"), 2.0);
    assert_eq!(stats.tags("unlocks"), ["cone"]);
    assert_eq!((stats.number("missing"), stats.text("missing")), (0.0, ""));
    let cone = PersistValue::Tags(vec!["cone".to_owned()]);
    assert_eq!(stats.fields.get("unlocks"), Some(&cone));

    // An add without a key is refused without being sent.
    let won = [("roundsWon", PersistChange::Add(1.0))];
    persist::update(player, "stats", won.clone(), PersistOptions::default());
    assert_eq!(started.borrow().len(), 1);

    // With a key it is sent, and get answers the changed record.
    persist::update(player, "stats", won, PersistOptions::keyed("round-7"));
    assert_eq!(started.borrow()[1].0, "Update");
    let mut updated = record(3.0);
    wire::bool(&mut updated, 2, true);
    reply(2, updated);
    let PersistRead::Loaded(stats) = persist::get(player, "stats") else {
        panic!("the record stays loaded");
    };
    assert_eq!(stats.number("roundsWon"), 3.0);

    // A Player kept after its player left reads as unavailable.
    let gone = Player::of(9);
    assert!(matches!(
        persist::get(gone, "stats"),
        PersistRead::Unavailable(_)
    ));
}

/// A score is reported for an enrolled player, and a refused one can be
/// sent again.
#[test]
fn a_score_is_reported_and_retried() {
    use crate::wire::Message;
    use crate::{
        bind_score_player, read_board, report_score, BoardPage, BoardQuery, Connection, Mod,
        Player, ScoreReport, ServiceFailure, ServiceRepliedEvent, Standing,
    };
    use std::cell::RefCell;

    thread_local! {
        static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }
    /// Mode keeps how its reports ended.
    #[derive(Default)]
    struct Mode;
    impl Mod for Mode {}
    let note = |text: String| SEEN.with(|seen| seen.borrow_mut().push(text));

    let started = service_host();
    let events = events!(Mode::default);
    let frame = |seconds: f64| {
        crate::tests::send(Mode::default, &events, "Frame", &crate::tests::at(seconds));
    };
    let reply = |call: u64, payload: Vec<u8>, error: &str| {
        let replied = ServiceRepliedEvent {
            call,
            payload,
            error: error.to_owned(),
            failure: if error.is_empty() {
                ServiceFailure::default()
            } else {
                ServiceFailure::Unavailable
            },
        };
        crate::tests::send(Mode::default, &events, "ServiceReplied", &replied);
    };
    frame(0.0);

    // The host enrolls the connection with a proof.
    let connection = Connection {
        player: Player::of(3),
        steam_id: 77,
        generation: 2,
        ..Connection::default()
    };
    let scorer = bind_score_player(&connection).expect("the host enrolled the player");
    assert_eq!(
        (scorer.slot, scorer.steam_id, scorer.proof.as_str()),
        (3, 77, "proof")
    );

    // A refused report is kept, and its retry sends the same bytes.
    let report = ScoreReport {
        board: "kills".to_owned(),
        value: 12,
        key: "round-1".to_owned(),
        ..ScoreReport::default()
    };
    report_score(&scorer, report, move |_: &mut Mode, result| match result {
        Ok(()) => note("confirmed".to_owned()),
        Err(refusal) => {
            note(format!("refused: {refusal}"));
            refusal.retry(move |_: &mut Mode, result| {
                note(format!("retried: {}", result.is_ok()));
            });
        }
    });
    let sent = ScoreReport::decode(&started.borrow()[0].1).unwrap();
    assert_eq!((sent.value, sent.connection), (12, Some(scorer.clone())));
    reply(1, Vec::new(), "the service is away");
    assert_eq!(started.borrow().len(), 2);
    assert_eq!(started.borrow()[0], started.borrow()[1]);
    reply(2, vec![1], "");
    let seen = SEEN.with(|seen| seen.borrow().clone());
    assert_eq!(seen, ["refused: the service is away", "retried: true"]);

    // A board reads as its rows, with the viewer named in the query.
    let query = BoardQuery {
        board: "kills".to_owned(),
        limit: 10,
        ..BoardQuery::default()
    };
    read_board(query, Some(&connection), move |_: &mut Mode, page| {
        let rows = page.map(|page| page.rows.len());
        note(format!("rows: {rows:?}"));
    });
    let asked = BoardQuery::decode(&started.borrow()[2].1).unwrap();
    assert_eq!(asked.player, Some(connection.player));
    let page = BoardPage {
        rows: vec![Standing {
            rank: 1,
            value: 12,
            ..Standing::default()
        }],
        ..BoardPage::default()
    };
    let mut body = Vec::new();
    page.encode(&mut body);
    reply(3, body, "");
    assert_eq!(
        SEEN.with(|seen| seen.borrow().last().cloned()),
        Some("rows: Ok(1)".to_owned())
    );
}

/// A probe waits on its step, then reports its verdict once and quits the
/// server.
#[test]
fn a_probe_reaches_a_verdict() {
    use crate::wire;
    use crate::ProbeRun;
    use std::cell::RefCell;
    use std::rc::Rc;

    let lines = Rc::new(RefCell::new(Vec::new()));
    let record = lines.clone();
    crate::host::fake::answer(move |method, data| {
        let mut r = wire::Reader::new(data);
        while let Some((_, kind)) = r.field().unwrap() {
            let text = r.string(kind).unwrap();
            record.borrow_mut().push(format!("{method}: {text}"));
        }
        Ok(Vec::new())
    });

    // A step that waits is logged once, and a pass ends the run.
    let mut run = ProbeRun::new("Duel", 120.0);
    run.tick(10.0, "two fighters", || false);
    run.tick(11.0, "two fighters", || false);
    run.tick(12.0, "two fighters", || true);
    run.tick(13.0, "two fighters", || "never checked");
    assert!(run.finished());
    assert_eq!(
        *lines.borrow(),
        [
            "Log: probe: waiting for two fighters",
            "Log: Duel probe PASS",
            "ServerCommand: quit"
        ]
    );

    // A run past its timeout fails, with the probe's state.
    lines.borrow_mut().clear();
    let mut slow = ProbeRun::new("Duel", 5.0);
    slow.tick(0.0, "a kill", || false);
    slow.tick_described(6.0, "a kill", || false, || "0 kills".to_owned());
    assert_eq!(
        lines.borrow()[1..],
        [
            "Log: probe failed: timed out waiting for a kill: 0 kills",
            "Log: Duel probe FAIL",
            "ServerCommand: quit"
        ]
    );
}

/// The frame clock reports a minute's frames and its costliest systems.
#[test]
fn the_frame_clock_reports_each_minute() {
    use crate::{frame_clock, wire};
    use std::cell::RefCell;
    use std::rc::Rc;

    let lines = Rc::new(RefCell::new(Vec::new()));
    let record = lines.clone();
    crate::host::fake::answer(move |method, data| {
        let mut r = wire::Reader::new(data);
        while let Some((_, kind)) = r.field().unwrap() {
            if method == "Log" {
                record.borrow_mut().push(r.string(kind).unwrap());
            } else {
                r.skip(kind).unwrap();
            }
        }
        Ok(Vec::new())
    });

    // Nothing is logged until a minute of ticks has passed.
    for tick in [1, 2, 3840] {
        frame_clock::begin();
        frame_clock::lap("bots");
        frame_clock::lap("hud");
        assert!(frame_clock::elapsed() >= 0.0);
        frame_clock::end(tick);
    }
    assert!(lines.borrow().is_empty());
    frame_clock::begin();
    frame_clock::lap("bots");
    frame_clock::end(3841);
    let report = lines.borrow()[0].clone();
    assert!(report.starts_with("frame work: p50 "), "{report}");
    assert!(
        report.contains(" ms over 4 frames; mean/max ms: "),
        "{report}"
    );
    assert!(
        report.contains("bots ") && report.contains("hud "),
        "{report}"
    );
}

/// Chat lines show to everyone who may read them, and leave when their time
/// is up.
#[test]
fn chat_lines_show_and_expire() {
    use crate::wire::{self, Message};
    use crate::{
        show_chat_lines, Change, ChatEvent, ChatLines, Connection, Mod, Pawn, Player,
        PlayersChangedEvent, StartEvent, StartResult,
    };
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Talk shows chat lines from its start.
    #[derive(Default)]
    struct Talk;
    impl Mod for Talk {
        fn start(&mut self, _event: &StartEvent) {
            show_chat_lines(ChatLines::default());
        }
    }

    // Player 1 is on team 2 and player 2 on team 3; the host records what
    // each is shown.
    let drawn = Rc::new(RefCell::new(Vec::new()));
    let record = drawn.clone();
    crate::host::fake::answer(move |method, data| {
        let mut out = Vec::new();
        match method {
            "Pawn" => {
                let slot = request(data).0 as i32;
                let pawn = Pawn {
                    hero: 13,
                    team: 1 + slot,
                    ..Pawn::default()
                };
                wire::message(&mut out, 1, &pawn);
            }
            "Ui" => {
                let (mut slot, mut change) = (0, Change::default());
                let mut r = wire::Reader::new(data);
                while let Some((number, kind)) = r.field().unwrap() {
                    match number {
                        1 => slot = r.int32(kind).unwrap(),
                        2 => change = Change::decode(r.bytes(kind).unwrap()).unwrap(),
                        _ => r.skip(kind).unwrap(),
                    }
                }
                record.borrow_mut().push((slot, change));
            }
            _ => {}
        }
        Ok(out)
    });
    let events = events!(Talk::default);
    let mut reply = Vec::new();
    let mut call = Vec::new();
    wire::string(&mut call, 1, "Start");
    crate::host::serve(&call, &mut reply, Talk::default, &events);

    // A mod that shows chat lines asks the game for its chat.
    let mut r = wire::Reader::new(&reply);
    let mut wants = StartResult::default();
    while let Some((number, kind)) = r.field().unwrap() {
        match number {
            2 => wants = StartResult::decode(r.bytes(kind).unwrap()).unwrap(),
            _ => r.skip(kind).unwrap(),
        }
    }
    assert!(wants.chat);
    let connection = |slot: i32, name: &str| Connection {
        player: Player::of(slot),
        generation: 1,
        ready: true,
        name: name.to_owned(),
        ..Connection::default()
    };
    let report = PlayersChangedEvent {
        players: vec![connection(1, "Ann"), connection(2, "Bob")],
    };
    crate::tests::send(Talk::default, &events, "PlayersChanged", &report);
    let frame = |seconds: f64| {
        crate::tests::send(Talk::default, &events, "Frame", &crate::tests::at(seconds));
    };
    let say = |slot: i32, text: &str, all: bool| {
        let line = ChatEvent {
            player: Player::of(slot),
            text: text.to_owned(),
            all,
            ..ChatEvent::default()
        };
        crate::tests::send(Talk::default, &events, "Chat", &line);
    };
    let texts = |slot: i32| -> Vec<String> {
        let drawn = drawn.borrow();
        let shown = drawn.iter().filter(|(to, _)| *to == slot);
        let nodes = shown.flat_map(|(_, change)| change.set.iter());
        let texts = nodes.filter(|node| !node.text.is_empty());
        texts.map(|node| node.text.clone()).collect()
    };
    frame(1.0);

    // A line for all reaches both players with the next frame; a command
    // reaches nobody.
    say(1, "hello there", true);
    say(1, "/help", true);
    assert!(drawn.borrow().is_empty());
    frame(2.0);
    assert_eq!(texts(1), ["Ann", "hello there"]);
    assert_eq!(texts(2), ["Ann", "hello there"]);

    // A team line reads only to the speaker's team, and is marked as one.
    drawn.borrow_mut().clear();
    say(2, "push left", false);
    frame(3.0);
    assert!(texts(1).is_empty());
    assert_eq!(texts(2), ["Bob (team)", "push left"]);

    // Past their six seconds the lines leave everyone's screen.
    drawn.borrow_mut().clear();
    frame(10.0);
    let removed = |slot: i32| -> usize {
        let drawn = drawn.borrow();
        let shown = drawn.iter().filter(|(to, _)| *to == slot);
        shown.map(|(_, change)| change.removed.len()).sum()
    };
    assert!(removed(1) > 0 && removed(2) > 0);
}

/// The interface module's names, taken whole, leave a console variable's and
/// a field's set as they are.
#[test]
fn the_interface_names_hide_no_set() {
    #[allow(unused_imports)]
    use crate::ui::*;
    crate::host::fake::answer(|_, _| Ok(Vec::new()));
    assert!(crate::console::citadel_trooper_gold_reward().set(40));
}
