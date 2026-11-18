//! Test-only local custody requests and their paired, one-use authority.
use super::{Request as LocalRequest, Response};
use crate::{Error, Result, identity::ProfileLease};
use nostr::{
    key::PublicKey,
    nips::nip19::{FromBech32, ToBech32},
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    net::UnixListener,
    sync::{mpsc, oneshot, watch},
    time::Instant,
};

const VERSION: u8 = 1;
const LIST_LIMIT: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "Coordinates", into = "Coordinates")]
pub(crate) struct Selection {
    pub(crate) server: PublicKey,
    pub(crate) offer_id: [u8; 16],
    pub(crate) revision: [u8; 32],
    pub(crate) generation: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Coordinates {
    server: String,
    offer_id: String,
    revision: String,
    generation: u64,
}

impl TryFrom<Coordinates> for Selection {
    type Error = &'static str;

    fn try_from(raw: Coordinates) -> std::result::Result<Self, Self::Error> {
        let server = PublicKey::from_bech32(&raw.server).map_err(|_| "peer_server_npub")?;
        if server.to_bech32().ok().as_deref() != Some(&raw.server) || raw.generation == 0 {
            return Err("peer_selection");
        }
        Ok(Self {
            server,
            offer_id: lowercase_hex(&raw.offer_id).ok_or("peer_offer_id")?,
            revision: lowercase_hex(&raw.revision).ok_or("peer_revision")?,
            generation: raw.generation,
        })
    }
}

impl From<Selection> for Coordinates {
    fn from(selection: Selection) -> Self {
        Self {
            server: selection.server.to_bech32().expect("npub encoding"),
            offer_id: hex::encode(selection.offer_id),
            revision: hex::encode(selection.revision),
            generation: selection.generation,
        }
    }
}

fn lowercase_hex<const N: usize>(value: &str) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    if value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        hex::decode_to_slice(value, &mut bytes).ok()?;
        return Some(bytes);
    }
    None
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    List {},
    Show { selection: Selection },
    Approve { selection: Selection },
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireRequest {
    version: u8,
    request: Request,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Pending,
    Activating,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub(crate) selection: Selection,
    pub(crate) state: State,
    pub(crate) held_at: u64,
    pub(crate) qualified_at: u64,
    pub(crate) expires_at: u64,
    pub(crate) activating_at: Option<u64>,
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Reply {
    List { entries: Vec<Entry> },
    Show { entry: Entry },
    Approve { entry: Entry },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Failure {
    BadShape,
    Forbidden,
    Stale,
    Expired,
    Busy,
    Stopped,
    Uncertain,
    Unavailable,
}

type Verdict<T> = std::result::Result<T, Failure>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireReply {
    version: u8,
    result: Outcome,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum Outcome {
    Ok { reply: Reply },
    Failure { failure: Failure },
}

impl Reply {
    fn matches(&self, request: Request) -> bool {
        match (self, request) {
            (Self::List { entries }, Request::List {}) => entries.len() <= LIST_LIMIT,
            (Self::Show { entry }, Request::Show { selection }) => entry.selection == selection,
            (Self::Approve { entry }, Request::Approve { selection }) => {
                entry.selection == selection && entry.state == State::Activating
            }
            _ => false,
        }
    }
}

fn response(result: Verdict<Reply>) -> Response {
    let result = match result {
        Ok(reply) => Outcome::Ok { reply },
        Err(failure) => Outcome::Failure { failure },
    };
    let response = Response::success(WireReply {
        version: VERSION,
        result,
    });
    if serde_json::to_vec(&response).is_ok_and(|bytes| bytes.len() <= super::FRAME_LIMIT) {
        response
    } else {
        Response::failure("peer_response_size")
    }
}

pub(crate) async fn client_request(path: &Path, request: Request) -> Verdict<Reply> {
    let response = super::exchange(
        path,
        &LocalRequest::Peer {
            request: WireRequest {
                version: VERSION,
                request,
            },
        },
        super::IO_TIMEOUT,
    )
    .await
    .map_err(|error| match error {
        Error::Invalid("ipc_peer_uid") => Failure::Forbidden,
        _ => Failure::Uncertain,
    })?;
    if response.command.is_some() || response.stream.is_some() || response.terminal_stream.is_some()
    {
        return Err(Failure::BadShape);
    }
    let result = match (response.ok, response.result, response.error.as_deref()) {
        (true, Some(result), None) => result,
        (false, None, Some("invalid_request")) => return Err(Failure::BadShape),
        _ => return Err(Failure::BadShape),
    };
    let WireReply { version, result } =
        serde_json::from_value(result).map_err(|_| Failure::BadShape)?;
    if version != VERSION {
        return Err(Failure::BadShape);
    }
    match result {
        Outcome::Ok { reply } if reply.matches(request) => Ok(reply),
        Outcome::Ok { .. } => Err(Failure::BadShape),
        Outcome::Failure { failure } => Err(failure),
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Lifetime {
    Starting,
    Registered,
    Stopped,
}

struct Pair {
    socket: PathBuf,
    root: PathBuf,
    profile_id: String,
    lifetime: watch::Sender<Lifetime>,
}

impl Pair {
    fn belongs_to(&self, profile: &ProfileLease) -> bool {
        self.root == profile.root() && self.profile_id == profile.profile_id()
    }

    fn close(&self) {
        self.lifetime.send_replace(Lifetime::Stopped);
    }
}

pub(crate) struct Worker {
    pair: Arc<Pair>,
    queue: mpsc::Receiver<Work>,
}

pub(crate) struct Listener {
    registration: Registration,
    queue: mpsc::Sender<Work>,
}

pub(crate) struct Control(Arc<Pair>);

pub(crate) fn pair(profile: &ProfileLease) -> (Worker, Listener, Control) {
    let (queue, receive) = mpsc::channel(4);
    let (lifetime, _) = watch::channel(Lifetime::Starting);
    let pair = Arc::new(Pair {
        socket: profile.socket_path(),
        root: profile.root().to_path_buf(),
        profile_id: profile.profile_id().to_owned(),
        lifetime,
    });
    (
        Worker {
            pair: Arc::clone(&pair),
            queue: receive,
        },
        Listener {
            registration: Registration(Arc::clone(&pair)),
            queue,
        },
        Control(pair),
    )
}

impl Control {
    pub(crate) fn stop(&self) {
        self.0.close();
    }
}

impl Worker {
    pub(crate) fn control(&self) -> Control {
        Control(Arc::clone(&self.pair))
    }

    pub(crate) fn try_recv(&mut self) -> Option<Work> {
        if self.registered() {
            self.queue.try_recv().ok()
        } else {
            None
        }
    }

    pub(crate) fn registered(&self) -> bool {
        *self.pair.lifetime.borrow() == Lifetime::Registered
    }

    pub(crate) fn stopped(&self) -> bool {
        *self.pair.lifetime.borrow() == Lifetime::Stopped
    }

    pub(crate) fn close(&self) {
        self.pair.close();
    }

    pub(crate) async fn ready(&self) -> bool {
        let mut state = self.pair.lifetime.subscribe();
        state
            .wait_for(|state| *state != Lifetime::Starting)
            .await
            .is_ok_and(|state| *state == Lifetime::Registered)
    }

    pub(crate) async fn stopping(&self) {
        wait_for_stop(&self.pair).await;
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.close();
    }
}

async fn wait_for_stop(pair: &Pair) {
    let mut state = pair.lifetime.subscribe();
    let _ = state.wait_for(|state| *state == Lifetime::Stopped).await;
}

pub(super) struct Registration(Arc<Pair>);

impl Drop for Registration {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[derive(Clone)]
pub(super) struct Intake {
    pair: Arc<Pair>,
    queue: mpsc::Sender<Work>,
}

impl Listener {
    pub(super) fn register(self, bound: &UnixListener) -> Result<(Registration, Intake)> {
        let Self {
            registration,
            queue,
        } = self;
        let pair = Arc::clone(&registration.0);
        if bound.local_addr()?.as_pathname() != Some(&pair.socket) {
            return Err(Error::Invalid("peer_listener_registration"));
        }
        pair.lifetime.send_if_modified(|state| {
            if *state != Lifetime::Starting {
                return false;
            }
            *state = Lifetime::Registered;
            true
        });
        Ok((registration, Intake { pair, queue }))
    }
}

impl Intake {
    pub(super) async fn stopping(&self) {
        wait_for_stop(&self.pair).await;
    }

    async fn exchange(&self, request: Request, uid: u32, deadline: Instant) -> Verdict<Reply> {
        let (respond, receiver) = oneshot::channel();
        let work = Work {
            pair: Arc::clone(&self.pair),
            request,
            deadline,
            respond,
            witness: matches!(request, Request::Approve { .. }).then_some(Witness { uid }),
            exchange: Arc::new(()),
        };
        {
            let state = self.pair.lifetime.borrow();
            if *state != Lifetime::Registered {
                return Err(Failure::Stopped);
            }
            if Instant::now() >= deadline {
                return Err(Failure::Uncertain);
            }
            self.queue.try_send(work).map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => Failure::Busy,
                mpsc::error::TrySendError::Closed(_) => Failure::Stopped,
            })?;
        }
        tokio::select! {
            biased;
            _ = self.stopping() => Err(Failure::Uncertain),
            result = tokio::time::timeout_at(deadline, receiver) => {
                result.ok().and_then(|value| value.ok()).unwrap_or(Err(Failure::Uncertain))
            }
        }
    }
}

pub(super) async fn handle(
    intake: Option<&Intake>,
    wire: WireRequest,
    uid: u32,
    deadline: Instant,
) -> Response {
    response(match intake {
        _ if wire.version != VERSION => Err(Failure::BadShape),
        Some(intake) => intake.exchange(wire.request, uid, deadline).await,
        None => Err(Failure::Stopped),
    })
}

struct Witness {
    uid: u32,
}

pub(crate) struct Work {
    pair: Arc<Pair>,
    request: Request,
    deadline: Instant,
    respond: oneshot::Sender<Verdict<Reply>>,
    witness: Option<Witness>,
    exchange: Arc<()>,
}

impl Work {
    pub(crate) fn request(&self) -> &Request {
        &self.request
    }

    fn check_live(&self, state: Lifetime) -> Verdict<()> {
        if Instant::now() >= self.deadline || self.respond.is_closed() {
            return Err(Failure::Uncertain);
        }
        if state != Lifetime::Registered {
            return Err(Failure::Stopped);
        }
        Ok(())
    }

    pub(crate) fn can_reply(&self) -> Verdict<()> {
        self.check_live(*self.pair.lifetime.borrow())
    }

    pub(crate) fn begin_manual(
        &mut self,
        worker: &Worker,
        profile: &ProfileLease,
    ) -> Verdict<Manual> {
        let Request::Approve { selection } = self.request else {
            return Err(Failure::Forbidden);
        };
        if !Arc::ptr_eq(&self.pair, &worker.pair) || !self.pair.belongs_to(profile) {
            return Err(Failure::Forbidden);
        }
        self.can_reply()?;
        let witness = self.witness.take().ok_or(Failure::Forbidden)?;
        Ok(Manual {
            pair: Arc::clone(&self.pair),
            selection,
            exchange: Arc::clone(&self.exchange),
            witness,
        })
    }

    pub(crate) fn reply(self, result: Verdict<Reply>) {
        let state = self.pair.lifetime.borrow();
        if self.check_live(*state).is_ok() {
            let result = result.and_then(|reply| {
                if reply.matches(self.request) {
                    Ok(reply)
                } else {
                    Err(Failure::BadShape)
                }
            });
            let _ = self.respond.send(result);
        }
    }
}

pub(crate) struct Manual {
    pair: Arc<Pair>,
    selection: Selection,
    exchange: Arc<()>,
    witness: Witness,
}

impl Manual {
    pub(crate) fn authorize(
        self,
        work: &Work,
        profile: &ProfileLease,
        selection: Selection,
    ) -> Verdict<u32> {
        if !Arc::ptr_eq(&self.pair, &work.pair)
            || !self.pair.belongs_to(profile)
            || self.selection != selection
            || work.request != (Request::Approve { selection })
            || !Arc::ptr_eq(&self.exchange, &work.exchange)
        {
            return Err(Failure::Forbidden);
        }
        work.can_reply()?;
        super::verify_peer_uid(self.witness.uid).map_err(|_| Failure::Forbidden)?;
        Ok(self.witness.uid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use tokio::{io::AsyncWriteExt, net::UnixStream};

    struct RuntimeDir(PathBuf);

    impl Drop for RuntimeDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn profile(base: &Path, name: &str) -> (ProfileLease, RuntimeDir) {
        std::fs::set_permissions(base, std::fs::Permissions::from_mode(0o700)).unwrap();
        let lease = ProfileLease::acquire(base, name).unwrap();
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(lease.runtime_dir())
            .unwrap();
        let runtime = RuntimeDir(lease.runtime_dir().to_owned());
        (lease, runtime)
    }

    fn selection() -> Selection {
        Selection {
            server: nostr::key::Keys::generate().public_key(),
            offer_id: [0xab; 16],
            revision: [0xad; 32],
            generation: 1,
        }
    }

    async fn next(worker: &mut Worker) -> Work {
        loop {
            if let Some(work) = worker.try_recv() {
                return work;
            }
            tokio::task::yield_now().await;
        }
    }

    #[test]
    fn closed_canonical_requests_and_bounded_exact_replies() {
        let request = LocalRequest::Peer {
            request: WireRequest {
                version: VERSION,
                request: Request::Approve {
                    selection: selection(),
                },
            },
        };
        let canonical = serde_json::to_string(&request).unwrap();
        assert!(!request.is_command());
        assert!(request.uses_short_exchange());
        assert!(serde_json::from_str::<LocalRequest>(&canonical).is_ok());
        let npub = match request {
            LocalRequest::Peer { request } => match request.request {
                Request::Approve { selection } => selection.server.to_bech32().unwrap(),
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };
        for invalid in [
            canonical.replace(&npub, &npub.to_uppercase()),
            canonical.replace(&npub, "alice"),
            canonical.replace(&"ab".repeat(16), &"AB".repeat(16)),
            canonical.replace(&"ad".repeat(32), "ad"),
            canonical.replace("\"generation\":1", "\"generation\":0"),
            canonical.replace("\"generation\":1", "\"generation\":1,\"uid\":0"),
            canonical.replace("\"generation\":1", "\"generation\":1,\"generation\":2"),
            r#"{"operation":"peer","request":{"version":1,"request":{"kind":"list","uid":0}}}"#
                .into(),
            r#"{"operation":"peer","request":{"version":1,"version":2,"request":{"kind":"list"}}}"#
                .into(),
        ] {
            assert!(serde_json::from_str::<LocalRequest>(&invalid).is_err());
        }
        let selected = selection();
        let entry = Entry {
            selection: selected,
            state: State::Pending,
            held_at: 1,
            qualified_at: 1,
            expires_at: 2,
            activating_at: None,
        };
        assert!(
            !Reply::List {
                entries: vec![entry.clone(); LIST_LIMIT + 1],
            }
            .matches(Request::List {})
        );
        assert!(
            !Reply::Approve {
                entry: entry.clone()
            }
            .matches(Request::Approve {
                selection: selected,
            })
        );
        assert!(!Reply::Show { entry }.matches(Request::Show {
            selection: Selection {
                generation: selected.generation + 1,
                ..selected
            }
        }));
    }

    #[tokio::test]
    async fn real_listener_binds_one_use_uid_to_exact_exchange() {
        let base = tempfile::tempdir().unwrap();
        let (lease, _runtime) = profile(base.path(), "paired");
        let (wrong_lease, _wrong_runtime) = profile(base.path(), "wrong");
        let path = lease.socket_path();
        let listener = super::super::bind(&path).unwrap();
        let (mut worker, paired, control) = pair(&lease);
        let (other_worker, _other_listener, _) = pair(&lease);
        assert!(!worker.registered());
        let (requests, mut ordinary) = mpsc::channel(1);
        let (_states, status) = watch::channel(Response::success("ready"));
        let (drain, drain_request) = oneshot::channel();
        let server = tokio::spawn(super::super::serve_with_peer(
            listener,
            requests,
            status,
            None,
            Some(paired),
            Some(drain_request),
        ));
        assert!(worker.ready().await);
        let selection = selection();
        let request = Request::Approve { selection };
        let client = tokio::spawn({
            let path = path.clone();
            async move { client_request(&path, request).await }
        });
        let mut work = next(&mut worker).await;
        assert_eq!(work.request(), &request);
        assert!(matches!(
            work.begin_manual(&other_worker, &lease),
            Err(Failure::Forbidden)
        ));
        assert!(matches!(
            work.begin_manual(&worker, &wrong_lease),
            Err(Failure::Forbidden)
        ));
        let witness = work.begin_manual(&worker, &lease).unwrap();
        assert!(matches!(
            work.begin_manual(&worker, &lease),
            Err(Failure::Forbidden)
        ));
        let second_client = tokio::spawn({
            let path = path.clone();
            async move { client_request(&path, request).await }
        });
        let mut duplicate = next(&mut worker).await;
        assert!(matches!(
            witness.authorize(&duplicate, &lease, selection),
            Err(Failure::Forbidden)
        ));
        let uid = duplicate
            .begin_manual(&worker, &lease)
            .unwrap()
            .authorize(&duplicate, &lease, selection)
            .unwrap();
        assert_eq!(uid, unsafe { libc::geteuid() });
        duplicate.reply(Err(Failure::Stale));
        work.reply(Err(Failure::Unavailable));
        assert_eq!(client.await.unwrap(), Err(Failure::Unavailable));
        assert_eq!(second_client.await.unwrap(), Err(Failure::Stale));
        assert!(ordinary.try_recv().is_err());

        // Retiring the peer worker cannot stop a healthy ordinary listener.
        control.stop();
        worker.stopping().await;
        assert!(worker.stopped());
        assert_eq!(
            client_request(&path, Request::List {}).await,
            Err(Failure::Stopped)
        );
        let response =
            super::super::exchange(&path, &LocalRequest::Status {}, super::super::IO_TIMEOUT)
                .await
                .unwrap();
        assert_eq!(response.result, Some(serde_json::json!("ready")));

        let mut stalled = UnixStream::connect(&path).await.unwrap();
        drain.send(()).unwrap();
        server.await.unwrap().unwrap();
        assert!(
            tokio::io::AsyncReadExt::read_u8(&mut stalled)
                .await
                .is_err()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_fences_started_manual_authority() {
        let base = tempfile::tempdir().unwrap();
        let (lease, _runtime) = profile(base.path(), "paired");
        let listener = super::super::bind(&lease.socket_path()).unwrap();
        let (mut worker, paired, _) = pair(&lease);
        let (_registration, intake) = paired.register(&listener).unwrap();
        let selection = selection();
        let request = Request::Approve { selection };
        let uid = unsafe { libc::geteuid() };
        let deadline = Instant::now() + super::super::IO_TIMEOUT;
        let waiting = tokio::spawn({
            let intake = intake.clone();
            async move { intake.exchange(request, uid, deadline).await }
        });
        let mut active = next(&mut worker).await;
        let witness = active.begin_manual(&worker, &lease).unwrap();
        tokio::time::advance(super::super::IO_TIMEOUT).await;
        assert_eq!(waiting.await.unwrap(), Err(Failure::Uncertain));
        assert!(matches!(
            witness.authorize(&active, &lease, selection),
            Err(Failure::Uncertain)
        ));
        active.reply(Err(Failure::Stale));
    }

    #[tokio::test]
    async fn real_listener_rejects_invalid_frames_and_fences_full_queue_on_abort() {
        let base = tempfile::tempdir().unwrap();
        let (lease, _runtime) = profile(base.path(), "paired");
        let path = lease.socket_path();
        let listener = super::super::bind(&path).unwrap();
        let (mut worker, paired, _) = pair(&lease);
        let (requests, mut ordinary) = mpsc::channel(1);
        let (_states, status) = watch::channel(Response::success("ready"));
        let (_drain, drain_request) = oneshot::channel();
        let server = tokio::spawn(super::super::serve_with_peer(
            listener,
            requests,
            status,
            None,
            Some(paired),
            Some(drain_request),
        ));
        assert!(worker.ready().await);

        let malformed =
            br#"{"operation":"peer","request":{"version":1,"request":{"kind":"list","uid":0}}}"#;
        for (length, body) in [
            (malformed.len() as u32, malformed.as_slice()),
            (super::super::FRAME_LIMIT as u32 + 1, &[]),
        ] {
            let mut stream = UnixStream::connect(&path).await.unwrap();
            stream.write_u32(length).await.unwrap();
            stream.write_all(body).await.unwrap();
            stream.shutdown().await.unwrap();
            let response: Response = super::super::read_frame(&mut stream).await.unwrap();
            assert!(!response.ok);
            assert_eq!(response.error.as_deref(), Some("invalid_request"));
        }
        assert!(worker.try_recv().is_none());
        assert!(ordinary.try_recv().is_err());

        let selection = selection();
        let request = Request::Approve { selection };
        let mut waiting = Vec::new();
        for _ in 0..4 {
            let path = path.clone();
            waiting.push(tokio::spawn(
                async move { client_request(&path, request).await },
            ));
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while worker.queue.len() < 4 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("all four Peer queue slots filled from real sockets");
        assert_eq!(client_request(&path, request).await, Err(Failure::Busy));
        assert!(ordinary.try_recv().is_err());

        let mut active = worker.try_recv().unwrap();
        let witness = active.begin_manual(&worker, &lease).unwrap();
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        worker.stopping().await;
        assert!(worker.try_recv().is_none());
        assert!(matches!(
            witness.authorize(&active, &lease, selection),
            Err(Failure::Stopped | Failure::Uncertain)
        ));
        for client in waiting {
            assert_eq!(client.await.unwrap(), Err(Failure::Uncertain));
        }
    }

    #[tokio::test]
    async fn wrong_or_never_polled_listener_retires_pair() {
        let base = tempfile::tempdir().unwrap();
        let (lease, _runtime) = profile(base.path(), "paired");
        let (wrong, _wrong_runtime) = profile(base.path(), "wrong");
        let listener = super::super::bind(&wrong.socket_path()).unwrap();
        let (worker, paired, _) = pair(&lease);
        assert!(paired.register(&listener).is_err());
        assert!(!worker.ready().await);
        assert!(worker.stopped());

        let (worker, paired, _) = pair(&lease);
        drop(paired);
        assert!(!worker.ready().await);
        assert!(worker.stopped());
    }
}
