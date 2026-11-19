//! Offline SQLCipher allocation for the disposable installed Manual proof.

use super::{
    paths::{CheckedFile, Paths},
    store::consent_checkpoint::{
        Route, Selector,
        provisioning::{
            Binding, prepared_consent::custody::fixture_pending_stage, stage_journal::FileWitness,
        },
    },
};
use crate::{
    identity::{IdentityMaterial, ProfileLease, open_private},
    ipc::peer_read::Entry,
    peer_quota_vfs::dispatch::assert_unmanaged_fixture_default,
};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest as _, Sha256};
use std::{
    fs::File,
    io::{self, Read as _},
    os::{fd::AsRawFd as _, unix::fs::MetadataExt as _},
    path::Path,
    time::Duration,
};
use storage_sqlite::{SqlCipherHardening, SqlCipherKey, open_hardened_sqlcipher};

const MIB: u64 = 1024 * 1024;
const STAGE_CAP: u64 = 16 * MIB;
const TARGET: u64 = 31 * MIB / 2;
const START_CAP: u64 = 4 * MIB;
const CURSOR_CAP: u64 = 24 * MIB;
const FREE_BYTES: u64 = 64 * MIB;

type Result<T> = std::result::Result<T, &'static str>;

#[allow(
    dead_code,
    reason = "used by the independently composed installed controller"
)]
pub(crate) struct NearCapStage {
    binding: Binding,
    witness: FileWitness,
    digest: [u8; 32],
    logical: u64,
    allocated: u64,
    pages: Pages,
}

#[allow(
    dead_code,
    reason = "used by the independently composed installed controller"
)]
impl NearCapStage {
    pub(crate) fn identity(&self) -> (u64, u64) {
        (
            u64::from_be_bytes(self.witness.device),
            u64::from_be_bytes(self.witness.inode),
        )
    }

    pub(crate) fn logical_bytes(&self) -> u64 {
        self.logical
    }

    pub(crate) fn allocated_bytes(&self) -> u64 {
        self.allocated
    }

    pub(crate) fn page_count(&self) -> u64 {
        self.pages.count
    }

    pub(crate) fn page_size(&self) -> u64 {
        self.pages.size
    }

    pub(crate) fn freelist_pages(&self) -> u64 {
        self.pages.free
    }

    pub(crate) fn assert_unchanged(
        &self,
        lease: &ProfileLease,
        material: &IdentityMaterial,
    ) -> Result<()> {
        if !lease.was_ready_at_acquire() {
            return Err("profile");
        }
        let paths = Paths::new(lease, material).map_err(|_| "profile")?;
        let file = paths
            .reopen_provisional(self.binding, self.witness)
            .map_err(|_| "stage_identity")?;
        if file.digest().map_err(|_| "stage_digest")? != self.digest {
            return Err("stage_digest");
        }
        Ok(())
    }
}

#[allow(
    dead_code,
    reason = "used by the independently composed installed controller"
)]
pub(crate) fn grow(
    lease: &ProfileLease,
    material: &IdentityMaterial,
    relay: &str,
    entry: &Entry,
) -> Result<NearCapStage> {
    assert_unmanaged_fixture_default()?;
    if !lease.was_ready_at_acquire() {
        return Err("profile");
    }
    let paths = Paths::new(lease, material).map_err(|_| "profile")?;
    let route = Route::configured(relay).map_err(|_| "origin")?;
    let cursor = paths.cursor().map_err(|_| "cursor_identity")?;
    let before = cursor_digest(&cursor)?;
    let result = read_pending(&paths, &cursor, material, &route, entry)
        .and_then(|(binding, witness)| grow_selected(&paths, binding, witness, STAGE_CAP));
    if cursor_digest(&cursor)? != before {
        return Err("cursor_digest");
    }
    result
}

fn read_pending(
    paths: &Paths<'_>,
    cursor: &CheckedFile,
    material: &IdentityMaterial,
    route: &Route,
    entry: &Entry,
) -> Result<(Binding, FileWitness)> {
    let mut conn = keyed(
        cursor.path(),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
        &SqlCipherKey::raw(*paths.cursor_key()),
    )?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|_| "cursor_open")?;
    conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
        .map_err(|_| "cursor_open")?;
    let tx = conn.transaction().map_err(|_| "cursor_open")?;
    let selection = entry.selection;
    let selected = fixture_pending_stage(
        &tx,
        material.signer().public_key(),
        route.id,
        (
            Selector {
                server: selection.server,
                id: selection.offer_id,
                revision: selection.revision,
            },
            selection.generation,
            entry.held_at,
            entry.expires_at,
        ),
    )
    .map_err(|_| "pending_selection");
    tx.rollback().map_err(|_| "cursor_close")?;
    conn.close().map_err(|_| "cursor_close")?;
    selected
}

fn cursor_digest(cursor: &CheckedFile) -> Result<[u8; 32]> {
    let file = open_private(cursor.path(), false).map_err(|_| "cursor_identity")?;
    cursor.matches_file(&file).map_err(|_| "cursor_identity")?;
    let mut digest = Sha256::new();
    let size =
        io::copy(&mut file.take(CURSOR_CAP + 1), &mut digest).map_err(|_| "cursor_digest")?;
    cursor.unchanged().map_err(|_| "cursor_identity")?;
    if size == 0 || size > CURSOR_CAP {
        return Err("cursor_digest");
    }
    Ok(digest.finalize().into())
}

fn keyed(path: &Path, access: OpenFlags, key: &SqlCipherKey) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        access | OpenFlags::SQLITE_OPEN_NOFOLLOW | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "sqlcipher_open")?;
    open_hardened_sqlcipher(&conn, key, SqlCipherHardening::live_cache())
        .map_err(|_| "sqlcipher_open")?;
    Ok(conn)
}

fn grow_selected(
    paths: &Paths<'_>,
    binding: Binding,
    witness: FileWitness,
    limit: u64,
) -> Result<NearCapStage> {
    let file = paths
        .reopen_provisional(binding, witness)
        .map_err(|_| "stage_identity")?;
    let fd = file.read_file().map_err(|_| "stage_identity")?;
    let len = fd.metadata().map_err(|_| "stage_extent")?.len();
    if len == 0 || len >= START_CAP {
        return Err("stage_start_extent");
    }
    require_free_space(&fd)?;
    let mut conn = keyed(
        file.path(),
        OpenFlags::SQLITE_OPEN_READ_WRITE,
        &SqlCipherKey::raw(*paths.provisional_key(binding)),
    )?;
    conn.busy_timeout(Duration::ZERO)
        .map_err(|_| "stage_open")?;
    let journal: String = conn
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .map_err(|_| "stage_open")?;
    if journal != "delete"
        || pragma(&conn, "synchronous")? != 2
        || pragma(&conn, "secure_delete")? != 1
        || pragma(&conn, "auto_vacuum")? != 2
    {
        return Err("stage_hardening");
    }
    file.ensure_unchanged().map_err(|_| "stage_identity")?;
    let pages = enlarge(&mut conn, len, limit)?;
    conn.close().map_err(|_| "sqlcipher_close")?;
    fd.sync_all().map_err(|_| "stage_sync")?;
    let closed = paths
        .reopen_provisional(binding, witness)
        .map_err(|_| "stage_identity")?;
    let closed_fd = closed.read_file().map_err(|_| "stage_identity")?;
    let meta = closed_fd.metadata().map_err(|_| "stage_extent")?;
    let logical = meta.len();
    let allocated = meta.blocks().checked_mul(512).ok_or("stage_extent")?;
    if pages.bytes() != Some(logical)
        || !(15 * MIB..STAGE_CAP).contains(&logical)
        || allocated < logical
    {
        return Err("stage_extent");
    }
    Ok(NearCapStage {
        binding,
        witness,
        digest: closed.digest().map_err(|_| "stage_digest")?,
        logical,
        allocated,
        pages,
    })
}

fn require_free_space(file: &File) -> Result<()> {
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // libc initializes the output struct on success and only borrows this open descriptor.
    if unsafe { libc::fstatvfs(file.as_raw_fd(), stats.as_mut_ptr()) } != 0 {
        return Err("stage_capacity");
    }
    let stats = unsafe { stats.assume_init() };
    if stats.f_bavail.saturating_mul(stats.f_frsize) < FREE_BYTES {
        return Err("stage_capacity");
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Pages {
    size: u64,
    count: u64,
    free: u64,
}

impl Pages {
    fn read(conn: &Connection) -> Result<Self> {
        Ok(Self {
            // SQLCipher intercepts the statement as its cipher setting. The
            // table-valued form reads the actual pager, as MDK compaction does.
            size: conn
                .query_row(
                    "SELECT CAST(page_size AS INTEGER) FROM pragma_page_size()",
                    [],
                    |row| row.get(0),
                )
                .map_err(|_| "page_size")?,
            count: pragma(conn, "page_count")?,
            free: pragma(conn, "freelist_count")?,
        })
    }

    fn bytes(self) -> Option<u64> {
        self.size.checked_mul(self.count)
    }
}

fn integer_pragma(conn: &Connection, name: &'static str) -> Result<i64> {
    conn.pragma_query_value(None, name, |row| row.get(0))
        .map_err(|_| name)
}

fn pragma(conn: &Connection, name: &'static str) -> Result<u64> {
    u64::try_from(integer_pragma(conn, name)?).map_err(|_| name)
}

type SchemaRow = (String, String, String, i64, Option<String>);

#[derive(PartialEq, Eq)]
struct Schema {
    user_version: i64,
    application_id: i64,
    rows: Vec<SchemaRow>,
}

impl Schema {
    fn read(conn: &Connection) -> Result<Self> {
        let mut stmt = conn
            .prepare("SELECT type,name,tbl_name,rootpage,sql FROM sqlite_schema ORDER BY type,name")
            .map_err(|_| "stage_schema")?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(|_| "stage_schema")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| "stage_schema")?;
        Ok(Self {
            user_version: integer_pragma(conn, "user_version")?,
            application_id: integer_pragma(conn, "application_id")?,
            rows,
        })
    }
}

fn integrity(conn: &Connection) -> Result<()> {
    let mut stmt = conn
        .prepare("PRAGMA cipher_integrity_check")
        .map_err(|_| "stage_integrity")?;
    if stmt.column_count() != 1
        || stmt.column_name(0).ok() != Some("cipher_integrity_check")
        || stmt
            .query([])
            .and_then(|mut rows| rows.next().map(|row| row.is_none()))
            != Ok(true)
    {
        return Err("stage_integrity");
    }
    let mut stmt = conn
        .prepare("PRAGMA quick_check")
        .map_err(|_| "stage_integrity")?;
    let result: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|_| "stage_integrity")?
        .collect::<rusqlite::Result<_>>()
        .map_err(|_| "stage_integrity")?;
    if result != ["ok"] {
        return Err("stage_integrity");
    }
    Ok(())
}

fn enlarge(conn: &mut Connection, len: u64, limit: u64) -> Result<Pages> {
    let before = Pages::read(conn)?;
    if !before.size.is_power_of_two() || before.bytes() != Some(len) || before.free > 64 {
        return Err("stage_start_extent");
    }
    integrity(conn)?;
    let schema = Schema::read(conn)?;
    let maximum_pages = (limit - 1) / before.size;
    conn.pragma_update(None, "cache_spill", 0)
        .map_err(|_| "stage_page_cap")?;
    conn.pragma_update(
        None,
        "max_page_count",
        i64::try_from(maximum_pages).map_err(|_| "stage_page_cap")?,
    )
    .map_err(|_| "stage_page_cap")?;
    if pragma(conn, "max_page_count")? != maximum_pages || pragma(conn, "cache_spill")? != 0 {
        return Err("stage_page_cap");
    }
    let grown = allocate(conn, TARGET.div_ceil(before.size))?;
    let tx = conn.transaction().map_err(|_| "stage_drop")?;
    tx.execute_batch("DROP TABLE marmish_fixture_padding")
        .map_err(|_| "stage_drop")?;
    tx.commit().map_err(|_| "stage_drop")?;
    let after = Pages::read(conn)?;
    if after.count != grown
        || after.size != before.size
        || after.free < before.free + (grown - before.count) * 9 / 10
        || Schema::read(conn)? != schema
    {
        return Err("stage_schema");
    }
    integrity(conn)?;
    Ok(after)
}

fn allocation_error(error: rusqlite::Error) -> &'static str {
    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull) {
        "stage_page_full"
    } else {
        "stage_allocate"
    }
}

fn allocate(conn: &mut Connection, target: u64) -> Result<u64> {
    let tx = conn.transaction().map_err(allocation_error)?;
    tx.execute_batch("CREATE TABLE marmish_fixture_padding(bytes BLOB NOT NULL) STRICT")
        .map_err(allocation_error)?;
    for _ in 0..256 {
        tx.execute(
            "INSERT INTO marmish_fixture_padding VALUES(randomblob(65536))",
            [],
        )
        .map_err(allocation_error)?;
        let count = pragma(&tx, "page_count")?;
        if count >= target {
            tx.commit().map_err(|_| "stage_allocate_commit")?;
            return Ok(count);
        }
    }
    Err("stage_allocate")
}

pub(super) fn verify_joined_fixture(paths: &Paths<'_>, (binding, witness): (Binding, FileWitness)) {
    use std::fs::{self, OpenOptions, Permissions};
    use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _, symlink};

    assert_unmanaged_fixture_default().unwrap();
    let original = paths.reopen_provisional(binding, witness).unwrap();
    let start_digest = original.digest().unwrap();
    let before = original.read_file().unwrap().metadata().unwrap();
    assert_eq!(
        grow_selected(
            paths,
            binding,
            FileWitness {
                inode: [0; 8],
                ..witness
            },
            STAGE_CAP
        )
        .err(),
        Some("stage_identity")
    );

    // Exercise the actual stage profile and same VFS using the wrong cipher key.
    assert!(
        keyed(
            original.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
            &SqlCipherKey::raw([0; 32])
        )
        .and_then(|conn| integrity(&conn))
        .is_err()
    );
    assert_eq!(
        grow_selected(paths, binding, witness, before.len() + 4097).err(),
        Some("stage_page_full")
    );
    assert_eq!(original.digest().unwrap(), start_digest);

    let sidecar = original.path().with_extension("db-journal");
    fs::write(&sidecar, []).unwrap();
    assert_eq!(
        grow_selected(paths, binding, witness, STAGE_CAP).err(),
        Some("stage_identity")
    );
    assert!(sidecar.exists());
    fs::remove_file(sidecar).unwrap();

    let other = original.path().with_extension("db-extra");
    fs::hard_link(original.path(), &other).unwrap();
    assert_eq!(
        grow_selected(paths, binding, witness, STAGE_CAP).err(),
        Some("stage_identity")
    );
    fs::remove_file(&other).unwrap();
    fs::set_permissions(original.path(), Permissions::from_mode(0o640)).unwrap();
    assert_eq!(
        grow_selected(paths, binding, witness, STAGE_CAP).err(),
        Some("stage_identity")
    );
    fs::set_permissions(original.path(), Permissions::from_mode(0o600)).unwrap();
    fs::rename(original.path(), &other).unwrap();
    symlink(&other, original.path()).unwrap();
    assert_eq!(
        grow_selected(paths, binding, witness, STAGE_CAP).err(),
        Some("stage_identity")
    );
    fs::remove_file(original.path()).unwrap();
    fs::rename(&other, original.path()).unwrap();

    // Exercise the size refusal without padding or truncating the genuine fixture.
    fs::rename(original.path(), &other).unwrap();
    let cap_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(original.path())
        .unwrap();
    cap_file.set_len(STAGE_CAP).unwrap();
    let cap_meta = cap_file.metadata().unwrap();
    let cap_witness = FileWitness {
        device: cap_meta.dev().to_be_bytes(),
        inode: cap_meta.ino().to_be_bytes(),
    };
    assert_eq!(
        grow_selected(paths, binding, cap_witness, STAGE_CAP).err(),
        Some("stage_start_extent")
    );
    assert_eq!(cap_file.metadata().unwrap().len(), STAGE_CAP);
    drop(cap_file);
    fs::remove_file(original.path()).unwrap();
    fs::rename(other, original.path()).unwrap();
    assert_eq!(original.digest().unwrap(), start_digest);

    let grown = grow_selected(paths, binding, witness, STAGE_CAP).unwrap();
    assert_eq!(grown.identity(), (before.dev(), before.ino()));
    assert!(grown.allocated_bytes() >= grown.logical_bytes());
    assert!(grown.logical_bytes() >= TARGET);
    assert!(grown.logical_bytes() < STAGE_CAP);
    assert_eq!(
        grown.logical_bytes(),
        grown.page_count() * grown.page_size()
    );
    assert!(grown.freelist_pages() > 64);
    assert_ne!(grown.digest, start_digest);
    assert_eq!(
        grow_selected(paths, binding, witness, STAGE_CAP).err(),
        Some("stage_start_extent")
    );
    assert_eq!(original.digest().unwrap(), grown.digest);
    println!(
        "near_cap logical_bytes={} allocated_bytes={} page_size={} page_count={} freelist_pages={}",
        grown.logical_bytes(),
        grown.allocated_bytes(),
        grown.page_size(),
        grown.page_count(),
        grown.freelist_pages(),
    );
}

pub(super) fn verify_bound_refusal(lease: ProfileLease) {
    let lease = std::sync::Arc::new(lease);
    let _quota = crate::peer_quota_vfs::dispatch::ReceiverQuota::bind_receiver(&lease).unwrap();
    assert_eq!(assert_unmanaged_fixture_default(), Err("stock_unix_vfs"));
}
