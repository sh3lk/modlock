//! frame_clock times a mod's frames and the systems inside them, so a slow
//! server's log shows how much of its frame is the mod's and which system
//! spends it. Each system calls lap once it finishes its part of the frame.
//! A minute's slowest frame is named by its own laps, since a system that
//! spikes once a minute ranks far below the steady ones by mean.
//!
//! ```ignore
//! fn frame(&mut self, event: &FrameEvent) {
//!     frame_clock::begin();
//!     self.move_bots();
//!     frame_clock::lap("bots");
//!     self.draw_hud();
//!     frame_clock::lap("hud");
//!     frame_clock::end(event.tick);
//! }
//! ```

use std::cell::RefCell;
use std::time::Instant;

use crate::generated::log_line;

/// REPORT_TICKS is a minute of ticks, the period of each report.
const REPORT_TICKS: u64 = 3840;

/// REPORTED_SYSTEMS is how many of the costliest systems a report names.
const REPORTED_SYSTEMS: usize = 8;

/// REPORTED_LAPS is how many laps of the slowest frame a report names.
const REPORTED_LAPS: usize = 3;

/// Cost is one system's time over the current minute, in milliseconds.
struct Cost {
    system: String,
    total: f64,
    max: f64,
}

/// Clock times the frames of one mod.
#[derive(Default)]
struct Clock {
    /// frames holds the milliseconds each frame of the current minute took.
    frames: Vec<f64>,
    /// costs holds each system's time, in the order first charged.
    costs: Vec<Cost>,
    /// laps holds the current frame's systems and milliseconds.
    laps: Vec<(String, f64)>,
    /// slowest holds the laps of the minute's slowest frame, which took
    /// slowest_ms, or None when no frame ran.
    slowest: Vec<(String, f64)>,
    slowest_ms: Option<f64>,
    start: Option<Instant>,
    mark: Option<Instant>,
    /// report_at is the tick that ends the current minute, zero before the
    /// first frame.
    report_at: u64,
}

thread_local! {
    /// CLOCK is the mod's one frame clock.
    static CLOCK: RefCell<Clock> = RefCell::default();
}

/// milliseconds returns the time since from, or zero before a frame began.
fn milliseconds(from: Option<Instant>) -> f64 {
    from.map_or(0.0, |from| from.elapsed().as_secs_f64() * 1e3)
}

/// begin starts timing a frame.
pub fn begin() {
    CLOCK.with(|clock| {
        let mut clock = clock.borrow_mut();
        clock.start = Some(Instant::now());
        clock.mark = clock.start;
        clock.laps.clear();
    });
}

/// elapsed is the milliseconds since the frame began.
pub fn elapsed() -> f64 {
    CLOCK.with(|clock| milliseconds(clock.borrow().start))
}

/// lap charges the time since the previous lap, or the frame's start, to
/// system.
pub fn lap(system: &str) {
    CLOCK.with(|clock| {
        // Measure from the previous lap.
        let mut clock = clock.borrow_mut();
        let spent = milliseconds(clock.mark);
        clock.mark = Some(Instant::now());
        clock.laps.push((system.to_owned(), spent));

        // Add it to the system's minute.
        match clock.costs.iter_mut().find(|cost| cost.system == system) {
            Some(cost) => {
                cost.total += spent;
                cost.max = cost.max.max(spent);
            }
            None => clock.costs.push(Cost {
                system: system.to_owned(),
                total: spent,
                max: spent,
            }),
        }
    });
}

/// end finishes the frame and, once a minute of ticks passed, logs the
/// minute's median and slowest frame with the systems that cost the most:
/// each one's mean and slowest milliseconds a frame, and the slowest frame's
/// costliest laps, to the server log. The minute ends on the first frame at
/// or past its last tick, since the host skips a slow mod's frames and may
/// skip that tick.
pub fn end(tick: u64) {
    let report = CLOCK.with(|clock| clock.borrow_mut().end(tick));
    if let Some(report) = report {
        log_line(report);
    }
}

impl Clock {
    /// end collects the frame and returns the minute's report once the
    /// minute ended.
    fn end(&mut self, tick: u64) -> Option<String> {
        // Collect the frame, keeping the slowest one's laps, until the
        // minute ends. The lists swap rather than copy.
        let elapsed = milliseconds(self.start);
        self.frames.push(elapsed);
        if self.slowest_ms.is_none_or(|slowest| elapsed > slowest) {
            self.slowest_ms = Some(elapsed);
            std::mem::swap(&mut self.slowest, &mut self.laps);
        }
        if self.report_at == 0 {
            self.report_at = tick + REPORT_TICKS;
        }
        if tick < self.report_at {
            return None;
        }
        self.report_at = tick + REPORT_TICKS;

        // Write the minute's frames and costliest systems, and start the
        // next.
        self.frames.sort_by(f64::total_cmp);
        let count = self.frames.len();
        let p50 = self.frames[count / 2];
        let max = self.frames[count - 1];
        self.costs.sort_by(|a, b| b.total.total_cmp(&a.total));
        let costliest = self.costs.iter().take(REPORTED_SYSTEMS);
        let systems: Vec<String> = costliest
            .map(|cost| {
                let mean = cost.total / count as f64;
                format!("{} {mean:.2}/{:.1}", cost.system, cost.max)
            })
            .collect();
        self.slowest.sort_by(|a, b| b.1.total_cmp(&a.1));
        let longest = self.slowest.iter().take(REPORTED_LAPS);
        let laps: Vec<String> = longest
            .map(|(system, spent)| format!("{system} {spent:.1}"))
            .collect();
        let report = format!(
            "frame work: p50 {p50:.1} max {max:.1} ms over {count} frames; mean/max ms: {}; slowest frame ms: {}",
            systems.join(", "),
            laps.join(", "),
        );
        self.frames.clear();
        self.costs.clear();
        self.slowest_ms = None;
        Some(report)
    }
}
