//! The speaking stack: who the player hears on voice chat, as a column of
//! portraits and names at the left edge of the screen. The game's top bar
//! marks each player it plays a voice for; a mod that hides the top bar loses
//! those marks, and show_speakers puts them back, as show_chat_lines puts
//! back the chat lines the top bar showed.
//!
//! The library draws one hidden row for each player in the game, and the
//! player's game shows a row while it plays that player's voice (an element's
//! speaking property), so voices starting and stopping send nothing. Rows
//! keep slot order, so a stack never reorders as people talk over each other.
//! The stack draws again only when a row's look changes: a player joins,
//! leaves, picks a hero or, through the mod's speaker, changes side.
//!
//! ```ignore
//! fn start(&mut self, _event: &StartEvent) {
//!     show_chat_lines(ChatLines::default());
//!     show_speakers(Speakers::default());
//! }
//! ```

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;

use super::chat::{speaker_look, ChatSpeaker, Look, SpeakerLook, BAND};
use super::element::{image, label, panel, Element};
use super::engine;
use super::style::{percent, px};
use crate::generated::{Align, Face, Flow, Style, Weight};
use crate::runtime::players;
use crate::theme;
use crate::Player;

/// VOICE_ICON is the stock top bar's voice chat icon.
const VOICE_ICON: &str = "file://{images}/icons/voice_chat_icon.psd";

/// RING_BACK is the dark ground inside a speaker's portrait ring.
const RING_BACK: u32 = 0x1c20_1aff;

/// Speakers shape the stack show_speakers draws.
#[derive(Clone, Debug)]
pub struct Speakers {
    /// scale sizes each row, its portrait, name and spacing together, against
    /// the default of 1, at which the portrait is 28 pixels on a 1080 screen.
    pub scale: f64,
    /// speaker says how a viewer sees a row's speaker, as ChatLines' speaker
    /// does; pass the same one so a player looks the same in both.
    pub speaker: Option<SpeakerLook>,
}

impl Default for Speakers {
    fn default() -> Self {
        Self {
            scale: 1.0,
            speaker: None,
        }
    }
}

impl Speakers {
    /// speaker takes the mod's say of how the second player, the viewer,
    /// sees the first, a row's speaker: a method of the mod, such as
    /// Self::seen, or a closure that takes the mod and the two players. It
    /// runs on the mod when the library checks each second whether any row
    /// changed.
    pub fn speaker<M: 'static>(
        mut self,
        look: impl Fn(&mut M, Player, Player) -> ChatSpeaker + 'static,
    ) -> Self {
        self.speaker = Some(SpeakerLook::new(look));
        self
    }
}

/// State holds the options while the mod shows the speaking stack, and each
/// viewer's last drawn looks by slot.
#[derive(Default)]
struct State {
    shown: Option<Speakers>,
    drawn: HashMap<i32, String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

/// show_speakers shows who each player hears on voice chat, at the left edge
/// of their screen, for as long as the mod runs.
pub fn show_speakers(options: Speakers) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.shown = Some(options);
        state.drawn.clear();
    });
    engine::voice_layer(engine::VoiceLayer { refresh, forget });
}

/// forget drops what was drawn for the player who left slot.
fn forget(slot: i32) {
    STATE.with(|state| {
        state.borrow_mut().drawn.remove(&slot);
    });
}

/// Row is one speaker's row as a viewer sees it.
struct Row {
    slot: i32,
    player: Player,
    look: Look,
}

/// rows are the players viewer may hear, as viewer sees them, in slot order.
fn rows(m: &mut dyn Any, viewer: Player, see: Option<&SpeakerLook>) -> Vec<Row> {
    let mut list = Vec::new();
    for connection in players() {
        if connection.bot {
            continue;
        }
        let look = speaker_look(m, connection.player, viewer, see);
        list.push(Row {
            slot: connection.player.slot(),
            player: connection.player,
            look,
        });
    }
    list
}

/// refresh checks whether viewer's speaking stack looks different from the
/// one last drawn for them. When it changed, it notes the new look and
/// returns the new stack element, or None when there is nobody to hear; when
/// unchanged, it returns None.
fn refresh(m: &mut dyn Any, viewer: Player) -> Option<Option<Element>> {
    // Clone options first so the state stays free while the mod's speaker
    // callback runs.
    let shown = STATE.with(|state| state.borrow().shown.clone())?;
    let list = rows(m, viewer, shown.speaker.as_ref());
    let looks: String = list
        .iter()
        .map(|row| {
            format!(
                "{}:{}:{}:{}:{}",
                row.slot,
                row.look.name,
                row.look.color,
                row.look.src.unwrap_or("undefined"),
                row.look.out,
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let unchanged = STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.drawn.get(&viewer.slot()).map(String::as_str) == Some(looks.as_str()) {
            return true;
        }
        state.drawn.insert(viewer.slot(), looks);
        false
    });
    if unchanged {
        return None;
    }
    Some(stack(&list, shown.scale))
}

/// stack draws viewer's stack: a hidden row for each player, which the
/// player's game shows while it plays that player's voice.
fn stack(list: &[Row], scale: f64) -> Option<Element> {
    if list.is_empty() {
        return None;
    }
    let scaled = |n: f64| (n * scale + 0.5).floor().max(1.0) as f32;
    let style = Style {
        flow: Flow::Down,
        horizontal_align: Align::Start,
        vertical_align: Align::Center,
        margin: Some([0.0, 0.0, scaled(160.0), scaled(24.0)].into()),
        ..Style::default()
    };
    let rows: Vec<Element> = list.iter().map(|row| speaker_row(row, scaled)).collect();
    Some(panel().style(style).child(rows).into())
}

/// speaker_row is one row: the portrait in a green ring, the name and the
/// icon.
fn speaker_row(row: &Row, scaled: impl Fn(f64) -> f32) -> Element {
    // The portrait sits in the ring, empty for a hidden or missing hero.
    let face = scaled(28.0);
    let whole = Style {
        width: Some(percent(100)),
        height: Some(percent(100)),
        ..Style::default()
    };
    let picture = row.look.src.map(|src| image().src(src).style(whole));
    let portrait = panel()
        .style(Style {
            width: Some(px(face)),
            height: Some(px(face)),
            vertical_align: Align::Center,
            border_radius: scaled(3.0),
            border_width: 2.0,
            border_color: theme::SUCCESS,
            background: RING_BACK,
            saturation: Some(if row.look.out { 0.0 } else { 1.0 }),
            ..Style::default()
        })
        .child(picture);
    let name = label().text(row.look.name.as_str()).style(Style {
        face: Face::Body,
        weight: Weight::Bold,
        font_size: scaled(16.0),
        color: row.look.color,
        vertical_align: Align::Center,
        margin: Some([0.0, 0.0, 0.0, scaled(8.0)].into()),
        max_width: Some(px(scaled(200.0))),
        ..Style::default()
    });
    let icon = image().src(VOICE_ICON).style(Style {
        width: Some(px(scaled(18.0))),
        height: Some(px(scaled(16.0))),
        vertical_align: Align::Center,
        margin: Some([0.0, 0.0, 0.0, scaled(8.0)].into()),
        tint: theme::SUCCESS,
        ..Style::default()
    });
    panel()
        .key(format!("speaker-{}", row.slot))
        .speaking(row.player)
        .style(Style {
            flow: Flow::Right,
            horizontal_align: Align::Start,
            background: BAND,
            border_radius: scaled(3.0),
            padding: Some([scaled(4.0), scaled(12.0), scaled(4.0), scaled(4.0)].into()),
            margin: Some([0.0, 0.0, scaled(4.0), 0.0].into()),
            ..Style::default()
        })
        .child(portrait)
        .child(name)
        .child(icon)
        .into()
}
