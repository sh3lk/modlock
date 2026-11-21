//! A spot is a base map and the objects players placed on it. Nothing is
//! compiled: loading a spot spawns each object as a solid model on the map
//! the world already runs, so any mode on that map can load one as its
//! arena.
//!
//! ```ignore
//! let mut arena = load_spot(&decode_spot(&bytes)?);
//! arena.bounce(&event.movement);
//! arena.clear();
//! ```
//!
//! encode_spot writes the spot document, whose bytes depend only on the
//! spot, and are the bytes the other languages' libraries write for it.

use std::collections::HashMap;

use crate::generated::{
    create_model, log_line, trace, Angles, ModelOptions, MovementSample, TraceOptions, Vector,
    WorldObject,
};
use crate::wire::{self, Error, Reader};

/// SpotObject is one solid object placed in a spot.
#[derive(Clone, Debug, PartialEq)]
pub struct SpotObject {
    /// model is the model's path, which the mode precached.
    pub model: String,
    /// position is the model's origin.
    pub position: Vector,
    /// facing is the model's rotation.
    pub facing: Angles,
    /// scale multiplies the model's size.
    pub scale: f32,
    /// bounce launches a hero who stands on the object upward at this speed,
    /// in units per second; zero makes an ordinary object.
    pub bounce: f32,
    /// placed_by is the Steam ID of the player who placed the object, or
    /// zero.
    pub placed_by: u64,
}

impl Default for SpotObject {
    fn default() -> Self {
        Self {
            model: String::new(),
            position: Vector::default(),
            facing: Angles::default(),
            scale: 1.0,
            bounce: 0.0,
            placed_by: 0,
        }
    }
}

/// Spot is a base map and the objects placed on it, in placement order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Spot {
    pub map: String,
    pub objects: Vec<SpotObject>,
}

/// index returns value's place among values, adding it when new.
fn index<T: PartialEq + Clone>(values: &mut Vec<T>, value: &T) -> u32 {
    let held = values.iter().position(|held| held == value);
    let at = held.unwrap_or_else(|| {
        values.push(value.clone());
        values.len() - 1
    });
    at as u32
}

/// encode_spot returns the spot document. Equal spots encode to equal bytes.
/// Positions and angles keep single precision.
pub fn encode_spot(spot: &Spot) -> Vec<u8> {
    // List each model and each builder once, in the order first seen.
    let (mut models, mut builders) = (Vec::new(), Vec::new());
    let mut objects = Vec::new();
    for object in &spot.objects {
        let model = index(&mut models, &object.model);
        let builder = match object.placed_by {
            0 => 0,
            placed_by => index(&mut builders, &placed_by) + 1,
        };
        let (position, facing) = (object.position, object.facing);
        let floats = [
            position.x as f32,
            position.y as f32,
            position.z as f32,
            facing.pitch as f32,
            facing.yaw as f32,
            facing.roll as f32,
            if object.scale == 1.0 {
                0.0
            } else {
                object.scale
            },
            object.bounce,
        ];

        // A field at its zero is left out, as the schema's encoding has it.
        let mut out = Vec::new();
        if model != 0 {
            wire::uint32(&mut out, 1, model);
        }
        for (number, value) in (2..).zip(floats) {
            if value != 0.0 {
                wire::float(&mut out, number, value);
            }
        }
        if builder != 0 {
            wire::uint32(&mut out, 10, builder);
        }
        objects.push(out);
    }

    // Write the document: the map, the tables and the objects.
    let mut out = Vec::new();
    if !spot.map.is_empty() {
        wire::string(&mut out, 1, &spot.map);
    }
    for model in &models {
        wire::string(&mut out, 2, model);
    }
    wire::packed(&mut out, 3, &builders, |out, builder| {
        wire::raw::fixed64(out, *builder)
    });
    for object in &objects {
        wire::bytes(&mut out, 4, object);
    }
    out
}

/// Row is one object of a spot document, with its model and builder as
/// their places in the document's tables.
#[derive(Default)]
struct Row {
    model: u32,
    floats: [f32; 8],
    builder: u32,
}

/// decode_spot reads a spot document.
pub fn decode_spot(bytes: &[u8]) -> Result<Spot, Error> {
    // Read the map, the tables and the objects' rows.
    let mut map = String::new();
    let (mut models, mut builders, mut rows) = (Vec::new(), Vec::<u64>::new(), Vec::new());
    let mut r = Reader::new(bytes);
    while let Some((number, kind)) = r.field()? {
        match number {
            1 => map = r.string(kind)?,
            2 => models.push(r.string(kind)?),
            3 => r.repeated(kind, wire::FIXED64, &mut builders, |r, kind| {
                r.fixed64(kind)
            })?,
            4 => rows.push(row(r.bytes(kind)?)?),
            _ => r.skip(kind)?,
        }
    }

    // Name each object's model and builder from the tables.
    let objects = rows.into_iter().map(|row| {
        let [x, y, z, pitch, yaw, roll, scale, bounce] = row.floats;
        let builder = row.builder.checked_sub(1).map(|at| at as usize);
        SpotObject {
            model: models.get(row.model as usize).cloned().unwrap_or_default(),
            position: Vector::new(f64::from(x), f64::from(y), f64::from(z)),
            facing: Angles::new(f64::from(pitch), f64::from(yaw), f64::from(roll)),
            scale: if scale == 0.0 { 1.0 } else { scale },
            bounce,
            placed_by: builder
                .and_then(|at| builders.get(at))
                .copied()
                .unwrap_or(0),
        }
    });
    Ok(Spot {
        map,
        objects: objects.collect(),
    })
}

/// row reads one object of a spot document.
fn row(bytes: &[u8]) -> Result<Row, Error> {
    let mut row = Row::default();
    let mut r = Reader::new(bytes);
    while let Some((number, kind)) = r.field()? {
        match number {
            1 => row.model = r.uint32(kind)?,
            2..=9 => row.floats[number as usize - 2] = r.float(kind)?,
            10 => row.builder = r.uint32(kind)?,
            _ => r.skip(kind)?,
        }
    }
    Ok(row)
}

/// Placed is one spot object standing in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub object: SpotObject,
    pub world: WorldObject,
    /// entity is the model's entity handle, as traces report it.
    pub entity: u32,
}

/// TRIGGERS is the trigger collision layer. Traces that look for placed
/// objects exclude it: spawn rooms and other volumes enclose the ground, and
/// a line starting inside one stops there.
pub const TRIGGERS: u64 = 1 << 2;

/// FEET_PROBE is how far below a hero's origin the bounce trace reaches.
const FEET_PROBE: f64 = 8.0;

/// LoadedSpot is a spot standing in the world. It keeps the placed objects
/// in placement order, so spot returns what a player sees.
#[derive(Debug, Default)]
pub struct LoadedSpot {
    map: String,
    placed: Vec<Placed>,
    /// entities maps each placed object's entity to its place in placed.
    entities: HashMap<u32, usize>,
}

impl LoadedSpot {
    /// new returns a spot on map with nothing placed.
    pub fn new(map: impl Into<String>) -> Self {
        Self {
            map: map.into(),
            ..Self::default()
        }
    }

    /// map is the spot's base map.
    pub fn map(&self) -> &str {
        &self.map
    }

    /// objects returns the placed objects in placement order.
    pub fn objects(&self) -> &[Placed] {
        &self.placed
    }

    /// place spawns object as a solid model and adds it to the spot. A model
    /// the game cannot place logs why and returns nothing.
    pub fn place(&mut self, object: SpotObject) -> Option<&Placed> {
        // Create the object's solid model in the world.
        let world = create_model(&ModelOptions {
            resource: object.model.clone(),
            position: object.position,
            facing: Some(object.facing),
            scale: Some(object.scale),
            solid: Some(true),
            ..ModelOptions::default()
        });

        // Skip an object the game does not create.
        let entity = world.and_then(WorldObject::entity);
        let (Some(world), Some(entity)) = (world, entity) else {
            if let Some(world) = world {
                world.remove();
            }
            log_line(format!("spot: skipped {}", object.model));
            return None;
        };

        // Index the placed object by its entity.
        self.entities.insert(entity, self.placed.len());
        self.placed.push(Placed {
            object,
            world,
            entity,
        });
        self.placed.last()
    }

    /// at returns the placed object whose entity a trace hit, if any.
    pub fn at(&self, entity: u32) -> Option<&Placed> {
        self.entities.get(&entity).map(|at| &self.placed[*at])
    }

    /// r#move puts the placed object of entity at position and facing,
    /// keeping its order. An entity not in this spot or a failed move leaves
    /// it as it was. It returns the object as it stands.
    pub fn r#move(&mut self, entity: u32, position: Vector, facing: Angles) -> Option<&Placed> {
        let at = *self.entities.get(&entity)?;
        let placed = &mut self.placed[at];
        if placed.world.r#move(&position, Some(&facing)) {
            placed.object.position = position;
            placed.object.facing = facing;
        }
        Some(&self.placed[at])
    }

    /// remove takes the placed object of entity out of the world and the
    /// spot. It reports whether the spot held it.
    pub fn remove(&mut self, entity: u32) -> bool {
        // Find the record in this spot.
        let Some(at) = self.entities.remove(&entity) else {
            return false;
        };

        // Remove the model and its record, and renumber the ones after it.
        self.placed.remove(at).world.remove();
        for place in self.entities.values_mut().filter(|place| **place > at) {
            *place -= 1;
        }
        true
    }

    /// clear removes every placed object.
    pub fn clear(&mut self) {
        for placed in self.placed.drain(..) {
            placed.world.remove();
        }
        self.entities.clear();
    }

    /// spot returns the spot as it stands.
    pub fn spot(&self) -> Spot {
        Spot {
            map: self.map.clone(),
            objects: self
                .placed
                .iter()
                .map(|placed| placed.object.clone())
                .collect(),
        }
    }

    /// bounce launches each sampled hero who stands on a bouncing object. A
    /// mode calls it with each frame's movement samples, for the heroes it
    /// watches.
    pub fn bounce(&self, movement: &[MovementSample]) {
        if !self.placed.iter().any(|placed| placed.object.bounce > 0.0) {
            return;
        }
        for sample in movement {
            if !sample.grounded || sample.velocity.z > 0.0 {
                continue;
            }
            let feet = sample.position;
            let hit = trace(&TraceOptions {
                start: Vector::new(feet.x, feet.y, feet.z + FEET_PROBE),
                end: Vector::new(feet.x, feet.y, feet.z - FEET_PROBE),
                exclude: Some(TRIGGERS),
                ignore: vec![sample.pawn],
                ..TraceOptions::default()
            });
            let stood = hit.and_then(|hit| self.at(hit.entity));
            let bounce = stood.map_or(0.0, |placed| placed.object.bounce);
            if bounce > 0.0 {
                let velocity = sample.velocity;
                let up = Vector::new(velocity.x, velocity.y, f64::from(bounce));
                sample.player.set_velocity(&up);
            }
        }
    }
}

/// load_spot places every object of spot and returns it standing. Objects
/// the game cannot place are skipped with a log line; the rest load.
pub fn load_spot(spot: &Spot) -> LoadedSpot {
    let mut loaded = LoadedSpot::new(spot.map.clone());
    for object in &spot.objects {
        loaded.place(object.clone());
    }
    loaded
}
