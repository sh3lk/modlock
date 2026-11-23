//! engine keeps what the mod shows each player and what each player's game
//! holds of it. Each draw lays the player's elements out as flat nodes whose
//! ids stay with their place in the tree, and sends the player only the nodes
//! that changed since the screen they hold.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write;
use std::rc::Rc;

use super::element::{label, panel, Children, Element, Handlers, Slot};
use super::style::FIT;
use crate::generated::{
    read_field, AbilityRow, Align, Change, Connection, Cue, Easing, Edges, FieldType, FieldValue,
    Flow, HudPart, Kind, Node, NodeInput, Style, Transform, Transition, UiInputEvent,
    UiInputEventValue, UiPressEvent,
};
use crate::heroes::HEROES;
use crate::runtime::{self, players, Interface};
use crate::wire;
use crate::{host, Player};

/// MAX_CUES is how many sounds the root keeps, so a player whose screens come
/// slower than the mod plays sounds misses none of the last few.
const MAX_CUES: usize = 8;

/// MAX_NOTICES bounds the stack; a newer notice pushes out the oldest.
const MAX_NOTICES: usize = 4;

/// MAX_DISPLAY bounds the rows ability_display shows.
const MAX_DISPLAY: usize = 64;

/// Bar runs the mod's handler of a click on the game's ability bar.
type Bar = Rc<dyn Fn(&mut dyn Any, Player, usize)>;

/// Notice is one toast, shown until the game clock passes until.
struct Notice {
    /// id numbers the notice, which keys it in the stack.
    id: u64,
    text: String,
    until: f64,
}

/// View is what the mod shows one player: its interface and its notices.
struct View {
    player: Player,
    /// root is the mod's interface: one element, or a list of them, which
    /// is empty while the mod shows none.
    root: Slot,
    /// hud lists the parts of the game's HUD to hide.
    hud: Vec<HudPart>,
    /// cursor is true while the mod frees the player's mouse cursor, and asks
    /// counts the mod's calls that freed it.
    cursor: bool,
    asks: u32,
    /// routes holds each route's name and value as JSON, in the order added.
    routes: Vec<(String, String)>,
    /// cues are the last sounds played, which the renderer plays once each.
    cues: Vec<Cue>,
    /// bar takes clicks on the game's ability bar, while the mod asks for
    /// them.
    bar: Option<Bar>,
    /// display is the abilities shown on Tab's rows and the ability bar.
    display: Vec<AbilityRow>,
    /// notices are the toasts showing, oldest first.
    notices: Vec<Notice>,
}

/// Held is one node of a screen: the node as its element gave it, without
/// its id and children, beside the ids of its children. A draw compares an
/// element with the node at its place as they stand, and copies only a node
/// that changed.
struct Held {
    node: Node,
    children: Vec<Rc<str>>,
}

impl Held {
    /// whole returns the node as the host takes it, under id.
    fn whole(&self, id: &str) -> Node {
        let mut node = self.node.clone();
        node.id = id.to_owned();
        node.children = self.children.iter().map(|id| id.to_string()).collect();
        node
    }
}

/// Screen is what one player's interface holds, as the host last accepted it.
#[derive(Default)]
struct Screen {
    /// nodes maps each node's id to the node, to find what changed, and order
    /// lists the ids in the order laid out, children before parents. A node
    /// that stayed the same from one screen to the next is shared by both.
    nodes: HashMap<Rc<str>, Rc<Held>>,
    order: Vec<Rc<str>>,
    /// ids maps each element's place in the tree to its node's id. A place is
    /// its parent's place, then "/" for the children and ",index" or ",=key"
    /// for one of them or one of a list among them.
    ids: HashMap<Rc<str>, Rc<str>>,
    /// handlers maps each button's, input's and layout's id to its handlers.
    handlers: HashMap<Rc<str>, Handlers>,
    /// next numbers the next new node, so an id is never reused.
    next: u64,
}

/// Library holds the interface library's state: what the mod shows each
/// player, the screens they hold, and the clock and counters they share.
struct Library {
    /// views holds what the mod shows each player, and screens each player's
    /// interface, by slot.
    views: HashMap<i32, View>,
    screens: HashMap<i32, Screen>,
    /// generations holds each slot's connection generation from the last
    /// player list, to drop a view when another player takes the slot.
    generations: HashMap<i32, u32>,
    /// now is the game clock at the last frame, in seconds.
    now: f64,
    next_notice: u64,
    /// next_cue numbers the next sound, so the renderer plays each once.
    next_cue: u32,
    /// sheets lists the mod's style sheets, in the order the mod added them.
    sheets: Vec<String>,
    /// layer draws the chat lines over every player's interface, once the
    /// mod shows them.
    layer: Option<Layer>,
    /// lines holds what each player sees of the layer, by slot, as it was
    /// last laid out on the mod; a player who sees nothing of it has none.
    lines: HashMap<i32, Element>,
}

thread_local! {
    static LIBRARY: RefCell<Library> = RefCell::new(Library {
        views: HashMap::new(),
        screens: HashMap::new(),
        generations: HashMap::new(),
        now: 0.0,
        next_notice: 0,
        next_cue: 1,
        sheets: Vec::new(),
        layer: None,
        lines: HashMap::new(),
    });
}

/// join gives the library its part in the events with the first interface
/// the mod shows. It takes the game clock as it stands, so a notice lasts as
/// long as asked, and notes who holds each slot, so a player who takes a slot
/// later starts with nothing shown.
fn join() {
    let interface = Interface {
        tick,
        press,
        changed,
        forget,
        returned,
    };
    if runtime::join(interface) {
        LIBRARY.with(|library| library.borrow_mut().now = runtime::now());
        if let Some((_, players)) = runtime::reported() {
            note(&players);
        }
    }
}

/// change changes what the mod shows player and sends the player what
/// changed.
fn change(player: Player, edit: impl FnOnce(&mut Library, i32)) -> bool {
    join();
    LIBRARY.with(|library| {
        let mut library = library.borrow_mut();
        let slot = player.slot();
        library.keep(player);
        edit(&mut library, slot);
        library.draw(player)
    })
}

/// Layer is a layer of every player's interface that the library draws
/// itself, beside the mod's own tree: the chat lines. lines draws on the mod
/// what one player sees of it, expire drops what is past its time and forget
/// what the player who left a slot had in it; both report whether any went.
///
/// The layer is laid out only when what it shows changes, inside the event
/// that changed it, where the library holds the mod; every draw between
/// shows the player what was laid out then.
#[derive(Clone, Copy)]
pub(crate) struct Layer {
    pub lines: fn(&mut dyn Any, Player) -> Option<Element>,
    pub expire: fn(f64) -> bool,
    pub forget: fn(i32) -> bool,
}

/// layer gives every player's interface the layer.
pub(crate) fn layer(layer: Layer) {
    join();
    LIBRARY.with(|library| library.borrow_mut().layer = Some(layer));
}

/// now returns the game clock at the last frame, in seconds.
pub(crate) fn now() -> f64 {
    join();
    LIBRARY.with(|library| library.borrow().now)
}

/// draw_everyone lays the layer out on the mod for every player in the game
/// and draws them, as after a change to the layer all of them see.
pub(crate) fn draw_everyone(m: &mut dyn Any) {
    join();
    let everyone: Vec<Player> = players()
        .iter()
        .map(|connection| connection.player)
        .collect();
    lay(m, &everyone);
    LIBRARY.with(|library| library.borrow_mut().everyone());
}

/// lay lays out on the mod what each of players sees of the layer, and keeps
/// it for their draws.
fn lay(m: &mut dyn Any, players: &[Player]) {
    let Some(layer) = LIBRARY.with(|library| library.borrow().layer) else {
        return;
    };

    // The library is free while the layer runs on the mod, since the mod's
    // code may show an interface.
    let laid: Vec<(i32, Option<Element>)> = players
        .iter()
        .map(|player| (player.slot(), (layer.lines)(m, *player)))
        .collect();
    LIBRARY.with(|library| {
        let lines = &mut library.borrow_mut().lines;
        for (slot, seen) in laid {
            match seen {
                Some(seen) => lines.insert(slot, seen),
                None => lines.remove(&slot),
            };
        }
    });
}

impl Library {
    /// keep makes the view of what the mod shows player, unless it has one.
    fn keep(&mut self, player: Player) {
        self.views.entry(player.slot()).or_insert_with(|| View {
            player,
            root: Slot::Many(Vec::new()),
            hud: Vec::new(),
            cursor: false,
            asks: 0,
            routes: Vec::new(),
            cues: Vec::new(),
            bar: None,
            display: Vec::new(),
            notices: Vec::new(),
        });
    }

    /// everyone draws every player in the game, except one who is away,
    /// whose client holds no screen to change.
    fn everyone(&mut self) {
        for connection in players() {
            if !connection.away {
                self.draw(connection.player);
            }
        }
    }

    /// view returns what the mod shows the player in slot, which change made.
    fn view(&mut self, slot: i32) -> &mut View {
        self.views.get_mut(&slot).expect("change made the view")
    }

    /// draw sends player what changed in their interface and notices. A
    /// player the mod has shown nothing yet gets an empty view first, so what
    /// every player sees, such as the chat lines, reaches them too.
    fn draw(&mut self, player: Player) -> bool {
        // Render the player's view against the last screen sent.
        self.keep(player);
        let slot = player.slot();
        let view = self.views.get_mut(&slot).expect("keep made the view");
        let player = view.player;
        let display = display_rows(view);

        // The layer shows what was last laid out on the mod: a draw has no
        // mod to lay it out on, and a player it laid nothing out for sees
        // none of it until it changes.
        let mut lines = self.lines.get_mut(&slot);
        let previous = self.screens.get(&slot);
        let laid = lines.as_deref_mut();
        let next = render(previous, view, &self.sheets, &display, laid);

        // Send the change; a failed send means the player may have left, so
        // send the screen whole.
        let sent = match diff(previous, &next) {
            Some(change) if !send(player, &change) => {
                let whole = render(None, view, &self.sheets, &display, lines);
                let sent = diff(None, &whole).is_some_and(|change| send(player, &change));
                sent.then_some(whole)
            }
            _ => Some(next),
        };

        // Keep the screen the player now holds.
        match sent {
            Some(screen) => {
                self.screens.insert(slot, screen);
                true
            }
            None => {
                self.screens.remove(&slot);
                false
            }
        }
    }
}

/// Layout holds one screen while place lays it out.
struct Layout<'a> {
    /// previous is the screen the player holds, whose ids and unchanged
    /// nodes the layout keeps.
    previous: Option<&'a Screen>,
    /// screen is the screen being laid out.
    screen: Screen,
    /// at is the place of the element being laid out.
    at: String,
}

impl Layout<'_> {
    /// add records under id a node and the ids of its children. A node the
    /// player holds unchanged is kept as it is, not copied.
    fn add(&mut self, id: Rc<str>, node: &Node, children: Vec<Rc<str>>) {
        let held = self.previous.and_then(|previous| previous.nodes.get(&id));
        let held = match held {
            Some(held) if held.children == children && held.node == *node => held.clone(),
            _ => Rc::new(Held {
                node: node.clone(),
                children,
            }),
        };
        self.screen.order.push(id.clone());
        self.screen.nodes.insert(id, held);
    }

    /// id returns the node id for the element at the place: the id the place
    /// had on the screen the player holds, or a new one.
    fn id(&mut self) -> Rc<str> {
        let held = self
            .previous
            .and_then(|previous| previous.ids.get_key_value(self.at.as_str()));
        let (place, id) = match held {
            Some((place, id)) => (place.clone(), id.clone()),
            None => {
                self.screen.next += 1;
                let id = base36(self.screen.next - 1);
                (Rc::from(self.at.as_str()), Rc::from(id))
            }
        };
        self.screen.ids.insert(place, id.clone());
        id
    }

    /// place lays out element at the place and returns its node's id.
    fn place(&mut self, element: &mut Element) -> Rc<str> {
        let id = self.id();

        // A keyed child keeps its place by key, the rest by index. A list
        // among the children is a place of its own, so a list that grows
        // moves no child after it.
        let own = self.at.len();
        self.at.push('/');
        let mut children = Vec::with_capacity(element.children.len());
        for (index, slot) in element.children.iter_mut().enumerate() {
            self.fill(index, slot, &mut children);
        }
        self.at.truncate(own);

        // A disabled element's handlers run nothing, whoever sends its press.
        let live = element.handlers.any() && !element.node.disabled;
        if let Some(NodeInput::Slider(slider)) = &mut element.node.input {
            slider.wheel = live && element.handlers.wheel.is_some();
        }
        if live {
            self.screen
                .handlers
                .insert(id.clone(), element.handlers.clone());
        }
        self.add(id.clone(), &element.node, children);
        id
    }

    /// fill lays out what slot holds as the child at index of the place, and
    /// adds its nodes' ids to children.
    fn fill(&mut self, index: usize, slot: &mut Slot, children: &mut Vec<Rc<str>>) {
        let inside = self.at.len();
        match slot {
            Slot::One(child) => {
                spot(&mut self.at, index, &child.key);
                children.push(self.place(child));
            }
            Slot::Many(list) => {
                spot(&mut self.at, index, "");
                let listed = self.at.len();
                for (index, child) in list.iter_mut().enumerate() {
                    spot(&mut self.at, index, &child.key);
                    children.push(self.place(child));
                    self.at.truncate(listed);
                }
            }
        }
        self.at.truncate(inside);
    }
}

/// spot adds to place the place of the child at index among the children
/// there, which is its key when it has one.
fn spot(place: &mut String, index: usize, key: &str) {
    place.push(',');
    match key {
        "" => {
            let _ = write!(place, "{index}");
        }
        key => {
            place.push('=');
            place.push_str(key);
        }
    }
}

/// base36 returns number in digits and small letters.
fn base36(mut number: u64) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(char::from_digit((number % 36) as u32, 36).unwrap_or('0'));
        number /= 36;
        if number == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

/// render lays view out as nodes, keeping the ids of elements that stayed in
/// place, beside a node for each route.
fn render(
    previous: Option<&Screen>,
    view: &mut View,
    sheets: &[String],
    display: &[AbilityRow],
    lines: Option<&mut Element>,
) -> Screen {
    // The screen is about as large as the one before it.
    let (nodes, handlers, next) = previous.map_or((0, 0, 0), |previous| {
        (previous.nodes.len(), previous.handlers.len(), previous.next)
    });
    let screen = Screen {
        nodes: HashMap::with_capacity(nodes),
        order: Vec::with_capacity(nodes),
        ids: HashMap::with_capacity(nodes),
        handlers: HashMap::with_capacity(handlers),
        next,
    };
    let mut layout = Layout {
        previous,
        screen,
        at: String::new(),
    };

    // The elements, the chat lines and the notices hang from the root.
    let mut children = Vec::new();
    layout.at.push('/');
    layout.fill(0, &mut view.root, &mut children);
    layout.at.clear();
    if let Some(lines) = lines {
        layout.at.push_str("/,2");
        children.push(layout.place(lines));
        layout.at.clear();
    }
    if !view.notices.is_empty() {
        layout.at.push_str("/,1");
        children.push(layout.place(&mut notices(&view.notices)));
        layout.at.clear();
    }

    // The root is the full-screen panel the elements hang from, and it lists
    // the parts of the game's HUD to hide, whether the cursor is free, the
    // mod's sheets and its last sounds.
    let root = Node {
        kind: Kind::Panel,
        hide_hud: view.hud.clone(),
        free_cursor: view.cursor,
        free_cursor_asks: view.asks,
        sheets: sheets.to_vec(),
        cues: view.cues.clone(),
        ability_bar_presses: view.bar.is_some(),
        ability_display: display.to_vec(),
        ..Node::default()
    };
    layout.add(Rc::from(""), &root, children);

    // A route's id holds a colon, which no element's id does.
    for (name, data) in &view.routes {
        let route = Node {
            kind: Kind::Route,
            route: name.clone(),
            data: data.clone(),
            ..Node::default()
        };
        layout.add(Rc::from(format!("route:{name}")), &route, Vec::new());
    }
    layout.screen
}

/// diff returns the change from previous to next, or None when nothing
/// changed.
fn diff(previous: Option<&Screen>, next: &Screen) -> Option<Change> {
    // Send each node that is not the one the player holds, and remove each
    // node gone.
    let held = |id: &Rc<str>| previous.and_then(|previous| previous.nodes.get(id));
    let set: Vec<Node> = next
        .order
        .iter()
        .filter_map(|id| Some((id, next.nodes.get(id)?)))
        .filter(|(id, node)| !held(id).is_some_and(|held| Rc::ptr_eq(held, node)))
        .map(|(id, node)| node.whole(id))
        .collect();
    let removed: Vec<String> = previous
        .map(|previous| {
            let gone = |id: &&Rc<str>| !next.nodes.contains_key(*id);
            let ids = previous.order.iter().filter(gone);
            ids.map(|id| id.to_string()).collect()
        })
        .unwrap_or_default();
    if previous.is_some() && set.is_empty() && removed.is_empty() {
        return None;
    }
    Some(Change {
        reset: previous.is_none(),
        set,
        removed,
        ..Change::default()
    })
}

/// send asks the host to apply change to the player.
fn send(player: Player, change: &Change) -> bool {
    host::send("Ui", |out| {
        if player.slot() != 0 {
            wire::int32(out, 1, player.slot());
        }
        wire::message(out, 2, change);
    })
}

/// notices stacks the notices at the lower left, newest at the bottom, clear
/// of the game's health, weapon and spirit stats and the status icons above
/// them.
fn notices(stack: &[Notice]) -> Element {
    // Each notice slides in from the left and fades as it expires; the stack
    // fades with its last notice.
    let motion = Transition {
        seconds: 0.25,
        easing: Easing::EaseOut,
        ..Transition::default()
    };
    let gone = Style {
        opacity: Some(0.0),
        ..Style::default()
    };
    let enter = Style {
        transform: Some(Transform {
            translate_x: -16.0,
            ..Transform::default()
        }),
        ..gone.clone()
    };
    let labels = stack.iter().map(|notice| {
        let style = Style {
            horizontal_align: Align::Start,
            margin: Some(Edges {
                top: 4.0,
                ..Edges::default()
            }),
            padding: Some([5, 10].into()),
            background: 0x1014_18d8,
            color: 0xf2f2_f2ff,
            font_size: 16.0,
            border_radius: 4.0,
            transition: Some(motion.clone()),
            ..Style::default()
        };
        label()
            .key(notice.id)
            .text(notice.text.as_str())
            .style(style)
            .enter(enter.clone())
            .exit(gone.clone())
    });
    let style = Style {
        width: Some(FIT),
        height: Some(FIT),
        flow: Flow::Down,
        horizontal_align: Align::Start,
        vertical_align: Align::End,
        margin: Some(Edges {
            bottom: 260.0,
            left: 24.0,
            ..Edges::default()
        }),
        transition: Some(motion.clone()),
        ..Style::default()
    };
    panel().style(style).exit(gone.clone()).child(labels).into()
}

/// display_rows returns the rows the view shows, with each player's name and
/// hero as the Tab screen shows them now.
fn display_rows(view: &View) -> Vec<AbilityRow> {
    // Read the names once, and each hero's class from its id.
    if view.display.is_empty() {
        return Vec::new();
    }
    let connections = players();
    view.display
        .iter()
        .map(|row| {
            // A Steam name is at most 32 characters, within the host's 128
            // bytes.
            let slot = row.player.slot();
            let name = connections
                .iter()
                .find(|connection| connection.player.slot() == slot)
                .map(|connection| connection.name.chars().take(32).collect())
                .unwrap_or_default();
            let hero = row
                .player
                .pawn()
                .and_then(|pawn| HEROES.iter().find(|hero| hero.id == pawn.hero))
                .map(|hero| hero.name.to_owned())
                .unwrap_or_default();

            // Each of the four slots shows its ability, or the game's own.
            AbilityRow {
                player: row.player,
                name,
                hero,
                own: slot == view.player.slot(),
                abilities: row.abilities.iter().take(4).cloned().collect(),
            }
        })
        .collect()
}

/// show replaces the interface the mod shows player with root: an element,
/// or a list of them, such as an Option, which holds one or none. An empty
/// root clears the mod's elements alone, and leaves the routes, the notices
/// and what the mod asked of the player's screen. A player who left and a
/// mod that restarted show nothing until the next call.
pub fn show(player: Player, root: impl Children) -> bool {
    let root = root.slot().0;
    change(player, |library, slot| library.view(slot).root = root)
}

/// hide removes the interface, the routes and the notices the mod shows
/// player.
pub fn hide(player: Player) -> bool {
    LIBRARY.with(|library| {
        let mut library = library.borrow_mut();
        library.views.remove(&player.slot());
        library.screens.remove(&player.slot());
    });
    send(
        player,
        &Change {
            reset: true,
            ..Change::default()
        },
    )
}

/// hide_hud hides parts of the game's own HUD from player until the next
/// call; no parts shows them all again.
pub fn hide_hud(player: Player, parts: &[HudPart]) -> bool {
    change(player, |library, slot| {
        library.view(slot).hud = parts.to_vec()
    })
}

/// free_cursor shows player's mouse cursor without Tab held while free is
/// true, so they can click the mod's panels, such as a pick screen; false
/// gives the mouse back to aiming. Each call with true shows the cursor again,
/// even when the player put it away with the game's own keys; a screen that
/// changes without a call leaves their choice be. The game does not stop the
/// player's shots, camera or movement while the cursor is up: a screen that
/// must not fire a weapon blocks the buttons it takes.
pub fn free_cursor(player: Player, free: bool) -> bool {
    change(player, |library, slot| {
        let view = library.view(slot);
        view.cursor = free;
        if free {
            view.asks += 1;
        }
    })
}

/// cursor_free reports whether player's mouse cursor is up now, whether the
/// mod freed it with free_cursor or the player did with the game's own keys.
/// It returns None while the player has no hero to read.
pub fn cursor_free(player: Player) -> Option<bool> {
    // Read the flag from the player's hero.
    let pawn = player.pawn()?;
    let class = "CCitadelPlayer_MovementServices";
    match read_field(pawn.entity, class, "m_bHasFreeCursor", FieldType::Bool)? {
        FieldValue::Boolean(free) => Some(free),
        _ => None,
    }
}

/// ability_bar_presses runs handler on the mod with the player and the slot,
/// 0 to 3, each time player clicks the icon or upgrade pips of an ability on
/// the game's own ability bar, as the game does when it trains one with Tab
/// held. The game answers a click from the hero's own abilities, and sends
/// nothing for an ability the mod replaced, so a mod that replaces abilities
/// answers here. Clicks reach the mod only while its screen holds the
/// player's HUD.
pub fn ability_bar_presses<M: 'static>(
    player: Player,
    handler: impl Fn(&mut M, Player, usize) + 'static,
) -> bool {
    let bar: Bar = Rc::new(move |m, player, slot| {
        if let Some(m) = m.downcast_mut::<M>() {
            handler(m, player, slot);
        }
    });
    change(player, |library, slot| library.view(slot).bar = Some(bar))
}

/// ignore_ability_bar stops player's clicks on the game's ability bar
/// reaching the mod.
pub fn ignore_ability_bar(player: Player) -> bool {
    change(player, |library, slot| library.view(slot).bar = None)
}

/// ability_display shows viewer each row's abilities in place of the ones the
/// game shows for that row's player: on the player's row of the game's Tab
/// screen, and for viewer's own row on viewer's ability bar too, in the game's
/// frame, ring and pips, with the game's ability window over each. A row
/// names its player and up to four abilities in slot order, each by its id,
/// as item_id gives it, with its picture and bought tiers; an ability left as
/// its default shows the game's own. The library fills each row's name, hero
/// and whether it is the viewer's own. Without rows the game's own come back.
pub fn ability_display(viewer: Player, rows: &[AbilityRow]) -> bool {
    let rows = rows[..rows.len().min(MAX_DISPLAY)].to_vec();
    change(viewer, |library, slot| library.view(slot).display = rows)
}

/// sheet adds the style sheet at path in the mod's content styles directory,
/// such as hud.css, to every interface the mod shows, after the sheets added
/// before it. Adding a sheet again changes nothing. A path is letters,
/// digits, '_', '-' and '.' in segments separated by '/', ending in .css;
/// another adds nothing and returns false.
pub fn sheet(path: &str) -> bool {
    // Refuse a path the renderer cannot load, and keep a sheet added before.
    let plain = path.split('/').all(|part| plain_name(part) && part != "..");
    if !path.ends_with(".css") || !plain {
        return false;
    }
    LIBRARY.with(|library| {
        let mut library = library.borrow_mut();
        if library.sheets.iter().any(|held| held == path) {
            return;
        }

        // Redraw every interface with the new sheet.
        library.sheets.push(path.to_owned());
        let shown: Vec<Player> = library.views.values().map(|view| view.player).collect();
        for player in shown {
            library.draw(player);
        }
    });
    true
}

/// route sends player the JSON text value under name, for every layout of the
/// mod to read with Modlock.route, until the next call; an empty value
/// removes it. A name is letters, digits, '_', '-' and '.'. A route with
/// another name, or a value that is not JSON, sends nothing and returns
/// false.
pub fn route(player: Player, name: &str, value: &str) -> bool {
    // Refuse a name no attribute can carry and a value that is not JSON.
    if !plain_name(name) || (!value.is_empty() && !is_json(value)) {
        return false;
    }

    // Store the value in the route's place, or forget the route, and redraw.
    change(player, |library, slot| {
        let routes = &mut library.view(slot).routes;
        let at = routes.iter().position(|(held, _)| held == name);
        match (at, value.is_empty()) {
            (Some(at), true) => {
                routes.remove(at);
            }
            (Some(at), false) => routes[at].1 = value.to_owned(),
            (None, true) => {}
            (None, false) => routes.push((name.to_owned(), value.to_owned())),
        }
    })
}

/// plain_name reports whether name is letters, digits, '_', '-' and '.'
/// alone.
fn plain_name(name: &str) -> bool {
    let plain = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.');
    !name.is_empty() && name.bytes().all(plain)
}

/// toast shows player text in a notice at the lower left for four seconds.
/// Notices stack, newest last, slide in, and fade as they expire.
pub fn toast(player: Player, text: impl Into<String>) -> bool {
    toast_for(player, text, 4.0)
}

/// toast_for shows player text in a notice for seconds, as toast does.
pub fn toast_for(player: Player, text: impl Into<String>, seconds: f64) -> bool {
    // Add the notice, keeping only the newest few.
    let text = text.into();
    change(player, |library, slot| {
        let notice = Notice {
            id: library.next_notice,
            text,
            until: library.now + seconds,
        };
        library.next_notice += 1;
        let notices = &mut library.view(slot).notices;
        notices.push(notice);
        let over = notices.len().saturating_sub(MAX_NOTICES);
        notices.drain(..over);
    })
}

/// sound plays the game's sound event sound, such as UI.ItemDraft.Start, in
/// player's game alone, once; seconds above 0 cuts it after that long, keeping
/// only its start. It plays with the next screen the player reads, and not at
/// all if the player reads it only after the mod's next few sounds.
pub fn sound(player: Player, sound: &str, seconds: f32) -> bool {
    sound_after(player, sound, 0.0, seconds)
}

/// sound_after plays sound as sound does, delay seconds after the player's
/// game reads it, so one draw can time a run of sounds, such as a wheel's
/// ticks. It does not play if the mod clears its interface first.
pub fn sound_after(player: Player, sound: &str, delay: f32, seconds: f32) -> bool {
    // Add the sound, keeping only the newest few.
    change(player, |library, slot| {
        let cue = Cue {
            number: library.next_cue,
            sound: sound.to_owned(),
            seconds,
            delay,
        };
        library.next_cue += 1;
        let cues = &mut library.view(slot).cues;
        cues.push(cue);
        let over = cues.len().saturating_sub(MAX_CUES);
        cues.drain(..over);
    })
}

/// handlers returns the handlers of the node the player in slot holds.
fn handlers(slot: i32, node: &str) -> Option<Handlers> {
    LIBRARY.with(|library| {
        let library = library.borrow();
        library.screens.get(&slot)?.handlers.get(node).cloned()
    })
}

/// press runs on the mod the handler of the button the player pressed, or of
/// the layout whose script sent a press with an action.
pub(crate) fn press(m: &mut dyn Any, event: &UiPressEvent) {
    // A press with an action came from a layout's script, not a button.
    let (player, slot) = (event.player, event.player.slot());
    if !event.action.is_empty() {
        if let Some(action) = handlers(slot, &event.node).and_then(|held| held.action) {
            action(m, player, &event.action, &event.payload);
        }
        return;
    }

    // A click on the game's ability bar names its slot, which no button's id
    // does.
    if let Some(ability) = ability_slot(&event.node) {
        let bar = LIBRARY.with(|library| library.borrow().views.get(&slot)?.bar.clone());
        if let Some(bar) = bar {
            bar(m, player, ability);
        }
        return;
    }
    if let Some(press) = handlers(slot, &event.node).and_then(|held| held.press) {
        press.run(m, player);
    }
}

/// ability_slot returns the slot a click on the game's ability bar names.
fn ability_slot(node: &str) -> Option<usize> {
    match node.strip_prefix("ability:")?.as_bytes() {
        [digit @ b'0'..=b'3'] => Some(usize::from(digit - b'0')),
        _ => None,
    }
}

/// changed runs on the mod the handler of the input the player changed, with
/// its value as the host checked it.
pub(crate) fn changed(m: &mut dyn Any, event: &UiInputEvent) {
    // Find the input's handlers on the screen the player holds.
    let player = event.player;
    let Some(held) = handlers(player.slot(), &event.node) else {
        return;
    };

    // Each value goes to the handler of its type.
    match (&event.value, held) {
        (
            Some(UiInputEventValue::Text(text)),
            Handlers {
                text: Some(run), ..
            },
        ) => run(m, player, text),
        (
            Some(UiInputEventValue::Number(number)),
            Handlers {
                number: Some(run), ..
            },
        ) => run(m, player, *number),
        (Some(UiInputEventValue::On(on)), Handlers { on: Some(run), .. }) => run(m, player, *on),
        (
            Some(UiInputEventValue::Choice(choice)),
            Handlers {
                choice: Some(run), ..
            },
        ) => run(m, player, *choice as usize),
        (
            Some(UiInputEventValue::Wheel(wheel)),
            Handlers {
                wheel: Some(run), ..
            },
        ) => run(m, player, wheel),
        _ => {}
    }
}

/// tick advances the clock to the frame at seconds and fades expiring
/// notices. It runs on the mod, which the chat lines may read.
pub(crate) fn tick(m: &mut dyn Any, seconds: f64) {
    // A chat line past its time redraws everyone.
    let expired = LIBRARY.with(|library| {
        let mut library = library.borrow_mut();
        library.now = seconds;
        library.layer.is_some_and(|layer| (layer.expire)(seconds))
    });
    if expired {
        draw_everyone(m);
    }
    LIBRARY.with(|library| {
        // Redraw only a view that lost a notice.
        let mut library = library.borrow_mut();
        let mut lost = Vec::new();
        for view in library.views.values_mut() {
            let before = view.notices.len();
            view.notices.retain(|notice| notice.until > seconds);
            if view.notices.len() != before {
                lost.push(view.player);
            }
        }
        for player in lost {
            library.draw(player);
        }
    });
}

/// returned sends a returning player their whole interface, which their
/// client lost while they were away.
pub(crate) fn returned(_m: &mut dyn Any, player: Player) {
    LIBRARY.with(|library| {
        let mut library = library.borrow_mut();
        if library.views.contains_key(&player.slot()) {
            library.screens.remove(&player.slot());
            library.draw(player);
        }
    });
}

/// forget drops the view and screen of each slot whose player left or was
/// replaced, so a new player in the slot starts with nothing shown. It runs
/// on the mod, which the chat lines may read.
pub(crate) fn forget(m: &mut dyn Any, players: &[Connection]) {
    // A line that left with its speaker redraws everyone. Otherwise each
    // player who came gets the lines laid out, for their first screen: it
    // may come from the mod's own call, where the library holds no mod.
    let (lost, came) = note(players);
    if lost {
        draw_everyone(m);
    } else {
        lay(m, &came);
    }
}

/// note notes who holds each slot and drops what the library kept for each
/// slot whose player left or was replaced. It reports whether a line of the
/// layer left with its speaker, and returns the players who came.
fn note(players: &[Connection]) -> (bool, Vec<Player>) {
    LIBRARY.with(|library| {
        // Find each slot whose connection is gone or new.
        let mut library = library.borrow_mut();
        let current: HashMap<i32, u32> = players
            .iter()
            .map(|connection| (connection.player.slot(), connection.generation))
            .collect();
        let gone: Vec<i32> = library
            .generations
            .iter()
            .filter(|(slot, generation)| current.get(slot) != Some(generation))
            .map(|(slot, _)| *slot)
            .collect();
        let mut lost = false;
        for slot in gone {
            library.views.remove(&slot);
            library.screens.remove(&slot);
            library.lines.remove(&slot);
            lost |= library.layer.is_some_and(|layer| (layer.forget)(slot));
        }
        let held = |connection: &&Connection| {
            let slot = connection.player.slot();
            library.generations.get(&slot) == Some(&connection.generation)
        };
        let came = players.iter().filter(|connection| !held(connection));
        let came = came.map(|connection| connection.player).collect();
        library.generations = current;
        (lost, came)
    })
}

/// is_json reports whether text is one JSON value.
fn is_json(text: &str) -> bool {
    let mut json = Json {
        rest: text.as_bytes(),
    };
    json.space();
    json.value(0) && {
        json.space();
        json.rest.is_empty()
    }
}

/// Json reads JSON text to check that it is JSON.
struct Json<'a> {
    rest: &'a [u8],
}

impl Json<'_> {
    /// space skips white space.
    fn space(&mut self) {
        while let [b' ' | b'\t' | b'\n' | b'\r', rest @ ..] = self.rest {
            self.rest = rest;
        }
    }

    /// take skips word when the text starts with it.
    fn take(&mut self, word: &[u8]) -> bool {
        match self.rest.strip_prefix(word) {
            Some(rest) => {
                self.rest = rest;
                true
            }
            None => false,
        }
    }

    /// digits skips a run of digits and reports whether it held any.
    fn digits(&mut self) -> bool {
        let count = self.rest.iter().take_while(|c| c.is_ascii_digit()).count();
        self.rest = &self.rest[count..];
        count != 0
    }

    /// value skips one value at depth levels of nesting.
    fn value(&mut self, depth: usize) -> bool {
        if depth > 64 {
            return false;
        }
        match self.rest.first() {
            Some(b'{') => self.list(depth, b'}', true),
            Some(b'[') => self.list(depth, b']', false),
            Some(b'"') => self.string(),
            Some(b't') => self.take(b"true"),
            Some(b'f') => self.take(b"false"),
            Some(b'n') => self.take(b"null"),
            _ => self.number(),
        }
    }

    /// list skips an object's members when keyed, or an array's elements,
    /// through the mark that closes it.
    fn list(&mut self, depth: usize, close: u8, keyed: bool) -> bool {
        self.rest = &self.rest[1..];
        self.space();
        if self.rest.first() == Some(&close) {
            self.rest = &self.rest[1..];
            return true;
        }
        loop {
            self.space();
            if keyed {
                if !self.string() {
                    return false;
                }
                self.space();
                if !self.take(b":") {
                    return false;
                }
                self.space();
            }
            if !self.value(depth + 1) {
                return false;
            }
            self.space();
            if self.take(&[close]) {
                return true;
            }
            if !self.take(b",") {
                return false;
            }
        }
    }

    /// string skips a string.
    fn string(&mut self) -> bool {
        if !self.take(b"\"") {
            return false;
        }
        loop {
            match self.rest {
                [b'"', rest @ ..] => {
                    self.rest = rest;
                    return true;
                }
                [b'\\', b'u', a, b, c, d, rest @ ..]
                    if [a, b, c, d].iter().all(|c| c.is_ascii_hexdigit()) =>
                {
                    self.rest = rest;
                }
                [b'\\', b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't', rest @ ..] => {
                    self.rest = rest;
                }
                [c, rest @ ..] if *c >= 0x20 && *c != b'\\' => self.rest = rest,
                _ => return false,
            }
        }
    }

    /// number skips a number.
    fn number(&mut self) -> bool {
        self.take(b"-");
        let whole = self.rest;
        if !self.digits() || (whole.len() - self.rest.len() > 1 && whole[0] == b'0') {
            return false;
        }
        if self.take(b".") && !self.digits() {
            return false;
        }
        if self.take(b"e") || self.take(b"E") {
            let _ = self.take(b"+") || self.take(b"-");
            return self.digits();
        }
        true
    }
}
