//! rotation runs a lobby through a fight in turns: everyone waits in one
//! line, each match seats whoever has played the fewest matches, the sides
//! are split by a session rating so neither stacks, and the fighters go to
//! the back of the line when it ends. Standings keeps each player's record by
//! Steam ID for the session, so leaving and rejoining keeps their place in
//! the order rather than resetting it. Nothing here calls the game: the mod
//! keeps the line and calls these with what it holds, so a test runs them
//! without the game.

use std::collections::HashMap;

/// STARTING_RATING is the rating a player starts the session at.
pub const STARTING_RATING: f64 = 1000.0;

/// RATING_STEP is how far one even match moves each side's rating, unless
/// the mod gives another step.
pub const RATING_STEP: f64 = 32.0;

/// Standing is one player's record for the session.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Standing {
    /// played counts the matches the player was seated for.
    pub played: u32,
    pub rating: f64,
    pub wins: u32,
    pub losses: u32,
    /// streak counts the matches won in a row, zero after a loss.
    pub streak: u32,
}

impl Default for Standing {
    fn default() -> Self {
        Self {
            played: 0,
            rating: STARTING_RATING,
            wins: 0,
            losses: 0,
            streak: 0,
        }
    }
}

/// Record is one player's standing beside what the mod keeps with it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record<Extra = ()> {
    pub standing: Standing,
    /// extra is what the mod keeps for the player, such as the matches
    /// played as each hero.
    pub extra: Extra,
}

/// Standings keeps a record for each Steam ID that joined this session. A
/// mod that keeps more per player names its type as Extra, and each new
/// record starts with that type's default.
#[derive(Clone, Debug, Default)]
pub struct Standings<Extra = ()> {
    by_steam_id: HashMap<u64, Record<Extra>>,
}

impl<Extra: Default> Standings<Extra> {
    /// new returns standings no one has joined.
    pub fn new() -> Self {
        Self {
            by_steam_id: HashMap::new(),
        }
    }

    /// join is steam_id's record, made at the starting rating on their first
    /// join. Its matches played rise to the fewest anyone of present, the
    /// Steam IDs waiting now, has played, so joining late or rejoining never
    /// puts a player ahead of the line.
    pub fn join(
        &mut self,
        steam_id: u64,
        present: impl IntoIterator<Item = u64>,
    ) -> &mut Record<Extra> {
        // Make the record on the first join, then raise it to the line.
        let waiting = present
            .into_iter()
            .filter_map(|id| self.by_steam_id.get(&id));
        let least = fewest_played(waiting.map(|record| &record.standing));
        let record = self.by_steam_id.entry(steam_id).or_insert_with(|| Record {
            standing: Standing {
                played: least,
                ..Standing::default()
            },
            extra: Extra::default(),
        });
        record.standing.played = record.standing.played.max(least);
        record
    }

    /// get is steam_id's record, or None before they join.
    pub fn get(&self, steam_id: u64) -> Option<&Record<Extra>> {
        self.by_steam_id.get(&steam_id)
    }

    /// get_mut is steam_id's record to change, or None before they join.
    pub fn get_mut(&mut self, steam_id: u64) -> Option<&mut Record<Extra>> {
        self.by_steam_id.get_mut(&steam_id)
    }

    /// standing is steam_id's standing, or a new player's before they join.
    pub fn standing(&self, steam_id: u64) -> Standing {
        self.get(steam_id)
            .map_or_else(Standing::default, |record| record.standing)
    }

    /// settle records a finished match between the sides of Steam IDs
    /// winners and losers, as the function settle does, and returns the
    /// rating change. It skips an ID that never joined.
    pub fn settle(&mut self, winners: &[u64], losers: &[u64], step: f64) -> f64 {
        let (mut won, mut lost) = (Vec::new(), Vec::new());
        for (steam_id, record) in &mut self.by_steam_id {
            if winners.contains(steam_id) {
                won.push(&mut record.standing);
            } else if losers.contains(steam_id) {
                lost.push(&mut record.standing);
            }
        }
        settle(won, lost, step)
    }
}

/// fewest_played is the fewest matches any of standings has played, zero
/// with none.
pub fn fewest_played<'a>(standings: impl IntoIterator<Item = &'a Standing>) -> u32 {
    let played = standings.into_iter().map(|standing| standing.played);
    played.min().unwrap_or(0)
}

/// next_match picks two sides of size players from the line: the 2 * size
/// who have played the fewest matches, the earlier in line first among
/// equals, dealt by rating in a snake (first, second, second, first, ...) so
/// the sum of ratings stays close. One a side is the two in rating order; two
/// a side is the best with the worst against the middle two. It returns None
/// while too few wait.
pub fn next_match<T: Clone>(
    line: &[T],
    size: usize,
    standing: impl Fn(&T) -> Standing,
) -> Option<(Vec<T>, Vec<T>)> {
    // The fewest played first; a stable sort keeps line order among equals.
    let mut seated: Vec<&T> = line.iter().collect();
    seated.sort_by_key(|player| standing(player).played);
    seated.truncate(2 * size);
    if size < 1 || seated.len() < 2 * size {
        return None;
    }

    // Deal by rating, best first, turning at each end of the pair of sides.
    seated.sort_by(|a, b| standing(b).rating.total_cmp(&standing(a).rating));
    let (mut first, mut second) = (Vec::new(), Vec::new());
    for (index, player) in seated.into_iter().enumerate() {
        let round = index / 2;
        if (index % 2 == 0) == (round % 2 == 0) {
            first.push(player.clone());
        } else {
            second.push(player.clone());
        }
    }
    Some((first, second))
}

/// after_match is the line once a match ends: everyone who did not fight
/// first, then the fighters, each part in line order.
pub fn after_match<T: Clone + PartialEq>(line: &[T], fought: &[T]) -> Vec<T> {
    let waited = line.iter().filter(|player| !fought.contains(player));
    let fighters = line.iter().filter(|player| fought.contains(player));
    waited.chain(fighters).cloned().collect()
}

/// rating_change is how far each winner's rating rises, and each loser's
/// falls, when a side rated winners beats a side rated losers: the Elo step,
/// larger for an upset.
pub fn rating_change(winners: f64, losers: f64, step: f64) -> f64 {
    let expected = 1.0 / (1.0 + 10f64.powf((losers - winners) / 400.0));
    // Half rounds up, as the other languages' libraries round it.
    (step * (1.0 - expected) + 0.5).floor()
}

/// settle records a finished match: each side's ratings move by the change
/// between the sides' average ratings, and the wins, losses and streaks
/// count it. It returns the rating change, zero with an empty side.
pub fn settle<'a>(
    winners: impl IntoIterator<Item = &'a mut Standing>,
    losers: impl IntoIterator<Item = &'a mut Standing>,
    step: f64,
) -> f64 {
    let winners: Vec<&mut Standing> = winners.into_iter().collect();
    let losers: Vec<&mut Standing> = losers.into_iter().collect();
    let change = if winners.is_empty() || losers.is_empty() {
        0.0
    } else {
        rating_change(average(&winners), average(&losers), step)
    };
    for standing in winners {
        standing.rating += change;
        standing.wins += 1;
        standing.streak += 1;
    }
    for standing in losers {
        standing.rating -= change;
        standing.losses += 1;
        standing.streak = 0;
    }
    change
}

/// average is the mean rating of standings.
fn average(standings: &[&mut Standing]) -> f64 {
    let sum: f64 = standings.iter().map(|standing| standing.rating).sum();
    sum / standings.len() as f64
}

/// least_used is the option to give a player next so each gets an even
/// share: the index of the smallest of counts, or prefer when it ties for
/// smallest. Counting each player's matches as each hero and seating
/// least_used gives everyone every hero about equally.
pub fn least_used(counts: &[u32], prefer: usize) -> usize {
    let Some(least) = counts.iter().min() else {
        return prefer;
    };
    if counts.get(prefer) == Some(least) {
        return prefer;
    }
    counts
        .iter()
        .position(|count| count == least)
        .unwrap_or(prefer)
}

/// leaderboard is the rows a viewer sees: players by wins and then rating,
/// the first rows of them, with viewer kept on the last row when they would
/// fall off the end.
pub fn leaderboard<T: Clone + PartialEq>(
    players: &[T],
    standing: impl Fn(&T) -> Standing,
    rows: usize,
    viewer: Option<&T>,
) -> Vec<T> {
    let mut ranked: Vec<&T> = players.iter().collect();
    ranked.sort_by(|a, b| {
        let (a, b) = (standing(a), standing(b));
        b.wins.cmp(&a.wins).then(b.rating.total_cmp(&a.rating))
    });
    let mut shown: Vec<T> = ranked.into_iter().take(rows).cloned().collect();
    if let (Some(viewer), Some(last)) = (viewer, shown.len().checked_sub(1)) {
        if players.contains(viewer) && !shown.contains(viewer) {
            shown[last] = viewer.clone();
        }
    }
    shown
}
