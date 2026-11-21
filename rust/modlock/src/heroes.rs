//! heroes lists every released hero and matches the names players type. The
//! table is generated from the Go library's, which follows the game's hero
//! data.

use std::collections::HashMap;
use std::sync::OnceLock;

#[rustfmt::skip]
#[path = "heroes.gen.rs"]
mod table;

pub use table::HEROES;

/// Hero is one released hero: its game name, such as hero_atlas, the title
/// players read and its fixed id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hero {
    /// name is the game's name for the hero, as select_hero takes it.
    pub name: &'static str,
    /// id is the hero's fixed id, as a pawn's hero reports it.
    pub id: u32,
    /// title is the hero's name as players read it.
    pub title: &'static str,
    /// portrait is the hero's small stock portrait, as an image's src.
    pub portrait: &'static str,
}

/// by_name returns each released hero under every name hero_named accepts,
/// reduced as plain reduces them. Game names go in last, so a title that
/// equals another hero's game name could never take it.
fn by_name() -> &'static HashMap<String, &'static Hero> {
    static BY_NAME: OnceLock<HashMap<String, &'static Hero>> = OnceLock::new();
    BY_NAME.get_or_init(|| {
        let mut by_name = HashMap::with_capacity(3 * HEROES.len());
        for hero in &HEROES {
            by_name.insert(plain(hero.title, false), hero);
        }
        for hero in &HEROES {
            let short = hero.name.strip_prefix("hero_").unwrap_or(hero.name);
            by_name.insert(short.to_owned(), hero);
            by_name.insert(hero.name.to_owned(), hero);
        }
        by_name
    })
}

/// hero_named returns the released hero that name names exactly: its game
/// name, such as hero_nano, that name without hero_, such as nano, or its
/// title in any case, with or without spaces and punctuation, such as Calico
/// or ladygeist. Unlike find_hero it matches no part of a name, so a
/// misspelled name finds no hero rather than another one.
pub fn hero_named(name: &str) -> Option<&'static Hero> {
    by_name().get(&plain(name, true)).copied()
}

/// find_hero matches what a player typed, such as geist, Lady Geist or
/// hero_ghost, to a hero: an exact game name or title first, then the first
/// title that starts with or contains it.
pub fn find_hero(typed: &str) -> Option<&'static Hero> {
    // Reduce what was typed to letters, digits and underscores.
    let key = plain(typed, true);
    if key.is_empty() {
        return None;
    }

    // Compare it with each name and plain title, the closest match first.
    let titles: Vec<String> = HEROES.iter().map(|hero| plain(hero.title, false)).collect();
    let heroes = || HEROES.iter().zip(&titles);
    heroes()
        .find(|(hero, title)| hero.name == key || **title == key)
        .or_else(|| heroes().find(|(_, title)| title.starts_with(&key)))
        .or_else(|| heroes().find(|(_, title)| title.contains(&key)))
        .map(|(hero, _)| hero)
}

/// plain reduces text to its lowercase ASCII letters and digits, and its
/// underscores when underscores is true.
fn plain(text: &str, underscores: bool) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || (*c == '_' && underscores))
        .collect()
}

/// hero_name returns the game's name of the hero name names, as hero_named
/// finds it, or name itself when it names no released hero, so the host's
/// error still reports it. Every generated call that takes a hero resolves it
/// through hero_name.
pub(crate) fn hero_name(name: &str) -> &str {
    hero_named(name).map_or(name, |hero| hero.name)
}
