//! seating keeps each player the mod seats on the hero and team it asked
//! for, and brings fallen heroes back after the respawn delay, through one
//! Cast it runs with the mod's frames.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::cast::{Bots, Cast, HeroPick, Want};
use crate::generated::Connection;
use crate::Player;

/// Seating is the hero and team Player::seat gives one player.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seating {
    /// hero is the hero: a name, an id, or HeroPick::Any, the default, for
    /// the hero the player has or else a random one.
    pub hero: HeroPick,
    /// team is the team, 2 or 3. None keeps the player's hero's team or joins
    /// the side with fewer heroes.
    pub team: Option<i32>,
}

/// Wish is what one seated player wants, with the generation of the
/// connection it was asked for, so a wish never passes to the next player in
/// the slot.
struct Wish {
    generation: u32,
    want: Want,
}

#[derive(Default)]
struct Seats {
    /// wishes holds each seated player's wish by slot.
    wishes: HashMap<i32, Wish>,
    /// respawn is how many seconds a fallen hero waits, or None for the
    /// game's own time.
    respawn: Option<f64>,
    /// started says whether the mod uses seating: it starts with the first
    /// seat or respawn delay, so a mod that uses neither pays nothing.
    started: bool,
    /// cast seats the players. It is None while it runs a frame.
    cast: Option<Cast<Connection>>,
}

impl Seats {
    /// held returns the wish of the cast's member, or None when the mod did
    /// not seat them.
    fn held(&self, member: &Connection) -> Option<&Wish> {
        self.wishes
            .get(&member.player.slot())
            .filter(|wish| wish.generation == member.generation)
    }

    /// start makes the Cast. Bots are members, so the mod can seat one and
    /// the respawn delay brings theirs back too.
    fn start(&mut self) {
        if !self.started {
            self.started = true;
            self.cast = Some(Cast::new(Bots::Join));
        }
    }
}

thread_local! {
    static SEATS: RefCell<Seats> = RefCell::default();
}

impl Player {
    /// seat gives the player a hero on a team, skipping the game's team and
    /// hero screens. Call it as the player joins, in OnJoin's player_joined.
    ///
    /// The hero appears on a later frame, and OnSpawn hears of it. The
    /// library asks the game again until the player has the seat, and once
    /// more after the player changes hero, so a call before the game is ready
    /// is not lost. Seating the player again replaces the seat, and it ends
    /// when they leave. It returns false when nobody holds the player's slot.
    pub fn seat(self, seating: Seating) -> bool {
        // Find the connection the seat is for.
        let Some(held) = self.connection() else {
            return false;
        };

        // Keep the seat for the Cast to ask for.
        SEATS.with(|seats| {
            let mut seats = seats.borrow_mut();
            let want = Want {
                hero: Some(seating.hero),
                team: seating.team,
                respawn: seats.respawn,
            };
            seats.wishes.insert(
                self.slot(),
                Wish {
                    generation: held.generation,
                    want,
                },
            );
            seats.start();
        });
        true
    }
}

/// set_respawn_delay brings every fallen hero back after seconds, in place of
/// the game's own respawn time. None leaves fallen heroes to the game again.
pub fn set_respawn_delay(seconds: Option<f64>) {
    SEATS.with(|seats| {
        let mut seats = seats.borrow_mut();
        seats.respawn = seconds;
        for wish in seats.wishes.values_mut() {
            wish.want.respawn = seconds;
        }
        seats.start();
    });
}

/// frame runs the Cast with the frame at seconds, once the mod uses seating.
pub(crate) fn frame(seconds: f64) {
    // Take the Cast for the length of the frame, so what it asks of the
    // seats finds them free.
    let Some(mut cast) = SEATS.with(|seats| seats.borrow_mut().cast.take()) else {
        return;
    };
    cast.frame(
        seconds,
        Connection::clone,
        |member, _| {
            SEATS.with(|seats| {
                let mut seats = seats.borrow_mut();
                if seats.held(&member).is_some() {
                    seats.wishes.remove(&member.player.slot());
                }
            })
        },
        |member| {
            SEATS.with(|seats| {
                let seats = seats.borrow();
                match seats.held(member) {
                    Some(wish) => wish.want.clone(),
                    None => Want {
                        respawn: seats.respawn,
                        ..Want::default()
                    },
                }
            })
        },
    );
    SEATS.with(|seats| seats.borrow_mut().cast = Some(cast));
}
