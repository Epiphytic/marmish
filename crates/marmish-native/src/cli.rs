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
use clap::{Parser, Subcommand, ValueEnum};
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
    trailing: Option<Vec<OsString>>,
    #[arg(long, global = true)]
    debug: bool,
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

async fn run(
    mut args: Args,
    daemon_binary: bool,
    layout: &crate::native_layout::ExecutableLayout,
) -> Result<u8> {
    if args.command.is_none() && daemon_binary {
        args.command = Some(FrontendCommand::Daemon);
    }
    if (args.max_commands.is_some() || args.max_uptime.is_some())
        && !matches!(args.command, Some(FrontendCommand::Daemon))
    {
        return Err(Error::Invalid("server_limit_operation"));
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
            daemon::run(config).await?;
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

fn spawn_service(child: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    // A separate session survives terminal hangup and client Ctrl-C. Only the
    // async-signal-safe setsid syscall runs between fork and exec.
    unsafe {
        child.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    child.spawn()
}

async fn command_socket(
    args: &Args,
    state: &std::path::Path,
    config: &CommandConfig,
) -> Result<PathBuf> {
    use std::process::Stdio;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
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
            let executable = std::env::current_exe()?.canonicalize()?;
            let daemon = executable
                .parent()
                .ok_or(Error::Invalid("daemon_installation"))?
                .join("marmishd");
            let mut child = std::process::Command::new(daemon);
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
            child
                .arg("daemon")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // No inherited terminal or output pipe holds the client open. The
            // launched service owns its profile lease and survives this client.
            let mut launched = spawn_service(&mut child)?;
            std::thread::spawn(move || {
                let _ = launched.wait();
            });
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
    if raw.get(1).map(OsString::as_os_str) == Some(OsStr::new("_vault-helper")) {
        if let Err(error) = crate::validate_executable_layout() {
            eprintln!("marmish: {error}");
            return ExitCode::from(255);
        }
        return marmish_vault::helper::run();
    }
    let trailing = raw
        .iter()
        .position(|v| v == OsStr::new("--"))
        .map(|i| raw[i + 1..].to_vec());
    let mut args = match Args::try_parse_from(raw) {
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
    args.trailing = trailing;
    let failure_code = if command_action(&args) { 255 } else { 1 };
    // Parsing needs no provider or private state. Canonical layout validation
    // still precedes every provider/profile access.
    let layout = match std::env::current_exe()
        .map_err(Error::from)
        .and_then(|path| crate::native_layout::resolve(&path))
    {
        Ok(layout) => layout,
        Err(error) => {
            eprintln!("marmish: {error}");
            return ExitCode::from(failure_code);
        }
    };
    if args.debug {
        crate::diagnostics::enable();
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("marmish: runtime_startup");
            return ExitCode::from(failure_code);
        }
    };
    match runtime.block_on(run(args, daemon_binary, &layout)) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("marmish: {error}");
            ExitCode::from(failure_code)
        }
    }
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
    #[test]
    fn service_launch_owns_a_separate_terminal_session() {
        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "exec sleep 30"]);
        let mut child = spawn_service(&mut command).unwrap();
        let pid = child.id() as libc::pid_t;
        assert_eq!(unsafe { libc::getsid(pid) }, pid);
        assert_eq!(unsafe { libc::getpgid(pid) }, pid);
        assert_ne!(unsafe { libc::getsid(0) }, pid);
        child.kill().unwrap();
        child.wait().unwrap();
    }
    #[test]
    fn parser_contract_is_consistent() {
        Args::command().debug_assert();
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
}
