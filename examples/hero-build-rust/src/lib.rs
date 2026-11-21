//! hero-build-rust reads a hero's recommended build from the host's game
//! data. Players type /build hero_inferno to see its categories and items.
use modlock::prelude::*;

/// Builds is the mod: it answers /build.
#[derive(Default)]
struct Builds;

impl Mod for Builds {
    fn start(&mut self, _event: &StartEvent) {
        command("build", Self::build);
    }
}

impl Builds {
    /// build writes the build of the hero the player named, or Infernus's,
    /// in their chat: one line for each category.
    fn build(&mut self, player: Player, args: &str) {
        let hero = if args.is_empty() {
            "hero_inferno"
        } else {
            args
        };
        let categories = hero_build(hero);
        if categories.is_empty() {
            player.chat(format!("No build for {hero}."));
            return;
        }
        for category in categories {
            let names: Vec<String> = category.items.into_iter().map(|item| item.item).collect();
            player.chat(format!("{}: {}", category.name, names.join(", ")));
        }
    }
}

modlock::run!(Builds);
