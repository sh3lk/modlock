//! element holds the elements a mod builds its interface from: one Tag for
//! each kind of panel the game draws, made by a function named for it.

use std::any::Any;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::generated::{
    Clock, Cycle, Dropdown, HeroLook, Kind, Node, NodeInput, NodeVariablesEntry, Scaling, Slider,
    Style, TextEntry, Toggle, Wheel,
};
use crate::items::item_id;
use crate::Player;

/// Run runs a handler on the mod with the player and what the event tells of.
type Run<Told> = Rc<dyn Fn(&mut dyn Any, Player, Told)>;

/// Typed runs an entry's handler on the mod with the player and their text.
type Typed = Rc<dyn Fn(&mut dyn Any, Player, &str)>;

/// Turned runs a slider's handler on the mod with the player and a step of
/// their wheel.
type Turned = Rc<dyn Fn(&mut dyn Any, Player, &Wheel)>;

/// Action runs a layout's handler on the mod with the player, the action and
/// its payload.
type Action = Rc<dyn Fn(&mut dyn Any, Player, &str, &str)>;

/// Pressing runs a press's handler on the mod with the player who pressed.
type Pressing = dyn Fn(&mut dyn Any, Player);

/// Press is the handler of a press, as a button's on_press takes it. A
/// component takes one as a property of type Option<Press>, and hands it to
/// the button it draws.
#[derive(Clone)]
pub struct Press(Rc<Pressing>);

impl Press {
    /// run runs the handler on the mod with the player who pressed.
    pub(crate) fn run(&self, m: &mut dyn Any, player: Player) {
        (self.0)(m, player)
    }
}

/// Pressed is what on_press takes: a method of the mod M, such as
/// Self::ready, a closure that takes the mod and the player, or a Press a
/// component was given.
pub trait Pressed<M> {
    /// press returns the handler as an element keeps it.
    fn press(self) -> Press;
}

impl<M: 'static, F: Fn(&mut M, Player) + 'static> Pressed<M> for F {
    fn press(self) -> Press {
        Press(Rc::new(move |m, player| {
            if let Some(m) = m.downcast_mut::<M>() {
                self(m, player);
            }
        }))
    }
}

impl Pressed<Press> for Press {
    fn press(self) -> Press {
        self
    }
}

/// Handlers are the handlers of one button, input or layout; each is None
/// when unset. Each runs on the mod.
#[derive(Clone, Default)]
pub(crate) struct Handlers {
    /// press runs when the player presses a button.
    pub press: Option<Press>,
    /// action runs when a layout's script sends a press, with its action and
    /// its payload as JSON text.
    pub action: Option<Action>,
    /// text runs when the player types in an entry.
    pub text: Option<Typed>,
    /// number runs when the player drags a slider.
    pub number: Option<Run<f32>>,
    /// on runs when the player checks a toggle.
    pub on: Option<Run<bool>>,
    /// choice runs when the player picks an option.
    pub choice: Option<Run<usize>>,
    /// wheel runs when the player turns the wheel over a slider.
    pub wheel: Option<Turned>,
}

impl Handlers {
    /// any reports whether any handler is set.
    pub fn any(&self) -> bool {
        self.press.is_some()
            || self.action.is_some()
            || self.text.is_some()
            || self.number.is_some()
            || self.on.is_some()
            || self.choice.is_some()
            || self.wheel.is_some()
    }
}

/// Slot is what one child call gave an element: one element, or a list whose
/// elements keep their places within it, so a list that grows moves no
/// element after it.
#[derive(Clone)]
pub(crate) enum Slot {
    One(Box<Element>),
    Many(Vec<Element>),
}

/// Element is one drawn element with its children: what a Tag builds, a
/// component returns and show takes.
#[derive(Clone)]
pub struct Element {
    /// key keeps the element's node in a list while the list changes.
    pub(crate) key: String,
    /// node holds the element's kind and properties as the game takes them,
    /// without its id and children.
    pub(crate) node: Node,
    pub(crate) children: Vec<Slot>,
    pub(crate) handlers: Handlers,
}

/// Children is what an element may hold: an element, or any list of them,
/// such as a Vec, an iterator or an Option, which holds one or none.
pub trait Children {
    #[doc(hidden)]
    fn slot(self) -> SlotOf;
}

/// SlotOf is the slot a Children makes.
#[doc(hidden)]
pub struct SlotOf(pub(crate) Slot);

/// spare returns value as a node keeps a part most nodes leave unset: None
/// for a value that sets nothing, which then takes no room and is not sent.
fn spare<T: Default + PartialEq>(value: T) -> Option<Box<T>> {
    (value != T::default()).then(|| Box::new(value))
}

/// slot_of returns the slot of one element. #[component] writes it for a
/// component, which is one element where it stands.
#[doc(hidden)]
pub fn slot_of(element: Element) -> SlotOf {
    SlotOf(Slot::One(Box::new(element)))
}

/// elements returns the elements children holds, in order. #[component]
/// writes it for a component that takes children.
#[doc(hidden)]
pub fn elements(children: impl Children) -> Vec<Element> {
    match children.slot().0 {
        Slot::One(element) => vec![*element],
        Slot::Many(elements) => elements,
    }
}

impl Element {
    /// key keeps the element's node in a list while the list changes, as a
    /// tag's key does.
    pub fn key(mut self, key: impl ToString) -> Self {
        self.key = key.to_string();
        self
    }

    /// panels returns how many panels the game draws for the element: itself
    /// and every element it holds.
    pub fn panels(&self) -> usize {
        let held = |slot: &Slot| match slot {
            Slot::One(element) => element.panels(),
            Slot::Many(elements) => elements.iter().map(Element::panels).sum(),
        };
        1 + self.children.iter().map(held).sum::<usize>()
    }
}

impl Children for Element {
    fn slot(self) -> SlotOf {
        SlotOf(Slot::One(Box::new(self)))
    }
}

impl<K> Children for Tag<K> {
    fn slot(self) -> SlotOf {
        SlotOf(Slot::One(Box::new(self.element)))
    }
}

impl<I, T> Children for I
where
    I: IntoIterator<Item = T>,
    T: Into<Element>,
{
    fn slot(self) -> SlotOf {
        SlotOf(Slot::Many(self.into_iter().map(Into::into).collect()))
    }
}

/// tag names each kind of element a Tag builds.
pub mod tag {
    /// Panel groups its children.
    pub struct Panel;
    /// Label shows text.
    pub struct Label;
    /// Image shows an image from the game's content.
    pub struct Image;
    /// Button holds its children and reports each press.
    pub struct Button;
    /// Hero shows a picture of a hero.
    pub struct Hero;
    /// Avatar shows a player's Steam picture.
    pub struct Avatar;
    /// Entry is a box the player types a line into.
    pub struct Entry;
    /// Slider is a bar the player drags to a number.
    pub struct Slider;
    /// Toggle is a box the player checks.
    pub struct Toggle;
    /// Dropdown lets the player pick one of its options.
    pub struct Dropdown;
    /// Layout loads a layout from the mod's content addon.
    pub struct Layout;
}

/// Tag builds one element of kind K: each method sets a property and returns
/// the tag, and the tag stands wherever an element does.
pub struct Tag<K> {
    element: Element,
    kind: PhantomData<K>,
}

impl<K> Clone for Tag<K> {
    fn clone(&self) -> Self {
        Tag {
            element: self.element.clone(),
            kind: PhantomData,
        }
    }
}

impl<K> From<Tag<K>> for Element {
    fn from(tag: Tag<K>) -> Element {
        tag.element
    }
}

/// of returns the tag of an element of kind.
fn of<K>(kind: Kind) -> Tag<K> {
    let node = Node {
        kind,
        ..Default::default()
    };
    let element = Element {
        key: String::new(),
        node,
        children: Vec::new(),
        handlers: Handlers::default(),
    };
    Tag {
        element,
        kind: PhantomData,
    }
}

/// panel returns a panel, which groups its children.
pub fn panel() -> Tag<tag::Panel> {
    of(Kind::Panel)
}

/// label returns a label, which shows text: its text, or the game's text for
/// its token in the player's language. A clock's time stands in for the text.
pub fn label() -> Tag<tag::Label> {
    of(Kind::Label)
}

/// text returns a label of plain text.
pub fn text(text: impl Into<String>) -> Tag<tag::Label> {
    label().text(text)
}

/// image returns an image, which shows the picture at its src in the game's
/// content, such as file://{images}/hud/icon.png.
pub fn image() -> Tag<tag::Image> {
    of(Kind::Image)
}

/// button returns a button, which holds its children and runs on_press with
/// the player who pressed it. Players press buttons while they have a cursor.
pub fn button() -> Tag<tag::Button> {
    of(Kind::Button)
}

/// hero returns a picture of a hero, by the game's hero id.
pub fn hero() -> Tag<tag::Hero> {
    of(Kind::HeroImage)
}

/// avatar returns the Steam picture of the player with its steam_id.
pub fn avatar() -> Tag<tag::Avatar> {
    of(Kind::Avatar)
}

/// entry returns a box the player types a line into.
pub fn entry() -> Tag<tag::Entry> {
    let mut tag = of(Kind::Input);
    tag.element.node.input = Some(NodeInput::TextEntry(TextEntry::default()));
    tag
}

/// slider returns a bar the player drags to a number from 0 to 1, or within
/// its range.
pub fn slider() -> Tag<tag::Slider> {
    let mut tag = of(Kind::Input);
    tag.element.node.input = Some(NodeInput::Slider(Slider::default()));
    tag
}

/// toggle returns a box the player checks.
pub fn toggle() -> Tag<tag::Toggle> {
    let mut tag = of(Kind::Input);
    tag.element.node.input = Some(NodeInput::Toggle(Toggle::default()));
    tag
}

/// dropdown returns a control that lets the player pick one of its options.
pub fn dropdown() -> Tag<tag::Dropdown> {
    let mut tag = of(Kind::Input);
    tag.element.node.input = Some(NodeInput::Dropdown(Dropdown::default()));
    tag
}

/// layout returns a layout loaded from the mod's content addon: src is its
/// path there, such as shop.xml.
pub fn layout() -> Tag<tag::Layout> {
    of(Kind::Layout)
}

/// The properties every element takes.
impl<K> Tag<K> {
    /// key keeps the element's node in a list while the list changes: an
    /// element with the key it had before stays the same panel, wherever it
    /// stands in the list now.
    pub fn key(mut self, key: impl ToString) -> Self {
        self.element.key = key.to_string();
        self
    }

    /// class names the element's classes in the mod's sheets, separated by
    /// spaces.
    pub fn class(mut self, classes: &str) -> Self {
        self.element.node.classes = classes.split_whitespace().map(str::to_owned).collect();
        self
    }

    /// style lays out and paints the element.
    pub fn style(mut self, style: Style) -> Self {
        self.element.node.style = style;
        self
    }

    /// hover is drawn over the style while the pointer is over the element.
    /// An element with a hover style takes the pointer.
    pub fn hover(mut self, style: Style) -> Self {
        self.element.node.hover = spare(style);
        self
    }

    /// enter is the style the element starts from when it first shows; its
    /// style's transition eases it from there.
    pub fn enter(mut self, style: Style) -> Self {
        self.element.node.enter = spare(style);
        self
    }

    /// exit is the style the element eases to as it leaves.
    pub fn exit(mut self, style: Style) -> Self {
        self.element.node.exit = spare(style);
        self
    }

    /// cycle swings the element to another style and back while it shows,
    /// such as a pulse or a blink.
    pub fn cycle(mut self, cycle: Cycle) -> Self {
        self.element.node.cycle = spare(cycle);
        self
    }

    /// tooltip is the text the game's tooltip shows over the element.
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.element.node.tooltip = text.into();
        self
    }

    /// item names an item by class name whose shop card shows over the
    /// element.
    pub fn item(mut self, name: &str) -> Self {
        self.element.node.item = item_id(name);
        self
    }

    /// ability names an ability by class name, such as ability_bebop_hook,
    /// whose window the game shows over the element, with the numbers of the
    /// hero of player, a hero that stands on the map.
    pub fn ability(mut self, name: &str, player: Player) -> Self {
        self.element.node.ability = item_id(name);
        self.element.node.ability_player = player.slot() as u32;
        self
    }

    /// hover_sound is the game's sound event played as the pointer enters.
    pub fn hover_sound(mut self, sound: impl Into<String>) -> Self {
        self.element.node.hover_sound = sound.into();
        self
    }

    /// hide_in_chat hides the element while the player has the game's chat
    /// box open.
    pub fn hide_in_chat(mut self, hide: bool) -> Self {
        self.element.node.hide_in_chat = hide;
        self
    }

    /// held adds children to the element.
    fn held(mut self, children: impl Children) -> Self {
        self.element.children.push(children.slot().0);
        self
    }
}

impl Tag<tag::Panel> {
    /// child adds to the panel an element, or a list of them: a Vec, an
    /// iterator, or an Option, which adds one or none.
    pub fn child(self, children: impl Children) -> Self {
        self.held(children)
    }
}

impl Tag<tag::Label> {
    /// text is the label's text.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.element.node.text = text.into();
        self
    }

    /// token names the game's localized text to show in place of the text,
    /// such as #Citadel_HeroName_Haze.
    pub fn token(mut self, token: impl Into<String>) -> Self {
        self.element.node.token = token.into();
        self
    }

    /// variable fills the {s:name} place in the token's text with value.
    pub fn variable(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let entry = NodeVariablesEntry {
            key: name.into(),
            value: value.into(),
        };
        self.element
            .node
            .variables
            .retain(|held| held.key != entry.key);
        self.element.node.variables.push(entry);
        self.element
            .node
            .variables
            .sort_by(|a, b| a.key.cmp(&b.key));
        self
    }

    /// clock counts a time in the player's game, so the mod draws it once and
    /// not every second.
    pub fn clock(mut self, clock: Clock) -> Self {
        self.element.node.clock = Some(clock);
        self
    }
}

impl Tag<tag::Image> {
    /// src is the image's path in the game's content.
    pub fn src(mut self, src: impl Into<String>) -> Self {
        self.element.node.image = src.into();
        self
    }

    /// scaling fits the picture into the image's box.
    pub fn scaling(mut self, scaling: Scaling) -> Self {
        self.element.node.scaling = scaling;
        self
    }
}

impl Tag<tag::Button> {
    /// child adds to the button an element, or a list of them, as a panel's
    /// child does.
    pub fn child(self, children: impl Children) -> Self {
        self.held(children)
    }

    /// on_press runs handler on the mod with the player who pressed the
    /// button: a method of the mod, such as Self::ready, or a closure that
    /// takes the mod and the player.
    pub fn on_press<M>(mut self, handler: impl Pressed<M>) -> Self {
        self.element.handlers.press = Some(handler.press());
        self
    }

    /// sound is the game's sound event the press plays, such as
    /// UI.MainMenu.Activate.
    pub fn sound(mut self, sound: impl Into<String>) -> Self {
        self.element.node.sound = sound.into();
        self
    }

    /// disabled makes the button take no presses.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.element.node.disabled = disabled;
        self
    }
}

impl Tag<tag::Hero> {
    /// id is the hero's id, as a pawn's hero and HEROES give it.
    pub fn id(mut self, hero: u32) -> Self {
        self.element.node.hero = hero as i32;
        self
    }

    /// look is which of the game's pictures of the hero to show.
    pub fn look(mut self, look: HeroLook) -> Self {
        self.element.node.hero_look = look;
        self
    }

    /// scaling fits the picture into the element's box.
    pub fn scaling(mut self, scaling: Scaling) -> Self {
        self.element.node.scaling = scaling;
        self
    }
}

impl Tag<tag::Avatar> {
    /// steam_id is whose picture the avatar shows.
    pub fn steam_id(mut self, steam_id: u64) -> Self {
        self.element.node.steam_id = steam_id;
        self
    }
}

impl Tag<tag::Entry> {
    /// input returns the entry's control.
    fn input(&mut self) -> &mut TextEntry {
        match &mut self.element.node.input {
            Some(NodeInput::TextEntry(input)) => input,
            _ => unreachable!("an entry holds a text entry"),
        }
    }

    /// value is the entry's text.
    pub fn value(mut self, text: impl Into<String>) -> Self {
        self.input().text = text.into();
        self
    }

    /// placeholder is the faint text the entry shows empty.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.input().placeholder = text.into();
        self
    }

    /// max_length is the entry's limit in characters, 100 unless set, and at
    /// most 240 bytes of text.
    pub fn max_length(mut self, characters: u32) -> Self {
        self.input().max_length = characters;
        self
    }

    /// disabled makes the entry take no changes.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.element.node.disabled = disabled;
        self
    }

    /// on_change runs handler on the mod with the player and the text they
    /// typed.
    pub fn on_change<M: 'static>(
        mut self,
        handler: impl Fn(&mut M, Player, &str) + 'static,
    ) -> Self {
        self.element.handlers.text = Some(Rc::new(move |m, player, text| {
            if let Some(m) = m.downcast_mut::<M>() {
                handler(m, player, text);
            }
        }));
        self
    }
}

impl Tag<tag::Slider> {
    /// input returns the slider's control.
    fn input(&mut self) -> &mut Slider {
        match &mut self.element.node.input {
            Some(NodeInput::Slider(input)) => input,
            _ => unreachable!("a slider holds a slider"),
        }
    }

    /// value is the slider's number.
    pub fn value(mut self, value: f32) -> Self {
        self.input().value = value;
        self
    }

    /// range is the least and the greatest number; unset, it is 0 to 1.
    pub fn range(mut self, min: f32, max: f32) -> Self {
        let input = self.input();
        (input.min, input.max) = (min, max);
        self
    }

    /// step is the distance between the stops the slider stops at.
    pub fn step(mut self, step: f32) -> Self {
        self.input().step = step;
        self
    }

    /// disabled makes the slider take no changes.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.element.node.disabled = disabled;
        self
    }

    /// on_change runs handler on the mod with the player and the number they
    /// dragged to.
    pub fn on_change<M: 'static>(
        mut self,
        handler: impl Fn(&mut M, Player, f32) + 'static,
    ) -> Self {
        self.element.handlers.number = Some(Rc::new(move |m, player, value| {
            if let Some(m) = m.downcast_mut::<M>() {
                handler(m, player, value);
            }
        }));
        self
    }

    /// on_wheel runs handler on the mod with the player and each step of the
    /// wheel they turn over the slider.
    pub fn on_wheel<M: 'static>(
        mut self,
        handler: impl Fn(&mut M, Player, &Wheel) + 'static,
    ) -> Self {
        self.element.handlers.wheel = Some(Rc::new(move |m, player, wheel| {
            if let Some(m) = m.downcast_mut::<M>() {
                handler(m, player, wheel);
            }
        }));
        self
    }
}

impl Tag<tag::Toggle> {
    /// text labels the toggle.
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.element.node.text = text.into();
        self
    }

    /// on checks the box.
    pub fn on(mut self, on: bool) -> Self {
        self.element.node.input = Some(NodeInput::Toggle(Toggle { on }));
        self
    }

    /// disabled makes the toggle take no changes.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.element.node.disabled = disabled;
        self
    }

    /// on_change runs handler on the mod with the player and whether they
    /// checked the box.
    pub fn on_change<M: 'static>(
        mut self,
        handler: impl Fn(&mut M, Player, bool) + 'static,
    ) -> Self {
        self.element.handlers.on = Some(Rc::new(move |m, player, on| {
            if let Some(m) = m.downcast_mut::<M>() {
                handler(m, player, on);
            }
        }));
        self
    }
}

impl Tag<tag::Dropdown> {
    /// input returns the dropdown's control.
    fn input(&mut self) -> &mut Dropdown {
        match &mut self.element.node.input {
            Some(NodeInput::Dropdown(input)) => input,
            _ => unreachable!("a dropdown holds a dropdown"),
        }
    }

    /// options are the choices, in order.
    pub fn options<T: Into<String>>(mut self, options: impl IntoIterator<Item = T>) -> Self {
        self.input().options = options.into_iter().map(Into::into).collect();
        self
    }

    /// selected is the index of the chosen option.
    pub fn selected(mut self, index: usize) -> Self {
        self.input().selected = index as u32;
        self
    }

    /// disabled makes the dropdown take no changes.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.element.node.disabled = disabled;
        self
    }

    /// on_change runs handler on the mod with the player and the index of the
    /// option they picked.
    pub fn on_change<M: 'static>(
        mut self,
        handler: impl Fn(&mut M, Player, usize) + 'static,
    ) -> Self {
        self.element.handlers.choice = Some(Rc::new(move |m, player, index| {
            if let Some(m) = m.downcast_mut::<M>() {
                handler(m, player, index);
            }
        }));
        self
    }
}

impl Tag<tag::Layout> {
    /// src is the layout's path in the mod's content addon.
    pub fn src(mut self, src: impl Into<String>) -> Self {
        self.element.node.layout = src.into();
        self
    }

    /// data is the JSON text the layout's script reads with Modlock.data.
    pub fn data(mut self, json: impl Into<String>) -> Self {
        self.element.node.data = json.into();
        self
    }

    /// on_action runs handler on the mod with the player, the action and the
    /// payload, as JSON text, of each press the layout's script sends.
    pub fn on_action<M: 'static>(
        mut self,
        handler: impl Fn(&mut M, Player, &str, &str) + 'static,
    ) -> Self {
        self.element.handlers.action = Some(Rc::new(move |m, player, action, payload| {
            if let Some(m) = m.downcast_mut::<M>() {
                handler(m, player, action, payload);
            }
        }));
        self
    }
}
