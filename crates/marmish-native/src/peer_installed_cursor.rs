//! Stock SQLCipher bounds for the private installed recipient cursor proof.

use super::{Connection, Error, HardenedPeerConnection, Path, Result, check_hardening, open_keyed};
use std::{fs, io};

const PAGE_SIZE: u64 = 4_096;
const MAX_PAGES: u64 = 6_144;
const MAX_MAIN_BYTES: u64 = MAX_PAGES * PAGE_SIZE;
const MAX_JOURNAL_BYTES: u64 = MAX_PAGES * (PAGE_SIZE + 8) + 2 * 65_536;
pub(crate) const RESERVED_BYTES: u64 = 49 * 1_048_576;

const _: () = assert!(MAX_MAIN_BYTES + MAX_JOURNAL_BYTES <= RESERVED_BYTES);

impl HardenedPeerConnection {
    /// The owner must hold the sole profile lease and recheck its checked cursor
    /// inode and scope around this open. This proof has no daemon caller yet.
    pub(crate) fn open_installed(path: &Path, key: [u8; 32]) -> Result<Self> {
        check_closed_image(path)?;
        let conn = open_keyed(path, key)?;
        // Verify DELETE mode rather than attempting to transition a foreign image.
        check_hardening(&conn)?;
        conn.pragma_update(None, "cache_spill", "OFF")?;
        conn.pragma_update(None, "max_page_count", MAX_PAGES as i64)?;
        let cipher_page_size: String =
            conn.pragma_query_value(None, "cipher_page_size", |row| row.get(0))?;
        if cipher_page_size.parse::<u64>().ok() != Some(PAGE_SIZE)
            || pragma(&conn, "page_count")? > MAX_PAGES
            || pragma(&conn, "max_page_count")? != MAX_PAGES
            || pragma(&conn, "cache_spill")? != 0
        {
            return Err(Error::Storage);
        }
        check_closed_image(path)?;
        Ok(Self(conn))
    }
}

fn pragma(conn: &Connection, name: &str) -> Result<u64> {
    let value = conn.pragma_query_value(None, name, |row| row.get::<_, i64>(0))?;
    u64::try_from(value).map_err(|_| Error::Storage)
}

fn check_closed_image(path: &Path) -> Result<()> {
    let main = fs::symlink_metadata(path).map_err(|_| Error::Storage)?;
    if !main.is_file() || main.len() > MAX_MAIN_BYTES || !main.len().is_multiple_of(PAGE_SIZE) {
        return Err(Error::Storage);
    }
    for suffix in ["-journal", "-wal", "-shm"] {
        // Recovery may use an untrusted old page count from a short journal
        // before SQLite applies the connection's max_page_count.
        match fs::symlink_metadata(sidecar(path, suffix)) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => (),
            _ => return Err(Error::Storage),
        }
    }
    Ok(())
}

fn sidecar(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut full = path.as_os_str().to_owned();
    full.push(suffix);
    full.into()
}

#[path = "peer_installed_cursor_tests.rs"]
mod tests;
