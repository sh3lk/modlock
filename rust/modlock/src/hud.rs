//! hud draws a mod's HUD in the stock HUD's look, from the tokens in theme.
//! Text sets VALVEPulp capitals over a soft shadow, plates and pills hold
//! text over a bright sky, and pips count as the stock round counter does.
//! The roster lists the players down the top right corner. Actions are the
//! game's menu buttons, with its hover and press sounds, and a meter fills
//! toward its value.
//!
//! ```ignore
//! use modlock::{hud, theme};
//! use modlock::ui::*;
//!
//! show(player, ui! {
//!     <panel>
//!         <hud::top_strip heading="Round 2">
//!             <hud::score near={1} far={0} />
//!         </hud::top_strip>
//!         <hud::roster>
//!             <hud::row name="Ann" hero={13} own />
//!         </hud::roster>
//!     </panel>
//! });
//! ```
//!
//! Each component is a function marked #[component], so a mod writes it as a
//! tag, by its path, with the function's parameters as the tag's attributes.

use crate::generated::{Align, Edges, Face, Flow, Overflow, Style, Transform, Transition};
use crate::heroes::HEROES;
use crate::theme::{self, PipShape, Role};
use crate::ui::{button, component, panel, ui, Element, Press, FILL, FIT};

/// arriving returns style set to ease in from slightly smaller, from the
/// members of from, and to fade out as it goes, over a quarter second, with
/// the styles it enters from and leaves to. A transition the style already
/// holds stays.
fn arriving(style: Style, from: Style) -> (Style, Style, Style) {
    let motion = Transition {
        seconds: 0.25,
        ..Transition::default()
    };
    let style = Style {
        transition: Some(motion),
        ..Style::default()
    }
    .merge(style);
    let gone = Style {
        opacity: Some(0.0),
        ..Style::default()
    };
    (style, gone.clone().merge(from), gone)
}

/// column returns the style of a panel that centers its children from top
/// down, top pixels from its parent's top.
fn column(top: f32) -> Style {
    Style {
        flow: Flow::Down,
        horizontal_align: Align::Center,
        margin: Some(Edges {
            top,
            ..Edges::default()
        }),
        ..Style::default()
    }
}

/// text is one label in a type role, over the soft shadow unless plated.
#[component]
pub fn text(
    /// role is the label's step of the type scale; Body unless set.
    role: Role,
    /// text is what the label reads.
    text: String,
    /// tone colors the text; a Label role is muted and the rest are text.
    tone: Option<u32>,
    /// plated marks text on a plate or pill, which takes no shadow: on the
    /// dark plate a shadow only smears the glyphs.
    plated: bool,
    /// align places the label across; the center unless set.
    align: Option<Align>,
    /// style is drawn over the role's style.
    style: Style,
) -> Element {
    // The role's type sets the face, size and case; a label reads muted.
    let kind = role.kind();
    let fallback = if role == Role::Label {
        theme::MUTED
    } else {
        theme::TEXT
    };
    let own = Style {
        face: kind.face,
        font_size: kind.size,
        uppercase: kind.uppercase,
        shadow: !plated,
        color: tone.unwrap_or(fallback),
        horizontal_align: align.unwrap_or(Align::Center),
        text_align: Align::Center,
        ..Style::default()
    };
    ui! { <label text={text} style={own.merge(style)} /> }
}

/// plate is the dark plate under HUD text, readable over a bright sky.
#[component]
pub fn plate(
    /// flow places the children; down unless set.
    flow: Option<Flow>,
    /// style is drawn over the plate's style.
    style: Style,
    children: Vec<Element>,
) -> Element {
    let own = Style {
        flow: flow.unwrap_or(Flow::Down),
        horizontal_align: Align::Center,
        background: theme::PLATE_COLOR,
        border_radius: theme::PLATE_RADIUS,
        padding: Some(theme::PLATE_PADDING.into()),
        ..Style::default()
    };
    ui! { <panel style={own.merge(style)}>{children}</panel> }
}

/// pill is the plate rounded into a capsule with a faint parchment edge,
/// flowing right: the counter under a top strip's heading.
#[component]
pub fn pill(
    /// flow places the children; right unless set.
    flow: Option<Flow>,
    /// style is drawn over the pill's style.
    style: Style,
    children: Vec<Element>,
) -> Element {
    let own = Style {
        flow: flow.unwrap_or(Flow::Right),
        horizontal_align: Align::Center,
        background: theme::PLATE_COLOR,
        border_radius: 50.0,
        border_width: 2.0,
        border_color: theme::PILL_EDGE,
        padding: Some([3, 6].into()),
        ..Style::default()
    };
    ui! { <panel style={own.merge(style)}>{children}</panel> }
}

/// tag names a status or a result in small heading capitals, in a 1px border
/// of its tone.
#[component]
pub fn tag(
    /// text is what the tag reads.
    text: String,
    /// tone colors the border, and the text or the fill; text unless set.
    tone: Option<u32>,
    /// filled fills the tag, for a result; unset, it is outlined, for a
    /// status read in a list.
    filled: bool,
    /// style is drawn over the tag's style.
    style: Style,
) -> Element {
    let tone = tone.unwrap_or(theme::TEXT);
    let (color, background) = match filled {
        true => (theme::ON_ACCENT, tone),
        false => (tone, theme::CLEAR),
    };
    let own = Style {
        face: Face::Block,
        font_size: 12.0,
        letter_spacing: 1.0,
        uppercase: true,
        color,
        background,
        border_width: 1.0,
        border_color: tone,
        border_radius: theme::RADIUS,
        padding: Some([4, 8].into()),
        vertical_align: Align::Center,
        ..Style::default()
    };
    ui! { <label text={text} style={own.merge(style)} /> }
}

/// banner announces a moment: a centered title in the heading face with a
/// muted detail under it. It grows in as it appears and fades as it goes.
#[component]
pub fn banner(
    /// title is the banner's heading.
    title: String,
    /// detail is the line under the title, if any.
    detail: String,
    /// tone colors the title.
    tone: Option<u32>,
    /// style is drawn over the banner's style.
    style: Style,
) -> Element {
    // The title, and the detail when there is one, keep their places.
    let under = (!detail.is_empty())
        .then(|| ui! { <text role={Role::Caption} text={detail} tone={theme::MUTED} /> });
    let smaller = Transform {
        scale_x: Some(0.92),
        scale_y: Some(0.92),
        ..Transform::default()
    };
    let from = Style {
        transform: Some(smaller),
        ..Style::default()
    };
    let (own, enter, exit) = arriving(column(20.0).merge(style), from);
    ui! {
        <panel style={own} enter={enter} exit={exit}>
            <text role={Role::Title} text={title} tone={tone.unwrap_or(theme::TEXT)} />
            {under}
        </panel>
    }
}

/// score is a counter's points, each side in its tone, in the title face. It
/// sits in a pill, so its text is plated.
#[component]
pub fn score(
    /// near is the near side's points.
    near: i64,
    /// far is the far side's points.
    far: i64,
    /// near_tone colors the near side; the accent unless set.
    near_tone: Option<u32>,
    /// far_tone colors the far side; text unless set.
    far_tone: Option<u32>,
) -> Element {
    // Each side's points, and the colon between them, leave their place
    // across to the row.
    let side = |points: i64, tone: u32| {
        ui! {
            <text
                role={Role::Title}
                text={points.to_string()}
                tone={tone}
                plated
                align={Align::Start}
                style={{ margin: [0, 10] }}
            />
        }
    };
    ui! {
        <panel style={{ flow: Flow::Right, vertical_align: Align::Center }}>
            {side(near, near_tone.unwrap_or(theme::ACCENT))}
            <text role={Role::Title} text=":" tone={theme::MUTED} plated align={Align::Start} />
            {side(far, far_tone.unwrap_or(theme::TEXT))}
        </panel>
    }
}

/// pips shows a count out of a fixed total as filled and empty pips, as the
/// stock round counter does.
#[component]
pub fn pips(
    /// filled is how many pips are lit.
    filled: usize,
    /// total is how many pips show.
    total: usize,
    /// tone fills the lit pips.
    tone: u32,
    /// shape is a round point, as the stock round counter, or a narrow tick;
    /// round unless set.
    shape: PipShape,
    /// mirror fills from the right, for the far side of a counter.
    mirror: bool,
    /// style is drawn over the row's style.
    style: Style,
) -> Element {
    // Each pip is lit from the left, or from the right when mirrored.
    let size = shape.size();
    let pip = |index: usize| {
        let lit = match mirror {
            true => index + filled >= total,
            false => index < filled,
        };
        let (fill, ring) = match lit {
            true => (tone, theme::FILLED_RING),
            false => (theme::EMPTY_PIP, theme::EMPTY_RING),
        };
        ui! {
            <panel
                key={format!("pip-{index}")}
                style={{
                    width: size.width,
                    height: size.height,
                    border_radius: size.radius,
                    margin: [0, 2],
                    background: fill,
                    border_width: 2,
                    border_color: ring,
                }}
            />
        }
    };
    let own = Style {
        flow: Flow::Right,
        vertical_align: Align::Center,
        ..Style::default()
    };
    ui! { <panel style={own.merge(style)}>{(0..total).map(pip)}</panel> }
}

/// meter is a bar that fills from the left toward its value, easing to each
/// new value: a charge, a capture or a timer.
#[component]
pub fn meter(
    /// value is how full the meter is, from 0 to 1.
    value: f32,
    /// tone fills the bar; the accent unless set.
    tone: Option<u32>,
    /// width is the meter's width in pixels; 160 unless set.
    width: Option<f32>,
    /// height is the meter's height in pixels; 8 unless set.
    height: Option<f32>,
    /// style is drawn over the meter's style.
    style: Style,
) -> Element {
    // The fill scales rather than resizes, so it eases without layout work.
    let (width, height) = (width.unwrap_or(160.0), height.unwrap_or(8.0));
    let grown = Transform {
        scale_x: Some(value.clamp(0.0, 1.0)),
        scale_y: Some(1.0),
        origin_x: Some(0.0),
        origin_y: Some(50.0),
        ..Transform::default()
    };
    let fill = Style {
        width: Some(crate::ui::percent(100)),
        height: Some(crate::ui::percent(100)),
        background: tone.unwrap_or(theme::ACCENT),
        transform: Some(grown),
        transition: Some(Transition {
            seconds: theme::METER_SECONDS,
            ..Transition::default()
        }),
        ..Style::default()
    };
    let own = Style {
        width: Some(crate::ui::px(width)),
        height: Some(crate::ui::px(height)),
        background: theme::RAISED,
        border_radius: height / 2.0,
        overflow_x: Overflow::Clip,
        overflow_y: Overflow::Clip,
        ..Style::default()
    };
    ui! {
        <panel style={own.merge(style)}>
            <panel style={fill} />
        </panel>
    }
}

/// action is a button in the game's menu look: capitals on a fill that
/// lights as the pointer enters, with the main menu's hover and press sounds.
#[component]
pub fn action(
    /// text is what the action reads.
    text: String,
    /// primary marks the one action a screen leads with.
    primary: bool,
    /// quiet marks an action that should not draw the eye.
    quiet: bool,
    /// disabled dims the action, which then takes no presses.
    disabled: bool,
    /// on_press runs with the player who pressed the action, as a button's
    /// does.
    on_press: Option<Press>,
    /// style is drawn over the action's style.
    style: Style,
) -> Element {
    // Each kind has its fill, edge and text, idle and hovered.
    let fill = |background| Style {
        background,
        ..Style::default()
    };
    let (idle, hover, color) = match (primary, quiet) {
        (true, _) => (fill(theme::TEXT), fill(theme::WHITE), theme::INK),
        (false, true) => (fill(theme::CLEAR), fill(theme::RAISED), theme::QUIET),
        (false, false) => (
            Style {
                border_width: 1.0,
                border_color: theme::FAINT,
                ..fill(theme::RAISED)
            },
            Style {
                border_color: theme::TEXT,
                ..fill(theme::LIT)
            },
            theme::TEXT,
        ),
    };

    // A disabled action dims, and neither lights nor ticks under the pointer.
    let mut own = Style {
        height: Some(crate::ui::px(theme::CONTROL)),
        padding: Some([0, 14].into()),
        border_radius: theme::RADIUS,
        transition: Some(Transition {
            seconds: theme::HOVER_SECONDS,
            ..Transition::default()
        }),
        ..Style::default()
    }
    .merge(idle);
    let mut held = button().sound(theme::PRESS_SOUND).disabled(disabled);
    if let Some(handler) = on_press {
        held = held.on_press(handler);
    }
    if disabled {
        own.opacity = Some(0.4);
    } else {
        held = held.hover(hover).hover_sound(theme::HOVER_SOUND);
    }
    let reads = ui! {
        <label
            text={text}
            style={{
                face: Face::Block,
                font_size: 16,
                uppercase: true,
                letter_spacing: 1,
                color: color,
                horizontal_align: Align::Center,
                vertical_align: Align::Center,
                no_wrap: true,
            }}
        />
    };
    held.style(own.merge(style)).child(reads).into()
}

/// top_strip is the top block of an arena HUD: a heading in spaced capitals,
/// the counter pill, and a line under it. Under the kept team bar, the
/// heading and pill fit above the stock objective bar and the line goes below
/// that bar. Its children are the counter's content, such as a score and
/// pips.
#[component]
pub fn top_strip(
    /// heading names the mode or round and the lane, over the counter.
    heading: String,
    /// line is a short line under the counter, such as a clock or the news.
    line: String,
    /// line_tone colors the line; text unless set.
    line_tone: Option<u32>,
    /// keep_team_bar seats the strip under the stock team bar, which keeps
    /// its clock, heroes and player slots; unset, it takes the bar's place.
    keep_team_bar: bool,
    children: Vec<Element>,
) -> Element {
    // The heading sits over the counter pill, and the line reads under both.
    let over = (!heading.is_empty()).then(|| {
        ui! { <text role={Role::Label} text={heading} style={{ letter_spacing: 1 }} /> }
    });
    let under = (!line.is_empty()).then(|| {
        ui! {
            <text
                role={Role::Label}
                text={line}
                tone={line_tone.unwrap_or(theme::TEXT)}
                style={{ margin: [4, 0, 0, 0], font_size: 18, letter_spacing: 1 }}
            />
        }
    });
    let counter = ui! { <pill style={{ margin: [4, 0, 0, 0] }}>{children}</pill> };

    // In the bar's place, one column holds them all; under the kept bar, the
    // line has its own below the objective bar.
    if !keep_team_bar {
        return ui! { <panel style={column(14.0)}>{over}{counter}{under}</panel> };
    }
    let below =
        under.map(|line| ui! { <panel style={column(theme::OBJECTIVE_BAR_BOTTOM)}>{line}</panel> });
    ui! {
        <panel style={{ width: FILL, height: FILL }}>
            <panel style={column(theme::TEAM_BAR_BOTTOM)}>{over}{counter}</panel>
            {below}
        </panel>
    }
}

/// ROSTER_ROWS is the most players the roster shows, above the minimap.
pub const ROSTER_ROWS: usize = 6;

/// ROSTER_NAME is the most characters a roster name shows.
const ROSTER_NAME: usize = 24;

/// PORTRAIT is the portrait square's side, its 1px edge included, and BADGE
/// the side of the avatar badge on a portrait's corner.
const PORTRAIT: f32 = 44.0;
const BADGE: f32 = 18.0;

/// row is one player as the roster shows them: name and activity, then the
/// portrait, whose edge reads in the accent on the viewing player's own row.
#[component]
pub fn row(
    /// name is the player's name.
    name: String,
    /// own marks the viewing player's own row.
    own: bool,
    /// hero is the player's hero as the pawn reports it, or 0 without one.
    hero: u32,
    /// activity is the line under the name, such as Free roam.
    activity: String,
    /// steam_id badges the portrait's corner with the player's Steam avatar;
    /// 0 shows none.
    steam_id: u64,
    /// selected edges the portrait in the accent, as for the row a card
    /// shows.
    selected: bool,
    /// on_press makes the row a button the viewer presses while they have a
    /// cursor, such as to open a card about that player.
    on_press: Option<Press>,
) -> Element {
    // The name and its line align right, against the portrait.
    let right = Style {
        horizontal_align: Align::End,
        text_align: Align::End,
        ..Style::default()
    };
    let under = Style {
        margin: Some(Edges {
            top: 1.0,
            ..Edges::default()
        }),
        ..right.clone()
    };
    let words = ui! {
        <panel style={{ flow: Flow::Down, vertical_align: Align::Center, margin: [0, 12, 0, 0] }}>
            <text role={Role::Label} text={clip(&name.to_uppercase(), ROSTER_NAME)} tone={theme::TEXT} style={right} />
            <text role={Role::Caption} text={activity} tone={theme::MUTED} style={under} />
        </panel>
    };

    // The portrait sits in a 1px edge on a raised square, empty without a
    // hero, with the player's avatar on its lower left corner.
    let picture = HEROES.iter().find(|known| known.id == hero).map(|known| {
        ui! { <image src={known.portrait} style={{ width: crate::ui::percent(100), height: crate::ui::percent(100) }} /> }
    });
    let badge = (steam_id != 0).then(|| {
        ui! {
            <avatar
                steam_id={steam_id}
                style={{
                    width: BADGE,
                    height: BADGE,
                    horizontal_align: Align::Start,
                    vertical_align: Align::End,
                    border_radius: BADGE / 2.0,
                    border_width: 1,
                    border_color: theme::DIM,
                }}
            />
        }
    });
    let edge = if own || selected {
        theme::ACCENT
    } else {
        theme::DIM
    };
    let portrait = ui! {
        <panel style={{ width: PORTRAIT, height: PORTRAIT, padding: 1, vertical_align: Align::Center, background: edge }}>
            <panel style={{ width: crate::ui::percent(100), height: crate::ui::percent(100), background: theme::RAISED }}>
                {picture}
                {badge}
            </panel>
        </panel>
    };

    // A player who joins slides in from the edge. A pressable row is a button
    // whose ground lifts under the pointer.
    let lying = Style {
        flow: Flow::Right,
        horizontal_align: Align::End,
        margin: Some(Edges {
            bottom: 8.0,
            ..Edges::default()
        }),
        ..Style::default()
    };
    let slid = Transform {
        translate_x: 16.0,
        ..Transform::default()
    };
    let from = Style {
        transform: Some(slid),
        ..Style::default()
    };
    let Some(handler) = on_press else {
        let (style, enter, exit) = arriving(lying, from);
        return ui! { <panel style={style} enter={enter} exit={exit}>{words}{portrait}</panel> };
    };
    let lifting = Style {
        border_radius: theme::RADIUS,
        transition: Some(Transition {
            seconds: theme::HOVER_SECONDS,
            ..Transition::default()
        }),
        ..lying
    };
    let (style, enter, exit) = arriving(lifting, from);
    ui! {
        <button
            style={style}
            hover={{ background: theme::RAISED }}
            enter={enter}
            exit={exit}
            hover_sound={theme::HOVER_SOUND}
            sound={theme::PRESS_SOUND}
            on_press={handler}
        >
            {words}
            {portrait}
        </button>
    }
}

/// roster stacks up to ROSTER_ROWS players down the top right corner, each
/// one's hero portrait at the edge with their name and a line, such as what
/// they are doing or their score, to its left. Its children are its rows,
/// and without one it draws nothing. A mode that hides the stock scoreboard's
/// heroes shows who else is here with it.
#[component]
pub fn roster(
    /// top is the roster's top margin in pixels; under the FPS readout unless
    /// set.
    top: Option<f32>,
    children: Vec<Element>,
) -> Option<Element> {
    // Without a row there is nothing to draw.
    if children.is_empty() {
        return None;
    }

    // Each row keeps its node by its place in the roster.
    let rows = children.into_iter().take(ROSTER_ROWS).enumerate();
    let rows = rows.map(|(index, row)| row.key(format!("row-{index}")));
    let own = Style {
        width: Some(FIT),
        height: Some(FIT),
        flow: Flow::Down,
        horizontal_align: Align::End,
        vertical_align: Align::Start,
        margin: Some(Edges {
            top: top.unwrap_or(theme::FPS_READOUT_BOTTOM),
            right: 24.0,
            ..Edges::default()
        }),
        ..Style::default()
    };
    Some(panel().style(own).child(rows).into())
}

/// roster_key returns what the roster of rows shows, to redraw only when it
/// changes.
pub fn roster_key(rows: &[Row]) -> String {
    // Each row the roster shows is a line: its marks, name, hero, activity
    // and account.
    let line = |row: &Row| {
        let mut line = String::new();
        for (set, mark) in [
            (row.own, '*'),
            (row.selected, '+'),
            (row.on_press.is_some(), '>'),
        ] {
            if set {
                line.push(mark);
            }
        }
        line.push_str(&format!("{}\t{}\t{}\t", row.name, row.hero, row.activity));
        if row.steam_id != 0 {
            line.push_str(&row.steam_id.to_string());
        }
        line
    };
    let lines: Vec<String> = rows.iter().take(ROSTER_ROWS).map(line).collect();
    lines.join("\n")
}

/// clip shortens text to at most width characters, at a word boundary when
/// the cut leaves one, and marks the cut with an ellipsis.
fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let cut: String = text.chars().take(width - 1).collect();
    let cut = match cut.rfind(' ') {
        Some(space) if space > 0 => &cut[..space],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([' ', '\t', '\n']))
}
