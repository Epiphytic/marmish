use super::*;
use crate::ipc::peer_read::State as PeerState;
use nostr::key::Keys;

const LIST: Request = Request::List {};
const FLOOR: u64 = 100;
const EXPIRES: u64 = 150;

fn generation() -> NonZeroU64 {
    NonZeroU64::MIN
}

fn entry() -> Entry {
    Entry {
        selection: Selection {
            server: Keys::generate().public_key(),
            offer_id: [9; 16],
            revision: [2; 32],
            generation: 4,
        },
        state: PeerState::Pending,
        held_at: FLOOR,
        expires_at: EXPIRES,
    }
}

fn origin(monotonic: Instant) -> ClockOrigin {
    ClockOrigin {
        monotonic,
        continuous: Duration::from_secs(10),
    }
}

fn sample(monotonic: Instant, wall: u64) -> impl FnOnce() -> Option<Sample> + Copy {
    at(origin(monotonic), wall)
}

fn at(time: ClockOrigin, wall: u64) -> impl FnOnce() -> Option<Sample> + Copy {
    move || Some(Sample { time, wall })
}

fn observation(entry: Option<Entry>) -> Observation {
    Observation {
        projection: entry.map_or(Projection::SyntheticEmpty, Projection::Pending),
        wall_floor: FLOOR,
        verified_wall: FLOOR,
    }
}

fn publish(producer: &Producer, time: Instant, entry: Option<Entry>) {
    assert!(producer.publish(
        producer.begin(false, || Some(origin(time))).unwrap(),
        observation(entry),
        sample(time, FLOOR)
    ));
}

fn query(
    reader: &Reader,
    request: Request,
    sample: impl FnOnce() -> Option<Sample> + Copy,
) -> Option<Reply> {
    let read = reader.admit(generation())?;
    let selected = read.select(request, sample)?;
    read.reply(selected, sample)
}

pub(in crate::ipc) fn publish_live_fixture(producer: &Producer) -> Entry {
    let ticket = producer.read_ticket().unwrap();
    let observed = Sample::now().unwrap();
    let row = Entry {
        held_at: observed.wall,
        expires_at: observed.wall + 30,
        ..entry()
    };
    assert!(producer.publish(
        ticket,
        Observation {
            projection: Projection::Pending(row),
            wall_floor: observed.wall,
            verified_wall: observed.wall,
        },
        Sample::now,
    ));
    row
}

#[test]
fn admitted_reply_uses_real_clocks_and_rechecks_listener_and_deadline() {
    let listener = generation();
    let foreign_listener = NonZeroU64::new(2).unwrap();
    let (producer, reader) = pair(listener).unwrap();
    let read = reader.admit(listener).unwrap();
    let deadline = ExchangeInstant::now() + Duration::from_secs(5);
    assert_eq!(read.reply_before_write(LIST, listener, deadline), None);

    let row = publish_live_fixture(&producer);
    assert_eq!(
        read.reply_before_write(LIST, foreign_listener, deadline),
        None
    );
    assert_eq!(
        read.reply_before_write(LIST, listener, ExchangeInstant::now()),
        None
    );
    assert_eq!(
        read.reply_before_write(LIST, listener, deadline),
        Some(Reply::List { entries: vec![row] })
    );
    reader.stop();
    assert_eq!(read.reply_before_write(LIST, listener, deadline), None);
    assert!(reader.admit(listener).is_none());
}

#[test]
fn only_a_qualified_observation_lists_or_shows() {
    let time = Instant::now();
    let current = sample(time, FLOOR);
    let (producer, reader) = pair(generation()).unwrap();
    assert_eq!(query(&reader, LIST, current), None);

    let row = entry();
    let selection = row.selection;
    publish(&producer, time, Some(row));
    assert_eq!(
        query(&reader, LIST, current),
        Some(Reply::List { entries: vec![row] })
    );
    let show = Request::Show { selection };
    assert_eq!(
        query(&reader, show, current),
        Some(Reply::Show { entry: row })
    );
    let other_server = Keys::generate().public_key();
    for selection in [
        Selection {
            server: other_server,
            ..selection
        },
        Selection {
            offer_id: [8; 16],
            ..selection
        },
        Selection {
            revision: [3; 32],
            ..selection
        },
        Selection {
            generation: 5,
            ..selection
        },
    ] {
        assert_eq!(query(&reader, Request::Show { selection }, current), None);
    }

    publish(&producer, time, None);
    assert_eq!(
        query(&reader, LIST, current),
        Some(Reply::List {
            entries: Vec::new()
        })
    );
    assert_eq!(query(&reader, show, current), None);
}

#[test]
fn admission_rechecks_declared_generation_age_and_wall_floor() {
    let time = Instant::now();
    let (producer, reader) = pair(generation()).unwrap();
    assert!(reader.admit(NonZeroU64::new(2).unwrap()).is_none());
    let read = reader.admit(generation()).unwrap();
    let row = entry();
    let ticket = producer.begin(false, || Some(origin(time))).unwrap();
    assert!(!producer.publish(
        ticket,
        Observation {
            verified_wall: FLOOR - 1,
            ..observation(Some(row))
        },
        sample(time, FLOOR)
    ));
    publish(&producer, time, Some(row));
    let origin = read.select(LIST, sample(time, FLOOR)).unwrap();
    assert!(read.reply(origin, sample(time + LIFETIME, FLOOR)).is_none());
    assert!(read.select(LIST, sample(time, FLOOR)).is_none());

    publish(&producer, time, Some(row));
    let origin = read.select(LIST, sample(time, FLOOR)).unwrap();
    assert!(read.reply(origin, sample(time, FLOOR - 1)).is_none());
    assert!(read.select(LIST, sample(time, FLOOR)).is_none());
}

#[test]
fn both_clock_samples_apply_at_their_exact_admission_boundaries() {
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let sampled = Sample::now().unwrap();
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!((before..=after).contains(&sampled.wall));

    let time = Instant::now();
    let (producer, reader) = pair(generation()).unwrap();
    let row = entry();
    let ticket = producer.begin(false, || Some(origin(time))).unwrap();
    assert!(!producer.publish(ticket, observation(Some(row)), || None));
    publish(&producer, time, Some(row));
    let read = reader.admit(generation()).unwrap();
    assert!(read.select(LIST, || None).is_none());
    assert!(read.select(LIST, sample(time, FLOOR)).is_none());
    publish(&producer, time, Some(row));
    let selected = read.select(LIST, sample(time, FLOOR)).unwrap();
    assert!(read.reply(selected, || None).is_none());
    assert!(read.select(LIST, sample(time, FLOOR)).is_none());
    publish(&producer, time, Some(row));

    let selected = read.select(LIST, sample(time, FLOOR)).unwrap();
    let almost_expired = time + LIFETIME - Duration::from_nanos(1);
    assert!(
        read.reply(selected, sample(almost_expired, FLOOR))
            .is_some()
    );
    assert!(
        read.select(LIST, sample(time - Duration::from_nanos(1), FLOOR))
            .is_none()
    );
}

#[test]
fn only_the_current_producer_can_latch_an_expired_record() {
    let time = Instant::now();
    let (producer, reader) = pair(generation()).unwrap();
    let (other, _) = pair(generation()).unwrap();
    let row = entry();
    let foreign = other.begin(false, || Some(origin(time))).unwrap();
    assert!(!producer.publish(foreign, observation(Some(row)), sample(time, EXPIRES)));
    publish(&producer, time, Some(row));

    let fresh = producer.begin(false, || Some(origin(time))).unwrap();
    assert!(!producer.publish(fresh, observation(Some(row)), sample(time, EXPIRES)));
    let again = producer.begin(true, || Some(origin(time))).unwrap();
    assert!(!producer.publish(again, observation(Some(row)), sample(time, EXPIRES - 1)));
    assert!(query(&reader, LIST, sample(time, EXPIRES - 1)).is_none());
}

#[test]
fn expiry_is_exact_and_latched_until_the_actor_stops() {
    let time = Instant::now();
    let (producer, reader) = pair(generation()).unwrap();
    let row = entry();
    let show = Request::Show {
        selection: row.selection,
    };
    publish(&producer, time, Some(row));
    let read = reader.admit(generation()).unwrap();
    let selected = read.select(show, sample(time, EXPIRES - 1)).unwrap();
    assert!(read.reply(selected, sample(time, EXPIRES)).is_none());
    assert_eq!(
        query(&reader, LIST, sample(time, EXPIRES)),
        Some(Reply::List {
            entries: Vec::new()
        })
    );
    assert_eq!(query(&reader, show, sample(time, EXPIRES - 1)), None);

    let new_time = time + LIFETIME;
    let ticket = producer.begin(true, || Some(origin(new_time))).unwrap();
    assert!(!producer.publish(
        ticket,
        observation(Some(row)),
        sample(new_time, EXPIRES - 1)
    ));
    assert_eq!(query(&reader, LIST, sample(new_time, EXPIRES)), None);

    drop(producer);
    let (producer, replacement) = pair(generation()).unwrap();
    publish(&producer, new_time, Some(row));
    assert_eq!(
        query(&replacement, show, sample(new_time, FLOOR)),
        Some(Reply::Show { entry: row })
    );
}

#[test]
fn a_second_invalidation_or_changed_owner_rejects_old_tickets_and_replies() {
    let time = Instant::now();
    let current = sample(time, FLOOR);
    let (producer, reader) = pair(generation()).unwrap();
    let (other, other_reader) = pair(generation()).unwrap();
    let first = producer.begin(true, || Some(origin(time))).unwrap();
    assert!(producer.publish(first, observation(Some(entry())), current));

    let other_turn = other.begin(true, || Some(origin(time))).unwrap();
    assert!(other.publish(other_turn, observation(Some(entry())), current));
    let read = reader.admit(generation()).unwrap();
    let old_reply = read.select(LIST, current).unwrap();
    let alternate = other_reader.admit(generation()).unwrap();
    let alternate_reply = alternate.select(LIST, current).unwrap();
    assert_eq!(old_reply.stamp.sequence, alternate_reply.stamp.sequence);
    assert!(alternate.reply(old_reply, current).is_none());
    let foreign = producer.begin(false, || Some(origin(time))).unwrap();
    assert_eq!(foreign.stamp.sequence, alternate_reply.stamp.sequence);
    assert!(!other.publish(foreign, observation(Some(entry())), current));

    let old_reply = read.select(LIST, current).unwrap();
    let first = producer.begin(true, || Some(origin(time))).unwrap();
    let second = producer.begin(true, || Some(origin(time))).unwrap();
    assert!(!producer.publish(first, observation(Some(entry())), current));
    assert!(producer.publish(second, observation(Some(entry())), current));
    assert!(read.reply(old_reply, current).is_none());

    let ticket = producer.begin(false, || Some(origin(time))).unwrap();
    reader.stop();
    assert!(!producer.publish(ticket, observation(Some(entry())), current));
    assert!(reader.admit(generation()).is_none());
    assert!(producer.begin(false, || Some(origin(time))).is_none());
}

#[test]
fn read_budget_and_failed_owners_release_or_refuse_immediately() {
    let time = Instant::now();
    let current = sample(time, FLOOR);
    let (producer, reader) = pair(generation()).unwrap();
    let handles = (0..READ_LIMIT)
        .map(|_| reader.admit(generation()).unwrap())
        .collect::<Vec<_>>();
    assert!(reader.admit(generation()).is_none());
    drop(handles);
    let read = reader.admit(generation()).unwrap();
    publish(&producer, time, Some(entry()));
    let selected = read.select(LIST, current).unwrap();
    drop(producer);
    assert!(read.reply(selected, current).is_none());

    let (producer, reader) = pair(generation()).unwrap();
    let read = reader.admit(generation()).unwrap();
    publish(&producer, time, Some(entry()));
    let inside = reader.clone();
    let poison = std::thread::spawn(move || {
        let _guard = inside.0.view.lock().unwrap();
        panic!("poison candidate view");
    });
    assert!(poison.join().is_err());
    assert!(read.select(LIST, current).is_none());
    assert!(reader.admit(generation()).is_none());
    assert!(producer.begin(false, || Some(origin(time))).is_none());
}

#[test]
fn suspend_clock_and_verified_wall_independently_bound_the_proof() {
    let time = Instant::now();
    let initial = origin(time);
    let (producer, reader) = pair(generation()).unwrap();
    let row = entry();
    let read = reader.admit(generation()).unwrap();

    let early = ClockOrigin {
        continuous: initial.continuous + LIFETIME - Duration::from_nanos(1),
        ..initial
    };
    let stale = ClockOrigin {
        continuous: initial.continuous + LIFETIME,
        ..initial
    };
    let backward = ClockOrigin {
        continuous: initial.continuous - Duration::from_nanos(1),
        ..initial
    };
    let ticket = producer.begin(false, || Some(initial)).unwrap();
    assert!(!producer.publish(ticket, observation(Some(row)), at(stale, FLOOR)));
    publish(&producer, time, Some(row));
    let selected = read.select(LIST, at(early, FLOOR)).unwrap();
    assert!(read.reply(selected, at(stale, FLOOR)).is_none());
    publish(&producer, time, Some(row));
    assert!(read.select(LIST, at(backward, FLOOR)).is_none());

    let ticket = producer.begin(false, || Some(initial)).unwrap();
    assert!(producer.publish(
        ticket,
        Observation {
            verified_wall: FLOOR + 2,
            ..observation(Some(row))
        },
        at(initial, FLOOR + 2),
    ));
    let selected = read.select(LIST, at(initial, FLOOR + 2)).unwrap();
    assert!(read.reply(selected, at(initial, FLOOR + 1)).is_none());
}

#[test]
fn refresh_retains_prior_until_exact_refusal_or_replacement() {
    let time = Instant::now();
    let current = sample(time, FLOOR);
    let (producer, reader) = pair(generation()).unwrap();
    let (foreign, _) = pair(generation()).unwrap();
    let row = entry();
    publish(&producer, time, Some(row));
    let stale = producer.begin(false, || Some(origin(time))).unwrap();
    assert_eq!(
        query(&reader, LIST, current),
        Some(Reply::List { entries: vec![row] })
    );

    publish(&producer, time, Some(row));
    producer.refuse(stale);
    producer.refuse(foreign.begin(false, || Some(origin(time))).unwrap());
    assert_eq!(
        query(&reader, LIST, current),
        Some(Reply::List { entries: vec![row] })
    );

    let exact = producer.begin(false, || Some(origin(time))).unwrap();
    producer.refuse(exact);
    assert!(query(&reader, LIST, current).is_none());

    publish(&producer, time, Some(row));
    assert!(producer.begin(false, || None).is_none());
    assert!(query(&reader, LIST, current).is_none());
}

#[cfg(marmish_peer_receiver_candidate)]
#[test]
fn refresh_timing_uses_actual_admission_and_reports_both_prior_deadlines() {
    use crate::diagnostics::pending_refresh::Clocks;

    let (producer, _) = pair(generation()).unwrap();
    let start = origin(Instant::now());
    let later = |monotonic_ms, continuous_ms| ClockOrigin {
        monotonic: start.monotonic + Duration::from_millis(monotonic_ms),
        continuous: start.continuous + Duration::from_millis(continuous_ms),
    };
    let clocks = |monotonic_ms, continuous_ms| {
        Some(Clocks {
            monotonic_ns: monotonic_ms * 1_000_000,
            continuous_ns: continuous_ms * 1_000_000,
        })
    };
    let row = entry();
    let admit = |started, admitted| {
        let ticket = producer.begin(false, || Some(started)).unwrap();
        producer.publish_timed(ticket, observation(Some(row)), at(admitted, FLOOR))
    };

    let first = admit(start, later(200, 300)).unwrap();
    assert_eq!(first.age(), clocks(200, 300));
    assert!(admit(later(500, 500), later(600, 1_500)).is_none());
    let second = admit(later(800, 900), later(970, 1_110)).unwrap();
    assert_eq!(second.age(), clocks(170, 210));
    assert_eq!(second.gap(first), clocks(770, 810));
    assert_eq!(second.prior_slack(first), clocks(30, -110));
    assert!(first.gap(second).is_none());
    assert!(first.prior_slack(second).is_none());
}
