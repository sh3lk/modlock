//! probe_run runs probes. A probe is a mod's self-check: a run on a real server that waits for each
//! step of the behavior it checks, then writes one verdict line and quits
//! the server, so a script can start the server and read whether the mod
//! works.
//!
//! ```ignore
//! let mut run = ProbeRun::new("Duel", 120.0);
//!
//! fn frame(&mut self, event: &FrameEvent) {
//!     let fighters = self.fighters.len();
//!     self.run.tick(event.time_seconds, "two fighters", || fighters >= 2);
//! }
//! ```
//!
//! The server log then reads "probe: waiting for two fighters" once, and
//! "Duel probe PASS" or "probe failed: ..." and "Duel probe FAIL" at the
//! end.

use crate::generated::{log_line, server_command};

/// Check is what a probe's check answers: Pass when the probe passes, Fail
/// with why when it fails, or Wait to check again next frame. A check may
/// return a bool, true for Pass and false for Wait, or a failure's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    Pass,
    Fail(String),
    Wait,
}

impl From<bool> for Check {
    fn from(passed: bool) -> Self {
        if passed {
            Check::Pass
        } else {
            Check::Wait
        }
    }
}

impl From<String> for Check {
    fn from(failure: String) -> Self {
        Check::Fail(failure)
    }
}

impl From<&str> for Check {
    fn from(failure: &str) -> Self {
        Check::Fail(failure.to_owned())
    }
}

/// ProbeRun drives one probe to a verdict. It times the run from its first
/// frame, logs each step the probe waits on once, fails a run that outlasts
/// its timeout, and reports PASS or FAIL before quitting the server. A frame
/// costs the check alone; the run makes host calls only when the step
/// changes and at the verdict.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeRun {
    name: String,
    timeout: f64,
    started: Option<f64>,
    finished: bool,
    logged: Option<String>,
}

impl ProbeRun {
    /// new returns the run whose verdict line reports name, such as the
    /// mod's, and which timeout bounds in seconds.
    pub fn new(name: impl Into<String>, timeout: f64) -> Self {
        Self {
            name: name.into(),
            timeout,
            started: None,
            finished: false,
            logged: None,
        }
    }

    /// elapsed returns the seconds since the run's first frame, starting it
    /// at now.
    pub fn elapsed(&mut self, now: f64) -> f64 {
        now - *self.started.get_or_insert(now)
    }

    /// finished reports whether the run reached its verdict.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// tick runs check for the frame at now until the run ends. step names
    /// what the probe waits on.
    pub fn tick<C: Into<Check>>(&mut self, now: f64, step: &str, check: impl FnOnce() -> C) {
        self.run(now, step, check, None::<fn() -> String>);
    }

    /// tick_described runs check as tick does, and adds what describe
    /// returns, the probe's state, to a timeout after a colon.
    pub fn tick_described<C: Into<Check>>(
        &mut self,
        now: f64,
        step: &str,
        check: impl FnOnce() -> C,
        describe: impl FnOnce() -> String,
    ) {
        self.run(now, step, check, Some(describe));
    }

    /// run runs check for the frame at now until the run ends, and adds what
    /// describe returns to a timeout when the probe gave one.
    fn run<C: Into<Check>>(
        &mut self,
        now: f64,
        step: &str,
        check: impl FnOnce() -> C,
        describe: Option<impl FnOnce() -> String>,
    ) {
        // Check the current step until it resolves or times out.
        if self.finished {
            return;
        }
        let result = if self.elapsed(now) > self.timeout {
            let state = describe.map_or_else(String::new, |describe| format!(": {}", describe()));
            Check::Fail(format!("timed out waiting for {step}{state}"))
        } else {
            check().into()
        };
        let failure = match result {
            Check::Wait => {
                if self.logged.as_deref() != Some(step) {
                    self.logged = Some(step.to_owned());
                    log_line(format!("probe: waiting for {step}"));
                }
                return;
            }
            Check::Pass => None,
            Check::Fail(failure) => Some(failure),
        };

        // Report the verdict and quit the server.
        self.finished = true;
        if let Some(failure) = &failure {
            log_line(format!("probe failed: {failure}"));
        }
        let verdict = if failure.is_none() { "PASS" } else { "FAIL" };
        log_line(format!("{} probe {verdict}", self.name));
        server_command("quit");
    }
}
