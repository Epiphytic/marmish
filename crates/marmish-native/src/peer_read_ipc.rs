//! Local candidate reads contain observations only and carry no authority.
use super::{IO_TIMEOUT, Request as LocalRequest, Response, exchange};
use crate::Error;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(crate) use super::peer_selection::Selection;

const LIST_LIMIT: usize = 16;
pub(super) const UNAVAILABLE: &str = "peer_unavailable";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    List {},
    Show { selection: Selection },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Pending,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub(crate) selection: Selection,
    pub(crate) state: State,
    pub(crate) held_at: u64,
    pub(crate) expires_at: u64,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Reply {
    List { entries: Vec<Entry> },
    Show { entry: Entry },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Failure {
    Unavailable,
    Busy,
    Timeout,
    InvalidRequest,
    InvalidResponse,
}

fn decode_reply(request: Request, response: Response) -> Result<Reply, Failure> {
    if response.command.is_some() || response.stream.is_some() || response.terminal_stream.is_some()
    {
        return Err(Failure::InvalidResponse);
    }
    let value = match (response.ok, response.result, response.error.as_deref()) {
        (true, Some(value), None) => value,
        (false, None, Some(UNAVAILABLE)) => return Err(Failure::Unavailable),
        (false, None, Some("busy")) => return Err(Failure::Busy),
        (false, None, Some("invalid_request")) => return Err(Failure::InvalidRequest),
        _ => return Err(Failure::InvalidResponse),
    };
    let reply = serde_json::from_value(value).map_err(|_| Failure::InvalidResponse)?;
    let valid = match (&reply, request) {
        (Reply::List { entries }, Request::List {}) => {
            entries.len() <= LIST_LIMIT
                && entries.iter().enumerate().all(|(index, entry)| {
                    entry.held_at < entry.expires_at
                        && entries[..index]
                            .iter()
                            .all(|prior| prior.selection != entry.selection)
                })
        }
        (Reply::Show { entry }, Request::Show { selection }) => {
            entry.selection == selection && entry.held_at < entry.expires_at
        }
        _ => false,
    };
    valid.then_some(reply).ok_or(Failure::InvalidResponse)
}

// The candidate frontend is wired together with the checked owner.
#[allow(dead_code)]
pub(crate) async fn client_request(path: &Path, request: Request) -> Result<Reply, Failure> {
    if matches!(request, Request::Show { selection } if selection.generation == 0) {
        return Err(Failure::InvalidRequest);
    }
    let response = exchange(path, &LocalRequest::PeerRead { request }, IO_TIMEOUT)
        .await
        .map_err(|error| match error {
            Error::Timeout => Failure::Timeout,
            Error::Invalid("ipc_frame_json" | "ipc_frame_size") => Failure::InvalidResponse,
            _ => Failure::Unavailable,
        })?;
    decode_reply(request, response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn selection() -> Selection {
        Selection {
            server: nostr::key::Keys::generate().public_key(),
            offer_id: [0xab; 16],
            revision: [0xcd; 32],
            generation: 1,
        }
    }

    #[test]
    fn reply_is_closed_bounded_and_matched_to_request() {
        let entry = Entry {
            selection: selection(),
            state: State::Pending,
            held_at: 100,
            expires_at: 101,
        };
        let show = Request::Show {
            selection: entry.selection,
        };
        let valid = || Response::success(Reply::Show { entry });
        assert_eq!(decode_reply(show, valid()), Ok(Reply::Show { entry }));
        assert_eq!(
            decode_reply(
                Request::Show {
                    selection: Selection {
                        generation: 2,
                        ..entry.selection
                    },
                },
                valid(),
            ),
            Err(Failure::InvalidResponse)
        );
        assert_eq!(
            decode_reply(Request::List {}, valid()),
            Err(Failure::InvalidResponse)
        );
        let mut extra = valid();
        extra.result.as_mut().unwrap()["entry"]["qualified_at"] = json!(99);
        assert_eq!(decode_reply(show, extra), Err(Failure::InvalidResponse));
        let mut mixed = valid();
        mixed.command = Some(crate::command_local::LocalResult::LocalBusy);
        assert_eq!(decode_reply(show, mixed), Err(Failure::InvalidResponse));
        let mut internal_state = valid();
        internal_state.result.as_mut().unwrap()["entry"]["state"] = json!("activating");
        assert_eq!(
            decode_reply(show, internal_state),
            Err(Failure::InvalidResponse)
        );
        assert_eq!(
            decode_reply(
                show,
                Response::success(Reply::Show {
                    entry: Entry {
                        expires_at: 100,
                        ..entry
                    }
                }),
            ),
            Err(Failure::InvalidResponse)
        );
        let entries: Vec<_> = (1..=LIST_LIMIT)
            .map(|generation| Entry {
                selection: Selection {
                    generation: generation as u64,
                    ..entry.selection
                },
                ..entry
            })
            .collect();
        assert_eq!(
            decode_reply(
                Request::List {},
                Response::success(Reply::List {
                    entries: entries.clone()
                }),
            ),
            Ok(Reply::List {
                entries: entries.clone()
            })
        );
        let over_limit = entries
            .into_iter()
            .chain([Entry {
                selection: Selection {
                    generation: 17,
                    ..entry.selection
                },
                ..entry
            }])
            .collect();
        for entries in [vec![entry, entry], over_limit] {
            assert_eq!(
                decode_reply(Request::List {}, Response::success(Reply::List { entries })),
                Err(Failure::InvalidResponse)
            );
        }
    }

    #[tokio::test]
    async fn candidate_unavailable_and_raw_writes_do_not_reach_ordinary_queue() {
        use crate::ipc;
        use std::os::unix::fs::PermissionsExt as _;
        use tokio::{
            net::UnixStream,
            sync::{mpsc, watch},
        };

        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("s");
        let listener = ipc::bind(&path).unwrap();
        let (_status, receiver) = watch::channel(Response::success(json!({"state":"active"})));
        let (queue, mut incoming) = mpsc::channel(1);
        let task = tokio::spawn(ipc::serve(listener, queue, receiver));

        let selection = selection();
        for request in [Request::List {}, Request::Show { selection }] {
            let wire = LocalRequest::PeerRead { request };
            assert!(!wire.is_command());
            assert!(wire.uses_short_exchange());
            assert_eq!(
                client_request(&path, request).await,
                Err(Failure::Unavailable)
            );
        }

        let coordinates = serde_json::to_value(selection).unwrap();
        let mut invalid_npub = coordinates.clone();
        let server = coordinates["server"].as_str().unwrap().to_ascii_uppercase();
        invalid_npub["server"] = json!(server);
        let mut invalid_hex = coordinates.clone();
        invalid_hex["offer_id"] = json!("AB".repeat(16));
        let mut zero_generation = coordinates.clone();
        zero_generation["generation"] = json!(0);

        for request in [
            json!({"kind":"approve","selection":coordinates}),
            json!({"kind":"decline","selection":coordinates}),
            json!({"kind":"list","extra":true}),
            json!({"kind":"show","selection":invalid_npub}),
            json!({"kind":"show","selection":invalid_hex}),
            json!({"kind":"show","selection":zero_generation}),
        ] {
            let response: Response = tokio::time::timeout(IO_TIMEOUT, async {
                let mut stream = UnixStream::connect(&path).await?;
                ipc::write_frame(
                    &mut stream,
                    &json!({"operation":"peer_read","request": request}),
                )
                .await?;
                ipc::read_frame(&mut stream).await
            })
            .await
            .unwrap()
            .unwrap();
            assert_eq!(
                decode_reply(Request::List {}, response),
                Err(Failure::InvalidRequest)
            );
        }
        assert!(matches!(
            incoming.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        let status = exchange(&path, &LocalRequest::Status {}, IO_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(status.result, Some(json!({"state":"active"})));

        let command = LocalRequest::CommandInvocations {
            after: None,
            limit: 1,
        };
        let (result, ()) = tokio::join!(ipc::command_request(&path, &command), async {
            let pending = tokio::time::timeout(IO_TIMEOUT, incoming.recv())
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(
                pending.request,
                LocalRequest::CommandInvocations { limit: 1, .. }
            ));
            assert!(
                pending
                    .respond
                    .send(Response::command(
                        crate::command_local::LocalResult::LocalBusy
                    ))
                    .is_ok()
            );
        });
        assert!(matches!(
            result,
            Ok(crate::command_local::LocalResult::LocalBusy)
        ));
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }

    #[tokio::test(start_paused = true)]
    async fn candidate_client_reports_timeout_at_one_exchange_deadline() {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("s");
        let listener = super::super::bind(&path).unwrap();

        let started = tokio::time::Instant::now();
        assert_eq!(
            client_request(&path, Request::List {}).await,
            Err(Failure::Timeout)
        );
        assert_eq!(started.elapsed(), IO_TIMEOUT);
        drop(listener);
    }
}
