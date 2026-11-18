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
use std::time::Duration;
use tokio::time::{Instant, timeout_at};

const CONTROL: u16 = 7435;
const GROUP: u16 = 445;
const FRAME_BUDGET: usize = 64;
const BYTE_BUDGET: usize = 2 * 1024 * 1024;
const CONTROL_CAP: usize = 65_536;
const REFUSED: Error = Error::Transport("peer_qualification_probe");
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
        let control = self.control(recipient, started, "")?;
        let oversize = self.control(recipient, started, &"x".repeat(49_152))?;
        if control.bytes.len() > CONTROL_CAP || oversize.bytes.len() <= CONTROL_CAP {
            return Err(REFUSED);
        }
        let group = self.group(&route, started)?;
        let wrong_group = self.group(&wrong, started)?;
        let mut writer = io.connect(connector, &self.writer, true).await?;
        writer.publish(&mut io, &control, true).await?;
        writer.publish(&mut io, &group, true).await?;
        writer.publish(&mut io, &oversize, false).await?;
        writer.publish(&mut io, &wrong_group, false).await?;
        // The writer cannot read the recipient's exact protected scope.
        let control_filter = json!({"kinds":[CONTROL], "#p":[recipient], "limit":4});
        writer
            .refuse_history(&mut io, "foreign-recipient", control_filter.clone())
            .await?;
        drop(writer);

        let outsider = io.connect(connector, &self.outsider, false).await?;
        drop(outsider);

        let group_filter = json!({"kinds":[GROUP], "#h":[route]});
        for (control_req, group_req) in [("control", "group"), ("control-reopen", "group-reopen")] {
            let mut reader = io.connect(connector, &self.recipient, true).await?;
            reader
                .history(&mut io, control_req, control_filter.clone(), &control)
                .await?;
            reader
                .history(&mut io, group_req, group_filter.clone(), &group)
                .await?;
            if control_req == "control" {
                reader
                    .refuse_history(
                        &mut io,
                        "control-wide",
                        json!({"kinds":[CONTROL], "limit":4}),
                    )
                    .await?;
                reader
                    .refuse_history(&mut io, "group-wide", json!({"kinds":[GROUP]}))
                    .await?;
                reader
                    .refuse_history(
                        &mut io,
                        "group-wrong",
                        json!({"kinds":[GROUP], "#h":[wrong]}),
                    )
                    .await?;
            }
        }
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
}

impl Completed {
    pub(super) fn evidence(self, relay: &Relay) -> Result<(u64, u64, [u8; 32])> {
        #[derive(Serialize)]
        struct Summary<'a> {
            schema: u8,
            suite: &'static str,
            nonce: String,
            origin: &'a str,
            trust_digest: String,
            started: u64,
            finished: u64,
            passed: [&'static str; 12],
        }
        if self.origin != relay.origin.as_str()
            || self.trust != qualification::trust_expression(&relay.trust)
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

struct Budget<'a> {
    authority: &'a ProbeAuthority<'a>,
    last_wall: u64,
    deadline: Instant,
    frames: usize,
    bytes: usize,
}

impl<'a> Budget<'a> {
    fn new(authority: &'a ProbeAuthority<'a>, wall: u64) -> Self {
        Self {
            authority,
            last_wall: wall,
            deadline: Instant::now() + Duration::from_secs(30),
            frames: FRAME_BUDGET,
            bytes: BYTE_BUDGET,
        }
    }

    fn admit(&mut self, bytes: usize) -> Result<()> {
        self.frames = self.frames.checked_sub(1).ok_or(REFUSED)?;
        self.bytes = self.bytes.checked_sub(bytes).ok_or(REFUSED)?;
        if Instant::now() >= self.deadline {
            return Err(TIMEOUT);
        }
        Ok(())
    }

    fn recheck(&mut self) -> Result<()> {
        let now = self.authority.check()?;
        if now < self.last_wall {
            return Err(REFUSED);
        }
        self.last_wall = now;
        Ok(())
    }

    async fn connect(
        &mut self,
        connector: &PeerRelayConnector,
        signer: &Keys,
        registered: bool,
    ) -> Result<Socket> {
        self.recheck()?;
        let (sink, stream) = connector.open_probe(self.authority, self.deadline).await?;
        let mut socket = Socket { sink, stream };
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
        socket.ack(self, &auth.id, registered).await?;
        Ok(socket)
    }
}

struct Socket {
    sink: WebSocketSink,
    stream: WebSocketStream,
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

    async fn ack(&mut self, budget: &mut Budget<'_>, id: &str, positive: bool) -> Result<()> {
        let text = self.text(budget).await?;
        let fields: Vec<&RawValue> = serde_json::from_str(&text).map_err(|_| REFUSED)?;
        let [verb, event, accepted, reason] = fields.as_slice() else {
            return Err(REFUSED);
        };
        if string(verb).as_deref() != Some("OK")
            || string(event).as_deref() != Some(id)
            || accepted.get() != if positive { "true" } else { "false" }
            || !string(reason).is_some_and(|reason| positive || explicit_refusal(&reason))
        {
            return Err(REFUSED);
        }
        Ok(())
    }

    async fn publish(
        &mut self,
        budget: &mut Budget<'_>,
        event: &Signed,
        positive: bool,
    ) -> Result<()> {
        self.send(budget, format!("[\"EVENT\",{}]", event.bytes))
            .await?;
        self.ack(budget, &event.id, positive).await
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
    ) -> Result<()> {
        self.send(budget, json!(["REQ", id, filter]).to_string())
            .await?;
        let text = self.text(budget).await?;
        let fields: Vec<&RawValue> = serde_json::from_str(&text).map_err(|_| REFUSED)?;
        if !matches!(fields.as_slice(), [verb, request, reason]
            if string(verb).as_deref() == Some("CLOSED")
                && string(request).as_deref() == Some(id)
                && string(reason).is_some_and(|text| explicit_refusal(&text)))
        {
            return Err(REFUSED);
        }
        Ok(())
    }
}

fn string(value: &RawValue) -> Option<String> {
    serde_json::from_str(value.get()).ok()
}

fn explicit_refusal(text: &str) -> bool {
    matches!(
        text.split_once(':'),
        Some(("restricted" | "blocked" | "invalid" | "auth-required", _))
    )
}
