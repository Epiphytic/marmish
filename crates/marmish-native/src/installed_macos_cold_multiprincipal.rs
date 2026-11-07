//! Cold-first, four-service experiment. The product and fixture remain source571.
use super::*;
const KIND: &str = "macos-cold-multiprincipal-diagnostic";
const LABELS: [&str; 3] = ["client-1", "client-2", "client-3"];
#[derive(clap::Args)]
pub(in super::super) struct Options {
    #[arg(long)]
    assets: PathBuf,
    #[arg(long)]
    bootstrap: PathBuf,
    #[arg(long)]
    fixture: PathBuf,
    #[arg(long)]
    workspace: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    pins: PathBuf,
}
impl Options {
    fn common(self) -> super::Options {
        super::Options {
            assets: self.assets,
            bootstrap: self.bootstrap,
            fixture: self.fixture,
            workspace: self.workspace,
            output: self.output,
            pins: self.pins,
            concurrent: false,
        }
    }
}
#[derive(Default, Serialize)]
struct ServiceProof {
    cancel: bool,
    idle: bool,
    stop: bool,
}
#[derive(Default, Serialize)]
struct Cleanup {
    clients: [ServiceProof; 3],
    server: ServiceProof,
}
impl Cleanup {
    fn complete(&self) -> bool {
        self.clients
            .iter()
            .chain([&self.server])
            .all(|p| p.cancel && p.idle && p.stop)
    }
}
#[derive(Serialize)]
struct Principal {
    label: &'static str,
    role: &'static str,
    account_sha256: String,
    session_sha256: Option<String>,
    group_sha256: Option<String>,
    open_sha256: Option<String>,
    pair_valid: bool,
}
impl Principal {
    fn new(index: usize, account: &str) -> Self {
        Self {
            label: LABELS[index],
            role: records::ROLES[index],
            account_sha256: records::digest(account.as_bytes()),
            session_sha256: None,
            group_sha256: None,
            open_sha256: None,
            pair_valid: false,
        }
    }
}
#[derive(Serialize)]
struct Report<'a> {
    schema: u8,
    kind: &'static str,
    qualification: bool,
    same_host_only: bool,
    relay: &'static str,
    pins: Option<&'a Pins>,
    probe_sha256: String,
    stage: String,
    failure: Option<String>,
    scenario: &'static str,
    cold_before_direct: bool,
    local_outbound_fixture_traffic: bool,
    principals: Vec<Principal>,
    observations: Vec<Observation>,
    cleanup: Cleanup,
    passed: bool,
}
struct Services {
    clients: [Option<matrix::Daemon>; 3],
    server: Option<matrix::Daemon>,
    environment: process::Environment,
    accounts: Vec<String>,
    fixture: PathBuf,
}
fn topology(accounts: &[String], sockets: &[PathBuf]) -> Result<()> {
    if accounts.len() != 4
        || sockets.len() != 4
        || accounts.iter().any(|a| !records::hexadecimal(a, 64))
        || accounts
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != 4
        || sockets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != 4
    {
        return Err(Error::Invalid("topology_mismatch"));
    }
    Ok(())
}
async fn invoke_named(
    o: &super::Options,
    env: &process::Environment,
    args: &[OsString],
    deadline: Instant,
    signal: watch::Receiver<bool>,
) -> Result<Capture> {
    capture(
        &o.workspace.join("bin/marmish"),
        args,
        &o.workspace,
        env,
        &[],
        deadline,
        signal,
    )
    .await
}
async fn setup_profile(
    o: &super::Options,
    state: &Services,
    spec: ProfileSpec<'_>,
    peers: Option<&[String]>,
    deadline: Instant,
    signal: watch::Receiver<bool>,
) -> Result<String> {
    let args = setup_args(&o.workspace, spec, peers)?;
    let c = invoke_named(o, &state.environment, &args, cutoff(deadline, 120), signal).await?;
    if c.interrupted {
        return Err(Error::Invalid("interrupted"));
    }
    let setup: prepare::Setup =
        serde_json::from_slice(&c.stdout.bytes).map_err(|_| Error::Invalid("setup_failed"))?;
    if !success(&c)
        || !setup_valid_named(
            &setup,
            spec,
            peers.is_none(),
            spec.role == "server" && peers.is_some(),
        )
    {
        return Err(Error::Invalid("setup_failed"));
    }
    Ok(hex::encode(crate::command_config::account(&setup.npub)?))
}
async fn setup(
    o: &super::Options,
    pins: &Pins,
    state: &mut Services,
    deadline: Instant,
    signal: watch::Receiver<bool>,
    stage: &mut String,
) -> Result<()> {
    state.fixture = install_image(o, pins, &state.environment, deadline, signal.clone()).await?;
    *stage = "setup".into();
    let setup_end = cutoff(deadline, 240);
    for name in LABELS.into_iter().chain(["server"]) {
        let spec = ProfileSpec {
            identity_name: name,
            role: if name == "server" { "server" } else { "client" },
        };
        state
            .accounts
            .push(setup_profile(o, state, spec, None, setup_end, signal.clone()).await?);
    }
    let mut grants = state.accounts[..3].to_vec();
    grants.sort();
    for (i, name) in LABELS.into_iter().chain(["server"]).enumerate() {
        let spec = ProfileSpec {
            identity_name: name,
            role: if i == 3 { "server" } else { "client" },
        };
        let peers = if i == 3 {
            grants.clone()
        } else {
            vec![state.accounts[3].clone()]
        };
        if setup_profile(o, state, spec, Some(&peers), setup_end, signal.clone()).await?
            != state.accounts[i]
        {
            return Err(bad());
        }
        let path = o.workspace.join("profiles").join(name).join("config.json");
        let config = records::read(&path, 1048576)?;
        let bytes = configure_peers(config, spec.role, &peers, &o.workspace, &state.fixture)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
    }
    Ok(())
}
async fn ready(
    daemon: &mut matrix::Daemon,
    deadline: Instant,
    mut signal: watch::Receiver<bool>,
) -> Result<()> {
    tokio::select! {biased; _=cancelled(&mut signal)=>Err(Error::Invalid("interrupted")), result=matrix::ready(daemon,deadline)=>result.map(|_|())}
}
async fn start(
    o: &super::Options,
    state: &mut Services,
    deadline: Instant,
    signal: watch::Receiver<bool>,
    stage: &mut String,
) -> Result<()> {
    let sockets = LABELS
        .into_iter()
        .chain(["server"])
        .map(|n| autoexit::socket(&o.workspace, n, &state.environment))
        .collect::<Result<Vec<_>>>()?;
    topology(&state.accounts, &sockets)?;
    let relays = [RELAY.to_owned()];
    *stage = "server_ready".into();
    state.server = Some(matrix::Daemon::start(
        &o.workspace,
        "server",
        sockets[3].clone(),
        &state.environment,
        &relays,
        1200,
    )?);
    for i in 0..3 {
        state.clients[i] = Some(matrix::Daemon::start(
            &o.workspace,
            LABELS[i],
            sockets[i].clone(),
            &state.environment,
            &relays,
            1200,
        )?);
    }
    let end = cutoff(deadline, 180);
    let [a, b, c] = &mut state.clients;
    let (s, a, b, c) = tokio::join!(
        ready(state.server.as_mut().ok_or_else(bad)?, end, signal.clone()),
        ready(a.as_mut().ok_or_else(bad)?, end, signal.clone()),
        ready(b.as_mut().ok_or_else(bad)?, end, signal.clone()),
        ready(c.as_mut().ok_or_else(bad)?, end, signal)
    );
    s?;
    *stage = "client_ready".into();
    a?;
    b?;
    c?;
    Ok(())
}
async fn list(
    daemon: &mut Option<matrix::Daemon>,
    deadline: Instant,
) -> Result<Vec<streaming_cleanup::Entry>> {
    let d = daemon.as_mut().ok_or_else(bad)?;
    streaming_cleanup::entries(&mut d.child, &d.socket, cutoff(deadline, 15)).await
}
fn case(o: &super::Options, state: &Services, i: usize) -> Result<StreamCase> {
    let nonce = nonce()?;
    Ok(StreamCase {
        args: stream_args_named(
            &o.workspace,
            &state.clients[i].as_ref().ok_or_else(bad)?.socket,
            LABELS[i],
            &state.accounts[3],
            &nonce,
            records::ROLES[i],
            true,
        )?,
        input: streaming_fixture::bytes(&nonce, 7, streaming_fixture::STDIN_BYTES)?,
        nonce,
        case: format!("cold-{}", i + 1),
    })
}
struct Inspection<'a> {
    case: &'a StreamCase,
    capture: &'a Capture,
    before: &'a [streaming_cleanup::Entry],
    client: &'a [streaming_cleanup::Entry],
    server: &'a [streaming_cleanup::Entry],
    accounts: &'a [String],
    start: Instant,
    wall_ms: u64,
    capture_failed: bool,
}
fn inspect_cold(
    pins: &Pins,
    fixture: &Path,
    principal: &mut Principal,
    e: Inspection<'_>,
) -> Observation {
    let mut obs = Observation::captured(&e.case.case, &e.case.input, e.capture, e.start);
    obs.wall_ms = e.wall_ms;
    if e.capture_failed {
        obs.capture_io = true;
        obs.fail("capture_io");
    }
    let id = match inspect_capture(&mut obs, e.case, e.capture) {
        Ok(id) => id,
        Err(_) => {
            obs.fail("capture_io");
            None
        }
    };
    if obs.failure.is_none() {
        obs.stage = "client_journal".into();
    }
    inspect_journals(
        &mut obs,
        e.before,
        e.client,
        e.server,
        e.accounts,
        id.as_deref(),
        true,
    );
    if let Ok((c, _)) = choose(e.before, e.client, e.server, e.accounts, id.as_deref()) {
        principal.session_sha256 = Some(records::digest(c.session.as_bytes()));
        principal.group_sha256 = Some(c.group_sha256.clone());
        principal.open_sha256 = c.open_digest.clone();
    }
    // Exact original Pair contract in addition to the shared diagnostic checks.
    principal.pair_valid = obs.input_bytes == streaming_fixture::STDIN_BYTES
        && obs.stdin_written == streaming_fixture::STDIN_BYTES
        && obs.stdout_bytes == streaming_fixture::STDOUT_BYTES
        && obs.output_match
        && obs.exit == Some(7)
        && obs.wall_ms <= 330000
        && obs.binding_equal
        && obs.client_terminal == Some(marmish_stream::wire::Terminal::Exited { code: 7 })
        && obs.server_terminal == obs.client_terminal
        && obs.failure.is_none();
    if !principal.pair_valid && obs.failure.is_none() {
        obs.fail("hash_mismatch");
    }
    if obs.failure.is_none() {
        obs.stage = "receipt".into();
    }
    if check_receipt_named(&mut obs, fixture, pins, &e.case.nonce, principal.role).is_err() {
        obs.fail("receipt_malformed");
    }
    obs
}
fn can_direct(captures: &[Capture; 3], interrupted: bool, deadline: Instant) -> bool {
    captures.iter().all(|c| c.cleanup) && !interrupted && Instant::now() < deadline
}
async fn batch(
    o: &super::Options,
    pins: &Pins,
    state: &mut Services,
    deadline: Instant,
    signal: watch::Receiver<bool>,
    report: &mut Report<'_>,
) -> Result<()> {
    let [a, b, c] = &mut state.clients;
    let (before_a, before_b, before_c) =
        tokio::join!(list(a, deadline), list(b, deadline), list(c, deadline));
    let before = [before_a?, before_b?, before_c?];
    let cases = [case(o, state, 0)?, case(o, state, 1)?, case(o, state, 2)?];
    let executable = o.workspace.join("bin/marmish");
    let end = cutoff(deadline, 330);
    let start = Instant::now();
    let run = |index: usize| {
        let c = &cases[index];
        let executable = &executable;
        let environment = &state.environment;
        let signal = signal.clone();
        async move {
            let begun = Instant::now();
            let result = capture(
                executable,
                &c.args,
                &o.workspace,
                environment,
                &c.input,
                end,
                signal,
            )
            .await;
            (
                result,
                begun.elapsed().as_millis().min(u64::MAX as u128) as u64,
            )
        }
    };
    let (a, b, c) = tokio::join!(run(0), run(1), run(2));
    let wall_ms = [a.1, b.1, c.1];
    let outcomes = [a.0, b.0, c.0];
    let failed = std::array::from_fn::<_, 3, _>(|i| outcomes[i].is_err());
    let captures = outcomes.map(|result| result.unwrap_or_default());
    let [a, b, c] = &mut state.clients;
    let (la, lb, lc, ls) = tokio::join!(
        list(a, deadline),
        list(b, deadline),
        list(c, deadline),
        list(&mut state.server, deadline)
    );
    let clients = [la, lb, lc];
    for (i, (case, capture)) in cases.iter().zip(&captures).enumerate() {
        let mut obs = match (&clients[i], &ls) {
            (Ok(client), Ok(server)) => inspect_cold(
                pins,
                &state.fixture,
                &mut report.principals[i],
                Inspection {
                    case,
                    capture,
                    before: &before[i],
                    client,
                    server,
                    accounts: &[state.accounts[i].clone(), state.accounts[3].clone()],
                    start,
                    wall_ms: wall_ms[i],
                    capture_failed: failed[i],
                },
            ),
            _ => {
                let mut obs = Observation::captured(&case.case, &case.input, capture, start);
                if failed[i] {
                    obs.capture_io = true;
                    obs.fail("capture_io");
                }
                if inspect_capture(&mut obs, case, capture).is_err() {
                    obs.fail("capture_io");
                }
                if obs.failure.is_none() {
                    obs.stage = "client_journal".into();
                }
                obs.fail("list_absent");
                if check_receipt_named(
                    &mut obs,
                    &state.fixture,
                    pins,
                    &case.nonce,
                    records::ROLES[i],
                )
                .is_err()
                {
                    obs.fail("receipt_malformed");
                }
                obs
            }
        };
        obs.wall_ms = wall_ms[i];
        if *signal.borrow() {
            obs.fail("interrupted");
        }
        report.observations.push(obs);
    }
    if can_direct(&captures, *signal.borrow(), deadline) {
        report.cold_before_direct = true;
        report.stage = "fixture_direct".into();
        let nonce = nonce()?;
        let args = streaming_fixture::arguments(&nonce, "macos-arm64")?;
        let input = streaming_fixture::bytes(&nonce, 7, streaming_fixture::STDIN_BYTES)?;
        let start = Instant::now();
        let outcome = capture(
            &state.fixture,
            &args,
            &o.workspace,
            &state.environment,
            &input,
            cutoff(deadline, 15),
            signal,
        )
        .await;
        let failed = outcome.is_err();
        let capture = outcome.unwrap_or_default();
        let mut obs = Observation::captured("direct-after", &input, &capture, start);
        obs.stage = "fixture_direct".into();
        if failed {
            obs.capture_io = true;
            obs.fail("capture_io");
        }
        if check_capture(&mut obs, &capture, &nonce, &capture.stderr.bytes).is_err() {
            obs.fail("capture_io");
        }
        if check_receipt_named(&mut obs, &state.fixture, pins, &nonce, "macos-arm64").is_err() {
            obs.fail("receipt_malformed");
        }
        report.observations.push(obs);
    }
    Ok(())
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Cancel,
    Idle,
    Stop,
}
trait ServiceActions {
    async fn perform(&mut self, phase: Phase, deadline: Instant) -> bool;
}
async fn phase<A: ServiceActions>(
    actions: &mut [A; 4],
    kind: Phase,
    deadline: Instant,
) -> [bool; 4] {
    let [a, b, c, d] = actions;
    let (a, b, c, d) = tokio::join!(
        a.perform(kind, deadline),
        b.perform(kind, deadline),
        c.perform(kind, deadline),
        d.perform(kind, deadline)
    );
    [a, b, c, d]
}
async fn finalize_actions<A: ServiceActions>(actions: &mut [A; 4], start: Instant) -> Cleanup {
    let cancel = phase(actions, Phase::Cancel, start + Duration::from_secs(75)).await;
    let idle = phase(actions, Phase::Idle, start + Duration::from_secs(120)).await;
    let stop = phase(actions, Phase::Stop, start + Duration::from_secs(165)).await;
    let proof = |i| ServiceProof {
        cancel: cancel[i],
        idle: idle[i],
        stop: stop[i],
    };
    Cleanup {
        clients: [proof(0), proof(1), proof(2)],
        server: proof(3),
    }
}
struct ServiceAction<'a> {
    daemon: &'a mut Option<matrix::Daemon>,
    o: &'a super::Options,
    environment: &'a process::Environment,
    index: usize,
}
impl ServiceActions for ServiceAction<'_> {
    async fn perform(&mut self, phase: Phase, deadline: Instant) -> bool {
        match phase {
            Phase::Cancel => match self.daemon.as_mut() {
                None => false,
                Some(d) if self.index == 3 => {
                    streaming_cleanup::cancel_all(&mut d.child, &d.socket, cutoff(deadline, 60))
                        .await
                        .is_ok()
                }
                Some(d) => {
                    cancel_client(self.o, self.environment, d, LABELS[self.index], deadline).await
                }
            },
            Phase::Idle => match self.daemon.as_mut() {
                Some(d) => idle(d, deadline).await,
                None => false,
            },
            Phase::Stop => match self.daemon.take() {
                Some(d) => d.stop_streaming(deadline, self.index == 3).await.is_ok(),
                None => false,
            },
        }
    }
}
async fn cancel_client(
    o: &super::Options,
    environment: &process::Environment,
    daemon: &mut matrix::Daemon,
    name: &str,
    deadline: Instant,
) -> bool {
    let Ok(entries) =
        streaming_cleanup::entries(&mut daemon.child, &daemon.socket, cutoff(deadline, 15)).await
    else {
        return false;
    };
    let active = entries
        .iter()
        .filter(|e| e.direction == "client" && e.open_digest.is_some() && e.terminal.is_none())
        .collect::<Vec<_>>();
    let entry = match active.as_slice() {
        [] => return true,
        [entry] => *entry,
        _ => return false,
    };
    let Ok(args) = cancel_args_named(&o.workspace, name, &daemon.socket, entry) else {
        return false;
    };
    let (_, signal) = watch::channel(false);
    invoke_named(o, environment, &args, cutoff(deadline, 45), signal)
        .await
        .is_ok_and(|c| {
            success(&c)
                && c.stderr
                    .bytes
                    .windows(b"terminal state confirmed".len())
                    .any(|b| b == b"terminal state confirmed")
        })
}
async fn finalize(o: &super::Options, state: &mut Services) -> Cleanup {
    let start = Instant::now();
    let [a, b, c] = &mut state.clients;
    let make = |daemon, index| ServiceAction {
        daemon,
        o,
        environment: &state.environment,
        index,
    };
    finalize_actions(
        &mut [
            make(a, 0),
            make(b, 1),
            make(c, 2),
            make(&mut state.server, 3),
        ],
        start,
    )
    .await
}
fn may_remove_private_state(cleanup: &Cleanup, observations: &[Observation]) -> bool {
    cleanup.complete() && observations.iter().all(|o| o.owned_group_clean)
}
fn complete(report: &Report<'_>, result: &Result<()>, interrupted: bool) -> bool {
    let unique = |f: fn(&Principal) -> Option<&str>| {
        let values = report
            .principals
            .iter()
            .map(f)
            .collect::<Option<std::collections::BTreeSet<_>>>();
        values.is_some_and(|s| s.len() == 3)
    };
    result.is_ok()
        && !interrupted
        && report.cleanup.complete()
        && report.cold_before_direct
        && report.principals.len() == 3
        && report.principals.iter().all(|p| p.pair_valid)
        && unique(|p| Some(&p.account_sha256))
        && unique(|p| p.session_sha256.as_deref())
        && unique(|p| p.group_sha256.as_deref())
        && unique(|p| p.open_sha256.as_deref())
        && report.observations.iter().map(|o| o.case.as_str()).eq([
            "cold-1",
            "cold-2",
            "cold-3",
            "direct-after",
        ])
        && report.observations.iter().all(|o| o.failure.is_none())
}
pub(in super::super) async fn run(options: Options) -> Result<()> {
    let o = options.common();
    let (signal, watcher) = signal_watcher()?;
    let result = run_inner(&o, signal).await;
    watcher.abort();
    let _ = watcher.await;
    result
}
async fn run_inner(o: &super::Options, signal: watch::Receiver<bool>) -> Result<()> {
    preflight_paths(o)?;
    directory(&o.output)?;
    let pins = records::read::<Pins>(&o.pins, 16384).and_then(|p| {
        p.validate()?;
        Ok(p)
    });
    let mut report = Report {
        schema: 1,
        kind: KIND,
        qualification: false,
        same_host_only: true,
        relay: RELAY,
        pins: pins.as_ref().ok(),
        probe_sha256: records::digest(&records::read_bytes(&std::env::current_exe()?, 268435456)?),
        stage: "pins".into(),
        failure: None,
        scenario: "cold-three-principals",
        cold_before_direct: false,
        local_outbound_fixture_traffic: false,
        principals: Vec::new(),
        observations: Vec::new(),
        cleanup: Cleanup::default(),
        passed: false,
    };
    let result = async {
        let pins = pins.as_ref().map_err(|_| bad())?;
        if crate::native_manifest::compiled_release_metadata().target != TARGET
            || unsafe { libc::geteuid() } == 0
        {
            return Err(bad());
        }
        let mut state = Services {
            clients: [None, None, None],
            server: None,
            environment: prepare_environment(o)?,
            accounts: Vec::new(),
            fixture: PathBuf::new(),
        };
        let end = Instant::now() + Duration::from_secs(900);
        let result = async {
            report.stage = "install".into();
            setup(o, pins, &mut state, end, signal.clone(), &mut report.stage).await?;
            report.principals = state.accounts[..3]
                .iter()
                .enumerate()
                .map(|(i, a)| Principal::new(i, a))
                .collect();
            start(o, &mut state, end, signal.clone(), &mut report.stage).await?;
            report.stage = "stream_capture".into();
            batch(o, pins, &mut state, end, signal.clone(), &mut report).await
        }
        .await;
        report.cleanup = finalize(o, &mut state).await;
        let removal = if may_remove_private_state(&report.cleanup, &report.observations) {
            fs::remove_dir_all(&o.workspace).map_err(Error::from)
        } else {
            Ok(())
        };
        result.and(removal)
    }
    .await;
    report.failure = result.as_ref().err().map(|e| {
        if matches!(e, Error::Invalid("topology_mismatch")) {
            "topology_mismatch".into()
        } else {
            category(&report.stage, e).into()
        }
    });
    report.passed = complete(&report, &result, *signal.borrow());
    if *signal.borrow() {
        report.failure = Some("interrupted".into());
    }
    if !report.passed && report.failure.is_none() {
        report.failure = Some(
            if !report.cleanup.complete() {
                "cleanup_unconfirmed"
            } else {
                "hash_mismatch"
            }
            .into(),
        );
    }
    publish_report(
        PublicationContext {
            output: &o.output,
            report_kind: KIND,
        },
        &records::canonical(&report)?,
        report.passed,
    )?;
    if report.passed { Ok(()) } else { Err(bad()) }
}
#[cfg(test)]
#[path = "installed_macos_cold_multiprincipal_tests.rs"]
mod tests;
