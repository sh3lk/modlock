//! hello-rust is the smallest Modlock mod in Rust: it answers /hello in chat,
//! reports the first server frame, and counts each player's kills in a row.
use std::collections::HashMap;

use modlock::prelude::*;

#[derive(Default)]
struct Hello {
    /// seen records whether the first frame has been reported.
    seen: bool,
    /// streaks holds each player's kills in a row.
    streaks: HashMap<Player, u32>,
}

impl Mod for Hello {
    fn start(&mut self, _event: &StartEvent) {
        command("hello", Self::hello);
        command("streak", Self::streak);
        every(60.0, Self::announce);
    }

    // Report the first server frame once.
    fn frame(&mut self, event: &FrameEvent) {
        if !self.seen {
            self.seen = true;
            log!("first frame at tick {}", event.tick);
        }
    }
}

impl OnKill for Hello {
    // A kill adds to the killer's streak and ends the victim's. A unit has no
    // player, and so no streak.
    fn killed(&mut self, event: &KillEvent) {
        if let Some(victim) = event.victim_player {
            self.streaks.remove(&victim);
        }
        if let Some(attacker) = event.attacker_player {
            *self.streaks.entry(attacker).or_default() += 1;
        }
    }
}

impl Hello {
    /// Greet each player who types /hello.
    fn hello(&mut self, player: Player, name: &str) {
        let name = if name.is_empty() { "you" } else { name };
        player.chat(format!("Hello from Rust, {name}!"));
    }

    fn streak(&mut self, player: Player) {
        let streak = self.streaks.get(&player).copied().unwrap_or(0);
        player.chat(format!("Your streak: {streak}"));
    }

    /// Tell everyone who leads, once a minute.
    fn announce(&mut self) {
        let Some((leader, streak)) = self.streaks.iter().max_by_key(|(_, streak)| **streak) else {
            return;
        };
        let name = leader
            .connection()
            .map_or_else(|| leader.to_string(), |connection| connection.name);
        for connection in players() {
            connection
                .player
                .chat(format!("{name} leads with {streak} in a row."));
        }
    }
}

modlock::run!(Hello::default());
