//! The dropper lets players build a spot while they play. A builder picks an
//! object from a catalog; a translucent preview follows their aim and snaps
//! to the surface it hits; a click drops it as a solid object everyone can
//! use at once. Aiming at a placed object picks it up to move it or deletes
//! it, and undo takes back the builder's own drops. Every builder drops into
//! the one spot the mode holds.
//!
//! The mode keeps the Dropper in its mod and tells it where: the panel's
//! buttons reach it through that function.
//!
//! ```ignore
//! let arena = load_spot(&spot);
//! let mut dropper = Dropper::new(catalog, arena, |mode: &mut Mode| mode.dropper.as_mut());
//! dropper.changed(|mode: &mut Mode, spot| mode.save(spot));
//! self.dropper = Some(dropper);
//! watch_input(DROPPER_BUTTONS | MODE_BUTTONS);
//!
//! // Each frame, each input and on the command:
//! dropper.frame(&event.movement);
//! dropper.input(event.player, event.pressed);
//! dropper.toggle(player);
//! ```
//!
//! The keys a builder uses are withheld from their hero while they build.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::bits::Buttons;
use crate::generated::{
    create_model, trace, Align, Angles, Flow, ModelOptions, MovementSample, Style, TraceOptions,
    Vector, WorldObject,
};
use crate::runtime::{after, players};
use crate::spot::{LoadedSpot, Spot, SpotObject, TRIGGERS};
use crate::ui::{button, color, hide, label, panel, show, slider, toast, Element};
use crate::Player;

/// CatalogEntry is one object builders can drop.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogEntry {
    /// name is what builders read in the catalog.
    pub name: String,
    /// category groups entries in the catalog, such as Crates.
    pub category: String,
    /// model is the model's path, which the mode precached.
    pub model: String,
    /// scale multiplies the model's size.
    pub scale: f32,
    /// bounce makes the object launch heroes upward at this speed.
    pub bounce: f32,
}

impl CatalogEntry {
    /// new returns the entry of an ordinary object at its model's own size.
    pub fn new(
        category: impl Into<String>,
        name: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            category: category.into(),
            model: model.into(),
            scale: 1.0,
            bounce: 0.0,
        }
    }
}

/// DROPPER_BUTTONS are the keys a builder uses, which the mode includes in
/// watch_input: fire drops, melee picks up or puts back, parry deletes,
/// reload turns, the first two abilities page the catalog and the third
/// undoes.
pub const DROPPER_BUTTONS: u64 = Buttons::ATTACK
    | Buttons::MELEE
    | Buttons::PARRY
    | Buttons::RELOAD
    | Buttons::ABILITY1
    | Buttons::ABILITY2
    | Buttons::ABILITY3;

/// REACH is how far a builder places objects, in game units.
const REACH: f64 = 1500.0;
/// GRID is the spacing positions snap to on floors.
const GRID: f64 = 8.0;
/// TURN_DEGREES is how far one press of reload turns the preview.
const TURN_DEGREES: f64 = 15.0;
/// FLOOR_NORMAL is the least upward normal that counts as a floor.
const FLOOR_NORMAL: f64 = 0.7;
/// PREVIEW_COLOR tints a preview green and translucent.
const PREVIEW_COLOR: u32 = 0x60ff_60a0;
/// UNDO_DEPTH is how many of a builder's drops undo remembers.
const UNDO_DEPTH: usize = 64;
/// WINDOW_SHOWN is how many catalog rows show at once around the selection.
const WINDOW_SHOWN: usize = 12;

/// Builder is one player building, with their preview and undo history.
struct Builder {
    player: Player,
    steam_id: u64,
    entry: usize,
    yaw: f64,
    preview: Option<WorldObject>,
    preview_model: Option<String>,
    /// aim is where the preview stands, or None while it cannot drop.
    aim: Option<(Vector, Angles)>,
    /// target is the entity of the placed object the builder aims at.
    target: Option<u32>,
    /// held is a placed object the builder picked up to move.
    held: Option<SpotObject>,
    /// drops are the builder's own drops, newest last, by entity.
    drops: Vec<u32>,
}

/// Changed runs on the mod with the spot as one change left it.
type Changed<M> = Rc<dyn Fn(&mut M, Spot)>;

/// Dropper is in-game building for one mode's spot. M is the mode's mod.
pub struct Dropper<M> {
    /// capacity bounds how many objects the spot holds.
    pub capacity: usize,
    /// spot is the spot builders drop into.
    pub spot: LoadedSpot,
    catalog: Vec<CatalogEntry>,
    builders: HashMap<i32, Builder>,
    /// changed runs on the mod with the spot after every drop, move and
    /// delete, once the mode gives one.
    changed: Option<Changed<M>>,
    /// held_by returns the Dropper from the mod, for the panel's buttons.
    held_by: fn(&mut M) -> Option<&mut Dropper<M>>,
}

impl<M: 'static> Dropper<M> {
    /// new returns the Dropper whose builders pick from catalog and drop
    /// into spot. held_by returns this Dropper from the mode's mod, where
    /// the mode keeps it.
    pub fn new(
        catalog: Vec<CatalogEntry>,
        spot: LoadedSpot,
        held_by: fn(&mut M) -> Option<&mut Dropper<M>>,
    ) -> Self {
        Self {
            capacity: usize::MAX,
            spot,
            catalog,
            builders: HashMap::new(),
            changed: None,
            held_by,
        }
    }

    /// changed runs changed on the mod with the spot after every drop, move,
    /// delete and undo, once for each, for a mode that saves it. It runs with
    /// the next frame, as a timer does, with the spot as that change left it.
    pub fn changed(&mut self, changed: impl Fn(&mut M, Spot) + 'static) {
        self.changed = Some(Rc::new(changed));
    }

    /// publish tells the mode of the spot as it stands now.
    fn publish(&self) {
        if let Some(changed) = self.changed.clone() {
            let spot = self.spot.spot();
            after(0.0, move |m: &mut M| changed(m, spot));
        }
    }

    /// building reports whether player is building.
    pub fn building(&self, player: Player) -> bool {
        self.builders.contains_key(&player.slot())
    }

    /// toggle starts or stops building for player.
    pub fn toggle(&mut self, player: Player) {
        // A second toggle ends the player's build session.
        if self.building(player) {
            self.stop(player);
            return;
        }

        // Require a connected player and a nonempty catalog.
        let mut connections = players().into_iter();
        let connection = connections.find(|held| held.player.slot() == player.slot());
        let Some(connection) = connection.filter(|_| !self.catalog.is_empty()) else {
            return;
        };

        // Start the builder facing the hero's yaw, snapped to a turn step.
        // Half a step rounds up, as the other languages' libraries round it.
        let yaw = player.pawn().map_or(0.0, |pawn| pawn.eye_angles.yaw);
        let builder = Builder {
            player,
            steam_id: connection.steam_id,
            entry: 0,
            yaw: (yaw / TURN_DEGREES + 0.5).floor() * TURN_DEGREES,
            preview: None,
            preview_model: None,
            aim: None,
            target: None,
            held: None,
            drops: Vec::new(),
        };
        self.builders.insert(player.slot(), builder);

        // Take the build buttons from the hero and show the dropper.
        player.block_input(DROPPER_BUTTONS);
        self.draw(player);
    }

    /// stop ends building for player, putting back an object they held.
    pub fn stop(&mut self, player: Player) {
        // Find the player's builder.
        let Some(builder) = self.builders.remove(&player.slot()) else {
            return;
        };

        // Place any held object back, then remove the preview.
        if let Some(held) = builder.held {
            self.spot.place(held);
        }
        if let Some(preview) = builder.preview {
            preview.remove();
        }

        // Return the buttons and the HUD.
        player.block_input(0);
        hide(player);
    }

    /// replace swaps the spot builders drop into, as when a saved spot
    /// loads.
    pub fn replace(&mut self, spot: LoadedSpot) {
        for builder in self.builders.values_mut() {
            builder.held = None;
            builder.target = None;
            builder.drops.clear();
        }
        self.spot.clear();
        self.spot = spot;
    }

    /// frame moves each builder's preview to their aim, drops builders who
    /// left, and launches heroes standing on bouncing objects.
    pub fn frame(&mut self, movement: &[MovementSample]) {
        let connections = players();
        let live: HashSet<i32> = connections.iter().map(|held| held.player.slot()).collect();
        let builders: Vec<Player> = self.builders.values().map(|held| held.player).collect();
        for player in builders {
            if live.contains(&player.slot()) {
                self.aim(player);
            } else {
                self.stop(player);
            }
        }
        self.spot.bounce(movement);
    }

    /// input applies a builder's key presses.
    pub fn input(&mut self, player: Player, pressed: u64) {
        // Find the player's builder.
        if !self.building(player) {
            return;
        }

        // Run the action of each pressed button. Each paging key steps from
        // the entry selected when its turn comes.
        let entry = |dropper: &Self| {
            let builder = dropper.builders.get(&player.slot());
            builder.map_or(0, |held| held.entry as i64)
        };
        if pressed & Buttons::ATTACK != 0 {
            self.drop_held(player);
        }
        if pressed & Buttons::MELEE != 0 {
            self.pick_up(player);
        }
        if pressed & Buttons::PARRY != 0 {
            self.delete(player);
        }
        if pressed & Buttons::RELOAD != 0 {
            self.turn(player, TURN_DEGREES);
        }
        if pressed & Buttons::ABILITY1 != 0 {
            self.select(player, entry(self) - 1);
        }
        if pressed & Buttons::ABILITY2 != 0 {
            self.select(player, entry(self) + 1);
        }
        if pressed & Buttons::ABILITY3 != 0 {
            self.undo(player);
        }
    }

    /// select shows the catalog entry at index, wrapping at either end.
    pub fn select(&mut self, player: Player, index: i64) {
        let count = self.catalog.len() as i64;
        let Some(builder) = self.builders.get_mut(&player.slot()) else {
            return;
        };
        if builder.held.is_some() || count == 0 {
            return;
        }
        builder.entry = index.rem_euclid(count) as usize;
        self.draw(player);
    }

    /// turn turns player's preview by degrees.
    pub fn turn(&mut self, player: Player, degrees: f64) {
        if let Some(yaw) = self.builders.get(&player.slot()).map(|held| held.yaw) {
            self.face(player, yaw + degrees);
        }
    }

    /// face sets player's preview to face degrees, and shows it on the
    /// panel's slider.
    pub fn face(&mut self, player: Player, degrees: f64) {
        if let Some(builder) = self.builders.get_mut(&player.slot()) {
            builder.yaw = degrees.rem_euclid(360.0);
            self.draw(player);
        }
    }

    /// undo removes player's newest drop that still stands.
    pub fn undo(&mut self, player: Player) {
        let Some(builder) = self.builders.get_mut(&player.slot()) else {
            return;
        };
        while let Some(entity) = builder.drops.pop() {
            if self.spot.remove(entity) {
                self.publish();
                return;
            }
        }
    }

    /// object returns what the builder's preview stands for.
    fn object(&self, builder: &Builder) -> Option<SpotObject> {
        // A held object keeps its look at the aim; a new one takes its
        // entry's.
        let (position, facing) = builder.aim?;
        if let Some(held) = &builder.held {
            return Some(SpotObject {
                position,
                facing,
                ..held.clone()
            });
        }
        let entry = &self.catalog[builder.entry];
        Some(SpotObject {
            model: entry.model.clone(),
            position,
            facing,
            scale: entry.scale,
            bounce: entry.bounce,
            placed_by: builder.steam_id,
        })
    }

    /// aim places the builder's preview where they look.
    fn aim(&mut self, player: Player) {
        // Clear the last aim and target before tracing again.
        let Some(builder) = self.builders.get_mut(&player.slot()) else {
            return;
        };
        let entry = &self.catalog[builder.entry];
        let held = builder.held.as_ref();
        let model = held.map_or(&entry.model, |held| &held.model).clone();
        let scale = held.map_or(entry.scale, |held| held.scale);
        builder.aim = None;
        builder.target = None;
        let Some(pawn) = player.pawn() else {
            return;
        };

        // Trace along the hero's view to the reach distance.
        let eye = pawn.eye_position;
        let hit = trace(&TraceOptions {
            start: eye,
            end: eye + pawn.eye_angles.forward() * REACH,
            exclude: Some(TRIGGERS),
            ignore: vec![pawn.entity],
            ..TraceOptions::default()
        });

        // Aim at the hit surface: snapped on a floor, facing out of a wall.
        if let Some(hit) = hit.filter(|hit| !hit.start_solid) {
            builder.target = self.spot.at(hit.entity).map(|placed| placed.entity);
            let floor = hit.normal.z >= FLOOR_NORMAL;
            let position = if floor {
                Vector::new(snap(hit.position.x), snap(hit.position.y), hit.position.z)
            } else {
                hit.position
            };
            let yaw = if floor {
                builder.yaw
            } else {
                hit.normal.y.atan2(hit.normal.x).to_degrees()
            };
            builder.aim = Some((position, Angles::level(yaw)));
        }

        // Show the preview at the new aim.
        preview(builder, model, scale);
    }

    /// drop_held places the preview as a solid object.
    fn drop_held(&mut self, player: Player) {
        // Build the object at the aim, or explain why there is none.
        let Some(builder) = self.builders.get(&player.slot()) else {
            return;
        };
        let Some(object) = self.object(builder) else {
            toast(player, "Aim at a surface to drop.");
            return;
        };

        // Refuse a new object when the spot is full.
        if builder.held.is_none() && self.spot.objects().len() >= self.capacity {
            toast(
                player,
                "The spot is full. Delete an object to drop another.",
            );
            return;
        }

        // Place the object in the spot.
        let Some(entity) = self.spot.place(object).map(|placed| placed.entity) else {
            toast(player, "That object could not be placed.");
            return;
        };

        // Record the drop for undo and publish the changed spot.
        if let Some(builder) = self.builders.get_mut(&player.slot()) {
            builder.held = None;
            builder.drops.push(entity);
            let over = builder.drops.len().saturating_sub(UNDO_DEPTH);
            builder.drops.drain(..over);
        }
        self.draw(player);
        self.publish();
    }

    /// pick_up lifts the aimed object to move it, or puts a held one back.
    fn pick_up(&mut self, player: Player) {
        // A second press while holding drops the held object.
        let Some(builder) = self.builders.get(&player.slot()) else {
            return;
        };
        if builder.held.is_some() {
            self.drop_held(player);
            return;
        }

        // Require an aimed object to pick up.
        let aimed = builder.target.and_then(|entity| self.spot.at(entity));
        let Some((entity, object)) = aimed.map(|placed| (placed.entity, placed.object.clone()))
        else {
            return;
        };

        // Lift the object out of the spot into the builder's hands.
        self.spot.remove(entity);
        if let Some(builder) = self.builders.get_mut(&player.slot()) {
            builder.yaw = object.facing.yaw;
            builder.held = Some(object);
            builder.target = None;
        }

        // Redraw and publish the changed spot.
        self.draw(player);
        self.publish();
    }

    /// delete removes the aimed object. The mode hears of it whenever the
    /// builder aimed at one, even one that was already gone.
    fn delete(&mut self, player: Player) {
        let Some(builder) = self.builders.get_mut(&player.slot()) else {
            return;
        };
        if let Some(entity) = builder.target.take() {
            self.spot.remove(entity);
            self.publish();
        }
    }

    /// draw shows the builder the catalog and their keys.
    fn draw(&self, player: Player) {
        let Some(builder) = self.builders.get(&player.slot()) else {
            return;
        };
        let held_by = self.held_by;
        let white = color("#ffffff");
        let pale = color("#dddddd");

        // Each row of the catalog picks its entry.
        let rows = self.catalog.iter().enumerate().map(|(index, entry)| {
            let style = Style {
                padding: [4, 10].into(),
                background: color(if index == builder.entry {
                    "#3c8c3cdd"
                } else {
                    "#00000099"
                }),
                color: white,
                font_size: 16.0,
                ..Style::default()
            };
            let pick = move |m: &mut M, player: Player| {
                if let Some(dropper) = held_by(m) {
                    dropper.select(player, index as i64);
                }
            };
            let text = format!("{} · {}", entry.category, entry.name);
            let row = button().key(index).style(style).on_press(pick);
            Element::from(row.child(crate::ui::text(text)))
        });
        let rows: Vec<Element> = window_of(rows.collect(), builder.entry);

        // The panel names the object in hand, the facing and the keys.
        let moving = builder.held.as_ref().map_or_else(String::new, |held| {
            let name = held.model.rsplit('/').next().unwrap_or(&held.model);
            format!("Moving {name}")
        });
        let small = |margin: i32| Style {
            font_size: 14.0,
            color: pale,
            margin: [margin, 0, 0, 0].into(),
            ..Style::default()
        };
        let title = Style {
            font_size: 22.0,
            bold: true,
            color: white,
            ..Style::default()
        };
        let frame = Style {
            flow: Flow::Down,
            horizontal_align: Align::End,
            vertical_align: Align::Center,
            margin: [0, 24, 0, 0].into(),
            ..Style::default()
        };
        let facing = slider()
            .value(builder.yaw as f32)
            .range(0.0, (360.0 - TURN_DEGREES) as f32)
            .step(TURN_DEGREES as f32)
            .on_change(move |m: &mut M, player: Player, degrees: f32| {
                if let Some(dropper) = held_by(m) {
                    dropper.face(player, f64::from(degrees));
                }
            });
        let keys = "Fire drop · Melee move · Parry delete · Reload turn · 1 and 2 choose · 3 undo · Tab to click";
        let shown = panel()
            .style(frame)
            .child(label().text("Object Dropper").style(title))
            .child(label().text(moving).style(small(0)))
            .child(rows)
            .child(label().text("Facing").style(small(8)))
            .child(facing)
            .child(label().text(keys).style(small(8)));
        show(player, shown);
    }
}

/// preview draws the builder's preview of model at their aim, or hides it.
fn preview(builder: &mut Builder, model: String, scale: f32) {
    // Remove a preview whose model changed or that has no aim.
    if builder.preview_model.as_ref() != Some(&model) || builder.aim.is_none() {
        if let Some(preview) = builder.preview.take() {
            preview.remove();
        }
        builder.preview_model = None;
    }

    // Without an aim there is nothing to preview.
    let Some((position, facing)) = builder.aim else {
        return;
    };

    // Move an existing preview to the aim.
    if let Some(preview) = builder.preview {
        preview.r#move(&position, Some(&facing));
        return;
    }

    // Create the preview model at the aim.
    builder.preview = create_model(&ModelOptions {
        resource: model.clone(),
        position,
        facing: Some(facing),
        scale: Some(scale),
        color: Some(PREVIEW_COLOR),
        glow: Some(true),
        ..ModelOptions::default()
    });
    builder.preview_model = Some(model);
}

/// window_of returns the rows around selected, so long catalogs stay on
/// screen.
fn window_of<T>(rows: Vec<T>, selected: usize) -> Vec<T> {
    let last = rows.len().saturating_sub(WINDOW_SHOWN);
    let start = selected.saturating_sub(WINDOW_SHOWN / 2).min(last);
    rows.into_iter().skip(start).take(WINDOW_SHOWN).collect()
}

/// snap rounds a coordinate to the floor grid, half a cell upward.
fn snap(value: f64) -> f64 {
    (value / GRID + 0.5).floor() * GRID
}
