//! Private candidate example. The ordinary installed launchers cannot enter here.
use crate::{
    Error, Result, daemon,
    daemon_launch::StartupReporter,
    identity::{BackendChoice, validate_name},
    native_install, native_layout, native_manifest,
    transport::{RelayPolicy, RouteMode, TransportLimits},
};
use rustls::pki_types::CertificateDer;
use std::{
    ffi::OsString,
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};
use x509_parser::prelude::parse_x509_certificate;

const CA_LIMIT: u64 = 16 * 1024;

struct Inputs {
    daemon: PathBuf,
    state_dir: PathBuf,
    identity: String,
    policy: RelayPolicy,
    ca: Option<PathBuf>,
}

fn arguments(mut args: impl Iterator<Item = OsString>) -> Result<Inputs> {
    fn missing() -> Error {
        Error::Invalid("qualification_arguments")
    }

    let daemon = PathBuf::from(args.next().ok_or_else(missing)?);
    let state_dir = PathBuf::from(args.next().ok_or_else(missing)?);
    let identity = args
        .next()
        .and_then(|s| s.into_string().ok())
        .ok_or_else(missing)?;
    let relay = args
        .next()
        .and_then(|s| s.into_string().ok())
        .ok_or_else(missing)?;
    let mode: RouteMode = args
        .next()
        .and_then(|s| s.into_string().ok())
        .ok_or_else(missing)?
        .parse()?;
    let ca = args.next().map(PathBuf::from);
    if args.next().is_some()
        || !daemon.is_absolute()
        || !state_dir.is_absolute()
        || ca.as_ref().is_some_and(|p| !p.is_absolute())
    {
        return Err(missing());
    }
    validate_name(&identity)?;
    let policy = RelayPolicy::parse(&[relay], mode, TransportLimits::default())?;
    if ca.is_some()
        && (policy.needs_tor()
            || !policy.urls().first().is_some_and(|url| {
                url.scheme() == "wss"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port().is_some()
                    && url.path() == "/"
            }))
    {
        return Err(Error::Invalid("qualification_fixture_route"));
    }
    Ok(Inputs {
        daemon,
        state_dir,
        identity,
        policy,
        ca,
    })
}

fn read_ca(path: &Path) -> Result<CertificateDer<'static>> {
    fn invalid() -> Error {
        Error::Invalid("qualification_fixture_ca")
    }

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| invalid())?;
    let metadata = file.metadata().map_err(|_| invalid())?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.nlink() != 1
        || !matches!(metadata.mode() & 0o7777, 0o400 | 0o600)
        || !(1..=CA_LIMIT).contains(&metadata.len())
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(CA_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() as u64 != metadata.len() {
        return Err(invalid());
    }
    let (rest, _) = parse_x509_certificate(&bytes).map_err(|_| invalid())?;
    if !rest.is_empty() {
        return Err(invalid());
    }
    Ok(CertificateDer::from(bytes))
}

fn installed(daemon: &Path) -> Result<native_layout::ExecutableLayout> {
    fn invalid() -> Error {
        Error::Invalid("qualification_installation")
    }

    if daemon.file_name().and_then(|value| value.to_str()) != Some("marmishd")
        || daemon.canonicalize()? != daemon
    {
        return Err(invalid());
    }
    let version = daemon.parent().ok_or_else(invalid)?;
    let prefix = version
        .parent()
        .filter(|dir| dir.file_name().is_some_and(|name| name == "versions"))
        .and_then(Path::parent)
        .ok_or_else(invalid)?;
    let id = version
        .file_name()
        .and_then(|id| id.to_str())
        .ok_or_else(invalid)?;
    let receipt = native_install::read_receipt(prefix)?;
    let (manifest, _) = native_install::read_version(prefix, id, &receipt.install_id)?;
    for asset in &manifest.files {
        native_manifest::verify_file(&version.join(asset.name), asset, true)?;
    }
    native_layout::resolve(daemon)
}

pub(super) fn entry() -> ExitCode {
    if let Err(error) = run() {
        if matches!(error, Error::DaemonStartup(_)) {
            eprintln!("marmish qualification: startup failed");
        } else {
            eprintln!("marmish qualification: {error}");
        }
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run() -> Result<()> {
    let startup = StartupReporter::take_registered_from_env()?;
    marmish_engine::initialize_process_limits().map_err(|_| Error::Invalid("process_limits"))?;
    let inputs = arguments(std::env::args_os().skip(1))?;
    let layout = installed(&inputs.daemon)?;
    let roots = inputs
        .ca
        .as_deref()
        .map(read_ca)
        .transpose()?
        .map(|ca| vec![ca]);
    let config = daemon::Config {
        state_dir: inputs.state_dir,
        identity: inputs.identity,
        backend: BackendChoice::File,
        helper: layout.helper,
        relay_policy: inputs.policy,
        allowed_peers: Vec::new(),
        retention: Duration::from_secs(86_400),
        max_commands: None,
        max_uptime: None,
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(Box::pin(daemon::registered::run(
            config,
            startup,
            &inputs.daemon,
            roots,
        )))
}

#[cfg(test)]
#[path = "registered_qualification_tests.rs"]
mod tests;
