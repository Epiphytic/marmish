use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use futures_util::future::join_all;
use nostr::event::FinalizeEvent;
use nostr::event::{Event, EventBuilder, EventId, Kind};
use nostr::filter::Filter;
use nostr::key::{Keys, PublicKey};
use nostr::types::{RelayUrl, Timestamp};
use nostr_sdk::client::{Client, ReqTarget};
use nostr_sdk::relay::{AcquisitionEnd, AcquisitionLimits, EventSendStatus, RelayAcquisition};
use serde::Serialize;
use serde_json::Value;
use serde_json::value::RawValue;
use tokio::time::{Instant, timeout_at};

use crate::profile_directory::{DirectoryId, UniqueFields};
use crate::transport::RelayPolicy;
use crate::{Error, Result};

const MAX_EVENTS: usize = 16;
const MAX_CONTENT_BYTES: usize = 16 * 1024;
const MAX_RELAY_EVENT_BYTES: usize = MAX_EVENTS * (MAX_CONTENT_BYTES + 4096);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Serialize)]
pub(crate) struct RelayCoverage {
    pub(crate) queried: usize,
    pub(crate) complete: usize,
}

/// Keep each unmodified field's original JSON, including numbers and nested
/// values the application doesn't interpret.
type Metadata = UniqueFields<Box<RawValue>>;

impl Metadata {
    fn parse(event: &Event) -> Result<Self> {
        if event.content.len() > MAX_CONTENT_BYTES {
            return Err(Error::Invalid("profile_metadata_size"));
        }
        serde_json::from_str(&event.content).map_err(|_| Error::Invalid("profile_metadata_json"))
    }

    fn nip05(&self) -> Result<Option<String>> {
        self.0
            .get("nip05")
            .map(|value| {
                let text: String = serde_json::from_str(value.get())
                    .map_err(|_| Error::Invalid("profile_metadata_nip05"))?;
                let identifier: DirectoryId = text
                    .parse()
                    .map_err(|_| Error::Invalid("profile_metadata_nip05"))?;
                Ok(identifier.to_string())
            })
            .transpose()
    }

    fn set_text(&mut self, name: &str, value: &str) -> Result<()> {
        let value = serde_json::value::to_raw_value(value)
            .map_err(|_| Error::Invalid("profile_metadata_json"))?;
        self.0.insert(name.to_owned(), value);
        Ok(())
    }
}

pub(crate) struct MetadataSnapshot {
    author: PublicKey,
    relays: Vec<RelayUrl>,
    events: Vec<Event>,
    pub(crate) coverage: RelayCoverage,
}

impl MetadataSnapshot {
    fn from_reports(
        author: PublicKey,
        relays: Vec<RelayUrl>,
        reports: Vec<Option<RelayAcquisition>>,
    ) -> Self {
        let mut events = Vec::new();
        let mut seen = BTreeSet::new();
        let mut complete = 0;
        for report in reports.into_iter().flatten() {
            if !verified_report(&report, author) {
                continue;
            }
            complete += 1;
            for event in report.events {
                if seen.insert(event.id) {
                    events.push(event);
                }
            }
        }
        events.sort_unstable_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let coverage = RelayCoverage {
            queried: relays.len(),
            complete,
        };
        Self {
            author,
            relays,
            events,
            coverage,
        }
    }

    pub(crate) fn verify_nip05(&self, expected: &str) -> Result<()> {
        if self.coverage.complete == 0 {
            return Err(Error::Transport("profile_metadata_no_complete_relay"));
        }
        let latest = self
            .events
            .first()
            .ok_or(Error::Invalid("profile_metadata_missing"))?;
        let selected_nip05 = Metadata::parse(latest)?.nip05()?;
        for event in self
            .events
            .iter()
            .skip(1)
            .take_while(|event| event.created_at == latest.created_at)
        {
            if Metadata::parse(event)?.nip05()? != selected_nip05 {
                return Err(Error::Invalid("profile_metadata_conflict"));
            }
        }
        if selected_nip05.as_deref() != Some(expected) {
            return Err(Error::Invalid("profile_metadata_nip05_mismatch"));
        }
        Ok(())
    }

    fn publication_base(&self) -> Result<Option<&Event>> {
        if self.coverage.complete == 0 || self.coverage.complete != self.coverage.queried {
            return Err(Error::Transport("profile_metadata_incomplete"));
        }
        for event in &self.events {
            Metadata::parse(event)?.nip05()?;
        }
        if let [first, second, ..] = &self.events[..]
            && first.created_at == second.created_at
        {
            return Err(Error::Invalid("profile_metadata_conflict"));
        }
        Ok(self.events.first())
    }

    pub(crate) fn prepare_publication(
        &self,
        nip05: &str,
        email: Option<&str>,
    ) -> Result<MetadataPublication> {
        let base = self.publication_base()?;
        let mut metadata = base
            .map(Metadata::parse)
            .transpose()?
            .unwrap_or_else(|| UniqueFields(BTreeMap::new()));
        metadata.set_text("nip05", nip05)?;
        let mut preview = serde_json::json!({ "nip05": nip05 });
        if let Some(email) = email {
            metadata.set_text("email", email)?;
            preview["email"] = Value::String(email.to_owned());
        }
        let content = serde_json::to_string(&metadata)
            .map_err(|_| Error::Invalid("profile_metadata_json"))?;
        if content.len() > MAX_CONTENT_BYTES {
            return Err(Error::Invalid("profile_metadata_size"));
        }
        Ok(MetadataPublication {
            author: self.author,
            relays: self.relays.clone(),
            base: base.map(|event| event.id),
            preview,
            content,
        })
    }
}

fn verified_report(report: &RelayAcquisition, author: PublicKey) -> bool {
    // A parsed EVENT the SDK subsequently filtered out can't make a clean
    // empty page. This catches wrong-author/kind and invalid signatures even
    // when the SDK never forwards the corresponding event to the caller.
    matches!(report.end, AcquisitionEnd::Completed)
        && report.raw_received_items == report.received_items
        && report.raw_received_items <= MAX_EVENTS
        && report.events.iter().all(|event| {
            event.kind == Kind::Metadata
                && event.pubkey == author
                && event.content.len() <= MAX_CONTENT_BYTES
                && event.verify().is_ok()
        })
}

pub(crate) struct MetadataPublication {
    author: PublicKey,
    relays: Vec<RelayUrl>,
    base: Option<EventId>,
    preview: Value,
    content: String,
}

impl MetadataPublication {
    pub(crate) fn preview(&self) -> &Value {
        &self.preview
    }

    pub(crate) fn sign(&self, fresh: &MetadataSnapshot, signer: &Keys) -> Result<Event> {
        self.sign_at(fresh, signer, Timestamp::now())
    }

    fn sign_at(&self, fresh: &MetadataSnapshot, signer: &Keys, now: Timestamp) -> Result<Event> {
        if signer.public_key() != self.author {
            return Err(Error::Identity("profile_metadata_signer"));
        }
        if fresh.author != self.author || fresh.relays != self.relays {
            return Err(Error::Invalid("profile_metadata_changed"));
        }
        let base = fresh.publication_base()?;
        if base.map(|event| event.id) != self.base {
            return Err(Error::Invalid("profile_metadata_changed"));
        }
        let mut builder = EventBuilder::new(Kind::Metadata, &self.content).custom_created_at(now);
        if let Some(event) = base {
            if event.created_at >= now {
                return Err(Error::Invalid("profile_metadata_timestamp"));
            }
            builder = builder.tags(event.tags.as_ref().iter().cloned());
        }
        builder
            .finalize(signer)
            .map_err(|_| Error::Identity("profile_metadata_sign"))
    }
}

fn configured_relays(policy: &RelayPolicy) -> Result<Vec<RelayUrl>> {
    policy
        .urls()
        .iter()
        .map(|url| RelayUrl::parse(url.as_str()).map_err(|_| Error::Invalid("relay_url")))
        .collect()
}

pub(crate) async fn acquire(
    client: &Client,
    policy: &RelayPolicy,
    author: PublicKey,
    deadline: Instant,
) -> Result<MetadataSnapshot> {
    Ok(acquire_configured(client, configured_relays(policy)?, author, deadline).await)
}

async fn acquire_configured(
    client: &Client,
    relays: Vec<RelayUrl>,
    author: PublicKey,
    deadline: Instant,
) -> MetadataSnapshot {
    let deadline = deadline.min(Instant::now() + OPERATION_TIMEOUT);
    let reports = join_all(relays.iter().map(|relay| async move {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        timeout_at(deadline, async {
            let request =
                ReqTarget::single(relay, [Filter::new().author(author).kind(Kind::Metadata)]);
            let limits = AcquisitionLimits::new(1, MAX_EVENTS, MAX_RELAY_EVENT_BYTES, remaining);
            let handle = client.acquire_events(request, limits).await.ok()?;
            handle.finish().await.ok()?.relays.remove(relay)
        })
        .await
        .ok()
        .flatten()
    }))
    .await;
    MetadataSnapshot::from_reports(author, relays, reports)
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum PublicationStatus {
    Acknowledged,
    Unconfirmed,
}

#[derive(Serialize)]
pub(crate) struct RelayPublication {
    // One-based position in configured relay order. Avoid serializing a
    // configured URL path or any text the relay included in its ACK.
    relay: usize,
    status: PublicationStatus,
}

#[derive(Serialize)]
pub(crate) struct PublicationReport {
    pub(crate) acknowledged: usize,
    pub(crate) queried: usize,
    pub(crate) relays: Vec<RelayPublication>,
}

pub(crate) async fn publish(
    client: &Client,
    policy: &RelayPolicy,
    event: &Event,
    deadline: Instant,
) -> Result<PublicationReport> {
    Ok(publish_configured(client, &configured_relays(policy)?, event, deadline).await)
}

async fn publish_configured(
    client: &Client,
    relays: &[RelayUrl],
    event: &Event,
    deadline: Instant,
) -> PublicationReport {
    let deadline = deadline.min(Instant::now() + OPERATION_TIMEOUT);
    let relays = join_all(relays.iter().enumerate().map(|(index, relay)| async move {
        let status = match timeout_at(deadline, async {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let output = client
                .send_event(event)
                .to([relay])
                .save_into_database(false)
                .ok_timeout(remaining)
                .authentication_timeout(remaining)
                .await
                .ok()?;
            output.success.get(relay).cloned()
        })
        .await
        {
            Ok(Some(EventSendStatus::Ack(_))) => PublicationStatus::Acknowledged,
            _ => PublicationStatus::Unconfirmed,
        };
        RelayPublication {
            relay: index + 1,
            status,
        }
    }))
    .await;
    let acknowledged = relays
        .iter()
        .filter(|relay| matches!(relay.status, PublicationStatus::Acknowledged))
        .count();
    PublicationReport {
        acknowledged,
        queried: relays.len(),
        relays,
    }
}

#[cfg(test)]
mod core_tests {
    use super::*;
    use nostr::event::Tag;

    fn event(keys: &Keys, time: u64, content: &str) -> Event {
        EventBuilder::new(Kind::Metadata, content)
            .custom_created_at(Timestamp::from_secs(time))
            .finalize(keys)
            .unwrap()
    }

    fn batch(events: Vec<Event>) -> RelayAcquisition {
        let count = events.len();
        RelayAcquisition {
            events: events.into_iter().collect(),
            end: AcquisitionEnd::Completed,
            raw_received_items: count,
            oldest_raw_created_at: None,
            received_items: count,
            received_event_bytes: 0,
            duplicates: 0,
            high_water_items: count,
            high_water_event_bytes: 0,
        }
    }

    fn snapshot(keys: &Keys, batches: Vec<Option<RelayAcquisition>>) -> MetadataSnapshot {
        let relays = (0..batches.len())
            .map(|n| RelayUrl::parse(&format!("wss://relay{n}.example/")).unwrap())
            .collect();
        MetadataSnapshot::from_reports(keys.public_key(), relays, batches)
    }

    #[test]
    fn newest_metadata_is_decisive_and_domain_case_is_normalized() {
        let signer = Keys::generate();
        let older = event(&signer, 1, r#"{"nip05":"old@example.com"}"#);
        let newer = event(&signer, 2, r#"{"nip05":"alice@EXAMPLE.com"}"#);
        let result = snapshot(&signer, vec![Some(batch(vec![older, newer])), None]);
        assert_eq!(result.coverage.complete, 1);
        assert_eq!(result.coverage.queried, 2);
        assert!(result.verify_nip05("alice@example.com").is_ok());
        assert!(matches!(
            result.verify_nip05("old@example.com"),
            Err(Error::Invalid("profile_metadata_nip05_mismatch"))
        ));
        assert!(matches!(
            result.prepare_publication("alice@example.com", None),
            Err(Error::Transport("profile_metadata_incomplete"))
        ));
    }

    #[test]
    fn newest_timestamp_conflicts_block_lookup_and_even_compatible_ties_block_publish() {
        let signer = Keys::generate();
        let first = event(&signer, 1, r#"{"nip05":"alice@example.com"}"#);
        let other = event(&signer, 1, r#"{"nip05":"mallory@example.com"}"#);
        let result = snapshot(
            &signer,
            vec![
                Some(batch(vec![first.clone()])),
                Some(batch(vec![other.clone()])),
            ],
        );
        assert_eq!(result.events[0].id, first.id.min(other.id));
        assert!(matches!(
            result.verify_nip05("alice@example.com"),
            Err(Error::Invalid("profile_metadata_conflict"))
        ));
        let compatible = event(
            &signer,
            1,
            r#"{"nip05":"alice@example.com","name":"compatible"}"#,
        );
        let result = snapshot(&signer, vec![Some(batch(vec![compatible, first]))]);
        assert!(result.verify_nip05("alice@example.com").is_ok());
        assert!(matches!(
            result.prepare_publication("alice@example.com", None),
            Err(Error::Invalid("profile_metadata_conflict"))
        ));
    }

    #[test]
    fn eose_invalid_events_and_budget_cannot_prove_empty_metadata() {
        let signer = Keys::generate();
        let manual = |report| snapshot(&signer, vec![Some(report)]);
        let assert_incomplete = |result: MetadataSnapshot| {
            assert_eq!(result.coverage.complete, 0);
            assert!(matches!(
                result.prepare_publication("alice@example.com", None),
                Err(Error::Transport("profile_metadata_incomplete"))
            ));
        };

        let mut missing_eose = batch(vec![]);
        missing_eose.end = AcquisitionEnd::TimedOut;
        assert_incomplete(manual(missing_eose));

        let mut filtered = batch(vec![]);
        filtered.raw_received_items = 1;
        assert_incomplete(manual(filtered));

        let wrong_kind = EventBuilder::new(Kind::TextNote, r#"{"nip05":"alice@example.com"}"#)
            .finalize(&signer)
            .unwrap();
        assert_incomplete(manual(batch(vec![wrong_kind])));

        let wrong_author = event(&Keys::generate(), 1, "{}");
        assert_incomplete(manual(batch(vec![wrong_author])));

        let mut bad_signature = event(&signer, 1, "{}");
        bad_signature.sig = event(&signer, 2, "{}").sig;
        assert_incomplete(manual(batch(vec![bad_signature])));

        let oversized = event(&signer, 1, &" ".repeat(MAX_CONTENT_BYTES + 1));
        assert_incomplete(manual(batch(vec![oversized])));
        let candidate = event(&signer, 1, "{}");
        assert_incomplete(manual(batch(vec![candidate.clone(); MAX_EVENTS + 1])));
        assert_eq!(
            manual(batch(vec![candidate; MAX_EVENTS])).coverage.complete,
            1
        );
    }

    #[test]
    fn invalid_newest_metadata_cannot_fall_back_and_invalid_old_blocks_publication() {
        let signer = Keys::generate();
        let good = event(&signer, 2, r#"{"nip05":"alice@example.com"}"#);
        for bad in [
            String::from("[]"),
            String::from(r#"{"nip05":3}"#),
            String::from(r#"{"nip05":"alice@example.com","nip\u00305":"mallory@example.com"}"#),
        ] {
            let newest = snapshot(
                &signer,
                vec![Some(batch(vec![good.clone(), event(&signer, 3, &bad)]))],
            );
            assert!(newest.verify_nip05("alice@example.com").is_err());
            let stale = snapshot(
                &signer,
                vec![Some(batch(vec![good.clone(), event(&signer, 1, &bad)]))],
            );
            assert!(stale.verify_nip05("alice@example.com").is_ok());
            assert!(
                stale
                    .prepare_publication("alice@example.com", None)
                    .is_err()
            );
        }
    }

    #[test]
    fn signing_rechecks_the_base_and_preserves_unknown_fields_and_tags() {
        let signer = Keys::generate();
        let tag = Tag::parse(["x-metadata", "example"]).unwrap();
        let base = EventBuilder::new(
            Kind::Metadata,
            r#"{"nip05":"old@example.com","email":"old@example.com","unknown":{"large":123456789012345678901234567890}}"#,
        )
        .custom_created_at(Timestamp::from_secs(1))
        .tags([tag.clone(), tag])
        .finalize(&signer)
        .unwrap();
        let initial = snapshot(
            &signer,
            vec![
                Some(batch(vec![base.clone()])),
                Some(batch(vec![base.clone()])),
            ],
        );
        let publication = initial
            .prepare_publication("alice@example.com", None)
            .unwrap();
        assert_eq!(
            publication.preview(),
            &serde_json::json!({ "nip05": "alice@example.com" })
        );
        let event = publication
            .sign_at(&initial, &signer, Timestamp::from_secs(2))
            .unwrap();
        assert!(event.verify().is_ok());
        assert_eq!(event.tags, base.tags);
        assert!(event.content.contains(r#""email":"old@example.com""#));
        assert!(event.content.contains("123456789012345678901234567890"));
        assert!(matches!(
            publication.sign_at(&initial, &signer, Timestamp::from_secs(1)),
            Err(Error::Invalid("profile_metadata_timestamp"))
        ));
        assert!(matches!(
            publication.sign_at(&initial, &Keys::generate(), Timestamp::from_secs(2)),
            Err(Error::Identity("profile_metadata_signer"))
        ));
        let changed = snapshot(
            &signer,
            vec![Some(batch(vec![event])), Some(batch(vec![base]))],
        );
        assert!(matches!(
            publication.sign_at(&changed, &signer, Timestamp::from_secs(3)),
            Err(Error::Invalid("profile_metadata_changed"))
        ));
        let email = initial
            .prepare_publication("alice@example.com", Some("new@example.com"))
            .unwrap();
        assert_eq!(email.preview()["email"], "new@example.com");
        let updated = email
            .sign_at(&initial, &signer, Timestamp::from_secs(2))
            .unwrap();
        assert!(updated.content.contains(r#""email":"new@example.com""#));
    }

    #[test]
    #[ignore = "opt-in latency measurement"]
    fn metadata_verification_latency() {
        let signer = Keys::generate();
        let event = event(
            &signer,
            1,
            r#"{"nip05":"alice@example.com","name":"Alice"}"#,
        );
        let mut times = Vec::new();
        let mut failures = 0;
        let relay = RelayUrl::parse("wss://relay.example/").unwrap();
        for sample in 0..220 {
            let reports = vec![Some(batch(vec![event.clone()]))];
            let relays = vec![relay.clone()];
            let before = std::time::Instant::now();
            let result = MetadataSnapshot::from_reports(signer.public_key(), relays, reports)
                .verify_nip05("alice@example.com");
            if result.is_err() {
                failures += 1;
            } else if sample >= 20 {
                times.push(before.elapsed().as_micros());
            }
        }
        times.sort_unstable();
        let percentile = |p: usize| times.get((times.len() * p).div_ceil(100).saturating_sub(1));
        eprintln!(
            "metadata_verify samples={} failures={failures} p50_us={:?} p95_us={:?} p99_us={:?}",
            times.len(),
            percentile(50),
            percentile(95),
            percentile(99),
        );
        assert_eq!(failures, 0);
    }
}

#[cfg(test)]
pub(crate) mod relay_tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    pub(crate) struct Fixture {
        pub(crate) relay: RelayUrl,
        pub(crate) queries: Arc<AtomicUsize>,
        pub(crate) publications: Arc<std::sync::Mutex<Vec<Event>>>,
        task: JoinHandle<()>,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    impl Fixture {
        pub(crate) async fn start(
            author: PublicKey,
            events: Vec<Event>,
            send_eose: bool,
            ack: bool,
        ) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let relay =
                RelayUrl::parse(&format!("ws://{}", listener.local_addr().unwrap())).unwrap();
            let queries = Arc::new(AtomicUsize::new(0));
            let publications = Arc::new(std::sync::Mutex::new(Vec::new()));
            let query_count = queries.clone();
            let publication_count = publications.clone();
            let task = tokio::spawn(async move {
                loop {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut ws = accept_async(stream).await.unwrap();
                    while let Some(Ok(message)) = ws.next().await {
                        let text = match message {
                            Message::Text(text) => text,
                            Message::Close(_) => break,
                            _ => continue,
                        };
                        let frame: Value = serde_json::from_str(&text).unwrap();
                        match frame[0].as_str() {
                            Some("REQ") => {
                                query_count.fetch_add(1, Ordering::Relaxed);
                                assert_eq!(
                                    frame[2],
                                    serde_json::json!({"authors":[author.to_hex()],"kinds":[0]})
                                );
                                for event in &events {
                                    let response = serde_json::json!(["EVENT", frame[1], event]);
                                    ws.send(Message::Text(response.to_string().into()))
                                        .await
                                        .unwrap();
                                }
                                if send_eose {
                                    let response = serde_json::json!(["EOSE", frame[1]]);
                                    ws.send(Message::Text(response.to_string().into()))
                                        .await
                                        .unwrap();
                                }
                            }
                            Some("EVENT") => {
                                let event: Event =
                                    serde_json::from_value(frame[1].clone()).unwrap();
                                assert_eq!(event.pubkey, author);
                                assert_eq!(event.kind, Kind::Metadata);
                                assert!(event.verify().is_ok());
                                let response =
                                    serde_json::json!(["OK", event.id, ack, "relay diagnostic"]);
                                publication_count.lock().unwrap().push(event);
                                ws.send(Message::Text(response.to_string().into()))
                                    .await
                                    .unwrap();
                            }
                            Some("CLOSE") => {}
                            unexpected => panic!("unexpected test frame: {unexpected:?}"),
                        }
                    }
                }
            });
            Self {
                relay,
                queries,
                publications,
                task,
            }
        }
    }

    #[tokio::test]
    async fn eose_after_bad_signature_is_not_a_clean_empty_page() {
        let keys = Keys::generate();
        let author = keys.public_key();
        let mut bad = EventBuilder::new(Kind::Metadata, "{}")
            .finalize(&keys)
            .unwrap();
        bad.content = r#"{"nip05":"alice@example.com"}"#.into();
        let invalid = Fixture::start(author, vec![bad], true, false).await;
        let empty = Fixture::start(author, vec![], true, true).await;
        let client = Client::new();
        let configured = vec![invalid.relay.clone(), empty.relay.clone()];
        for relay in &configured {
            client.add_relay(relay).await.unwrap();
            client
                .try_connect_relay(relay, Duration::from_secs(2))
                .await
                .unwrap();
        }
        let snapshot = acquire_configured(
            &client,
            configured,
            author,
            Instant::now() + Duration::from_secs(2),
        )
        .await;
        assert_eq!(
            (snapshot.coverage.complete, snapshot.coverage.queried),
            (1, 2)
        );
        assert!(matches!(
            snapshot.prepare_publication("alice@example.com", None),
            Err(Error::Transport("profile_metadata_incomplete"))
        ));
        client.shutdown().await;
    }

    #[tokio::test]
    async fn targeted_profile_query_and_publication_keep_partial_evidence() {
        let keys = Keys::generate();
        let author = keys.public_key();
        let profile = EventBuilder::new(Kind::Metadata, r#"{"nip05":"alice@example.com"}"#)
            .finalize(&keys)
            .unwrap();
        let complete = Fixture::start(author, vec![profile.clone()], true, true).await;
        let incomplete = Fixture::start(author, vec![profile.clone()], false, false).await;
        let unconfigured = Fixture::start(author, vec![], true, true).await;
        let client = Client::new();
        for relay in [&complete.relay, &incomplete.relay, &unconfigured.relay] {
            client.add_relay(relay).await.unwrap();
            client
                .try_connect_relay(relay, Duration::from_secs(2))
                .await
                .unwrap();
        }
        let configured = vec![complete.relay.clone(), incomplete.relay.clone()];
        let snapshot = acquire_configured(
            &client,
            configured.clone(),
            author,
            Instant::now() + Duration::from_millis(700),
        )
        .await;
        assert_eq!(
            (snapshot.coverage.complete, snapshot.coverage.queried),
            (1, 2)
        );
        assert!(snapshot.verify_nip05("alice@example.com").is_ok());
        assert!(
            snapshot
                .prepare_publication("alice@example.com", None)
                .is_err()
        );
        // Use an already signed event to exercise ACK reporting independently
        // of the expected fail-closed publication decision above.
        let report = publish_configured(
            &client,
            &configured,
            &profile,
            Instant::now() + Duration::from_secs(2),
        )
        .await;
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::json!({
                "acknowledged": 1, "queried": 2,
                "relays": [
                    {"relay": 1, "status": "acknowledged"},
                    {"relay": 2, "status": "unconfirmed"}
                ]
            })
        );
        assert_eq!(unconfigured.queries.load(Ordering::Relaxed), 0);
        assert!(unconfigured.publications.lock().unwrap().is_empty());
        client.shutdown().await;
    }
}
