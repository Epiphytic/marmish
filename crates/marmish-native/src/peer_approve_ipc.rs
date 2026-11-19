//! Candidate-local approval wire and dedicated client.
use super::{
    IO_TIMEOUT, Request as LocalRequest, Response, read_frame, verify_peer, write_frame_body,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::{
    net::UnixStream,
    time::{Instant, timeout_at},
};

pub(crate) use super::peer_selection::Selection;

const VERSION: u8 = 1;
pub(crate) const UNAVAILABLE: &str = "peer_unavailable";

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    version: u8,
    pub(super) selection: Selection,
}

impl Request {
    fn new(selection: Selection) -> Self {
        Self {
            version: VERSION,
            selection,
        }
    }

    pub(super) fn is_supported(self) -> bool {
        self.version == VERSION && self.selection.generation != 0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum State {
    Activating,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Recorded {
    version: u8,
    selection: Selection,
    state: State,
}

// Decode a typed frame directly. Going through a JSON Value for result would
// erase duplicate nested fields before the closed Recorded decoder sees them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    ok: bool,
    result: Option<Recorded>,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    Unavailable,
    Busy,
    Stale,
    Expired,
    InvalidRequest,
    Uncertain,
}

impl Failure {
    fn category(self) -> &'static str {
        match self {
            Self::Unavailable => UNAVAILABLE,
            Self::Busy => "busy",
            Self::Stale => "peer_stale",
            Self::Expired => "peer_expired",
            Self::InvalidRequest => "invalid_request",
            Self::Uncertain => "operation_uncertain",
        }
    }
}

pub(super) fn response(selection: Selection, result: Result<(), Failure>) -> Response {
    match result {
        Ok(()) => Response::success(Recorded {
            version: VERSION,
            selection,
            state: State::Activating,
        }),
        Err(error) => Response::failure(error.category()),
    }
}

fn decode(selection: Selection, envelope: Envelope) -> Result<(), Failure> {
    match (envelope.ok, envelope.result, envelope.error.as_deref()) {
        (true, Some(recorded), None)
            if recorded.version == VERSION && recorded.selection == selection =>
        {
            Ok(())
        }
        (false, None, Some(UNAVAILABLE)) => Err(Failure::Unavailable),
        (false, None, Some("busy")) => Err(Failure::Busy),
        (false, None, Some("peer_stale")) => Err(Failure::Stale),
        (false, None, Some("peer_expired")) => Err(Failure::Expired),
        (false, None, Some("invalid_request")) => Err(Failure::InvalidRequest),
        _ => Err(Failure::Uncertain),
    }
}

#[allow(
    dead_code,
    reason = "candidate approval CLI is attached only during actor composition"
)]
pub(crate) async fn client_request(path: &Path, selection: Selection) -> Result<(), Failure> {
    let request = Request::new(selection);
    if !request.is_supported() {
        return Err(Failure::InvalidRequest);
    }
    let deadline = Instant::now() + IO_TIMEOUT;
    let mut stream = timeout_at(deadline, UnixStream::connect(path))
        .await
        .map_err(|_| Failure::Unavailable)?
        .map_err(|_| Failure::Unavailable)?;
    verify_peer(&stream).map_err(|_| Failure::Unavailable)?;
    if Instant::now() >= deadline {
        return Err(Failure::Unavailable);
    }

    let response = timeout_at(deadline, async {
        write_frame_body(&mut stream, &LocalRequest::PeerApprove { request }).await?;
        read_frame(&mut stream).await
    })
    .await
    .map_err(|_| Failure::Uncertain)?
    .map_err(|_| Failure::Uncertain)?;
    decode(selection, response)
}

#[cfg(test)]
#[path = "peer_approve_ipc_tests.rs"]
mod tests;
