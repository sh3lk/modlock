//! A puppet is a hero that follows a recorded track: where a hero stood,
//! looked and how much health it had at a fixed step, played back with the
//! controls of a player. It drives a bot added with add_bot or any other
//! player's hero.
//!
//! ```ignore
//! let bot = BotOptions { name: "Replay".to_owned(), team: 2, ..BotOptions::default() };
//! let puppet = Puppet::spawn(bot, track, PuppetOptions::default())?;
//! puppet.set_ghost(true);
//! puppet.play();
//! ```
//!
//! The library moves every playing puppet once per frame from the game
//! clock.

use std::cell::RefCell;
use std::rc::Rc;

use crate::generated::{
    add_bot, hold_modifier_state, write_field, Angles, BotOptions, FieldType, FieldValue,
    FrameEvent, Vector,
};
use crate::runtime::{self, players};
use crate::Player;

/// TICKS_PER_SECOND is the game's tick rate; a track's steps count ticks.
pub const TICKS_PER_SECOND: f64 = 64.0;

/// GHOST_STATES are the modifier states of a ghost: it takes no damage,
/// enemies and units neither target nor count it, and its unit status stays
/// hidden.
pub const GHOST_STATES: [&str; 6] = [
    "MODIFIER_STATE_INVULNERABLE",
    "MODIFIER_STATE_NO_TARGET",
    "MODIFIER_STATE_IGNORED_BY_UNIT_FILTER",
    "MODIFIER_STATE_IGNORED_BY_NPC_TARGETING",
    "MODIFIER_STATE_INVISIBLE_ON_MINIMAP",
    "MODIFIER_STATE_UNIT_STATUS_HIDDEN",
];

/// GHOST_ALPHA is how opaque a ghost is drawn, from 0 to 255.
const GHOST_ALPHA: u32 = 110;

/// RENDER_TRANS_COLOR is the render mode that blends a model by its color
/// alpha.
const RENDER_TRANS_COLOR: f64 = 1.0;

/// LONGEST_FRAME bounds the time a frame advances playback, in seconds.
const LONGEST_FRAME: f64 = 0.25;

/// SNAP_UNITS is the distance beyond which a hero teleports to its pose,
/// unless the mod gives another.
const SNAP_UNITS: f64 = 200.0;

/// Track is a recorded hero path in columns: sample i was taken at tick
/// i * step_ticks. The columns of x, y, z and yaw are as long as the track.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    /// step_ticks is the ticks between two samples.
    pub step_ticks: f64,
    /// x, y and z are the hero's origin, at its feet.
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Vec<f64>,
    /// yaw is where the hero faces, in degrees.
    pub yaw: Vec<f64>,
    /// pitch is where the hero looks up or down, in degrees; level when
    /// None.
    pub pitch: Option<Vec<f64>>,
    /// health is the hero's health; the hero's own is kept when None.
    pub health: Option<Vec<f64>>,
}

impl Track {
    /// ticks is the tick of the track's last sample.
    pub fn ticks(&self) -> f64 {
        self.x.len().saturating_sub(1) as f64 * self.step_ticks
    }

    /// pose_at reads the track at tick, blending the two samples around it:
    /// positions and health linearly, angles along the shorter arc. A tick
    /// past either end reads that end. It returns None for a track without
    /// samples.
    pub fn pose_at(&self, tick: f64) -> Option<Pose> {
        // Find the samples around the tick and how far between them it
        // falls.
        let last = self.x.len().checked_sub(1)?;
        let at = (tick / self.step_ticks).clamp(0.0, last as f64);
        let i = (at.floor() as usize).min(last.saturating_sub(1));
        let j = (i + 1).min(last);
        let t = at - i as f64;

        // Blend the columns, and take the velocity from the segment's slope.
        let seconds = self.step_ticks / TICKS_PER_SECOND;
        let blend = |column: &[f64]| lerp(sample(column, i), sample(column, j), t);
        let slope = |column: &[f64]| (sample(column, j) - sample(column, i)) / seconds;
        let velocity = if i == j {
            Vector::default()
        } else {
            Vector::new(slope(&self.x), slope(&self.y), slope(&self.z))
        };
        Some(Pose {
            position: Vector::new(blend(&self.x), blend(&self.y), blend(&self.z)),
            facing: Angles {
                pitch: self.pitch.as_deref().map_or(0.0, blend),
                yaw: lerp_angle(sample(&self.yaw, i), sample(&self.yaw, j), t),
                roll: 0.0,
            },
            velocity,
            health: self.health.as_deref().map(blend),
        })
    }
}

/// Pose is a track read at one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vector,
    pub facing: Angles,
    /// velocity is the motion between the two samples the pose lies in.
    pub velocity: Vector,
    /// health is None when the track records none.
    pub health: Option<f64>,
}

/// lerp blends a toward b by t.
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// lerp_angle blends the angle a toward b by t along the shorter way round.
fn lerp_angle(a: f64, b: f64, t: f64) -> f64 {
    let delta = (((b - a) % 360.0) + 540.0) % 360.0 - 180.0;
    a + delta * t
}

/// sample reads column at index, zero where it is short.
fn sample(column: &[f64], index: usize) -> f64 {
    column.get(index).copied().unwrap_or(0.0)
}

/// PuppetState is where playback stands: Paused holds the hero at the tick,
/// Playing advances it with the game clock, and Ended is a play that reached
/// the track's last tick, which seek or play from a lower tick restarts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PuppetState {
    #[default]
    Paused,
    Playing,
    Ended,
}

/// PuppetOptions tunes a puppet.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PuppetOptions {
    /// snap_units is the distance, in world units, beyond which the hero
    /// teleports to the pose instead of gliding to it: after a seek, or
    /// across a gap in the recording. None is 200.
    pub snap_units: Option<f64>,
    /// ghost starts the puppet as a ghost.
    pub ghost: bool,
}

/// Body is a puppet's playback: its track, where it stands in it and what it
/// last wrote to its hero.
struct Body {
    player: Player,
    track: Track,
    tick: f64,
    rate: f64,
    state: PuppetState,
    /// clock is the game time of the last frame, None until one arrives.
    clock: Option<f64>,
    /// drawn is whether the hero stands at the current tick.
    drawn: bool,
    ghost: bool,
    dead: bool,
    /// looked is the entity and ghost setting sync_look last wrote.
    looked: Option<(u32, bool)>,
    last_health: Option<i32>,
    snap_units: f64,
}

thread_local! {
    /// LIVE holds the puppets the library moves each frame.
    static LIVE: RefCell<Vec<Rc<RefCell<Body>>>> = const { RefCell::new(Vec::new()) };
}

/// Puppet plays a Track on a hero. Playback starts paused at tick 0; the
/// hero moves only while the puppet is playing or after a seek. The value is
/// a handle: its copies control one puppet, which plays until stop.
#[derive(Clone)]
pub struct Puppet {
    body: Rc<RefCell<Body>>,
}

impl Puppet {
    /// new returns the puppet that drives player's hero along track. It owns
    /// the hero's placement and, when the track records health, its health.
    pub fn new(player: Player, track: Track, options: PuppetOptions) -> Self {
        let body = Rc::new(RefCell::new(Body {
            player,
            track,
            tick: 0.0,
            rate: 1.0,
            state: PuppetState::Paused,
            clock: None,
            drawn: false,
            ghost: false,
            dead: false,
            looked: None,
            last_health: None,
            snap_units: options.snap_units.unwrap_or(SNAP_UNITS),
        }));
        runtime::hook(|hooks| hooks.puppets = Some(advance_puppets));
        LIVE.with(|live| live.borrow_mut().push(body.clone()));
        let puppet = Puppet { body };
        if options.ghost {
            puppet.set_ghost(true);
        }
        puppet
    }

    /// spawn adds a bot and returns a puppet for it, or None when no bot
    /// could join. The bot appears at the track's first pose, unless bot
    /// names a place.
    pub fn spawn(bot: BotOptions, track: Track, options: PuppetOptions) -> Option<Self> {
        let start = track.pose_at(0.0).map(|pose| pose.position);
        let player = add_bot(&BotOptions {
            position: bot.position.or(start),
            ..bot
        })?;
        Some(Puppet::new(player, track, options))
    }

    /// player is the player whose hero the puppet drives.
    pub fn player(&self) -> Player {
        self.body.borrow().player
    }

    /// state is where playback stands.
    pub fn state(&self) -> PuppetState {
        self.body.borrow().state
    }

    /// tick is the playback position, in track ticks.
    pub fn tick(&self) -> f64 {
        self.body.borrow().tick
    }

    /// rate is the playback speed: 1 is real time, 0.5 half speed, negative
    /// runs backward.
    pub fn rate(&self) -> f64 {
        self.body.borrow().rate
    }

    /// set_rate sets the playback speed.
    pub fn set_rate(&self, rate: f64) {
        self.body.borrow_mut().rate = rate;
    }

    /// is_ghost reports whether the puppet is a ghost.
    pub fn is_ghost(&self) -> bool {
        self.body.borrow().ghost
    }

    /// play starts or resumes playback; from the end it restarts at tick 0.
    pub fn play(&self) {
        let mut body = self.body.borrow_mut();
        if body.state == PuppetState::Ended && body.rate >= 0.0 {
            body.seek(0.0);
        }
        body.state = PuppetState::Playing;
    }

    /// pause holds the hero where it stands.
    pub fn pause(&self) {
        self.body.borrow_mut().state = PuppetState::Paused;
    }

    /// seek moves playback to tick, clamped to the track, and places the
    /// hero there on the next frame. Playback keeps its state, except that
    /// an ended puppet is paused at the new tick.
    pub fn seek(&self, tick: f64) {
        self.body.borrow_mut().seek(tick);
    }

    /// set_ghost draws the hero translucent and holds the modifier states of
    /// GHOST_STATES, so it takes no damage and is not targeted; off restores
    /// both. The hero's collision with other heroes is the game's own.
    pub fn set_ghost(&self, ghost: bool) {
        let mut body = self.body.borrow_mut();
        body.ghost = ghost;
        if let Some(pawn) = body.player.pawn() {
            body.sync_look(pawn.entity);
        }
    }

    /// stop ends the puppet and removes its bot; a hero that is not a bot
    /// stays.
    pub fn stop(&self) {
        // Stop moving, and draw the hero solid again.
        LIVE.with(|live| {
            let mut live = live.borrow_mut();
            live.retain(|held| !Rc::ptr_eq(held, &self.body));
        });
        let mut body = self.body.borrow_mut();
        if let Some(pawn) = body.player.pawn().filter(|_| body.ghost) {
            body.ghost = false;
            body.sync_look(pawn.entity);
        }
        let slot = body.player.slot();
        let mut connections = players().into_iter();
        if connections.any(|held| held.player.slot() == slot && held.bot) {
            body.player.remove_bot();
        }
    }

    /// advance moves playback to the game clock at seconds and places the
    /// hero. The library calls it each frame; a mod calls it only to step a
    /// puppet by hand.
    pub fn advance(&self, seconds: f64) {
        self.body.borrow_mut().advance(seconds);
    }
}

/// advance_puppets moves every playing puppet to the game clock of the
/// frame.
fn advance_puppets(event: &FrameEvent) {
    let live = LIVE.with(|live| live.borrow().clone());
    for body in live {
        body.borrow_mut().advance(event.time_seconds);
    }
}

impl Body {
    /// seek moves playback to tick, clamped to the track.
    fn seek(&mut self, tick: f64) {
        self.tick = tick.clamp(0.0, self.track.ticks());
        self.drawn = false;
        self.last_health = None;
        if self.state == PuppetState::Ended {
            self.state = PuppetState::Paused;
        }
    }

    /// sync_look writes the ghost look and states to the hero entity when
    /// the entity or the ghost setting differs from what it last wrote, so a
    /// hero that respawned takes them again.
    fn sync_look(&mut self, entity: u32) {
        // Skip a hero that already wears the look.
        if self.looked == Some((entity, self.ghost)) {
            return;
        }
        self.looked = Some((entity, self.ghost));
        for state in GHOST_STATES {
            hold_modifier_state(entity, state, Some(self.ghost));
        }
        let (alpha, mode) = if self.ghost {
            (GHOST_ALPHA, RENDER_TRANS_COLOR)
        } else {
            (255, 0.0)
        };
        let class = "CBaseModelEntity";
        let mode = FieldValue::Number(mode);
        write_field(entity, class, "m_nRenderMode", FieldType::Uint8, &mode);
        let color = FieldValue::Number(f64::from(0xff_ffff | (alpha << 24)));
        write_field(entity, class, "m_clrRender", FieldType::Uint32, &color);
    }

    /// advance moves playback to the game clock at seconds and places the
    /// hero.
    fn advance(&mut self, seconds: f64) {
        // Playback moves by the game time since the last frame.
        let elapsed = (seconds - self.clock.unwrap_or(seconds)).min(LONGEST_FRAME);
        self.clock = Some(seconds);
        if self.state == PuppetState::Playing && elapsed > 0.0 {
            self.tick += elapsed * TICKS_PER_SECOND * self.rate;
            let end = self.track.ticks();
            if self.tick >= end || self.tick <= 0.0 {
                self.tick = self.tick.clamp(0.0, end);
                self.state = PuppetState::Ended;
            }
            self.drawn = false;
        }

        // A paused hero is placed once per seek, a playing one every frame.
        if !self.drawn {
            self.place();
        }
    }

    /// place puts the hero at the current tick; it retries next frame
    /// without a hero.
    fn place(&mut self) {
        // The hero must exist to be placed.
        let (Some(pawn), Some(pose)) = (self.player.pawn(), self.track.pose_at(self.tick)) else {
            return;
        };
        self.sync_look(pawn.entity);

        // A large gap teleports; otherwise the hero glides with the recorded
        // motion.
        let gap = pose.position.distance(pawn.position);
        let placed = if gap > self.snap_units || self.state != PuppetState::Playing {
            self.player.teleport(&pose.position, &pose.facing, None)
        } else {
            let velocity = Some(&pose.velocity);
            self.player.r#move(&pose.position, &pose.facing, velocity)
        };
        if placed {
            self.drawn = true;
        }
        self.set_health(pawn.entity, pose.health);
    }

    /// set_health gives the hero the recorded health. A recorded zero kills
    /// the hero once; the first health above zero after that respawns it,
    /// and the next frame places it.
    fn set_health(&mut self, entity: u32, health: Option<f64>) {
        // A recorded death or revival changes whether the hero lives.
        let Some(health) = health else {
            return;
        };
        if health <= 0.0 {
            if !self.dead {
                self.dead = self.player.kill();
            }
            return;
        }
        if self.dead {
            self.dead = !self.player.respawn();
            self.drawn = false;
            return;
        }
        let rounded = (health + 0.5).floor() as i32;
        if self.last_health != Some(rounded) {
            let value = FieldValue::Number(f64::from(rounded));
            write_field(entity, "CBaseEntity", "m_iHealth", FieldType::Int32, &value);
            self.last_health = Some(rounded);
        }
    }
}
