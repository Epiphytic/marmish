//! Exact process and socket proof for live Terminal registry recovery.
use super::store::{Host, Proof, Registry, Slot, State};
use super::tmux::{self, Inventory, Manager};
use super::{CLEANUP, UNAVAILABLE};
use crate::{Error, Result};
use std::ffi::{CStr, CString};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io;
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::time::{Duration, Instant};

const STORAGE_CAPACITY: Error = Error::Storage("terminal_slot_storage_capacity");
const BOOT: Error = Error::Storage("terminal_boot_proof");
const MAX_PATHS: usize = 64;
const EXIT_OBSERVATION: Duration = Duration::from_secs(1);
const EXIT_POLL: Duration = Duration::from_millis(10);

impl Host {
    pub fn current() -> Result<Self> {
        let boot = marmish_engine::system_boot_id().map_err(|_| BOOT)?;
        #[cfg(target_os = "linux")]
        let namespace = file_id(&File::open("/proc/self/ns/pid")?.metadata()?);
        #[cfg(target_os = "macos")]
        let namespace = (0, 0);
        Ok(Self { boot, namespace })
    }

    fn proof(self, inventory: Inventory) -> Proof {
        Proof {
            host: self,
            pid: inventory.pid,
            directory: inventory.directory,
            socket: inventory.socket,
        }
    }
}

pub(super) fn check_capacity(manager: &Manager) -> Result<()> {
    tmux::check_directory(&manager.root)?;
    let mut paths = fs::read_dir(&manager.root)?;
    for _ in 0..MAX_PATHS {
        match paths.next() {
            None => return Ok(()),
            Some(entry) => {
                entry?;
            }
        }
    }
    Err(STORAGE_CAPACITY)
}

pub(super) fn pin_ready(registry: &mut Registry, slot: Slot, inventory: Inventory) -> Result<()> {
    let proof = Host::current()?.proof(inventory);
    match slot.state {
        State::Prepared => registry.ready(slot, inventory.session, Some(proof)),
        _ if slot.proof.is_none() => registry.pin(slot, proof, inventory.session),
        _ if slot.proof == Some(proof) => Ok(()),
        _ => Err(UNAVAILABLE),
    }
}

pub(super) fn inventory(
    registry: &mut Registry,
    manager: &mut Manager,
    slot: Slot,
    until: Instant,
) -> Result<Inventory> {
    if let Some(proof) = slot.proof
        && proof.host != Host::current()?
    {
        return Err(UNAVAILABLE);
    }
    let id = match slot.state {
        State::Prepared => None,
        State::Ready(id) => Some(id),
        State::Retiring(id) => id,
    };
    let inventory = manager.inventory_until(slot, id, until)?;
    pin_ready(registry, slot, inventory)?;
    Ok(inventory)
}

pub(super) fn retire_if_absent(
    registry: &mut Registry,
    manager: &Manager,
    slot: Slot,
    deadline: Instant,
) -> Result<()> {
    let Some(proof) = slot.proof else {
        if matches!(slot.state, State::Prepared | State::Retiring(None)) {
            let root = checked_root(manager)?;
            let name = slot_name(slot)?;
            if child_attributes(&root, &name)?.is_none() {
                return registry.retire_unstarted(slot);
            }
        }
        return Err(UNAVAILABLE);
    };
    let host = Host::current()?;
    let until = deadline.min(Instant::now() + EXIT_OBSERVATION);
    while !process_absent(host, proof, probe_process) {
        if host != proof.host || Instant::now() >= until {
            return Err(CLEANUP);
        }
        std::thread::sleep(EXIT_POLL.min(until.saturating_duration_since(Instant::now())));
    }

    finish_retirement(registry, manager, slot, proof, host)
}

fn finish_retirement(
    registry: &mut Registry,
    manager: &Manager,
    slot: Slot,
    proof: Proof,
    host: Host,
) -> Result<()> {
    let path = path_proof(manager, slot, proof);
    if host.boot != proof.host.boot && path.is_err() {
        registry.retire_old_boot(slot, proof)?;
        crate::diagnostics::event("terminal_recovery", "old_private_directory_preserved");
        return Ok(());
    }
    path?;
    if !matches!(slot.state, State::Retiring(_)) {
        registry.retiring(slot)?;
    }
    remove_owned_path(manager, slot, proof)?;
    registry.retired(slot)
}

fn probe_process(pid: i32) -> bool {
    // SAFETY: a strictly positive pid is checked below. Signal zero performs
    // existence/permission checks and does not deliver a signal.
    pid > 0
        && unsafe { libc::kill(pid, 0) } == -1
        && absent_errno(io::Error::last_os_error().raw_os_error())
}

fn absent_errno(errno: Option<i32>) -> bool {
    errno == Some(libc::ESRCH)
}

fn process_absent(host: Host, proof: Proof, probe: impl FnOnce(i32) -> bool) -> bool {
    host.boot != proof.host.boot || host.namespace == proof.host.namespace && probe(proof.pid)
}

struct OwnedPath {
    root: File,
    name: CString,
    directory: Option<File>,
    has_socket: bool,
}

fn checked_root(manager: &Manager) -> Result<File> {
    tmux::check_directory(&manager.root)?;
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(&manager.root)?;
    if file_id(&root.metadata()?) != file_id(&fs::symlink_metadata(&manager.root)?) {
        return Err(UNAVAILABLE);
    }
    Ok(root)
}

fn slot_name(slot: Slot) -> Result<CString> {
    CString::new(hex::encode(slot.id.0)).map_err(|_| UNAVAILABLE)
}

fn path_proof(manager: &Manager, slot: Slot, proof: Proof) -> Result<OwnedPath> {
    let root = checked_root(manager)?;
    let name = slot_name(slot)?;
    let directory = match open_child(&root, &name) {
        Ok(directory) => directory,
        Err(error)
            if error.raw_os_error() == Some(libc::ENOENT)
                && matches!(slot.state, State::Retiring(_)) =>
        {
            return Ok(OwnedPath {
                root,
                name,
                directory: None,
                has_socket: false,
            });
        }
        Err(_) => return Err(UNAVAILABLE),
    };
    let metadata = directory.metadata()?;
    if file_id(&metadata) != proof.directory
        || metadata.uid() != manager.uid
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(UNAVAILABLE);
    }
    let has_socket = verify_socket(manager, slot, &directory, proof)?;
    Ok(OwnedPath {
        root,
        name,
        directory: Some(directory),
        has_socket,
    })
}

fn verify_socket(manager: &Manager, slot: Slot, directory: &File, proof: Proof) -> Result<bool> {
    let Some(attributes) = child_attributes(directory, c"s")? else {
        return Ok(false);
    };
    if !matches_socket(&attributes, manager.uid, proof.socket) {
        return Err(UNAVAILABLE);
    }
    let socket = manager.socket(slot.id);
    let matches_name = || -> Result<bool> {
        let path = fs::symlink_metadata(&socket)?;
        Ok(path.file_type().is_socket() && file_id(&path) == proof.socket)
    };
    if !matches_name()? || !tmux::connection_refused(&socket)? || !matches_name()? {
        return Err(CLEANUP);
    }
    if !child_attributes(directory, c"s")?
        .is_some_and(|attributes| matches_socket(&attributes, manager.uid, proof.socket))
    {
        return Err(CLEANUP);
    }
    Ok(true)
}

fn remove_owned_path(manager: &Manager, slot: Slot, proof: Proof) -> Result<()> {
    let path = path_proof(manager, slot, proof)?;
    if let Some(directory) = path.directory {
        if path.has_socket {
            unlink_child(&directory, c"s", 0)?;
        }
        directory.sync_all()?;
        if child_attributes(&path.root, &path.name)?
            .is_none_or(|current| attributes_id(&current) != proof.directory)
        {
            return Err(UNAVAILABLE);
        }
        unlink_child(&path.root, &path.name, libc::AT_REMOVEDIR)?;
    }
    path.root.sync_all()?;
    Ok(())
}

fn open_child(root: &File, name: &CStr) -> io::Result<File> {
    // SAFETY: root remains open and name is a terminated single component.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a newly owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn child_attributes(root: &File, name: &CStr) -> Result<Option<libc::stat>> {
    let mut value = MaybeUninit::uninit();
    // SAFETY: root and name remain live and fstatat writes to the exclusive value.
    let rc = unsafe {
        libc::fstatat(
            root.as_raw_fd(),
            name.as_ptr(),
            value.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if rc == 0 {
        // SAFETY: fstatat succeeded and initialized the entire stat.
        return Ok(Some(unsafe { value.assume_init() }));
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOENT) {
        Ok(None)
    } else {
        Err(error.into())
    }
}

fn unlink_child(root: &File, name: &CStr, flags: libc::c_int) -> Result<()> {
    // SAFETY: root and name remain live. Each caller supplies a verified
    // exact child name and uses only zero or AT_REMOVEDIR as flags.
    if unsafe { libc::unlinkat(root.as_raw_fd(), name.as_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(())
}

fn matches_socket(attributes: &libc::stat, uid: libc::uid_t, id: (u64, u64)) -> bool {
    attributes.st_uid == uid
        && attributes.st_mode & libc::S_IFMT == libc::S_IFSOCK
        && attributes_id(attributes) == id
}

fn file_id(metadata: &Metadata) -> (u64, u64) {
    (metadata.dev(), metadata.ino())
}

#[allow(clippy::unnecessary_cast)] // libc dev_t and ino_t vary by target.
fn attributes_id(attributes: &libc::stat) -> (u64, u64) {
    (attributes.st_dev as u64, attributes.st_ino as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_backend::supervisor::{Account, SlotId};
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::os::unix::net::UnixListener;
    use std::path::Path;

    fn old_slot(index: u8, account: Account) -> Slot {
        Slot {
            id: SlotId([index; 16]),
            account,
            default: index % 2 == 1,
            marker: [index + 1; 16],
            state: State::Prepared,
            proof: None,
        }
    }

    fn sample_host() -> Host {
        Host {
            boot: [1; 16],
            namespace: (1, 2),
        }
    }

    fn add_v1_schema(registry: &Registry) {
        registry.connection.execute_batch(
            "DROP TABLE slot;
             CREATE TABLE slot (
                slot BLOB PRIMARY KEY NOT NULL CHECK(length(slot)=16),
                account BLOB NOT NULL CHECK(length(account)=32),
                is_default INTEGER NOT NULL CHECK(is_default IN (0,1)),
                marker BLOB NOT NULL CHECK(length(marker)=16),
                state INTEGER NOT NULL CHECK(state IN (0,1,2)),
                native_id INTEGER CHECK(native_id BETWEEN 0 AND 4294967295),
                CHECK((state=0 AND native_id IS NULL) OR (state=1 AND native_id IS NOT NULL) OR state=2)
             ) STRICT;
             CREATE UNIQUE INDEX default_by_account ON slot(account) WHERE is_default=1;
             PRAGMA user_version=1;",
        ).unwrap();
    }

    #[test]
    fn core_tests_terminal_live_supervisor_v1_migration_and_presence() {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let open = || {
            Registry::open_at(
                root.path(),
                "00112233445566778899aabbccddeeff",
                [2; 32],
                &[3; 32],
            )
            .unwrap()
        };
        let mut registry = open();
        add_v1_schema(&registry);
        let slot = old_slot(1, [2; 32]);
        registry.prepare(slot).unwrap();
        registry
            .connection
            .execute(
                "UPDATE slot SET state=1,native_id=?1 WHERE slot=?2",
                rusqlite::params![7, slot.id.0],
            )
            .unwrap();
        drop(registry);
        let mut migrated = open();
        let [row] = migrated.slots().unwrap().try_into().ok().unwrap();
        assert!(row.id == slot.id && row.state == State::Ready(7) && row.proof.is_none());
        let current = sample_host();
        let proof = Proof {
            host: current,
            pid: 123,
            directory: (1, 16),
            socket: (1, 17),
        };
        assert!(migrated.pin(row, Proof { pid: 0, ..proof }, 7).is_err());
        assert!(migrated.pin(row, proof, 8).is_err());
        migrated.pin(row, proof, 7).unwrap();
        assert!(migrated.pin(row, Proof { pid: 456, ..proof }, 7).is_err());
        drop(migrated);
        let [row] = open().slots().unwrap().try_into().ok().unwrap();
        assert!(row.proof == Some(proof));
        assert!(absent_errno(Some(libc::ESRCH)));
        assert!(!absent_errno(Some(libc::EPERM)));
        assert!(!absent_errno(None));
        let path = root.path().join("terminal.sqlite3");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o600);
        assert!(!fs::read(path).unwrap().starts_with(b"SQLite format 3\0"));
        assert!(!process_absent(current, proof, |pid| {
            assert_eq!(pid, 123);
            false
        }));
        assert!(process_absent(current, proof, |pid| {
            assert_eq!(pid, 123);
            true
        }));
        assert!(!process_absent(
            Host {
                namespace: (9, 9),
                ..current
            },
            proof,
            |_| panic!("wrong PID namespace probed")
        ));
        assert!(process_absent(
            Host {
                boot: [8; 16],
                ..current
            },
            proof,
            |_| panic!("reused post-reboot PID probed")
        ));
    }

    fn filesystem_row(
        registry: &mut Registry,
        manager: &Manager,
        index: u8,
        host: Host,
        pid: i32,
        bind: bool,
    ) -> (Slot, Option<UnixListener>) {
        let mut slot = old_slot(index, [index; 32]);
        let socket = manager.socket(slot.id);
        let directory = socket.parent().unwrap();
        fs::DirBuilder::new().mode(0o700).create(directory).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut proof = Proof {
            host,
            pid,
            directory: file_id(&fs::metadata(directory).unwrap()),
            socket: file_id(&fs::symlink_metadata(&socket).unwrap()),
        };
        if bind {
            proof.directory.0 ^= 1;
            proof.socket.0 ^= 1;
        }
        registry.prepare(slot).unwrap();
        registry.ready(slot, 0, Some(proof)).unwrap();
        slot.state = State::Ready(0);
        slot.proof = Some(proof);
        (slot, bind.then_some(listener))
    }

    #[tokio::test]
    #[ignore = "uses a trusted local tmux and real private filesystem and SQLCipher state"]
    async fn terminal_live_supervisor_recovery_exact_paths_and_reboot() {
        let fixture = super::super::tests::Fixture::new().await;
        let (lease, identity) = fixture.identity();
        let manager = Manager::new(&fixture.tmux, lease.root()).unwrap();
        let open = || Registry::open(&lease, &identity).unwrap();
        let mut registry = open();
        let host = Host::current().unwrap();
        let child = std::process::Command::new(Path::new("/usr/bin/true"))
            .spawn()
            .unwrap();
        let dead = i32::try_from(child.id()).unwrap();
        let mut owned = super::super::backend::ChildState::new(child);
        tokio::time::timeout(Duration::from_secs(2), async {
            while !owned.waitable().unwrap() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(!probe_process(dead), "owned zombie was treated as absent");
        assert!(owned.child.wait().unwrap().success());
        assert!(probe_process(dead));

        let (same_boot_ambiguous, listener) =
            filesystem_row(&mut registry, &manager, 1, host, dead, true);
        assert!(
            retire_if_absent(
                &mut registry,
                &manager,
                same_boot_ambiguous,
                Instant::now() + EXIT_OBSERVATION
            )
            .is_err()
        );
        let listener = listener.unwrap();
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert!(manager.socket(same_boot_ambiguous.id).exists());

        let mut old_boot = host;
        old_boot.boot[0] ^= 1;
        let our_pid = i32::try_from(std::process::id()).unwrap();
        let (old, unrelated) = filesystem_row(&mut registry, &manager, 2, old_boot, our_pid, true);
        let unrelated = unrelated.unwrap();
        for index in 0..62 {
            fs::create_dir(manager.root.join(format!("survives-{index}"))).unwrap();
        }
        assert!(matches!(
            check_capacity(&manager),
            Err(Error::Storage("terminal_slot_storage_capacity"))
        ));
        retire_if_absent(
            &mut registry,
            &manager,
            old,
            Instant::now() + EXIT_OBSERVATION,
        )
        .unwrap();
        assert!(!registry.slots().unwrap().iter().any(|row| row.id == old.id));
        assert!(manager.socket(old.id).exists());
        assert_eq!(
            unrelated.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        fs::remove_dir(manager.root.join("survives-0")).unwrap();
        assert!(check_capacity(&manager).is_ok());

        let (refused, _) = filesystem_row(&mut registry, &manager, 3, host, dead, false);
        assert!(manager.socket(refused.id).exists());
        retire_if_absent(
            &mut registry,
            &manager,
            refused,
            Instant::now() + EXIT_OBSERVATION,
        )
        .unwrap();
        assert!(!manager.socket(refused.id).parent().unwrap().exists());

        for cut in 0..=2 {
            let (row, _) = filesystem_row(&mut registry, &manager, 4 + cut, host, dead, false);
            registry.retiring(row).unwrap();
            let path = manager.socket(row.id);
            if cut >= 1 {
                fs::remove_file(&path).unwrap();
            }
            if cut >= 2 {
                fs::remove_dir(path.parent().unwrap()).unwrap();
            }
            drop(registry);
            registry = open();
            let record = registry
                .slots()
                .unwrap()
                .into_iter()
                .find(|current| current.id == row.id)
                .unwrap();
            assert!(matches!(record.state, State::Retiring(Some(0))));
            retire_if_absent(
                &mut registry,
                &manager,
                record,
                Instant::now() + EXIT_OBSERVATION,
            )
            .unwrap();
            assert!(!path.parent().unwrap().exists());
            assert!(
                !registry
                    .slots()
                    .unwrap()
                    .iter()
                    .any(|current| current.id == row.id)
            );
        }
    }
}
