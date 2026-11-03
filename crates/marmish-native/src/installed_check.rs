//! Feature-gated build tooling. No installed runtime entry point dispatches here.
#[path = "installed_cleanup.rs"]
mod cleanup;
#[path = "installed_fixture.rs"]
mod fixture;
pub(super) use cleanup::OwnedCleanupGuard;
#[path = "installed_matrix.rs"]
mod matrix;
#[path = "installed_prepare.rs"]
mod prepare;
#[path = "installed_process.rs"]
mod process;
#[path = "installed_records.rs"]
mod records;
use crate::{Error, Result};
use clap::{Parser, Subcommand};
use records::{Context, ROLES, TARGETS};
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Parser)]
#[command(name = "marmish-installed-check")]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    ReleaseInfo,
    Run {
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        identities: PathBuf,
        #[arg(long)]
        inbox: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Rollup {
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        records: PathBuf,
        #[arg(long)]
        matrix_result: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Install the exact candidate and create two fresh offline identities.
    Prepare {
        #[arg(long)]
        context: PathBuf,
        #[arg(long)]
        assets: PathBuf,
        #[arg(long)]
        bootstrap: PathBuf,
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        role: String,
        #[arg(long)]
        output: PathBuf,
    },
    /// Produce the immutable common run binding from actual artifact bytes.
    Context(ContextOptions),
}
#[derive(clap::Args)]
struct ContextOptions {
    #[arg(long)]
    repository: String,
    #[arg(long)]
    run_id: u64,
    #[arg(long)]
    run_attempt: u64,
    #[arg(long)]
    candidate_run_id: u64,
    #[arg(long)]
    candidate_run_attempt: u64,
    #[arg(long)]
    source: String,
    #[arg(long)]
    harness_sha256: String,
    #[arg(long)]
    manifests: PathBuf,
    #[arg(long)]
    tooling_manifests: PathBuf,
    #[arg(long)]
    bootstrap: PathBuf,
    #[arg(long, default_value = "matrix")]
    cohort: String,
    #[arg(long = "relay", required = true)]
    relays: Vec<String>,
    #[arg(long)]
    expires_at: u64,
    #[arg(long)]
    output: PathBuf,
}

fn now() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| Error::Invalid("installed_clock"))
}
fn context(o: &ContextOptions) -> Result<Context> {
    let mut pins = BTreeMap::new();
    let mut tools = BTreeMap::new();
    let mut tooling = Vec::new();
    let mut version = None;
    for (role, target) in ROLES.into_iter().zip(TARGETS) {
        let bytes = manifest_bytes(&o.manifests.join(format!("{role}.manifest")))?;
        let m = crate::native_manifest::Manifest::parse(&bytes)?;
        if m.target != target
            || m.commit != o.source
            || version.as_ref().is_some_and(|v| v != &m.version)
        {
            return Err(Error::Invalid("installed_manifest"));
        }
        version = Some(m.version);
        pins.insert(role.to_owned(), records::digest(&bytes));
        let b = records::read_bytes(
            &o.tooling_manifests.join(format!("{role}-tooling.json")),
            65536,
        )?;
        let t: records::ToolingManifest =
            serde_json::from_slice(&b).map_err(|_| Error::Invalid("installed_tooling"))?;
        t.validate(role, &o.source, &o.harness_sha256)?;
        tools.insert(role.to_owned(), records::digest(&b));
        tooling.push(t);
    }
    let mut nonce = [0; 32];
    getrandom::fill(&mut nonce).map_err(|_| Error::Identity("random"))?;
    let c = Context {
        schema: 1,
        repository: o.repository.clone(),
        run_id: o.run_id,
        run_attempt: o.run_attempt,
        candidate_run_id: o.candidate_run_id,
        candidate_run_attempt: o.candidate_run_attempt,
        nonce: hex::encode(nonce),
        bootstrap_sha256: records::digest(&records::read_bytes(&o.bootstrap, 131072)?),
        source_commit: o.source.clone(),
        harness_sha256: o.harness_sha256.clone(),
        cohort: o.cohort.clone(),
        roles: ROLES.map(str::to_owned),
        relays: o.relays.clone(),
        manifests: pins,
        tooling_manifests: tools,
        tooling: tooling
            .try_into()
            .map_err(|_| Error::Invalid("installed_tooling"))?,
        version: version.ok_or(Error::Invalid("installed_manifest"))?,
        created_at: now()?,
        expires_at: o.expires_at,
    };
    c.validate()?;
    Ok(c)
}
fn manifest_bytes(path: &std::path::Path) -> Result<Vec<u8>> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)?;
    let m = f.metadata()?;
    if !m.is_file()
        || m.nlink() != 1
        || m.len() > 4096
        || m.mode() & 0o022 != 0
        || m.uid() != unsafe { libc::geteuid() }
    {
        return Err(Error::Invalid("installed_manifest"));
    }
    let mut b = Vec::new();
    f.by_ref().take(4097).read_to_end(&mut b)?;
    if b.len() > 4096 {
        return Err(Error::Invalid("installed_manifest"));
    }
    Ok(b)
}
pub(crate) async fn entry() -> u8 {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == "--release-info")
        && std::env::args_os().count() == 2
    {
        return release_info();
    }
    let result = match Args::parse().command {
        Command::Run {
            context,
            workspace,
            identities,
            inbox,
            output,
        } => {
            matrix::run(matrix::Run {
                context,
                workspace,
                identities,
                inbox,
                output,
            })
            .await
        }
        Command::Rollup {
            context,
            records,
            matrix_result,
            output,
        } => matrix::rollup(&context, &records, &matrix_result, &output),
        Command::ReleaseInfo => {
            return release_info();
        }
        Command::Prepare {
            context,
            assets,
            bootstrap,
            workspace,
            role,
            output,
        } => {
            prepare::run(prepare::Prepare {
                context,
                assets,
                bootstrap,
                workspace,
                role,
                output,
            })
            .await
        }
        Command::Context(o) => context(&o).and_then(|c| records::write(&o.output, &c)),
    };
    match result {
        Ok(()) => 0,
        Err(_) => {
            eprintln!("marmish-installed-check: invalid_or_failed");
            1
        }
    }
}

pub(crate) fn fixture_entry() -> u8 {
    fixture::entry()
}

pub(super) fn release_info() -> u8 {
    use std::io::Write;
    let result = serde_json::to_vec(&crate::native_manifest::compiled_release_metadata())
        .map_err(|_| ())
        .and_then(|mut b| {
            b.push(b'\n');
            std::io::stdout().write_all(&b).map_err(|_| ())
        });
    if result.is_ok() { 0 } else { 1 }
}
