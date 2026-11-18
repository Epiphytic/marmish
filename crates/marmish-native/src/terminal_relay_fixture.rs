//! Bounded loopback NIP-42 relay for native transport proofs.

#[path = "peer_private_wss_relay.rs"]
mod peer;
use crate::transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits};
use futures_util::{SinkExt, StreamExt};
use nostr::event::{Event, EventId};
use nostr::key::PublicKey;
use nostr::types::{Timestamp, Url};
pub(crate) use peer::{
    GROUP_HISTORY_LIMIT, GROUP_KIND, GroupFault, GroupFingerprint, GroupHistoryCut, GroupWrite,
};
use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::pki_types::PrivatePkcs8KeyDer;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, timeout};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::tungstenite::{Message, protocol::WebSocketConfig};

pub(super) const STEP: Duration = Duration::from_secs(5);
const FRAME_LIMIT: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 16;
const MAX_SUBSCRIPTIONS: usize = 16;
const MAX_PUBLICATIONS: usize = 128;
const MAX_OBSERVATIONS: usize = 512;

#[derive(Clone, Copy, Eq, PartialEq, Default)]
pub(super) enum Mode {
    #[default]
    Qualifying,
    KnownAccountsOnly,
    AuthorsMustAuthenticate,
    NoChallenge,
    WrongAuthAcknowledgement,
}

#[derive(Clone, Debug)]
pub(super) enum Observation {
    Authentication(PublicKey, bool),
    Read(bool, Option<PublicKey>),
    Write(EventId, u16, bool),
    WriteBytes(EventId, usize),
    Request(PublicKey, Subscription),
    Close(PublicKey, Subscription),
}

#[derive(Clone, Debug)]
pub(super) struct Subscription {
    id: String,
    filters: Vec<Value>,
}

#[derive(Default)]
enum QuietConnection {
    #[default]
    Standard,
    AwaitingRequest,
    Subscribed(String),
}

struct QuietAliases {
    accounts: [PublicKey; 2],
    seen: [usize; 2],
    unexpected: usize,
}

struct Connection {
    quiet: QuietConnection,
    retired: bool,
    account: Option<PublicKey>,
    subscriptions: Vec<Subscription>,
    outgoing: mpsc::Sender<Value>,
}

#[derive(Default)]
struct State {
    mode: Mode,
    quiet: Option<QuietAliases>,
    peer: Option<peer::Scope>,
    acknowledge_duplicates: bool,
    duplicate_acks: usize,
    ordinary: BTreeSet<PublicKey>,
    connections: BTreeMap<usize, Connection>,
    publication_limit: Option<usize>,
    publications: Vec<Event>,
    observations: Vec<Observation>,
    hold_gift_wrap_to: Option<PublicKey>,
    held_gift_wrap: Option<Event>,
    hold_eose_for: Option<PublicKey>,
    held_eose: Option<(usize, String)>,
}

impl State {
    fn observe(&mut self, observation: Observation) {
        let limit = self
            .publication_limit
            .map_or(MAX_OBSERVATIONS, |limit| limit * 4);
        assert!(self.observations.len() < limit, "relay transcript bound");
        self.observations.push(observation);
    }

    fn publish(&mut self, connection: usize, event: Event, raw: &str) -> Option<Value> {
        let account = self.connections[&connection].account;
        let kind = u16::from(event.kind);
        let authenticated = account.is_some();
        let authorized_author = account.is_some_and(|account| {
            self.mode != Mode::AuthorsMustAuthenticate
                || self.ordinary.contains(&account)
                || account == event.pubkey
        });
        if kind == GROUP_KIND && self.peer.as_ref().is_some_and(|peer| peer.group.is_some()) {
            let group = self.peer.as_mut().unwrap().group.as_mut().unwrap();
            let result = account.and_then(|sender| group.accept(sender, &event, raw));
            self.observe(Observation::Write(event.id, kind, result.is_some()));
            let Some((duplicate, acknowledged)) = result else {
                return Some(json!([
                    "OK",
                    event.id,
                    false,
                    "restricted: fixture Group write refused"
                ]));
            };
            let id = event.id;
            if !duplicate {
                self.dispatch_except(event, account);
            }
            return acknowledged.then(|| json!(["OK", id, true, ""]));
        }
        let accepted = authenticated
            && authorized_author
            && event.verify().is_ok()
            && self
                .peer
                .as_mut()
                .is_none_or(|peer| peer.accept(account.unwrap(), &event, raw));
        self.observe(Observation::Write(event.id, kind, accepted));
        if accepted
            && self.acknowledge_duplicates
            && self.publications.iter().any(|stored| stored.id == event.id)
        {
            self.duplicate_acks += 1;
            return Some(json!([
                "OK",
                event.id,
                true,
                "duplicate: fixture stored event"
            ]));
        }
        if accepted {
            if kind == 445 {
                self.observe(Observation::WriteBytes(event.id, raw.len()));
            }
            if kind == 1059
                && self
                    .hold_gift_wrap_to
                    .is_some_and(|recipient| auth_tag(&event, "p", &recipient.to_hex()))
            {
                assert!(self.held_gift_wrap.replace(event.clone()).is_none());
                self.hold_gift_wrap_to = None;
            } else {
                self.dispatch(event.clone());
            }
        }
        let reason = if !authenticated {
            "auth-required: fixture authentication required"
        } else if !accepted {
            "restricted: fixture write refused"
        } else {
            ""
        };
        Some(json!(["OK", event.id, accepted, reason]))
    }

    fn dispatch(&mut self, event: Event) {
        self.dispatch_except(event, None);
    }

    fn dispatch_except(&mut self, event: Event, author: Option<PublicKey>) {
        assert!(
            self.publications.len() < self.publication_limit.unwrap_or(MAX_PUBLICATIONS),
            "relay event bound"
        );
        for peer in self.connections.values() {
            if author.is_some() && peer.account == author {
                continue;
            }
            for subscription in &peer.subscriptions {
                if subscription.matches(&event) {
                    peer.outgoing
                        .try_send(json!(["EVENT", subscription.id, event]))
                        .expect("bounded fixture dispatch");
                }
            }
        }
        self.publications.push(event);
    }
}

impl Subscription {
    fn serves(&self, kind: u16, tag: &str, value: &Value) -> bool {
        self.filters.iter().any(|filter| {
            filter
                .get("kinds")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.contains(&json!(kind)))
                && filter
                    .get(tag)
                    .and_then(Value::as_array)
                    .is_some_and(|values| values.contains(value))
        })
    }

    fn matches(&self, event: &Event) -> bool {
        let tag_matches = |filter: &Value, name: &str| {
            let Some(values) = filter.get(format!("#{name}")).and_then(Value::as_array) else {
                return true;
            };
            event.tags.iter().any(|tag| {
                let tag = tag.as_slice();
                tag.first().is_some_and(|first| first == name)
                    && tag
                        .get(1)
                        .is_some_and(|value| values.iter().any(|expected| expected == value))
            })
        };
        self.filters.iter().any(|filter| {
            let event_kind = u16::from(event.kind);
            includes(filter, "kinds", &json!(event_kind))
                && includes(filter, "authors", &json!(event.pubkey))
                && includes(filter, "ids", &json!(event.id))
                && filter
                    .get("since")
                    .and_then(Value::as_u64)
                    .is_none_or(|since| event.created_at.as_secs() >= since)
                && filter
                    .get("until")
                    .and_then(Value::as_u64)
                    .is_none_or(|until| event.created_at.as_secs() <= until)
                && tag_matches(filter, "p")
                && tag_matches(filter, "h")
                && tag_matches(filter, "d")
        })
    }
}

fn includes(filter: &Value, field: &str, expected: &Value) -> bool {
    filter
        .get(field)
        .and_then(Value::as_array)
        .is_none_or(|values| values.contains(expected))
}

fn read_allowed(account: Option<PublicKey>, filters: &[Value]) -> bool {
    let Some(account) = account else { return false };
    !filters.is_empty()
        && filters.iter().all(|filter| {
            // This fixture accepts only explicit kinds, keeping wildcard reads
            // outside the experimental inbox and group authorization contract.
            let Some(kinds) = filter.get("kinds").and_then(Value::as_array) else {
                return false;
            };
            if kinds.is_empty()
                || kinds
                    .iter()
                    .any(|kind| kind.as_u64().is_none_or(|n| n > u64::from(u16::MAX)))
            {
                return false;
            }
            let required = json!([account]);
            let addressed = filter.get("#p") == Some(&required);
            let scoped_group = filter
                .get("#h")
                .and_then(Value::as_array)
                .is_some_and(|routes| {
                    !routes.is_empty()
                        && routes.iter().all(|route| {
                            route.as_str().is_some_and(|hex| {
                                hex::decode(hex).is_ok_and(|bytes| bytes.len() == 32)
                            })
                        })
                });
            (!kinds.contains(&json!(1059)) || addressed)
                && (!kinds.contains(&json!(445)) || scoped_group)
        })
}

fn auth_tag(event: &Event, name: &str, value: &str) -> bool {
    event
        .tags
        .iter()
        .filter(|tag| tag.as_slice().first().is_some_and(|s| s == name))
        .map(|tag| tag.as_slice())
        .eq([&[name, value][..]])
}

fn quiet_message(state: &mut State, id: usize, fields: &[Value]) -> Vec<Value> {
    let quiet = state.quiet.as_mut().unwrap();
    let connection = state.connections.get_mut(&id).unwrap();
    match (&connection.quiet, fields) {
        (QuietConnection::AwaitingRequest, [verb, subscription, filter]) if verb == "REQ" => {
            if let Some((id, alias)) = subscription.as_str().zip(
                quiet
                    .accounts
                    .iter()
                    .position(|account| *filter == json!({"kinds": [1059], "#p": [account]})),
            ) && !id.is_empty()
                && id.len() <= 64
            {
                quiet.seen[alias] += 1;
                connection.quiet = QuietConnection::Subscribed(id.to_owned());
                return vec![json!(["EOSE", id])];
            }
        }
        (QuietConnection::Subscribed(id), [verb, subscription])
            if verb == "CLOSE" && subscription.as_str() == Some(id.as_str()) =>
        {
            return vec![];
        }
        _ => (),
    }
    quiet.unexpected += 1;
    retire_connection(state, id);
    vec![]
}

fn handle_message(
    state: &Mutex<State>,
    id: usize,
    relay: &Url,
    challenge: &str,
    challenge_started: Instant,
    fields: &[Value],
    raw: &str,
) -> Vec<Value> {
    let mut state = state.lock().unwrap();
    if state.connections[&id].retired {
        return vec![];
    }
    if !matches!(state.connections[&id].quiet, QuietConnection::Standard) {
        return quiet_message(&mut state, id, fields);
    }
    match fields.first().and_then(Value::as_str) {
        Some("AUTH") => {
            if state.peer.as_ref().is_some_and(|peer| peer.group.is_some())
                && state.connections[&id].account.is_some()
            {
                retire_connection(&mut state, id);
                return vec![];
            }
            let Ok(event) = serde_json::from_value::<Event>(fields[1].clone()) else {
                return vec![];
            };
            let eligible = (state.mode != Mode::KnownAccountsOnly
                || state.ordinary.contains(&event.pubkey))
                && state
                    .peer
                    .as_ref()
                    .is_none_or(|peer| peer.includes(event.pubkey));
            let fresh = event
                .created_at
                .as_secs()
                .abs_diff(Timestamp::now().as_secs())
                <= 30
                && challenge_started.elapsed() <= Duration::from_secs(30);
            let accepted = eligible
                && fresh
                && u16::from(event.kind) == 22242
                && event.verify().is_ok()
                && auth_tag(&event, "relay", relay.as_str())
                && auth_tag(&event, "challenge", challenge)
                && state.connections[&id]
                    .account
                    .is_none_or(|account| account == event.pubkey);
            if accepted {
                state.connections.get_mut(&id).unwrap().account = Some(event.pubkey);
            }
            state.observe(Observation::Authentication(event.pubkey, accepted));
            let reason = if accepted {
                ""
            } else {
                "restricted: fixture AUTH refused"
            };
            let acknowledged = if state.mode == Mode::WrongAuthAcknowledgement {
                EventId::from_byte_array([1; 32])
            } else {
                event.id
            };
            vec![json!(["OK", acknowledged, accepted, reason])]
        }
        Some("REQ") => {
            let Some(subscription_id) = fields[1].as_str() else {
                return vec![];
            };
            let filters = &fields[2..];
            let account = state.connections[&id].account;
            let permitted = state.peer.as_ref().map_or_else(
                || read_allowed(account, filters),
                |peer| peer.read_allowed(account, filters),
            );
            state.observe(Observation::Read(permitted, account));
            if !permitted {
                let reason = if account.is_none() {
                    "auth-required: fixture authentication required"
                } else {
                    "restricted: fixture read refused"
                };
                return vec![json!(["CLOSED", subscription_id, reason])];
            }
            let subscription = Subscription {
                id: subscription_id.into(),
                filters: filters.to_vec(),
            };
            let subscriptions = &mut state.connections.get_mut(&id).unwrap().subscriptions;
            subscriptions.retain(|entry| entry.id != subscription_id);
            assert!(
                subscriptions.len() < MAX_SUBSCRIPTIONS,
                "fixture subscription bound"
            );
            subscriptions.push(subscription.clone());
            let account = account.unwrap();
            state.observe(Observation::Request(account, subscription.clone()));
            if let Some(group) = state.peer.as_mut().and_then(|peer| peer.group.as_mut())
                && subscription.serves(GROUP_KIND, "#h", &json!(hex::encode(group.route)))
            {
                return match group.history(account, subscription_id) {
                    Some((frames, false)) => frames,
                    Some((_, true)) => {
                        retire_connection(&mut state, id);
                        vec![]
                    }
                    None => {
                        state
                            .connections
                            .get_mut(&id)
                            .unwrap()
                            .subscriptions
                            .retain(|entry| entry.id != subscription_id);
                        vec![json!([
                            "CLOSED",
                            subscription_id,
                            "restricted: fixture Group history bound"
                        ])]
                    }
                };
            }
            let account = Some(account);
            let mut matches: Vec<_> = state
                .publications
                .iter()
                .filter(|event| subscription.matches(event))
                .collect();
            if state.peer.is_some() {
                matches.sort_by_key(|event| (std::cmp::Reverse(event.created_at), event.id));
                matches.truncate(peer::history_limit(filters));
            }
            let mut responses: Vec<_> = matches
                .into_iter()
                .map(|event| json!(["EVENT", subscription_id, event]))
                .collect();
            if state.hold_eose_for.is_some_and(|alias| {
                account == Some(alias) && subscription.serves(1059, "#p", &json!(alias))
            }) {
                assert!(state.held_eose.replace((id, subscription.id)).is_none());
                state.hold_eose_for = None;
            } else {
                responses.push(json!(["EOSE", subscription_id]));
            }
            responses
        }
        Some("CLOSE") => {
            let connection = state.connections.get_mut(&id).unwrap();
            if let Some(index) = connection
                .subscriptions
                .iter()
                .position(|entry| Some(entry.id.as_str()) == fields[1].as_str())
            {
                let removed = connection.subscriptions.remove(index);
                let account = connection.account.unwrap();
                state.observe(Observation::Close(account, removed));
            }
            vec![]
        }
        Some("EVENT") => serde_json::from_value(fields[1].clone())
            .ok()
            .into_iter()
            .filter_map(|event| state.publish(id, event, raw))
            .collect(),
        Some("NEG-OPEN") => vec![json!([
            "NEG-ERR",
            fields[1],
            "unsupported: fixture reconciliation"
        ])],
        _ => vec![],
    }
}

fn retire_connection(state: &mut State, id: usize) {
    let connection = state.connections.get_mut(&id).unwrap();
    connection.retired = true;
    connection.account = None;
    connection.subscriptions.clear();
    let _ = connection.outgoing.try_send(Value::Null);
}

async fn serve_connection(
    raw: TcpStream,
    acceptor: TlsAcceptor,
    state: Arc<Mutex<State>>,
    relay: Url,
    id: usize,
    probe_limit: Option<usize>,
) {
    let Ok(Ok(tls)) = timeout(STEP, acceptor.accept(raw)).await else {
        return;
    };
    let frame_limit = probe_limit.unwrap_or_else(|| {
        if state.lock().unwrap().peer.is_some() {
            peer::MESSAGE_LIMIT
        } else {
            FRAME_LIMIT
        }
    });
    let options = WebSocketConfig::default()
        .max_message_size(Some(frame_limit))
        .max_frame_size(Some(frame_limit));
    let Ok(Ok(mut socket)) = timeout(
        STEP,
        tokio_tungstenite::accept_async_with_config(tls, Some(options)),
    )
    .await
    else {
        return;
    };
    let (outgoing, mut queued) = mpsc::channel(32);
    let omit_challenge = {
        let mut state = state.lock().unwrap();
        let quiet = state.quiet.is_some();
        state.connections.insert(
            id,
            Connection {
                quiet: if quiet {
                    QuietConnection::AwaitingRequest
                } else {
                    QuietConnection::Standard
                },
                retired: false,
                account: None,
                subscriptions: Vec::new(),
                outgoing,
            },
        );
        quiet || state.mode == Mode::NoChallenge
    };
    let challenge = hex::encode(super::random::<32>().unwrap());
    let started = Instant::now();
    let challenge_message = Message::Text(json!(["AUTH", challenge]).to_string().into());
    if omit_challenge
        || matches!(
            timeout(STEP, socket.send(challenge_message)).await,
            Ok(Ok(()))
        )
    {
        loop {
            let replies = tokio::select! {
                incoming = socket.next() => match incoming {
                    Some(Ok(Message::Text(raw))) => {
                        let Ok(fields) = serde_json::from_str::<Vec<Value>>(&raw) else { break };
                        if fields.len() < 2 || fields.len() > 18 { break; }
                        handle_message(&state, id, &relay, &challenge, started, &fields, &raw)
                    }
                    Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                        if socket.flush().await.is_err() { break; }
                        vec![]
                    }
                    _ => break,
                },
                outgoing = queued.recv() => match outgoing {
                    Some(event) if !event.is_null() => vec![event],
                    Some(_) => break,
                    None => break,
                },
            };
            if state.lock().unwrap().connections[&id].retired {
                break;
            }
            let mut connected = true;
            for reply in replies {
                if !matches!(
                    timeout(STEP, socket.send(Message::Text(reply.to_string().into()))).await,
                    Ok(Ok(()))
                ) {
                    connected = false;
                    break;
                }
            }
            if !connected {
                break;
            }
        }
    }
    state.lock().unwrap().connections.remove(&id);
}

fn tls() -> (TlsAcceptor, rustls::RootCertStore, Vec<u8>) {
    let mut ca = CertificateParams::default();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages.push(KeyUsagePurpose::KeyCertSign);
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca, ca_key);
    let leaf_key = KeyPair::generate().unwrap();
    let leaf = CertificateParams::new(vec!["127.0.0.1".into()])
        .unwrap()
        .signed_by(&leaf_key, &issuer)
        .unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf.der().clone()],
        PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
    )
    .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(ca_cert.der().clone()).unwrap();
    (
        TlsAcceptor::from(Arc::new(config)),
        roots,
        ca_cert.der().to_vec(),
    )
}

pub(crate) struct Relay {
    pub(crate) url: Url,
    roots: rustls::RootCertStore,
    qualification_ca: Vec<u8>,
    peer: Option<PeerIdentity>,
    state: Arc<Mutex<State>>,
    task: JoinHandle<()>,
}

struct PeerIdentity {
    origin: Url,
    accounts: [PublicKey; 2],
}

/// Evidence for the exact two-account Peer relay instance and origin.
pub(crate) struct PeerMode<'a> {
    relay: &'a Relay,
    identity: &'a PeerIdentity,
}

impl<'a> PeerMode<'a> {
    pub(crate) fn qualify(
        self,
        path: &Path,
        recipient: PublicKey,
        founder: PublicKey,
    ) -> Option<(&'a str, RelayConnector)> {
        let identity = self.identity;
        (recipient != founder
            && identity.accounts.contains(&recipient)
            && identity.accounts.contains(&founder)
            && self.relay.url == identity.origin)
            .then(|| {
                (
                    identity.origin.as_str(),
                    self.relay.connector(path, true, None),
                )
            })
    }
}

impl Relay {
    pub(crate) async fn start() -> Self {
        Self::start_with(None, None, None).await
    }

    pub(crate) async fn start_peer(accounts: [PublicKey; 2]) -> Self {
        Self::start_with(Some(accounts), None, None).await
    }

    pub(crate) async fn start_peer_group(accounts: [PublicKey; 2], route: [u8; 32]) -> Self {
        Self::start_with(Some(accounts), Some(route), None).await
    }

    pub(crate) async fn start_peer_qualification(
        accounts: [PublicKey; 2],
        route: [u8; 32],
    ) -> Self {
        // Receive an over-cap event so the scoped policy can explicitly refuse
        // it. Existing fixture modes retain their original websocket bound.
        Self::start_with(Some(accounts), Some(route), Some(128 * 1024)).await
    }

    pub(crate) fn qualification_ca(&self) -> &[u8] {
        &self.qualification_ca
    }

    pub(crate) fn group_history_cut(
        &self,
        who: PublicKey,
        request: &str,
    ) -> Option<GroupHistoryCut> {
        self.state
            .lock()
            .unwrap()
            .peer
            .as_ref()?
            .group
            .as_ref()?
            .cuts
            .get(&(who, request.into()))
            .cloned()
    }

    pub(crate) fn group_writes(&self) -> Vec<GroupWrite> {
        self.state
            .lock()
            .unwrap()
            .peer
            .as_ref()
            .unwrap()
            .group
            .as_ref()
            .unwrap()
            .writes
            .clone()
    }

    pub(crate) fn group_fault(&self, fault: GroupFault) {
        let mut state = self.state.lock().unwrap();
        let group = state.peer.as_mut().unwrap().group.as_mut().unwrap();
        assert!(
            group.fault.replace(fault).is_none(),
            "one isolated Group fault"
        );
    }

    pub(crate) fn peer_mode(&self) -> Option<PeerMode<'_>> {
        let identity = self.peer.as_ref()?;
        Some(PeerMode {
            relay: self,
            identity,
        })
    }

    pub(crate) fn peer_transcript(&self) -> Vec<peer::Write> {
        self.state
            .lock()
            .unwrap()
            .peer
            .as_ref()
            .unwrap()
            .writes
            .clone()
    }

    pub(super) fn arm_quiet_aliases(&self, accounts: [PublicKey; 2]) {
        let mut state = self.state.lock().unwrap();
        assert!(state.peer.is_none() && state.mode == Mode::Qualifying);
        assert!(
            accounts[0] != accounts[1] && accounts.iter().all(|key| !state.ordinary.contains(key))
        );
        assert!(
            state
                .quiet
                .replace(QuietAliases {
                    accounts,
                    seen: [0; 2],
                    unexpected: 0,
                })
                .is_none()
        );
    }

    pub(super) fn quiet_alias_observations(&self) -> ([usize; 2], usize) {
        let state = self.state.lock().unwrap();
        let quiet = state.quiet.as_ref().unwrap();
        (quiet.seen, quiet.unexpected)
    }

    pub(crate) fn publication_attempts(&self, kind: u16) -> usize {
        self.state
            .lock()
            .unwrap()
            .observations
            .iter()
            .filter(|observation| matches!(observation, Observation::Write(_, found, _) if *found == kind))
            .count()
    }

    pub(crate) fn peer_only_publications(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.peer.is_some()
            && state.observations.iter().all(
                |observation| !matches!(observation, Observation::Write(_, kind, _) if *kind != 7435),
            )
    }

    pub(crate) fn authenticated(&self, account: PublicKey) -> bool {
        self.state
            .lock()
            .unwrap()
            .observations
            .iter()
            .any(|entry| matches!(entry, Observation::Authentication(who, true) if *who == account))
    }

    pub(crate) async fn stop(&mut self) {
        self.task.abort();
        let error = (&mut self.task).await.expect_err("fixture cancelled");
        assert!(error.is_cancelled(), "fixture stopped without panic");
    }

    async fn start_with(
        accounts: Option<[PublicKey; 2]>,
        group: Option<[u8; 32]>,
        probe_limit: Option<usize>,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("wss://{}/", listener.local_addr().unwrap())).unwrap();
        let (acceptor, roots, qualification_ca) = tls();
        let peer = accounts.map(|accounts| PeerIdentity {
            origin: url.clone(),
            accounts,
        });
        let state = Arc::new(Mutex::new(State {
            peer: accounts.map(|accounts| peer::Scope::new(accounts).with_group(group)),
            ..State::default()
        }));
        let worker_state = state.clone();
        let worker_url = url.clone();
        let task = tokio::spawn(async move {
            let mut peers = JoinSet::new();
            let mut next = 0usize;
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (raw, _) = accepted.unwrap();
                        assert!(peers.len() < MAX_CONNECTIONS, "fixture socket bound");
                        next += 1;
                        peers.spawn(serve_connection(raw, acceptor.clone(), worker_state.clone(), worker_url.clone(), next, probe_limit));
                    }
                    result = peers.join_next(), if !peers.is_empty() => {
                        result.unwrap().unwrap();
                    }
                }
            }
        });
        Self {
            url,
            roots,
            qualification_ca,
            peer,
            state,
            task,
        }
    }

    pub(super) fn roots(&self) -> rustls::RootCertStore {
        self.roots.clone()
    }

    pub(super) fn accepted_publications(&self) -> usize {
        self.state.lock().unwrap().publications.len()
    }

    pub(super) fn allow_installed_benchmark(&self) {
        let mut state = self.state.lock().unwrap();
        assert!(state.publications.is_empty());
        state.publication_limit = Some(8_192);
    }

    pub(super) fn hold_next_gift_wrap(&self, recipient: PublicKey) {
        let mut state = self.state.lock().unwrap();
        assert!(state.hold_gift_wrap_to.is_none() && state.held_gift_wrap.is_none());
        state.hold_gift_wrap_to = Some(recipient);
    }

    pub(super) fn gift_wrap_is_held(&self) -> bool {
        self.state.lock().unwrap().held_gift_wrap.is_some()
    }

    pub(super) fn release_gift_wrap(&self) {
        let mut state = self.state.lock().unwrap();
        let event = state.held_gift_wrap.take().expect("one held gift wrap");
        state.dispatch(event);
    }

    pub(super) fn hold_next_alias_eose(&self, alias: PublicKey) {
        let mut state = self.state.lock().unwrap();
        assert!(state.hold_eose_for.is_none() && state.held_eose.is_none());
        state.hold_eose_for = Some(alias);
    }

    pub(super) fn release_alias_eose(&self) {
        let mut state = self.state.lock().unwrap();
        let (connection, subscription) = state.held_eose.take().expect("authenticated alias EOSE");
        // Use the same ordered queue as live EVENT dispatch: the first
        // client Probe already queued for this socket precedes this EOSE.
        state.connections[&connection]
            .outgoing
            .try_send(json!(["EOSE", subscription]))
            .expect("bounded fixture EOSE dispatch");
    }

    pub(super) fn allow_multi_room_workload(&self) {
        let mut state = self.state.lock().unwrap();
        assert!(state.publications.is_empty());
        state.publication_limit = Some(2 * MAX_PUBLICATIONS);
    }

    pub(super) fn acknowledge_duplicates(&self) {
        self.state.lock().unwrap().acknowledge_duplicates = true;
    }

    pub(super) fn duplicate_acks(&self) -> usize {
        self.state.lock().unwrap().duplicate_acks
    }

    pub(crate) fn connector(
        &self,
        path: &Path,
        trusted: bool,
        extra: Option<&Url>,
    ) -> RelayConnector {
        let mut urls = vec![self.url.to_string()];
        urls.extend(extra.map(Url::to_string));
        let limits = TransportLimits {
            handshake_timeout: STEP,
            ..TransportLimits::default()
        };
        let policy = RelayPolicy::parse(&urls, RouteMode::Off, limits).unwrap();
        let dirs = ArtiDirs {
            state: path.join("arti_state"),
            cache: path.join("arti_cache"),
        };
        if trusted {
            RelayConnector::for_loopback_fixture(policy, dirs, self.url.clone(), self.roots.clone())
                .unwrap()
        } else {
            RelayConnector::new(policy, dirs).unwrap()
        }
    }

    pub(super) fn paired_connector(&self, other: &Self, path: &Path) -> RelayConnector {
        let limits = TransportLimits {
            handshake_timeout: STEP,
            ..TransportLimits::default()
        };
        let policy = RelayPolicy::parse(
            &[self.url.to_string(), other.url.to_string()],
            RouteMode::Off,
            limits,
        )
        .unwrap();
        let dirs = ArtiDirs {
            state: path.join("arti_state"),
            cache: path.join("arti_cache"),
        };
        RelayConnector::for_loopback_fixtures(
            policy,
            dirs,
            vec![
                (self.url.clone(), self.roots.clone()),
                (other.url.clone(), other.roots.clone()),
            ],
        )
        .unwrap()
    }

    pub(super) fn subscription_counts(
        &self,
        account: PublicKey,
        kind: u16,
        tag: &str,
        value: &Value,
    ) -> (usize, usize) {
        self.state
            .lock()
            .unwrap()
            .observations
            .iter()
            .fold((0, 0), |(req, close), entry| match entry {
                Observation::Request(who, sub)
                    if *who == account && sub.serves(kind, tag, value) =>
                {
                    (req + 1, close)
                }
                Observation::Close(who, sub) if *who == account && sub.serves(kind, tag, value) => {
                    (req, close + 1)
                }
                _ => (req, close),
            })
    }

    pub(super) fn has_live_subscription(&self, account: PublicKey, id: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .connections
            .values()
            .filter(|connection| connection.account == Some(account))
            .any(|connection| connection.subscriptions.iter().any(|sub| sub.id == id))
    }

    pub(super) fn stored_gift_wraps_to(&self, recipient: PublicKey) -> usize {
        let recipient = recipient.to_hex();
        self.state
            .lock()
            .unwrap()
            .publications
            .iter()
            .filter(|event| u16::from(event.kind) == 1059 && auth_tag(event, "p", &recipient))
            .count()
    }

    pub(super) fn distinct_gift_wraps_before_reply(
        &self,
        receiver: PublicKey,
        reply_receiver: PublicKey,
    ) -> usize {
        let receiver = receiver.to_hex();
        let reply_receiver = reply_receiver.to_hex();
        let state = self.state.lock().unwrap();
        let wraps = state
            .publications
            .iter()
            .filter(|event| u16::from(event.kind) == 1059);
        assert!(
            wraps
                .clone()
                .any(|event| auth_tag(event, "p", &reply_receiver))
        );
        wraps
            .take_while(|event| !auth_tag(event, "p", &reply_receiver))
            .filter(|event| auth_tag(event, "p", &receiver))
            .map(|event| event.id)
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub(super) fn assert_healthy(&self) {
        assert!(
            !self.task.is_finished(),
            "loopback relay stopped unexpectedly"
        );
    }

    pub(super) fn assert_no_plaintext(&self, marker: &[u8]) {
        let state = self.state.lock().unwrap();
        assert!(!state.publications.is_empty());
        for event in &state.publications {
            let json = serde_json::to_vec(event).unwrap();
            assert!(!json.windows(marker.len()).any(|window| window == marker));
        }
    }

    pub(crate) fn ordinary(&self, account: PublicKey) {
        self.state.lock().unwrap().ordinary.insert(account);
    }

    pub(super) fn mode(&self, mode: Mode) {
        self.state.lock().unwrap().mode = mode;
    }

    pub(super) fn seed_stored_events(&self, events: impl IntoIterator<Item = Event>) {
        let mut state = self.state.lock().unwrap();
        assert!(state.connections.is_empty());
        state.publications.extend(events);
        assert!(state.publications.len() <= MAX_PUBLICATIONS);
    }

    pub(super) fn replay_alias_inbox(&self, account: PublicKey) -> usize {
        let state = self.state.lock().unwrap();
        let mut replayed = 0;
        for connection in state
            .connections
            .values()
            .filter(|source| source.account == Some(account))
        {
            for subscription in &connection.subscriptions {
                for event in state
                    .publications
                    .iter()
                    .filter(|event| subscription.matches(event))
                {
                    connection
                        .outgoing
                        .try_send(json!(["EVENT", subscription.id, event]))
                        .unwrap();
                    replayed += 1;
                }
            }
        }
        replayed
    }

    pub(super) fn refuse_exact_subscription(&self, account: PublicKey, id: &str) {
        let state = self.state.lock().unwrap();
        let source = state.connections.values().find(|source| {
            source.account == Some(account) && source.subscriptions.iter().any(|sub| sub.id == id)
        });
        source
            .expect("verified fixture subscription")
            .outgoing
            .try_send(json!(["CLOSED", id, "restricted: fixture read revoked"]))
            .unwrap();
    }

    pub(super) fn disconnect(&self, account: PublicKey) {
        let state = self.state.lock().unwrap();
        state
            .connections
            .values()
            .find(|source| source.account == Some(account))
            .expect("verified fixture source")
            .outgoing
            .try_send(Value::Null)
            .unwrap();
    }

    pub(super) async fn observed(&self, predicate: impl Fn(&Observation) -> bool) {
        timeout(STEP, async {
            loop {
                if self
                    .state
                    .lock()
                    .unwrap()
                    .observations
                    .iter()
                    .any(&predicate)
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("relay observation");
    }

    pub(super) async fn accepted_event_bytes(&self, id: EventId) -> usize {
        self.observed(|entry| matches!(entry, Observation::WriteBytes(event, _) if *event == id))
            .await;
        self.state
            .lock()
            .unwrap()
            .observations
            .iter()
            .find_map(|entry| match entry {
                Observation::WriteBytes(event, bytes) if *event == id => Some(*bytes),
                _ => None,
            })
            .expect("observed accepted WSS event length")
    }

    pub(super) fn assert_zero_package_writes(&self) {
        assert_eq!(self.package_writes(), 0);
    }

    pub(super) fn package_writes(&self) -> usize {
        self.state
            .lock()
            .unwrap()
            .observations
            .iter()
            .filter(|observation| matches!(observation, Observation::Write(_, 30443, _)))
            .count()
    }

    pub(super) async fn subscription(
        &self,
        account: PublicKey,
        kind: u16,
        tag: &str,
        value: Value,
    ) -> String {
        timeout(STEP, async {
            loop {
                {
                    let state = self.state.lock().unwrap();
                    for connection in state.connections.values() {
                        if connection.account != Some(account) {
                            continue;
                        }
                        for subscription in &connection.subscriptions {
                            if subscription.serves(kind, tag, &value) {
                                return subscription.id.clone();
                            }
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("active relay subscription")
    }

    pub(super) fn retain_for_replay(&self, event: Event) {
        event.verify().unwrap();
        let mut state = self.state.lock().unwrap();
        assert!(
            state.publications.len() < MAX_PUBLICATIONS,
            "relay event bound"
        );
        assert!(state.connections.values().all(|connection| {
            connection
                .subscriptions
                .iter()
                .all(|subscription| !subscription.matches(&event))
        }));
        state.publications.push(event);
    }

    pub(super) fn inject(&self, account: PublicKey, id: &str, event: &Event) {
        let state = self.state.lock().unwrap();
        let peer = state
            .connections
            .values()
            .find(|peer| {
                peer.account == Some(account)
                    && peer
                        .subscriptions
                        .iter()
                        .any(|subscription| subscription.id == id)
            })
            .expect("wire misdelivery requires an active subscription");
        peer.outgoing
            .try_send(json!(["EVENT", id, event]))
            .expect("bounded relay replay");
        peer.outgoing
            .try_send(json!(["EOSE", id]))
            .expect("bounded completion");
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Relay {
    pub(super) async fn assert_inbox_read_policy(&self) {
        use nostr::event::{EventBuilder, FinalizeEvent, Kind, Tag};
        use nostr::key::Keys;
        let socket_addr = self.url.host_str().zip(self.url.port()).unwrap();
        let raw = timeout(STEP, TcpStream::connect(socket_addr))
            .await
            .unwrap()
            .unwrap();
        let tls = timeout(
            STEP,
            crate::transport::tls_stream_with_roots(Box::new(raw), "127.0.0.1", self.roots.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        let (mut websocket, _) = timeout(
            STEP,
            tokio_tungstenite::client_async(self.url.as_str(), tls),
        )
        .await
        .unwrap()
        .unwrap();
        let chat = timeout(STEP, websocket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let challenge: Value = serde_json::from_str(chat.to_text().unwrap()).unwrap();
        assert_eq!(challenge[0], "AUTH");
        let own = Keys::generate();
        let foreign = Keys::generate().public_key();
        let params = |kind: &str, value: &str| Tag::parse([kind, value]).unwrap();
        let auth = |challenge: &str| {
            EventBuilder::new(Kind::Custom(22242), "")
                .tags([
                    params("relay", self.url.as_str()),
                    params("challenge", challenge),
                ])
                .finalize(&own)
                .unwrap()
        };
        for (request, response, permitted) in [
            (
                json!(["REQ", "preauth", {"kinds":[1059], "#p":[own.public_key()]}]),
                "CLOSED",
                None,
            ),
            (
                json!(["AUTH", auth("invalid-challenge")]),
                "OK",
                Some(false),
            ),
            (
                json!(["AUTH", auth(challenge[1].as_str().unwrap())]),
                "OK",
                Some(true),
            ),
            (
                json!(["REQ", "foreign", {"kinds":[1059], "#p":[foreign]}]),
                "CLOSED",
                None,
            ),
            (json!(["REQ", "broad", {"kinds":[1059]}]), "CLOSED", None),
            (json!(["REQ", "all-events", {}]), "CLOSED", None),
            (
                json!(["REQ", "all-author-events", {"authors":[foreign]}]),
                "CLOSED",
                None,
            ),
            (
                json!(["REQ", "mixed-kinds", {"kinds":[1059, 1]}]),
                "CLOSED",
                None,
            ),
            (json!(["REQ", "no-kinds", {"kinds":[]}]), "CLOSED", None),
            (
                json!(["REQ", "invalid-kinds", {"kinds":"1059"}]),
                "CLOSED",
                None,
            ),
            (
                json!(["REQ", "mixed", {"kinds":[1059], "#p":[own.public_key(), foreign]}]),
                "CLOSED",
                None,
            ),
            (
                json!(["REQ", "exact", {"kinds":[1059], "#p":[own.public_key()]}]),
                "EOSE",
                None,
            ),
        ] {
            timeout(
                STEP,
                websocket.send(Message::Text(request.to_string().into())),
            )
            .await
            .unwrap()
            .unwrap();
            let reply = timeout(STEP, websocket.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let reply: Value = serde_json::from_str(reply.to_text().unwrap()).unwrap();
            assert_eq!(reply[0], response);
            if let Some(permitted) = permitted {
                assert_eq!(reply[2], permitted);
            }
        }
    }
}
