//! cast keeps a mode's players on the heroes and teams they should have. A
//! mode keeps a Cast of its own members; the library seats players with one
//! too, for Player::seat and set_respawn_delay.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::generated::{log_line, server_command, Connection, Pawn, SelectHero};
use crate::heroes::{hero_named, HEROES};
use crate::runtime::{reported, reports};
use crate::Player;

/// CHANGE_SECONDS is how long Cast leaves a member without a hero after they
/// had one, or after they joined when their hero is free. The game's own
/// Change Hero removes the hero before it spawns the new one, and seating the
/// member in that gap would override their pick.
const CHANGE_SECONDS: f64 = 3.0;

/// HeroPick is the hero a player should have.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HeroPick {
    /// Any keeps the hero the player has, or else seats them on a random
    /// released hero.
    #[default]
    Any,
    /// Named is a hero by its name as players know it or as the game does,
    /// such as haze or hero_haze.
    Named(String),
    /// Id is a hero by its fixed id.
    Id(u32),
}

/// ANY_HERO is the name that stands for HeroPick::Any, as the other
/// languages' libraries spell it.
const ANY_HERO: &str = "any";

impl From<&str> for HeroPick {
    fn from(name: &str) -> Self {
        if name == ANY_HERO {
            return HeroPick::Any;
        }
        HeroPick::Named(name.to_owned())
    }
}

impl From<String> for HeroPick {
    fn from(name: String) -> Self {
        if name == ANY_HERO {
            return HeroPick::Any;
        }
        HeroPick::Named(name)
    }
}

impl From<u32> for HeroPick {
    fn from(id: u32) -> Self {
        HeroPick::Id(id)
    }
}

/// Want is what a member's hero should be; each member it leaves at None is
/// free.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Want {
    /// hero seats the member on a hero.
    pub hero: Option<HeroPick>,
    /// team is the team, 2 or 3, to seat the member on. Without one a member
    /// keeps their hero's team, or else joins the side with fewer heroes.
    pub team: Option<i32>,
    /// respawn is how many seconds a fallen hero waits before Cast asks for
    /// it back. None leaves a fallen hero to the game.
    pub respawn: Option<f64>,
}

/// Bots is what a Cast does with a bot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bots {
    /// Skip leaves bots out of the cast.
    #[default]
    Skip,
    /// Join makes a bot a member as a human is.
    Join,
    /// Kick asks the server to remove each bot, once per connection.
    Kick,
}

/// Seat is one member with what Cast last saw of their hero.
struct Seat<T> {
    /// connection is the member's connection as it joined.
    connection: Connection,
    member: T,
    /// pawn is the member's hero this frame, or None.
    pawn: Option<Pawn>,
    /// alive reports whether the hero was standing this frame.
    alive: bool,
    /// fell_at is when Cast last saw the hero fall.
    fell_at: f64,
    /// hero is the member's hero last frame, or 0 without one.
    hero: u32,
    /// hero_lost_at is when the member last went from a hero to none.
    hero_lost_at: f64,
    joined_at: f64,
    /// asked_at is when Cast last asked the game for the want.
    asked_at: f64,
}

impl<T> Seat<T> {
    fn player(&self) -> Player {
        self.connection.player
    }

    /// shown returns the hero and team the member's pawn shows, or zeros
    /// without a pawn.
    fn shown(&self) -> (u32, i32) {
        self.pawn
            .as_ref()
            .map_or((0, 0), |pawn| (pawn.hero, pawn.team))
    }
}

/// Cast keeps one member of type T for each human who finished joining.
/// Members are keyed by slot and connection generation, so a reconnect is a
/// leave and a join. Cast learns of joins and leaves from the game's player
/// reports and settles them on the next frame; it never walks the player list
/// on a frame where nothing changed.
///
/// A member may carry a Want: to sit on a hero and team, and to come back a
/// while after its hero falls. A human whose want leaves the hero free still
/// gets one when the game leaves them on a team without a hero, as its own
/// spawn on joining can, since nothing else would ever seat them. Each frame
/// Cast compares the want with what the member's hero shows, asks the game
/// again at most once a second while they differ, and stops asking once they
/// match. That is a reconciliation loop: a lost or refused ask is only asked
/// again, never reported as a failure.
///
/// A frame costs one pawn read per member.
///
/// A mode keeps the Cast in its mod and runs it from its frame, with what to
/// do as players come and go:
///
/// ```ignore
/// self.cast.frame(
///     event.time_seconds,
///     |connection| Fighter::new(connection),
///     |fighter, _| log!("{} left", fighter.name),
///     |fighter| fighter.want.clone(),
/// );
/// ```
pub struct Cast<T> {
    bots: Bots,
    /// seats holds one seat per member in the order they joined.
    seats: Vec<Seat<T>>,
    /// kicked holds the connection generation each kicked bot's slot had.
    kicked: HashMap<i32, u32>,
    /// reports is the count of player lists the game had reported when Cast
    /// last settled one.
    reports: u64,
    /// random numbers the heroes Cast picks.
    random: RandomState,
    picks: u64,
}

impl<T> Cast<T> {
    /// new returns a Cast that does with bots what bots says. It settles the
    /// player list the game last reported on its first frame.
    pub fn new(bots: Bots) -> Self {
        Cast {
            bots,
            seats: Vec::new(),
            kicked: HashMap::new(),
            reports: 0,
            random: RandomState::new(),
            picks: 0,
        }
    }

    /// frame settles the joins and leaves reported since the last frame,
    /// reads each member's hero, and asks the game once a second for any
    /// want it does not show yet. seconds is the game clock. join makes the
    /// member for a player who finished joining, leave ends a member whose
    /// connection left the slot, and want returns what a member wants now.
    pub fn frame(
        &mut self,
        seconds: f64,
        join: impl FnMut(&Connection) -> T,
        leave: impl FnMut(T, &Connection),
        want: impl Fn(&T) -> Want,
    ) {
        // Settle a player list the game reported since the last frame.
        if self.reports != reports() {
            if let Some((count, players)) = reported() {
                self.reports = count;
                self.settle(&players, seconds, join, leave);
            }
        }

        // Reconcile each member's hero with their want.
        for at in 0..self.seats.len() {
            self.reconcile(at, seconds, &want);
        }
    }

    /// len is the number of members.
    pub fn len(&self) -> usize {
        self.seats.len()
    }

    /// is_empty reports whether the cast has no member.
    pub fn is_empty(&self) -> bool {
        self.seats.is_empty()
    }

    /// members lists the members in the order they joined.
    pub fn members(&self) -> impl Iterator<Item = &T> {
        self.seats.iter().map(|seat| &seat.member)
    }

    /// members_mut lists the members to change, in the order they joined.
    pub fn members_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.seats.iter_mut().map(|seat| &mut seat.member)
    }

    /// get returns the member in player's slot, if any.
    pub fn get(&self, player: Player) -> Option<&T> {
        self.seat_of(player).map(|seat| &seat.member)
    }

    /// get_mut returns the member in player's slot to change, if any.
    pub fn get_mut(&mut self, player: Player) -> Option<&mut T> {
        let seats = self.seats.iter_mut();
        let mut held = seats.filter(|seat| seat.player().slot() == player.slot());
        held.next().map(|seat| &mut seat.member)
    }

    /// pawn returns the member's hero as Cast read it this frame.
    pub fn pawn(&self, player: Player) -> Option<&Pawn> {
        self.seat_of(player).and_then(|seat| seat.pawn.as_ref())
    }

    /// seat_of returns the seat of the member in player's slot.
    fn seat_of(&self, player: Player) -> Option<&Seat<T>> {
        let mut seats = self.seats.iter();
        seats.find(|seat| seat.player().slot() == player.slot())
    }

    /// settle ends the members whose connection left and makes the new ones,
    /// joined at now.
    fn settle(
        &mut self,
        players: &[Connection],
        now: f64,
        mut join: impl FnMut(&Connection) -> T,
        mut leave: impl FnMut(T, &Connection),
    ) {
        // A member leaves when their slot empties or a new connection takes
        // it.
        let present: HashMap<i32, u32> = players
            .iter()
            .map(|connection| (connection.player.slot(), connection.generation))
            .collect();
        let (kept, left): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.seats)
                .into_iter()
                .partition(|seat| {
                    present.get(&seat.player().slot()) == Some(&seat.connection.generation)
                });
        self.seats = kept;
        for seat in left {
            leave(seat.member, &seat.connection);
        }

        // A human who finished joining becomes a member, and a bot as the
        // policy says.
        for connection in players {
            let seated = self
                .seats
                .iter()
                .any(|seat| seat.player() == connection.player);
            if !connection.ready || seated {
                continue;
            }
            if connection.bot && self.bots != Bots::Join {
                if self.bots == Bots::Kick {
                    self.kick(connection);
                }
                continue;
            }
            self.seats.push(Seat {
                connection: connection.clone(),
                member: join(connection),
                pawn: None,
                alive: false,
                fell_at: f64::NEG_INFINITY,
                hero: 0,
                hero_lost_at: f64::NEG_INFINITY,
                joined_at: now,
                asked_at: f64::NEG_INFINITY,
            });
        }
    }

    /// kick asks the server to remove a bot, once per connection.
    fn kick(&mut self, connection: &Connection) {
        // Remember the connection so a later report does not kick it again.
        let slot = connection.player.slot();
        if self.kicked.insert(slot, connection.generation) == Some(connection.generation) {
            return;
        }

        // Ask the server console to remove the bot by name.
        if !server_command(format!("kick \"{}\"", connection.name)) {
            log_line(format!("bot {} not kicked", connection.name));
        }
    }

    /// reconcile reads the hero of the member at index at and asks for the
    /// part of the want it lacks.
    fn reconcile(&mut self, at: usize, now: f64, want: &impl Fn(&T) -> Want) {
        // Note when the hero falls. A failed read counts as no pawn this
        // frame.
        let seat = &mut self.seats[at];
        let pawn = seat.player().pawn();
        let had = pawn.as_ref().map_or(0, |pawn| pawn.hero);
        let alive = had != 0 && pawn.as_ref().is_some_and(|pawn| pawn.health > 0);
        if seat.alive && !alive {
            seat.fell_at = now;
        }
        seat.pawn = pawn;
        seat.alive = alive;

        // Note when a member who had a hero is left without one, so a hero
        // change the game is making finishes before Cast seats them again.
        if had == 0 && seat.hero != 0 {
            seat.hero_lost_at = now;
        }
        seat.hero = had;
        let changing = had == 0 && now - seat.hero_lost_at < CHANGE_SECONDS;

        // Ask at most once a second, first for the seat, then for the hero
        // back.
        if now - seat.asked_at < 1.0 || changing {
            return;
        }
        let want = want(&seat.member);
        let hero = want.hero.clone().or_else(|| stranded(seat, now));
        match hero {
            Some(hero) if !seated(seat, &want, &hero) => {
                seat.asked_at = now;
                self.seat(at, &want, hero);
            }
            _ => {
                let waited = want
                    .respawn
                    .is_some_and(|respawn| respawn > 0.0 && now - seat.fell_at >= respawn);
                if waited && had != 0 && !alive {
                    seat.asked_at = now;
                    seat.player().respawn();
                }
            }
        }
    }

    /// seat asks the game to put the member at index at on hero and their
    /// team. A refused ask is asked again on a later frame.
    fn seat(&mut self, at: usize, want: &Want, hero: HeroPick) {
        // Any hero keeps the one the member has, or else takes a random one.
        let (had, _) = self.seats[at].shown();
        let hero = match hero {
            HeroPick::Any if had != 0 => SelectHero::HeroId(had),
            HeroPick::Any => {
                self.picks += 1;
                let pick = self.random.hash_one(self.picks) as usize % HEROES.len();
                SelectHero::HeroName(HEROES[pick].name.to_owned())
            }
            HeroPick::Named(name) => SelectHero::HeroName(name),
            HeroPick::Id(id) => SelectHero::HeroId(id),
        };

        // Seat the member on the team they want, or else the side that needs
        // them.
        let team = want.team.unwrap_or_else(|| self.side(at));
        self.seats[at].player().select_hero(&hero, team);
    }

    /// side returns the team the member at index at joins: their hero's, or
    /// the one with fewer heroes.
    fn side(&self, at: usize) -> i32 {
        // A hero keeps its team. The game puts each connecting player on a
        // team before they have a hero, so a member without one is counted
        // afresh.
        let (hero, team) = self.seats[at].shown();
        if hero != 0 && (team == 2 || team == 3) {
            return team;
        }

        // Count the seated heroes on each side and join the smaller.
        let on = |side| {
            self.seats
                .iter()
                .enumerate()
                .filter(|(other, seat)| {
                    *other != at && seat.shown().0 != 0 && seat.shown().1 == side
                })
                .count()
        };
        if on(3) < on(2) {
            3
        } else {
            2
        }
    }
}

/// stranded returns Any for a human whose hero is free when the game has left
/// them on a team without one for CHANGE_SECONDS since they joined, and
/// otherwise None, leaving the hero free. Such a pawn never gets a hero on
/// its own, while a player still choosing one has no pawn and a spectator
/// stands on team 1. A bot is left to the game, which seats it.
fn stranded<T>(seat: &Seat<T>, now: f64) -> Option<HeroPick> {
    let (hero, team) = seat.shown();
    let heroless = seat.pawn.is_some() && hero == 0 && (team == 2 || team == 3);
    (heroless && !seat.connection.bot && now - seat.joined_at >= CHANGE_SECONDS)
        .then_some(HeroPick::Any)
}

/// seated reports whether the member's hero shows the hero and team they
/// want.
fn seated<T>(seat: &Seat<T>, want: &Want, hero: &HeroPick) -> bool {
    // A member needs a hero on the team they want before the hero counts.
    let (shown, team) = seat.shown();
    if shown == 0 || want.team.is_some_and(|wanted| wanted != team) {
        return false;
    }

    // A name outside the released heroes, such as one the game added since,
    // is met by whatever hero the game gave for it.
    match hero {
        HeroPick::Any => true,
        HeroPick::Id(id) => shown == *id,
        HeroPick::Named(name) => hero_named(name).is_none_or(|known| shown == known.id),
    }
}
