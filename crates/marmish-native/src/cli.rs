use crate::command_cli::{self, Action, Context, Selection, Selector, StdinMode};
use crate::command_config::CommandConfig;
use crate::{
    Error, Result,
    daemon::{self, Config},
    foundation_check,
    identity::{BackendChoice, socket_path_for},
    ipc::{self, Request},
    transport::{RelayPolicy, RouteMode, TransportLimits},
};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

#[derive(Clone, Copy, ValueEnum)]
enum Backend {
    Auto,
    Keychain,
    File,
}

impl From<Backend> for BackendChoice {
    fn from(value: Backend) -> Self {
        match value {
            Backend::Auto => Self::Auto,
            Backend::Keychain => Self::Keychain,
            Backend::File => Self::File,
        }
    }
}

#[derive(Parser)]
#[command(
    version,
    about = "Marmish native identity and encrypted transport foundation"
)]
struct Args {
    #[arg(skip)]
    startup: Option<crate::daemon_launch::StartupReporter>,
    #[arg(skip)]
    trailing: Option<Vec<OsString>>,
    #[arg(skip)]
    rsh: Option<crate::rsync_cli::Invocation>,
    #[arg(skip)]
    authorize_disallowed: bool,
    #[arg(long, global = true)]
    debug: bool,
    /// Keep the daemon attached to the terminal or service supervisor.
    #[arg(long, global = true)]
    foreground: bool,
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    #[arg(short = 'i', long, default_value = "default", global = true)]
    identity: String,
    #[arg(long, global = true)]
    socket: Option<PathBuf>,
    #[arg(long, global = true)]
    relay: Vec<String>,
    #[arg(long, global = true)]
    tor: Option<String>,
    #[arg(long, value_enum, default_value = "auto", global = true)]
    key_storage: Backend,
    #[arg(long, global = true)]
    allow_peer: Vec<String>,
    #[arg(long, default_value_t = 86_400, global = true)]
    retention_seconds: u64,
    #[arg(short = 'u', long, global = true)]
    user: Option<String>,
    #[arg(long, global = true)]
    srv: Vec<String>,
    #[arg(long, global = true, conflicts_with = "stdin_none")]
    stdin: bool,
    #[arg(long, global = true)]
    stdin_none: bool,
    #[arg(long, global = true)]
    detach: bool,
    /// Open a fresh independent command room instead of reusing the default.
    #[arg(long, global = true)]
    new_session: bool,
    #[arg(long, default_value_t = 180, global = true, value_parser = clap::value_parser!(u64).range(1..=86400))]
    wait_seconds: u64,
    #[arg(long, global = true)]
    no_autostart: bool,
    #[arg(long, global = true)]
    max_commands: Option<String>,
    #[arg(long, global = true)]
    max_uptime: Option<String>,
    #[command(subcommand)]
    command: Option<FrontendCommand>,
}

#[derive(clap::Args)]
#[group(skip)]
#[command(group(clap::ArgGroup::new("selector").required(true).args(["handle", "handle_file", "invocation", "latest", "stream"])))]
struct SelectArgs {
    /// Cancel this locally recorded stream ID, including an uncertain attachment.
    #[arg(long)]
    stream: Option<String>,
    /// Cancel an owned server process without requiring a remote grant.
    #[arg(long, requires = "stream")]
    local: bool,
    /// Exact private handle JSON previously printed by marmish.
    #[arg(long)]
    handle: Option<String>,
    #[arg(long)]
    handle_file: Option<PathBuf>,
    #[arg(long)]
    invocation: Option<String>,
    #[arg(long)]
    latest: bool,
    #[arg(value_name = "DESTINATION")]
    destination: Option<String>,
}

#[derive(clap::Args)]
struct CopyArgs {
    /// Remote npub or configured alias, optionally followed by @relay.
    remote: String,
    /// Literal source path (local for put; remote for get).
    source: OsString,
    /// Literal destination path (remote for put; local for get).
    target: OsString,
    #[arg(long)]
    compress: bool,
    /// Resume this exact transfer with unchanged paths and preservation options.
    #[arg(long)]
    resume: Option<String>,
    #[arg(long)]
    overwrite: bool,
    #[arg(long)]
    preserve_mode: bool,
    #[arg(long)]
    preserve_mtime: bool,
    #[arg(long, default_value_t=660, value_parser=clap::value_parser!(u64).range(1..=86400))]
    idle_seconds: u64,
    #[arg(long, default_value_t=300, value_parser=clap::value_parser!(u64).range(1..=86400))]
    keepalive_seconds: u64,
    /// Capability probe deadline, no greater than the idle timeout.
    #[arg(long, default_value_t=15, value_parser=clap::value_parser!(u64).range(1..=86400))]
    probe_seconds: u64,
}

#[derive(Subcommand)]
enum FrontendCommand {
    /// Inspect retained local download receipts or acknowledge an uncertain record.
    CopyReceipts {
        #[arg(long, requires_all = ["revision", "acknowledge_retained_state"])]
        forget: Option<String>,
        #[arg(long, requires = "forget", value_parser = clap::value_parser!(u64).range(1..))]
        revision: Option<u64>,
        /// Remove only the index record; retained files require manual inspection.
        #[arg(long, requires = "forget")]
        acknowledge_retained_state: bool,
    },
    /// Copy one local regular file to a remote literal path.
    Put(CopyArgs),
    /// Copy one remote regular file to a local literal path.
    Get(CopyArgs),
    /// Rsync remote shell: rsync -s -e 'marmish rsh'. Global options go before rsh.
    Rsh,
    /// Execute with binary stdin/stdout streams and explicit remote cancellation.
    Stream {
        destination: String,
        #[arg(long)]
        compress: bool,
        #[arg(long, default_value_t = 660, value_parser = clap::value_parser!(u64).range(1..=86400))]
        idle_seconds: u64,
        #[arg(long, default_value_t = 300, value_parser = clap::value_parser!(u64).range(1..=86400))]
        keepalive_seconds: u64,
        #[arg(last = true, required = true)]
        argv: Vec<OsString>,
    },
    /// List locally recorded streams and their last known remote states.
    Streams {
        destination: Option<String>,
    },
    /// Acknowledge that an uncertain orphan may still be running, releasing credit.
    StreamReconcile {
        stream: String,
        #[arg(long, required = true)]
        acknowledge_possible_live: bool,
    },
    /// Ask macOS for access to this existing profile, then verify without UI.
    KeychainAuthorize,
    /// Print this binary's native version, build target and source commit.
    ReleaseInfo,
    /// Install a verified v0.2 bundle or select an already installed version.
    Install {
        #[arg(long, conflicts_with = "select")]
        source_dir: Option<PathBuf>,
        #[arg(long, requires = "source_dir")]
        manifest: Option<PathBuf>,
        #[arg(long, requires = "source_dir")]
        manifest_sha256: Option<String>,
        #[arg(long, conflicts_with = "source_dir")]
        select: Option<String>,
        #[arg(long)]
        prefix: PathBuf,
        #[arg(long)]
        bin_dir: PathBuf,
    },
    InstallStatus {
        #[arg(long)]
        prefix: PathBuf,
        #[arg(long)]
        bin_dir: PathBuf,
    },
    Uninstall {
        #[arg(long)]
        prefix: PathBuf,
        #[arg(long)]
        bin_dir: PathBuf,
    },
    /// Create or update this named identity and its offline command configuration.
    Setup {
        #[arg(long)]
        interactive: bool,
        #[arg(long, value_parser = ["client", "server", "both", "c", "s", "b"])]
        role: Option<String>,
        #[arg(long)]
        grant: Vec<String>,
        #[arg(long)]
        revoke: Vec<String>,
        #[arg(long)]
        trust: Vec<String>,
        #[arg(long)]
        untrust: Vec<String>,
        #[arg(long)]
        update: bool,
        #[arg(long)]
        no_grants: bool,
    },
    /// Initialize encrypted native storage without connecting to relays.
    Init,
    /// List durable command handles without command arguments or output.
    Commands {
        destination: Option<String>,
    },
    /// Query an explicitly selected remote invocation.
    CommandStatus(SelectArgs),
    /// Explicitly request cancellation of an invocation.
    Cancel(SelectArgs),
    /// Resume waiting and copying output using durable cursors.
    Wait(SelectArgs),
    Output(SelectArgs),
    /// Remote invocation: DESTINATION -- /absolute/program [arguments...].
    #[command(external_subcommand)]
    Remote(Vec<OsString>),
    /// Run this identity's serialized transport owner.
    Daemon,
    Status,
    #[command(hide = true)]
    CommandEvidence {
        #[arg(long)]
        after: Option<u64>,
        #[arg(long, default_value_t = 64)]
        limit: u32,
    },
    #[command(hide = true)]
    CommandCleanupStatus,
    #[command(hide = true)]
    CommandLocalCancel {
        cursor: u64,
    },
    #[command(hide = true)]
    CommandPrepareCleanup,
    OpenConversation {
        peer: String,
    },
    /// Send a bounded foundation test payload, encoded as canonical base64.
    Send {
        session: String,
        id: String,
        data: String,
    },
    Received {
        #[arg(long)]
        after: Option<u64>,
        #[arg(long, default_value_t = 1)]
        limit: usize,
    },
    /// Fresh-key encrypted exchange, restart, and receipt validation. No commands.
    FoundationCheck,
    /// Disposable-key live remote-command validation through explicit relays.
    #[command(name = "command-check")]
    ValidateExecution,
}

fn state_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path);
    }
    #[cfg(target_os = "linux")]
    if let Some(base) = std::env::var_os("XDG_DATA_HOME") {
        let base = PathBuf::from(base);
        if base.is_absolute() {
            return Ok(base.join("marmish/v2"));
        }
    }
    let home = PathBuf::from(std::env::var_os("HOME").ok_or(Error::Invalid("home_directory"))?);
    #[cfg(target_os = "macos")]
    return Ok(home.join("Library/Application Support/Marmish/v2"));
    #[cfg(not(target_os = "macos"))]
    Ok(home.join(".local/share/marmish/v2"))
}

fn is_daemon(args: &Args, daemon_binary: bool) -> bool {
    matches!(args.command, Some(FrontendCommand::Daemon))
        || (daemon_binary && args.command.is_none())
}

fn validate_daemon_options(args: &Args, daemon_binary: bool) -> Result<()> {
    if args.foreground && !is_daemon(args, daemon_binary) {
        return Err(Error::Invalid("foreground_operation"));
    }
    if args.new_session && is_daemon(args, daemon_binary) {
        return Err(Error::Invalid("new_session_operation"));
    }
    Ok(())
}

async fn run(
    mut args: Args,
    daemon_binary: bool,
    layout: &crate::native_layout::ExecutableLayout,
) -> Result<u8> {
    validate_daemon_options(&args, daemon_binary)?;
    if matches!(args.command, Some(FrontendCommand::KeychainAuthorize)) {
        let started = std::time::Instant::now();
        if daemon_binary || args.authorize_disallowed {
            return Err(Error::Invalid("keychain_authorize_arguments"));
        }
        return keychain_authorize(&args, layout, started).await;
    }
    if args.command.is_none() && daemon_binary {
        args.command = Some(FrontendCommand::Daemon);
    }
    if (args.max_commands.is_some() || args.max_uptime.is_some())
        && !matches!(args.command, Some(FrontendCommand::Daemon))
    {
        return Err(Error::Invalid("server_limit_operation"));
    }
    if args.new_session
        && !matches!(
            args.command,
            Some(
                FrontendCommand::Remote(_)
                    | FrontendCommand::Stream { .. }
                    | FrontendCommand::Put(_)
                    | FrontendCommand::Get(_)
            )
        )
    {
        return Err(Error::Invalid("new_session_operation"));
    }
    if matches!(args.command, Some(FrontendCommand::ReleaseInfo)) {
        println!(
            "{}",
            serde_json::to_string(&crate::native_manifest::compiled_release_metadata())
                .map_err(|_| Error::Invalid("report_encoding"))?
        );
        return Ok(0);
    }
    if let Some(action) = match &args.command {
        Some(FrontendCommand::Install {
            source_dir,
            manifest,
            manifest_sha256,
            select,
            prefix,
            bin_dir,
        }) => {
            let action = if let Some(version) = select {
                crate::native_install::InstallAction::Select {
                    version: version.clone(),
                }
            } else {
                crate::native_install::InstallAction::Install {
                    source_dir: source_dir
                        .clone()
                        .ok_or(Error::Invalid("install_arguments"))?,
                    manifest: manifest
                        .clone()
                        .ok_or(Error::Invalid("install_arguments"))?,
                    manifest_sha256: crate::native_manifest::hex32(
                        manifest_sha256
                            .as_deref()
                            .ok_or(Error::Invalid("install_arguments"))?,
                    )?,
                }
            };
            Some((action, prefix.clone(), bin_dir.clone()))
        }
        Some(FrontendCommand::InstallStatus { prefix, bin_dir }) => Some((
            crate::native_install::InstallAction::Status,
            prefix.clone(),
            bin_dir.clone(),
        )),
        Some(FrontendCommand::Uninstall { prefix, bin_dir }) => Some((
            crate::native_install::InstallAction::Uninstall,
            prefix.clone(),
            bin_dir.clone(),
        )),
        _ => None,
    } {
        let report = crate::native_install::run(crate::native_install::InstallOptions {
            action: action.0,
            prefix: action.1,
            bin_dir: action.2,
            state_dir: args.state_dir.clone(),
            debug: args.debug,
        })?;
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|_| Error::Invalid("report_encoding"))?
        );
        return Ok(0);
    }
    let mode: RouteMode = args.tor.as_deref().unwrap_or("auto").parse()?;
    if matches!(args.command, Some(FrontendCommand::ValidateExecution)) {
        let report = crate::mdk_adapter::command_check::run(args.relay, mode).await?;
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|_| Error::Invalid("report_encoding"))?
        );
        return Ok(if report["ok"].as_bool() == Some(true) {
            0
        } else {
            1
        });
    }
    if matches!(args.command, Some(FrontendCommand::FoundationCheck)) {
        let report = foundation_check::run(args.relay, mode).await?;
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|_| Error::Invalid("report_encoding"))?
        );
        return Ok(if report["ok"].as_bool() == Some(true) {
            0
        } else {
            1
        });
    }
    let state = state_dir(args.state_dir.clone())?;
    if let Some(FrontendCommand::Setup {
        interactive,
        role,
        grant,
        revoke,
        trust,
        untrust,
        update,
        no_grants,
    }) = &args.command
    {
        use crate::native_setup::{SetupOptions, SetupRole};
        let role = match role.as_deref() {
            Some("client" | "c") => Some(SetupRole::Client),
            Some("server" | "s") => Some(SetupRole::Server),
            Some("both" | "b") => Some(SetupRole::Both),
            None => None,
            _ => return Err(Error::Invalid("setup_role")),
        };
        let report = crate::native_setup::run(SetupOptions {
            state_dir: state,
            identity: args.identity,
            backend: args.key_storage.into(),
            helper: layout.helper.clone(),
            interactive: *interactive,
            role,
            relays: (!args.relay.is_empty()).then_some(args.relay),
            tor: args.tor,
            grants: grant.clone(),
            revoke: revoke.clone(),
            trust: trust.clone(),
            untrust: untrust.clone(),
            update: *update,
            no_grants: *no_grants,
            debug: args.debug,
        })
        .await?;
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|_| Error::Invalid("report_encoding"))?
        );
        return Ok(0);
    }
    if daemon_binary && (command_action(&args) || stream_action(&args)) {
        return Err(Error::Invalid("daemon_operation"));
    }
    if stream_action(&args) {
        return run_stream_command(&args, &state).await;
    }
    if command_action(&args) {
        return run_command(&args, &state).await;
    }
    if matches!(
        args.command,
        Some(FrontendCommand::Init | FrontendCommand::Daemon)
    ) {
        let relay_policy = if args.relay.is_empty() {
            CommandConfig::load(&state.join(&args.identity))?
                .relay_policy()
                .clone()
        } else {
            RelayPolicy::parse(&args.relay, mode, TransportLimits::default())?
        };
        let config = Config {
            max_commands: args
                .max_commands
                .as_deref()
                .map(crate::command_limits::parse_max_commands)
                .transpose()?,
            max_uptime: args
                .max_uptime
                .as_deref()
                .map(crate::command_limits::parse_max_uptime)
                .transpose()?,
            state_dir: state,
            identity: args.identity,
            backend: args.key_storage.into(),
            helper: layout.helper.clone(),
            relay_policy,
            allowed_peers: args.allow_peer,
            retention: std::time::Duration::from_secs(args.retention_seconds),
        };
        if matches!(args.command, Some(FrontendCommand::Init)) {
            let (profile, owner) = daemon::open(config).await?;
            let snapshot = owner.status();
            let stopped = daemon::stop_silent(profile, owner).await;
            let snapshot = snapshot?;
            stopped?;
            println!(
                "{}",
                serde_json::to_string(&snapshot).map_err(|_| Error::Invalid("status_encoding"))?
            );
        } else {
            daemon::run_with_startup(config, args.startup).await?;
        }
        return Ok(0);
    }
    if daemon_binary {
        return Err(Error::Invalid("daemon_operation"));
    }
    let socket = match args.socket {
        Some(path) => path,
        None => socket_path_for(&state, &args.identity)?,
    };
    let request = match args.command.ok_or(Error::Invalid("client_operation"))? {
        FrontendCommand::Status => Request::Status {},
        FrontendCommand::CommandEvidence { after, limit } => {
            Request::CommandEvidence { after, limit }
        }
        FrontendCommand::CommandCleanupStatus => Request::CommandCleanupStatus {},
        FrontendCommand::CommandLocalCancel { cursor } => Request::CommandLocalCancel { cursor },
        FrontendCommand::CommandPrepareCleanup => Request::CommandPrepareCleanup {},
        FrontendCommand::OpenConversation { peer } => Request::OpenConversation { peer },
        FrontendCommand::Send { session, id, data } => Request::Send { session, id, data },
        FrontendCommand::Received { after, limit } => Request::Received { after, limit },
        _ => return Err(Error::Invalid("client_operation")),
    };
    let response = ipc::request(&socket, &request).await?;
    println!(
        "{}",
        serde_json::to_string(&response).map_err(|_| Error::Invalid("response_encoding"))?
    );
    Ok(if response.ok { 0 } else { 1 })
}

fn command_action(args: &Args) -> bool {
    matches!(
        args.command,
        Some(
            FrontendCommand::Remote(_)
                | FrontendCommand::Commands { .. }
                | FrontendCommand::CommandStatus(_)
                | FrontendCommand::Cancel(_)
                | FrontendCommand::Wait(_)
                | FrontendCommand::Output(_)
        )
    ) || (args.command.is_none() && args.user.is_some())
}

fn stream_action(args: &Args) -> bool {
    matches!(
        &args.command,
        Some(
            FrontendCommand::Stream { .. }
                | FrontendCommand::Put(_)
                | FrontendCommand::Get(_)
                | FrontendCommand::Rsh
                | FrontendCommand::CopyReceipts { .. }
                | FrontendCommand::Streams { .. }
                | FrontendCommand::StreamReconcile { .. }
        )
    ) || matches!(&args.command, Some(FrontendCommand::Cancel(select)) if select.stream.is_some())
}

async fn stream_preparation<T>(
    signals: &mut Option<crate::stream_cli::ExecutionSignals>,
    operation: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    if let Some(signals) = signals.as_mut() {
        tokio::select! {
            biased;
            interrupt = signals.interrupted() => {
                interrupt?;
                use std::io::Write;
                let _ = writeln!(std::io::stderr().lock(),
                    "marmish stream: interrupted before remote Open; no remote child was launched");
                Err(Error::Invalid("stream_interrupted_before_open"))
            }
            result = operation => result,
        }
    } else {
        operation.await
    }
}

async fn run_stream_command(args: &Args, state: &std::path::Path) -> Result<u8> {
    crate::identity::validate_name(&args.identity)?;
    use crate::stream_cli::{ExecutionSignals, SignalPolicy};
    let mut signals = match &args.command {
        Some(FrontendCommand::Stream { argv, .. }) => {
            if args.detach || args.stdin_none || args.trailing.as_ref() != Some(argv) {
                return Err(Error::Invalid("stream_arguments"));
            }
            Some(ExecutionSignals::register(SignalPolicy::Interactive)?)
        }
        Some(FrontendCommand::Put(_) | FrontendCommand::Get(_)) => {
            if args.detach || args.stdin || args.stdin_none {
                return Err(Error::Invalid("copy_arguments"));
            }
            Some(ExecutionSignals::register(SignalPolicy::Interactive)?)
        }
        Some(FrontendCommand::Rsh) => {
            if args.rsh.is_none() {
                return Err(Error::Invalid("rsync_arguments"));
            }
            Some(ExecutionSignals::register(SignalPolicy::Rsync)?)
        }
        _ => None,
    };
    let config = CommandConfig::load(&state.join(&args.identity))?;
    let socket = stream_preparation(&mut signals, command_socket(args, state, &config)).await?;
    let context = Context {
        socket,
        profile: args.identity.clone(),
    };
    match &args.command {
        Some(FrontendCommand::Stream {
            destination,
            argv,
            compress,
            idle_seconds,
            keepalive_seconds,
        }) => {
            let target = stream_preparation(
                &mut signals,
                resolve_target(&config, args, Some(destination)),
            )
            .await?;
            crate::stream_cli::execute(
                context,
                target.remote,
                target.relays,
                argv.clone(),
                crate::stream_cli::ExecuteOptions {
                    compress: *compress,
                    new_session: args.new_session,
                    idle: Duration::from_secs(*idle_seconds),
                    keepalive: Duration::from_secs(*keepalive_seconds),
                    signals: signals.take().ok_or(Error::Invalid("stream_signal"))?,
                },
            )
            .await
        }
        Some(FrontendCommand::Rsh) => {
            let rsh = args.rsh.as_ref().ok_or(Error::Invalid("rsync_arguments"))?;
            let target = stream_preparation(
                &mut signals,
                resolve_target_with_user(
                    &config,
                    args,
                    Some(&rsh.destination),
                    rsh.login.as_deref(),
                ),
            )
            .await?;
            crate::stream_cli::execute(
                context,
                target.remote,
                target.relays,
                rsh.argv.clone(),
                crate::stream_cli::ExecuteOptions {
                    compress: rsh.compress,
                    new_session: false,
                    idle: rsh.idle,
                    keepalive: rsh.keepalive,
                    signals: signals.take().ok_or(Error::Invalid("stream_signal"))?,
                },
            )
            .await
        }
        Some(FrontendCommand::Put(copy) | FrontendCommand::Get(copy)) => {
            let target = stream_preparation(
                &mut signals,
                resolve_target(&config, args, Some(&copy.remote)),
            )
            .await?;
            crate::copy_cli::execute(
                context,
                target.remote,
                target.relays,
                crate::copy_cli::Options {
                    direction: if matches!(args.command, Some(FrontendCommand::Put(_))) {
                        marmish_stream::copy_protocol::Direction::Put
                    } else {
                        marmish_stream::copy_protocol::Direction::Get
                    },
                    source: copy.source.clone(),
                    destination: copy.target.clone(),
                    resume: copy.resume.clone(),
                    overwrite: copy.overwrite,
                    preserve_mode: copy.preserve_mode,
                    preserve_mtime: copy.preserve_mtime,
                    compress: copy.compress,
                    idle: Duration::from_secs(copy.idle_seconds),
                    keepalive: Duration::from_secs(copy.keepalive_seconds),
                    probe: Duration::from_secs(copy.probe_seconds),
                    retention: config.limits().copy_receipt_retention.as_secs(),
                    new_session: args.new_session,
                    signals: signals.take().ok_or(Error::Invalid("stream_signal"))?,
                },
            )
            .await
        }
        Some(FrontendCommand::CopyReceipts {
            forget,
            revision,
            acknowledge_retained_state,
        }) => {
            if let Some(namespace) = forget {
                if !acknowledge_retained_state {
                    return Err(Error::Invalid("copy_forget_acknowledgement"));
                }
                crate::copy_local_gc::forget(
                    &context,
                    namespace.clone(),
                    revision.ok_or(Error::Invalid("copy_forget_revision"))?,
                )
                .await?;
                use std::io::Write;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "marmish copy: index record forgotten; retained files were not removed and require manual inspection"
                );
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&crate::copy_local_gc::inspect(&context).await?)
                        .map_err(|_| Error::Invalid("report_encoding"))?
                );
            }
            Ok(0)
        }
        Some(FrontendCommand::Streams { destination }) => {
            let remote = target_filter(&config, args, destination.as_deref()).await?;
            crate::stream_cli::list(&context, remote).await
        }
        Some(FrontendCommand::Cancel(select)) => {
            let (remote, relays) = if select.destination.is_some() || args.user.is_some() {
                let target = resolve_target(&config, args, select.destination.as_deref()).await?;
                (Some(target.remote), target.relays)
            } else {
                target_filter(&config, args, None).await?;
                (
                    None,
                    config
                        .relay_policy()
                        .urls()
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                )
            };
            crate::stream_cli::cancel_selected(
                &context,
                select
                    .stream
                    .as_deref()
                    .ok_or(Error::Invalid("stream_id"))?,
                remote,
                select.local,
                relays,
            )
            .await
        }
        Some(FrontendCommand::StreamReconcile {
            stream,
            acknowledge_possible_live,
        }) => crate::stream_cli::reconcile(&context, stream, *acknowledge_possible_live).await,
        _ => Err(Error::Invalid("stream_operation")),
    }
}

fn validate_separator(values: &[OsString], argv: &[OsString], destination: bool) -> Result<()> {
    let valid = if destination {
        values.len() == argv.len() + 2 && values[1] == "--" && values[2..] == *argv
    } else {
        values == argv
    };
    if valid {
        Ok(())
    } else {
        Err(Error::Invalid("command_separator"))
    }
}

fn selector(value: &SelectArgs, remote: Option<crate::command_wire::Account>) -> Result<Selection> {
    let selector = if let Some(handle) = &value.handle {
        Selector::Handle(crate::command_client::CommandHandle::decode(
            handle.as_bytes(),
        )?)
    } else if let Some(path) = &value.handle_file {
        Selector::HandleFile(path.clone())
    } else if let Some(invocation) = &value.invocation {
        crate::foundation::id_timestamp(invocation)?;
        Selector::Invocation(
            hex::decode(invocation)
                .map_err(|_| Error::Invalid("command_id"))?
                .try_into()
                .map_err(|_| Error::Invalid("command_id"))?,
        )
    } else if value.latest {
        Selector::Latest
    } else {
        return Err(Error::Invalid("command_selector"));
    };
    Ok(Selection { selector, remote })
}

async fn autostart_daemon(
    args: &Args,
    state: &std::path::Path,
    config: &CommandConfig,
) -> Result<()> {
    let executable = std::env::current_exe()?.canonicalize()?;
    let daemon = executable
        .parent()
        .ok_or(Error::Invalid("daemon_installation"))?
        .join("marmishd");
    let mut child = std::process::Command::new(&daemon);
    child
        .arg("--state-dir")
        .arg(state)
        .arg("-i")
        .arg(&args.identity)
        .arg("--key-storage")
        .arg(match args.key_storage {
            Backend::Auto => "auto",
            Backend::Keychain => "keychain",
            Backend::File => "file",
        })
        .arg("--tor")
        .arg(match config.relay_policy().mode() {
            RouteMode::Auto => "auto",
            RouteMode::Always => "always",
            RouteMode::Off => "off",
        });
    for relay in config.relay_policy().urls() {
        child.arg("--relay").arg(relay.as_str());
    }
    if args.debug {
        child.arg("--debug");
    }
    child.arg("daemon");
    let arguments = child
        .get_args()
        .map(OsStr::to_os_string)
        .collect::<Vec<_>>();
    let state = state.to_owned();
    let identity = args.identity.clone();
    let launched = tokio::task::spawn_blocking(move || {
        crate::daemon_launch::launch(&daemon, &arguments, &state, &identity)
    })
    .await
    .map_err(|_| Error::Invalid("daemon_startup_worker"))?;
    match launched {
        Ok(()) | Err(Error::Identity("profile_in_use")) => (),
        Err(Error::DaemonStartup(message)) if message == "identity: profile_in_use" => (),
        Err(error) => return Err(error),
    }
    Ok(())
}

async fn command_socket(
    args: &Args,
    state: &std::path::Path,
    config: &CommandConfig,
) -> Result<PathBuf> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    let mut started = false;
    loop {
        let socket = args
            .socket
            .clone()
            .map(Ok)
            .unwrap_or_else(|| socket_path_for(state, &args.identity));
        match socket {
            Ok(socket) => {
                match tokio::time::timeout_at(deadline, ipc::request(&socket, &Request::Status {}))
                    .await
                {
                    Ok(Ok(response)) if response.ok => {
                        if response
                            .result
                            .as_ref()
                            .and_then(|v| v.get("state"))
                            .and_then(|v| v.as_str())
                            != Some("activating")
                        {
                            return Ok(socket);
                        }
                        started = true;
                    }
                    Ok(Err(Error::Io(ref e)))
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                        ) => {}
                    Ok(Err(error)) => return Err(error),
                    Err(_) => return Err(Error::Timeout),
                    _ => return Err(Error::Invalid("daemon_status")),
                }
            }
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        // These options forbid launching a daemon, but an authenticated
        // activating daemon already owns startup and still needs readiness time.
        if (args.no_autostart || args.socket.is_some()) && !started {
            return Err(Error::Invalid("daemon_unavailable"));
        }
        if !started {
            autostart_daemon(args, state, config).await?;
            started = true;
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(Error::Timeout);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn resolve_target(
    config: &CommandConfig,
    args: &Args,
    destination: Option<&str>,
) -> Result<crate::command_target::ResolvedTarget> {
    resolve_target_with_user(config, args, destination, args.user.as_deref()).await
}

async fn resolve_target_with_user(
    config: &CommandConfig,
    args: &Args,
    destination: Option<&str>,
    user: Option<&str>,
) -> Result<crate::command_target::ResolvedTarget> {
    use crate::command_target::{self, RelaySource};
    let target = command_target::parse(config, destination, user, &args.relay, &args.srv)?;
    let relays = match target.source {
        RelaySource::Configured => config
            .relay_policy()
            .urls()
            .iter()
            .map(ToString::to_string)
            .collect(),
        RelaySource::Explicit(relays) => relays,
        RelaySource::Srv(names) => crate::command_srv::resolve(&names).await?,
    };
    // Freeze and compare the complete DNS answer before opening a profile or IPC.
    command_target::finalize(config, target.remote, &relays)
}

async fn target_filter(
    config: &CommandConfig,
    args: &Args,
    destination: Option<&str>,
) -> Result<Option<crate::command_wire::Account>> {
    if destination.is_some() || args.user.is_some() {
        Ok(Some(
            resolve_target(config, args, destination).await?.remote,
        ))
    } else if !args.relay.is_empty() || !args.srv.is_empty() {
        Err(Error::Invalid("conflicting_destination"))
    } else {
        Ok(None)
    }
}

async fn run_command(args: &Args, state: &std::path::Path) -> Result<u8> {
    crate::identity::validate_name(&args.identity)?;
    let config = CommandConfig::load(&state.join(&args.identity))?;
    let wait = Duration::from_secs(args.wait_seconds);
    let action = match &args.command {
        Some(FrontendCommand::Remote(values)) => {
            let argv = args
                .trailing
                .as_ref()
                .ok_or(Error::Invalid("command_separator"))?
                .clone();
            if argv.is_empty() {
                return Err(Error::Invalid("command_argv"));
            }
            let destination = if values.len() == argv.len() && values == &argv {
                None
            } else {
                Some(
                    values
                        .first()
                        .and_then(|v| v.to_str())
                        .ok_or(Error::Invalid("invalid_destination"))?,
                )
            };
            validate_separator(values, &argv, destination.is_some())?;
            let target = resolve_target(&config, args, destination).await?;
            Action::Submit {
                remote: target.remote,
                relays: target.relays,
                argv,
                stdin: if args.stdin {
                    StdinMode::Read
                } else if args.stdin_none {
                    StdinMode::None
                } else {
                    StdinMode::Auto
                },
                detach: args.detach,
                new_session: args.new_session,
                wait,
            }
        }
        Some(FrontendCommand::Commands { destination }) => Action::Commands {
            remote: target_filter(&config, args, destination.as_deref()).await?,
        },
        Some(FrontendCommand::CommandStatus(value)) => Action::Status {
            selection: selector(
                value,
                target_filter(&config, args, value.destination.as_deref()).await?,
            )?,
        },
        Some(FrontendCommand::Cancel(value)) => Action::Cancel {
            selection: selector(
                value,
                target_filter(&config, args, value.destination.as_deref()).await?,
            )?,
        },
        Some(FrontendCommand::Wait(value)) => Action::Wait {
            selection: selector(
                value,
                target_filter(&config, args, value.destination.as_deref()).await?,
            )?,
            wait,
        },
        Some(FrontendCommand::Output(value)) => Action::Output {
            selection: selector(
                value,
                target_filter(&config, args, value.destination.as_deref()).await?,
            )?,
            wait,
        },
        _ => return Err(Error::Invalid("command_argv")),
    };
    let socket = command_socket(args, state, &config).await?;
    command_cli::run(
        Context {
            socket,
            profile: args.identity.clone(),
        },
        action,
    )
    .await
}

async fn keychain_authorize(
    args: &Args,
    layout: &crate::native_layout::ExecutableLayout,
    started: std::time::Instant,
) -> Result<u8> {
    if !cfg!(target_os = "macos") {
        return Err(Error::Identity("keychain_authorize_unsupported"));
    }
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal()
        || !std::io::stderr().is_terminal()
        || unsafe { libc::tcgetpgrp(libc::STDIN_FILENO) } != unsafe { libc::getpgrp() }
    {
        return Err(Error::Identity("keychain_authorize_foreground_required"));
    }
    // Register all streams before a worker can launch a helper. Retain them
    // across both phases so a signal in the inter-helper gap is not lost.
    use tokio::signal::unix::{SignalKind, signal};
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    let mut hangup = signal(SignalKind::hangup())?;
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = cancel.clone();
    let base = state_dir(args.state_dir.clone())?;
    let name = args.identity.clone();
    let helper = layout.helper.clone();
    eprintln!(
        "macOS may ask for consent for two existing Marmish items. Persistent background access depends on your own OS authorization choice; Allow once does not guarantee future access."
    );
    let mut worker = tokio::task::spawn_blocking(move || {
        let lease = crate::identity::ProfileLease::acquire_existing(&base, &name)?;
        lease.authorize_keychain(&helper, started, &worker_cancel)
    });
    let result = tokio::select! {
        result = &mut worker => result,
        _ = interrupt.recv() => { cancel.store(true, std::sync::atomic::Ordering::SeqCst); worker.await },
        _ = terminate.recv() => { cancel.store(true, std::sync::atomic::Ordering::SeqCst); worker.await },
        _ = hangup.recv() => { cancel.store(true, std::sync::atomic::Ordering::SeqCst); worker.await },
    }.map_err(|_| Error::Identity("keychain_authorize_worker"))?;
    if cancel.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::Identity("keychain_authorize_interrupted"));
    }
    result?;
    println!("Keychain access verified for this profile.");
    Ok(0)
}

fn authorization_disallowed(matches: &clap::ArgMatches) -> bool {
    // Derive groups also have CommandLine sources; inspect actual arguments,
    // rather than accidentally treating the enclosing Args group as a flag.
    Args::command().get_arguments().any(|arg| {
        let id = arg.get_id().as_str();
        matches.value_source(id) == Some(clap::parser::ValueSource::CommandLine)
            && !matches!(id, "debug" | "state_dir" | "identity")
    })
}

fn usage_exit_code(error: &clap::Error) -> u8 {
    if matches!(
        error.kind(),
        clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
    ) {
        0
    } else {
        255
    }
}

fn parse_public(raw: &[OsString]) -> std::result::Result<Args, clap::Error> {
    let rsh = crate::rsync_cli::split(raw, Args::command())?;
    let root = rsh.as_ref().map_or(raw, |rsh| rsh.root.as_slice());
    let matches = Args::command().try_get_matches_from(root)?;
    let mut args = Args::from_arg_matches(&matches)?;
    if rsh.is_some() && !matches!(args.command, Some(FrontendCommand::Rsh)) {
        return Err(clap::Error::raw(
            clap::error::ErrorKind::InvalidValue,
            "rsync parser mismatch",
        ));
    }
    args.authorize_disallowed = authorization_disallowed(&matches);
    args.trailing = if rsh.is_none() {
        raw.iter()
            .position(|value| value == OsStr::new("--"))
            .map(|index| raw[index + 1..].to_vec())
    } else {
        None
    };
    args.rsh = rsh;
    Ok(args)
}

pub fn main(daemon_binary: bool) -> ExitCode {
    let raw = std::env::args_os().collect::<Vec<_>>();
    if raw
        .get(1)
        .is_some_and(|arg| arg == OsStr::new(marmish_stream::copy_protocol::HELPER_ENTRYPOINT))
    {
        use std::os::fd::FromRawFd;
        let descriptor = raw.get(3).and_then(|arg| arg.to_str()).and_then(|text| {
            text.parse::<i32>()
                .ok()
                .filter(|fd| *fd > 3 && fd.to_string() == text)
        });
        if raw.len() != 4
            || raw
                .get(2)
                .is_none_or(|arg| arg != OsStr::new("--context-fd"))
            || descriptor.is_none()
        {
            eprintln!("marmish copy: helper_context_invalid");
            return ExitCode::from(1);
        }
        let fd = descriptor.unwrap();
        if [fd, 0, 1]
            .into_iter()
            .any(|descriptor| unsafe { libc::fcntl(descriptor, libc::F_GETFD) } < 0)
        {
            return ExitCode::from(1);
        }
        // SAFETY: the dedicated child owns this open descriptor and all stdio.
        let context = unsafe { std::fs::File::from_raw_fd(fd) };
        let input = unsafe { std::fs::File::from_raw_fd(0) };
        let output = unsafe { std::fs::File::from_raw_fd(1) };
        std::process::exit(crate::copy_helper::run(context, input, output) as i32);
    }
    // The provider helper has its own private protocol; validate its canonical
    // executable before entering it, without parsing public CLI arguments.
    let helper_action = raw.get(1).map(OsString::as_os_str);
    if matches!(helper_action, Some(value) if value == OsStr::new("_vault-helper") || value == OsStr::new("_vault-authorize") || value == OsStr::new("_vault-verify-authorization"))
    {
        if let Err(error) = crate::validate_executable_layout() {
            eprintln!("marmish: {error}");
            return ExitCode::from(255);
        }
        return match helper_action {
            Some(value) if value == OsStr::new("_vault-authorize") => {
                marmish_vault::helper::run_authorization(
                    marmish_vault::helper::AuthorizationMode::Interactive,
                )
            }
            Some(value) if value == OsStr::new("_vault-verify-authorization") => {
                marmish_vault::helper::run_authorization(
                    marmish_vault::helper::AuthorizationMode::Verify,
                )
            }
            _ => marmish_vault::helper::run(),
        };
    }
    let mut args = match parse_public(&raw) {
        Ok(args) => args,
        Err(error) => {
            let code = usage_exit_code(&error);
            let _ = error.print();
            if code != 0 {
                eprintln!("marmish: invalid_arguments");
            }
            return ExitCode::from(code);
        }
    };
    let copy_action = matches!(
        args.command,
        Some(FrontendCommand::Put(_) | FrontendCommand::Get(_))
    );
    let failure_code = if command_action(&args) || stream_action(&args) {
        255
    } else {
        1
    };
    if let Err(error) = validate_daemon_options(&args, daemon_binary) {
        print_error(&error);
        return ExitCode::from(failure_code);
    }
    let startup = match crate::daemon_launch::StartupReporter::take_from_env(
        args.foreground && is_daemon(&args, daemon_binary),
    ) {
        Ok(startup) => startup,
        Err(error) => {
            print_error(&error);
            return ExitCode::from(failure_code);
        }
    };
    args.startup = startup.clone();
    // Parsing needs no provider or private state. Canonical layout validation
    // still precedes every provider/profile access.
    let layout = match std::env::current_exe()
        .map_err(Error::from)
        .and_then(|path| crate::native_layout::resolve(&path))
    {
        Ok(layout) => layout,
        Err(error) => {
            if let Some(reporter) = &startup {
                reporter.failed(&error);
            }
            print_error(&error);
            return ExitCode::from(failure_code);
        }
    };
    if is_daemon(&args, daemon_binary) && !args.foreground {
        let result = state_dir(args.state_dir.clone()).and_then(|state| {
            crate::daemon_launch::launch(
                &std::env::current_exe()?,
                &raw[1..],
                &state,
                &args.identity,
            )
        });
        return match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                print_error(&error);
                ExitCode::from(failure_code)
            }
        };
    }
    if args.debug {
        crate::diagnostics::enable();
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            let error = Error::Invalid("runtime_startup");
            if let Some(reporter) = &startup {
                reporter.failed(&error);
            }
            print_error(&error);
            return ExitCode::from(failure_code);
        }
    };
    match run_async(runtime, run(args, daemon_binary, &layout)) {
        Ok(code) => {
            // A clean limit/signal exit during activation never became ready.
            if let Some(reporter) = &startup {
                reporter.failed(&Error::Invalid("daemon_stopped_before_ready"));
            }
            ExitCode::from(code)
        }
        Err(error) => {
            if let Some(reporter) = &startup {
                reporter.failed(&error);
            }
            print_error(&error);
            ExitCode::from(if copy_action {
                crate::copy_cli::failure_exit(&error)
            } else {
                failure_code
            })
        }
    }
}

/// An interrupted frontend may have abandoned a blocking daemon startup wait.
/// The independently launched local daemon may finish starting; runtime teardown
/// must not turn that wait into a delay in acknowledging the user's interrupt.
fn run_async(
    runtime: tokio::runtime::Runtime,
    operation: impl std::future::Future<Output = Result<u8>>,
) -> Result<u8> {
    let result = runtime.block_on(operation);
    if matches!(
        &result,
        Err(Error::Invalid("stream_interrupted_before_open"))
    ) {
        runtime.shutdown_background();
    }
    result
}

fn print_error(error: &Error) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "marmish: {error}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    #[test]
    fn forgetting_copy_index_requires_exact_revision_and_retention_acknowledgement() {
        assert!(Args::try_parse_from(["marmish", "copy-receipts"]).is_ok());
        assert!(Args::try_parse_from(["marmish", "copy-receipts", "--forget", "opaque"]).is_err());
        assert!(
            Args::try_parse_from([
                "marmish",
                "copy-receipts",
                "--forget",
                "opaque",
                "--revision",
                "1"
            ])
            .is_err()
        );
        assert!(
            Args::try_parse_from([
                "marmish",
                "copy-receipts",
                "--forget",
                "opaque",
                "--revision",
                "1",
                "--acknowledge-retained-state"
            ])
            .is_ok()
        );
    }
    #[test]
    fn copy_paths_preserve_literal_non_utf8_bytes_and_leading_flags() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let source = OsString::from_vec(b"-source;$(echo x) \xff\n".to_vec());
        let target = OsString::from_vec(b"-target*? \xfe\t".to_vec());
        for action in ["put", "get"] {
            let args = Args::try_parse_from([
                OsString::from("marmish"),
                OsString::from(action),
                OsString::from("--compress"),
                OsString::from("--"),
                OsString::from("remote-alias"),
                source.clone(),
                target.clone(),
            ])
            .unwrap();
            let copy = match args.command.unwrap() {
                FrontendCommand::Put(copy) | FrontendCommand::Get(copy) => copy,
                _ => panic!("copy action required"),
            };
            assert!(copy.compress);
            assert_eq!(copy.source.as_bytes(), source.as_bytes());
            assert_eq!(copy.target.as_bytes(), target.as_bytes());
            assert_eq!(copy.remote, "remote-alias");
        }
    }
    #[test]
    fn early_stream_signal_does_not_wait_for_blocking_autostart() {
        const MARKER: &str = "MARMISH_TEST_AUTOSTART_SIGNAL_MARKER";
        if let Some(marker) = std::env::var_os(MARKER) {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            let result = run_async(runtime, async {
                let mut signals = Some(crate::stream_cli::ExecutionSignals::register(
                    crate::stream_cli::SignalPolicy::Rsync,
                )?);
                stream_preparation(&mut signals, async {
                    let (entered, waiting) = tokio::sync::oneshot::channel();
                    // Model a launcher which has entered its blocking startup
                    // wait. Runtime Drop would otherwise wait indefinitely.
                    let wait = tokio::task::spawn_blocking(move || {
                        let _ = entered.send(());
                        loop {
                            std::thread::park();
                        }
                    });
                    waiting.await.map_err(|_| Error::Invalid("fixture"))?;
                    std::fs::write(marker, b"ready")?;
                    wait.await.map_err(|_| Error::Invalid("fixture"))?;
                    Ok(0)
                })
                .await
            });
            assert!(matches!(
                result,
                Err(Error::Invalid("stream_interrupted_before_open"))
            ));
            return;
        }
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("ready");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cli::tests::early_stream_signal_does_not_wait_for_blocking_autostart",
                "--nocapture",
            ])
            .env(MARKER, &marker)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !marker.exists() {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("fixture exited before startup wait: {status}");
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("fixture did not enter startup wait");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Signal only our live, unreaped child. Its Tokio listener already exists.
        assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGUSR1) }, 0);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                let output = child.wait_with_output().unwrap();
                assert!(status.success(), "{output:?}");
                assert!(
                    output
                        .stderr
                        .windows(b"interrupted before remote Open".len())
                        .any(|part| part == b"interrupted before remote Open")
                );
                break;
            }
            if std::time::Instant::now() >= deadline {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!("frontend waited for blocking startup: {output:?}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn public(values: &[&str]) -> std::result::Result<Args, clap::Error> {
        parse_public(&values.iter().map(OsString::from).collect::<Vec<_>>())
    }

    #[test]
    fn rsh_production_parser_separates_globals_and_opaque_bootstrap() {
        for identity in ["work", "rsh"] {
            for spelling in [
                vec!["-i".to_owned(), identity.to_owned()],
                vec!["--identity".to_owned(), identity.to_owned()],
                vec![format!("-i{identity}")],
                vec![format!("-i={identity}")],
                vec![format!("--identity={identity}")],
            ] {
                let mut raw = vec![OsString::from("marmish")];
                raw.extend(spelling.into_iter().map(OsString::from));
                raw.extend(
                    [
                        "rsh",
                        "--compress",
                        "-l",
                        "remote-alias",
                        "relay.example",
                        "rsync",
                        "--server",
                        "-slogDtpre.iLsfxCIvu",
                    ]
                    .map(OsString::from),
                );
                let args = parse_public(&raw).unwrap();
                assert_eq!(args.identity, identity);
                assert!(stream_action(&args));
                assert!(!command_action(&args));
                let rsh = args.rsh.unwrap();
                assert_eq!(rsh.destination, "relay.example");
                assert_eq!(rsh.login.as_deref(), Some("remote-alias"));
                assert!(rsh.compress);
                assert_eq!((rsh.idle.as_secs(), rsh.keepalive.as_secs()), (660, 300));
                assert_eq!(
                    rsh.argv,
                    ["rsync", "--server", "-slogDtpre.iLsfxCIvu"].map(OsString::from)
                );
            }
        }
        let args = public(&[
            "marmish",
            "--state-dir=rsh",
            "rsh",
            "remote-alias",
            "rsync",
            "--server",
            "-s",
        ])
        .unwrap();
        assert_eq!(args.state_dir, Some(PathBuf::from("rsh")));
        assert!(args.trailing.is_none());
        assert!(matches!(
            public(&["marmish", "peer", "--", "echo", "rsh"])
                .unwrap()
                .command,
            Some(FrontendCommand::Remote(_))
        ));
    }

    #[test]
    fn rsh_remote_globals_never_become_local_configuration() {
        for dangerous in [
            "--state-dir",
            "--socket",
            "--relay",
            "--srv",
            "-i",
            "-u",
            "--help",
            "--version",
            "--",
        ] {
            let error = public(&[
                "marmish",
                "rsh",
                "remote-alias",
                "rsync",
                "--server",
                "-s",
                dangerous,
            ])
            .err()
            .unwrap();
            assert_eq!(usage_exit_code(&error), 255, "{dangerous}");
        }
        for before in [
            "--detach",
            "--new-session",
            "--foreground",
            "--stdin",
            "--stdin-none",
        ] {
            assert!(
                public(&["marmish", before, "rsh", "peer", "rsync", "--server", "-s"]).is_err()
            );
        }
        for pair in [
            ["-u", "peer"],
            ["--user", "peer"],
            ["--wait-seconds", "10"],
            ["--allow-peer", "peer"],
            ["--max-commands", "3"],
            ["--max-uptime", "30m"],
        ] {
            assert!(
                public(&[
                    "marmish", pair[0], pair[1], "rsh", "peer", "rsync", "--server", "-s"
                ])
                .is_err()
            );
        }
        for suffix in [
            vec!["--", "peer", "rsync", "--server", "-s"],
            vec!["-l", "peer", "other@relay", "rsync", "--server", "-s"],
            vec!["peer@@relay", "rsync", "--server", "-s"],
            vec!["-l", "one", "-l", "two", "relay", "rsync", "--server", "-s"],
        ] {
            let mut raw = vec!["marmish", "rsh"];
            raw.extend(suffix);
            assert!(public(&raw).is_err());
        }
        assert_eq!(
            usage_exit_code(&public(&["marmish", "rsh", "--help"]).err().unwrap()),
            0
        );
        assert_eq!(
            usage_exit_code(&public(&["marmish", "--help", "rsh"]).err().unwrap()),
            0
        );
    }

    #[test]
    fn rsh_preserves_non_utf8_executable_and_iconv_bytes_but_rejects_non_utf8_host() {
        use std::os::unix::ffi::OsStringExt;
        let mut raw = [
            "marmish",
            "rsh",
            "peer",
            "rsync",
            "--server",
            "-s",
            "--iconv=utf8",
        ]
        .map(OsString::from)
        .to_vec();
        raw[3] = OsString::from_vec(b"/opt/a b;$(literal)\xff".to_vec());
        raw[6] = OsString::from_vec(b"--iconv=\xff".to_vec());
        let args = parse_public(&raw).unwrap();
        assert_eq!(args.rsh.unwrap().argv, raw[3..]);
        raw[2] = OsString::from_vec(vec![255]);
        assert!(parse_public(&raw).is_err());
    }

    #[test]
    fn rsh_global_metadata_arity_is_closed_and_explicit() {
        let mut command = Args::command();
        command.build();
        for argument in command.get_arguments().filter(|arg| arg.is_global_set()) {
            let takes_value = match argument.get_id().as_str() {
                "debug" | "foreground" | "stdin" | "stdin_none" | "detach" | "new_session"
                | "no_autostart" => false,
                "state_dir" | "identity" | "socket" | "relay" | "tor" | "key_storage"
                | "allow_peer" | "retention_seconds" | "user" | "srv" | "wait_seconds"
                | "max_commands" | "max_uptime" => true,
                id => panic!("new global option requires rsync parser review: {id}"),
            };
            assert_eq!(argument.get_action().takes_values(), takes_value);
            if let Some(range) = argument.get_num_args() {
                assert_eq!(range.min_values(), usize::from(takes_value));
                assert_eq!(range.max_values(), usize::from(takes_value));
            }
        }
    }

    #[tokio::test]
    async fn rsh_is_rejected_by_daemon_before_profile_access() {
        let directory = tempfile::tempdir().unwrap();
        let mut args = public(&["marmishd", "rsh", "peer", "rsync", "--server", "-s"]).unwrap();
        args.state_dir = Some(directory.path().join("absent"));
        let layout = crate::native_layout::ExecutableLayout {
            helper: directory.path().join("absent-helper"),
            worker: directory.path().join("absent-worker"),
        };
        assert!(matches!(
            run(args, true, &layout).await,
            Err(Error::Invalid("daemon_operation"))
        ));
        assert!(directory.path().read_dir().unwrap().next().is_none());
    }

    #[tokio::test]
    async fn command_socket_waits_for_existing_activation_without_autostart() {
        use std::os::unix::fs::PermissionsExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let config = CommandConfig::from_bytes(
            br#"{"version":1,"role":"client","relays":["wss://127.0.0.1:9/"],"tor":"off","trusted_remotes":[],"execution_clients":[],"command_policy":{"version":1},"cwd":"/","limits":{},"aliases":{}}"#,
        )
        .unwrap();
        for (explicit_socket, no_autostart) in [(false, true), (true, false), (true, true)] {
            let directory = tempfile::Builder::new()
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir()
                .unwrap();
            let mut profile =
                crate::identity::ProfileLease::acquire(directory.path(), "default").unwrap();
            let _identity = profile
                .prepare(
                    BackendChoice::File,
                    &directory.path().join("unused-helper"),
                    std::time::Instant::now() + Duration::from_secs(5),
                )
                .unwrap();
            let socket = profile.socket_path();
            let listener = ipc::bind(&socket).unwrap();
            let server = tokio::spawn(async move {
                // Each response follows a real Status request: readiness changes
                // by protocol exchange, rather than by a test timing assumption.
                for state in ["activating", "active"] {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let length = stream.read_u32().await.unwrap() as usize;
                    assert!(length > 0 && length <= 65_536);
                    let mut bytes = vec![0; length];
                    stream.read_exact(&mut bytes).await.unwrap();
                    assert!(matches!(
                        serde_json::from_slice::<Request>(&bytes).unwrap(),
                        Request::Status {}
                    ));
                    let response = serde_json::to_vec(&ipc::Response::success(
                        serde_json::json!({"state": state}),
                    ))
                    .unwrap();
                    stream.write_u32(response.len() as u32).await.unwrap();
                    stream.write_all(&response).await.unwrap();
                    stream.shutdown().await.unwrap();
                }
            });
            let mut args = Args::try_parse_from(["marmish"]).unwrap();
            args.no_autostart = no_autostart;
            args.socket = explicit_socket.then(|| socket.clone());
            let resolved = tokio::time::timeout(
                Duration::from_secs(3),
                command_socket(&args, directory.path(), &config),
            )
            .await
            .expect("existing activating daemon must become ready")
            .unwrap();
            assert_eq!(resolved, socket);
            server.await.unwrap();

            // Once the listener is gone, neither restricted mode may launch a
            // replacement daemon. A missing endpoint must fail immediately.
            std::fs::remove_file(&socket).unwrap();
            assert!(matches!(
                tokio::time::timeout(
                    Duration::from_secs(1),
                    command_socket(&args, directory.path(), &config),
                )
                .await
                .expect("absent daemon must not enter startup wait"),
                Err(Error::Invalid("daemon_unavailable"))
            ));
            profile.cleanup_runtime().unwrap();
        }
    }

    #[test]
    fn usage_failures_have_transport_error_exit_status() {
        let error =
            Args::try_parse_from(["marmish", "--wait-seconds", "0", "peer", "--", "/bin/false"])
                .err()
                .unwrap();
        assert_eq!(usage_exit_code(&error), 255);
        let help = Args::try_parse_from(["marmish", "--help"]).err().unwrap();
        assert_eq!(usage_exit_code(&help), 0);
    }
    #[tokio::test]
    async fn new_session_is_only_a_new_remote_command_option() {
        let root = tempfile::tempdir().unwrap();
        let layout = crate::native_layout::ExecutableLayout {
            helper: root.path().join("missing-helper"),
            worker: root.path().join("missing-worker"),
        };
        for operation in ["status", "commands", "daemon", "release-info", "setup"] {
            let args = Args::try_parse_from(["marmish", "--new-session", operation]).unwrap();
            assert!(
                matches!(
                    run(args, false, &layout).await,
                    Err(Error::Invalid("new_session_operation"))
                ),
                "{operation}"
            );
        }
        let args = Args::try_parse_from([
            "marmish",
            "--new-session",
            "install",
            "--prefix",
            "/missing",
            "--bin-dir",
            "/missing/bin",
        ])
        .unwrap();
        assert!(matches!(
            run(args, false, &layout).await,
            Err(Error::Invalid("new_session_operation"))
        ));
        assert!(root.path().read_dir().unwrap().next().is_none());
        let args = Args::try_parse_from([
            "marmish",
            "--new-session",
            "--detach",
            "peer",
            "--",
            "/bin/echo",
        ])
        .unwrap();
        assert!(args.new_session && args.detach);
        assert!(matches!(args.command, Some(FrontendCommand::Remote(_))));
        let args =
            Args::try_parse_from(["marmish", "peer", "--", "/bin/echo", "--new-session"]).unwrap();
        assert!(!args.new_session);
    }
    #[test]
    fn foreground_is_only_valid_for_daemon_operation() {
        for operation in ["status", "init", "release-info", "setup", "commands"] {
            let args = Args::try_parse_from(["marmish", "--foreground", operation]).unwrap();
            assert!(matches!(
                validate_daemon_options(&args, false),
                Err(Error::Invalid("foreground_operation"))
            ));
        }
        for (raw, daemon_binary) in [
            (vec!["marmish", "--foreground", "daemon"], false),
            (vec!["marmishd", "--foreground"], true),
            (vec!["marmishd", "daemon", "--foreground"], true),
        ] {
            let args = Args::try_parse_from(raw).unwrap();
            validate_daemon_options(&args, daemon_binary).unwrap();
        }
    }
    #[test]
    fn parser_contract_is_consistent() {
        Args::command().debug_assert();
    }
    #[test]
    fn stream_cli_preserves_literal_argv_and_cancel_selection() {
        let args = Args::try_parse_from([
            "marmish",
            "stream",
            "peer",
            "--compress",
            "--",
            "printf",
            "%s",
            "a b;$(x)\\\"\n",
            "",
        ])
        .unwrap();
        let Some(FrontendCommand::Stream {
            argv,
            compress,
            idle_seconds,
            keepalive_seconds,
            ..
        }) = args.command
        else {
            panic!("stream command");
        };
        assert!(compress);
        assert_eq!((idle_seconds, keepalive_seconds), (660, 300));
        assert_eq!(
            argv,
            ["printf", "%s", "a b;$(x)\\\"\n", ""].map(OsString::from)
        );
        assert!(Args::try_parse_from(["marmish", "stream", "peer", "printf"]).is_err());
        let args =
            Args::try_parse_from(["marmish", "cancel", "--stream", &"00".repeat(16), "peer"])
                .unwrap();
        assert!(stream_action(&args));
        assert!(
            Args::try_parse_from([
                "marmish",
                "cancel",
                "--stream",
                &"00".repeat(16),
                "--latest"
            ])
            .is_err()
        );
    }
    #[tokio::test]
    async fn bare_identity_filters_command_listing_wait_and_output() {
        use nostr::nips::nip19::ToBech32;
        let remote = nostr::key::PublicKey::from_hex(&"02".repeat(32)).unwrap();
        let npub = remote.to_bech32().unwrap();
        let config = CommandConfig::from_bytes(
            &serde_json::to_vec(&serde_json::json!({
                "version":1,"role":"both","relays":crate::transport::default_relays(),
                "tor":"auto","trusted_remotes":[],"execution_clients":[],
                "command_policy":{"version":1},"cwd":null,"limits":{},"aliases":{}
            }))
            .unwrap(),
        )
        .unwrap();
        for operation in ["commands", "wait", "output"] {
            let mut raw = vec!["marmish", operation, &npub];
            if operation != "commands" {
                raw.push("--latest");
            }
            let args = Args::try_parse_from(raw).unwrap();
            let destination = match &args.command {
                Some(FrontendCommand::Commands { destination }) => destination.as_deref(),
                Some(FrontendCommand::Wait(select) | FrontendCommand::Output(select)) => {
                    select.destination.as_deref()
                }
                _ => panic!("target operation"),
            };
            assert_eq!(
                target_filter(&config, &args, destination).await.unwrap(),
                Some(remote.to_bytes())
            );
        }
    }
    #[test]
    fn remote_separator_preserves_options_and_non_utf8_argv() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let bytes = OsString::from_vec(vec![b'v', 0xff]);
        let args = Args::try_parse_from(vec![
            OsString::from("marmish"),
            "npub@relay.example".into(),
            "--".into(),
            "/bin/echo".into(),
            "--debug".into(),
            bytes.clone(),
        ])
        .unwrap();
        assert!(!args.debug);
        match args.command {
            Some(FrontendCommand::Remote(values)) => {
                assert_eq!(values.last().unwrap().as_bytes(), bytes.as_bytes());
                assert_eq!(values.first().unwrap(), "npub@relay.example");
            }
            _ => panic!("remote"),
        }
    }
    #[test]
    fn destination_requires_immediate_separator() {
        for middle in [vec![], vec!["-i", "prod"], vec!["--detach"], vec!["extra"]] {
            let mut raw = vec!["marmish", "peer"];
            raw.extend(middle.iter().copied());
            raw.extend(["--", "/bin/true"]);
            let args = Args::try_parse_from(raw).unwrap();
            let Some(FrontendCommand::Remote(values)) = args.command else {
                panic!("remote")
            };
            let result = validate_separator(&values, &[OsString::from("/bin/true")], true);
            assert_eq!(result.is_ok(), middle.is_empty());
        }
    }
    #[test]
    fn explicit_user_without_host_and_selector_requirements() {
        let args = Args::try_parse_from([
            "marmish",
            "-u",
            "remote",
            "--relay",
            "wss://relay.example/",
            "--",
            "/bin/true",
        ])
        .unwrap();
        assert!(matches!(args.command, Some(FrontendCommand::Remote(_))));
        assert!(Args::try_parse_from(["marmish", "wait"]).is_err());
        assert!(Args::try_parse_from(["marmish", "wait", "--latest"]).is_ok());
        assert!(
            Args::try_parse_from(["marmish", "wait", "--latest", "--invocation", "abc"]).is_err()
        );
        assert!(Args::try_parse_from(["marmish", "wait", "target"]).is_err());
        assert!(matches!(
            Args::try_parse_from(["marmish", "status"]).unwrap().command,
            Some(FrontendCommand::Status)
        ));
    }
    #[test]
    fn authorization_explicit_global_allowlist_rejects_defaults_in_both_positions() {
        for options in [
            vec!["--key-storage", "auto"],
            vec!["--retention-seconds", "86400"],
            vec!["--wait-seconds", "180"],
            vec!["--tor", "off"],
            vec!["--socket", "/unused"],
            vec!["--relay", "wss://relay.example/"],
            vec!["--new-session"],
        ] {
            for after in [false, true] {
                let mut raw = vec!["marmish"];
                if !after {
                    raw.extend(options.iter().copied());
                }
                raw.push("keychain-authorize");
                if after {
                    raw.extend(options.iter().copied());
                }
                let matches = Args::command().try_get_matches_from(raw).unwrap();
                assert!(
                    authorization_disallowed(&matches),
                    "{options:?} after={after}"
                );
            }
        }
        for raw in [
            vec!["marmish", "keychain-authorize"],
            vec![
                "marmish",
                "--debug",
                "-i",
                "named",
                "--state-dir",
                "/unused",
                "keychain-authorize",
            ],
            vec!["marmish", "keychain-authorize", "--debug", "-i", "named"],
        ] {
            let matches = Args::command().try_get_matches_from(raw).unwrap();
            assert!(
                !authorization_disallowed(&matches),
                "{:?}",
                matches
                    .ids()
                    .map(|id| (id.as_str(), matches.value_source(id.as_str())))
                    .collect::<Vec<_>>()
            );
        }
    }
    #[tokio::test]
    async fn authorization_frontend_and_non_mac_rejection_precede_profile_access() {
        let root = tempfile::tempdir().unwrap();
        let layout = crate::native_layout::ExecutableLayout {
            helper: root.path().join("missing"),
            worker: root.path().join("missing-worker"),
        };
        let args = Args::try_parse_from(["marmishd", "keychain-authorize"]).unwrap();
        assert!(matches!(
            run(args, true, &layout).await,
            Err(Error::Invalid("keychain_authorize_arguments"))
        ));
        if !cfg!(target_os = "macos") {
            let args = Args::try_parse_from([
                "marmish",
                "--state-dir",
                root.path().to_str().unwrap(),
                "keychain-authorize",
            ])
            .unwrap();
            assert!(matches!(
                run(args, false, &layout).await,
                Err(Error::Identity("keychain_authorize_unsupported"))
            ));
        }
        assert!(root.path().read_dir().unwrap().next().is_none());
    }
}
