//! One serialized, authenticated Peer relay socket for the private fixture.

use std::{
    collections::VecDeque,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use async_wsocket::Message;
use futures_util::{FutureExt, SinkExt, StreamExt};
use nostr::{
    event::{Event, EventBuilder, FinalizeEvent, Kind, Tag},
    key::{Keys, PublicKey},
    types::Url,
};
use nostr_sdk::transport::websocket::{WebSocketSink, WebSocketStream, WebSocketTransport};
use serde_json::{json, value::RawValue};
use tokio::time::{Instant, timeout_at};

use crate::transport::RelayConnector;

use super::{
    peer_control_wire::{Flight, Prepared},
    wire::{self, Arrival, Budget, CARRIER, Failure, Packet, RECORD_LIMIT, Reserve, Result},
};

const PREDRAIN_LIMIT: usize = 12;

#[derive(Debug)]
struct Scope {
    origin: String,
    recipient: [u8; 32],
    namespace: [u8; 16],
    live: AtomicBool,
}

#[derive(Debug)]
pub(super) struct Page {
    scope: Arc<Scope>,
    request: String,
    bounds: [u64; 2],
    records: Vec<Arrival>,
}

impl Page {
    pub(super) fn request(&self) -> (&str, [u64; 2]) {
        (&self.request, self.bounds)
    }

    pub(super) fn is_full(&self) -> bool {
        self.records.len() == RECORD_LIMIT
    }

    pub(super) fn records(&self) -> &[Arrival] {
        &self.records
    }
}

#[derive(Debug)]
pub(super) struct LiveRecord {
    scope: Arc<Scope>,
    subscription: String,
    arrival: Arrival,
}

impl LiveRecord {
    pub(super) fn arrival(&self) -> &Arrival {
        &self.arrival
    }
}

pub(super) struct Owner {
    socket: Option<Socket>,
    scope: Arc<Scope>,
    live_id: String,
    next_page: u64,
    live_records: VecDeque<LiveRecord>,
    live_reserve: Reserve,
}

impl Owner {
    pub(super) async fn open(
        connector: &RelayConnector,
        origin: &str,
        recipient: PublicKey,
        signer: &Keys,
        deadline: Instant,
    ) -> Result<Self> {
        let [endpoint] = connector.policy().urls() else {
            return Err(Failure::Binding);
        };
        if !origin.starts_with("wss://")
            || endpoint.as_str() != origin
            || signer.public_key() != recipient
        {
            return Err(Failure::Binding);
        }
        check_deadline(deadline)?;
        limited(deadline, connector.bootstrap(deadline)).await?;
        let (sink, stream) = limited(deadline, connector.connect(endpoint, None)).await?;
        let mut socket = Socket { sink, stream };

        let mut namespace = [0; 16];
        getrandom::fill(&mut namespace).map_err(|_| Failure::Transport)?;
        let live_id = format!("mp/{}/l", hex::encode(namespace));
        socket.ready(endpoint, &live_id, signer, deadline).await?;
        Ok(Self {
            socket: Some(socket),
            scope: Arc::new(Scope {
                origin: origin.to_owned(),
                recipient: recipient.to_bytes(),
                namespace,
                live: AtomicBool::new(true),
            }),
            live_id,
            next_page: 0,
            live_records: VecDeque::with_capacity(RECORD_LIMIT),
            live_reserve: Reserve::default(),
        })
    }

    pub(super) fn accepts_page(&self, page: &Page) -> bool {
        self.current(&page.scope)
    }

    pub(super) fn origin(&self) -> &str {
        &self.scope.origin
    }

    pub(super) fn accepts_live(&self, record: &LiveRecord) -> bool {
        self.current(&record.scope) && record.subscription == self.live_id
    }

    fn current(&self, scope: &Arc<Scope>) -> bool {
        Arc::ptr_eq(scope, &self.scope) && scope.live.load(Ordering::Acquire)
    }

    pub(super) async fn wait_live(&mut self, deadline: Instant) -> Result<Option<LiveRecord>> {
        self.socket()?;
        if deadline <= Instant::now() {
            return Ok(None);
        }
        if let Some(record) = self.pop_live() {
            return Ok(Some(record));
        }
        let mut budget = Budget::default();
        let result = self
            .active(async |owner| owner.read_live(&mut budget, deadline).await)
            .await;
        match result {
            Ok(()) => Ok(self.pop_live()),
            Err(Failure::Deadline) => Ok(None),
            Err(error) => self.fail(error),
        }
    }

    async fn read_live(&mut self, budget: &mut Budget, deadline: Instant) -> Result<()> {
        let text = self.socket()?.text(budget, deadline).await?;
        self.push_live(wire::packet(&text)?)
    }

    fn pop_live(&mut self) -> Option<LiveRecord> {
        let record = self.live_records.pop_front()?;
        self.live_reserve.release(record.arrival.body.get().len());
        Some(record)
    }

    pub(super) async fn page(&mut self, bounds: [u64; 2], deadline: Instant) -> Result<Page> {
        self.socket()?;
        check_deadline(deadline)?;
        match self
            .active(async |owner| owner.scan(bounds, deadline).await)
            .await
        {
            Ok(page) => Ok(page),
            Err(error) => self.fail(error),
        }
    }

    async fn scan(&mut self, bounds: [u64; 2], deadline: Instant) -> Result<Page> {
        let [lower, upper] = bounds;
        if lower > upper {
            return Err(Failure::Binding);
        }
        let mut budget = Budget::default();
        self.predrain(&mut budget, deadline).await?;
        self.next_page = self.next_page.checked_add(1).ok_or(Failure::Capacity)?;
        let request = format!(
            "mp/{}/p/{}",
            hex::encode(self.scope.namespace),
            self.next_page
        );
        let req = json!(["REQ", request, {
            "kinds": [CARRIER], "#p": [hex::encode(self.scope.recipient)],
            "since": lower, "until": upper, "limit": RECORD_LIMIT,
        }]);
        self.socket()?.send(req.to_string(), deadline).await?;

        let mut reserve = Reserve::default();
        let mut records: Vec<Arrival> = Vec::with_capacity(RECORD_LIMIT);
        loop {
            let text = self.socket()?.text(&mut budget, deadline).await?;
            match wire::packet(&text)? {
                Packet::Event(id, body) if id == request => {
                    reserve.charge(body.get().len())?;
                    let item = Arrival::parse(body, &self.scope.recipient)?;
                    if !(lower..=upper).contains(&item.stamp)
                        || records.last().is_some_and(|last| last.stamp < item.stamp)
                        || records.iter().any(|earlier| earlier.id == item.id)
                    {
                        return Err(Failure::Grammar);
                    }
                    records.push(item);
                }
                Packet::End(id) if id == request => break,
                packet => self.push_live(packet)?,
            }
        }
        self.socket()?
            .send(json!(["CLOSE", request]).to_string(), deadline)
            .await?;
        Ok(Page {
            scope: self.scope.clone(),
            request,
            bounds,
            records,
        })
    }

    pub(super) async fn publish<T>(
        &mut self,
        prepared: &Prepared<T>,
        flight: Flight,
        deadline: Instant,
    ) -> Result<bool> {
        self.socket()?;
        check_deadline(deadline)?;
        let mut budget = Budget::default();
        let outcome = self
            .active(async |owner| {
                let frame = owner
                    .prepare_write(prepared, flight, &mut budget, deadline)
                    .await?;
                owner
                    .publish_flight(frame, flight, &mut budget, deadline)
                    .await
                    .map_err(|_| Failure::Ambiguous)
            })
            .await;
        match outcome {
            Ok(accepted) => Ok(accepted),
            Err(error) => self.fail(error),
        }
    }

    async fn prepare_write<T>(
        &mut self,
        prepared: &Prepared<T>,
        flight: Flight,
        budget: &mut Budget,
        deadline: Instant,
    ) -> Result<String> {
        self.predrain(budget, deadline).await?;
        let text = publication(prepared.frame(), flight)?;
        check_deadline(deadline)?;
        Ok(text)
    }

    async fn publish_flight(
        &mut self,
        frame: String,
        flight: Flight,
        budget: &mut Budget,
        deadline: Instant,
    ) -> Result<bool> {
        self.socket()?.send(frame, deadline).await?;
        loop {
            let text = self.socket()?.text(budget, deadline).await?;
            match wire::packet(&text)? {
                Packet::Acknowledged(id, positive) if id == flight.carrier_id.to_bytes() => {
                    return Ok(positive);
                }
                packet => self.push_live(packet)?,
            }
        }
    }

    async fn predrain(&mut self, budget: &mut Budget, deadline: Instant) -> Result<()> {
        for index in 0..=PREDRAIN_LIMIT {
            check_deadline(deadline)?;
            let Some(message) = self.socket()?.stream.next().now_or_never() else {
                return Ok(());
            };
            if index == PREDRAIN_LIMIT {
                return Err(Failure::Capacity);
            }
            if let Some(text) = self.socket()?.inspect(message, budget, deadline).await? {
                self.push_live(wire::packet(&text)?)?;
            }
        }
        unreachable!("predrain exits by the thirteenth frame");
    }

    fn push_live(&mut self, packet: Packet<'_>) -> Result<()> {
        let Packet::Event(sub, body) = packet else {
            return Err(Failure::Grammar);
        };
        if sub != self.live_id {
            return Err(Failure::Grammar);
        }
        self.live_reserve.charge(body.get().len())?;
        self.live_records.push_back(LiveRecord {
            scope: self.scope.clone(),
            subscription: sub,
            arrival: Arrival::parse(body, &self.scope.recipient)?,
        });
        Ok(())
    }

    pub(super) async fn stop(&mut self, deadline: Instant) -> Result<()> {
        self.socket()?;
        check_deadline(deadline)?;
        let message = json!(["CLOSE", self.live_id]).to_string();
        let result = self
            .active(async |owner| {
                let socket = owner.socket()?;
                socket.send(message, deadline).await?;
                limited(deadline, socket.sink.close()).await
            })
            .await;
        self.retire();
        result
    }

    // Dropping a pending operation can leave an unanswered request or a
    // partly written frame. Its socket generation may never be used again.
    async fn active<T>(
        &mut self,
        operation: impl AsyncFnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        let mut guard = ActiveOperation {
            owner: self,
            completed: false,
        };
        let result = operation(guard.owner).await;
        guard.completed = true;
        result
    }

    fn socket(&mut self) -> Result<&mut Socket> {
        self.socket.as_mut().ok_or(Failure::Retired)
    }

    fn fail<T>(&mut self, failure: Failure) -> Result<T> {
        self.retire();
        Err(failure)
    }

    fn retire(&mut self) {
        self.scope.live.store(false, Ordering::Release);
        self.socket.take();
        self.live_records.clear();
        self.live_reserve = Reserve::default();
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        self.retire();
    }
}

struct ActiveOperation<'a> {
    owner: &'a mut Owner,
    completed: bool,
}

impl Drop for ActiveOperation<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.owner.retire();
        }
    }
}

struct Socket {
    sink: WebSocketSink,
    stream: WebSocketStream,
}

impl Socket {
    async fn ready(
        &mut self,
        endpoint: &Url,
        live_id: &str,
        signer: &Keys,
        deadline: Instant,
    ) -> Result<()> {
        let mut budget = Budget::default();
        let text = self.text(&mut budget, deadline).await?;
        let Packet::Challenge(challenge) = wire::packet(&text)? else {
            return Err(Failure::Grammar);
        };
        let relay = Tag::parse(["relay", endpoint.as_str()]).map_err(|_| Failure::Binding)?;
        let challenge = Tag::parse(["challenge", &challenge]).map_err(|_| Failure::Binding)?;
        let event = EventBuilder::new(Kind::Custom(22242), "")
            .tags([relay, challenge])
            .finalize(signer)
            .map_err(|_| Failure::Binding)?;
        self.send(json!(["AUTH", event]).to_string(), deadline)
            .await?;
        let text = self.text(&mut budget, deadline).await?;
        if !matches!(wire::packet(&text)?, Packet::Acknowledged(id, true) if id == event.id.to_bytes())
        {
            return Err(Failure::Grammar);
        }
        let live = json!(["REQ", live_id, {
            "kinds": [CARRIER], "#p": [signer.public_key().to_hex()], "limit": 0,
        }]);
        self.send(live.to_string(), deadline).await?;
        let text = self.text(&mut budget, deadline).await?;
        if !matches!(wire::packet(&text)?, Packet::End(id) if id == live_id) {
            return Err(Failure::Grammar);
        }
        Ok(())
    }

    async fn send(&mut self, text: String, deadline: Instant) -> Result<()> {
        limited(deadline, self.sink.send(Message::Text(text))).await
    }

    async fn text(&mut self, budget: &mut Budget, deadline: Instant) -> Result<String> {
        loop {
            check_deadline(deadline)?;
            let arrival = timeout_at(deadline, self.stream.next())
                .await
                .map_err(|_| Failure::Deadline)?;
            check_received(deadline)?;
            if let Some(text) = self.inspect(arrival, budget, deadline).await? {
                return Ok(text);
            }
        }
    }

    async fn inspect<E>(
        &mut self,
        arrival: Option<std::result::Result<Message, E>>,
        budget: &mut Budget,
        deadline: Instant,
    ) -> Result<Option<String>> {
        let message = arrival
            .ok_or(Failure::Transport)?
            .map_err(|_| Failure::Transport)?;
        budget.admit(matches!(&message, Message::Ping(_) | Message::Pong(_)))?;
        match message {
            Message::Text(text) => Ok(Some(text)),
            Message::Ping(payload) if payload.len() <= 125 => {
                limited(deadline, self.sink.flush())
                    .await
                    .map_err(|_| Failure::Transport)?;
                Ok(None)
            }
            Message::Pong(payload) if payload.len() <= 125 => Ok(None),
            Message::Close(_) => Err(Failure::Transport),
            _ => Err(Failure::Grammar),
        }
    }
}

pub(super) fn publication(bytes: &[u8], flight: Flight) -> Result<String> {
    let raw: &RawValue = serde_json::from_slice(bytes).map_err(|_| Failure::Binding)?;
    let (id, _) = Arrival::index(raw, &flight.recipient.to_bytes())?;
    if id != flight.carrier_id.to_bytes() || raw.get().as_bytes() != bytes {
        return Err(Failure::Binding);
    }
    let event: Event = serde_json::from_str(raw.get()).map_err(|_| Failure::Binding)?;
    event.verify().map_err(|_| Failure::Binding)?;
    Ok(format!("[\"EVENT\",{}]", raw.get()))
}

fn check_received(deadline: Instant) -> Result<()> {
    // A consumed frame cannot be put back if the deadline wins.
    check_deadline(deadline).map_err(|_| Failure::Transport)
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(Failure::Deadline);
    }
    Ok(())
}

async fn limited<T, E>(
    deadline: Instant,
    work: impl Future<Output = std::result::Result<T, E>>,
) -> Result<T> {
    check_deadline(deadline)?;
    let result = timeout_at(deadline, work)
        .await
        .map_err(|_| Failure::Deadline)?
        .map_err(|_| Failure::Transport)?;
    check_deadline(deadline)?;
    Ok(result)
}

#[tokio::test(start_paused = true)]
async fn peer_raw_socket_core_deadline_boundaries() {
    use std::{
        future::{pending, poll_fn},
        time::Duration,
    };
    use tokio::time::advance;

    let step = Duration::from_secs(1);
    let end = Instant::now() + step;
    // The same bound wraps the close flush, and a pending flush must time out.
    assert_eq!(
        limited(end, pending::<std::result::Result<(), ()>>()).await,
        Err(Failure::Deadline)
    );
    assert_eq!(
        limited::<(), ()>(
            end,
            poll_fn(|_| panic!("expired operation must not be polled"))
        )
        .await,
        Err(Failure::Deadline)
    );
    assert_eq!(check_received(end), Err(Failure::Transport));

    let end = Instant::now() + step;
    let completes_at_deadline = async {
        advance(step).await;
        Ok::<(), ()>(())
    };
    assert_eq!(
        limited(end, completes_at_deadline).await,
        Err(Failure::Deadline)
    );
}
