//! RaceGate stands a checkpoint or finish gate across a lane: a window of
//! thin glowing lines inside a racing checker band, round the window's sides
//! and top, in Tron cyan by default. A runner reads it at once as "go
//! through here":
//!
//! ```ignore
//! let foot = ground(spot, Ground::default()).unwrap_or(spot);
//! let gate = RaceGate::stand(&RaceGateOptions::at(foot, 90.0));
//! gate.remove();
//! ```
//!
//! The band is world text: long strings of square glyphs, which keep drawing
//! from far off where single glyphs stop. Each strip draws four times,
//! offset a little across and up, so the squares fill their cells and touch
//! at the corners, and each copy stands a step deeper than the last so the
//! copies do not flicker against each other. The window is engine beams
//! between named points. World text and beams ignore alpha, so the band's
//! outer ring fakes a fade with a darker color.

use std::cell::Cell;

use crate::generated::{
    create_entity, create_particle, create_text, fire_input, remove_entity, show_entity_only_to,
    Angles, EntityOptions, EntityValue, KeyValue, ParticleOptions, TextAlign, TextOptions,
    TextVerticalAlign, Vector, WorldObject,
};

/// RaceGateOptions place a gate and choose its look.
#[derive(Clone, Debug, PartialEq)]
pub struct RaceGateOptions {
    /// foot is the road point under the middle of the gate's opening.
    pub foot: Vector,
    /// yaw is the direction a runner passes through the gate, in degrees.
    pub yaw: f64,
    /// width is the opening's width, 112 units by default, in whole cells.
    pub width: f64,
    /// height is the opening's height, 144 units by default, in whole cells.
    pub height: f64,
    /// color lights the window and the band, 0xRRGGBBAA, Tron cyan by
    /// default.
    pub color: u32,
    /// faded colors the band's outer ring, a darker cyan by default.
    pub faded: u32,
    /// viewer is the one player slot that sees the gate; everyone when None.
    pub viewer: Option<i32>,
}

impl RaceGateOptions {
    /// at returns the options of a gate of the default size and look that
    /// stands at foot across the direction yaw.
    pub fn at(foot: Vector, yaw: f64) -> Self {
        Self {
            foot,
            yaw,
            width: 112.0,
            height: 144.0,
            color: TRON,
            faded: TRON_FADED,
            viewer: None,
        }
    }
}

/// CELL is the side of one checker square, in world units.
const CELL: f64 = 8.0;

/// MARGIN is how many cells deep the band is round the window.
const MARGIN: i64 = 3;

/// BLOCK fills a checker cell: U+25A0, a square glyph.
const BLOCK: &str = "■";

/// GAP is an empty cell: the full-width space, as wide as the block.
const GAP: &str = "\u{3000}";

/// BLOCK_SCALE gives the block glyph an advance of one cell.
const BLOCK_SCALE: f32 = 4.05;

/// INK_SPREAD grows the glyph's ink to its cell's edges, in world units.
const INK_SPREAD: f64 = 0.6;

/// INK_DEPTH parts the four copies of a strip in depth, enough that the
/// depth buffer keeps them apart instead of flickering between them.
const INK_DEPTH: f64 = 0.15;

/// COLUMN_RISE aligns a turned column's glyph origin with its cell's edge.
const COLUMN_RISE: f64 = 1.75;

/// ROW_ACROSS aligns a centered row's ink with its cells.
const ROW_ACROSS: f64 = -1.2;

/// ROW_RISE centers a row's ink on its cells.
const ROW_RISE: f64 = 0.5;

/// FRAME_WIDTH is the width of the window's outer line.
const FRAME_WIDTH: f32 = 2.0;

/// RAIL_INSET is how far the window's inner rail runs inside its outer line.
const RAIL_INSET: f64 = 8.0;

/// RAIL_ALPHA is the inner rail's opacity, of 0xff.
const RAIL_ALPHA: u32 = 0x60;

/// TRON is the default glow, a bright cyan.
const TRON: u32 = 0x3ef4_ffff;

/// TRON_FADED is the default outer ring, a dark cyan.
const TRON_FADED: u32 = 0x1470_7aff;

/// ANCHOR_PARTICLE draws nothing; a beam's end rides it.
const ANCHOR_PARTICLE: &str = "particles/generic/null.vpcf";

/// BEAM_MATERIAL is the flat material the window draws with. The game loads
/// it in every world, so a gate draws without a precache.
const BEAM_MATERIAL: &str = "materials/physics/wireframe_basic.vmat";

/// RACE_GATE_RESOURCES are what a gate draws with, for a mod's precache.
pub const RACE_GATE_RESOURCES: [&str; 2] = [ANCHOR_PARTICLE, BEAM_MATERIAL];

thread_local! {
    /// NEXT_POINT numbers the beams' named points, keeping each name unique.
    static NEXT_POINT: Cell<u64> = const { Cell::new(0) };
}

/// RaceGate is a gate standing in the world, which remove takes down.
#[derive(Debug, Default)]
pub struct RaceGate {
    /// objects are the band's texts and the beams' carrier particles.
    objects: Vec<WorldObject>,
    /// entities are the beams and their named points.
    entities: Vec<u32>,
}

/// Plane is the gate's plane: its foot, its yaw in radians, and its across.
struct Plane {
    foot: Vector,
    yaw: f64,
    across: (f64, f64),
}

impl Plane {
    /// at places a point side units across the gate, up units over its foot
    /// and deep units past it along the lane.
    fn at(&self, side: f64, up: f64, deep: f64) -> Vector {
        Vector {
            x: self.foot.x + side * self.across.0 + self.yaw.cos() * deep,
            y: self.foot.y + side * self.across.1 + self.yaw.sin() * deep,
            z: self.foot.z + up,
        }
    }
}

impl RaceGate {
    /// stand stands a gate across yaw at foot. A part the world refuses is
    /// left out, so the gate stands whatever the world made.
    pub fn stand(options: &RaceGateOptions) -> RaceGate {
        // Size the opening in whole cells, half a cell rounding up as the
        // other languages' libraries round it, and face its plane across
        // the lane.
        let radians = options.yaw.to_radians();
        let half_width = (options.width / CELL / 2.0 + 0.5).floor() * CELL;
        let height = (options.height / CELL + 0.5).floor() * CELL;
        let color = options.color;
        let plane = Plane {
            foot: options.foot,
            yaw: radians,
            across: (-radians.sin(), radians.cos()),
        };

        // Draw the window's outer line and inner rail, then the band round
        // them.
        let mut gate = RaceGate::default();
        let rail = (color & !0xff) | RAIL_ALPHA;
        gate.outline(&plane, half_width, height, 0.0, color, FRAME_WIDTH);
        gate.outline(
            &plane,
            half_width,
            height,
            RAIL_INSET,
            rail,
            FRAME_WIDTH / 2.0,
        );
        gate.band(&plane, half_width, height, color, options.faded);

        // Show the gate to its one viewer, when it has one.
        if let Some(viewer) = options.viewer {
            for object in &gate.objects {
                object.show_only_to(&[viewer]);
            }
            for entity in &gate.entities {
                show_entity_only_to(*entity, &[viewer]);
            }
        }
        gate
    }

    /// remove takes every part of the gate out of the world.
    pub fn remove(&mut self) {
        for entity in self.entities.drain(..) {
            remove_entity(entity);
        }
        for object in self.objects.drain(..) {
            object.remove();
        }
    }

    /// band stands the checker band round the window: long columns down its
    /// sides and long rows over its top, every other cell filled.
    fn band(&mut self, plane: &Plane, half_width: f64, height: f64, lit: u32, faded: u32) {
        // Count the cells: the band reaches MARGIN cells past the sides and
        // top.
        let width = half_width + MARGIN as f64 * CELL;
        let columns = (width * 2.0 / CELL) as i64;
        let opening_rows = (height / CELL) as i64;
        let rows = opening_rows + MARGIN;

        // Each side column runs from under the outer row to the road. It
        // starts on the cell its checker parity fills, so the pattern runs
        // round the corners.
        for column in 0..columns {
            if column >= MARGIN && column < columns - MARGIN {
                continue;
            }
            let first = rows - 2 - ((column + rows - 2) % 2);
            let count = first.div_euclid(2) + 1;
            let outer = column == 0 || column == columns - 1;
            self.strip(
                plane,
                Strip {
                    text: blocks(count),
                    side: -width + (column as f64 + 0.5) * CELL,
                    up: (first + 1) as f64 * CELL + COLUMN_RISE,
                    pitch: 90.0,
                    color: if outer { faded } else { lit },
                },
            );
        }

        // Each row over the opening runs between the side columns; the outer
        // row spans the whole band.
        for row in opening_rows..rows {
            let outer = row == rows - 1;
            let (start, end) = if outer {
                (0, columns)
            } else {
                (MARGIN, columns - MARGIN)
            };
            let first = start + ((start + row) % 2);
            let count = (end - first + 1).div_euclid(2);
            self.strip(
                plane,
                Strip {
                    text: blocks(count),
                    side: -width + ((first + count) as f64 - 0.5) * CELL + ROW_ACROSS,
                    up: (row as f64 + 0.5) * CELL + ROW_RISE,
                    pitch: 0.0,
                    color: if outer { faded } else { lit },
                },
            );
        }
    }

    /// strip stands a strip as four copies, offset by INK_SPREAD across and
    /// up so the ink fills each cell, each copy a step of INK_DEPTH deeper.
    fn strip(&mut self, plane: &Plane, strip: Strip) {
        let yaw = plane.yaw.to_degrees() - 90.0;
        let down = strip.pitch == 90.0;
        for dx in [-1.0, 1.0] {
            for dy in [-1.0, 1.0] {
                let side = strip.side + dx * INK_SPREAD;
                let up = strip.up + dy * INK_SPREAD;
                let deep = INK_DEPTH * (dx + 2.0 * dy + 3.0) / 2.0;
                let shown = create_text(&TextOptions {
                    text: strip.text.clone(),
                    position: plane.at(side, up, deep),
                    facing: Some(Angles::new(strip.pitch, yaw, 90.0)),
                    font_size: Some(100.0),
                    scale: Some(BLOCK_SCALE),
                    color: Some(strip.color),
                    align: Some(if down {
                        TextAlign::Left
                    } else {
                        TextAlign::Center
                    }),
                    vertical_align: Some(TextVerticalAlign::Center),
                    ..TextOptions::default()
                });
                self.objects.extend(shown);
            }
        }
    }

    /// outline strings beams round the window's four corners, inset from its
    /// outer line.
    fn outline(
        &mut self,
        plane: &Plane,
        half_width: f64,
        height: f64,
        inset: f64,
        color: u32,
        width: f32,
    ) {
        // Name one point at each corner, shared by the two beams that meet
        // there.
        let side = half_width - inset;
        let corners = [
            plane.at(-side, inset, 0.0),
            plane.at(side, inset, 0.0),
            plane.at(side, height - inset, 0.0),
            plane.at(-side, height - inset, 0.0),
        ];
        let named: Vec<Option<String>> = corners.iter().map(|at| self.anchor(*at)).collect();
        let Some(names) = named.into_iter().collect::<Option<Vec<String>>>() else {
            return;
        };

        // Join each corner to the next, the last back to the first.
        let key = |key: &str, value: EntityValue| KeyValue {
            key: key.to_owned(),
            value: Some(value),
        };
        for (index, corner) in corners.iter().enumerate() {
            let beam = create_entity(&EntityOptions {
                designer_name: "env_beam".to_owned(),
                position: *corner,
                key_values: vec![
                    key("LightningStart", EntityValue::Text(names[index].clone())),
                    key(
                        "LightningEnd",
                        EntityValue::Text(names[(index + 1) % 4].clone()),
                    ),
                    key("texture", EntityValue::Text(BEAM_MATERIAL.to_owned())),
                    key("BoltWidth", EntityValue::Number(width)),
                    key("NoiseAmplitude", EntityValue::Number(0.0)),
                    key("TextureScroll", EntityValue::Integer(0)),
                    key("life", EntityValue::Number(0.0)),
                    key("rendercolor", EntityValue::Color(color)),
                    key("renderamt", EntityValue::Integer(255)),
                    key("spawnflags", EntityValue::Integer(1)),
                ],
                ..EntityOptions::default()
            });
            self.entities.extend(beam);
        }
    }

    /// anchor spawns a named point at position for a beam's end, riding an
    /// empty particle, and returns its name.
    fn anchor(&mut self, position: Vector) -> Option<String> {
        // Spawn the empty particle the point rides.
        let carrier = create_particle(&ParticleOptions {
            resource: ANCHOR_PARTICLE.to_owned(),
            position,
            ..ParticleOptions::default()
        })?;
        let Some(host) = carrier.entity().filter(|host| *host != 0) else {
            carrier.remove();
            return None;
        };
        self.objects.push(carrier);

        // Name the point uniquely past a reload, and set it on the particle.
        let number = NEXT_POINT.replace(NEXT_POINT.get() + 1);
        let name = format!("racegate_{host}_{number}");
        let point = create_entity(&EntityOptions {
            designer_name: "info_particle_system".to_owned(),
            position,
            key_values: vec![KeyValue {
                key: "targetname".to_owned(),
                value: Some(EntityValue::Text(name.clone())),
            }],
            ..EntityOptions::default()
        })?;
        self.entities.push(point);
        let activator = EntityValue::Text("!activator".to_owned());
        fire_input(point, "SetParent", Some(&activator), Some(host));
        Some(name)
    }
}

/// Strip is one string of the band and where it stands.
struct Strip {
    /// text is the strip's blocks and gaps.
    text: String,
    /// side is how far across the gate the strip stands.
    side: f64,
    /// up is how high over the foot the strip stands.
    up: f64,
    /// pitch turns the strip in the gate's plane: 90 reads down, 0 across.
    pitch: f64,
    /// color is the strip's color, 0xRRGGBBAA.
    color: u32,
}

/// blocks returns count blocks with a gap between each two.
fn blocks(count: i64) -> String {
    vec![BLOCK; count.max(0) as usize].join(GAP)
}
