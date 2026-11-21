//! puppet shows the playback helpers. The first /puppet adds a ghost bot that
//! walks a square starting where the player stands. After that, /puppet
//! pause, play, seek <tick> and rate <n> control its playback, /puppet restore
//! sets the player's hero to 700 health, level 5 and 1000 souls where it
//! stands, and /puppet hit has the puppet hurt the player for 50.
use modlock::prelude::*;

/// SIDE is the side of the square the puppet walks.
const SIDE: f64 = 300.0;

#[derive(Default)]
struct Show {
    /// puppet is the live puppet, absent until /puppet adds one.
    puppet: Option<Puppet>,
}

impl Mod for Show {
    fn start(&mut self, _event: &StartEvent) {
        command("puppet", Self::puppet);
    }
}

impl Show {
    /// puppet adds the puppet on the first call, and after that controls its
    /// playback or the player's hero.
    fn puppet(&mut self, player: Player, args: &str) {
        // Read the verb and the player's hero.
        let (verb, value) = args.split_once(' ').unwrap_or((args, ""));
        let Some(pawn) = player.pawn() else {
            return;
        };

        // Without a puppet, any verb adds one beside the player and starts it.
        let Some(puppet) = &self.puppet else {
            let bot = BotOptions {
                name: "Puppet".to_owned(),
                team: if pawn.team == 3 { 3 } else { 2 },
                hero: Some(BotOptionsHero::HeroName("hero_wraith".to_owned())),
                position: None,
            };
            let options = PuppetOptions {
                ghost: true,
                ..PuppetOptions::default()
            };
            self.puppet = Puppet::spawn(bot, square(pawn.position), options);
            if let Some(puppet) = &self.puppet {
                puppet.play();
            }
            return;
        };

        // Each other verb controls the puppet's playback or the player's hero.
        match verb {
            "pause" => puppet.pause(),
            "play" => puppet.play(),
            "seek" => puppet.seek(value.parse().unwrap_or(0.0)),
            "rate" => puppet.set_rate(value.parse().unwrap_or(1.0)),
            "restore" => {
                let snapshot = HeroSnapshot {
                    health: 700,
                    max_health: 700,
                    level: Some(5),
                    souls: Some(1000),
                    ..HeroSnapshot::at(pawn.position, pawn.eye_angles)
                };
                restore_hero(player, snapshot, |_: &mut Show, error| {
                    log!("restore {}", error.unwrap_or("held"));
                });
            }
            "hit" => {
                replay_damage(&ReplayedHit::new(player, 50.0).by(puppet.player()));
            }
            _ => {}
        }
    }
}

/// square is a track from origin around a square and back to origin, reaching
/// one corner each second.
fn square(origin: Vector) -> Track {
    let offsets = [
        (0.0, 0.0),
        (SIDE, 0.0),
        (SIDE, SIDE),
        (0.0, SIDE),
        (0.0, 0.0),
    ];
    Track {
        step_ticks: 64.0,
        x: offsets
            .iter()
            .map(|(across, _)| origin.x + across)
            .collect(),
        y: offsets.iter().map(|(_, along)| origin.y + along).collect(),
        z: offsets.iter().map(|_| origin.z).collect(),
        yaw: vec![0.0, 90.0, 180.0, 270.0, 0.0],
        pitch: None,
        health: Some(vec![700.0; 5]),
    }
}

modlock::run!(Show::default());
