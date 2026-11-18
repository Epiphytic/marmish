use super::state::{
    Applied, ClientBinding, Clock, HeartbeatView, PermissionView, Receiver, Refused,
};
use super::*;
use marmish_engine::{CommandPolicy, KeyRestrictions, PrincipalId, Restrictions};
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

const ROOM: Hex32 = Hex32([0x11; 32]);
const NOW: u64 = 100_000;
const WEEK: u64 = 7 * 24 * 3600;

fn nz(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn binding() -> ClientBinding {
    ClientBinding {
        room: ROOM,
        local_client: Hex32([0x22; 32]),
        pinned_server: Hex32([0x33; 32]),
        local_generation: nz(1),
    }
}

fn clock(wall: u64, monotonic: u64) -> Clock {
    Clock {
        wall,
        monotonic: Duration::from_secs(monotonic),
    }
}

fn beat(sequence: u64, sampled_at: u64, interval: u32, permission: Option<Generation>) -> Record {
    Record::new(
        ROOM,
        Payload::Heartbeat(Heartbeat {
            sequence: nz(sequence),
            sampled_at,
            interval,
            host: HostStatus::default(),
            permission,
        }),
    )
}

fn send(receiver: &mut Receiver, record: &Record, now: Clock) -> Result<Applied, Refused> {
    let created_at = match &record.payload {
        Payload::Heartbeat(heartbeat) => heartbeat.sampled_at,
        Payload::Permission(permission) => permission.issued_at,
    };
    receiver.receive(binding(), created_at, &record.encode().unwrap(), now, true)
}

fn page(generation: Generation, issued_at: u64, content: PageContent) -> Record {
    Record::new(
        ROOM,
        Payload::Permission(PermissionRecord {
            issued_at,
            generation,
            content,
        }),
    )
}

fn chunk(position: u8, total: u8, data: Vec<u8>) -> PageContent {
    PageContent::Chunk {
        page: position,
        total,
        encoded: PageBytes(data),
    }
}

fn raw_put(receiver: &mut Receiver, issued_at: u64, bytes: &[u8], now: Clock) -> Applied {
    receiver
        .receive(binding(), issued_at, bytes, now, true)
        .unwrap()
}

fn policy_with_patterns(patterns: Vec<String>) -> Description {
    Description::from_engine(
        CommandPolicyDescriptionV1 {
            version: 1,
            layers: vec![Restrictions {
                allow_exec: None,
                deny_exec: patterns,
            }],
        },
        Capability::Granted,
        Capability::Denied,
    )
    .unwrap()
}

fn sized_description(target: usize) -> Description {
    let normalized = || format!("/{}", "a".repeat(MAX_PATTERN_BYTES - 1));
    let base = policy_with_patterns((0..MAX_PATTERNS).map(|_| normalized()).collect());
    let mut delta = target - base.canonical_bytes().unwrap().len();
    let patterns = (0..MAX_PATTERNS)
        .map(|_| {
            let extra = delta.min(MAX_PATTERN_BYTES - 1);
            delta -= extra;
            let quote = if extra.is_multiple_of(2) { "" } else { "\"" };
            format!(
                "/{}{}{}",
                "\\".repeat(extra - quote.len()),
                quote,
                "a".repeat(MAX_PATTERN_BYTES - 1 - extra)
            )
        })
        .collect();
    assert_eq!(delta, 0);
    let description = policy_with_patterns(patterns);
    assert_eq!(description.canonical_bytes().unwrap().len(), target);
    description
}

fn records(plan: &PermissionPlan, at: u64) -> Vec<Vec<u8>> {
    plan.records(ROOM, at).collect::<Result<_, _>>().unwrap()
}

fn multi_page() -> Description {
    policy_with_patterns(
        (0..64)
            .map(|i| format!("/bin/{i}/{}*", "a".repeat(300)))
            .collect(),
    )
}

#[test]
fn peer_heartbeat_core_digest_and_actual_engine_policy_parity() {
    let principal = PrincipalId([13; 32]);
    let policy = CommandPolicy {
        version: 1,
        defaults: Restrictions {
            allow_exec: None,
            deny_exec: vec!["/bin/**".to_string()],
        },
        keys: vec![KeyRestrictions {
            principal,
            restrictions: Restrictions {
                allow_exec: Some(vec![]),
                deny_exec: vec![],
            },
        }],
    };
    policy.validate_for_principals(&[principal]).unwrap();
    let mut engine = policy.describe_for_principal(principal);
    let description =
        Description::from_engine(engine.clone(), Capability::Denied, Capability::Unsupported)
            .unwrap();
    let canonical = description.canonical_bytes().unwrap();
    assert_eq!(canonical, br#"{"version":1,"layers":[{"allow_exec":null,"deny_exec":["/bin/**"]},{"allow_exec":[],"deny_exec":[]}],"interactive":"denied","shell_command":"unsupported"}"#);
    assert_eq!(
        hex::encode(description_digest(&canonical).0),
        "5c97461dc1c73e799f0e6ac0c38d58202b6886c166a3aee388c9a261fd21ae7b"
    );
    assert_eq!(
        Description::decode_canonical(&canonical, description_digest(&canonical)),
        Ok(description)
    );

    let mut other = policy.clone();
    other.keys[0].restrictions.allow_exec = Some(vec![
        r"/bin/***/a\\b/\*".to_string(),
        "/bin/\u{202e}".to_string(),
    ]);
    other.validate_for_principals(&[principal]).unwrap();
    let actual = other.describe_for_principal(principal);
    let description =
        Description::from_engine(actual.clone(), Capability::Granted, Capability::Denied).unwrap();
    let bytes = description.canonical_bytes().unwrap();
    assert_eq!(
        Description::decode_canonical(&bytes, description_digest(&bytes)),
        Ok(description)
    );
    let observed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        observed["layers"],
        serde_json::to_value(actual.layers).unwrap()
    );

    engine.layers.push(Restrictions::default());
    assert_eq!(
        Description::from_engine(engine, Capability::Denied, Capability::Unsupported),
        Err(WireError::Invalid)
    );
    for pattern in ["relative", "/bin/abc\\", r"/bin/\q", "/a\n"] {
        assert!(!valid_pattern(pattern));
    }
    assert!(!valid_pattern(&format!(
        "/{}",
        "a".repeat(MAX_PATTERN_BYTES)
    )));
    assert!(valid_pattern(&format!(
        "/{}",
        "a".repeat(MAX_PATTERN_BYTES - 1)
    )));
}

#[test]
fn peer_heartbeat_core_closed_shapes_and_raw_gates() {
    let record = beat(1, NOW, 300, None);
    let bytes = record.encode().unwrap();
    let json = String::from_utf8(bytes.clone()).unwrap();
    let null = json.replace(
        "\"host\":{}",
        "\"host\":{\"username\":null,\"load\":null,\"ips\":null},\"permission\":null",
    );
    assert_eq!(Record::decode(null.as_bytes()), Ok(record.clone()));
    for mutated in [
        json.replace("\"version\":1", "\"version\":2"),
        json.replace("\"version\":1", "\"version\":1,\"r\":0"),
        json.replace("\"sequence\":1", "\"sequence\":1,\"sequence\":1"),
        json.replace("\"sequence\":1", "\"sequence\":0"),
        json.replace("\"sequence\":1", "\"sequence\":18446744073709551616"),
        json.replace(
            "\"host\":{}",
            "\"host\":{\"username\":null,\"username\":null}",
        ),
        json.replace("\"interval\":300", "\"interval\":4294967296"),
        json.replace(
            "\"kind\":\"heartbeat\"",
            "\"kind\":\"heartbeat\",\"kind\":\"heartbeat\"",
        ),
        json.replace(
            "\"kind\":\"heartbeat\"",
            "\"kind\":\"heartbeat\",\"extra\":42",
        ),
        json.replace("\"kind\":\"heartbeat\"", "\"kind\":\"unexpected\""),
        json.replace("\"host\":{}", "\"host\":{\"power\":3}"),
        json.replace(&hex::encode(ROOM.0), &"AA".repeat(32)),
        format!("{json}{{}}"),
    ] {
        assert_eq!(
            Record::decode(mutated.as_bytes()),
            Err(WireError::Invalid),
            "{mutated}"
        );
    }
    let mut padded = bytes;
    padded.resize(HEARTBEAT_LIMIT, b' ');
    assert_eq!(Record::decode(&padded), Ok(record));
    padded.push(b' ');
    assert_eq!(Record::decode(&padded), Err(WireError::TooLong));
    padded.resize(RECORD_LIMIT + 1, b'v');
    assert_eq!(Record::decode(&padded), Err(WireError::TooLong));
}

#[test]
fn peer_heartbeat_core_host_fields_and_maximum() {
    let generation = Generation {
        ordinal: nz(u64::MAX),
        digest: Hex32([0xff; 32]),
    };
    let mut disks: Vec<_> = (0..8)
        .map(|index| DiskStatus {
            label: format!(
                "{}{}",
                "\"".repeat(29),
                (0..3)
                    .map(|i| if index & (1 << i) == 0 { '"' } else { '\\' })
                    .collect::<String>()
            ),
            used: 10_000_000_000_000_000_000,
            available: 8_000_000_000_000_000_000,
        })
        .collect();
    disks.sort_by(|a, b| a.label.cmp(&b.label));
    let host = HostStatus {
        username: Some("\"".repeat(64)),
        uptime: Some(MAX_UPTIME),
        load: Some([MAX_LOAD; 3]),
        disks: Some(disks),
        ips: Some(
            (0..8)
                .map(|i| {
                    format!("ffff:ffff:ffff:ffff:ffff:ffff:ffff:fff{i}")
                        .parse()
                        .unwrap()
                })
                .collect(),
        ),
    };
    let record = Record::new(
        ROOM,
        Payload::Heartbeat(Heartbeat {
            sequence: nz(u64::MAX),
            sampled_at: u64::MAX,
            interval: 86_400,
            host,
            permission: Some(generation),
        }),
    );
    let bytes = record.encode().unwrap();
    eprintln!("maximal heartbeat fixture: {} canonical bytes", bytes.len());
    assert!(bytes.len() <= 2048);
    assert_eq!(Record::decode(&bytes), Ok(record));

    let json = String::from_utf8(beat(1, NOW, 300, None).encode().unwrap()).unwrap();
    for invalid in [
        r#"{"username":""}"#,
        r#"{"username":"a\u061c"}"#,
        r#"{"username":"a\u2028"}"#,
        r#"{"username":"a\u202e"}"#,
        r#"{"username":"a\u2069"}"#,
        r#"{"username":"a\n"}"#,
        r#"{"uptime":31557600001}"#,
        r#"{"load":[1,2]}"#,
        r#"{"load":[1,2,1000000001]}"#,
        r#"{"ips":[]}"#,
        r#"{"ips":["fe80::1%lo"]}"#,
        r#"{"ips":["0:0:0:0:0:0:0:1"]}"#,
        r#"{"ips":["127.0.0.1","127.0.0.1"]}"#,
        r#"{"ips":["::1","127.0.0.1"]}"#,
        r#"{"disks":[]}"#,
        r#"{"disks":[{"label":"","used":1,"available":0}]}"#,
        r#"{"disks":[{"label":"/","used":1,"available":0}]}"#,
        r#"{"disks":[{"label":"root","used":0,"available":0}]}"#,
        r#"{"disks":[{"label":"root","used":18446744073709551615,"available":1}]}"#,
        r#"{"disks":[{"label":"root","used":1,"available":1},{"label":"root","used":1,"available":1}]}"#,
    ] {
        assert_eq!(
            Record::decode(
                json.replace("\"host\":{}", &format!("\"host\":{invalid}"))
                    .as_bytes()
            ),
            Err(WireError::Invalid),
            "{invalid}"
        );
    }
    let allowed = json.replace(
        "\"host\":{}",
        r#""host":{"ips":["0.0.0.0","127.0.0.1","224.0.0.1","::1","fe80::1"]}"#,
    );
    assert!(Record::decode(allowed.as_bytes()).is_ok());
}

#[test]
fn peer_heartbeat_core_time_bindings_and_watermarks() {
    for interval in [299, 86_401] {
        assert_eq!(
            beat(1, NOW, interval, None).encode(),
            Err(WireError::Invalid)
        );
    }
    for interval in [300, 301, 319, 320, 86_400] {
        let jitter = match interval {
            300 | 301 | 319 => 15,
            320 => 16,
            _ => 300,
        };
        let freshness = u64::from(interval) + jitter + 120;
        let at = 1_000_000;
        let now = clock(at, 500);
        let mut receiver = Receiver::new(binding());
        assert_eq!(receiver.heartbeat(now), HeartbeatView::Unknown);
        assert_eq!(
            send(&mut receiver, &beat(1, at + 121, interval, None), now),
            Err(Refused::SignedTime)
        );
        assert_eq!(
            send(&mut receiver, &beat(1, at + 120, interval, None), now),
            Ok(Applied::Heartbeat)
        );
        assert!(
            matches!(receiver.heartbeat(now), HeartbeatView::Fresh(value) if value.age.is_zero())
        );
        let fresh = beat(2, at - freshness + 1, interval, None);
        assert_eq!(send(&mut receiver, &fresh, now), Ok(Applied::Heartbeat));
        assert!(
            matches!(receiver.heartbeat(clock(at + 1, 500)), HeartbeatView::Stale(Some(value))
            if value.sample.sequence == nz(2) && value.received_at == at && value.age.as_secs() == freshness)
        );
        assert_eq!(
            send(&mut receiver, &beat(3, at - freshness, interval, None), now),
            Ok(Applied::Watermark)
        );
        assert_eq!(
            send(&mut receiver, &beat(3, at, interval, None), now),
            Ok(Applied::Ignored)
        );
        let last = freshness + u64::from(interval);
        assert_eq!(
            send(&mut receiver, &beat(4, at - last, interval, None), now),
            Err(Refused::Expired)
        );
        assert_eq!(
            send(&mut receiver, &beat(4, at - last + 1, interval, None), now),
            Ok(Applied::Watermark)
        );
        assert_eq!(
            send(&mut receiver, &beat(4, at, interval, None), now),
            Ok(Applied::Ignored)
        );
        assert_eq!(
            send(&mut receiver, &beat(1, at, interval, None), now),
            Ok(Applied::Ignored)
        );
        assert!(
            matches!(receiver.heartbeat(now), HeartbeatView::Fresh(value) if value.sample.sequence == nz(2))
        );

        assert_eq!(
            receiver.heartbeat(clock(at - 1, 500)),
            HeartbeatView::Unknown
        );
        assert_eq!(receiver.heartbeat(clock(at, 499)), HeartbeatView::Unknown);
        let expired = clock(at + WEEK, 500);
        receiver.sweep(expired);
        assert_eq!(receiver.heartbeat(expired), HeartbeatView::Stale(None));
        assert_eq!(
            send(&mut receiver, &beat(4, at + WEEK, interval, None), expired),
            Ok(Applied::Ignored)
        );
    }

    let mut receiver = Receiver::new(binding());
    let beat = beat(u64::MAX, u64::MAX, 300, None);
    let data = beat.encode().unwrap();
    let now = clock(u64::MAX, 100);
    for active in [
        ClientBinding {
            local_generation: nz(2),
            ..binding()
        },
        ClientBinding {
            pinned_server: Hex32([0x44; 32]),
            ..binding()
        },
        ClientBinding {
            local_client: Hex32([0x44; 32]),
            ..binding()
        },
    ] {
        assert_eq!(
            receiver.receive(active, u64::MAX, &data, now, true),
            Err(Refused::Binding)
        );
    }
    let mut different_room = beat.clone();
    different_room.room = Hex32([0x44; 32]);
    assert_eq!(
        send(&mut receiver, &different_room, now),
        Err(Refused::Binding)
    );
    assert_eq!(
        receiver.receive(binding(), u64::MAX - 1, &data, now, true),
        Err(Refused::SignedTime)
    );
    assert_eq!(
        receiver.receive(binding(), u64::MAX, &data, clock(0, 100), true),
        Err(Refused::Clock)
    );
    assert_eq!(send(&mut receiver, &beat, now), Ok(Applied::Heartbeat));
    assert_eq!(
        receiver.receive(binding(), u64::MAX, &data, now, true),
        Ok(Applied::Ignored)
    );
}

#[test]
fn peer_heartbeat_core_permission_maximum_encoding_and_framing() {
    let plan = PermissionPlan::new(nz(u64::MAX), &sized_description(DESCRIPTION_LIMIT)).unwrap();
    let pages = records(&plan, u64::MAX);
    assert_eq!(pages.len(), 64);
    assert_eq!(pages[63].len(), 8024);
    let mut spaced = pages[63].clone();
    spaced.resize(RECORD_LIMIT, b' ');
    assert!(Record::decode(&spaced).is_ok());
    spaced.push(b' ');
    assert_eq!(Record::decode(&spaced), Err(WireError::TooLong));

    let too_large = PermissionPlan::new(nz(1), &sized_description(DESCRIPTION_LIMIT + 1)).unwrap();
    let [bytes]: [Vec<u8>; 1] = records(&too_large, NOW).try_into().unwrap();
    let control = page(
        too_large.generation,
        NOW,
        PageContent::TooLarge {
            description_len: 368_641,
        },
    );
    assert_eq!(Record::decode(&bytes), Ok(control));
    for (length, accepted) in [
        (368_640, false),
        (368_641, true),
        (533_440, true),
        (533_441, false),
    ] {
        assert_eq!(
            page(
                too_large.generation,
                NOW,
                PageContent::TooLarge {
                    description_len: length
                }
            )
            .encode()
            .is_ok(),
            accepted
        );
    }

    let valid = page(too_large.generation, NOW, chunk(0, 1, vec![b'Z']))
        .encode()
        .unwrap();
    let json = String::from_utf8(valid).unwrap();
    assert!(json.contains("Wg=="));
    for invalid in [
        json.replace("Wg==", "Wh=="),
        json.replace("Wg==", "Wg"),
        json.replace("Wg==", ""),
        json.replace("\"page\":0", "\"page\":0,\"page\":0"),
        json.replace("\"page\":0", "\"page\":256"),
        json.replace("\"total\":1", "\"total\":0"),
        json.replace("\"total\":1", "\"total\":65"),
        json.replace("\"total\":1", "\"total\":1,\"path\":\"/tmp\""),
        json.replace("\"total\":1", "\"total\":2"),
        json.replace(&format!("\"issued_at\":{NOW}"), "\"issued_at\":0"),
    ] {
        assert_eq!(
            Record::decode(invalid.as_bytes()),
            Err(WireError::Invalid),
            "{invalid}"
        );
    }
}

#[test]
fn peer_heartbeat_core_permissions_stage_expiry_budget_and_clock() {
    let plan = PermissionPlan::new(nz(1), &multi_page()).unwrap();
    let pages = records(&plan, NOW);
    let now = clock(NOW, 2000);
    let mut receiver = Receiver::new(binding());
    assert_eq!(
        receiver.receive(binding(), NOW, &pages[0], now, false),
        Ok(Applied::Deferred)
    );
    assert_eq!(receiver.reserved_bytes(), 0);
    let last = pages.last().unwrap();
    assert_eq!(raw_put(&mut receiver, NOW, last, now), Applied::Pending);
    assert_eq!(receiver.reserved_bytes(), pages.len() * PAGE_BYTES);
    assert_eq!(receiver.permission(now), PermissionView::Unavailable);
    assert_eq!(
        raw_put(&mut receiver, NOW, last, clock(NOW + 1, 2001)),
        Applied::Ignored
    );
    receiver.sweep(clock(NOW + 899, 2899));
    assert!(receiver.reserved_bytes() > 0);
    receiver.sweep(clock(NOW + 899, 1999));
    assert!(receiver.reserved_bytes() > 0);
    assert_eq!(
        receiver.receive(binding(), NOW, &pages[0], clock(NOW + 900, 2900), true),
        Err(Refused::Expired)
    );
    assert!(
        receiver.reserved_bytes() > 0,
        "refused input does not mutate the stage"
    );
    receiver.sweep(clock(NOW + 900, 2900));
    assert_eq!(receiver.reserved_bytes(), 0);
    assert_eq!(
        raw_put(&mut receiver, NOW, last, clock(NOW + 899, 2900)),
        Applied::Ignored
    );

    let retry = records(&plan, NOW + 1);
    for (index, data) in retry.iter().rev().enumerate() {
        let result = raw_put(&mut receiver, NOW + 1, data, clock(NOW + 900, 2900));
        assert_eq!(
            result,
            if index + 1 == retry.len() {
                Applied::Permission
            } else {
                Applied::Pending
            }
        );
    }
    assert_eq!(receiver.reserved_bytes(), 0);
    assert!(
        matches!(receiver.permission(clock(NOW + 900, 2900)), PermissionView::Fresh(g, _) if g == plan.generation)
    );
    assert!(matches!(
        receiver.permission(clock(NOW + 899, 2900)),
        PermissionView::Cached(..)
    ));

    // Advancing monotonic time alone also makes a previously fresh heartbeat stale.
    let fresh = beat(1, NOW + 900, 300, Some(plan.generation));
    assert_eq!(
        send(&mut receiver, &fresh, clock(NOW + 900, 2900)),
        Ok(Applied::Heartbeat)
    );
    assert!(matches!(
        receiver.heartbeat(clock(NOW + 900, 2900 + 435)),
        HeartbeatView::Stale(Some(_))
    ));

    // Higher ordinal may have an earlier signed issue. Its fence belongs to that ordinal.
    let newer = PermissionPlan::new(nz(2), &multi_page()).unwrap();
    let higher_pages = records(&newer, NOW);
    assert_eq!(
        raw_put(&mut receiver, NOW, &higher_pages[0], clock(NOW + 899, 3000)),
        Applied::Pending
    );
    assert!(
        matches!(receiver.permission(clock(NOW + 900, 3000)), PermissionView::Cached(g, _) if g == plan.generation)
    );
}

#[test]
fn peer_heartbeat_core_conflicts_and_stale_heartbeats() {
    let desc = multi_page();
    let older = PermissionPlan::new(nz(1), &desc).unwrap();
    let newer = PermissionPlan::new(nz(2), &desc).unwrap();
    let mut receiver = Receiver::new(binding());
    let now = clock(NOW, 5000);
    for data in records(&older, NOW) {
        raw_put(&mut receiver, NOW, &data, now);
    }
    let staged = records(&newer, NOW);
    assert_eq!(
        raw_put(&mut receiver, NOW, &staged[0], now),
        Applied::Pending
    );
    let future = Generation {
        ordinal: nz(3),
        digest: Hex32([55; 32]),
    };
    assert_eq!(
        send(&mut receiver, &beat(10, NOW - 435, 300, Some(future)), now),
        Ok(Applied::Watermark)
    );
    assert_eq!(receiver.reserved_bytes(), staged.len() * PAGE_BYTES);
    assert_eq!(receiver.heartbeat(now), HeartbeatView::Stale(None));
    assert_eq!(
        send(&mut receiver, &beat(10, NOW, 300, Some(future)), now),
        Ok(Applied::Ignored)
    );

    let conflict = page(
        newer.generation,
        NOW,
        chunk(0, staged.len() as u8, vec![0; PAGE_BYTES]),
    );
    assert_eq!(send(&mut receiver, &conflict, now), Ok(Applied::Poisoned));
    assert_eq!(receiver.reserved_bytes(), 0);
    assert_eq!(receiver.permission(now), PermissionView::Unavailable);
    assert_eq!(
        raw_put(&mut receiver, NOW, &staged[0], now),
        Applied::Ignored
    );
    assert_eq!(
        send(&mut receiver, &beat(11, NOW, 300, Some(future)), now),
        Ok(Applied::Heartbeat)
    );
    assert_eq!(
        receiver.permission(now),
        PermissionView::Cached(older.generation, &desc)
    );
    assert_eq!(
        send(
            &mut receiver,
            &page(future, NOW, chunk(0, 1, vec![b'x'])),
            now
        ),
        Ok(Applied::Poisoned)
    );
    assert_eq!(receiver.permission(now), PermissionView::Unavailable);

    let canonical = desc.canonical_bytes().unwrap();
    let mut noncanonical = canonical.clone();
    noncanonical.push(b' ');
    assert_eq!(
        Description::decode_canonical(&noncanonical, description_digest(&noncanonical)),
        Err(WireError::Invalid)
    );

    // One valid, decoded total difference at the same issue is equivocation.
    let generation = Generation {
        ordinal: nz(5),
        digest: description_digest(&canonical),
    };
    let original = page(generation, NOW, chunk(0, 2, vec![b'x'; PAGE_BYTES]));
    let different = page(generation, NOW, chunk(1, 3, vec![b'x'; PAGE_BYTES]));
    assert_eq!(send(&mut receiver, &original, now), Ok(Applied::Pending));
    assert_eq!(send(&mut receiver, &different, now), Ok(Applied::Poisoned));

    // A different authenticated digest at equal ordinal fences a completed snapshot.
    for bytes in records(&PermissionPlan::new(nz(6), &desc).unwrap(), NOW) {
        raw_put(&mut receiver, NOW, &bytes, now);
    }
    assert!(matches!(
        receiver.permission(now),
        PermissionView::Fresh(..)
    ));
    let same_ordinal = Generation {
        ordinal: nz(6),
        digest: Hex32([55; 32]),
    };
    assert_eq!(
        send(&mut receiver, &beat(12, NOW, 300, Some(same_ordinal)), now),
        Ok(Applied::Heartbeat)
    );
    assert_eq!(receiver.permission(now), PermissionView::Unavailable);
    let higher = Generation {
        ordinal: nz(7),
        ..same_ordinal
    };
    assert_eq!(
        send(&mut receiver, &beat(13, NOW, 300, Some(higher)), now),
        Ok(Applied::Heartbeat)
    );
    assert_eq!(
        receiver.permission(now),
        PermissionView::Unavailable,
        "conflicted completion was evicted"
    );
}

#[test]
fn peer_heartbeat_core_reaffirmation_and_retention() {
    let description = multi_page();
    let plan = PermissionPlan::new(nz(1), &description).unwrap();
    let mut receiver = Receiver::new(binding());
    for data in records(&plan, NOW) {
        raw_put(&mut receiver, NOW, &data, clock(NOW, 0));
    }
    let stale = NOW + 3_900;
    assert_eq!(
        receiver.permission(clock(stale, 3900)),
        PermissionView::Cached(plan.generation, &description)
    );
    assert_eq!(
        send(
            &mut receiver,
            &beat(1, stale, 300, Some(plan.generation)),
            clock(stale, 3900)
        ),
        Ok(Applied::Heartbeat)
    );
    assert_eq!(receiver.reserved_bytes(), 0);
    assert_eq!(
        receiver.permission(clock(stale, 3900)),
        PermissionView::Fresh(plan.generation, &description)
    );
    let drop = clock(NOW + WEEK, WEEK);
    assert_eq!(
        send(
            &mut receiver,
            &beat(2, drop.wall - 1, 300, Some(plan.generation)),
            drop
        ),
        Ok(Applied::Heartbeat)
    );
    receiver.sweep(drop);
    assert_eq!(receiver.permission(drop), PermissionView::Unavailable);

    let reissue = records(&plan, drop.wall);
    assert_eq!(
        raw_put(&mut receiver, drop.wall, &reissue[0], drop),
        Applied::Pending
    );
    assert_eq!(receiver.permission(drop), PermissionView::Unavailable);
    for data in &reissue[1..] {
        raw_put(&mut receiver, drop.wall, data, drop);
    }
    assert_eq!(
        receiver.permission(drop),
        PermissionView::Fresh(plan.generation, &description)
    );
    let cached = clock(drop.wall + 3900, WEEK + 3900);
    assert_eq!(
        receiver.permission(cached),
        PermissionView::Cached(plan.generation, &description)
    );
    let second = records(&plan, cached.wall);
    let unused_reissue = records(&plan, cached.wall + 1);
    assert_eq!(
        raw_put(&mut receiver, cached.wall, &second[0], cached),
        Applied::Pending
    );
    assert_eq!(
        raw_put(&mut receiver, cached.wall + 1, &unused_reissue[1], cached),
        Applied::Ignored
    );
    assert_eq!(
        receiver.permission(cached),
        PermissionView::Cached(plan.generation, &description)
    );
    for data in &second[1..] {
        raw_put(&mut receiver, cached.wall, data, cached);
    }
    assert_eq!(
        receiver.permission(cached),
        PermissionView::Fresh(plan.generation, &description)
    );
    assert_eq!(
        raw_put(
            &mut receiver,
            cached.wall,
            &second[0],
            clock(cached.wall + 1, WEEK + 3901)
        ),
        Applied::Ignored
    );
    assert!(matches!(
        receiver.permission(clock(drop.wall + WEEK, 2 * WEEK)),
        PermissionView::Cached(..)
    ));
}

#[test]
fn peer_heartbeat_core_too_large_replay_retention_and_poison() {
    let description = multi_page();
    let original = PermissionPlan::new(nz(1), &description).unwrap();
    let generation = Generation {
        ordinal: nz(2),
        digest: Hex32([32; 32]),
    };
    let length = DESCRIPTION_LIMIT as u32 + 1;
    let over = |ordinal, now| {
        page(
            ordinal,
            now,
            PageContent::TooLarge {
                description_len: length,
            },
        )
    };
    let mut receiver = Receiver::new(binding());
    let now = clock(NOW, 500);
    for data in records(&original, NOW) {
        raw_put(&mut receiver, NOW, &data, now);
    }
    assert_eq!(
        send(
            &mut receiver,
            &over(generation, NOW),
            clock(NOW + 900, 1400)
        ),
        Err(Refused::Expired)
    );
    assert_eq!(
        receiver.permission(now),
        PermissionView::Fresh(original.generation, &description)
    );
    assert_eq!(
        send(&mut receiver, &over(generation, NOW), now),
        Ok(Applied::Permission)
    );
    assert_eq!(
        receiver.permission(now),
        PermissionView::TooLarge(generation, length)
    );
    assert_eq!(
        send(
            &mut receiver,
            &over(generation, NOW),
            clock(NOW + 800, 1300)
        ),
        Ok(Applied::Ignored)
    );
    assert_eq!(
        send(
            &mut receiver,
            &over(generation, NOW + 4),
            clock(NOW + 4, 504)
        ),
        Ok(Applied::Permission)
    );
    let seven_days = clock(NOW + WEEK, WEEK + 500);
    assert_eq!(
        receiver.permission(seven_days),
        PermissionView::TooLarge(generation, length)
    );
    assert_eq!(
        send(
            &mut receiver,
            &beat(1, seven_days.wall, 300, Some(generation)),
            seven_days
        ),
        Ok(Applied::Heartbeat)
    );
    let expired = clock(seven_days.wall + 4, WEEK + 504);
    receiver.sweep(expired);
    assert_eq!(receiver.permission(expired), PermissionView::Unavailable);
    assert_eq!(
        send(
            &mut receiver,
            &over(generation, NOW + 4),
            clock(NOW + 4, WEEK + 505)
        ),
        Ok(Applied::Ignored)
    );
    assert_eq!(
        send(&mut receiver, &over(generation, expired.wall), expired),
        Ok(Applied::Permission)
    );
    assert_eq!(
        send(
            &mut receiver,
            &page(generation, expired.wall, chunk(0, 1, vec![b'x'])),
            expired
        ),
        Ok(Applied::Poisoned)
    );
    assert_eq!(receiver.permission(expired), PermissionView::Unavailable);
    assert_eq!(
        send(&mut receiver, &over(generation, expired.wall), expired),
        Ok(Applied::Ignored)
    );
}

#[test]
fn peer_heartbeat_core_full_stage_suppresses_partial_content() {
    let description = sized_description(DESCRIPTION_LIMIT);
    let plan = PermissionPlan::new(nz(9), &description).unwrap();
    let pages = records(&plan, NOW);
    let mut receiver = Receiver::new(binding());
    let now = clock(NOW, 1);
    for index in (1..64).map(|item| (37 * item) % 64) {
        assert_eq!(
            raw_put(&mut receiver, NOW, &pages[index], now),
            Applied::Pending
        );
        assert_eq!(receiver.reserved_bytes(), DESCRIPTION_LIMIT);
        assert_eq!(receiver.permission(now), PermissionView::Unavailable);
    }
    assert_eq!(
        receiver.receive(binding(), NOW, &pages[0], now, false),
        Ok(Applied::Permission)
    );
    assert_eq!(
        receiver.permission(now),
        PermissionView::Fresh(plan.generation, &description)
    );
    assert_eq!(receiver.reserved_bytes(), 0);
}

#[test]
fn peer_heartbeat_core_terminal_page_and_completed_control_conflicts() {
    let description = multi_page();
    let plan = PermissionPlan::new(nz(1), &description).unwrap();
    let mut receiver = Receiver::new(binding());
    let now = clock(NOW, 1);
    for record in records(&plan, NOW) {
        raw_put(&mut receiver, NOW, &record, now);
    }
    let contradiction = page(
        plan.generation,
        NOW,
        PageContent::TooLarge {
            description_len: DESCRIPTION_LIMIT as u32 + 1,
        },
    );
    assert_eq!(
        send(&mut receiver, &contradiction, now),
        Ok(Applied::Poisoned)
    );
    assert_eq!(receiver.permission(now), PermissionView::Unavailable);

    let next = Generation {
        ordinal: nz(2),
        ..plan.generation
    };
    let end = page(next, NOW, chunk(1, 2, vec![0; 2]));
    let shorter = page(next, NOW, chunk(1, 2, vec![0; 1]));
    assert_eq!(send(&mut receiver, &end, now), Ok(Applied::Pending));
    assert_eq!(send(&mut receiver, &shorter, now), Ok(Applied::Poisoned));
    assert_eq!(receiver.reserved_bytes(), 0);
}

#[test]
fn peer_heartbeat_core_description_bounds_reject_overflowing_collections() {
    let layer = |entries: usize| serde_json::json!({"allow_exec": null, "deny_exec": vec!["/bin/**"; entries]});
    for layers in [
        vec![layer(MAX_PATTERNS + 1)],
        vec![layer(MAX_PATTERNS), layer(1)],
        vec![layer(1); MAX_LAYERS + 1],
    ] {
        let bytes = serde_json::to_vec(&serde_json::json!({
            "version":1, "layers": layers, "interactive":"granted", "shell_command":"denied"
        }))
        .unwrap();
        assert!(serde_json::from_slice::<Description>(&bytes).is_err());
    }
}

#[test]
#[ignore = "local codec benchmark, one synthetic workload at a time"]
fn peer_status_codec_benchmark() {
    fn run(name: &str, samples: usize, bytes: usize, mut operation: impl FnMut()) {
        let mut elapsed = Vec::with_capacity(samples);
        for _ in 0..samples {
            let start = Instant::now();
            operation();
            elapsed.push(start.elapsed());
        }
        elapsed.sort_unstable();
        let percentile =
            |percent| elapsed[(samples * percent as usize).div_ceil(100) - 1].as_secs_f64() * 1e6;
        let seconds: f64 = elapsed.iter().map(Duration::as_secs_f64).sum();
        eprintln!(
            "{name}: n={samples} bytes={bytes} p50={:.2}us p95={:.2}us p99={:.2}us ops/s={:.1} MiB/s={:.2}",
            percentile(50),
            percentile(95),
            percentile(99),
            samples as f64 / seconds,
            (samples * bytes) as f64 / (seconds * 1024.0 * 1024.0)
        );
    }

    let mut record = beat(1, NOW, 3600, None);
    let Payload::Heartbeat(sample) = &mut record.payload else {
        unreachable!();
    };
    sample.host = HostStatus {
        username: Some("benchmark-user".to_string()),
        uptime: Some(900_000),
        load: Some([250, 375, 600]),
        disks: Some(vec![DiskStatus {
            label: "benchmark".to_string(),
            used: 4096,
            available: 14_444,
        }]),
        ips: Some(vec![
            "192.0.2.3".parse().unwrap(),
            "2001:db8::f".parse().unwrap(),
        ]),
    };
    let heartbeat = record.encode().unwrap();
    run("heartbeat decode", 2000, heartbeat.len(), || {
        black_box(Record::decode(black_box(&heartbeat)).unwrap());
    });
    run("heartbeat receive", 2000, heartbeat.len(), || {
        let mut receiver = Receiver::new(binding());
        black_box(raw_put(
            &mut receiver,
            NOW,
            black_box(&heartbeat),
            clock(NOW, 0),
        ));
    });
    for (name, description, count, shuffle) in [
        ("permission ordered", multi_page(), 80, false),
        ("permission shuffled", multi_page(), 80, true),
        (
            "permission at cap shuffled",
            sized_description(DESCRIPTION_LIMIT),
            25,
            true,
        ),
    ] {
        let plan = PermissionPlan::new(nz(1), &description).unwrap();
        let mut pages = records(&plan, NOW);
        if shuffle {
            for index in (1..pages.len()).rev() {
                pages.swap(index, (1471 * index + 7) % (index + 1));
            }
        }
        let bytes = pages.iter().map(Vec::len).sum();
        run(name, count, bytes, || {
            let mut receiver = Receiver::new(binding());
            for data in &pages {
                black_box(raw_put(&mut receiver, NOW, black_box(data), clock(NOW, 0)));
            }
            assert!(matches!(
                receiver.permission(clock(NOW, 0)),
                PermissionView::Fresh(..)
            ));
        });
    }
}
