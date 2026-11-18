use super::*;
use crate::peer_status_wire::{DiskStatus, Payload};
use marmish_engine::{KeyRestrictions, Restrictions};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::time::Instant;

fn number(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

const ROOM: Hex32 = Hex32([1; 32]);
const SERVER: Hex32 = Hex32([2; 32]);
const CLIENT: Hex32 = Hex32([3; 32]);
const OTHER: Hex32 = Hex32([4; 32]);

fn binding() -> Binding {
    Binding::new(ROOM, SERVER, CLIENT, number(1)).unwrap()
}

fn proof(side: Direction, route: Route) -> Accepted {
    Accepted {
        binding: binding(),
        local: if side == Direction::Server {
            SERVER
        } else {
            CLIENT
        },
        side,
        route,
    }
}

fn view(settings: Option<&Rc<Settings>>, route: Route, revision: u64) -> View<'_> {
    View {
        accepted: proof(Direction::Server, route),
        execution_granted: true,
        revision: number(revision),
        settings,
    }
}

fn fields() -> Fields {
    Fields {
        base: BaseFields {
            username: true,
            uptime: false,
            load: true,
        },
        disks: vec![("allowed".into(), "/".into())],
        interfaces: vec![("loop".into(), Families::V4)],
    }
}

fn settings() -> Rc<Settings> {
    Settings::enable(binding(), None, fields(), None).unwrap()
}

fn clock(wall: u64, mono: u64) -> Clock {
    Clock {
        wall,
        monotonic: Duration::from_secs(mono),
    }
}

fn reserved(seq: u64) -> Reservation {
    Reservation {
        binding: binding(),
        sequence: number(seq),
    }
}

#[derive(Default)]
struct ScriptPort {
    starts: Vec<Duration>,
    replies: VecDeque<Progress>,
    next_start: Option<StartError>,
    polls: usize,
}

impl SamplerPort for ScriptPort {
    fn start(&mut self, _: Request, budget: Duration) -> Result<(), StartError> {
        self.starts.push(budget);
        self.next_start.take().map_or(Ok(()), Err)
    }

    fn poll(&mut self) -> Progress {
        self.polls += 1;
        self.replies.pop_front().unwrap_or(Progress::Pending)
    }
}

type Fixture = Planner<ScriptPort, Box<dyn FnMut() -> Option<Clock>>>;

fn fixture() -> (Rc<Cell<Option<Clock>>>, Fixture) {
    let time = Rc::new(Cell::new(Some(clock(100, 0))));
    let read = Rc::clone(&time);
    let source: Box<dyn FnMut() -> Option<Clock>> = Box::new(move || read.get());
    (
        time,
        Planner::new(binding(), ScriptPort::default(), source, None, None),
    )
}

fn move_to(time: &Cell<Option<Clock>>, mono: u64) {
    time.set(Some(clock(100 + mono, mono)));
}

fn present() -> HostStatus {
    HostStatus {
        username: Some("fixture".into()),
        uptime: Some(100),
        load: Some([1, 2, 3]),
        disks: Some(vec![
            DiskStatus {
                label: "allowed".into(),
                used: 1,
                available: 2,
            },
            DiskStatus {
                label: "discard".into(),
                used: 2,
                available: 1,
            },
        ]),
        ips: Some(vec![Ipv4Addr::LOCALHOST.into()]),
    }
}

fn complete(
    planner: &mut Fixture,
    active: Option<&Rc<Settings>>,
    route: Route,
    revision: u64,
    host: HostStatus,
) -> Ready {
    planner.port.replies.push_back(Progress::Ready(host));
    let Ok(Tick::Ready(pin)) = planner.tick(Some(view(active, route, revision))) else {
        panic!("sample did not become ready");
    };
    pin
}

fn decode(encoded: &Encoded) -> Heartbeat {
    let record = Record::decode(&encoded.bytes).unwrap();
    assert_eq!(record.room, encoded.room);
    let Payload::Heartbeat(heartbeat) = record.payload else {
        panic!("expected heartbeat")
    };
    assert_eq!(heartbeat.sampled_at, encoded.created_at);
    heartbeat
}

#[test]
fn planner_core_default_bounds_authority_and_address_gate() {
    let (_, mut planner) = fixture();
    let enabled = settings();
    assert_eq!(enabled.interval, 3600);
    for value in [299, 86_401] {
        assert!(matches!(
            Settings::enable(binding(), Some(value), fields(), None),
            Err(Denied::Settings)
        ));
    }
    for value in [300, 86_400] {
        let valid = Settings::enable(binding(), Some(value), fields(), None).unwrap();
        let delay = valid.next_delay().as_secs();
        assert!((u64::from(value)..=u64::from(value + (value / 20).min(300))).contains(&delay));
    }
    let mut bad = fields();
    bad.interfaces = vec![("".into(), Families::V4)];
    assert!(matches!(
        Settings::enable(binding(), None, bad, None),
        Err(Denied::Settings)
    ));
    assert!(enabled.fields.addresses(Route::Confidential).is_none());
    assert!(enabled.fields.addresses(Route::AddressSafe).is_some());
    assert!(matches!(planner.tick(None), Err(Denied::Authority)));
    assert!(matches!(
        planner.tick(Some(view(None, Route::Confidential, 1))),
        Err(Denied::Disabled)
    ));
    for accepted in [
        proof(Direction::Client, Route::Confidential),
        Accepted {
            local: OTHER,
            ..proof(Direction::Server, Route::Confidential)
        },
        Accepted {
            binding: Binding {
                generation: number(2),
                ..binding()
            },
            ..proof(Direction::Server, Route::AddressSafe)
        },
        proof(Direction::Server, Route::Unavailable),
    ] {
        assert!(matches!(
            planner.tick(Some(View {
                accepted,
                ..view(Some(&enabled), Route::Confidential, 1)
            })),
            Err(Denied::Authority)
        ));
    }
    assert!(matches!(
        planner.tick(Some(View {
            execution_granted: false,
            ..view(Some(&enabled), Route::Confidential, 1)
        })),
        Err(Denied::Authority)
    ));
    assert!(planner.port.starts.is_empty());

    assert!(matches!(
        planner.tick(Some(view(Some(&enabled), Route::Confidential, 1))),
        Err(Denied::Revision)
    ));
    let active = || Some(view(Some(&enabled), Route::Confidential, 2));
    assert!(matches!(planner.tick(active()), Ok(Tick::Started)));
    assert!(matches!(planner.tick(active()), Ok(Tick::Pending)));
    assert_eq!(planner.port.starts, [SAMPLE_WINDOW]);
    let pin = complete(
        &mut planner,
        Some(&enabled),
        Route::Confidential,
        2,
        present(),
    );
    let result = decode(&planner.release(active(), pin, reserved(1)).unwrap());
    assert!(result.host.ips.is_none());
    assert!(result.host.uptime.is_none());
    assert_eq!(result.host.disks.as_ref().unwrap().len(), 1);
    assert!(result.host.username.is_some() && result.host.load.is_some());
}

#[test]
fn planner_core_busy_cadence_overflow_and_clock_recovery() {
    let (time, mut planner) = fixture();
    let enabled = settings();
    let active = || Some(view(Some(&enabled), Route::Confidential, 2));
    planner.port.next_start = Some(StartError::Busy);
    assert!(matches!(planner.tick(active()), Ok(Tick::Busy)));
    assert_eq!(planner.port.starts.len(), 1);
    assert!(matches!(planner.tick(active()), Ok(Tick::Started)));
    assert!(matches!(planner.tick(active()), Ok(Tick::Pending)));
    move_to(&time, 1);
    assert!(matches!(planner.tick(active()), Ok(Tick::Pending)));
    time.set(None);
    assert!(matches!(planner.tick(active()), Err(Denied::Clock)));
    time.set(Some(clock(100, 1)));
    assert!(matches!(planner.tick(active()), Err(Denied::Clock)));
    move_to(&time, 2);
    planner.port.replies.push_back(Progress::Ready(present()));
    assert!(matches!(planner.tick(active()), Ok(Tick::Draining)));
    move_to(&time, 30);
    assert!(matches!(planner.tick(active()), Ok(Tick::Idle)));
    move_to(&time, 31);
    assert!(matches!(planner.tick(active()), Ok(Tick::Started)));
    let pin = complete(
        &mut planner,
        Some(&enabled),
        Route::Confidential,
        2,
        present(),
    );
    assert!(planner.release(active(), pin, reserved(1)).is_ok());
    let due = 31 + enabled.next_delay().as_secs();
    move_to(&time, due - 1);
    assert!(matches!(planner.tick(active()), Ok(Tick::Idle)));
    move_to(&time, due + 100_000);
    assert!(matches!(planner.tick(active()), Ok(Tick::Started)));
    let pin = complete(
        &mut planner,
        Some(&enabled),
        Route::Confidential,
        2,
        present(),
    );
    assert!(planner.release(active(), pin, reserved(2)).is_ok());
    assert!(matches!(planner.tick(active()), Ok(Tick::Idle)));

    let (time, mut fresh) = fixture();
    time.set(Some(Clock {
        wall: 1,
        monotonic: Duration::MAX,
    }));
    assert!(matches!(fresh.tick(active()), Err(Denied::Clock)));
    assert!(fresh.port.starts.is_empty());
}

#[test]
fn planner_core_budget_and_clock_change_during_release() {
    for (after_encoding, expected) in [(None, Denied::Clock), (Some(clock(102, 2)), Denied::Ticket)]
    {
        let readings = Rc::new(RefCell::new(VecDeque::from([
            Some(clock(100, 0)),
            Some(Clock {
                wall: 100,
                monotonic: Duration::from_millis(500),
            }),
            Some(clock(101, 1)),
            Some(clock(101, 1)),
            Some(clock(101, 1)),
            after_encoding,
        ])));
        let read = Rc::clone(&readings);
        let source: Box<dyn FnMut() -> Option<Clock>> =
            Box::new(move || read.borrow_mut().pop_front().unwrap());
        let mut planner = Planner::new(binding(), ScriptPort::default(), source, None, None);
        let active = settings();
        let current = || Some(view(Some(&active), Route::Confidential, 1));
        assert!(matches!(planner.tick(current()), Ok(Tick::Started)));
        assert_eq!(planner.port.starts, [Duration::from_millis(1500)]);
        let pin = complete(
            &mut planner,
            Some(&active),
            Route::Confidential,
            1,
            present(),
        );
        assert!(
            matches!(planner.release(current(), pin, reserved(1)), Err(error) if error == expected)
        );

        let retry = if expected == Denied::Clock { 31 } else { 32 };
        readings.borrow_mut().extend([
            Some(clock(100 + retry - 1, retry - 1)),
            Some(clock(100 + retry, retry)),
            Some(clock(100 + retry, retry)),
        ]);
        assert!(matches!(planner.tick(current()), Ok(Tick::Idle)));
        assert!(matches!(planner.tick(current()), Ok(Tick::Started)));
    }
}

#[test]
fn planner_core_epoch_drains_late_data_and_rejects_stale_revision() {
    let (time, mut planner) = fixture();
    let first = settings();
    let second = settings();
    let safe = || Some(view(Some(&first), Route::AddressSafe, 1));
    assert!(matches!(planner.tick(safe()), Ok(Tick::Started)));
    let epoch = planner.epoch;
    assert!(matches!(planner.tick(None), Err(Denied::Authority)));
    assert!(planner.epoch > epoch);
    planner.port.replies.push_back(Progress::Ready(present()));
    move_to(&time, 30);
    assert!(matches!(planner.tick(safe()), Ok(Tick::Draining)));
    assert!(planner.sample.is_none());
    assert!(matches!(planner.tick(safe()), Ok(Tick::Started)));
    let safe_pin = complete(&mut planner, Some(&first), Route::AddressSafe, 1, present());
    assert!(matches!(planner.tick(None), Err(Denied::Authority)));
    assert!(matches!(
        planner.release(safe(), safe_pin, reserved(1)),
        Err(Denied::Ticket)
    ));

    let withheld = || Some(view(Some(&second), Route::Confidential, 2));
    assert!(matches!(planner.tick(withheld()), Ok(Tick::Idle)));
    assert!(matches!(planner.tick(safe()), Err(Denied::Revision)));
    move_to(&time, 60);
    assert!(matches!(planner.tick(withheld()), Ok(Tick::Started)));
    let pin = complete(
        &mut planner,
        Some(&second),
        Route::Confidential,
        2,
        present(),
    );
    assert!(matches!(
        planner.release(withheld(), pin, reserved(1)),
        Err(Denied::Sequence)
    ));
    move_to(&time, 62);
    assert!(matches!(planner.tick(withheld()), Ok(Tick::Idle)));
    move_to(&time, 92);
    assert!(matches!(planner.tick(withheld()), Ok(Tick::Started)));
    let pin = complete(
        &mut planner,
        Some(&second),
        Route::Confidential,
        2,
        present(),
    );
    let result = decode(&planner.release(withheld(), pin, reserved(2)).unwrap());
    assert!(result.host.ips.is_none());
}

#[test]
fn planner_core_release_pin_sequence_and_deadline() {
    let (time, mut planner) = fixture();
    let (_, mut other) = fixture();
    let enabled = settings();
    let active = || Some(view(Some(&enabled), Route::AddressSafe, 1));
    assert!(matches!(planner.tick(active()), Ok(Tick::Started)));
    assert!(matches!(other.tick(active()), Ok(Tick::Started)));
    let pin = complete(
        &mut planner,
        Some(&enabled),
        Route::AddressSafe,
        1,
        present(),
    );
    let foreign = complete(&mut other, Some(&enabled), Route::AddressSafe, 1, present());
    assert!(matches!(
        planner.release(active(), foreign, reserved(1)),
        Err(Denied::Ticket)
    ));
    let result = decode(&planner.release(active(), pin, reserved(2)).unwrap());
    assert_eq!(
        result.host.ips.as_deref(),
        Some([Ipv4Addr::LOCALHOST.into()].as_slice())
    );

    let changed = settings();
    move_to(&time, 30);
    let new = || Some(view(Some(&changed), Route::Confidential, 2));
    assert!(matches!(planner.tick(new()), Ok(Tick::Started)));
    let pin = complete(
        &mut planner,
        Some(&changed),
        Route::Confidential,
        2,
        present(),
    );
    move_to(&time, 32);
    assert!(matches!(
        planner.release(new(), pin, reserved(3)),
        Err(Denied::Ticket)
    ));
    move_to(&time, 62);
    assert!(matches!(planner.tick(new()), Ok(Tick::Started)));
    let pin = complete(
        &mut planner,
        Some(&changed),
        Route::Confidential,
        2,
        present(),
    );
    let impossible = Reservation {
        binding: Binding {
            room: OTHER,
            ..binding()
        },
        sequence: number(1000),
    };
    assert!(matches!(
        planner.release(new(), pin, impossible),
        Err(Denied::Sequence)
    ));
    assert_eq!(planner.sequence, Some(number(3)));
}

fn policy(defaults: Restrictions, keyed: Restrictions) -> CommandPolicy {
    let result = CommandPolicy {
        version: 1,
        defaults,
        keys: vec![
            KeyRestrictions {
                principal: PrincipalId(CLIENT.0),
                restrictions: keyed,
            },
            KeyRestrictions {
                principal: PrincipalId(OTHER.0),
                restrictions: Restrictions {
                    allow_exec: Some(vec!["/elsewhere/**".into()]),
                    deny_exec: vec![],
                },
            },
        ],
    };
    result
        .validate_for_principals(&[PrincipalId(CLIENT.0), PrincipalId(OTHER.0)])
        .unwrap();
    result
}

fn policy_settings(
    command: &CommandPolicy,
    ordinal: u64,
    interactive: Capability,
    shell_command: Capability,
) -> Rc<Settings> {
    Settings::enable(
        binding(),
        None,
        fields(),
        Some(Policy {
            command,
            ordinal: number(ordinal),
            interactive,
            shell_command,
        }),
    )
    .unwrap()
}

fn authenticated(at: u64) -> Authenticated {
    Authenticated {
        author: SERVER,
        leaf: SERVER,
        current_epoch: true,
        created_at: at,
    }
}

#[test]
fn planner_core_policy_engine_parity_and_capabilities() {
    let none = Restrictions::default();
    let restricted = Restrictions {
        allow_exec: None,
        deny_exec: vec!["/private/**".into()],
    };
    let empty = Restrictions {
        allow_exec: Some(vec![]),
        deny_exec: vec![],
    };
    let allow = Restrictions {
        allow_exec: Some(vec![r"/opt/***/a/\*".into()]),
        deny_exec: vec!["/opt/blocked/*".into()],
    };
    for (defaults, keyed, count, interactive, shell_command) in [
        (
            restricted.clone(),
            none.clone(),
            1,
            Capability::Unsupported,
            Capability::Denied,
        ),
        (none, empty, 1, Capability::Granted, Capability::Unsupported),
        (
            restricted,
            allow,
            2,
            Capability::Denied,
            Capability::Granted,
        ),
    ] {
        let (_, mut planner) = fixture();
        let config = policy(defaults, keyed);
        let effective = config.describe_for_principal(PrincipalId(CLIENT.0));
        assert_eq!(effective.layers.len(), count);
        let expected = Description::from_engine(effective, interactive, shell_command).unwrap();
        let enabled = policy_settings(&config, 2, interactive, shell_command);
        let active = || Some(view(Some(&enabled), Route::Confidential, 1));
        let mut projection = Projection::new(binding());
        let client = Some(proof(Direction::Client, Route::Confidential));
        assert_eq!(planner.start_issue(active()), Ok(true));
        let issue = planner.next_issue(active()).unwrap().unwrap();
        assert_eq!(
            projection.receive(
                client,
                authenticated(issue.created_at),
                &issue.bytes,
                clock(100, 0),
                true
            ),
            Ok(Applied::Permission)
        );
        assert!(planner.next_issue(active()).unwrap().is_none());
        let (HeartbeatView::Unknown, PermissionView::Fresh(current, observed)) =
            projection.view(client, clock(100, 0))
        else {
            panic!("permissions should be verified");
        };
        assert_eq!(observed, &expected);
        assert_eq!(current.ordinal, number(2));
        assert!(matches!(planner.tick(active()), Ok(Tick::Started)));
        let pin = complete(
            &mut planner,
            Some(&enabled),
            Route::Confidential,
            1,
            HostStatus::default(),
        );
        assert_eq!(
            decode(&planner.release(active(), pin, reserved(1)).unwrap()).permission,
            Some(current)
        );
    }
}

#[test]
fn planner_core_permission_issue_fence_and_page_invalidation() {
    let (time, mut planner) = fixture();
    let baseline = policy(
        Restrictions::default(),
        Restrictions {
            allow_exec: Some(
                (0..12)
                    .map(|i| format!("/catalog/{i}/{}", "x".repeat(1000)))
                    .collect(),
            ),
            deny_exec: vec![],
        },
    );
    let changed = policy(
        Restrictions::default(),
        Restrictions {
            allow_exec: Some(vec![]),
            deny_exec: vec![],
        },
    );
    let first = policy_settings(&baseline, 5, Capability::Denied, Capability::Unsupported);
    let same_ordinal = policy_settings(&changed, 5, Capability::Denied, Capability::Unsupported);
    let lower = policy_settings(&changed, 4, Capability::Denied, Capability::Unsupported);
    let second = policy_settings(&changed, 6, Capability::Denied, Capability::Unsupported);
    let active = || Some(view(Some(&first), Route::Confidential, 1));
    let client = Some(proof(Direction::Client, Route::Confidential));
    let mut projection = Projection::new(binding());
    assert_eq!(planner.start_issue(active()), Ok(true));
    let issue = planner.next_issue(active()).unwrap().unwrap();
    assert_eq!(
        projection.receive(
            client,
            authenticated(issue.created_at),
            &issue.bytes,
            clock(100, 0),
            true
        ),
        Ok(Applied::Pending)
    );
    assert!(matches!(
        planner.next_issue(Some(view(Some(&second), Route::Confidential, 1))),
        Err(Denied::Revision)
    ));
    assert!(planner.next_issue(active()).unwrap().is_none());

    time.set(Some(clock(100, 30)));
    assert_eq!(planner.start_issue(active()), Err(Denied::Policy));
    move_to(&time, 31);
    assert_eq!(planner.start_issue(active()), Ok(true));
    let reissue = planner.next_issue(active()).unwrap().unwrap();
    assert_eq!(
        projection.receive(
            client,
            authenticated(reissue.created_at),
            &reissue.bytes,
            clock(131, 31),
            true
        ),
        Ok(Applied::Ignored)
    );
    move_to(&time, 91);
    assert!(matches!(planner.next_issue(active()), Err(Denied::Ticket)));
    move_to(&time, 901);
    assert_eq!(planner.start_issue(active()), Ok(true));
    while let Some(page) = planner.next_issue(active()).unwrap() {
        let applied = projection
            .receive(
                client,
                authenticated(page.created_at),
                &page.bytes,
                clock(1001, 901),
                true,
            )
            .unwrap();
        assert!(matches!(applied, Applied::Pending | Applied::Permission));
    }
    assert!(matches!(
        projection.view(client, clock(1001, 901)).1,
        PermissionView::Fresh(_, _)
    ));

    assert_eq!(
        planner.start_issue(Some(view(Some(&same_ordinal), Route::Confidential, 2))),
        Err(Denied::Policy)
    );
    assert_eq!(
        planner.start_issue(Some(view(Some(&lower), Route::Confidential, 3))),
        Err(Denied::Policy)
    );
    move_to(&time, 931);
    assert_eq!(
        planner.start_issue(Some(view(Some(&second), Route::Confidential, 4))),
        Ok(true)
    );
    assert!(
        planner
            .next_issue(Some(view(Some(&second), Route::Confidential, 4)))
            .unwrap()
            .is_some()
    );
}

#[test]
fn planner_core_projection_duplicate_refusal_retirement_and_replacement() {
    let (_, mut planner) = fixture();
    let enabled = settings();
    let active = || Some(view(Some(&enabled), Route::Confidential, 1));
    assert!(matches!(planner.tick(active()), Ok(Tick::Started)));
    let pin = complete(
        &mut planner,
        Some(&enabled),
        Route::Confidential,
        1,
        present(),
    );
    let heartbeat = planner.release(active(), pin, reserved(1)).unwrap();
    let mut projection = Projection::new(binding());
    let c = proof(Direction::Client, Route::Confidential);
    let now = clock(101, 1);
    fn receive(
        projection: &mut Projection,
        proof: Accepted,
        event: Authenticated,
        sample: &Encoded,
        now: Clock,
    ) -> Result<Applied, Refused> {
        projection.receive(Some(proof), event, &sample.bytes, now, true)
    }
    assert_eq!(
        receive(
            &mut projection,
            c,
            authenticated(heartbeat.created_at),
            &heartbeat,
            now
        ),
        Ok(Applied::Heartbeat)
    );
    for event in [
        Authenticated {
            author: OTHER,
            ..authenticated(100)
        },
        Authenticated {
            leaf: OTHER,
            ..authenticated(100)
        },
        Authenticated {
            current_epoch: false,
            ..authenticated(100)
        },
    ] {
        assert_eq!(
            receive(&mut projection, c, event, &heartbeat, now),
            Err(Refused::Binding)
        );
    }
    for wrong in [
        proof(Direction::Server, Route::Confidential),
        proof(Direction::Client, Route::Unavailable),
        Accepted {
            binding: Binding {
                room: OTHER,
                ..binding()
            },
            ..c
        },
        Accepted {
            binding: Binding {
                generation: number(2),
                ..binding()
            },
            ..c
        },
    ] {
        assert_eq!(
            receive(&mut projection, wrong, authenticated(100), &heartbeat, now),
            Err(Refused::Binding)
        );
        assert!(matches!(
            projection.view(Some(wrong), now).0,
            HeartbeatView::Unknown
        ));
    }
    let later = clock(131, 31);
    assert_eq!(
        receive(&mut projection, c, authenticated(100), &heartbeat, later),
        Ok(Applied::Ignored)
    );
    let (HeartbeatView::Fresh(observed), _) = projection.view(Some(c), later) else {
        panic!("expected verified sample")
    };
    assert_eq!(observed.received_at, 101);
    assert_eq!(observed.age, Duration::from_secs(31));
    assert!(matches!(
        projection.view(None, later).0,
        HeartbeatView::Unknown
    ));
    assert!(matches!(
        projection.view(Some(c), later).0,
        HeartbeatView::Fresh(_)
    ));
    assert!(matches!(
        projection.view(Some(c), clock(5000, 4900)).0,
        HeartbeatView::Stale(Some(_))
    ));
    projection.retire();
    assert!(matches!(
        projection.view(Some(c), later).0,
        HeartbeatView::Unknown
    ));
    assert_eq!(
        receive(&mut projection, c, authenticated(100), &heartbeat, later),
        Err(Refused::Binding)
    );
    let replacement = Binding {
        generation: number(2),
        room: OTHER,
        ..binding()
    };
    let mut replacement_view = Projection::new(replacement);
    let new_proof = Accepted {
        binding: replacement,
        ..c
    };
    assert_eq!(
        receive(
            &mut replacement_view,
            new_proof,
            authenticated(100),
            &heartbeat,
            later
        ),
        Err(Refused::Binding)
    );
    assert!(matches!(
        replacement_view.view(Some(new_proof), later).0,
        HeartbeatView::Unknown
    ));
}

#[test]
#[ignore = "opt-in native heartbeat planner, sampler and projection latency"]
fn planner_native_roundtrip_benchmark() {
    let directory = tempfile::tempdir().unwrap();
    let enabled = Settings::enable(
        binding(),
        None,
        Fields {
            base: BaseFields {
                username: true,
                uptime: true,
                load: true,
            },
            disks: vec![("fixture".into(), directory.path().into())],
            interfaces: vec![(
                if cfg!(target_os = "macos") {
                    "lo0"
                } else {
                    "lo"
                }
                .into(),
                Families::V4,
            )],
        },
        None,
    )
    .unwrap();
    let offset = Cell::new(0u64);
    let adjusted = || {
        let real = native_time()?;
        let step = offset.get();
        Some(Clock {
            wall: real.wall.checked_add(step)?,
            monotonic: real.monotonic.checked_add(Duration::from_secs(step))?,
        })
    };
    let mut planner = Planner::new(binding(), Sampler::spawn().unwrap(), adjusted, None, None);
    let active = || Some(view(Some(&enabled), Route::AddressSafe, 1));
    let incoming = Some(proof(Direction::Client, Route::AddressSafe));
    let mut client = Projection::new(binding());
    let mut samples = Vec::new();
    for run in 0..=200 {
        offset.set(run * 7200);
        let start = Instant::now();
        assert!(matches!(planner.tick(active()), Ok(Tick::Started)));
        let submitted = Instant::now();
        let pin = loop {
            match planner.tick(active()).unwrap() {
                Tick::Ready(pin) => break pin,
                Tick::Pending => {
                    assert!(submitted.elapsed() < SAMPLE_WINDOW);
                    std::thread::yield_now();
                }
                _ => panic!("native planner did not complete"),
            }
        };
        let sampled = Instant::now();
        let message = planner.release(active(), pin, reserved(run + 1)).unwrap();
        let encoded = Instant::now();
        let received = adjusted().unwrap();
        assert_eq!(
            client.receive(
                incoming,
                authenticated(message.created_at),
                &message.bytes,
                received,
                true
            ),
            Ok(Applied::Heartbeat)
        );
        let done = Instant::now();
        let (HeartbeatView::Fresh(found), _) = client.view(incoming, received) else {
            panic!("missing native heartbeat");
        };
        if run == 0 {
            assert!(
                found.sample.host.username.is_some()
                    && found.sample.host.uptime.is_some()
                    && found.sample.host.load.is_some()
            );
            assert!(
                found
                    .sample
                    .host
                    .disks
                    .as_ref()
                    .is_some_and(|disks| disks.len() == 1)
            );
            assert!(
                found.sample.host.ips.as_deref() == Some([Ipv4Addr::LOCALHOST.into()].as_slice())
            );
        } else {
            samples.push([
                done - start,
                (submitted - start) + (encoded - sampled),
                sampled - submitted,
                done - encoded,
            ]);
        }
    }
    println!("native heartbeat planner samples={}", samples.len());
    for (column, stage) in ["total", "planner_encode", "native_worker", "projection"]
        .into_iter()
        .enumerate()
    {
        let mut values: Vec<_> = samples.iter().map(|sample| sample[column]).collect();
        values.sort_unstable();
        let percentile = |pct: usize| values[(pct * values.len()).div_ceil(100) - 1].as_micros();
        println!(
            "{stage} p50_us={} p95_us={} p99_us={} max_us={}",
            percentile(50),
            percentile(95),
            percentile(99),
            percentile(100)
        );
    }
}
