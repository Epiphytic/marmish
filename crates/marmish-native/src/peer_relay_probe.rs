//! Disposable relay qualification. There is no profile signer or native group
//! authority in this module.
use super::{ProbeAuthority, qualification};
use crate::{Error, Result, command_config::peer_intent::Relay, transport::PeerRelayConnector};
use async_wsocket::Message;
use futures_util::{SinkExt, StreamExt};
use nostr::{
    event::{EventBuilder, FinalizeEvent, Kind, Tag},
    key::{Keys, PublicKey},
    nips::nip44::{self, Version},
    types::Timestamp,
};
use nostr_sdk::transport::websocket::{WebSocketSink, WebSocketStream};
use serde::Serialize;
use serde_json::{Value, json, value::RawValue};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, Weak},
    time::Duration,
};
use tokio::time::{Instant, timeout_at};

const CONTROL: u16 = 7435;
const GROUP: u16 = 445;
const FRAME_BUDGET: usize = 64;
const BYTE_BUDGET: usize = 2 * 1024 * 1024;
const CONTROL_CAP: usize = 65_536;
const REFUSED: Error = Error::Transport("peer_qualification_probe");
const TRAFFIC_BUDGET: Error = Error::Transport("peer_qualification_traffic_budget");
const SOCKET_BUDGET: Error = Error::Transport("peer_qualification_socket_budget");
const TIMEOUT: Error = Error::Transport("peer_qualification_deadline");

pub(super) struct Plan {
    writer: Keys,
    recipient: Keys,
    outsider: Keys,
    route: [u8; 32],
    nonce: [u8; 32],
}

impl Plan {
    pub(super) fn new() -> Result<Self> {
        let mut bytes = [0; 64];
        getrandom::fill(&mut bytes).map_err(|_| REFUSED)?;
        let (route, nonce) = bytes.split_at(32);
        Ok(Self {
            writer: Keys::generate(),
            recipient: Keys::generate(),
            outsider: Keys::generate(),
            route: route.try_into().expect("route length"),
            nonce: nonce.try_into().expect("nonce length"),
        })
    }

    #[cfg(test)]
    pub(super) fn relay_registration(&self) -> ([PublicKey; 2], [u8; 32]) {
        (
            [self.writer.public_key(), self.recipient.public_key()],
            self.route,
        )
    }

    pub(super) fn operator_view(&self) -> String {
        format!(
            "Register disposable writer {} and recipient {} for this test.\nLeave outsider {} unregistered.\nAuthorize the two registered principals on Group route {}.\n",
            self.writer.public_key(),
            self.recipient.public_key(),
            self.outsider.public_key(),
            hex::encode(self.route),
        )
    }

    pub(super) async fn execute(
        self,
        connector: &PeerRelayConnector,
        authority: &ProbeAuthority<'_>,
        started: u64,
    ) -> Result<Completed> {
        let relay = authority.relay;
        let mut io = Budget::new(authority, started);
        let route = hex::encode(self.route);
        let mut wrong = self.route;
        wrong[0] ^= 1;
        let wrong = hex::encode(wrong);
        let recipient = self.recipient.public_key();
        let control = self.control(recipient, started, &"x".repeat(40_800))?;
        let oversize = self.control(recipient, started, &"x".repeat(49_152))?;
        if control.bytes.len() > CONTROL_CAP || oversize.bytes.len() <= CONTROL_CAP {
            return Err(REFUSED);
        }
        let group = self.group(&route, started)?;
        let wrong_group = self.group(&wrong, started)?;
        let mut writer = io
            .connect(connector, &self, Turn::Writer)
            .await
            .map_err(at("peer_qualification_writer_auth"))?;
        writer
            .publish(&mut io, &control, None)
            .await
            .map_err(at("peer_qualification_control_write"))?;
        writer
            .publish(&mut io, &oversize, Some(Refusal::Size))
            .await
            .map_err(at("peer_qualification_control_size"))?;
        writer
            .publish(&mut io, &group, None)
            .await
            .map_err(at("peer_qualification_group_write"))?;
        writer
            .publish(&mut io, &wrong_group, Some(Refusal::Scope))
            .await
            .map_err(at("peer_qualification_group_foreign_write"))?;
        // The writer cannot read the recipient's exact protected scope.
        let control_filter = json!({
            "kinds":[CONTROL], "#p":[recipient], "since":started, "until":started, "limit":4,
        });
        writer
            .refuse_history(
                &mut io,
                "foreign-recipient",
                control_filter.clone(),
                Refusal::Scope,
                false,
            )
            .await
            .map_err(at("peer_qualification_control_foreign_read"))?;
        drop(writer);

        let group_filter = json!({"kinds":[GROUP], "#h":[route]});
        let mut outsider = io
            .connect(connector, &self, Turn::Outsider)
            .await
            .map_err(at("peer_qualification_outsider_auth"))?;
        let outsider_control = self.control(recipient, started, " outsider")?;
        outsider
            .publish(&mut io, &outsider_control, Some(Refusal::Unauthenticated))
            .await
            .map_err(at("peer_qualification_control_unauthenticated_write"))?;
        outsider
            .refuse_history(
                &mut io,
                "unauth-control",
                control_filter.clone(),
                Refusal::Unauthenticated,
                false,
            )
            .await
            .map_err(at("peer_qualification_control_unauthenticated_read"))?;
        outsider
            .refuse_history(
                &mut io,
                "unauth-group",
                group_filter.clone(),
                Refusal::Unauthenticated,
                false,
            )
            .await
            .map_err(at("peer_qualification_group_unauthenticated_read"))?;
        drop(outsider);
        for (turn, auth, control_req, control_check, group_req, group_check) in [
            (
                Turn::Recipient,
                "peer_qualification_recipient_auth",
                "control",
                "peer_qualification_control_history",
                "group",
                "peer_qualification_group_history",
            ),
            (
                Turn::Reconnect,
                "peer_qualification_reconnect_auth",
                "control-reopen",
                "peer_qualification_control_reconnect",
                "group-reopen",
                "peer_qualification_group_reconnect",
            ),
        ] {
            let mut reader = io.connect(connector, &self, turn).await.map_err(at(auth))?;
            reader
                .history(&mut io, control_req, control_filter.clone(), &control)
                .await
                .map_err(at(control_check))?;
            reader
                .history(&mut io, group_req, group_filter.clone(), &group)
                .await
                .map_err(at(group_check))?;
            if turn == Turn::Recipient {
                reader
                    .refuse_history(
                        &mut io,
                        "control-wide",
                        json!({"kinds":[CONTROL], "since":started, "until":started, "limit":4}),
                        Refusal::Scope,
                        true,
                    )
                    .await
                    .map_err(at("peer_qualification_control_wide_read"))?;
                reader
                    .refuse_history(
                        &mut io,
                        "group-wide",
                        json!({"kinds":[GROUP]}),
                        Refusal::Scope,
                        true,
                    )
                    .await
                    .map_err(at("peer_qualification_group_wide_read"))?;
                reader
                    .refuse_history(
                        &mut io,
                        "group-wrong",
                        json!({"kinds":[GROUP], "#h":[wrong]}),
                        Refusal::Scope,
                        false,
                    )
                    .await
                    .map_err(at("peer_qualification_group_foreign_read"))?;
            }
        }
        io.sockets.complete()?;
        io.recheck()?;
        let finished = io.last_wall;
        if Instant::now() >= io.deadline {
            return Err(TIMEOUT);
        }
        Ok(Completed {
            nonce: self.nonce,
            origin: relay.origin.to_string(),
            trust: qualification::trust_expression(&relay.trust),
            started,
            finished,
            begun: io.begun,
        })
    }

    fn control(&self, recipient: PublicKey, now: u64, pad: &str) -> Result<Signed> {
        let author = Keys::generate();
        let plain = format!("marmish qualification {}{pad}", hex::encode(self.nonce));
        let content = nip44::encrypt(author.secret_key(), &recipient, &plain, Version::V2)
            .map_err(|_| REFUSED)?;
        Signed::new(
            EventBuilder::new(Kind::Custom(CONTROL), content)
                .tags([
                    Tag::public_key(recipient),
                    Tag::parse(["expiration", &now.saturating_add(300).to_string()])
                        .map_err(|_| REFUSED)?,
                ])
                .custom_created_at(Timestamp::from_secs(now)),
            &author,
        )
    }

    fn group(&self, route: &str, now: u64) -> Result<Signed> {
        Signed::new(
            EventBuilder::new(Kind::Custom(GROUP), hex::encode(self.nonce))
                .tags([Tag::parse(["h", route]).map_err(|_| REFUSED)?])
                .custom_created_at(Timestamp::from_secs(now)),
            &self.writer,
        )
    }
}

pub(super) struct Completed {
    nonce: [u8; 32],
    origin: String,
    trust: String,
    started: u64,
    finished: u64,
    begun: Instant,
}

impl Completed {
    pub(super) fn valid_clock(&self, wall: u64) -> bool {
        reasonable_clock(self.started, self.finished, wall, self.begun.elapsed())
    }

    pub(super) fn evidence(&self, relay: &Relay, wall: u64) -> Result<(u64, u64, [u8; 32])> {
        #[derive(Serialize)]
        struct Summary<'a> {
            schema: u8,
            suite: &'static str,
            nonce: String,
            origin: &'a str,
            trust_digest: String,
            started: u64,
            finished: u64,
            passed: [&'static str; 13],
        }
        if self.origin != relay.origin.as_str()
            || self.trust != qualification::trust_expression(&relay.trust)
            || !self.valid_clock(wall)
        {
            return Err(REFUSED);
        }
        let digest = serde_json::to_vec(&Summary {
            schema: 1,
            suite: "private-wss-probe-v1",
            nonce: hex::encode(self.nonce),
            origin: &self.origin,
            trust_digest: hex::encode(Sha256::digest(self.trust.as_bytes())),
            started: self.started,
            finished: self.finished,
            passed: [
                "registered-nip42",
                "unregistered-nip42-refused",
                "unauthenticated-control-write-and-both-history-scopes-refused",
                "control-write",
                "control-read-eose-exact-bytes",
                "control-reconnect-eose-exact-bytes",
                "control-cross-recipient-refused",
                "control-wide-refused",
                "control-oversize-refused",
                "group-write",
                "group-read-and-reconnect-eose-exact-bytes",
                "group-wrong-route-read-and-write-refused",
                "group-wide-refused",
            ],
        })
        .map_err(|_| REFUSED)?;
        Ok((self.started, self.finished, Sha256::digest(digest).into()))
    }
}

struct Signed {
    id: String,
    bytes: String,
}

impl Signed {
    fn new(builder: EventBuilder, signer: &Keys) -> Result<Self> {
        let event = builder.finalize(signer).map_err(|_| REFUSED)?;
        Ok(Self {
            id: event.id.to_hex(),
            bytes: serde_json::to_value(&event)
                .map_err(|_| REFUSED)?
                .to_string(),
        })
    }
}

// Admission serializes physical socket ownership. The only repeated
// principal is the final turn, which always receives a fresh NIP-42 challenge.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Turn {
    Writer,
    Outsider,
    Recipient,
    Reconnect,
}

#[derive(Default)]
struct SocketBudget {
    opened: usize,
    active: Weak<()>,
}

impl SocketBudget {
    const ORDER: [Turn; 4] = [
        Turn::Writer,
        Turn::Outsider,
        Turn::Recipient,
        Turn::Reconnect,
    ];

    fn admit(&mut self, turn: Turn) -> Result<Arc<()>> {
        if self.active.upgrade().is_some() || Self::ORDER.get(self.opened) != Some(&turn) {
            return Err(SOCKET_BUDGET);
        }
        let permit = Arc::new(());
        self.active = Arc::downgrade(&permit);
        self.opened += 1;
        Ok(permit)
    }

    fn complete(&self) -> Result<()> {
        if self.opened != Self::ORDER.len() || self.active.upgrade().is_some() {
            return Err(SOCKET_BUDGET);
        }
        Ok(())
    }
}

struct Budget<'a> {
    authority: &'a ProbeAuthority<'a>,
    last_wall: u64,
    initial_wall: u64,
    begun: Instant,
    deadline: Instant,
    frames: usize,
    bytes: usize,
    sockets: SocketBudget,
}

impl<'a> Budget<'a> {
    fn new(authority: &'a ProbeAuthority<'a>, wall: u64) -> Self {
        let begun = Instant::now();
        Self {
            authority,
            last_wall: wall,
            initial_wall: wall,
            begun,
            deadline: begun + Duration::from_secs(30),
            frames: FRAME_BUDGET,
            bytes: BYTE_BUDGET,
            sockets: SocketBudget::default(),
        }
    }

    fn admit(&mut self, bytes: usize) -> Result<()> {
        self.frames = self.frames.checked_sub(1).ok_or(TRAFFIC_BUDGET)?;
        self.bytes = self.bytes.checked_sub(bytes).ok_or(TRAFFIC_BUDGET)?;
        if Instant::now() >= self.deadline {
            return Err(TIMEOUT);
        }
        Ok(())
    }

    fn recheck(&mut self) -> Result<()> {
        let now = self.authority.check()?;
        if !reasonable_clock(self.initial_wall, self.last_wall, now, self.begun.elapsed()) {
            return Err(Error::Invalid("peer_qualification_clock"));
        }
        self.last_wall = now;
        Ok(())
    }

    async fn connect(
        &mut self,
        connector: &PeerRelayConnector,
        plan: &Plan,
        turn: Turn,
    ) -> Result<Socket> {
        self.recheck()?;
        let permit = self.sockets.admit(turn)?;
        let signer = match turn {
            Turn::Writer => &plan.writer,
            Turn::Outsider => &plan.outsider,
            Turn::Recipient | Turn::Reconnect => &plan.recipient,
        };
        let (sink, stream) = connector.open_probe(self.authority, self.deadline).await?;
        let mut socket = Socket {
            sink,
            stream,
            _permit: permit,
        };
        let message = socket.text(self).await?;
        let fields: Vec<&RawValue> = serde_json::from_str(&message).map_err(|_| REFUSED)?;
        let [verb, challenge] = fields.as_slice() else {
            return Err(REFUSED);
        };
        let challenge = string(challenge).ok_or(REFUSED)?;
        if string(verb).as_deref() != Some("AUTH")
            || !(1..=256).contains(&challenge.len())
            || challenge.chars().any(char::is_control)
        {
            return Err(REFUSED);
        }
        let auth = Signed::new(
            EventBuilder::new(Kind::Custom(22242), "").tags([
                Tag::parse(["relay", self.authority.relay.origin.as_str()]).map_err(|_| REFUSED)?,
                Tag::parse(["challenge", &challenge]).map_err(|_| REFUSED)?,
            ]),
            signer,
        )?;
        socket
            .send(self, format!("[\"AUTH\",{}]", auth.bytes))
            .await?;
        socket
            .ack(
                self,
                &auth.id,
                (turn == Turn::Outsider).then_some(Refusal::Unregistered),
            )
            .await?;
        Ok(socket)
    }
}

struct Socket {
    sink: WebSocketSink,
    stream: WebSocketStream,
    _permit: Arc<()>,
}

impl Socket {
    async fn send(&mut self, budget: &mut Budget<'_>, text: String) -> Result<()> {
        budget.recheck()?;
        budget.admit(text.len())?;
        timeout_at(budget.deadline, self.sink.send(Message::Text(text)))
            .await
            .map_err(|_| TIMEOUT)?
            .map_err(|_| REFUSED)
    }

    async fn text(&mut self, budget: &mut Budget<'_>) -> Result<String> {
        loop {
            let frame = timeout_at(budget.deadline, self.stream.next())
                .await
                .map_err(|_| TIMEOUT)?
                .ok_or(REFUSED)?
                .map_err(|_| REFUSED)?;
            match frame {
                Message::Text(text) => {
                    budget.admit(text.len())?;
                    return Ok(text);
                }
                Message::Ping(data) if data.len() <= 125 => {
                    budget.admit(data.len())?;
                    budget.admit(data.len())?;
                    timeout_at(budget.deadline, self.sink.flush())
                        .await
                        .map_err(|_| TIMEOUT)?
                        .map_err(|_| REFUSED)?;
                }
                Message::Pong(data) if data.len() <= 125 => budget.admit(data.len())?,
                _ => return Err(REFUSED),
            }
        }
    }

    async fn ack(
        &mut self,
        budget: &mut Budget<'_>,
        id: &str,
        refusal: Option<Refusal>,
    ) -> Result<()> {
        let text = self.text(budget).await?;
        let fields: Vec<&RawValue> = serde_json::from_str(&text).map_err(|_| REFUSED)?;
        let [verb, event, accepted, reason] = fields.as_slice() else {
            return Err(REFUSED);
        };
        if string(verb).as_deref() != Some("OK")
            || string(event).as_deref() != Some(id)
            || accepted.get() != if refusal.is_none() { "true" } else { "false" }
            || !string(reason)
                .is_some_and(|reason| refusal.is_none_or(|expected| expected.matches(&reason)))
        {
            return Err(REFUSED);
        }
        Ok(())
    }

    async fn publish(
        &mut self,
        budget: &mut Budget<'_>,
        event: &Signed,
        refusal: Option<Refusal>,
    ) -> Result<()> {
        self.send(budget, format!("[\"EVENT\",{}]", event.bytes))
            .await?;
        self.ack(budget, &event.id, refusal).await
    }

    async fn history(
        &mut self,
        budget: &mut Budget<'_>,
        id: &str,
        filter: Value,
        expected: &Signed,
    ) -> Result<()> {
        self.send(budget, json!(["REQ", id, filter]).to_string())
            .await?;
        let text = self.text(budget).await?;
        let fields: Vec<&RawValue> = serde_json::from_str(&text).map_err(|_| REFUSED)?;
        let [verb, subscription, event] = fields.as_slice() else {
            return Err(REFUSED);
        };
        // Exact full signed bytes match the independently constructed positive
        // event, including its route, principal, signature and event id.
        if string(verb).as_deref() != Some("EVENT")
            || string(subscription).as_deref() != Some(id)
            || event.get() != expected.bytes
        {
            return Err(REFUSED);
        }
        let text = self.text(budget).await?;
        let fields: Vec<&RawValue> = serde_json::from_str(&text).map_err(|_| REFUSED)?;
        if !matches!(fields.as_slice(), [verb, request]
            if string(verb).as_deref() == Some("EOSE") && string(request).as_deref() == Some(id))
        {
            return Err(REFUSED);
        }
        self.send(budget, json!(["CLOSE", id]).to_string()).await
    }

    async fn refuse_history(
        &mut self,
        budget: &mut Budget<'_>,
        id: &str,
        filter: Value,
        refusal: Refusal,
        exact_positive_already_observed: bool,
    ) -> Result<()> {
        self.send(budget, json!(["REQ", id, filter]).to_string())
            .await?;
        let text = self.text(budget).await?;
        let fields: Vec<&RawValue> = serde_json::from_str(&text).map_err(|_| REFUSED)?;
        match fields.as_slice() {
            [verb, request, reason]
                if string(verb).as_deref() == Some("CLOSED")
                    && string(request).as_deref() == Some(id)
                    && string(reason).is_some_and(|text| refusal.matches(&text)) =>
            {
                Ok(())
            }
            [verb, request]
                if exact_positive_already_observed
                    && string(verb).as_deref() == Some("EOSE")
                    && string(request).as_deref() == Some(id) =>
            {
                self.send(budget, json!(["CLOSE", id]).to_string()).await
            }
            _ => Err(REFUSED),
        }
    }
}

fn at(check: &'static str) -> impl FnOnce(Error) -> Error {
    move |error| match error {
        Error::Transport("peer_qualification_probe") => Error::Transport(check),
        other => other,
    }
}

fn string(value: &RawValue) -> Option<String> {
    serde_json::from_str(value.get()).ok()
}

#[derive(Clone, Copy)]
enum Refusal {
    Unregistered,
    Unauthenticated,
    Scope,
    Size,
}

impl Refusal {
    fn matches(self, message: &str) -> bool {
        let Some((code, detail)) = message.split_once(':') else {
            return false;
        };
        match self {
            Self::Unregistered => matches!(code, "restricted" | "blocked"),
            Self::Unauthenticated => code == "auth-required",
            Self::Scope => code == "restricted",
            Self::Size => {
                let detail = detail.to_ascii_lowercase();
                matches!(code, "invalid" | "restricted")
                    && (detail.contains("size") || detail.contains("too large"))
            }
        }
    }
}

fn reasonable_clock(initial: u64, previous: u64, current: u64, elapsed: Duration) -> bool {
    // Whole-second wall readings straddle a fractional monotonic observation.
    current >= previous && current.saturating_sub(initial) <= elapsed.as_secs().saturating_add(2)
}

#[cfg(test)]
pub(super) fn check_test_budget(authority: &ProbeAuthority<'_>) {
    let wall = authority.check().unwrap();
    let mut count = Budget::new(authority, wall);
    for _ in 0..FRAME_BUDGET {
        count.admit(1).unwrap();
    }
    assert!(matches!(
        count.admit(1),
        Err(Error::Transport("peer_qualification_traffic_budget"))
    ));

    let mut size = Budget::new(authority, wall);
    size.admit(BYTE_BUDGET - 1).unwrap();
    size.admit(1).unwrap();
    assert!(matches!(
        size.admit(1),
        Err(Error::Transport("peer_qualification_traffic_budget"))
    ));

    let mut time = Budget::new(authority, wall);
    time.deadline = Instant::now();
    assert!(matches!(
        time.admit(1),
        Err(Error::Transport("peer_qualification_deadline"))
    ));
    assert!(reasonable_clock(100, 101, 102, Duration::ZERO));
    assert!(!reasonable_clock(100, 101, 100, Duration::ZERO));
    assert!(!reasonable_clock(100, 101, 103, Duration::ZERO));
    assert!(reasonable_clock(100, 103, 131, Duration::from_secs(30)));
}

#[cfg(test)]
#[test]
fn peer_qualification_socket_budget_rejects_overlap_reorder_and_second_reconnect() {
    let mut budget = SocketBudget::default();
    assert!(matches!(
        budget.admit(Turn::Reconnect),
        Err(Error::Transport("peer_qualification_socket_budget"))
    ));
    assert!(budget.complete().is_err());
    let writer = budget.admit(Turn::Writer).unwrap();
    assert!(matches!(
        budget.admit(Turn::Outsider),
        Err(Error::Transport("peer_qualification_socket_budget"))
    ));
    drop(writer);
    drop(budget.admit(Turn::Outsider).unwrap());
    drop(budget.admit(Turn::Recipient).unwrap());
    let reopened = budget.admit(Turn::Reconnect).unwrap();
    assert!(budget.complete().is_err());
    drop(reopened);
    assert!(budget.complete().is_ok());
    assert!(matches!(
        budget.admit(Turn::Reconnect),
        Err(Error::Transport("peer_qualification_socket_budget"))
    ));
    assert!(matches!(
        budget.admit(Turn::Writer),
        Err(Error::Transport("peer_qualification_socket_budget"))
    ));
}
