//! Test-only sample accounting for the installed Manual scheduler.
use super::*;
use futures_util::{StreamExt as _, stream::FuturesUnordered};
use peer_read::{Entry, Failure, Reply, Request};
use serde::{Deserialize, Serialize};

const PERIOD: Duration = Duration::from_millis(500);
const PERIOD_NS: i64 = 500_000_000;
const PROOF_LIFETIME_NS: i64 = 1_000_000_000;
const SAMPLE_COUNT: usize = 20;
const MAX_LOG_BYTES: usize = 128 * 1024;
const PRESSURE_WIDTH: usize = 96;
const PRESSURE_PAUSE: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Clocks {
    monotonic_ns: i64,
    continuous_ns: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Published,
    TicketUnavailable,
    NoPending,
    ProofRefused,
    SelectionFailed,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct Refresh {
    schema: u8,
    #[serde(rename = "event")]
    _event: RefreshEvent,
    schedule_ns: i64,
    start_lag_ns: i64,
    missed_ticks: u64,
    outcome: Outcome,
    proof_age: Option<Clocks>,
    publish_gap: Option<Clocks>,
    prior_slack: Option<Clocks>,
}

#[derive(Clone, Copy, Deserialize)]
enum RefreshEvent {
    #[serde(rename = "pending_refresh")]
    PendingRefresh,
}

pub(super) struct RefreshLog {
    file: File,
    offset: usize,
    partial: Vec<u8>,
    records: Vec<Refresh>,
    event_bytes: usize,
    read_calls: usize,
    capture_ns: u128,
}

impl RefreshLog {
    pub(super) fn new(file: File) -> Self {
        Self {
            file,
            offset: 0,
            partial: Vec::new(),
            records: Vec::new(),
            event_bytes: 0,
            read_calls: 0,
            capture_ns: 0,
        }
    }

    fn collect(&mut self) {
        let began = Instant::now();
        let mut chunk = [0; 4096];
        loop {
            let remaining = MAX_LOG_BYTES - self.offset;
            assert!(remaining > 0, "bounded diagnostic log reached its limit");
            let limit = remaining.min(chunk.len());
            let count = self
                .file
                .read_at(&mut chunk[..limit], self.offset as u64)
                .expect("read fixture diagnostic log");
            self.read_calls += 1;
            if count == 0 {
                break;
            }
            self.offset += count;
            self.partial.extend_from_slice(&chunk[..count]);
            let Some(last_newline) = self.partial.iter().rposition(|byte| *byte == b'\n') else {
                continue;
            };
            for line in self.partial[..last_newline].split(|byte| *byte == b'\n') {
                // The fixture shares stderr with startup diagnostics. Select only the
                // fixed event and reject invalid or incomplete *complete* event lines.
                if !line
                    .windows(b"\"event\":\"pending_refresh\"".len())
                    .any(|part| part == b"\"event\":\"pending_refresh\"")
                {
                    continue;
                }
                let record: Refresh = serde_json::from_slice(line)
                    .unwrap_or_else(|_| panic!("invalid fixed refresh event schema"));
                let prior = self.records.last();
                assert_eq!(record.schema, 1, "unsupported refresh schema version");
                assert!(
                    record.schedule_ns >= 0
                        && record.schedule_ns % PERIOD_NS == 0
                        && (0..PERIOD_NS).contains(&record.start_lag_ns)
                        && prior.is_none_or(|prior| record.schedule_ns > prior.schedule_ns),
                    "invalid refresh schedule coordinates"
                );
                self.records.push(record);
                self.event_bytes += line.len() + 1;
            }
            self.partial.drain(..=last_newline);
        }
        self.capture_ns += began.elapsed().as_nanos();
    }

    pub(super) async fn report_empty(&mut self, socket: &Path) {
        let deadline = Instant::now() + WAIT;
        while self.records.len() < 3 && Instant::now() < deadline {
            self.collect();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let startup_samples = &self.records[..self.records.len().min(3)];
        let mut outcomes = Outcomes::default();
        for record in startup_samples {
            outcomes.add(record.outcome);
        }
        let read = request(socket, None, 0).await;
        println!(
            "manual_empty {}",
            json!({"sample_count": startup_samples.len(), "refresh_outcomes": outcomes, "ipc": read.outcome})
        );
        assert_eq!(startup_samples.len(), 3, "empty refresh samples recorded");
        assert_eq!(outcomes.no_pending, startup_samples.len());
        assert_eq!(read.outcome, ReadOutcome::Unavailable);
    }

    async fn anchor_pending(&mut self) -> i64 {
        timeout(WAIT, async {
            loop {
                self.collect();
                if let Some(record) = self
                    .records
                    .last()
                    .filter(|r| r.outcome == Outcome::Published)
                {
                    break record.schedule_ns;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("cold exact Pending has a preceding publication")
    }

    async fn window(&mut self, anchor_ns: i64) -> RefreshWindow {
        let last_slot = anchor_ns + PERIOD_NS * SAMPLE_COUNT as i64;
        let deadline = Instant::now() + PERIOD * SAMPLE_COUNT as u32 + Duration::from_secs(2);
        loop {
            self.collect();
            // Wait one additional record to bracket a missing final publication.
            // A fixed terminal deadline also preserves incomplete windows.
            if self
                .records
                .iter()
                .rev()
                .take_while(|r| r.schedule_ns >= last_slot)
                .any(|r| r.outcome == Outcome::Published)
                || Instant::now() >= deadline
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        RefreshWindow::from_records(&self.records, anchor_ns, last_slot)
    }

    pub(super) async fn qualify(&mut self, socket: &Path, entry: Entry) -> bool {
        let anchor = self.anchor_pending().await;
        let start = Instant::now();
        let (refresh, ipc) = tokio::join!(self.window(anchor), probes(socket, entry, start));
        let baseline = report("independent", &refresh, &ipc, &[]);

        let anchor = self.anchor_pending().await;
        let start = Instant::now();
        let (refresh, ipc, load) = tokio::join!(
            self.window(anchor),
            probes(socket, entry, start),
            pressure(socket, entry, start),
        );
        let loaded = report("concurrent_pressure", &refresh, &ipc, &load);
        println!(
            "manual_refresh_capture {}",
            json!({
                "bytes_read": self.offset,
                "event_bytes": self.event_bytes,
                "events": self.records.len(),
                "read_calls": self.read_calls,
                "capture_ns": u64::try_from(self.capture_ns).unwrap_or(u64::MAX),
            })
        );
        baseline && loaded
    }
}

#[derive(Default, Serialize)]
struct Outcomes {
    published: usize,
    ticket_unavailable: usize,
    no_pending: usize,
    proof_refused: usize,
    selection_failed: usize,
}

impl Outcomes {
    fn add(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Published => self.published += 1,
            Outcome::TicketUnavailable => self.ticket_unavailable += 1,
            Outcome::NoPending => self.no_pending += 1,
            Outcome::ProofRefused => self.proof_refused += 1,
            Outcome::SelectionFailed => self.selection_failed += 1,
        }
    }
}

#[derive(Default)]
struct RefreshWindow {
    outcomes: Outcomes,
    recorded: usize,
    missed_ticks: u64,
    start_lag: Vec<i64>,
    age: Vec<Clocks>,
    gap: Vec<Clocks>,
    slack: Vec<Clocks>,
    bracketed: bool,
}

impl RefreshWindow {
    fn from_records(records: &[Refresh], anchor: i64, last_slot: i64) -> Self {
        let mut window = Self::default();
        for record in records.iter().filter(|record| record.schedule_ns > anchor) {
            if record.schedule_ns > last_slot {
                // Include only continuity coordinates from the first publication
                // after the window when its tail did not end in a publication.
                if window.bracketed {
                    break;
                }
                if record.outcome == Outcome::Published {
                    window.record_continuity(record);
                    window.bracketed = true;
                    break;
                }
                continue;
            }
            window.recorded += 1;
            window.outcomes.add(record.outcome);
            window.missed_ticks = window.missed_ticks.saturating_add(record.missed_ticks);
            window.start_lag.push(record.start_lag_ns);
            window.age.extend(record.proof_age);
            window.record_continuity(record);
            window.bracketed =
                record.schedule_ns == last_slot && record.outcome == Outcome::Published;
        }
        window
    }

    fn record_continuity(&mut self, record: &Refresh) {
        self.gap.extend(record.publish_gap);
        self.slack.extend(record.prior_slack);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ReadOutcome {
    Exact,
    Busy,
    Unavailable,
    Timeout,
    Unexpected,
}

struct ReadSample {
    outcome: ReadOutcome,
    latency_ns: i64,
    show: bool,
}

async fn request(socket: &Path, expected: Option<Entry>, index: usize) -> ReadSample {
    let show = expected.is_some() && !index.is_multiple_of(2);
    let query = match (show, expected) {
        (true, Some(entry)) => Request::Show {
            selection: entry.selection,
        },
        _ => Request::List {},
    };
    let start = Instant::now();
    let reply = peer_read::client_request(socket, query).await;
    let latency_ns = i64::try_from(start.elapsed().as_nanos()).unwrap_or(i64::MAX);
    let outcome = match reply {
        Ok(Reply::List { entries }) if expected.as_slice() == entries.as_slice() => {
            ReadOutcome::Exact
        }
        Ok(Reply::Show { entry }) if expected == Some(entry) => ReadOutcome::Exact,
        Err(Failure::Busy) => ReadOutcome::Busy,
        Err(Failure::Unavailable) => ReadOutcome::Unavailable,
        Err(Failure::Timeout) => ReadOutcome::Timeout,
        _ => ReadOutcome::Unexpected,
    };
    ReadSample {
        outcome,
        latency_ns,
        show,
    }
}

async fn probes(socket: &Path, entry: Entry, start: Instant) -> Vec<ReadSample> {
    let mut probes = FuturesUnordered::new();
    for index in 0..SAMPLE_COUNT {
        probes.push(async move {
            let due = start + PERIOD * index as u32 + PERIOD / 2;
            tokio::time::sleep_until(due).await;
            request(socket, Some(entry), index).await
        });
    }
    let mut results = Vec::with_capacity(SAMPLE_COUNT);
    while let Some(result) = probes.next().await {
        results.push(result);
    }
    results
}

async fn pressure(socket: &Path, entry: Entry, start: Instant) -> Vec<ReadSample> {
    let deadline = start + PERIOD * SAMPLE_COUNT as u32;
    let mut results = Vec::new();
    while Instant::now() < deadline {
        let mut batch = FuturesUnordered::new();
        for index in 0..PRESSURE_WIDTH {
            batch.push(request(socket, Some(entry), index));
        }
        while let Some(result) = batch.next().await {
            results.push(result);
        }
        tokio::time::sleep(PRESSURE_PAUSE).await;
    }
    results
}

#[derive(Debug, Serialize)]
struct Percentiles {
    p50: i64,
    p95: i64,
    max: i64,
}

fn percentiles(values: impl IntoIterator<Item = i64>) -> Option<Percentiles> {
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_unstable();
    let rank = |percent: usize| values[(values.len() * percent).div_ceil(100) - 1];
    Some(Percentiles {
        p50: rank(50),
        p95: rank(95),
        max: *values.last()?,
    })
}

#[derive(Serialize)]
struct ByClock<T> {
    monotonic: T,
    continuous: T,
}

fn by_clock<T>(samples: &[Clocks], aggregate: impl Fn(Vec<i64>) -> T) -> ByClock<T> {
    ByClock {
        monotonic: aggregate(samples.iter().map(|clock| clock.monotonic_ns).collect()),
        continuous: aggregate(samples.iter().map(|clock| clock.continuous_ns).collect()),
    }
}

#[derive(Default, Serialize)]
struct IpcResults {
    list: usize,
    show: usize,
    exact: usize,
    unavailable: usize,
    timeout: usize,
    busy: usize,
    unexpected: usize,
}

fn ipc_counts(samples: &[ReadSample]) -> IpcResults {
    let mut counts = IpcResults::default();
    for sample in samples {
        if sample.show {
            counts.show += 1;
        } else {
            counts.list += 1;
        }
        match sample.outcome {
            ReadOutcome::Exact => counts.exact += 1,
            ReadOutcome::Busy => counts.busy += 1,
            ReadOutcome::Unavailable => counts.unavailable += 1,
            ReadOutcome::Timeout => counts.timeout += 1,
            ReadOutcome::Unexpected => counts.unexpected += 1,
        }
    }
    counts
}

fn report(
    label: &str,
    refresh: &RefreshWindow,
    probes: &[ReadSample],
    load: &[ReadSample],
) -> bool {
    let ages = by_clock(&refresh.age, percentiles);
    let gaps = by_clock(&refresh.gap, percentiles);
    let min_slack = by_clock(&refresh.slack, |values| values.into_iter().min());
    let longest_unavailable = by_clock(&refresh.slack, |values| {
        values.into_iter().min().map(|value| -value.min(0))
    });
    let probe_counts = ipc_counts(probes);
    let load_counts = ipc_counts(load);
    let refresh_ok = refresh.recorded == SAMPLE_COUNT
        && refresh.outcomes.published == SAMPLE_COUNT
        && refresh.missed_ticks == 0
        && refresh.age.len() == SAMPLE_COUNT
        && refresh.gap.len() == SAMPLE_COUNT
        && refresh.slack.len() == SAMPLE_COUNT
        && refresh.bracketed
        && [ages.monotonic.as_ref(), ages.continuous.as_ref()]
            .into_iter()
            .all(|stats| {
                stats.is_some_and(|stats| stats.p95 <= PERIOD_NS && stats.max < PROOF_LIFETIME_NS)
            })
        && [min_slack.monotonic, min_slack.continuous]
            .into_iter()
            .all(|slack| slack.is_some_and(|slack| slack > 0));
    let probes_ok = probes.len() == SAMPLE_COUNT
        && probe_counts.list == SAMPLE_COUNT / 2
        && probe_counts.show == SAMPLE_COUNT / 2
        && probe_counts.unexpected == 0
        && (label != "independent" || probe_counts.exact == SAMPLE_COUNT);
    println!(
        "manual_near_cap {}",
        json!({
            "cohort": label,
            "qualified_refresh": refresh_ok,
            "qualified_probes": probes_ok,
            "scheduled_slots": SAMPLE_COUNT,
            "recorded_slots": refresh.recorded,
            "unrecorded_slots": SAMPLE_COUNT - refresh.recorded,
            "reported_missed_ticks": refresh.missed_ticks,
            "publications": refresh.outcomes,
            "start_lag_ns": percentiles(refresh.start_lag.iter().copied()),
            "proof_age_ns": ages,
            "publish_gap_ns": gaps,
            "minimum_prior_slack_ns": min_slack,
            "longest_confirmed_unavailable_ns": longest_unavailable,
            "gap_fully_bracketed": refresh.bracketed && refresh.slack.len() >= refresh.outcomes.published,
            "continuity_pairs": refresh.slack.len(),
            "probe_counts": probe_counts,
            "probe_latency_ns": percentiles(probes.iter().map(|result| result.latency_ns)),
            "pressure_offered_concurrency": if load.is_empty() { 0 } else { PRESSURE_WIDTH },
            "pressure_counts": load_counts,
            "pressure_latency_ns": percentiles(load.iter().map(|result| result.latency_ns)),
        })
    );
    refresh_ok
        && probes_ok
        && (label == "independent" || (!load.is_empty() && load_counts.unexpected == 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_log_waits_for_record_terminator_without_recounting() {
        let mut file = tempfile::tempfile().unwrap();
        let mut log = RefreshLog::new(file.try_clone().unwrap());
        let line = br#"{"schema":1,"event":"pending_refresh","schedule_ns":500000000,"start_lag_ns":1,"missed_ticks":0,"outcome":"no_pending","proof_age":null,"publish_gap":null,"prior_slack":null}"#;
        file.write_all(&line[..50]).unwrap();
        log.collect();
        assert!(log.records.is_empty());
        file.write_all(&line[50..]).unwrap();
        log.collect();
        assert!(log.records.is_empty());
        file.write_all(b"\n").unwrap();
        log.collect();
        log.collect();
        assert_eq!(log.records.len(), 1);
        assert_eq!(log.records[0].outcome, Outcome::NoPending);
        assert_eq!(log.event_bytes, line.len() + 1);
    }

    #[test]
    fn refresh_window_keeps_skipped_slots_and_brackets_an_unavailable_tail() {
        let age = Clocks {
            monotonic_ns: 401_000_000,
            continuous_ns: 403_000_000,
        };
        let gap = Clocks {
            monotonic_ns: 1_499_000_000,
            continuous_ns: 1_501_000_000,
        };
        let slack = Clocks {
            monotonic_ns: -750_000_000,
            continuous_ns: -751_000_000,
        };
        let refused = Refresh {
            schema: 1,
            _event: RefreshEvent::PendingRefresh,
            schedule_ns: 2 * PERIOD_NS,
            start_lag_ns: 10,
            missed_ticks: 1,
            outcome: Outcome::ProofRefused,
            proof_age: None,
            publish_gap: None,
            prior_slack: None,
        };
        let late = Refresh {
            schedule_ns: 3 * PERIOD_NS,
            missed_ticks: 0,
            outcome: Outcome::Published,
            proof_age: Some(age),
            publish_gap: Some(gap),
            prior_slack: Some(slack),
            ..refused
        };
        let records = [refused, late];
        let incomplete = RefreshWindow::from_records(&records[..1], 0, 2 * PERIOD_NS);
        assert!(!incomplete.bracketed);
        assert!(incomplete.slack.is_empty());
        let measured = RefreshWindow::from_records(&records, 0, 2 * PERIOD_NS);
        assert_eq!(measured.recorded, 1);
        assert_eq!(measured.missed_ticks, 1);
        assert_eq!(measured.outcomes.proof_refused, 1);
        assert_eq!(measured.outcomes.published, 0);
        assert!(measured.bracketed);
        assert!(measured.age.is_empty());
        assert_eq!(measured.slack[0].continuous_ns, slack.continuous_ns);
        assert_eq!(percentiles([1, 600, 400, 200, 300]).unwrap().p95, 600);
    }
}
