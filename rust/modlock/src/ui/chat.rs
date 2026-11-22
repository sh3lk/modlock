//! Chat lines: what players just said, shown as subtitles low in the middle
//! of the screen. The game's top bar shows a fresh chat line under the
//! speaker's portrait; a mod that hides the top bar loses those lines, and
//! show_chat_lines puts them back where the eye already rests: centred a
//! little above the ability bar, clear of the crosshair, newest at the
//! bottom.
//!
//! Each line is the speaker's portrait, their name in their team's color and
//! the line itself, on a slim, translucent band. At most four show; a
//! speaker's next line replaces their last, and older lines dim. A team line
//! reads only to the speaker's team; a line for all reads to everyone. A
//! command, such as /help, is for the mod and is not shown. The lines give
//! way while the player has the game's chat box open, which shows the chat
//! history itself.
//!
//! The interface library draws the lines as their own layer of every
//! player's interface, beside the mod's own tree.
//!
//! ```ignore
//! fn start(&mut self, _event: &StartEvent) {
//!     show_chat_lines(ChatLines::default());
//! }
//! ```
//!
//! A mode whose sides are not the game's teams, or that keeps a hero secret,
//! says how each viewer sees a speaker with a method of the mod:
//!
//! ```ignore
//! fn start(&mut self, _event: &StartEvent) {
//!     show_chat_lines(ChatLines::default().speaker(Self::seen));
//! }
//!
//! fn seen(&mut self, speaker: Player, _viewer: Player) -> ChatSpeaker {
//!     ChatSpeaker {
//!         out: self.found.contains(&speaker),
//!         ..ChatSpeaker::default()
//!     }
//! }
//! ```

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use super::element::{image, label, panel, Element};
use super::engine;
use super::style::{percent, px};
use crate::generated::{
    Align, ChatEvent, Easing, Face, Flow, Style, Transform, Transition, Weight,
};
use crate::heroes::HEROES;
use crate::runtime;
use crate::theme;
use crate::Player;

/// LANE_BOTTOM is the lane's distance from the bottom of a 1080 screen: the
/// ability bar's top, with room for its upgrade pips, at 190, and a gap of
/// 40 above it, so the lines read apart from the bar.
const LANE_BOTTOM: f64 = 230.0;

/// FADES is each line's opacity by its age, newest first.
const FADES: [f32; 3] = [1.0, 0.85, 0.7];

/// AMBER and SAPPHIRE are each game team's name color, its tone lifted to
/// read on the band.
const AMBER: u32 = 0xf0a7_3aff;
const SAPPHIRE: u32 = 0x8fb1_f0ff;

/// BAND is the color of the band a line sits on.
const BAND: u32 = 0x1013_0d9e;

/// ChatLines shape the lines show_chat_lines draws.
#[derive(Clone, Debug)]
pub struct ChatLines {
    /// seconds is how long a line stays up, 6 by default.
    pub seconds: f64,
    /// lines is the most lines shown at once, newest kept, 4 by default.
    pub lines: usize,
    /// bottom is the lane's distance from the bottom of the screen, in
    /// pixels on a 1080 screen; by default it sits a little above the
    /// ability bar.
    pub bottom: f64,
    /// scale sizes each line, its text, portrait, band and spacing together,
    /// against the default of 1: 0.8 draws the text at 13 pixels on a band
    /// at most 448 wide. Sizes round to whole pixels.
    pub scale: f64,
    /// portraits shows each speaker's portrait before their name, as it does
    /// by default.
    pub portraits: bool,
    /// speaker says how a viewer sees a line's speaker, for a mode whose
    /// sides are not the game's teams or that keeps a hero secret; the
    /// speaker method sets it. Without it, and for each part it leaves
    /// unset, a speaker shows their hero, and their name in their game
    /// team's color.
    pub speaker: Option<SpeakerLook>,
}

impl ChatLines {
    /// speaker takes the mod's say of how the second player, the viewer,
    /// sees the first, a line's speaker: a method of the mod, such as
    /// Self::seen, or a closure that takes the mod and the two players. It
    /// runs on the mod each time the lines change: when a line is heard,
    /// when one leaves, and for a player who joins while lines show.
    pub fn speaker<M: 'static>(
        mut self,
        look: impl Fn(&mut M, Player, Player) -> ChatSpeaker + 'static,
    ) -> Self {
        self.speaker = Some(SpeakerLook(Rc::new(move |m, speaker, viewer| {
            match m.downcast_mut::<M>() {
                Some(m) => look(m, speaker, viewer),
                None => ChatSpeaker::default(),
            }
        })));
        self
    }
}

/// Looking runs the mod's say of how a viewer sees a speaker on the mod,
/// with the speaker and the viewer.
type Looking = dyn Fn(&mut dyn Any, Player, Player) -> ChatSpeaker;

/// SpeakerLook is the mod's say of how a viewer sees a line's speaker, as
/// ChatLines keeps it; ChatLines::speaker makes one.
#[derive(Clone)]
pub struct SpeakerLook(Rc<Looking>);

impl std::fmt::Debug for SpeakerLook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeakerLook").finish_non_exhaustive()
    }
}

impl Default for ChatLines {
    fn default() -> Self {
        Self {
            seconds: 6.0,
            lines: 4,
            bottom: LANE_BOTTOM,
            scale: 1.0,
            portraits: true,
            speaker: None,
        }
    }
}

/// ChatSpeaker is how one viewer sees a line's speaker.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatSpeaker {
    /// color is the name's color; a light color reads best.
    pub color: Option<u32>,
    /// hero is the portrait's hero, 0 for an empty square.
    pub hero: Option<u32>,
    /// note follows the name in brackets, such as found.
    pub note: String,
    /// out greys the portrait and the name, as for a player knocked out.
    pub out: bool,
}

/// Line is one player's latest chat line.
struct Line {
    speaker: Player,
    text: String,
    /// team is the speaker's team for a team line, or 0 for a line to all.
    team: i32,
    /// until is when the line leaves, in game seconds.
    until: f64,
}

/// Chat holds the options while the mod shows chat lines, and each
/// speaker's fresh line, oldest first: a speaker's new line goes to the end.
#[derive(Default)]
struct Chat {
    shown: Option<ChatLines>,
    lines: Vec<Line>,
}

thread_local! {
    static CHAT: RefCell<Chat> = RefCell::default();
}

/// show_chat_lines shows chat lines as subtitles above the ability bar, for
/// every player, for as long as the mod runs. Call it in the mod's start, so
/// the server sends the mod its chat.
pub fn show_chat_lines(options: ChatLines) {
    CHAT.with(|chat| chat.borrow_mut().shown = Some(options));
    runtime::hook(|hooks| hooks.chat = Some(heard));
    engine::layer(engine::Layer {
        lines: chat_lines,
        on_mod: speaker_set,
        expire,
        forget,
    });
}

/// chat_shown reports whether the mod shows chat lines.
pub fn chat_shown() -> bool {
    CHAT.with(|chat| chat.borrow().shown.is_some())
}

/// speaker_set reports whether the mod says how a speaker is seen, so the
/// lines read the mod.
fn speaker_set() -> bool {
    CHAT.with(|chat| {
        let shown = &chat.borrow().shown;
        shown.as_ref().is_some_and(|shown| shown.speaker.is_some())
    })
}

/// heard shows a chat line to every player who may read it. It runs on the
/// mod, for the mod's say of each speaker.
fn heard(m: &mut dyn Any, line: &ChatEvent) {
    if hear(line, engine::now()) {
        engine::draw_everyone(m);
    }
}

/// hear keeps line, written at now, and reports whether the lines changed.
fn hear(line: &ChatEvent, now: f64) -> bool {
    // Commands and empty lines are not shown.
    let text = line.text.trim();
    if text.is_empty() || text.starts_with('/') {
        return false;
    }
    let Some(seconds) = CHAT.with(|chat| chat.borrow().shown.as_ref().map(|shown| shown.seconds))
    else {
        return false;
    };

    // A team line reads only to the team the speaker is on as they write
    // it. The speaker's line moves to the end, as the newest.
    let team = if line.all {
        0
    } else {
        line.player.pawn().map_or(0, |pawn| pawn.team)
    };
    CHAT.with(|chat| {
        let lines = &mut chat.borrow_mut().lines;
        lines.retain(|held| held.speaker.slot() != line.player.slot());
        lines.push(Line {
            speaker: line.player,
            text: text.to_owned(),
            team,
            until: now + seconds,
        });
    });
    true
}

/// expire drops the lines past their time and reports whether any went.
fn expire(now: f64) -> bool {
    CHAT.with(|chat| {
        let lines = &mut chat.borrow_mut().lines;
        let before = lines.len();
        lines.retain(|line| now < line.until);
        lines.len() != before
    })
}

/// forget drops the line of the player who left slot, reporting whether one
/// went.
fn forget(slot: i32) -> bool {
    CHAT.with(|chat| {
        let lines = &mut chat.borrow_mut().lines;
        let before = lines.len();
        lines.retain(|line| line.speaker.slot() != slot);
        lines.len() != before
    })
}

/// Said is one line as it is drawn: who said it, what, and to whom.
struct Said {
    speaker: Player,
    text: String,
    team: i32,
}

/// chat_lines draws the fresh lines viewer may read, newest at the bottom,
/// or None with nothing to draw. The mod's say of each speaker runs on m.
fn chat_lines(m: &mut dyn Any, viewer: Player) -> Option<Element> {
    // Nothing to draw is the common case.
    let (shown, held) = CHAT.with(|chat| {
        let chat = chat.borrow();
        let held = chat.lines.iter().map(|line| Said {
            speaker: line.speaker,
            text: line.text.clone(),
            team: line.team,
        });
        (chat.shown.clone(), held.collect::<Vec<Said>>())
    });
    let shown = shown.filter(|_| !held.is_empty())?;

    // Keep the newest lines viewer may read, oldest first.
    let team = viewer.pawn().map_or(0, |pawn| pawn.team);
    let mut readable: Vec<Said> = held
        .into_iter()
        .filter(|line| line.team == 0 || line.team == team)
        .collect();
    if readable.is_empty() {
        return None;
    }
    let kept = readable.split_off(readable.len().saturating_sub(shown.lines));
    let size = Sizes::at(shown.scale, shown.portraits);

    // The lane is centred above the ability bar and gives way to the game's
    // chat box; each line keeps its speaker's key, so a new line adds one
    // subtitle and dims the rest in place. The chat is free while the mod
    // says how a speaker is seen, since its code may show chat lines.
    let count = kept.len();
    let subtitles = kept.iter().enumerate().map(|(at, line)| {
        let look = shown.speaker.as_ref();
        let seen = look.map(|look| (look.0)(m, line.speaker, viewer));
        subtitle(line, seen.unwrap_or_default(), count - 1 - at, &size)
    });
    let lane = Style {
        flow: Flow::Down,
        horizontal_align: Align::Center,
        vertical_align: Align::End,
        margin: [0.0, 0.0, shown.bottom as f32, 0.0].into(),
        ..Style::default()
    };
    let subtitles: Vec<Element> = subtitles.collect();
    Some(
        panel()
            .hide_in_chat(true)
            .style(lane)
            .child(subtitles)
            .into(),
    )
}

/// Sizes are a subtitle's measures at a scale, in pixels on a 1080 screen.
/// At scale 1 the band grows at most 560 wide, centred: its left edge, at
/// 680, stays well clear of the stamina meter, which ends at 530 left of the
/// crosshair. The name and the line share it, and a longer line wraps inside
/// it, in the width left after the padding, the portrait, the name and their
/// gaps.
struct Sizes {
    text: f32,
    name: f32,
    line_height: f32,
    /// portrait is the side of the speaker's square portrait, 0 for none.
    portrait: f32,
    /// name_width is the most of the band a speaker's name takes.
    name_width: f32,
    /// line_width is the most of the band the line takes before it wraps.
    line_width: f32,
    /// pad is the band's padding: top, right, bottom, left.
    pad: [f32; 4],
    /// gap is the space after the portrait, then after the name.
    gap: [f32; 2],
    /// space is the gap between lines.
    space: f32,
    radius: f32,
}

impl Sizes {
    /// at is the subtitle's measures at scale, with or without portraits.
    fn at(scale: f64, portraits: bool) -> Self {
        // Each measure at scale 1, scaled and rounded; without a portrait
        // the band pads its left as its right and drops the portrait's gap.
        let px = |n: f64| (n * scale + 0.5).floor().max(1.0) as f32;
        let portrait = if portraits { px(20.0) } else { 0.0 };
        let left = if portraits { px(3.0) } else { px(8.0) };
        let pad = [px(2.0), px(8.0), px(3.0), left];
        let gap = [if portraits { px(6.0) } else { 0.0 }, px(7.0)];
        let name_width = px(160.0);
        Self {
            text: px(16.0),
            name: px(14.0),
            line_height: px(20.0),
            portrait,
            name_width,
            line_width: px(560.0) - pad[1] - pad[3] - portrait - gap[0] - name_width - gap[1],
            pad,
            gap,
            space: px(3.0),
            radius: px(3.0),
        }
    }
}

/// subtitle is one line as its viewer sees it: the portrait, the name and
/// the text, 16 pixels at scale 1, on a slim, translucent band, dimmer with
/// age.
fn subtitle(line: &Said, seen: ChatSpeaker, age: usize, size: &Sizes) -> Element {
    // The speaker's look: the mod's say, else their hero and team.
    let pawn = line.speaker.pawn();
    let lifted = match pawn.as_ref().map(|pawn| pawn.team) {
        Some(2) => Some(AMBER),
        Some(3) => Some(SAPPHIRE),
        _ => None,
    };
    let color = if seen.out {
        theme::QUIET
    } else {
        seen.color.or(lifted).unwrap_or(theme::TEXT)
    };
    let team = if line.team != 0 { "team" } else { "" };
    let notes = [team, seen.note.as_str()].map(|note| match note {
        "" => String::new(),
        note => format!(" ({note})"),
    });
    let named = line.speaker.connection().map(|held| held.name);
    let name = format!("{}{}", named.unwrap_or_default(), notes.concat());
    let hero = seen.hero.or(pawn.map(|pawn| pawn.hero)).unwrap_or(0);
    let src = HEROES
        .iter()
        .find(|held| held.id == hero)
        .map(|held| held.portrait);

    // The portrait sits on a raised square, empty for a hidden or missing
    // hero.
    let square = Style {
        width: Some(px(size.portrait)),
        height: Some(px(size.portrait)),
        vertical_align: Align::Start,
        border_radius: (size.radius - 1.0).max(1.0),
        background: theme::RAISED,
        saturation: Some(if seen.out { 0.0 } else { 1.0 }),
        ..Style::default()
    };
    let whole = Style {
        width: Some(percent(100)),
        height: Some(percent(100)),
        ..Style::default()
    };
    let picture = src.map(|src| image().src(src).style(whole));
    let portrait = panel().style(square).child(picture);

    // The name and the line, which wraps inside the band.
    let words = |font_size: f32, color: u32, gap: f32, top: f32, width: f32| Style {
        face: Face::Body,
        weight: Weight::Bold,
        vertical_align: Align::Start,
        text_align: Align::Start,
        font_size,
        line_height: size.line_height,
        color,
        margin: [top, 0.0, 0.0, gap].into(),
        max_width: Some(px(width)),
        ..Style::default()
    };
    let speaker =
        label()
            .text(name)
            .style(words(size.name, color, size.gap[0], 1.0, size.name_width));
    let said = label().text(line.text.as_str()).style(words(
        size.text,
        theme::TEXT,
        size.gap[1],
        0.0,
        size.line_width,
    ));

    // The band eases in from below and fades as it leaves.
    let band = Style {
        flow: Flow::Right,
        horizontal_align: Align::Center,
        background: BAND,
        border_radius: size.radius,
        padding: size.pad.into(),
        margin: [0.0, 0.0, size.space, 0.0].into(),
        opacity: Some(FADES[age.min(FADES.len() - 1)]),
        transition: Transition {
            seconds: 0.25,
            easing: Easing::EaseOut,
            ..Transition::default()
        },
        ..Style::default()
    };
    let gone = Style {
        opacity: Some(0.0),
        ..Style::default()
    };
    let enter = Style {
        transform: Transform {
            translate_y: 8.0,
            ..Transform::default()
        },
        ..gone.clone()
    };
    let shown = panel()
        .key(format!("line-{}", line.speaker.slot()))
        .style(band)
        .enter(enter)
        .exit(gone);
    let shown = if size.portrait > 0.0 {
        shown.child(portrait)
    } else {
        shown
    };
    shown.child(speaker).child(said).into()
}
