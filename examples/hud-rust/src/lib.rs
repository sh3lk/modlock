//! rounds exercises the HUD kit of the Rust library in Modlock's own
//! sandbox: a round counter at the top, the players down the corner, and a
//! banner as a round ends.
use modlock::prelude::*;
use modlock::ui::*;
use modlock::{hud, theme};

/// ROUNDS is how many rounds a side wins the match with.
const ROUNDS: usize = 3;

#[derive(Default)]
struct Rounds {
    /// won holds the rounds amber and sapphire have won.
    won: [usize; 2],
    /// winner is the side that won the last round, while its banner shows.
    winner: Option<usize>,
}

impl Mod for Rounds {
    fn start(&mut self, _event: &StartEvent) {
        command("win", Self::win);
    }

    fn players_changed(&mut self, _event: &PlayersChangedEvent) {
        self.draw();
    }
}

impl Rounds {
    /// win gives the round to the side of the player who typed it, and shows
    /// its banner for three seconds.
    fn win(&mut self, player: Player) {
        let side = usize::from(player.pawn().is_some_and(|pawn| pawn.team == 3));
        self.won[side] += 1;
        self.winner = Some(side);
        self.draw();
        after(3.0, |rounds: &mut Rounds| {
            rounds.winner = None;
            rounds.draw();
        });
    }

    /// draw shows every player the HUD.
    fn draw(&self) {
        let [amber, sapphire] = self.won;
        for viewer in players() {
            hide_hud(viewer.player, &[HudPart::TopBar]);
            show(
                viewer.player,
                ui! {
                    <panel style={{ width: FILL, height: FILL }}>
                        <hud::top_strip heading={format!("First to {ROUNDS}")}>
                            <hud::pips filled={amber} total={ROUNDS} tone={theme::AMBER} />
                            <hud::score near={amber} far={sapphire} near_tone={theme::AMBER} far_tone={theme::SAPPHIRE} />
                            <hud::pips filled={sapphire} total={ROUNDS} tone={theme::SAPPHIRE} mirror />
                        </hud::top_strip>
                        {self.winner.map(|side| ui! { <won side={side} amber={amber} sapphire={sapphire} /> })}
                        <hud::roster>
                            {players().into_iter().map(|connection| ui! {
                                <seat connection={connection} viewer={viewer.player} />
                            })}
                        </hud::roster>
                    </panel>
                },
            );
        }
    }
}

/// won announces the side that took the round, with the score under it.
#[component]
fn won(side: usize, amber: usize, sapphire: usize) -> Element {
    let (name, tone) = [("Amber", theme::AMBER), ("Sapphire", theme::SAPPHIRE)][side];
    ui! {
        <hud::banner
            title={format!("{name} takes the round")}
            detail={format!("{amber} to {sapphire}")}
            tone={tone}
        />
    }
}

/// seat is one player's row of the roster, as viewer sees it.
#[component]
fn seat(connection: Connection, viewer: Player) -> Element {
    let hero = connection.player.pawn().map_or(0, |pawn| pawn.hero);
    ui! {
        <hud::row
            name={connection.name}
            own={connection.player == viewer}
            hero={hero}
            steam_id={connection.steam_id}
            activity={if connection.bot { "Bot" } else { "Playing" }}
        />
    }
}

modlock::run!(Rounds::default());
