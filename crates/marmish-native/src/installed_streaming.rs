//! Candidate-installer qualification with bounded, asynchronous three-platform rendezvous.
use super::{
    matrix::{self, Daemon, Run},
    process::Environment,
    records::{self, Context, IdentityReady, ROLES, TARGETS},
    streaming_cases::{self, Endpoint},
    streaming_cleanup,
    streaming_diagnostic::{Origin, Stage, Tracker},
    streaming_fixture, streaming_prerequisites,
    streaming_records::{self, ClientResult, ServerReady, ServerReceipt, ServerResult},
    transfer_cases, transfer_fixture,
};
use crate::{Error, Result};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::time::Instant;
fn bad() -> Error {
    Error::Invalid("installed_streaming")
}
fn failure_category(error: &Error) -> &'static str {
    match error {
        Error::Timeout => "timeout",
        Error::Io(_) => "local_io",
        _ => "validation_or_transport",
    }
}
fn deadline(epoch: u64) -> Result<Instant> {
    Ok(Instant::now()
        + Duration::from_secs(epoch.checked_sub(super::now()?).ok_or(Error::Timeout)?))
}
fn copy_fixture(ctx: &Context, workspace: &Path, index: usize) -> Result<PathBuf> {
    let driver = records::verify_tools(ctx, index)?;
    let bytes = records::read_bytes(
        &driver
            .parent()
            .ok_or_else(bad)?
            .join("marmish-installed-fixture"),
        268435456,
    )?;
    if records::digest(&bytes) != ctx.tooling[index].files.fixture.sha256 {
        return Err(bad());
    }
    let dir = workspace.join("fixture");
    fs::DirBuilder::new().mode(0o700).create(&dir)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(dir.join("receipts"))?;
    let executable = dir.join("main");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&executable)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(executable.canonicalize()?)
}
async fn configure(
    workspace: &Path,
    name: &str,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    fixture: &Path,
    environment: &Environment,
    until: Instant,
) -> Result<()> {
    matrix::configure(workspace, name, ctx, ids, fixture, environment, until).await?;
    let path = workspace.join("profiles").join(name).join("config.json");
    let mut config: serde_json::Value = records::read(&path, 1048576)?;
    config["cwd"] = serde_json::json!(workspace.to_str().ok_or_else(bad)?);
    config["limits"] = serde_json::json!({"max_uptime":1800});
    if ctx.cohort == "edge-stream" {
        config["limits"]["active_per_principal"] = serde_json::json!(2);
    }
    let mut allowed = vec![fixture.to_str().ok_or_else(bad)?.to_owned()];
    if let Some(pin) = streaming_prerequisites::rsync(ctx, workspace)? {
        allowed.push(pin.path);
    }
    if matches!(
        ctx.cohort.as_str(),
        "copy-matrix" | "edge-copy" | "copy-interrupt-put" | "copy-interrupt-get"
    ) {
        allowed.push(
            workspace
                .join("bin/marmish")
                .canonicalize()?
                .to_str()
                .ok_or_else(bad)?
                .into(),
        );
    }
    if name == "server" {
        config["command_policy"]["defaults"]["allow_exec"] = serde_json::json!(allowed);
    }
    crate::command_config::CommandConfig::from_bytes(&records::canonical(&config)?)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)?;
    file.write_all(&records::canonical(&config)?)?;
    file.sync_all()?;
    Ok(())
}
async fn servers(
    o: &Run,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    until: Instant,
    diagnostic: &mut Tracker,
) -> Result<BTreeMap<String, ServerReady>> {
    loop {
        let mut all = BTreeMap::new();
        for role in ROLES {
            diagnostic.peer(Stage::ServerRecordRead, role);
            let path = o.inbox.join(format!("{role}-server-ready.json"));
            if path.try_exists()? {
                let Some(record) = records::read_published::<ServerReady>(&path, 65536)? else {
                    continue;
                };
                diagnostic.peer(Stage::ServerRecordValidate, role);
                record.validate(ctx, ids, role)?;
                all.insert(role.into(), record);
            }
        }
        if all.len() == 3 {
            return Ok(all);
        }
        diagnostic.enter(Stage::ServerRendezvousWait);
        if Instant::now() >= until {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
fn common_budget(servers: &BTreeMap<String, ServerReady>) -> Result<streaming_records::Budget> {
    if servers.len() != 3 {
        return Err(bad());
    }
    let ready_by = servers.values().map(|s| s.ready_by).min().ok_or_else(bad)?;
    let traffic_by = servers
        .values()
        .map(|s| s.traffic_by)
        .min()
        .ok_or_else(bad)?;
    let stop_by = servers.values().map(|s| s.stop_by).min().ok_or_else(bad)?;
    if servers.values().any(|s| s.ready_at >= ready_by) {
        return Err(bad());
    }
    Ok(streaming_records::Budget {
        ready_by,
        traffic_by,
        stop_by,
    })
}
fn validate_client(
    c: &ClientResult,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    servers: &BTreeMap<String, ServerReady>,
    role: &str,
) -> Result<()> {
    if c.schema != 1
        || c.kind != "client-result"
        || c.context_sha256 != ctx.digest()?
        || c.role != role
        || c.identity_ready_sha256 != streaming_records::pins(ids)?
        || c.server_ready_sha256 != matrix::pins(servers)?
        || !c.complete
        || !c.failed_cells.is_empty()
    {
        return Err(bad());
    }
    if super::edges::is_edge(&ctx.cohort) {
        let proof = c.edges.as_ref().ok_or_else(bad)?;
        if !c.pairs.is_empty() || c.interruption.is_some() {
            return Err(bad());
        }
        return proof.validate(&ctx.cohort);
    }
    if c.edges.is_some() {
        return Err(bad());
    }
    if let Some(put) = interruption_direction(&ctx.cohort) {
        let proof = c.interruption.as_ref().ok_or_else(bad)?;
        proof.validate()?;
        if !c.pairs.is_empty() || proof.direction != if put { "put" } else { "get" } {
            return Err(bad());
        }
        return Ok(());
    }
    if c.interruption.is_some() || c.pairs.len() != 3 * streaming_records::cases(&ctx.cohort)?.len()
    {
        return Err(bad());
    }
    let cases = streaming_records::cases(&ctx.cohort)?;
    let expected = ROLES
        .into_iter()
        .flat_map(|server| cases.iter().map(move |case| (server, *case)));
    for (pair, (server, case)) in c.pairs.iter().zip(expected) {
        if pair.client != role || pair.server != server || pair.case != case {
            return Err(bad());
        }
        pair.validate(ctx, ids)?;
    }
    Ok(())
}
async fn clients(
    o: &Run,
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    servers: &BTreeMap<String, ServerReady>,
    until: Instant,
) -> Result<BTreeMap<String, ClientResult>> {
    loop {
        let mut all = BTreeMap::new();
        for role in ROLES {
            let path = o.inbox.join(format!("{role}-client-result.json"));
            if path.try_exists()? {
                let Some(record) = records::read_published::<ClientResult>(&path, 1048576)? else {
                    continue;
                };
                validate_client(&record, ctx, ids, servers, role)?;
                all.insert(role.into(), record);
            }
        }
        if all.len() == 3 {
            return Ok(all);
        }
        if Instant::now() >= until {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
async fn cell(
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    role: &str,
    server: &str,
    ready: &ServerReady,
    endpoint: &Endpoint<'_>,
    until: Instant,
) -> Result<Vec<streaming_records::Pair>> {
    match ctx.cohort.as_str() {
        "stream-matrix" => Ok(vec![
            streaming_cases::duplex(ctx, ids, role, server, ready, endpoint, until).await?,
        ]),
        "copy-matrix" => transfer_cases::copy(ctx, ids, role, server, ready, endpoint, until).await,
        "rsync-matrix" => {
            transfer_cases::rsync(ctx, ids, role, server, ready, endpoint, until).await
        }
        _ => Err(bad()),
    }
}
async fn collect_receipts(
    ctx: &Context,
    ids: &BTreeMap<String, IdentityReady>,
    role: &str,
    clients: &BTreeMap<String, ClientResult>,
    server: &mut Daemon,
    executable: &Path,
    until: Instant,
) -> Result<Vec<ServerReceipt>> {
    let entries = streaming_cleanup::entries(&mut server.child, &server.socket, until).await?;
    let mut result = Vec::new();
    for client in ROLES {
        for case in streaming_records::cases(&ctx.cohort)? {
            let pair = clients[client]
                .pairs
                .iter()
                .find(|p| p.server == role && p.case == *case)
                .ok_or_else(bad)?;
            let entry = entries
                .iter()
                .find(|e| records::digest(e.stream.as_bytes()) == pair.stream)
                .ok_or_else(bad)?;
            if entry.direction != "server"
                || entry.client != ids[client].client_account
                || entry.server != ids[role].server_account
                || entry.active
                || entry.state != "terminal"
                || entry.cancel_requested
                || records::digest(entry.session.as_bytes()) != pair.session
                || entry.open_digest.as_deref() != Some(pair.open_digest.as_str())
                || entry.group_sha256 != pair.group_sha256
                || entry.terminal
                    != Some(marmish_stream::wire::Terminal::Exited { code: pair.exit })
            {
                return Err(bad());
            }
            let controls = pair
                .transfer
                .as_ref()
                .map(|t| t.controls.clone())
                .unwrap_or_default();
            for control in &controls {
                let e = entries
                    .iter()
                    .find(|e| records::digest(e.stream.as_bytes()) == control.stream_sha256)
                    .ok_or_else(bad)?;
                if e.direction != "server"
                    || e.client != ids[client].client_account
                    || e.server != ids[role].server_account
                    || e.active
                    || e.cancel_requested
                    || e.state != "terminal"
                    || e.terminal != Some(marmish_stream::wire::Terminal::Exited { code: 0 })
                    || records::digest(e.session.as_bytes()) != control.session_sha256
                    || e.open_digest.as_deref() != Some(control.open_digest.as_str())
                    || e.group_sha256 != control.group_sha256
                {
                    return Err(bad());
                }
            }
            let (fixture, transfer) = if *case == "duplex" {
                let receipt: streaming_fixture::Receipt = records::read(
                    &streaming_fixture::receipt_path(executable, &pair.nonce)?,
                    65536,
                )?;
                let index = ROLES.iter().position(|r| *r == role).ok_or_else(bad)?;
                if receipt
                    != streaming_fixture::receipt(
                        executable,
                        &ctx.tooling[index].files.fixture.sha256,
                        &pair.nonce,
                        client,
                    )?
                {
                    return Err(bad());
                }
                (Some(receipt), None)
            } else {
                let receipt: transfer_fixture::Receipt = records::read(
                    &transfer_fixture::receipt_path(executable, &pair.nonce, case)?,
                    65536,
                )?;
                if receipt.nonce != pair.nonce
                    || receipt.client != client
                    || receipt.case != *case
                    || receipt.items != transfer_fixture::expected(&pair.nonce, case)?
                {
                    return Err(bad());
                }
                (None, Some(receipt))
            };
            result.push(ServerReceipt {
                client: client.into(),
                case: case.to_string(),
                stream: pair.stream.clone(),
                session: pair.session.clone(),
                open_digest: pair.open_digest.clone(),
                group_sha256: pair.group_sha256.clone(),
                fixture,
                transfer,
                controls,
            });
        }
    }
    Ok(result)
}
struct Runtime {
    server: Option<Daemon>,
    client: Option<Daemon>,
}
impl Runtime {
    async fn close(&mut self, until: Instant) -> (bool, bool, bool) {
        let mut clean = true;
        for daemon in [&mut self.server, &mut self.client].into_iter().flatten() {
            if streaming_cleanup::cancel_all(&mut daemon.child, &daemon.socket, until)
                .await
                .is_err()
            {
                clean = false;
            }
        }
        let server = match self.server.take() {
            Some(d) => d.stop_streaming(until, true).await.is_ok(),
            None => false,
        };
        let client = match self.client.take() {
            Some(d) => d.stop_streaming(until, false).await.is_ok(),
            None => false,
        };
        (clean, server, client)
    }
}
fn interruption_direction(cohort: &str) -> Option<bool> {
    match cohort {
        "copy-interrupt-put" => Some(true),
        "copy-interrupt-get" => Some(false),
        _ => None,
    }
}
pub(super) async fn run(o: Run) -> Result<()> {
    let mut diagnostic = Tracker::new();
    let result = run_tracked(o, &mut diagnostic).await;
    if let Err(error) = &result {
        diagnostic.emit(error, &super::streaming_observation::stderr_sink);
    }
    result
}
async fn run_tracked(o: Run, diagnostic: &mut Tracker) -> Result<()> {
    let inspection_origin = Instant::now();
    let ctx: Context = records::read(&o.context, 65536)?;
    ctx.validate()?;
    // Other reviewed cohorts are enabled only when their complete driver is integrated.
    let cases = if super::edges::is_edge(&ctx.cohort) {
        &[][..]
    } else {
        streaming_records::cases(&ctx.cohort)?
    };
    let ids = records::identities(&o.identities, &ctx)?;
    let index = TARGETS
        .iter()
        .position(|t| *t == crate::native_manifest::compiled_release_metadata().target)
        .ok_or_else(bad)?;
    let role = ROLES[index];
    diagnostic.role(role);
    let own = records::own_identity(&o.workspace, &ctx)?;
    if records::canonical(&own)? != records::canonical(&ids[role])? {
        return Err(bad());
    }
    crate::identity::check_dir(&o.workspace)?;
    crate::identity::check_dir(&o.output)?;
    let environment = Environment {
        home: o.workspace.join("home"),
        temporary: o.workspace.join("tmp"),
        utilities: o.workspace.join("utilities"),
    };
    let executable = copy_fixture(&ctx, &o.workspace, index)?;
    let preparation = deadline(ctx.created_at.checked_add(720).ok_or_else(bad)?)?;
    for name in ["client", "server"] {
        configure(
            &o.workspace,
            name,
            &ctx,
            &ids,
            &executable,
            &environment,
            preparation,
        )
        .await?;
    }
    let start = super::now()?;
    let budget = streaming_records::budget(&ctx, start, start)?;
    let ready_by = deadline(budget.ready_by)?;
    let traffic_by = deadline(budget.traffic_by)?;
    let mut stop_by = deadline(budget.stop_by)?;
    let server_socket = super::autoexit::socket(&o.workspace, "server", &environment)?;
    let client_socket = super::autoexit::socket(&o.workspace, "client", &environment)?;
    if server_socket == client_socket {
        return Err(Error::Invalid("installed_socket_collision"));
    }
    let mut runtime = Runtime {
        server: None,
        client: None,
    };
    let mut receipt_records = Vec::new();
    let mut interruption = None;
    let mut edges = None;
    let mut ready_hash = String::new();
    let mut termination =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    diagnostic.origin(Origin::WorkError);
    let work = async {
        diagnostic.enter(Stage::DaemonStartup);
        runtime.server = Some(Daemon::start(
            &o.workspace,
            "server",
            server_socket,
            &environment,
            &ctx.relays,
            1800,
        )?);
        runtime.client = Some(Daemon::start(
            &o.workspace,
            "client",
            client_socket,
            &environment,
            &ctx.relays,
            1800,
        )?);
        diagnostic.enter(Stage::DaemonReadiness);
        matrix::ready(runtime.server.as_mut().ok_or_else(bad)?, ready_by).await?;
        matrix::ready(runtime.client.as_mut().ok_or_else(bad)?, ready_by).await?;
        diagnostic.enter(Stage::OwnReadyPublish);
        let record = ServerReady {
            schema: 1,
            kind: "server-ready".into(),
            context_sha256: ctx.digest()?,
            role: role.into(),
            identity_ready_sha256: streaming_records::pins(&ids)?,
            server_account: own.server_account,
            started_at: start,
            ready_at: super::now()?,
            ready_by: budget.ready_by,
            traffic_by: budget.traffic_by,
            stop_by: budget.stop_by,
            fixture_sha256: ctx.tooling[index].files.fixture.sha256.clone(),
            root_label: "transfer-v1".into(),
            rsync: streaming_prerequisites::rsync(&ctx, &o.workspace)?,
        };
        record.validate(&ctx, &ids, role)?;
        ready_hash = records::digest(&records::canonical(&record)?);
        records::write(&o.output.join(format!("{role}-server-ready.json")), &record)?;
        let all_servers = servers(&o, &ctx, &ids, ready_by, diagnostic).await?;
        diagnostic.enter(Stage::CommonBudget);
        let common = common_budget(&all_servers)?;
        if super::now()? >= common.ready_by {
            return Err(Error::Timeout);
        }
        let traffic_by = deadline(common.traffic_by)?;
        stop_by = deadline(common.stop_by)?;
        let client = runtime.client.as_mut().ok_or_else(bad)?;
        let ep = Endpoint {
            daemon: std::sync::Arc::new(tokio::sync::Mutex::new(&mut client.child)),
            socket: &client.socket,
            environment: &environment,
            workspace: &o.workspace,
        };
        if super::edges::is_edge(&ctx.cohort) {
            diagnostic.enter(Stage::EdgeExecute);
            let server = runtime.server.as_mut().ok_or_else(bad)?;
            let server_ep = Endpoint {
                daemon: std::sync::Arc::new(tokio::sync::Mutex::new(&mut server.child)),
                socket: &server.socket,
                environment: &environment,
                workspace: &o.workspace,
            };
            let edge_c = super::edge_signals::Cohort {
                context: &ctx,
                identities: &ids,
                role,
                ready: &all_servers[role],
                client: &ep,
                server: &server_ep,
                deadline: traffic_by
                    .checked_sub(Duration::from_secs(60))
                    .ok_or_else(bad)?,
            };
            let evidence = super::edges::run(edge_c).await?;
            diagnostic.enter(Stage::CompletedResultValidation);
            let result = ClientResult {
                schema: 1,
                kind: "client-result".into(),
                context_sha256: ctx.digest()?,
                role: role.into(),
                identity_ready_sha256: streaming_records::pins(&ids)?,
                server_ready_sha256: matrix::pins(&all_servers)?,
                pairs: Vec::new(),
                failed_cells: Vec::new(),
                interruption: None,
                edges: Some(evidence.clone()),
                complete: true,
            };
            validate_client(&result, &ctx, &ids, &all_servers, role)?;
            diagnostic.enter(Stage::ClientResultPublish);
            records::write(
                &o.output.join(format!("{role}-client-result.json")),
                &result,
            )?;
            diagnostic.enter(Stage::RemoteClientBarrier);
            clients(&o, &ctx, &ids, &all_servers, traffic_by).await?;
            diagnostic.enter(Stage::ServerConfirmation);
            super::edges::confirm_server(
                &evidence,
                &super::edge_signals::Cohort {
                    context: &ctx,
                    identities: &ids,
                    role,
                    ready: &all_servers[role],
                    client: &ep,
                    server: &server_ep,
                    deadline: traffic_by,
                },
            )
            .await?;
            edges = Some(evidence);
            return Ok(());
        }
        if let Some(put) = interruption_direction(&ctx.cohort) {
            diagnostic.enter(Stage::InterruptionExecute);
            let server = runtime.server.as_mut().ok_or_else(bad)?;
            let server_ep = Endpoint {
                daemon: std::sync::Arc::new(tokio::sync::Mutex::new(&mut server.child)),
                socket: &server.socket,
                environment: &environment,
                workspace: &o.workspace,
            };
            let evidence = super::copy_interruption_cases::run(
                super::copy_interruption_cases::Cohort {
                    context: &ctx,
                    identities: &ids,
                    role,
                    ready: &all_servers[role],
                    client: &ep,
                    server: &server_ep,
                    deadline: traffic_by
                        .checked_sub(Duration::from_secs(60))
                        .ok_or_else(bad)?,
                },
                put,
            )
            .await?;
            diagnostic.enter(Stage::CompletedResultValidation);
            let result = ClientResult {
                schema: 1,
                kind: "client-result".into(),
                context_sha256: ctx.digest()?,
                role: role.into(),
                identity_ready_sha256: streaming_records::pins(&ids)?,
                server_ready_sha256: matrix::pins(&all_servers)?,
                pairs: Vec::new(),
                failed_cells: Vec::new(),
                interruption: Some(evidence.clone()),
                edges: None,
                complete: true,
            };
            validate_client(&result, &ctx, &ids, &all_servers, role)?;
            diagnostic.enter(Stage::ClientResultPublish);
            records::write(
                &o.output.join(format!("{role}-client-result.json")),
                &result,
            )?;
            diagnostic.enter(Stage::RemoteClientBarrier);
            clients(&o, &ctx, &ids, &all_servers, traffic_by).await?;
            diagnostic.enter(Stage::ServerConfirmation);
            super::copy_interruption_cases::confirm_server(
                &evidence,
                &server_ep,
                &ids[role].client_account,
                &ids[role].server_account,
                traffic_by,
            )
            .await?;
            interruption = Some(evidence);
            return Ok(());
        }
        diagnostic.enter(Stage::LocalCells);
        let until = streaming_cases::cell_deadline(traffic_by);
        let (a, b, c) = tokio::join!(
            cell(
                &ctx,
                &ids,
                role,
                ROLES[0],
                &all_servers[ROLES[0]],
                &ep,
                until
            ),
            cell(
                &ctx,
                &ids,
                role,
                ROLES[1],
                &all_servers[ROLES[1]],
                &ep,
                until
            ),
            cell(
                &ctx,
                &ids,
                role,
                ROLES[2],
                &all_servers[ROLES[2]],
                &ep,
                until
            )
        );
        let mut pairs = Vec::new();
        let mut failed_cells = Vec::new();
        for (server, outcome) in ROLES.into_iter().zip([a, b, c]) {
            match outcome {
                Ok(completed) => pairs.extend(completed),
                Err(error) => {
                    for case in streaming_records::cases(&ctx.cohort)? {
                        failed_cells.push(streaming_records::FailedCell {
                            server: server.into(),
                            case: (*case).into(),
                            status: "unqualified".into(),
                            category: failure_category(&error).into(),
                        });
                    }
                }
            }
        }
        diagnostic.enter(Stage::CompletedResultValidation);
        let complete = failed_cells.is_empty();
        let result = ClientResult {
            schema: 1,
            kind: "client-result".into(),
            context_sha256: ctx.digest()?,
            role: role.into(),
            identity_ready_sha256: streaming_records::pins(&ids)?,
            server_ready_sha256: matrix::pins(&all_servers)?,
            pairs,
            failed_cells,
            interruption: None,
            edges: None,
            complete,
        };
        if complete {
            validate_client(&result, &ctx, &ids, &all_servers, role)?;
        }
        diagnostic.enter(Stage::ClientResultPublish);
        records::write(
            &o.output.join(format!("{role}-client-result.json")),
            &result,
        )?;
        if !complete {
            diagnostic.enter(Stage::CellsIncomplete);
            return Err(bad());
        }
        diagnostic.enter(Stage::RemoteClientBarrier);
        let all_clients = clients(&o, &ctx, &ids, &all_servers, traffic_by).await?;
        diagnostic.enter(Stage::ServerReceipts);
        receipt_records = collect_receipts(
            &ctx,
            &ids,
            role,
            &all_clients,
            runtime.server.as_mut().ok_or_else(bad)?,
            &executable,
            traffic_by,
        )
        .await?;
        Ok::<_, Error>(())
    };
    let (origin, result) = tokio::select! {
        r = tokio::time::timeout_at(traffic_by, work) => match r {
            Ok(result) => (Origin::WorkError, result),
            Err(_) => (Origin::WorkDeadline, Err(Error::Timeout)),
        },
        _ = termination.recv() => (Origin::ExternalSignal, Err(Error::Invalid("installed_interrupted"))),
    };
    diagnostic.origin(origin);
    if let Err(error) = &result {
        diagnostic.emit(error, &super::streaming_observation::stderr_sink);
    }
    diagnostic.origin(Origin::PostWorkError);
    diagnostic.enter(Stage::PostWorkFallback);
    let client_path = o.output.join(format!("{role}-client-result.json"));
    let failed_client_record = (|| -> Result<()> {
        if !client_path.try_exists()? {
            records::write(
                &client_path,
                &ClientResult {
                    schema: 1,
                    kind: "client-result".into(),
                    context_sha256: ctx.digest()?,
                    role: role.into(),
                    identity_ready_sha256: streaming_records::pins(&ids)?,
                    server_ready_sha256: BTreeMap::new(),
                    pairs: Vec::new(),
                    interruption: None,
                    edges: None,
                    failed_cells: ROLES
                        .into_iter()
                        .flat_map(|server| {
                            cases.iter().map(move |case| streaming_records::FailedCell {
                                server: server.into(),
                                case: (*case).into(),
                                status: "skipped".into(),
                                category: "cohort_incomplete".into(),
                            })
                        })
                        .collect(),
                    complete: false,
                },
            )?;
        }
        Ok(())
    })();
    if super::streaming_inspection::eligible(&ctx.cohort, &result) {
        super::streaming_inspection::failure(
            runtime.client.as_mut(),
            runtime.server.as_mut(),
            &ids,
            role,
            stop_by,
            inspection_origin,
            &mut termination,
        )
        .await;
    }
    // Cleanup must run even when publishing failure evidence itself fails.
    let (clean, server, client) = runtime.close(stop_by).await;
    failed_client_record?;
    diagnostic.enter(Stage::PostWorkCompletion);
    let complete = result.is_ok()
        && clean
        && server
        && client
        && if super::edges::is_edge(&ctx.cohort) {
            edges.is_some() && interruption.is_none() && receipt_records.is_empty()
        } else if interruption_direction(&ctx.cohort).is_some() {
            interruption.is_some() && receipt_records.is_empty()
        } else {
            receipt_records.len() == 3 * streaming_records::cases(&ctx.cohort)?.len()
        };
    diagnostic.enter(Stage::ServerResultPublish);
    let report = ServerResult {
        schema: 1,
        kind: "server-result".into(),
        context_sha256: ctx.digest()?,
        role: role.into(),
        identity_ready_sha256: streaming_records::pins(&ids)?,
        server_ready_sha256: ready_hash,
        receipts: receipt_records,
        interruption,
        edges,
        stream_cleanup_complete: clean,
        server_stop_complete: server,
        client_stop_complete: client,
        complete,
    };
    records::write(
        &o.output.join(format!("{role}-server-result.json")),
        &report,
    )?;
    diagnostic.enter(Stage::PostWorkCompletion);
    if complete { Ok(()) } else { Err(bad()) }
}

fn validate_receipt(
    r: &ServerReceipt,
    pair: &streaming_records::Pair,
    server: &ServerReady,
) -> Result<()> {
    if r.stream != pair.stream
        || r.session != pair.session
        || r.open_digest != pair.open_digest
        || r.group_sha256 != pair.group_sha256
        || r.case != pair.case
        || r.controls
            != pair
                .transfer
                .as_ref()
                .map(|t| t.controls.clone())
                .unwrap_or_default()
    {
        return Err(bad());
    }
    if pair.case == "duplex" {
        let fixture = r.fixture.as_ref().ok_or_else(bad)?;
        if r.transfer.is_some()
            || fixture.schema != 1
            || fixture.nonce != pair.nonce
            || fixture.client != pair.client
            || fixture.case != "duplex"
            || fixture.executable_sha256 != server.fixture_sha256
            || fixture.argv_sha256 != streaming_fixture::argument_digest(&pair.nonce, &pair.client)?
            || fixture.stdin_bytes != pair.stdin_bytes
            || fixture.stdin_sha256 != pair.stdin_sha256
            || fixture.stdout_bytes != pair.stdout_bytes
            || fixture.stdout_sha256 != pair.stdout_sha256
            || fixture.stderr_bytes != pair.stderr_bytes
            || fixture.stderr_sha256 != pair.stderr_sha256
            || fixture.exit != pair.exit
        {
            return Err(bad());
        }
    } else {
        let receipt = r.transfer.as_ref().ok_or_else(bad)?;
        let observed = pair.transfer.as_ref().ok_or_else(bad)?;
        if r.fixture.is_some()
            || receipt.schema != 1
            || receipt.nonce != pair.nonce
            || receipt.client != pair.client
            || receipt.case != pair.case
            || receipt.executable_sha256 != server.fixture_sha256
            || receipt.items != observed.items
            || receipt.checkpoint
                != if pair.case == "put" {
                    observed.checkpoint.clone()
                } else {
                    None
                }
            || records::canonical(&observed.rsync)? != records::canonical(&server.rsync)?
        {
            return Err(bad());
        }
    }
    Ok(())
}

const ROLLUP_FAILURE: &str = "matrix-rollup.failure.json";
struct RenderedRollup {
    json: Vec<u8>,
    markdown: Vec<u8>,
}
pub(super) fn rollup(
    context: &Path,
    directory: &Path,
    job_result: &str,
    output: &Path,
) -> Result<()> {
    // Invalid context cannot produce a trustworthy context-bound failure marker.
    let ctx: Context = records::read(context, 65536)?;
    ctx.validate()?;
    let mut category = "publication_failure";
    let result = (|| {
        crate::identity::check_dir(output)?;
        if fs::read_dir(output)?.next().is_some() {
            category = "output_not_empty";
            return Err(bad());
        }
        let report = build_rollup(&ctx, directory, job_result).inspect_err(|_| {
            category = "validation_or_platform_failure";
        })?;
        records::write_raw(&output.join("matrix-rollup.md"), &report.markdown)?;
        // JSON is the final success commit marker; errors can still occur after linking.
        records::write_raw(&output.join("matrix-rollup.json"), &report.json)
    })();
    if result.is_err() {
        publish_rollup_failure(&ctx, directory, output, category);
    }
    result
}
fn publish_rollup_failure(ctx: &Context, directory: &Path, output: &Path, category: &str) {
    let missing: Vec<_> = ROLES
        .into_iter()
        .flat_map(|role| {
            [
                "identity-ready",
                "server-ready",
                "client-result",
                "server-result",
            ]
            .into_iter()
            .map(move |kind| format!("{role}-{kind}.json"))
        })
        .filter(|name| !directory.join(name).is_file())
        .collect();
    let category = if category == "validation_or_platform_failure" && !missing.is_empty() {
        "missing_platform_records"
    } else {
        category
    };
    let Ok(digest) = ctx.digest() else {
        return;
    };
    let report = serde_json::json!({"schema":1,"scope":"installed-streaming","cohort":ctx.cohort,"context_sha256":digest,"source_commit":ctx.source_commit,"complete":false,"status":"FAIL","category":category,"missing_records":missing,"passed_cells":0,"qualification":"none"});
    // Failure always takes precedence over a visible PASS left by a post-link error.
    // Preserve the original error even if storage also prevents this marker.
    let _ = records::write(&output.join(ROLLUP_FAILURE), &report);
    let _ = records::write(&output.join("matrix-rollup.json"), &report);
    let _ = records::write_raw(&output.join("matrix-rollup.md"),format!("# Installed streaming qualification failed\n\nCategory: {category}. No platform qualification is claimed.\n").as_bytes());
}
fn build_rollup(ctx: &Context, directory: &Path, job_result: &str) -> Result<RenderedRollup> {
    if !super::edges::is_edge(&ctx.cohort) {
        streaming_records::cases(&ctx.cohort)?;
    }
    let ids = records::matrix_identities(directory, ctx)?;
    let mut servers = BTreeMap::new();
    for role in ROLES {
        let s: ServerReady =
            records::read(&directory.join(format!("{role}-server-ready.json")), 65536)?;
        s.validate(ctx, &ids, role)?;
        servers.insert(role.into(), s);
    }
    common_budget(&servers)?;
    let mut client_results = BTreeMap::new();
    for role in ROLES {
        let c: ClientResult = records::read(
            &directory.join(format!("{role}-client-result.json")),
            1048576,
        )?;
        validate_client(&c, ctx, &ids, &servers, role)?;
        client_results.insert(role, c);
    }
    let mut cells = Vec::new();
    for server in ROLES {
        let report: ServerResult = records::read(
            &directory.join(format!("{server}-server-result.json")),
            1048576,
        )?;
        if report.schema != 1
            || report.kind != "server-result"
            || report.role != server
            || report.context_sha256 != ctx.digest()?
            || report.identity_ready_sha256 != streaming_records::pins(&ids)?
            || report.server_ready_sha256 != records::digest(&records::canonical(&servers[server])?)
            || !report.complete
            || !report.stream_cleanup_complete
            || !report.server_stop_complete
            || !report.client_stop_complete
        {
            return Err(bad());
        }
        if super::edges::is_edge(&ctx.cohort) {
            let proof = report.edges.as_ref().ok_or_else(bad)?;
            let client = client_results[server].edges.as_ref().ok_or_else(bad)?;
            proof.validate(&ctx.cohort)?;
            if !report.receipts.is_empty()
                || report.interruption.is_some()
                || records::canonical(proof)? != records::canonical(client)?
            {
                return Err(bad());
            }
            for (case, wall_ns) in proof.metrics() {
                cells.push(serde_json::json!({"client":server,"server":server,"case":case,"passed":true,"wall_ns":wall_ns,"payload_bytes":0,"payload_bytes_per_second":0.0,"payload_goodput_measured":false}));
            }
            continue;
        }
        if report.edges.is_some() {
            return Err(bad());
        }
        if interruption_direction(&ctx.cohort).is_some() {
            let proof = report.interruption.as_ref().ok_or_else(bad)?;
            let client = client_results[server]
                .interruption
                .as_ref()
                .ok_or_else(bad)?;
            proof.validate()?;
            if !report.receipts.is_empty()
                || records::canonical(proof)? != records::canonical(client)?
            {
                return Err(bad());
            }
            let resumed_bytes = proof.committed.size - proof.resume_prefix.durable_offset;
            cells.push(serde_json::json!({"client":server,"server":server,"case":ctx.cohort,"passed":true,
                "wall_ns":proof.wall_ns,"payload_bytes":proof.committed.size,
                "payload_bytes_per_second":proof.committed.size as f64*1e9/proof.wall_ns as f64,
                "resume_wall_ns":proof.resume_wall_ns,"resume_payload_bytes":resumed_bytes,
                "resume_payload_bytes_per_second":resumed_bytes as f64*1e9/proof.resume_wall_ns as f64,
                "durable_prefix_observed_while_live":proof.partial.durable_offset,
                "durable_prefix_before_resume":proof.resume_prefix.durable_offset}));
            continue;
        }
        if report.interruption.is_some()
            || report.receipts.len() != 3 * streaming_records::cases(&ctx.cohort)?.len()
        {
            return Err(bad());
        }
        for client in ROLES {
            for case in streaming_records::cases(&ctx.cohort)? {
                let pair = client_results[client]
                    .pairs
                    .iter()
                    .find(|p| p.server == server && p.case == *case)
                    .ok_or_else(bad)?;
                let matches: Vec<_> = report
                    .receipts
                    .iter()
                    .filter(|r| r.client == client && r.case == *case)
                    .collect();
                if matches.len() != 1 {
                    return Err(bad());
                }
                validate_receipt(matches[0], pair, &servers[server])?;
                let bytes = pair.transfer.as_ref().map_or(
                    (pair.stdin_bytes + pair.stdout_bytes + pair.stderr_bytes) as u64,
                    |t| t.payload_bytes,
                );
                cells.push(serde_json::json!({"client":client,"server":server,"case":case,"passed":true,"wall_ns":pair.wall_ns,"payload_bytes":bytes,"payload_bytes_per_second":bytes as f64*1e9/pair.wall_ns as f64}));
            }
        }
    }
    if job_result != "success" {
        return Err(bad());
    }
    let report = serde_json::json!({"schema":1,"scope":"installed-streaming","cohort":ctx.cohort,"context_sha256":ctx.digest()?,"artifact_prefix":ctx.prefix(),"source_commit":ctx.source_commit,
        "candidate_run_id":ctx.candidate_run_id,"candidate_run_attempt":ctx.candidate_run_attempt,"complete":true,"passed_cells":cells.len(),"expected_cells":if super::edges::is_edge(&ctx.cohort){3*match ctx.cohort.as_str(){"edge-stream"=>9,"edge-rsync"=>6,"edge-copy"=>22,_=>return Err(bad())}}else if interruption_direction(&ctx.cohort).is_some(){3}else{9*streaming_records::cases(&ctx.cohort)?.len()},"samples_per_directed_case":1,"edge_case_observations":match ctx.cohort.as_str(){"edge-stream"=>27,"edge-rsync"=>18,"edge-copy"=>60,_=>0},"edge_receipt_seed_records":if ctx.cohort=="edge-copy"{6}else{0},"edge_relay_cli_attempts":match ctx.cohort.as_str(){"edge-stream"=>42,"edge-rsync"=>18,"edge-copy"=>66,_=>0},"measured_cli_operations":if super::edges::is_edge(&ctx.cohort){3*match ctx.cohort.as_str(){"edge-stream"=>15,"edge-rsync"=>6,"edge-copy"=>22,_=>return Err(bad())}}else{match interruption_direction(&ctx.cohort){Some(true)=>cells.len()*3,Some(false)=>cells.len()*2,None=>cells.len()}},"fixture_control_cli_operations":if super::edges::is_edge(&ctx.cohort) || ctx.cohort == "stream-matrix" || interruption_direction(&ctx.cohort).is_some() {0} else {cells.len()*2},"administrative_cancel_operations":if interruption_direction(&ctx.cohort).is_some(){cells.len()}else{0},"cells":cells,
        "metric_definitions":{"wall_ns":if super::edges::is_edge(&ctx.cohort){"Complete monotonic functional edge-case lifetime, including local same-UID observations, cancellation and cleanup; no payload goodput or RTT measured"}else if interruption_direction(&ctx.cohort).is_some(){"Monotonic interruption workflow including source creation, detach checkpoint, owned kill, remote cancel and verified resume; not RTT"}else{"Monotonic installed CLI plus subsequent journal and terminal observations, including transport, command and output; excludes later independent receipt and cleanup validation; not RTT"},"resume_wall_ns":"Installed resume CLI lifetime including immutable-source revalidation and transfer; not RTT","payload_bytes_per_second":"Validated application payload bytes divided by wall time; finite transfer modes use verified file bytes"},
        "limitations":["One sample per directed case; no percentile estimates","Candidate qualification does not publish or qualify a public release"]});
    render_rollup(&report)
}
fn render_rollup(report: &serde_json::Value) -> Result<RenderedRollup> {
    let mut markdown = String::from(
        "# Installed streaming matrix\n\nThis Markdown is non-authoritative. PASS requires a green job from the current run and attempt, complete:true in matrix-rollup.json, and no matrix-rollup.failure.json.\n\nOne sample per directed case (nine pairs for matrices, three same-platform interruption cases). Matrix wall time includes installed CLI startup, encrypted transport, execution, output delivery and subsequent journal and terminal observations; it excludes later independent receipt and cleanup validation. Interruption wall time includes the complete detach/cancel/resume workflow. Edge-case wall time includes scenario observations and cleanup, not request-to-stop latency. None is network RTT. Speed uses validated application bytes (verified file bytes for transfers). No percentile estimates.\n\n| Client | Server | Case | Wall (ms) | Payload (bytes) | Speed (bytes/s) |\n| --- | --- | --- | ---: | ---: | ---: |\n",
    );
    for cell in report["cells"].as_array().ok_or_else(bad)? {
        use std::fmt::Write;
        let speed = if cell["payload_goodput_measured"] == false {
            "N/A".to_string()
        } else {
            format!(
                "{:.3}",
                cell["payload_bytes_per_second"].as_f64().ok_or_else(bad)?
            )
        };
        writeln!(
            markdown,
            "| {} | {} | {} | {:.3} | {} | {} |",
            cell["client"].as_str().ok_or_else(bad)?,
            cell["server"].as_str().ok_or_else(bad)?,
            cell["case"].as_str().ok_or_else(bad)?,
            cell["wall_ns"].as_u64().ok_or_else(bad)? as f64 / 1e6,
            cell["payload_bytes"].as_u64().ok_or_else(bad)?,
            speed
        )
        .map_err(|_| bad())?;
    }
    markdown.push_str(
        "\nCandidate qualification only; no public release was published by this test.\n",
    );
    let json = records::canonical(report)?;
    if json.len() as u64 > records::RECORD_LIMIT || markdown.len() as u64 > records::RECORD_LIMIT {
        return Err(bad());
    }
    Ok(RenderedRollup {
        json,
        markdown: markdown.into_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::super::records::{FilePin, Installation};
    use super::*;
    use streaming_records::{Pair, ServerResult};

    fn evidence() -> (
        Context,
        BTreeMap<String, IdentityReady>,
        BTreeMap<String, ServerReady>,
    ) {
        let mut ctx = records::tests::ctx();
        ctx.cohort = "stream-matrix".into();
        ctx.expires_at = ctx.created_at + 3300;
        let ids: BTreeMap<_, _> = ROLES
            .iter()
            .enumerate()
            .map(|(i, role)| {
                let identity = IdentityReady {
                    schema: 1,
                    kind: "identity-ready".into(),
                    context_sha256: ctx.digest().unwrap(),
                    role: (*role).into(),
                    nonce: format!("{:064x}", i + 1),
                    client_account: format!("{:064x}", i * 2 + 100),
                    server_account: format!("{:064x}", i * 2 + 101),
                    identity_ready_at: ctx.created_at,
                    installation: Installation {
                        version: ctx.version.clone(),
                        target: TARGETS[i].into(),
                        source_commit: ctx.source_commit.clone(),
                        manifest_sha256: ctx.manifests[*role].clone(),
                        bootstrap_sha256: ctx.bootstrap_sha256.clone(),
                        files: crate::native_manifest::NAMES
                            .into_iter()
                            .map(|n| {
                                (
                                    n.into(),
                                    FilePin {
                                        sha256: "aa".repeat(32),
                                        bytes: 1,
                                    },
                                )
                            })
                            .collect(),
                    },
                };
                identity.validate(&ctx, role).unwrap();
                ((*role).into(), identity)
            })
            .collect();
        let budget = streaming_records::budget(&ctx, ctx.created_at, ctx.created_at).unwrap();
        let servers = ROLES
            .iter()
            .enumerate()
            .map(|(i, role)| {
                let ready = ServerReady {
                    schema: 1,
                    kind: "server-ready".into(),
                    context_sha256: ctx.digest().unwrap(),
                    role: (*role).into(),
                    identity_ready_sha256: streaming_records::pins(&ids).unwrap(),
                    server_account: ids[*role].server_account.clone(),
                    started_at: ctx.created_at,
                    ready_at: ctx.created_at + 1,
                    ready_by: budget.ready_by,
                    traffic_by: budget.traffic_by,
                    stop_by: budget.stop_by,
                    fixture_sha256: ctx.tooling[i].files.fixture.sha256.clone(),
                    root_label: "transfer-v1".into(),
                    rsync: None,
                };
                ready.validate(&ctx, &ids, role).unwrap();
                ((*role).into(), ready)
            })
            .collect();
        (ctx, ids, servers)
    }
    #[tokio::test]
    async fn rendezvous_diagnostics_distinguish_read_validation_and_wait_without_peer_blame() {
        for (mode, expected_stage, expected_peer) in [
            ("json", "server_record_read", Some(ROLES[0])),
            ("binding", "server_record_validate", Some(ROLES[0])),
            ("missing", "server_rendezvous_wait", None),
            ("pending", "server_rendezvous_wait", None),
        ] {
            let root = tempfile::tempdir().unwrap();
            let (ctx, ids, mut records) = evidence();
            let options = Run {
                context: root.path().join("context"),
                workspace: root.path().join("workspace"),
                identities: root.path().join("identities"),
                inbox: root.path().into(),
                output: root.path().join("output"),
            };
            let path = root.path().join(format!("{}-server-ready.json", ROLES[0]));
            match mode {
                "json" => fs::write(&path, b"{invalid-json").unwrap(),
                "pending" => {
                    fs::write(&path, b"not parsed while linked").unwrap();
                    fs::hard_link(&path, root.path().join("staging")).unwrap();
                }
                "binding" => {
                    records.get_mut(ROLES[0]).unwrap().root_label = "wrong-root".into();
                    fs::write(&path, records::canonical(&records[ROLES[0]]).unwrap()).unwrap();
                }
                _ => {}
            }
            let mut diagnostic = Tracker::new();
            diagnostic.origin(Origin::WorkError);
            let error = match servers(&options, &ctx, &ids, Instant::now(), &mut diagnostic).await {
                Ok(_) => panic!("invalid rendezvous unexpectedly succeeded"),
                Err(error) => error,
            };
            let bytes = std::cell::RefCell::new(Vec::new());
            diagnostic.emit(&error, &|line| {
                bytes.borrow_mut().extend_from_slice(line);
                Ok(())
            });
            let bytes = bytes.into_inner();
            let prefix = b"marmish installed diagnostic: ";
            let value: serde_json::Value =
                serde_json::from_slice(&bytes[prefix.len()..bytes.len() - 1]).unwrap();
            assert_eq!(value["stage"], expected_stage);
            assert_eq!(value["peer_role"].as_str(), expected_peer);
            assert_eq!(value["origin"], "work_error");
        }
    }

    fn pair(
        ctx: &Context,
        ids: &BTreeMap<String, IdentityReady>,
        client: &str,
        server: &str,
    ) -> Pair {
        let nonce = streaming_records::nonce(ctx, ids, client, server, "duplex").unwrap();
        Pair {
            client: client.into(),
            server: server.into(),
            case: "duplex".into(),
            nonce: nonce.clone(),
            stream: records::digest(format!("stream:{client}:{server}").as_bytes()),
            session: records::digest(format!("session:{client}:{server}").as_bytes()),
            open_digest: records::digest(format!("open:{client}:{server}").as_bytes()),
            group_sha256: records::digest(format!("group:{client}:{server}").as_bytes()),
            stdin_bytes: streaming_fixture::STDIN_BYTES,
            stdin_sha256: records::digest(
                &streaming_fixture::bytes(&nonce, 7, streaming_fixture::STDIN_BYTES).unwrap(),
            ),
            stdout_bytes: streaming_fixture::STDOUT_BYTES,
            stdout_sha256: records::digest(
                &streaming_fixture::bytes(&nonce, 43, streaming_fixture::STDOUT_BYTES).unwrap(),
            ),
            stderr_bytes: streaming_fixture::STDERR_BYTES,
            stderr_sha256: records::digest(
                &streaming_fixture::bytes(&nonce, 149, streaming_fixture::STDERR_BYTES).unwrap(),
            ),
            exit: 7,
            wall_ns: 1_000_000,
            complete: true,
            failure: None,
            transfer: None,
        }
    }
    #[test]
    fn missing_platform_records_produce_closed_sanitized_failure_report() {
        let (ctx, _, _) = evidence();
        let temp = crate::journal::private_test_directory();
        let input = temp.path().join("input");
        let output = temp.path().join("output");
        fs::DirBuilder::new().mode(0o700).create(&input).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&output).unwrap();
        let context = temp.path().join("context.json");
        records::write(&context, &ctx).unwrap();
        assert!(rollup(&context, &input, "failure", &output).is_err());
        let value: serde_json::Value =
            records::read(&output.join("matrix-rollup.json"), 65536).unwrap();
        assert_eq!(value["complete"], false);
        assert_eq!(value["category"], "missing_platform_records");
        assert_eq!(value["missing_records"].as_array().unwrap().len(), 12);
        assert_eq!(value["passed_cells"], 0);
        assert_eq!(value["qualification"], "none");
        assert!(
            !records::canonical(&value)
                .unwrap()
                .windows(temp.path().as_os_str().len())
                .any(|w| w == temp.path().as_os_str().as_encoded_bytes())
        );
    }
    pub(super) fn rollup_fixture() -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let (ctx, ids, servers) = evidence();
        let temp = crate::journal::private_test_directory();
        let input = temp.path().join("input");
        let output = temp.path().join("output");
        fs::DirBuilder::new().mode(0o700).create(&input).unwrap();
        fs::DirBuilder::new().mode(0o700).create(&output).unwrap();
        let context = temp.path().join("context.json");
        records::write(&context, &ctx).unwrap();
        for role in ROLES {
            records::write(
                &input.join(format!("{role}-identity-ready.json")),
                &ids[role],
            )
            .unwrap();
            records::write(
                &input.join(format!("{role}-server-ready.json")),
                &servers[role],
            )
            .unwrap();
            let result = ClientResult {
                schema: 1,
                kind: "client-result".into(),
                context_sha256: ctx.digest().unwrap(),
                role: role.into(),
                identity_ready_sha256: streaming_records::pins(&ids).unwrap(),
                server_ready_sha256: matrix::pins(&servers).unwrap(),
                interruption: None,
                edges: None,
                pairs: ROLES
                    .into_iter()
                    .map(|s| pair(&ctx, &ids, role, s))
                    .collect(),
                failed_cells: Vec::new(),
                complete: true,
            };
            validate_client(&result, &ctx, &ids, &servers, role).unwrap();
            records::write(&input.join(format!("{role}-client-result.json")), &result).unwrap();
            let receipts = ROLES
                .into_iter()
                .map(|client| {
                    let p = pair(&ctx, &ids, client, role);
                    ServerReceipt {
                        client: client.into(),
                        case: "duplex".into(),
                        stream: p.stream,
                        session: p.session,
                        open_digest: p.open_digest,
                        group_sha256: p.group_sha256,
                        fixture: Some(
                            streaming_fixture::receipt(
                                Path::new("unused"),
                                &servers[role].fixture_sha256,
                                &p.nonce,
                                client,
                            )
                            .unwrap(),
                        ),
                        transfer: None,
                        controls: Vec::new(),
                    }
                })
                .collect();
            let server = ServerResult {
                schema: 1,
                kind: "server-result".into(),
                context_sha256: ctx.digest().unwrap(),
                role: role.into(),
                identity_ready_sha256: streaming_records::pins(&ids).unwrap(),
                server_ready_sha256: records::digest(&records::canonical(&servers[role]).unwrap()),
                receipts,
                interruption: None,
                edges: None,
                stream_cleanup_complete: true,
                server_stop_complete: true,
                client_stop_complete: true,
                complete: true,
            };
            records::write(&input.join(format!("{role}-server-result.json")), &server).unwrap();
        }
        (temp, context, input, output)
    }
    #[test]
    fn rollup_recomputes_all_nine_bindings_and_literal_argument_receipts() {
        let (_temp, context, input, output) = rollup_fixture();
        rollup(&context, &input, "success", &output).unwrap();
        let report: serde_json::Value =
            records::read(&output.join("matrix-rollup.json"), 1048576).unwrap();
        assert_eq!(report["passed_cells"], 9);
        assert!(!output.join(ROLLUP_FAILURE).exists());
        assert_eq!(fs::read_dir(&output).unwrap().count(), 2);
        // An independently changed literal argument digest must invalidate the rollup.
        let path = input.join(format!("{}-server-result.json", ROLES[0]));
        let mut forged: ServerResult = records::read(&path, 1048576).unwrap();
        forged.receipts[0].fixture.as_mut().unwrap().argv_sha256 = "00".repeat(32);
        fs::write(&path, records::canonical(&forged).unwrap()).unwrap();
        let forged_output = _temp.path().join("forged-output");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&forged_output)
            .unwrap();
        assert!(rollup(&context, &input, "success", &forged_output).is_err());
        let failure: serde_json::Value =
            records::read(&forged_output.join(ROLLUP_FAILURE), 65536).unwrap();
        assert_eq!(failure["category"], "validation_or_platform_failure");
    }
    #[test]
    fn rollup_publication_faults_are_authoritatively_failed_without_replacing_pass() {
        use records::{PublicationFault, PublicationFaultStage};
        for (name, stage, pass_remains) in [
            ("matrix-rollup.md", PublicationFaultStage::BeforeLink, false),
            (
                "matrix-rollup.json",
                PublicationFaultStage::AfterLinkErr,
                true,
            ),
        ] {
            let (_temp, context, input, output) = rollup_fixture();
            let _fault = PublicationFault::install(name, stage);
            let error = rollup(&context, &input, "success", &output).unwrap_err();
            assert!(matches!(error, Error::Io(_)));
            let marker: serde_json::Value =
                records::read(&output.join(ROLLUP_FAILURE), 65536).unwrap();
            assert_eq!(marker["status"], "FAIL");
            assert_eq!(marker["category"], "publication_failure");
            let json: serde_json::Value =
                records::read(&output.join("matrix-rollup.json"), 1048576).unwrap();
            assert_eq!(json["complete"], pass_remains);
            let md = fs::read_to_string(output.join("matrix-rollup.md")).unwrap();
            if pass_remains {
                assert!(md.contains("non-authoritative"));
            }
            let mut names: Vec<_> = fs::read_dir(&output)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            assert_eq!(names.len(), 3);
            assert!(
                names
                    .iter()
                    .all(|n| !n.to_string_lossy().starts_with(".record-"))
            );
        }
    }
    #[test]
    fn invalid_context_and_failed_job_cannot_publish_pass() {
        let (_temp, context, input, output) = rollup_fixture();
        let error = rollup(&context, &input, "failure", &output).unwrap_err();
        assert!(matches!(error, Error::Invalid("installed_streaming")));
        let marker: serde_json::Value = records::read(&output.join(ROLLUP_FAILURE), 65536).unwrap();
        assert_eq!(marker["category"], "validation_or_platform_failure");
        let json: serde_json::Value =
            records::read(&output.join("matrix-rollup.json"), 65536).unwrap();
        assert_eq!(json["complete"], false);
        let (_temp, context, input, output) = rollup_fixture();
        fs::write(&context, b"invalid context").unwrap();
        assert!(rollup(&context, &input, "success", &output).is_err());
        assert_eq!(fs::read_dir(&output).unwrap().count(), 0);
    }
    #[test]
    fn rollup_preserves_foreign_output_and_existing_failure_marker() {
        use std::os::unix::fs::MetadataExt;
        let (_temp, context, input, output) = rollup_fixture();
        let path = output.join("matrix-rollup.json");
        fs::write(&path, b"foreign bytes").unwrap();
        let before = fs::metadata(&path).unwrap();
        assert!(matches!(
            rollup(&context, &input, "success", &output),
            Err(Error::Invalid("installed_streaming"))
        ));
        assert_eq!(fs::read(&path).unwrap(), b"foreign bytes");
        assert_eq!(fs::metadata(&path).unwrap().ino(), before.ino());
        let marker = fs::read(output.join(ROLLUP_FAILURE)).unwrap();
        assert!(rollup(&context, &input, "success", &output).is_err());
        assert_eq!(fs::read(output.join(ROLLUP_FAILURE)).unwrap(), marker);
        let report: serde_json::Value = serde_json::from_slice(&marker).unwrap();
        assert_eq!(report["category"], "output_not_empty");
    }
    #[test]
    fn edge_markdown_does_not_present_unmeasured_goodput_as_speed() {
        let rendered = render_rollup(&serde_json::json!({"cells":[{
            "client":"linux-amd64","server":"linux-amd64","case":"stream-peer-quota",
            "wall_ns":1000000,"payload_bytes":0,"payload_bytes_per_second":0.0,
            "payload_goodput_measured":false
        }]}))
        .unwrap();
        let markdown = String::from_utf8(rendered.markdown).unwrap();
        assert!(markdown.contains("| 1.000 | 0 | N/A |"));
        assert!(!markdown.contains("| 0.000 |"));
    }
    #[test]
    fn common_barrier_uses_earliest_start_and_retains_cleanup_headroom() {
        let (_, _, mut servers) = evidence();
        let original = common_budget(&servers).unwrap();
        let late = servers.get_mut(ROLES[2]).unwrap();
        late.started_at += 30;
        late.ready_by += 30;
        late.traffic_by += 30;
        late.stop_by += 30;
        late.ready_at += 30;
        assert_eq!(common_budget(&servers).unwrap(), original);
        servers.get_mut(ROLES[2]).unwrap().ready_at = original.ready_by;
        assert!(common_budget(&servers).is_err());
    }
}

#[cfg(test)]
#[path = "installed_streaming_publication_tests.rs"]
mod publication_tests;
