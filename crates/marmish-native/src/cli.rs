use crate::{
    Error, Result,
    daemon::{self, Config},
    foundation_check,
    identity::{BackendChoice, socket_path_for},
    ipc::{self, Request},
    transport::{RelayPolicy, RouteMode, TransportLimits},
};
use clap::{Parser, Subcommand, ValueEnum};
use std::{path::PathBuf, process::ExitCode};

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
    #[arg(long, default_value = "auto", global = true)]
    tor: String,
    #[arg(long, value_enum, default_value = "auto", global = true)]
    key_storage: Backend,
    #[arg(long, global = true)]
    allow_peer: Vec<String>,
    #[arg(long, default_value_t = 86_400, global = true)]
    retention_seconds: u64,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize encrypted native storage without connecting to relays.
    Init,
    /// Run this identity's serialized transport owner.
    Daemon,
    Status,
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

async fn run(args: Args, daemon_binary: bool) -> Result<bool> {
    let mode: RouteMode = args.tor.parse()?;
    if matches!(args.command, Command::ValidateExecution) {
        let report = crate::mdk_adapter::command_check::run(args.relay, mode).await?;
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|_| Error::Invalid("report_encoding"))?
        );
        return Ok(report["ok"].as_bool() == Some(true));
    }
    if matches!(args.command, Command::FoundationCheck) {
        let report = foundation_check::run(args.relay, mode).await?;
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|_| Error::Invalid("report_encoding"))?
        );
        return Ok(report["ok"].as_bool() == Some(true));
    }
    let state = state_dir(args.state_dir)?;
    if matches!(args.command, Command::Init | Command::Daemon) {
        let config = Config {
            state_dir: state,
            identity: args.identity,
            backend: args.key_storage.into(),
            helper: std::env::current_exe()?,
            relay_policy: RelayPolicy::parse(&args.relay, mode, TransportLimits::default())?,
            allowed_peers: args.allow_peer,
            retention: std::time::Duration::from_secs(args.retention_seconds),
        };
        if matches!(args.command, Command::Init) {
            let (profile, owner) = daemon::open(config).await?;
            let snapshot = owner.status();
            let stopped = daemon::stop(profile, owner).await;
            let snapshot = snapshot?;
            stopped?;
            println!(
                "{}",
                serde_json::to_string(&snapshot).map_err(|_| Error::Invalid("status_encoding"))?
            );
        } else {
            daemon::run(config).await?;
        }
        return Ok(true);
    }
    if daemon_binary {
        return Err(Error::Invalid("daemon_operation"));
    }
    let socket = match args.socket {
        Some(path) => path,
        None => socket_path_for(&state, &args.identity)?,
    };
    let request = match args.command {
        Command::Status => Request::Status {},
        Command::OpenConversation { peer } => Request::OpenConversation { peer },
        Command::Send { session, id, data } => Request::Send { session, id, data },
        Command::Received { after, limit } => Request::Received { after, limit },
        _ => return Err(Error::Invalid("client_operation")),
    };
    let response = ipc::request(&socket, &request).await?;
    println!(
        "{}",
        serde_json::to_string(&response).map_err(|_| Error::Invalid("response_encoding"))?
    );
    Ok(response.ok)
}

pub fn main(daemon_binary: bool) -> ExitCode {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("_vault-helper")) {
        return marmish_vault::helper::run();
    }
    let args = Args::parse();
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
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run(args, daemon_binary)) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("marmish: {error}");
            ExitCode::FAILURE
        }
    }
}
