//! theme holds the HUD kit's tokens, each named once: the colors, type scale
//! and sizes the kit's components draw with, and that a mod's own panels can
//! share. They follow the stock HUD's look: VALVEPulp capitals and Retail
//! Demo text in off-white with a dark shadow, team colors and translucent
//! black plates.
//!
//! A color is the number a style holds, 0xRRGGBBAA.

use crate::generated::Face;

/// The tones are the colors a surface may use for meaning. Text, team and
/// danger colors are the stock HUD's offWhite, team1Color, team2Color and
/// colorEnemy.
/// TEXT is the off-white of the HUD's text.
pub const TEXT: u32 = 0xffef_d7ff;
/// MUTED is text that reads under the rest.
pub const MUTED: u32 = 0xbfb3_a1ff;
/// ACCENT marks the player's own choices.
pub const ACCENT: u32 = 0xfd73_6dff;
/// ON_ACCENT is text on an accent fill.
pub const ON_ACCENT: u32 = 0x0707_09ff;
/// AMBER is the Hidden King's team color.
pub const AMBER: u32 = 0xd486_0bff;
/// SAPPHIRE is the Archmother's team color.
pub const SAPPHIRE: u32 = 0x4d75_c3ff;
/// GAS is the gas of a closing arena.
pub const GAS: u32 = 0x3ee0_8fff;
/// SUCCESS marks done, under par, ready and live: a step darker than gas,
/// so status text and tags sit calmer on the plates.
pub const SUCCESS: u32 = 0x36c4_7fff;
/// DANGER marks an enemy or a loss.
pub const DANGER: u32 = 0xff41_0dff;
/// GOLD marks a prize.
pub const GOLD: u32 = 0xebc9_34ff;

/// team_tone returns the color of a game team, by its team number: 2 is the
/// Hidden King's amber, 3 the Archmother's sapphire.
pub fn team_tone(team: i32) -> Option<u32> {
    match team {
        2 => Some(AMBER),
        3 => Some(SAPPHIRE),
        _ => None,
    }
}

/// The shades are the neutrals a full screen draws with, from the stock main
/// menu's base styles, so a full screen reads as part of the game's own
/// menus. Fills are opaque offWhite tints of the offBlack backdrop: Panorama
/// blends translucent fills far stronger than their alpha.
/// QUIET is offWhite at about half strength: idle navigation and
/// secondary text.
pub const QUIET: u32 = 0xffef_d78c;
/// FAINT is offWhite at the popup button's 44%: idle pill borders.
pub const FAINT: u32 = 0xffef_d770;
/// DIM is offWhite at about 40%: counts, placeholders and offline
/// players.
pub const DIM: u32 = 0x6b67_5dff;
/// HAIRLINE is offWhite at 13%: hairlines between sections and plate
/// edges.
pub const HAIRLINE: u32 = 0x2b2a_24ff;
/// INK is offBlack: text on light fills, and the backdrop's hue.
pub const INK: u32 = 0x1013_0dff;
/// BACKDROP is the menu backdrop: an opaque offBlack that hides the stock
/// menu.
pub const BACKDROP: u32 = 0x0b0d_09ff;
/// VEIL is the backdrop at 95%, under a panel the player reads over the
/// game.
pub const VEIL: u32 = 0x0b0d_09f2;
/// PLATE is offWhite at 4%: a plate on the backdrop.
pub const PLATE: u32 = 0x1516_11ff;
/// RAISED is offWhite at 8%: a row or stat inside a plate.
pub const RAISED: u32 = 0x1e1f_19ff;
/// LIT is offWhite at 13%: a hovered or joined row.
pub const LIT: u32 = 0x2b2a_24ff;
/// WHITE is the full-strength fill of a lit primary button.
pub const WHITE: u32 = 0xffff_ffff;
/// CLEAR is no fill or border, where a state turns one off.
pub const CLEAR: u32 = 0x0000_0000;

/// The shared sizes, in pixels: panel buttons and fields share one height,
/// row buttons a shorter one, and every control and tag one corner radius.
/// CONTROL is the height of panel buttons and fields.
pub const CONTROL: f32 = 32.0;
/// ROW_CONTROL is the height of a button inside a list row.
pub const ROW_CONTROL: f32 = 28.0;
/// ROW is the height of one list row.
pub const ROW: f32 = 32.0;
/// RADIUS rounds buttons, fields and tags.
pub const RADIUS: f32 = 4.0;
/// PANEL_RADIUS rounds panels.
pub const PANEL_RADIUS: f32 = 8.0;

/// Role names one step of the HUD type scale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Role {
    /// Display is the largest heading.
    Display,
    /// Title is a heading or a score.
    Title,
    /// Body is running text.
    #[default]
    Body,
    /// Label is a small heading in capitals.
    Label,
    /// Caption is small running text.
    Caption,
}

/// Type is one type role: its face, size in pixels, and whether in capitals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Type {
    pub face: Face,
    pub size: f32,
    pub uppercase: bool,
}

impl Role {
    /// kind returns the role's type, which gives every build the same
    /// typography.
    pub fn kind(self) -> Type {
        let (face, size, uppercase) = match self {
            Role::Display => (Face::Block, 46.0, true),
            Role::Title => (Face::Block, 26.0, true),
            Role::Body => (Face::Body, 17.0, false),
            Role::Label => (Face::Block, 15.0, true),
            Role::Caption => (Face::Body, 14.0, false),
        };
        Type {
            face,
            size,
            uppercase,
        }
    }
}

/// The plate is the translucent plate under HUD text. It stays dark enough
/// for muted captions to read over a bright sky.
/// COLOR is the plate's fill.
pub const PLATE_COLOR: u32 = 0x0000_00e0;
/// RADIUS rounds the plate's corners.
pub const PLATE_RADIUS: f32 = 5.0;
/// PADDING is the plate's vertical and horizontal padding in pixels.
pub const PLATE_PADDING: [f32; 2] = [10.0, 18.0];
/// PILL_EDGE is a pill's resting edge, a faint parchment line.
pub const PILL_EDGE: u32 = 0xffef_d71a;

/// PipShape is a round point, as in the stock round counter, or a narrow
/// tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PipShape {
    #[default]
    Round,
    Tick,
}

/// PipSize is one pip shape's size and corner radius, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PipSize {
    pub width: f32,
    pub height: f32,
    pub radius: f32,
}

impl PipShape {
    /// size returns the shape's size and corner radius.
    pub fn size(self) -> PipSize {
        match self {
            PipShape::Round => PipSize {
                width: 18.0,
                height: 18.0,
                radius: 9.0,
            },
            PipShape::Tick => PipSize {
                width: 7.0,
                height: 18.0,
                radius: 2.0,
            },
        }
    }
}

/// The stock round counter's point borders, and an unearned pip's fill:
/// clear, so only its faint ring shows.
/// EMPTY_RING is an unearned pip's border.
pub const EMPTY_RING: u32 = 0xffef_d72e;
/// FILLED_RING is an earned pip's border.
pub const FILLED_RING: u32 = 0x1013_0de6;
/// EMPTY is an unearned pip's fill.
pub const EMPTY_PIP: u32 = 0x0000_0000;

/// Where the stock HUD's parts end, in pixels on a 1080 high HUD.
/// TEAM_BAR_BOTTOM is where the stock team bar ends; a strip that keeps
/// the bar sits below it.
pub const TEAM_BAR_BOTTOM: f32 = 84.0;
/// OBJECTIVE_BAR_BOTTOM is where the stock objective bar under the team
/// bar ends, its urn and Rift progress; a strip's line goes below it so
/// the bar's labels stay clear.
pub const OBJECTIVE_BAR_BOTTOM: f32 = 220.0;
/// FPS_READOUT_BOTTOM is how far down the top right corner HUD starts:
/// under the game's FPS readout, whose third line, the red slow server
/// frame warning, ends about 62px down.
pub const FPS_READOUT_BOTTOM: f32 = 68.0;

/// HOVER_SOUND and PRESS_SOUND are the game's interface sound events for the
/// kit's controls: the main menu's hover tick and press.
pub const HOVER_SOUND: &str = "UI.MainMenu.Hover";
pub const PRESS_SOUND: &str = "UI.MainMenu.Activate";

/// HOVER_SECONDS is how long a control takes to answer the pointer, and
/// METER_SECONDS how long a meter takes to reach its value: quick enough to
/// feel immediate.
pub const HOVER_SECONDS: f32 = 0.15;
pub const METER_SECONDS: f32 = 0.25;
