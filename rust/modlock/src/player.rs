use std::fmt;

/// Player addresses one connected player by their server slot. Two Players
/// are equal only when they name the same connection: a person who takes a
/// slot another left gets a different Player, so a map keyed by Player never
/// hands the newcomer the entry of the one who left. Player::of makes one;
/// the default Player names nobody.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Player {
    slot: i32,
    /// tenure is one more than the number of connections the slot had lost
    /// when the Player was made, so it is zero only in the default Player.
    tenure: u32,
}

impl Player {
    /// held returns the Player of the tenure of slot.
    pub(crate) fn held(slot: i32, tenure: u32) -> Self {
        Player { slot, tenure }
    }

    /// slot identifies the player on the server.
    pub fn slot(self) -> i32 {
        self.slot
    }
}

impl fmt::Display for Player {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "player {}", self.slot)
    }
}
