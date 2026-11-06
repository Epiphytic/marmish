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
#[command(group(clap::ArgGroup::new("selector").required(true).args(["handle", "handle_file", "invocation", "latest"])))]
struct SelectArgs {
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

#[derive(Subcommand)]
enum FrontendCommand {
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
    if args.new_session && !matches!(args.command, Some(FrontendCommand::Remote(_))) {
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
    if daemon_binary && command_action(&args) {
        return Err(Error::Invalid("daemon_operation"));
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
        if args.no_autostart || args.socket.is_some() {
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
    use crate::command_target::{self, RelaySource};
    let target = command_target::parse(
        config,
        destination,
        args.user.as_deref(),
        &args.relay,
        &args.srv,
    )?;
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

pub fn main(daemon_binary: bool) -> ExitCode {
    let raw = std::env::args_os().collect::<Vec<_>>();
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
    let trailing = raw
        .iter()
        .position(|v| v == OsStr::new("--"))
        .map(|i| raw[i + 1..].to_vec());
    let matches = match Args::command().try_get_matches_from(raw.clone()) {
        Ok(matches) => matches,
        Err(error) => {
            let code = usage_exit_code(&error);
            let _ = error.print();
            if code != 0 {
                eprintln!("marmish: invalid_arguments");
            }
            return ExitCode::from(code);
        }
    };
    let authorize_disallowed = authorization_disallowed(&matches);
    let mut args = match Args::from_arg_matches(&matches) {
        Ok(args) => args,
        Err(_) => return ExitCode::from(255),
    };
    args.authorize_disallowed = authorize_disallowed;
    args.trailing = trailing;
    let failure_code = if command_action(&args) { 255 } else { 1 };
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
    match runtime.block_on(run(args, daemon_binary, &layout)) {
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
            ExitCode::from(failure_code)
        }
    }
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
