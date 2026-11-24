//! camera moves a player's camera. set_orbit_camera gives a hero the
//! hideout's orbit camera on any map. The pawn's networked m_bInHideoutZone
//! moves its client to the hideout view: the orbit camera at the game's entry
//! level, with the hideout HUD in place of the game HUD. The wheel zooms only
//! while the pawn also holds MODIFIER_STATE_HIDEOUT_ZONE, since the server
//! ignores the wheel's orbit_camera_in and orbit_camera_out without it. The
//! wheel's first zoom adds modifier_citadel_orbit_camera, which keeps the
//! zoom level, so returning the normal camera removes it too.
//!
//! play_camera plays curves (the curve module) on move_camera's queued lerps:
//! the operations on one param in one context run in order, so a whole curve
//! goes to the client at once and plays without the mod's timers.
//!
//! entity_camera shows a player the view of a point_camera of their own,
//! which turns on all three axes, while their hero stays in play.

use std::cell::Cell;
use std::rc::Rc;

use crate::curve::{chain, hold, line, wave, Curve, Ease, LerpShape, Wave};
use crate::generated::{
    create_entity, fire_input, hold_entity_modifier, hold_modifier_state, move_entity, read_field,
    remove_entity, show_entity_only_to, write_field, Angles, CameraLerp, CameraMotion,
    CameraOptions, CameraParam, CameraValue, EntityOptions, EntityValue, FieldType, FieldValue,
    KeyValue, Vector,
};
use crate::runtime::{self, Timer};
use crate::Player;

/// RATE is how many lerps each second play a span the game's lerp cannot
/// draw, unless the mod gives another rate.
const RATE: f64 = 30.0;

/// FREQUENCY is how many full swings a shake makes each second, unless the
/// mod gives another.
const FREQUENCY: f64 = 8.0;

/// CAMERA_OVERRIDE_MODIFIER is the stock modifier whose client handler
/// declares MODIFIER_VALUE_CAMERA_ENTITY_OVERRIDE, routing the local player's
/// camera through a client C_PointCamera named pre_intro_camera, for 0.1
/// seconds with no blend, and then intro_camera, with no end, while keeping
/// their living hero active.
const CAMERA_OVERRIDE_MODIFIER: &str = "modifier_citadel_hideout_intro";

/// CAMERA_OVERRIDE_EXIT_MODIFIER is the modifier the game leaves on the hero
/// as the override ends.
const CAMERA_OVERRIDE_EXIT_MODIFIER: &str = "modifier_citadel_hideout_intro_exit";

/// PRIMARY_CAMERA_NAME is the name of the camera the override stays on.
const PRIMARY_CAMERA_NAME: &str = "intro_camera";

/// PRE_INTRO_CAMERA_NAME is the name of the camera the override cuts to
/// first.
const PRE_INTRO_CAMERA_NAME: &str = "pre_intro_camera";

/// FL_AT_CONTROLS is FL_ATCONTROLS, bit 6 of CBaseEntity.m_fFlags, which
/// CCitadel_Modifier_HideoutIntro::OnCreated sets so SetupMove skips the
/// move axes.
const FL_AT_CONTROLS: u32 = 0x40;

/// FIELD_OF_VIEW is an entity camera's horizontal field of view in degrees,
/// unless the mod gives another.
const FIELD_OF_VIEW: f64 = 90.0;

/// ENTITY_CAMERA_SETTLE_SECONDS is how long an entity camera takes to settle
/// after it opens. The game client blends into intro_camera from a snapshot
/// of the opening view for about five seconds, interpolating pitch, yaw and
/// roll as wrapped angles, so an orientation that crosses 180 degrees of
/// roll, or passes over the top in pitch, during the blend swings the view
/// the wrong way and stalls it. Hold the camera clear of those crossings,
/// such as level, until EntityCamera::settled; from then on any orientation
/// renders exactly.
pub const ENTITY_CAMERA_SETTLE_SECONDS: f64 = 6.0;

thread_local! {
    /// MOVES counts the moves started, to give each its own context.
    static MOVES: Cell<u64> = const { Cell::new(0) };
}

/// set_orbit_camera gives the player's hero the hideout's orbit camera,
/// zoomed with the wheel, or returns the normal camera and drops the wheel's
/// zoom. The hideout HUD replaces the game HUD while it is on. It returns
/// false when the player has no hero or a call fails.
pub fn set_orbit_camera(player: Player, enabled: bool) -> bool {
    // A player between lives has no hero to switch.
    let Some(pawn) = player.pawn().filter(|pawn| pawn.entity != 0) else {
        return false;
    };

    // The flag picks the view; the held state lets the wheel zoom it.
    let flag = write_field(
        pawn.entity,
        "CCitadelPlayerPawn",
        "m_bInHideoutZone",
        FieldType::Bool,
        &FieldValue::Boolean(enabled),
    );
    let zoom = hold_modifier_state(pawn.entity, "MODIFIER_STATE_HIDEOUT_ZONE", Some(enabled));
    if enabled {
        return flag && zoom;
    }

    // The wheel's orbit modifier keeps its zoom level after the flag clears.
    let modifier = EntityValue::Text("modifier_citadel_orbit_camera".to_owned());
    let orbit = fire_input(pawn.entity, "RemoveModifier", Some(&modifier), None);
    flag && zoom && orbit
}

/// EntityCameraOptions configures a player's 6-DOF point_camera override.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EntityCameraOptions {
    /// position is where the camera starts in the world.
    pub position: Vector,
    /// facing is the camera's starting orientation: pitch, yaw and full
    /// roll.
    pub facing: Angles,
    /// fov is the horizontal field of view in degrees. None is 90.
    pub fov: Option<f64>,
    /// z_near overrides the near clipping plane when given.
    pub z_near: Option<f64>,
    /// z_far overrides the far clipping plane when given.
    pub z_far: Option<f64>,
    /// smooth lets the game blend in from the hero over half a second.
    /// Without it the view snaps to the camera on its first frame, by also
    /// placing pre_intro_camera, which follows the camera until stop.
    pub smooth: bool,
    /// keep_controls clears the FL_ATCONTROLS flag that the stock
    /// camera-override modifier sets on the hero pawn, so the movement axes
    /// of the left stick and of WASD, forward and left, keep reaching
    /// MovementSample.command while the camera is open. Each move clears it
    /// again when the game has set it again, as when the modifier returns to
    /// a respawned hero. Without it the stock modifier freezes the movement
    /// axes, which suits cinematic cameras.
    pub keep_controls: bool,
}

/// EntityCamera drives one player's 6-DOF point_camera override.
pub struct EntityCamera {
    player: Player,
    entity: u32,
    /// pre_entity is the camera of the opening cut, None for a smooth
    /// opening and once the camera stopped.
    pre_entity: Cell<Option<u32>>,
    /// held_pawn is the pawn that holds the modifier itself, or None for a
    /// hold on the player.
    held_pawn: Option<u32>,
    keep_controls: bool,
    stopped: Cell<bool>,
    /// settled is set once the game's opening blend has run out.
    settled: Rc<Cell<bool>>,
    settle: Timer,
}

impl EntityCamera {
    /// entity is the player's primary point_camera entity handle.
    pub fn entity(&self) -> u32 {
        self.entity
    }

    /// settled reports whether ENTITY_CAMERA_SETTLE_SECONDS have passed since
    /// the camera opened, so it renders full roll and loops without the
    /// opening blend's swing.
    pub fn settled(&self) -> bool {
        self.settled.get() && !self.stopped.get()
    }

    /// move places the camera at position and facing, preserving full roll.
    /// It returns false once the camera stopped or when the call fails.
    pub fn r#move(&self, position: Vector, facing: Angles) -> bool {
        // A stopped camera moves nothing.
        if self.stopped.get() {
            return false;
        }

        // The modifier sets FL_ATCONTROLS each time it attaches, so check the
        // current pawn on every move rather than once per pawn handle.
        if self.keep_controls {
            if let Some(current) = self.player.pawn().filter(|pawn| pawn.entity != 0) {
                clear_at_controls(current.entity);
            }
        }

        // pre_intro_camera follows intro_camera through the opening blend.
        let moved = move_entity(self.entity, Some(&position), Some(&facing), None);
        if let Some(pre_entity) = self.pre_entity.get() {
            move_entity(pre_entity, Some(&position), Some(&facing), None);
        }
        moved
    }

    /// set_fov updates the camera's field of view in degrees. It returns
    /// false once the camera stopped or when the call fails.
    pub fn set_fov(&self, fov: f64) -> bool {
        if self.stopped.get() {
            return false;
        }
        let fov = FieldValue::Number(fov);
        let wrote = write_field(
            self.entity,
            "CPointCamera",
            "m_FOV",
            FieldType::Float32,
            &fov,
        );
        if let Some(pre_entity) = self.pre_entity.get() {
            write_field(
                pre_entity,
                "CPointCamera",
                "m_FOV",
                FieldType::Float32,
                &fov,
            );
        }
        wrote
    }

    /// stop removes the camera and the modifier and returns to the hero
    /// camera. It returns false when the hold was not released or the camera
    /// was already gone; stopping again does nothing and returns true.
    pub fn stop(&self) -> bool {
        // Stop once; the cameras and the settle timer end together.
        if self.stopped.replace(true) {
            return true;
        }
        self.settle.cancel();

        // Release the hold and take the intro and exit modifiers off the hero.
        let released = match self.held_pawn {
            Some(pawn) => hold_entity_modifier(pawn, CAMERA_OVERRIDE_MODIFIER, Some(false)),
            None => self
                .player
                .hold_modifier(CAMERA_OVERRIDE_MODIFIER, Some(false)),
        };
        if let Some(current) = self.player.pawn().filter(|pawn| pawn.entity != 0) {
            for modifier in [CAMERA_OVERRIDE_MODIFIER, CAMERA_OVERRIDE_EXIT_MODIFIER] {
                let modifier = EntityValue::Text(modifier.to_owned());
                fire_input(current.entity, "RemoveModifier", Some(&modifier), None);
            }
        }

        // Remove both cameras.
        if let Some(pre_entity) = self.pre_entity.take() {
            remove_entity(pre_entity);
        }
        let removed = remove_entity(self.entity);
        released && removed
    }
}

/// camera_keys builds the spawn key values of a point_camera named
/// targetname.
fn camera_keys(targetname: &str, options: &EntityCameraOptions) -> Vec<KeyValue> {
    let key = |key: &str, value: EntityValue| KeyValue {
        key: key.to_owned(),
        value: Some(value),
    };
    let fov = options.fov.unwrap_or(FIELD_OF_VIEW);
    let mut keys = vec![
        key("targetname", EntityValue::Text(targetname.to_owned())),
        key("FOV", EntityValue::Number(fov as f32)),
        key("UseScreenAspectRatio", EntityValue::Boolean(true)),
        key("align_with_parent", EntityValue::Boolean(false)),
    ];
    if let Some(z_near) = options.z_near {
        keys.push(key("zNear", EntityValue::Number(z_near as f32)));
    }
    if let Some(z_far) = options.z_far {
        keys.push(key("zFar", EntityValue::Number(z_far as f32)));
    }
    keys
}

/// spawn_point_camera creates one point_camera that only player receives.
fn spawn_point_camera(
    player: Player,
    targetname: &str,
    options: &EntityCameraOptions,
) -> Option<u32> {
    let entity = create_entity(&EntityOptions {
        designer_name: "point_camera".to_owned(),
        position: options.position,
        facing: Some(options.facing),
        transmit_always: Some(true),
        key_values: camera_keys(targetname, options),
        ..EntityOptions::default()
    })?;
    if !show_entity_only_to(entity, &[player.slot()]) {
        remove_entity(entity);
        return None;
    }
    Some(entity)
}

/// clear_at_controls clears FL_ATCONTROLS on entity when it is set, so the
/// server's SetupMove copies the movement axes of CUserCmd into CMoveData
/// instead of zeroing them.
fn clear_at_controls(entity: u32) {
    let flags = read_field(entity, "CBaseEntity", "m_fFlags", FieldType::Uint32);
    let Some(FieldValue::Number(flags)) = flags else {
        return;
    };
    let flags = flags as u32;
    if flags & FL_AT_CONTROLS == 0 {
        return;
    }
    let cleared = FieldValue::Number(f64::from(flags & !FL_AT_CONTROLS));
    write_field(
        entity,
        "CBaseEntity",
        "m_fFlags",
        FieldType::Uint32,
        &cleared,
    );
}

/// entity_camera gives player's living hero a 6-DOF point_camera view on any
/// map without modifying the game client. The stock client's camera manager
/// reads the origin of C_PointCamera, its full three-axis angles, including
/// 360-degree roll and over-the-top pitch loops, and its field of view every
/// frame while keeping the player on their living hero. Each player receives
/// only their own camera.
///
/// The stock modifier sets FL_ATCONTROLS on the hero, so the movement axes,
/// forward and left, read 0 while the camera is open, which suits cinematic
/// cameras. Set keep_controls to clear FL_ATCONTROLS, so WASD and the left
/// stick's axes keep arriving in MovementSample.command. While CSimpleCamera
/// is active the stock client skips the look polling of
/// CCitadel_ThirdPersonCamera, so the mouse and the right stick do not update
/// the view angles.
///
/// It returns None when the player has no living hero or a host call fails.
pub fn entity_camera(player: Player, options: EntityCameraOptions) -> Option<EntityCamera> {
    // The camera override modifier attaches to a living hero pawn.
    let pawn = player.pawn().filter(|pawn| pawn.entity != 0)?;

    // Spawn the persistent intro_camera and, for a cut on the first frame,
    // pre_intro_camera.
    let entity = spawn_point_camera(player, PRIMARY_CAMERA_NAME, &options)?;
    let mut pre_entity = None;
    if !options.smooth {
        pre_entity = spawn_point_camera(player, PRE_INTRO_CAMERA_NAME, &options);
        if pre_entity.is_none() {
            remove_entity(entity);
            return None;
        }
    }

    // Hold the stock camera-override modifier on the living hero pawn. A
    // player hold survives respawns but needs an owned ability to apply it,
    // which a freshly spawned hero may not have yet; the pawn's own hold
    // needs none.
    let mut held_pawn = None;
    if !player.hold_modifier(CAMERA_OVERRIDE_MODIFIER, Some(true)) {
        if !hold_entity_modifier(pawn.entity, CAMERA_OVERRIDE_MODIFIER, Some(true)) {
            if let Some(pre_entity) = pre_entity {
                remove_entity(pre_entity);
            }
            remove_entity(entity);
            return None;
        }
        held_pawn = Some(pawn.entity);
    }

    // Let the movement axes through when asked; the modifier froze them as
    // it attached.
    if options.keep_controls {
        clear_at_controls(pawn.entity);
    }

    // The camera settles once the game's opening blend has run out.
    let settled = Rc::new(Cell::new(false));
    let settling = settled.clone();
    let settle = runtime::repeat(ENTITY_CAMERA_SETTLE_SECONDS, move || {
        settling.set(true);
        false
    });

    Some(EntityCamera {
        player,
        entity,
        pre_entity: Cell::new(pre_entity),
        held_pawn,
        keep_controls: options.keep_controls,
        stopped: Cell::new(false),
        settled,
        settle,
    })
}

/// Play plays curves on a player's camera: a curve for each param that
/// moves. All tracks start together, so a pull back and a wider view can
/// share one timeline.
#[derive(Clone, Default)]
pub struct Play {
    tracks: Vec<(CameraParam, Curve)>,
    place: Place,
    relative: bool,
    delay: Option<f64>,
    looping: bool,
    rate: Option<f64>,
}

/// Place names where a move goes: its context, its priority and the hero.
#[derive(Clone, Debug, Default, PartialEq)]
struct Place {
    context: Option<String>,
    priority: Option<u32>,
    target: Option<u32>,
}

impl Play {
    /// new returns a move of no params.
    pub fn new() -> Self {
        Self::default()
    }

    /// track plays curve on param.
    pub fn track(mut self, param: CameraParam, curve: Curve) -> Self {
        self.tracks.push((param, curve));
        self
    }

    /// relative adds the curves' values to each param's usual value.
    pub fn relative(mut self) -> Self {
        self.relative = true;
        self
    }

    /// delay starts the tracks after this many seconds.
    pub fn delay(mut self, seconds: f64) -> Self {
        self.delay = Some(seconds);
        self
    }

    /// looping plays the tracks again each time they end, until cancelled.
    pub fn looping(mut self) -> Self {
        self.looping = true;
        self
    }

    /// rate is how many lerps each second play a span the game's lerp cannot
    /// draw, 30 unless set. A straight or bias and gain span is always one
    /// lerp.
    pub fn rate(mut self, rate: f64) -> Self {
        self.rate = Some(rate);
        self
    }

    /// context names the operations; without one the move takes a name of
    /// its own.
    pub fn context(mut self, context: impl Into<String>) -> Self {
        self.place.context = Some(context.into());
        self
    }

    /// priority decides between these tracks and another context's moves.
    pub fn priority(mut self, priority: u32) -> Self {
        self.place.priority = Some(priority);
        self
    }

    /// target is the entity handle of the hero whose camera moves.
    pub fn target(mut self, entity: u32) -> Self {
        self.place.target = Some(entity);
        self
    }
}

/// CameraMove is a set of curves playing on a player's camera.
pub struct CameraMove {
    player: Player,
    context: String,
    duration: f64,
    timer: Option<Timer>,
}

impl CameraMove {
    /// context names the move's operations.
    pub fn context(&self) -> &str {
        &self.context
    }

    /// duration is how long the move runs, in seconds; infinite when it
    /// loops.
    pub fn duration(&self) -> f64 {
        self.duration
    }

    /// cancel ends the move at once and returns each param to its usual
    /// value.
    pub fn cancel(&self) -> bool {
        if let Some(timer) = &self.timer {
            timer.cancel();
        }
        self.player.clear_camera(Some(&self.context))
    }
}

/// play_camera plays a curve for each param of the player's camera, sent at
/// once as lerps queued in one context. Each param returns to its usual
/// value after its curve ends, so a curve ends where the camera rests: at
/// zero when relative. Moves in their own contexts take turns by priority.
/// It returns None when the player has no hero or a call fails.
pub fn play_camera(player: Player, play: Play) -> Option<CameraMove> {
    // Each move takes a context of its own unless the mod names one.
    let context = play.place.context.clone().unwrap_or_else(|| {
        MOVES.with(|moves| {
            moves.set(moves.get() + 1);
            format!("modlock.camera.{}", moves.get())
        })
    });
    let lengths = play.tracks.iter().map(|(_, curve)| curve.duration());
    let length = lengths.fold(0.0, f64::max);
    let (delay, looping) = (play.delay, play.looping);
    let sending = context.clone();
    let send = move |delay: Option<f64>| {
        let mut tracks = play.tracks.iter();
        tracks.all(|(param, curve)| send_curve(player, &play, &sending, *param, delay, curve))
    };

    // A looping move queues two rounds, then one more as each ends, so the
    // queue never runs dry; it stops once the player has no hero.
    let mut timer = None;
    let mut sent = send(delay);
    if sent && looping && length > 0.0 {
        sent = send(None);
        let clearing = context.clone();
        timer = Some(runtime::repeat(length, move || {
            let queued = send(None);
            if !queued {
                player.clear_camera(Some(&clearing));
            }
            queued
        }));
    }

    // A move that half reached the camera is taken back off it.
    let duration = if looping {
        f64::INFINITY
    } else {
        delay.unwrap_or(0.0) + length
    };
    let started = CameraMove {
        player,
        context,
        duration,
        timer,
    };
    if !sent {
        started.cancel();
        return None;
    }
    Some(started)
}

/// Pulse moves a param away, holds it and brings it back.
#[derive(Clone, Debug)]
pub struct Pulse {
    /// param is the camera value the pulse moves.
    pub param: CameraParam,
    /// by is how far the param moves from its usual value.
    pub by: CameraValue,
    /// rise is how long the move out takes, in seconds.
    pub rise: f64,
    /// hold is how long the param stays out, in seconds.
    pub hold: f64,
    /// fall is how long the move back takes, in seconds.
    pub fall: f64,
    /// ease shapes the move out; the move back is its mirror.
    pub ease: Ease,
    /// context names the pulse's operations; without one it takes a name of
    /// its own.
    pub context: Option<String>,
    /// priority decides between this pulse and another context's moves.
    pub priority: Option<u32>,
    /// target is the entity handle of the hero whose camera moves.
    pub target: Option<u32>,
}

impl Pulse {
    /// new returns the pulse that moves param by an amount over rise
    /// seconds, holds it for hold and brings it back over fall, at an even
    /// pace and in a context of its own.
    pub fn new(
        param: CameraParam,
        by: impl Into<CameraValue>,
        rise: f64,
        hold: f64,
        fall: f64,
    ) -> Self {
        Self {
            param,
            by: by.into(),
            rise,
            hold,
            fall,
            ease: Ease::Linear,
            context: None,
            priority: None,
            target: None,
        }
    }
}

/// camera_pulse moves one param of the player's camera by an amount, holds
/// it there and brings it back, as an ability pulls the camera back while it
/// charges. It returns None when the player has no hero or a call fails.
pub fn camera_pulse(player: Player, pulse: Pulse) -> Option<CameraMove> {
    let by = pulse.by;
    let rest = match by {
        CameraValue::Number(_) => CameraValue::Number(0.0),
        CameraValue::Position(_) => CameraValue::Position(Vector::default()),
    };
    let curve = chain([
        line(rest.clone(), by.clone(), pulse.rise, pulse.ease.clone()),
        hold(by.clone(), pulse.hold),
        line(rest, by, pulse.fall, pulse.ease).reverse(),
    ]);
    let place = Place {
        context: pulse.context,
        priority: pulse.priority,
        target: pulse.target,
    };
    play_relative(player, pulse.param, curve, place)
}

/// Shake shakes a param around its usual value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shake {
    /// param is the camera value that shakes, the horizontal offset unless
    /// set.
    pub param: Option<CameraParam>,
    /// amplitude is how far the first swing reaches.
    pub amplitude: f64,
    /// duration is how long the shake runs, in seconds.
    pub duration: f64,
    /// frequency is how many full swings it makes each second, 8 unless set.
    pub frequency: Option<f64>,
    /// context names the shake's operations; without one it takes a name of
    /// its own.
    pub context: Option<String>,
    /// priority decides between this shake and another context's moves.
    pub priority: Option<u32>,
    /// target is the entity handle of the hero whose camera moves.
    pub target: Option<u32>,
}

/// camera_shake swings one param of the player's camera to either side of
/// its usual value, smaller each time, and settles back. It returns None
/// when the player has no hero or a call fails.
pub fn camera_shake(player: Player, shake: Shake) -> Option<CameraMove> {
    let curve = wave(Wave {
        amplitude: shake.amplitude,
        duration: shake.duration,
        frequency: shake.frequency.unwrap_or(FREQUENCY),
        decay: true,
    });
    let param = shake.param.unwrap_or(CameraParam::HorizontalOffset);
    let place = Place {
        context: shake.context,
        priority: shake.priority,
        target: shake.target,
    };
    play_relative(player, param, curve, place)
}

/// play_relative plays one curve on one param, added to its usual value.
fn play_relative(
    player: Player,
    param: CameraParam,
    curve: Curve,
    place: Place,
) -> Option<CameraMove> {
    let play = Play {
        place,
        ..Play::new().track(param, curve).relative()
    };
    play_camera(player, play)
}

/// send_curve queues one param's curve as lerps: one for a span with a lerp
/// shape, and for any other span one per sample at the rate. Only the first
/// lerp waits for the delay; the rest queue behind it.
fn send_curve(
    player: Player,
    play: &Play,
    context: &str,
    param: CameraParam,
    delay: Option<f64>,
    curve: &Curve,
) -> bool {
    // Every lerp of the track shares its context; only the first carries the
    // delay.
    let rate = play.rate.unwrap_or(RATE);
    let mut delay = delay;
    let mut lerp = |start: CameraValue, end: CameraValue, duration: f64, shape: LerpShape| {
        let motion = CameraLerp {
            start: Some(start),
            end: Some(end),
            bias: shape.bias.map(|bias| bias as f32),
            gain: shape.gain.map(|gain| gain as f32),
            duration: Some(duration as f32),
        };
        player.move_camera(&CameraOptions {
            context: Some(context.to_owned()),
            param,
            motion: Some(CameraMotion::Lerp(motion)),
            delay: delay.take().map(|delay| delay as f32),
            relative: play.relative.then_some(true),
            priority: play.place.priority,
            shared: None,
            target: play.place.target,
        })
    };

    // A span the game draws goes out as one lerp, any other as samples. An
    // empty span adds nothing to the queue.
    curve.spans.iter().all(|span| {
        if span.duration <= 0.0 {
            return true;
        }
        if let Some(shape) = span.lerp {
            return lerp(span.at(0.0), span.at(1.0), span.duration, shape);
        }

        // Samples split the span evenly, at least one per span.
        let steps = (span.duration * rate).ceil().max(1.0);
        (0..steps as u32).all(|step| {
            let (from, to) = (f64::from(step) / steps, f64::from(step + 1) / steps);
            let shape = LerpShape::default();
            lerp(span.at(from), span.at(to), span.duration / steps, shape)
        })
    })
}
