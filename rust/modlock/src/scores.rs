//! scores reports results to the host's score service and reads its boards.
//! A mode enrolls each participant with bind_score_player while they are
//! connected, keeps what it returns with the participant, and reports their
//! score or the match's places when the game ends, even after they left.
//! Each call ends on the mod in a later frame.
//!
//! ```ignore
//! let scorer = bind_score_player(&connection)?;
//! let report = ScoreReport { board: "kills".to_owned(), value: 12, ..ScoreReport::default() };
//! report_score(&scorer, report, |mode: &mut Mode, result| mode.reported(result));
//! ```

use std::any::Any;
use std::time::Duration;

use crate::generated::{
    call_service, BoardPage, BoardQuery, Connection, MatchReport, ScoreConnection, ScoreReport,
};
use crate::runtime;
use crate::services::{self, ServiceError};
use crate::wire::Message;
use crate::Player;

/// SERVICE names the host service that keeps scores.
const SERVICE: &str = "scores";

/// REQUEST_LIMIT is the score service's greatest request, in bytes.
const REQUEST_LIMIT: usize = 16 * 1024;

/// TOO_LARGE is the error both the library and the host give for a request
/// past the limit.
const TOO_LARGE: &str = "The score call must fit in 16 KiB";

/// PROOF_LIMIT is the greatest length of an enrollment's proof.
const PROOF_LIMIT: usize = 64;

/// PLACES_LIMIT is the most places a match reports.
const PLACES_LIMIT: usize = 64;

/// ScoreRefusal is why a score or a match was not confirmed. The report is
/// kept, with its key and time, so retry sends the same one again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScoreRefusal {
    /// message says why the host did not confirm the report.
    pub message: String,
    method: &'static str,
    payload: Vec<u8>,
}

impl ScoreRefusal {
    /// retry sends the report again and runs done on the mod with how it
    /// ended. A report refused before it was sent, such as one too large, is
    /// refused again.
    pub fn retry<M: 'static>(self, done: impl FnOnce(&mut M, Result<(), ScoreRefusal>) + 'static) {
        submit(self.method, self.payload, done);
    }
}

impl std::fmt::Display for ScoreRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// identity returns the fields the host needs to check a live connection.
fn identity(connection: &Connection) -> ScoreConnection {
    ScoreConnection {
        slot: connection.player.slot(),
        steam_id: connection.steam_id,
        generation: connection.generation,
        proof: String::new(),
    }
}

/// bind_score_player enrolls a live connection for reporting, while the mode
/// serves it. Keep the result with the participant until the score or match
/// is reported. It returns None when the host did not enroll the connection.
pub fn bind_score_player(connection: &Connection) -> Option<ScoreConnection> {
    // Ask the host to verify the identity before it can be kept for scoring.
    let mut payload = Vec::new();
    identity(connection).encode(&mut payload);
    let reply = call_service(SERVICE, "Bind", &payload)?;
    let bound = ScoreConnection::decode(&reply).ok()?;
    (!bound.proof.is_empty()).then_some(bound)
}

/// report_score reports an enrolled player's result to the board the report
/// names, and runs done on the mod in a later frame with how it ended. Give
/// the report a stable key for each round or run if the game records more
/// than one result per player and board.
pub fn report_score<M: 'static>(
    player: &ScoreConnection,
    mut report: ScoreReport,
    done: impl FnOnce(&mut M, Result<(), ScoreRefusal>) + 'static,
) {
    // Use the enrolled connection even if its original slot has been reused.
    report.player = Player::of(player.slot);
    report.connection = Some(player.clone());
    let fits = player.proof.len() <= PROOF_LIMIT;
    submit("Report", encoded(&report, fits), done);
}

/// report_match reports the enrolled players' places together, with shared
/// places for ties, and runs done on the mod in a later frame with how it
/// ended. Each place carries the enrollment of its player.
pub fn report_match<M: 'static>(
    report: MatchReport,
    done: impl FnOnce(&mut M, Result<(), ScoreRefusal>) + 'static,
) {
    // Check the count before encoding the cohort.
    if report.places.is_empty() || report.places.len() > PLACES_LIMIT {
        let refusal = ScoreRefusal {
            message: "The match needs one to 64 places".to_owned(),
            method: "Match",
            payload: Vec::new(),
        };
        runtime::soon(move |m| ended(m, done, Err(refusal)));
        return;
    }
    let proofs = report
        .places
        .iter()
        .filter_map(|place| place.connection.as_ref());
    let fits = proofs
        .map(|held| held.proof.len())
        .all(|length| length <= PROOF_LIMIT);
    submit("Match", encoded(&report, fits), done);
}

/// read_board reads a fresh page of the board the query names, with viewer's
/// own row when the mode names a live viewer, and runs done on the mod in a
/// later frame with the page or why there is none.
pub fn read_board<M: 'static>(
    mut query: BoardQuery,
    viewer: Option<&Connection>,
    done: impl FnOnce(&mut M, Result<BoardPage, String>) + 'static,
) {
    // A viewer row belongs to the current connection named in this read.
    if let Some(viewer) = viewer {
        query.player = Some(viewer.player);
        query.connection = Some(identity(viewer));
    }
    let payload = encoded(&query, true);
    if payload.is_empty() {
        runtime::soon(move |m| ended(m, done, Err(TOO_LARGE.to_owned())));
        return;
    }
    let read = Box::new(
        move |m: &mut dyn Any, reply: Result<Vec<u8>, ServiceError>| {
            let page = reply
                .map_err(|error| error.message)
                .and_then(|reply| BoardPage::decode(&reply).map_err(|error| error.0.to_owned()));
            ended(m, done, page);
        },
    );
    services::call_later(SERVICE, "Read", &payload, Duration::ZERO, read);
}

/// encoded returns the request's bytes, or none for a request past the
/// limit or one whose parts do not fit.
fn encoded(request: &impl Message, fits: bool) -> Vec<u8> {
    let mut payload = Vec::new();
    if fits {
        request.encode(&mut payload);
    }
    if payload.len() > REQUEST_LIMIT {
        payload.clear();
    }
    payload
}

/// ended runs done on the mod with result.
fn ended<M: 'static, T>(m: &mut dyn Any, done: impl FnOnce(&mut M, T), result: T) {
    if let Some(m) = m.downcast_mut::<M>() {
        done(m, result);
    }
}

/// submit sends the report's bytes, the same on each retry, and runs done on
/// the mod with how it ended. Empty bytes are a report too large to send.
fn submit<M: 'static>(
    method: &'static str,
    payload: Vec<u8>,
    done: impl FnOnce(&mut M, Result<(), ScoreRefusal>) + 'static,
) {
    if payload.is_empty() {
        let message = TOO_LARGE.to_owned();
        let refusal = ScoreRefusal {
            message,
            method,
            payload,
        };
        runtime::soon(move |m| ended(m, done, Err(refusal)));
        return;
    }
    let sent = payload.clone();
    let confirmed = Box::new(
        move |m: &mut dyn Any, reply: Result<Vec<u8>, ServiceError>| {
            let result = reply.map(|_| ()).map_err(|error| ScoreRefusal {
                message: error.message,
                method,
                payload: sent,
            });
            ended(m, done, result);
        },
    );
    services::call_later(SERVICE, method, &payload, Duration::ZERO, confirmed);
}
