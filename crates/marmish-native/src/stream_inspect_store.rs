//! Explicit read-only SAME-UID progress projection; never returns command bytes.
use super::*;
use crate::stream_service::*;
use marmish_stream::{MAX_CONTENT, MAX_OFFSET, wire};

const RECORD_LIMIT: usize = 8;
const SLOT_LIMIT: usize = 5;
const RECORD_BYTES: usize = 524_288;
const RESPONSE_BYTES: usize = 49_152;
const RECORD_SQL: &str = "SELECT CASE WHEN length(k)<=8192 THEN k END,server_side,active,CASE WHEN length(record)<=524288 THEN record END,CASE WHEN length(principal)=32 THEN principal END FROM live_streams ORDER BY rowid DESC LIMIT 9";
const SLOT_SQL: &str = "SELECT sequence,CASE WHEN length(digest)=32 THEN digest END,CASE WHEN length(request)<=32768 THEN request END,response IS NOT NULL,CASE WHEN length(response)<=32768 THEN response END FROM stream_slots WHERE k=?1 ORDER BY rowid LIMIT 6";
// One materialized header prefix; no payload/native-ID/evidence/intent BLOB is read.
const OUTBOX_SQL: &str = "WITH headers AS MATERIALIZED (SELECT rowid,CASE WHEN length(k)<=8192 THEN k END AS k,CASE state WHEN 'unsubmitted' THEN 0 WHEN 'prepared' THEN 1 WHEN 'queued' THEN 2 WHEN 'published' THEN 3 WHEN 'uncertain' THEN 4 WHEN 'failed' THEN 5 WHEN 'acked' THEN 6 WHEN 'retired_failed' THEN 7 ELSE 8 END AS bucket,native_id IS NOT NULL AS native_present,intent IS NOT NULL AS intent_present,evidence IS NOT NULL AS evidence_present FROM stream_publications ORDER BY rowid DESC LIMIT 1025), bounded AS (SELECT * FROM headers ORDER BY rowid DESC LIMIT 1024) SELECT k,bucket,count(*),sum(native_present),sum(intent_present),sum(evidence_present),(SELECT count(*)>1024 FROM headers) FROM bounded WHERE k IN (?1,?2,?3,?4,?5,?6,?7,?8) GROUP BY k,bucket UNION ALL SELECT NULL,NULL,0,0,0,0,(SELECT count(*)>1024 FROM headers)";
const BUCKETS: [InspectionPublicationState; 9] = [
    InspectionPublicationState::Unsubmitted,
    InspectionPublicationState::Prepared,
    InspectionPublicationState::Queued,
    InspectionPublicationState::Published,
    InspectionPublicationState::Uncertain,
    InspectionPublicationState::Failed,
    InspectionPublicationState::Acked,
    InspectionPublicationState::RetiredFailed,
    InspectionPublicationState::Other,
];

impl Journal {
    pub(crate) fn stream_inspect(&self) -> Result<StreamInspection> {
        let tx = self.conn.unchecked_transaction().map_err(db)?;
        let mut snapshot = StreamInspection {
            schema: 1,
            entries: Vec::new(),
            records_truncated: false,
            outbox_truncated: false,
        };
        let mut keys = Vec::new();
        {
            let mut query = tx.prepare(RECORD_SQL).map_err(db)?;
            let mut rows = query.query([]).map_err(db)?;
            while let Some(row) = rows.next().map_err(db)? {
                if snapshot.entries.len() == RECORD_LIMIT {
                    snapshot.records_truncated = true;
                    break;
                }
                let key: Option<String> = row.get(0).map_err(db)?;
                let direction: i64 = row.get(1).map_err(db)?;
                let active: i64 = row.get(2).map_err(db)?;
                let bytes: Option<Vec<u8>> = row.get(3).map_err(db)?;
                let principal: Option<Vec<u8>> = row.get(4).map_err(db)?;
                let record = bytes
                    .as_deref()
                    .filter(|b| b.len() <= RECORD_BYTES)
                    .and_then(|b| serde_json::from_slice::<StreamRecord>(b).ok());
                let valid = record.as_ref().is_some_and(|r| {
                    key.as_deref() == Some(r.key().as_str())
                        && principal.as_deref() == Some(r.binding.client.as_slice())
                        && direction == i64::from(r.server_side)
                        && active == i64::from(r.active)
                        && r.sequence <= MAX_OFFSET
                        && r.stdin_accepted <= MAX_OFFSET
                        && r.stdin_delivered <= r.stdin_accepted
                        && r.stdout_eof.is_none_or(|n| n <= MAX_OFFSET)
                        && r.stderr_eof.is_none_or(|n| n <= MAX_OFFSET)
                });
                let mut entry = empty_entry();
                if valid {
                    let record = record.expect("validated record");
                    entry.binding = Some(record.binding.clone());
                    entry.direction = Some(if record.server_side {
                        InspectionDirection::Server
                    } else {
                        InspectionDirection::Client
                    });
                    entry.integrity = InspectionIntegrity::Valid;
                    entry.outbox = empty_outbox();
                    entry.phase = Some(phase(&record.state));
                    entry.server_hello_validated = record.server_side.then_some(record.hello);
                    entry.open_present = Some(record.open_digest.is_some());
                    entry.active = Some(record.active);
                    entry.cancel_requested = Some(record.cancel);
                    entry.projection_invalidated = Some(record.projection_invalidated);
                    entry.record_sequence = Some(record.sequence);
                    entry.record_stdin_accepted = Some(record.stdin_accepted);
                    entry.record_stdin_delivered = Some(record.stdin_delivered);
                    entry.stdout_eof = record.stdout_eof;
                    entry.stderr_eof = record.stderr_eof;
                    entry.terminal = record.terminal;
                    let key = record.key();
                    let (slots, truncated) = inspect_slots(&tx, &key, &record.binding)?;
                    if slots
                        .iter()
                        .any(|s| s.integrity == InspectionIntegrity::Invalid)
                    {
                        entry.integrity = InspectionIntegrity::Invalid;
                    }
                    entry.slots = slots;
                    entry.slots_truncated = truncated;
                    keys.push((key, snapshot.entries.len()));
                }
                snapshot.entries.push(entry);
            }
        }
        let mut parameters = vec![String::new(); RECORD_LIMIT];
        for (index, (key, _)) in keys.iter().enumerate() {
            parameters[index] = key.clone();
        }
        {
            let mut query = tx.prepare(OUTBOX_SQL).map_err(db)?;
            let mut rows = query
                .query(rusqlite::params_from_iter(parameters.iter()))
                .map_err(db)?;
            while let Some(row) = rows.next().map_err(db)? {
                snapshot.outbox_truncated = row.get(6).map_err(db)?;
                let key: Option<String> = row.get(0).map_err(db)?;
                if let Some((_, index)) = keys
                    .iter()
                    .find(|(k, _)| Some(k.as_str()) == key.as_deref())
                {
                    let bucket: i64 = row.get(1).map_err(db)?;
                    let bucket =
                        usize::try_from(bucket).map_err(|_| Error::Storage("inspection_bucket"))?;
                    let target = snapshot.entries[*index]
                        .outbox
                        .get_mut(bucket)
                        .ok_or(Error::Storage("inspection_bucket"))?;
                    target.count = row.get(2).map_err(db)?;
                    target.native_id_present = row.get(3).map_err(db)?;
                    target.intent_present = row.get(4).map_err(db)?;
                    target.evidence_present = row.get(5).map_err(db)?;
                }
            }
        }
        tx.commit().map_err(db)?;
        check_size(&snapshot)?;
        Ok(snapshot)
    }
}
fn check_size(snapshot: &StreamInspection) -> Result<()> {
    let envelope = crate::ipc::Response::stream(StreamLocalResult::Inspection {
        snapshot: Box::new(snapshot.clone()),
    });
    if serde_json::to_vec(&envelope)
        .map_err(|_| Error::Storage("inspection_encode"))?
        .len()
        > RESPONSE_BYTES
    {
        return Err(Error::Invalid("inspection_capacity"));
    }
    Ok(())
}
fn empty_entry() -> StreamInspectionEntry {
    StreamInspectionEntry {
        binding: None,
        direction: None,
        integrity: InspectionIntegrity::Invalid,
        phase: None,
        server_hello_validated: None,
        open_present: None,
        active: None,
        cancel_requested: None,
        projection_invalidated: None,
        record_sequence: None,
        record_stdin_accepted: None,
        record_stdin_delivered: None,
        stdout_eof: None,
        stderr_eof: None,
        terminal: None,
        slots: Vec::new(),
        slots_truncated: false,
        outbox: Vec::new(),
    }
}
fn empty_outbox() -> Vec<InspectionOutbox> {
    BUCKETS
        .iter()
        .map(|bucket| InspectionOutbox {
            bucket: *bucket,
            count: 0,
            native_id_present: 0,
            intent_present: 0,
            evidence_present: 0,
        })
        .collect()
}

fn phase(value: &str) -> InspectionPhase {
    use InspectionPhase::*;
    match value {
        "hello" => Hello,
        "opening" => Opening,
        "preparing" => Preparing,
        "launching" => Launching,
        "running" => Running,
        "cancelling" => Cancelling,
        "terminal" => Terminal,
        "authority_revoked" => AuthorityRevoked,
        "lost" => Lost,
        "unknown_launch" => UnknownLaunch,
        "transport_uncertain" => TransportUncertain,
        "reconciled_possible_live" => ReconciledPossibleLive,
        "cleanup_uncertain" => CleanupUncertain,
        "server_stopping" => ServerStopping,
        "server_stopped" => ServerStopped,
        "projection_invalidated" => ProjectionInvalidated,
        _ => Other,
    }
}
fn inspect_slots(
    conn: &Connection,
    key: &str,
    binding: &wire::Binding,
) -> Result<(Vec<InspectionSlot>, bool)> {
    let mut query = conn.prepare(SLOT_SQL).map_err(db)?;
    let mut rows = query.query([key]).map_err(db)?;
    let mut slots = Vec::new();
    let mut truncated = false;
    while let Some(row) = rows.next().map_err(db)? {
        if slots.len() == SLOT_LIMIT {
            truncated = true;
            break;
        }
        let sequence: i64 = row.get(0).map_err(db)?;
        let digest: Option<Vec<u8>> = row.get(1).map_err(db)?;
        let request: Option<Vec<u8>> = row.get(2).map_err(db)?;
        let cached: bool = row.get(3).map_err(db)?;
        let response: Option<Vec<u8>> = row.get(4).map_err(db)?;
        slots.push(inspect_slot(
            binding,
            sequence,
            digest.as_deref(),
            request.as_deref(),
            cached,
            response.as_deref(),
        ));
    }
    Ok((slots, truncated))
}
fn inspect_slot(
    binding: &wire::Binding,
    sequence: i64,
    digest: Option<&[u8]>,
    request: Option<&[u8]>,
    cached: bool,
    response: Option<&[u8]>,
) -> InspectionSlot {
    let mut slot = InspectionSlot {
        integrity: InspectionIntegrity::Invalid,
        kind: InspectionKind::Invalid,
        sequence: None,
        response_cached: cached,
        exchange: None,
        response: None,
    };
    let valid = (|| {
        let request_bytes = request.filter(|b| b.len() <= MAX_CONTENT)?;
        let req = wire::decode_request(request_bytes).ok()?;
        if &req.binding != binding
            || Some(req.sequence) != u64::try_from(sequence).ok()
            || Some(wire::request_digest(request_bytes).as_slice()) != digest
        {
            return None;
        }
        let response = if cached {
            let response =
                wire::decode_response(response.filter(|b| b.len() <= MAX_CONTENT)?).ok()?;
            if response.binding != req.binding
                || response.sequence != req.sequence
                || Some(response.request_sha256.as_slice()) != digest
                || !response_matches(&req.body, &response.body)
            {
                return None;
            }
            Some(response)
        } else {
            None
        };
        Some((req, response))
    })();
    if let Some((request, response)) = valid {
        slot.integrity = InspectionIntegrity::Valid;
        slot.sequence = Some(request.sequence);
        slot.kind = match request.body {
            wire::RequestBody::Hello { .. } => InspectionKind::Hello,
            wire::RequestBody::Open { .. } => InspectionKind::Open,
            wire::RequestBody::Exchange {
                ref stdin,
                stdin_eof,
                stdout_ack,
                stderr_ack,
            } => {
                slot.exchange = Some(InspectionExchange {
                    stdin_offset: stdin.as_ref().map(|b| b.offset),
                    stdin_end: stdin.as_ref().and_then(|b| b.end().ok()),
                    stdin_eof,
                    stdout_ack,
                    stderr_ack,
                });
                InspectionKind::Exchange
            }
            wire::RequestBody::Status => InspectionKind::Status,
            wire::RequestBody::Cancel => InspectionKind::Cancel,
            wire::RequestBody::CopyHello { .. } => InspectionKind::CopyHello,
            wire::RequestBody::OpenCopy { .. } => InspectionKind::OpenCopy,
        };
        if let Some(wire::Response {
            body:
                wire::ResponseBody::Exchange {
                    stdin_accepted,
                    stdin_delivered,
                    stdout,
                    stderr,
                    stdout_eof,
                    stderr_eof,
                    ..
                },
            ..
        }) = response
        {
            slot.response = Some(InspectionExchangeResponse {
                stdin_accepted,
                stdin_delivered,
                stdout_end: stdout.as_ref().and_then(|b| b.end().ok()),
                stderr_end: stderr.as_ref().and_then(|b| b.end().ok()),
                stdout_eof,
                stderr_eof,
            });
        }
    }
    slot
}

fn response_matches(request: &wire::RequestBody, response: &wire::ResponseBody) -> bool {
    use wire::{RequestBody as Q, ResponseBody as A};
    matches!(
        (request, response),
        (_, A::Error { .. })
            | (Q::Hello { .. }, A::Hello { .. })
            | (Q::CopyHello { .. }, A::CopyHello { .. })
            | (Q::Open { .. } | Q::OpenCopy { .. }, A::Opened { .. })
            | (
                Q::CopyHello { .. } | Q::OpenCopy { .. },
                A::CopyRefused { .. }
            )
            | (
                Q::Exchange { .. } | Q::Status | Q::Cancel,
                A::Exchange { .. }
            )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use marmish_stream::codec::{self, Codec};

    fn setup() -> (tempfile::TempDir, Journal) {
        let directory = super::super::private_test_directory();
        let journal = Journal::open(
            &directory.path().join("journal.db"),
            &[7; 32],
            &hex::encode([2; 32]),
        )
        .unwrap();
        (directory, journal)
    }
    fn record(id: u8, server_side: bool) -> StreamRecord {
        StreamRecord {
            binding: wire::Binding {
                stream: [id; 16],
                session: [9; 16],
                client: [1; 32],
                server: [2; 32],
            },
            group: vec![5],
            server_side,
            hello: true,
            copy_hello: false,
            open_digest: None,
            launch: StreamLaunch::Exec {
                argv: vec![b"SECRET_ARGV_NOT_INSPECTED".to_vec()],
            },
            codec: Codec::None,
            state: "hello".into(),
            cancel: false,
            active: false,
            terminal: None,
            identity: None,
            child_disposition: None,
            revision: 0,
            sequence: 0,
            stdin_accepted: 0,
            stdin_delivered: 0,
            stdout_eof: None,
            stderr_eof: None,
            completed_at: None,
            last_exchange_digest: None,
            payload_drained: false,
            payload_expired: false,
            projection_invalidated: false,
        }
    }
    fn add_slot(
        journal: &Journal,
        record: &StreamRecord,
        sequence: u64,
        body: wire::RequestBody,
        response: Option<wire::ResponseBody>,
    ) -> Vec<u8> {
        let request = wire::encode_request(&wire::Request {
            binding: record.binding.clone(),
            sequence,
            body,
        })
        .unwrap();
        let digest = wire::request_digest(&request);
        let response = response.map(|body| {
            wire::encode_response(&wire::Response {
                binding: record.binding.clone(),
                sequence,
                request_sha256: digest,
                body,
            })
            .unwrap()
        });
        journal.conn.execute("INSERT INTO stream_slots(k,digest,sequence,request,response) VALUES(?1,?2,?3,?4,?5)",params![record.key(),digest.as_slice(),i64::try_from(sequence).unwrap(),request,response]).unwrap();
        digest.to_vec()
    }
    fn exchange_response() -> wire::ResponseBody {
        wire::ResponseBody::Exchange {
            stdin_accepted: 10,
            stdin_delivered: 7,
            stdin_closed: false,
            stdout: Some(codec::encode(30, b"out", Codec::None).unwrap()),
            stderr: None,
            stdout_eof: None,
            stderr_eof: None,
            cancel_requested: false,
            terminal: None,
        }
    }
    #[test]
    fn inspection_projects_slots_counters_and_side_asymmetry_without_mutation() {
        let (_directory, mut journal) = setup();
        let mut server = record(1, true);
        server.state = "preparing".into();
        server.sequence = 3;
        server.stdin_accepted = 6;
        server.stdin_delivered = 4;
        journal.stream_insert(&server).unwrap();
        let mut client = record(2, false);
        client.state = "opening".into();
        client.open_digest = Some([8; 32]);
        journal.stream_insert(&client).unwrap();
        add_slot(
            &journal,
            &server,
            0,
            wire::RequestBody::Hello {
                codecs: vec![Codec::None],
            },
            None,
        );
        add_slot(
            &journal,
            &server,
            0,
            wire::RequestBody::Open {
                argv: vec![b"SECRET_SLOT_ARGV".to_vec()],
                codec: Codec::None,
            },
            None,
        );
        add_slot(
            &journal,
            &server,
            3,
            wire::RequestBody::Exchange {
                stdin: Some(codec::encode(6, b"data", Codec::None).unwrap()),
                stdin_eof: None,
                stdout_ack: 30,
                stderr_ack: 12,
            },
            Some(exchange_response()),
        );
        let before = journal.conn.total_changes();
        let snapshot = journal.stream_inspect().unwrap();
        assert_eq!(journal.conn.total_changes(), before);
        let c = &snapshot.entries[0];
        let s = &snapshot.entries[1];
        assert!(c.direction == Some(InspectionDirection::Client));
        assert_eq!(c.server_hello_validated, None);
        assert!(c.phase == Some(InspectionPhase::Opening));
        assert_eq!(s.server_hello_validated, Some(true));
        assert!(s.phase == Some(InspectionPhase::Preparing));
        assert_eq!(s.record_stdin_delivered, Some(4));
        assert_eq!(s.slots.len(), 3);
        assert!(s.slots[0].kind == InspectionKind::Hello);
        assert!(!s.slots[0].response_cached);
        assert!(s.slots[1].kind == InspectionKind::Open);
        let e = &s.slots[2];
        assert!(e.kind == InspectionKind::Exchange && e.response_cached);
        assert_eq!(e.exchange.as_ref().unwrap().stdin_end, Some(10));
        assert_eq!(e.response.as_ref().unwrap().stdin_delivered, 7);
        assert_eq!(e.response.as_ref().unwrap().stdout_end, Some(33));
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("SECRET"));
        assert!(serde_json::from_str::<StreamInspection>(&encoded).is_ok());
    }
    #[test]
    fn inspection_rejects_slot_digest_sequence_binding_and_cached_response_conflicts() {
        let (_directory, mut journal) = setup();
        let r = record(1, true);
        journal.stream_insert(&r).unwrap();
        let digest = add_slot(
            &journal,
            &r,
            1,
            wire::RequestBody::Exchange {
                stdin: None,
                stdin_eof: None,
                stdout_ack: 0,
                stderr_ack: 0,
            },
            Some(exchange_response()),
        );
        let original: Vec<u8> = journal
            .conn
            .query_row("SELECT request FROM stream_slots", [], |r| r.get(0))
            .unwrap();
        let good_response: Vec<u8> = journal
            .conn
            .query_row("SELECT response FROM stream_slots", [], |r| r.get(0))
            .unwrap();
        let mut wrong = wire::decode_response(&good_response).unwrap();
        wrong.binding.client = [8; 32];
        let bad_response = wire::encode_response(&wrong).unwrap();
        for (sequence, digest, request, response) in [
            (-1, digest.clone(), original.clone(), good_response.clone()),
            (1, vec![9; 32], original.clone(), good_response.clone()),
            (
                1,
                digest.clone(),
                vec![b'x'; MAX_CONTENT + 1],
                good_response.clone(),
            ),
            (1, digest.clone(), original.clone(), bad_response),
            (1, digest.clone(), original, vec![0; MAX_CONTENT + 1]),
        ] {
            journal
                .conn
                .execute(
                    "UPDATE stream_slots SET sequence=?1,digest=?2,request=?3,response=?4",
                    params![sequence, digest, request, response],
                )
                .unwrap();
            let snapshot = journal.stream_inspect().unwrap();
            let entry = &snapshot.entries[0];
            assert!(entry.integrity == InspectionIntegrity::Invalid);
            assert!(entry.slots[0].kind == InspectionKind::Invalid);
            assert!(entry.slots[0].exchange.is_none() && entry.slots[0].response.is_none());
            assert_eq!(entry.slots[0].sequence, None);
        }
    }
    #[test]
    fn inspection_slot_kind_and_request_binding_are_validated_before_projection() {
        let r = record(1, true);
        let body = wire::RequestBody::Exchange {
            stdin: None,
            stdin_eof: None,
            stdout_ack: 0,
            stderr_ack: 0,
        };
        let request = wire::encode_request(&wire::Request {
            binding: r.binding.clone(),
            sequence: 1,
            body: body.clone(),
        })
        .unwrap();
        let digest = wire::request_digest(&request);
        let wrong_kind = wire::encode_response(&wire::Response {
            binding: r.binding.clone(),
            sequence: 1,
            request_sha256: digest,
            body: wire::ResponseBody::Hello {
                codecs: vec![Codec::None],
                frame_bytes: 8192,
                window_bytes: 262144,
            },
        })
        .unwrap();
        let slot = inspect_slot(
            &r.binding,
            1,
            Some(&digest),
            Some(&request),
            true,
            Some(&wrong_kind),
        );
        assert!(slot.integrity == InspectionIntegrity::Invalid && slot.response.is_none());
        let mut other = r.binding.clone();
        other.stream = [8; 16];
        let request = wire::encode_request(&wire::Request {
            binding: other,
            sequence: 1,
            body,
        })
        .unwrap();
        let digest = wire::request_digest(&request);
        let slot = inspect_slot(&r.binding, 1, Some(&digest), Some(&request), false, None);
        assert!(slot.integrity == InspectionIntegrity::Invalid && slot.exchange.is_none());
    }
    #[test]
    fn inspection_record_invalidity_cannot_fabricate_zero_progress() {
        let (_directory, mut journal) = setup();
        let r = record(1, true);
        journal.stream_insert(&r).unwrap();
        for bytes in [vec![b'x'; RECORD_BYTES + 1], b"{}".to_vec()] {
            journal
                .conn
                .execute("UPDATE live_streams SET record=?1", [bytes])
                .unwrap();
            let snapshot = journal.stream_inspect().unwrap();
            let e = &snapshot.entries[0];
            assert!(e.integrity == InspectionIntegrity::Invalid);
            assert!(
                e.binding.is_none()
                    && e.record_sequence.is_none()
                    && e.record_stdin_delivered.is_none()
            );
        }
    }
    #[test]
    fn inspection_limits_newest_records_slots_and_publication_headers() {
        let (_directory, mut journal) = setup();
        for id in 1..=9 {
            journal.stream_insert(&record(id, true)).unwrap();
        }
        let r = record(9, true);
        for sequence in 1..=6 {
            add_slot(&journal, &r, sequence, wire::RequestBody::Status, None);
        }
        let tx = journal.conn.transaction().unwrap();
        for id in 0..1025 {
            let state = match id {
                1024 => "published",
                1023 => "queued",
                _ => "SECRET_UNKNOWN_STATE",
            };
            tx.execute("INSERT INTO stream_publications(group_id,event_id,k,payload,state,native_id,intent,evidence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![[5u8].as_slice(),id.to_string(),r.key(),b"SECRET_PAYLOAD".as_slice(),state,Some(b"SECRET_NATIVE".as_slice()),Some(b"SECRET_INTENT".as_slice()),Some(b"SECRET_EVIDENCE".as_slice())]).unwrap();
        }
        tx.commit().unwrap();
        let before = journal.conn.total_changes();
        let snapshot = journal.stream_inspect().unwrap();
        assert_eq!(before, journal.conn.total_changes());
        assert!(snapshot.records_truncated && snapshot.outbox_truncated);
        assert_eq!(snapshot.entries.len(), 8);
        let entry = &snapshot.entries[0];
        assert_eq!(entry.binding.as_ref().unwrap().stream, [9; 16]);
        assert!(entry.slots_truncated);
        assert_eq!(entry.slots.len(), 5);
        assert_eq!(entry.outbox[3].count, 1);
        assert_eq!(entry.outbox[2].count, 1);
        assert_eq!(entry.outbox[8].count, 1022);
        assert_eq!(entry.outbox[3].native_id_present, 1);
        assert!(!serde_json::to_string(&snapshot).unwrap().contains("SECRET"));
    }
    #[test]
    fn inspection_sql_plans_use_rowid_order_and_existing_key_index() {
        let (_directory, journal) = setup();
        let plan = |sql: &str, parameter: bool| {
            let mut q = journal
                .conn
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            if parameter {
                q.query_map(["k"], |r| r.get::<_, String>(3))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
            } else {
                q.query_map([], |r| r.get::<_, String>(3))
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap()
            }
        };
        let records = plan(RECORD_SQL, false).join(";");
        assert!(!records.contains("TEMP B-TREE"), "{records}");
        let slots = plan(SLOT_SQL, true).join(";");
        assert!(
            slots.contains("SEARCH stream_slots USING INDEX") && slots.contains("k=?"),
            "{slots}"
        );
    }
    #[test]
    fn inspection_closed_schema_and_unchanged_list_codec() {
        assert_eq!(
            serde_json::to_string(&StreamLocalRequest::List { remote: None }).unwrap(),
            r#"{"operation":"list","remote":null}"#
        );
        assert_eq!(
            serde_json::to_string(&StreamLocalResult::Entries { entries: vec![] }).unwrap(),
            r#"{"state":"entries","entries":[]}"#
        );
        assert_eq!(
            serde_json::to_string(&StreamLocalRequest::Inspect {}).unwrap(),
            r#"{"operation":"inspect"}"#
        );
        assert!(
            serde_json::from_str::<StreamLocalRequest>(
                r#"{"operation":"inspect","remote":"secret"}"#
            )
            .is_err()
        );
        let (_directory, journal) = setup();
        let snapshot = journal.stream_inspect().unwrap();
        let mut value = serde_json::to_value(snapshot).unwrap();
        value["extra"] = true.into();
        assert!(serde_json::from_value::<StreamInspection>(value).is_err());
        assert!(serde_json::from_str::<InspectionKind>(r#""unknown""#).is_err());
    }
    #[test]
    fn inspection_worst_case_envelope_stays_below_capacity() {
        let mut entry = empty_entry();
        entry.binding = Some(record(255, true).binding);
        entry.direction = Some(InspectionDirection::Server);
        entry.phase = Some(InspectionPhase::ReconciledPossibleLive);
        entry.integrity = InspectionIntegrity::Valid;
        entry.record_sequence = Some(MAX_OFFSET);
        entry.record_stdin_accepted = Some(MAX_OFFSET);
        entry.record_stdin_delivered = Some(MAX_OFFSET);
        entry.stdout_eof = Some(MAX_OFFSET);
        entry.stderr_eof = Some(MAX_OFFSET);
        entry.terminal = Some(wire::Terminal::Signaled { signal: 255 });
        entry.server_hello_validated = Some(false);
        entry.open_present = Some(false);
        entry.active = Some(false);
        entry.cancel_requested = Some(false);
        entry.projection_invalidated = Some(false);
        let slot = InspectionSlot {
            integrity: InspectionIntegrity::Valid,
            kind: InspectionKind::Exchange,
            sequence: Some(MAX_OFFSET),
            response_cached: true,
            exchange: Some(InspectionExchange {
                stdin_offset: Some(MAX_OFFSET),
                stdin_end: Some(MAX_OFFSET),
                stdin_eof: Some(MAX_OFFSET),
                stdout_ack: MAX_OFFSET,
                stderr_ack: MAX_OFFSET,
            }),
            response: Some(InspectionExchangeResponse {
                stdin_accepted: MAX_OFFSET,
                stdin_delivered: MAX_OFFSET,
                stdout_end: Some(MAX_OFFSET),
                stderr_end: Some(MAX_OFFSET),
                stdout_eof: Some(MAX_OFFSET),
                stderr_eof: Some(MAX_OFFSET),
            }),
        };
        entry.slots = vec![slot; 5];
        entry.outbox = empty_outbox();
        for bucket in &mut entry.outbox {
            bucket.count = 1024;
            bucket.native_id_present = 1024;
            bucket.evidence_present = 1024;
            bucket.intent_present = 1024;
        }
        let snapshot = StreamInspection {
            schema: 1,
            entries: vec![entry; 8],
            records_truncated: true,
            outbox_truncated: true,
        };
        check_size(&snapshot).unwrap();
    }
    #[tokio::test(start_paused = true)]
    async fn inspection_queued_attempt_outlives_client_drop_but_not_ipc_deadline() {
        use crate::ipc;
        use tokio::sync::{mpsc, watch};
        let (directory, journal) = setup();
        let socket = directory.path().join("i");
        let listener = ipc::bind(&socket).unwrap();
        let (queue, mut receive) = mpsc::channel::<ipc::Pending>(1);
        let (_status, snapshots) = watch::channel(ipc::Response::success(
            serde_json::json!({"state":"active"}),
        ));
        let server = tokio::spawn(ipc::serve(listener, queue.clone(), snapshots));
        for expired in [false, true] {
            let path = socket.clone();
            let caller = tokio::spawn(async move {
                ipc::stream_request(&path, StreamLocalRequest::Inspect {}).await
            });
            // Deliberately stalled actor: do not dequeue until the caller is dropped.
            while queue.capacity() != 0 {
                tokio::task::yield_now().await;
            }
            caller.abort();
            let _ = caller.await;
            let pending = receive.recv().await.unwrap();
            assert!(matches!(
                pending.request,
                ipc::Request::Stream {
                    request: StreamLocalRequest::Inspect {}
                }
            ));
            assert!(!pending.respond.is_closed());
            if expired {
                tokio::time::advance(std::time::Duration::from_secs(15)).await;
                for _ in 0..20 {
                    if pending.respond.is_closed() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                assert!(pending.respond.is_closed());
                assert!(pending.deadline <= tokio::time::Instant::now());
            } else {
                assert!(pending.deadline > tokio::time::Instant::now());
                let before = journal.conn.total_changes();
                let snapshot = journal.stream_inspect().unwrap();
                assert_eq!(before, journal.conn.total_changes());
                assert!(
                    pending
                        .respond
                        .send(ipc::Response::stream(StreamLocalResult::Inspection {
                            snapshot: Box::new(snapshot)
                        }))
                        .is_ok()
                );
                tokio::task::yield_now().await;
            }
        }
        server.abort();
        let _ = server.await;
    }
}
