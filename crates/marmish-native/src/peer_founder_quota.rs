//! One founder path in an isolated test process. Default-VFS delegation remains
//! active only outside the protected directory family.

use super::*;

#[path = "peer_founder_native.rs"]
mod native;

pub(crate) use native::{Created, Expected, Image};

const LEDGER: &str = "founder-ledger.db";
const LEDGER_LIMIT: u64 = 1024 * 1024;

pub(super) struct FounderPaths {
    directory: PathBuf,
    identity: (u64, u64),
    registered: OnceLock<PathBuf>,
}

impl FounderPaths {
    pub(super) fn protects(&self, path: &Path) -> bool {
        let in_family = |path: &Path| {
            path.parent()
                .zip(path.file_name())
                .is_some_and(|(parent, name)| {
                    is_family(name)
                        && (parent == self.directory
                            || parent.canonicalize().as_ref().ok() == Some(&self.directory))
                })
        };
        if in_family(path) || path.canonicalize().is_ok_and(|path| in_family(&path)) {
            return true;
        }
        let Ok(meta) = path.metadata() else {
            return false;
        };
        if meta.nlink() <= 1 || meta.dev() != self.identity.0 {
            return false;
        }
        // Ordinary single-inode paths don't walk the managed directory.
        // A suspicious same-volume alias must fail closed if inventory cannot
        // be inspected, including before selection of a single main.
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return true;
        };
        entries.enumerate().any(|(index, entry)| {
            // This family has at most a ledger, ledger journal, one main and
            // its journal. Extra entries cannot establish alias safety.
            index >= 5
                || entry.map_or(true, |entry| {
                    is_family(&entry.file_name())
                        && entry
                            .path()
                            .symlink_metadata()
                            .map_or(true, |file| witness(&file) == witness(&meta))
                })
        })
    }

    fn recheck(&self) -> Result<()> {
        let metadata = self
            .directory
            .symlink_metadata()
            .map_err(|_| "founder_dir")?;
        if witness(&metadata) != self.identity
            || !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7777 != 0o700
            || self.directory.canonicalize().as_ref().ok() != Some(&self.directory)
        {
            return Err("founder_dir");
        }
        Ok(())
    }
}

fn is_family(leaf: &OsStr) -> bool {
    let Some(name) = leaf.to_str() else {
        return false;
    };
    let main = SIDECARS
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .unwrap_or(name);
    main.strip_prefix("founder-")
        .and_then(|value| value.strip_suffix(".db"))
        .is_some_and(|digits| {
            !digits.starts_with('0')
                && digits.bytes().all(|byte| byte.is_ascii_digit())
                && digits.parse::<NonZeroU64>().is_ok()
        })
}

pub(crate) struct FounderQuota<'a> {
    dispatch: &'static Dispatch,
    paths: &'static FounderPaths,
    lease: &'a ProfileLease,
    quarantined: bool,
    stop_in_transaction: bool,
}

impl<'a> FounderQuota<'a> {
    pub(crate) fn mount(lease: &'a ProfileLease) -> Result<Self> {
        let directory = lease.root().join("peer");
        let metadata = directory.symlink_metadata().map_err(|_| "founder_dir")?;
        let identity = witness(&metadata);
        if lease
            .root()
            .symlink_metadata()
            .map_err(|_| "founder_profile")?
            .dev()
            != identity.0
            || !local_volume(&directory)?
        {
            return Err("founder_dir");
        }
        let candidate = FounderPaths {
            directory,
            identity,
            registered: OnceLock::new(),
        };
        candidate.recheck()?;
        let dispatch = Dispatch::mount()?;
        dispatch.assert_default()?;
        let _lock = dispatch.file_turn.try_lock().map_err(|_| "founder_busy")?;
        if dispatch.paths.get().is_some() {
            return Err("founder_other_owner");
        }
        dispatch
            .founder_paths
            .set(candidate)
            .map_err(|_| "founder_other_owner")?;
        let paths = dispatch
            .founder_paths
            .get()
            .expect("founder_path_registration");
        Ok(Self {
            dispatch,
            paths,
            lease,
            quarantined: false,
            stop_in_transaction: false,
        })
    }

    pub(crate) fn fixture_stop_in_synced_transaction(&mut self) {
        self.stop_in_transaction = true;
    }

    pub(crate) fn owns(&self, lease: &ProfileLease) -> bool {
        ptr::eq(self.lease, lease)
    }

    pub(crate) fn main(&self, generation: NonZeroU64) -> PathBuf {
        self.paths
            .directory
            .join(format!("founder-{generation}.db"))
    }

    pub(crate) fn ledger(&self) -> PathBuf {
        self.paths.directory.join(LEDGER)
    }

    pub(crate) fn inventory(&self, exact_main: Option<&Path>) -> Result<u64> {
        self.paths.recheck()?;
        let ledger = self.ledger();
        let journal = sidecar(&ledger, "-journal");
        fs::read_dir(&self.paths.directory)
            .map_err(|_| "founder_inventory")?
            .try_fold(0u64, |total, entry| {
                let path = entry.map_err(|_| "founder_inventory")?.path();
                if exact_main == Some(path.as_path()) {
                    return Ok(total);
                }
                if path != ledger && path != journal {
                    return Err("founder_inventory");
                }
                total
                    .checked_add(bytes(&path)?.ok_or("founder_inventory")?)
                    .filter(|amount| *amount <= LEDGER_LIMIT)
                    .ok_or("founder_inventory")
            })
    }

    fn run<T>(
        &mut self,
        generation: NonZeroU64,
        file: (u64, u64),
        operation: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<(T, u64)> {
        if self.quarantined {
            return Err("founder_quarantined");
        }
        self.quarantined = true;
        let result = self.run_once(generation, file, operation);
        self.quarantined = result.is_err();
        result
    }

    fn run_once<T>(
        &self,
        generation: NonZeroU64,
        file: (u64, u64),
        operation: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<(T, u64)> {
        self.dispatch.assert_default()?;
        let _lock = self
            .dispatch
            .file_turn
            .try_lock()
            .map_err(|_| "founder_busy")?;
        let main = self.main(generation);
        let registered = self.paths.registered.get_or_init(|| main.clone());
        if *registered != main {
            return Err("founder_path_binding");
        }
        let initial = bytes(&main)?.ok_or("founder_file")?;
        let measured = main.symlink_metadata().map_err(|_| "founder_file")?;
        if measured.dev() != self.paths.identity.0
            || witness(&measured) != file
            || initial > CLOSED_STAGE_LIMIT
        {
            return Err("founder_file");
        }
        let mut quota = Quota::new(
            self.dispatch.managed,
            &main,
            TOTAL_ADMISSION - self.inventory(Some(&main))?,
            false,
        )?;
        let state = Arc::get_mut(&mut quota).expect("new_founder_quota");
        state.witness = Some(file);
        state.founder_main_limit = Some(CLOSED_STAGE_LIMIT);
        state
            .crash_after_database_write
            .store(self.stop_in_transaction, Ordering::Relaxed);
        let _active = Active::enter(&quota)?;
        let value = operation(&main)?;
        if Arc::strong_count(&quota) != 2 {
            return Err("founder_open_handles");
        }
        self.dispatch.assert_default()?;
        self.paths.recheck()?;
        quota.verify_dispatch()?;
        let evidence = quota.evidence();
        Ok((value, evidence.journal_writes))
    }
}

struct Active;

impl Active {
    fn enter(quota: &Arc<Quota>) -> Result<Self> {
        ACTIVE.with(|scope| {
            let mut held = scope.borrow_mut();
            if held.is_some() {
                return Err("founder_recursive_turn");
            }
            *held = Some(quota.clone());
            Ok(Self)
        })
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        ACTIVE.with(|scope| scope.borrow_mut().take());
    }
}
