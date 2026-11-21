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

use std::cell::Cell;

use crate::curve::{chain, hold, line, wave, Curve, Ease, LerpShape, Wave};
use crate::generated::{
    fire_input, hold_modifier_state, write_field, CameraLerp, CameraMotion, CameraOptions,
    CameraParam, CameraValue, EntityValue, FieldType, FieldValue, Vector,
};
use crate::runtime::{self, Timer};
use crate::Player;

/// RATE is how many lerps each second play a span the game's lerp cannot
/// draw, unless the mod gives another rate.
const RATE: f64 = 30.0;

/// FREQUENCY is how many full swings a shake makes each second, unless the
/// mod gives another.
const FREQUENCY: f64 = 8.0;

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
        line(rest.clone(), by.clone(), pulse.rise, pulse.ease),
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
