//! ui draws each player's interface from a tree of elements, written as tags:
//!
//! ```ignore
//! use modlock::prelude::*;
//! use modlock::ui::*;
//!
//! show(player, ui! {
//!     <panel style={{ flow: Flow::Down, padding: 16 }}>
//!         "Round 2"
//!         <button on_press={Self::ready}>"Ready"</button>
//!     </panel>
//! });
//! ```
//!
//! Each tag is the function named for it, and each attribute a method of
//! what the function returns, so the same tree is plain code too:
//!
//! ```ignore
//! show(
//!     player,
//!     panel()
//!         .style(Style { flow: Flow::Down, padding: 16.into(), ..Default::default() })
//!         .child(text("Round 2"))
//!         .child(button().on_press(Self::ready).child(text("Ready"))),
//! );
//! ```
//!
//! A list is anything that yields elements: a Vec, an iterator, or an Option,
//! which holds one or none. It stands between braces among a tag's children.
//!
//! A component is a function marked #[component], whose parameters are its
//! tag's attributes:
//!
//! ```ignore
//! #[component]
//! fn row(name: String, points: u32) -> Element {
//!     ui! {
//!         <panel style={{ flow: Flow::Right }}>
//!             <label text={name} />
//!             <label text={points.to_string()} />
//!         </panel>
//!     }
//! }
//!
//! ui! { <row name="Ann" points={3} /> }
//! ```
//!
//! Each show lays the tree out as flat nodes whose ids stay with their place
//! in the tree, and sends the player only the nodes that changed since the
//! screen they hold. A button's handler runs on the mod with the next server
//! frame, as a command's does.
//!
//! A Style, and each type it holds, is the schema's own, so a color is a
//! number, 0xRRGGBBAA, and a size a Length: px(200), percent(50), FIT or
//! FILL.

mod chat;
mod element;
mod engine;
mod style;

pub use chat::{chat_shown, show_chat_lines, ChatLines, ChatSpeaker};
pub use element::{
    avatar, button, dropdown, entry, hero, image, label, layout, panel, slider, tag, text, toggle,
    Children, Element, Tag,
};
#[doc(hidden)]
pub use element::{elements, slot_of, SlotOf};
pub use element::{Press, Pressed};
pub use engine::{
    ability_bar_presses, ability_display, cursor_free, free_cursor, hide, hide_hud,
    ignore_ability_bar, route, sheet, show, sound, sound_after, toast, toast_for,
};
pub use modlock_macros::{component, ui};
pub use style::{color, gradient, percent, px, rgb, rgba, Set, FILL, FIT};
