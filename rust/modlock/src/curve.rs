//! curve builds camera curves: values over time for play_camera to send to a
//! player's camera. A curve is a list of spans played one after another. A
//! span the game's lerp can draw, straight or shaped by its bias and gain,
//! plays as one lerp; any other span is sampled into short lerps. Every
//! constructor and method here builds spans, so they compose: keyframes
//! through points, smooth curves, speeds, orbits, waves, chains, repeats and
//! reversals.

use std::f64::consts::PI;
use std::rc::Rc;

use crate::generated::{CameraValue, Vector};

/// LerpShape is the shape the game's own lerp gives a move: Valve's bias and
/// gain curves, each linear at 0.5. A value left None is linear.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LerpShape {
    /// bias below 0.5 starts slow and speeds up; above, starts fast.
    pub bias: Option<f64>,
    /// gain above 0.5 starts and ends slow; below, fast at both ends.
    pub gain: Option<f64>,
}

/// Ease maps the fraction of a span played, 0 to 1, to the fraction moved.
/// Linear, Bias and Gain are the eases the game draws itself, as is a Custom
/// one made with a lerp shape, so a move with one plays as one lerp.
#[derive(Clone, Copy, Debug, Default)]
pub enum Ease {
    /// Linear moves at an even pace.
    #[default]
    Linear,
    /// Bias is the game's bias curve; see LerpShape.
    Bias(f64),
    /// Gain is the game's gain curve; see LerpShape.
    Gain(f64),
    /// In starts slow and speeds up.
    In,
    /// Out starts fast and slows to a stop.
    Out,
    /// InOut starts and ends slow.
    InOut,
    /// Sine starts and ends slow, more gently than InOut.
    Sine,
    /// Back passes its end a little and settles back onto it.
    Back,
    /// Elastic overshoots its end and rings down onto it.
    Elastic,
    /// Bounce drops onto its end and bounces to rest.
    Bounce,
    /// Custom moves by the function a mod gives; shaped makes one.
    Custom(Shaped),
}

/// Shaped is the ease a mod makes with shaped: the function it moves by and
/// the shape the game draws it with, if it has one.
#[derive(Clone, Copy)]
pub struct Shaped {
    at: &'static dyn Fn(f64) -> f64,
    lerp: Option<LerpShape>,
}

impl std::fmt::Debug for Shaped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shaped")
            .field("lerp", &self.lerp)
            .finish_non_exhaustive()
    }
}

/// shaped is the ease that moves by at, drawn by the game with lerp when it
/// has one, so the game plays a span of it as one lerp from its start to its
/// end; without one the span is sampled. The ease keeps at for as long as
/// the mod runs, so a mod makes each ease once and uses it again.
pub fn shaped(at: impl Fn(f64) -> f64 + 'static, lerp: Option<LerpShape>) -> Ease {
    Ease::Custom(Shaped {
        at: Box::leak(Box::new(at)),
        lerp,
    })
}

impl Ease {
    /// at returns the fraction moved a fraction t of the way through.
    pub fn at(self, t: f64) -> f64 {
        match self {
            Ease::Linear => t,
            Ease::Bias(b) => bias(t, b),
            Ease::Gain(g) => gain(t, g),
            Ease::In => t * t * t,
            Ease::Out => 1.0 - (1.0 - t).powi(3),
            Ease::InOut if t < 0.5 => 4.0 * t * t * t,
            Ease::InOut => 1.0 - (2.0 - 2.0 * t).powi(3) / 2.0,
            Ease::Sine => (1.0 - (PI * t).cos()) / 2.0,
            Ease::Back => 1.0 + 2.70158 * (t - 1.0).powi(3) + 1.70158 * (t - 1.0).powi(2),
            Ease::Elastic if t <= 0.0 || t >= 1.0 => t,
            Ease::Elastic => {
                2f64.powf(-10.0 * t) * (((t * 10.0 - 0.75) * 2.0 * PI) / 3.0).sin() + 1.0
            }
            Ease::Bounce => bounce(t),
            Ease::Custom(shaped) => (shaped.at)(t),
        }
    }

    /// lerp is the shape the game draws the ease with, if it can.
    pub fn lerp(self) -> Option<LerpShape> {
        match self {
            Ease::Linear => Some(LerpShape::default()),
            Ease::Bias(bias) => Some(LerpShape {
                bias: Some(bias),
                gain: None,
            }),
            Ease::Gain(gain) => Some(LerpShape {
                bias: None,
                gain: Some(gain),
            }),
            Ease::Custom(shaped) => shaped.lerp,
            _ => None,
        }
    }
}

/// bounce is four parabolas, each a smaller bounce than the one before.
fn bounce(t: f64) -> f64 {
    let k = 7.5625;
    if t < 1.0 / 2.75 {
        k * t * t
    } else if t < 2.0 / 2.75 {
        k * (t - 1.5 / 2.75).powi(2) + 0.75
    } else if t < 2.5 / 2.75 {
        k * (t - 2.25 / 2.75).powi(2) + 0.9375
    } else {
        k * (t - 2.625 / 2.75).powi(2) + 0.984375
    }
}

/// bias is Valve's Bias: t raised to the power that maps 0.5 to b.
fn bias(t: f64, b: f64) -> f64 {
    t.powf(b.ln() / 0.5f64.ln())
}

/// gain is Valve's Gain: two mirrored bias halves meeting at the middle.
fn gain(t: f64, g: f64) -> f64 {
    if t < 0.5 {
        bias(2.0 * t, 1.0 - g) / 2.0
    } else {
        1.0 - bias(2.0 - 2.0 * t, 1.0 - g) / 2.0
    }
}

/// Span is one piece of a curve.
#[derive(Clone)]
pub struct Span {
    /// duration is how long the span plays, in seconds.
    pub duration: f64,
    /// lerp marks a span the game plays as one lerp from at(0) to at(1) with
    /// this shape; a span without it is sampled.
    pub lerp: Option<LerpShape>,
    value: Rc<dyn Fn(f64) -> CameraValue>,
}

impl Span {
    /// new returns the span that plays for duration seconds the values at
    /// gives for each fraction of the way through, 0 to 1.
    pub fn new(
        duration: f64,
        lerp: Option<LerpShape>,
        at: impl Fn(f64) -> CameraValue + 'static,
    ) -> Self {
        Self {
            duration,
            lerp,
            value: Rc::new(at),
        }
    }

    /// at returns the value a fraction u of the way through, 0 to 1.
    pub fn at(&self, u: f64) -> CameraValue {
        (self.value)(u)
    }
}

/// Curve is a list of spans played in order.
#[derive(Clone, Default)]
pub struct Curve {
    pub spans: Vec<Span>,
}

impl Curve {
    /// then plays next after the curve.
    pub fn then(mut self, next: Curve) -> Curve {
        self.spans.extend(next.spans);
        self
    }

    /// reverse plays the curve backward. A gain curve is its own mirror and
    /// stays one lerp; a biased span backward is no bias curve, so it is
    /// sampled.
    pub fn reverse(&self) -> Curve {
        let backward = self.spans.iter().rev().map(|span| {
            let forward = span.clone();
            let lerp = span.lerp.filter(|shape| shape.bias.is_none());
            Span::new(span.duration, lerp, move |u| forward.at(1.0 - u))
        });
        Curve {
            spans: backward.collect(),
        }
    }

    /// repeat plays the curve a number of times.
    pub fn repeat(&self, times: usize) -> Curve {
        chain((0..times).map(|_| self.clone()))
    }

    /// ping_pong plays the curve forward and then backward.
    pub fn ping_pong(&self) -> Curve {
        self.clone().then(self.reverse())
    }

    /// duration returns how long the curve plays, in seconds.
    pub fn duration(&self) -> f64 {
        self.spans.iter().map(|span| span.duration.max(0.0)).sum()
    }

    /// value_at returns the curve's value a number of seconds in, or its last
    /// value once it has ended. An empty curve has no value.
    pub fn value_at(&self, seconds: f64) -> Option<CameraValue> {
        let mut left = seconds;
        for span in &self.spans {
            if span.duration <= 0.0 {
                continue;
            }
            if left <= span.duration {
                return Some(span.at(left.max(0.0) / span.duration));
            }
            left -= span.duration;
        }
        self.spans.last().map(|span| span.at(1.0))
    }
}

/// Key is one point of a keyframed curve.
#[derive(Clone, Debug)]
pub struct Key {
    /// at is the time of the key, in seconds after the curve starts.
    pub at: f64,
    /// value is the param's value at the key.
    pub value: CameraValue,
    /// ease shapes the move from the key before this one.
    pub ease: Ease,
}

impl Key {
    /// new returns the key that holds value at seconds, reached at an even
    /// pace.
    pub fn new(at: f64, value: impl Into<CameraValue>) -> Self {
        Self {
            at,
            value: value.into(),
            ease: Ease::Linear,
        }
    }

    /// ease shapes the move from the key before this one.
    pub fn ease(mut self, ease: Ease) -> Self {
        self.ease = ease;
        self
    }
}

/// line moves from one value to another over a duration.
pub fn line(
    from: impl Into<CameraValue>,
    to: impl Into<CameraValue>,
    duration: f64,
    ease: Ease,
) -> Curve {
    let (from, to) = (from.into(), to.into());
    Curve {
        spans: vec![Span::new(duration, ease.lerp(), move |u| {
            mix(&from, &to, ease.at(u))
        })],
    }
}

/// hold keeps a value for a duration.
pub fn hold(value: impl Into<CameraValue>, duration: f64) -> Curve {
    let value = value.into();
    Curve {
        spans: vec![Span::new(duration, Some(LerpShape::default()), move |_| {
            value.clone()
        })],
    }
}

/// keyframes passes through keys in time order. A key's ease shapes the move
/// into it; two keys at one time make a jump, and two with one value a hold.
/// The curve starts at the first key, so a first key after zero waits there.
/// smooth passes through the keys on a curve, without corners, instead of a
/// straight line from each to the next.
pub fn keyframes(keys: &[Key], smooth: bool) -> Curve {
    // A first key after zero holds its value until its time.
    let mut curve = Curve::default();
    if let Some(first) = keys.first().filter(|first| first.at > 0.0) {
        curve = hold(first.value.clone(), first.at);
    }
    for (index, to) in keys.iter().enumerate().skip(1) {
        // Each pair of keys is one span; a jump takes no time and adds none.
        let from = &keys[index - 1];
        if to.at <= from.at {
            continue;
        }
        let duration = to.at - from.at;
        if !smooth {
            curve = curve.then(line(
                from.value.clone(),
                to.value.clone(),
                duration,
                to.ease,
            ));
            continue;
        }

        // A smooth span bends toward the keys on either side, on a
        // Catmull-Rom spline, which passes through every key.
        let before = index.checked_sub(2).map_or(from, |at| &keys[at]);
        let after = keys.get(index + 1).unwrap_or(to);
        let points = [before, from, to, after].map(|key| key.value.clone());
        let ease = to.ease;
        curve.spans.push(Span::new(duration, None, move |u| {
            spline(&points, ease.at(u))
        }));
    }
    curve
}

/// Through times a curve through points by its speed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Through {
    /// speed is how far the param moves each second.
    pub speed: f64,
    /// ease shapes each move between two points.
    pub ease: Ease,
    /// smooth passes through the points on a curve, as keyframes does.
    pub smooth: bool,
}

/// through moves through points at a speed, so each move takes as long as
/// its distance needs.
pub fn through(points: &[CameraValue], options: Through) -> Curve {
    let mut at = 0.0;
    let mut last: Option<&CameraValue> = None;
    let keys: Vec<Key> = points
        .iter()
        .map(|value| {
            if let Some(last) = last {
                at += distance(last, value) / options.speed;
            }
            last = Some(value);
            Key::new(at, value.clone()).ease(options.ease)
        })
        .collect();
    keyframes(&keys, options.smooth)
}

/// Orbit circles a point.
#[derive(Clone, Copy, Debug)]
pub struct Orbit {
    /// center is the point the circle goes around.
    pub center: Vector,
    /// radius is the circle's radius.
    pub radius: f64,
    /// duration is how long the orbit takes, in seconds.
    pub duration: f64,
    /// turns is how many times it goes around, 1 unless set. Negative turns
    /// clockwise.
    pub turns: f64,
    /// start is the angle it starts at, in degrees from the x axis.
    pub start: f64,
    /// rise is how far it climbs over the whole orbit.
    pub rise: f64,
    /// ease shapes the pace around the circle.
    pub ease: Ease,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            center: Vector::default(),
            radius: 0.0,
            duration: 0.0,
            turns: 1.0,
            start: 0.0,
            rise: 0.0,
            ease: Ease::Linear,
        }
    }
}

/// orbit circles a point, for the target param, as a spiral when it rises.
pub fn orbit(options: Orbit) -> Curve {
    // Angles run in radians from the start angle, around the z axis.
    let Orbit { center, radius, .. } = options;
    let start = options.start.to_radians();
    let span = Span::new(options.duration, None, move |u| {
        let t = options.ease.at(u);
        let angle = start + t * options.turns * 2.0 * PI;
        CameraValue::Position(Vector {
            x: center.x + radius * angle.cos(),
            y: center.y + radius * angle.sin(),
            z: center.z + options.rise * t,
        })
    });
    Curve { spans: vec![span] }
}

/// Wave swings a number around zero.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wave {
    /// amplitude is how far the first swing reaches.
    pub amplitude: f64,
    /// frequency is how many full swings it makes each second.
    pub frequency: f64,
    /// duration is how long it swings, in seconds.
    pub duration: f64,
    /// decay shrinks the swings to nothing by the end, as a shake settles;
    /// without it they keep their size.
    pub decay: bool,
}

/// wave swings a number to either side of zero and ends at zero.
pub fn wave(options: Wave) -> Curve {
    let span = Span::new(options.duration, None, move |u| {
        let size = if options.decay { 1.0 - u } else { 1.0 };
        let swing = (2.0 * PI * options.frequency * options.duration * u).sin();
        number(options.amplitude * size * swing)
    });
    Curve { spans: vec![span] }
}

/// chain plays curves one after another.
pub fn chain(curves: impl IntoIterator<Item = Curve>) -> Curve {
    Curve {
        spans: curves.into_iter().flat_map(|curve| curve.spans).collect(),
    }
}

/// number returns the camera value of a number.
fn number(value: f64) -> CameraValue {
    CameraValue::Number(value as f32)
}

/// mix returns the value a fraction t of the way from a to b.
fn mix(a: &CameraValue, b: &CameraValue, t: f64) -> CameraValue {
    if let (CameraValue::Number(a), CameraValue::Number(b)) = (a, b) {
        let (a, b) = (f64::from(*a), f64::from(*b));
        return number(a + (b - a) * t);
    }
    CameraValue::Position(vector(a).lerp(vector(b), t))
}

/// spline returns the Catmull-Rom point a fraction t from the second of
/// points to the third.
fn spline(points: &[CameraValue; 4], t: f64) -> CameraValue {
    let f = |a: f64, b: f64, c: f64, d: f64| {
        0.5 * (2.0 * b
            + (c - a) * t
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t
            + (3.0 * b - a - 3.0 * c + d) * t * t * t)
    };
    if let [CameraValue::Number(a), CameraValue::Number(b), CameraValue::Number(c), CameraValue::Number(d)] =
        points
    {
        return number(f(
            f64::from(*a),
            f64::from(*b),
            f64::from(*c),
            f64::from(*d),
        ));
    }
    let [a, b, c, d] = [&points[0], &points[1], &points[2], &points[3]].map(vector);
    CameraValue::Position(Vector {
        x: f(a.x, b.x, c.x, d.x),
        y: f(a.y, b.y, c.y, d.y),
        z: f(a.z, b.z, c.z, d.z),
    })
}

/// distance returns how far apart two values are.
fn distance(a: &CameraValue, b: &CameraValue) -> f64 {
    vector(a).distance(vector(b))
}

/// vector reads a value as a position; a number lies on the x axis.
fn vector(value: &CameraValue) -> Vector {
    match value {
        CameraValue::Number(number) => Vector::new(f64::from(*number), 0.0, 0.0),
        CameraValue::Position(position) => *position,
    }
}

impl From<f64> for CameraValue {
    fn from(value: f64) -> Self {
        number(value)
    }
}

impl From<f32> for CameraValue {
    fn from(value: f32) -> Self {
        CameraValue::Number(value)
    }
}

impl From<i32> for CameraValue {
    fn from(value: i32) -> Self {
        CameraValue::Number(value as f32)
    }
}

impl From<Vector> for CameraValue {
    fn from(value: Vector) -> Self {
        CameraValue::Position(value)
    }
}
