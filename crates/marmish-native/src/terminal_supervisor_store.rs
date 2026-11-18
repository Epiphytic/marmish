//! Dedicated terminal management journal. It has no wire or PTY data.
use super::{Account, CAPACITY, MAX_SLOTS, SlotId, UNAVAILABLE};
use crate::identity::{self, IdentityMaterial, ProfileLease};
use crate::{Error, Result};
use hkdf::Hkdf;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use sha2::Sha256;
use std::fs::{self, File};
use std::path::Path;
use std::time::Duration;
use zeroize::Zeroizing;

const SCHEMA: i64 = 2;
const STORE: Error = Error::Storage("terminal_registry");
const FORMAT: Error = Error::Storage("terminal_registry_format");
const BINDING: Error = Error::Storage("terminal_registry_binding");
const GENERATIONS: Error = Error::Storage("terminal_registry_generation");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    Prepared,
    Ready(u32),
    Retiring(Option<u32>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Host {
    pub boot: [u8; 16],
    pub namespace: (u64, u64),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Proof {
    pub host: Host,
    pub pid: i32,
    pub directory: (u64, u64),
    pub socket: (u64, u64),
}

impl Proof {
    const BYTES: usize = 68;

    fn encode(self) -> Result<[u8; Self::BYTES]> {
        if self.pid <= 0 || self.directory.1 == 0 || self.socket.1 == 0 {
            return Err(FORMAT);
        }
        let mut value = [0; Self::BYTES];
        value[..4].copy_from_slice(&self.pid.to_be_bytes());
        value[4..20].copy_from_slice(&self.host.boot);
        for (bytes, field) in value[20..].chunks_exact_mut(8).zip([
            self.host.namespace.0,
            self.host.namespace.1,
            self.directory.0,
            self.directory.1,
            self.socket.0,
            self.socket.1,
        ]) {
            bytes.copy_from_slice(&field.to_be_bytes());
        }
        Ok(value)
    }

    fn decode(value: [u8; Self::BYTES]) -> Result<Self> {
        let pid = i32::from_be_bytes(value[..4].try_into().map_err(|_| FORMAT)?);
        let boot = value[4..20].try_into().map_err(|_| FORMAT)?;
        let (fields, _) = value[20..].as_chunks::<8>();
        let [ns_dev, ns_ino, dir_dev, dir_ino, sock_dev, sock_ino] = fields else {
            return Err(FORMAT);
        };
        let proof = Self {
            host: Host {
                boot,
                namespace: (u64::from_be_bytes(*ns_dev), u64::from_be_bytes(*ns_ino)),
            },
            pid,
            directory: (u64::from_be_bytes(*dir_dev), u64::from_be_bytes(*dir_ino)),
            socket: (u64::from_be_bytes(*sock_dev), u64::from_be_bytes(*sock_ino)),
        };
        proof.encode()?;
        Ok(proof)
    }
}

#[derive(Clone, Copy)]
pub(super) struct Slot {
    pub id: SlotId,
    pub account: Account,
    pub default: bool,
    pub marker: [u8; 16],
    pub state: State,
    pub proof: Option<Proof>,
}

pub(crate) struct Registry {
    pub(super) connection: Connection,
}

impl Registry {
    pub fn open(profile: &ProfileLease, material: &IdentityMaterial) -> Result<Self> {
        let account = material.public_key();
        if !profile.setup_snapshot().ready || profile.public_key() != Some(account) {
            return Err(BINDING);
        }
        Self::open_at(
            profile.root(),
            profile.profile_id(),
            account.to_bytes(),
            material.sqlcipher_root(),
        )
    }

    pub(super) fn open_at(
        root: &Path,
        profile_id: &str,
        account: Account,
        root_key: &[u8; 32],
    ) -> Result<Self> {
        identity::check_dir(root)?;
        let path = root.join("terminal.sqlite3");
        let rollback = root.join("terminal.sqlite3-journal");
        let key = derive_key(profile_id, root_key)?;
        match fs::symlink_metadata(&rollback) {
            Ok(_) => drop(identity::open_private(&rollback, false)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let file = identity::open_private(&path, true)?;
        file.sync_all()?;
        File::open(root)?.sync_all()?;
        marmish_engine::initialize_storage().map_err(|_| STORE)?;
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(db)?;
        connection
            .busy_timeout(Duration::from_millis(250))
            .map_err(db)?;
        set_key(&connection, &key)?;
        let table_count: i64 = connection
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
            .map_err(db)?;
        let schema: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(db)?;
        if !matches!((schema, table_count), (0, 0) | (1 | SCHEMA, _)) {
            return Err(FORMAT);
        }
        connection
            .execute_batch(
                "PRAGMA temp_store=MEMORY; PRAGMA secure_delete=ON; PRAGMA synchronous=FULL;",
            )
            .map_err(db)?;
        let mode: String = connection
            .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .map_err(db)?;
        if mode != "delete" {
            return Err(FORMAT);
        }
        if schema == 0 {
            migrate(&mut connection, profile_id, account)?;
        }
        let binding: (String, Account) = connection
            .query_row(
                "SELECT profile, account FROM binding WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(db)?;
        if binding.0 != profile_id || binding.1 != account {
            return Err(BINDING);
        }
        if schema == 1 {
            migrate_v1(&mut connection)?;
        }
        Ok(Self { connection })
    }

    pub(super) fn slots(&self) -> Result<Vec<Slot>> {
        let mut statement = self
            .connection
            .prepare("SELECT slot, account, is_default, marker, state, native_id, native_proof FROM slot ORDER BY slot LIMIT ?1")
            .map_err(db)?;
        let mut rows = statement.query([(MAX_SLOTS + 1) as i64]).map_err(db)?;
        let mut slots = Vec::new();
        while let Some(row) = rows.next().map_err(db)? {
            let native = row.get(5).map_err(db)?;
            let state = match (row.get::<_, i64>(4).map_err(db)?, native) {
                (0, None) => State::Prepared,
                (1, Some(id)) => State::Ready(id),
                (2, id) => State::Retiring(id),
                _ => return Err(FORMAT),
            };
            slots.push(Slot {
                id: SlotId(row.get(0).map_err(db)?),
                account: row.get(1).map_err(db)?,
                default: row.get(2).map_err(db)?,
                marker: row.get(3).map_err(db)?,
                state,
                proof: row
                    .get::<_, Option<[u8; Proof::BYTES]>>(6)
                    .map_err(db)?
                    .map(Proof::decode)
                    .transpose()?,
            });
        }
        if slots.len() > MAX_SLOTS {
            return Err(FORMAT);
        }
        Ok(slots)
    }

    pub(super) fn prepare(&mut self, slot: Slot) -> Result<()> {
        let tx = self.connection.transaction().map_err(db)?;
        let count: i64 = tx
            .query_row("SELECT count(*) FROM slot", [], |row| row.get(0))
            .map_err(db)?;
        if count >= MAX_SLOTS as i64 {
            return Err(CAPACITY);
        }
        tx.execute(
            "INSERT INTO slot(slot, account, is_default, marker, state) VALUES (?1, ?2, ?3, ?4, 0)",
            params![slot.id.0, slot.account, slot.default, slot.marker],
        )
        .map_err(db)?;
        tx.commit().map_err(db)
    }

    pub(super) fn ready(&mut self, slot: Slot, native_id: u32, proof: Option<Proof>) -> Result<()> {
        let proof = proof.map(Proof::encode).transpose()?;
        changed(self.connection.execute(
            "UPDATE slot SET state=1,native_id=?2,native_proof=?3 WHERE slot=?1 AND state=0 AND account=?4 AND marker=?5",
            params![slot.id.0, native_id, proof.as_ref().map(|value| value.as_slice()), slot.account, slot.marker],
        ))
    }

    pub(super) fn pin(&mut self, slot: Slot, proof: Proof, native_id: u32) -> Result<()> {
        let proof = proof.encode()?;
        changed(self.connection.execute(
            "UPDATE slot SET native_proof=?2 WHERE slot=?1 AND native_id=?3 AND native_proof IS NULL AND account=?4 AND marker=?5 AND state IN (1,2)",
            params![slot.id.0, proof.as_slice(), native_id, slot.account, slot.marker],
        ))
    }

    pub(super) fn retiring(&mut self, slot: Slot) -> Result<()> {
        changed(self.connection.execute(
            "UPDATE slot SET state=2 WHERE slot=?1 AND account=?2 AND marker=?3",
            params![slot.id.0, slot.account, slot.marker],
        ))
    }

    pub(super) fn retired(&mut self, slot: Slot) -> Result<()> {
        changed(self.connection.execute(
            "DELETE FROM slot WHERE slot=?1 AND state=2 AND account=?2 AND marker=?3",
            params![slot.id.0, slot.account, slot.marker],
        ))
    }

    pub(super) fn retire_old_boot(&mut self, slot: Slot, proof: Proof) -> Result<()> {
        let proof = proof.encode()?;
        changed(self.connection.execute(
            "DELETE FROM slot WHERE slot=?1 AND account=?2 AND marker=?3 AND native_proof=?4 AND state IN (1,2)",
            params![slot.id.0, slot.account, slot.marker, proof.as_slice()],
        ))
    }

    pub(super) fn retire_unstarted(&mut self, slot: Slot) -> Result<()> {
        changed(self.connection.execute(
            "DELETE FROM slot WHERE slot=?1 AND account=?2 AND marker=?3 AND state IN (0,2) AND native_id IS NULL AND native_proof IS NULL",
            params![slot.id.0, slot.account, slot.marker],
        ))
    }

    pub(super) fn allocate(&mut self) -> Result<u64> {
        let tx = self.connection.transaction().map_err(db)?;
        let previous: [u8; 8] = tx
            .query_row("SELECT generation FROM binding WHERE id=1", [], |row| {
                row.get(0)
            })
            .map_err(db)?;
        let next = u64::from_be_bytes(previous)
            .checked_add(1)
            .ok_or(GENERATIONS)?;
        changed(tx.execute(
            "UPDATE binding SET generation=?1 WHERE id=1",
            params![next.to_be_bytes()],
        ))?;
        tx.commit().map_err(db)?;
        Ok(next)
    }
}

fn changed(value: rusqlite::Result<usize>) -> Result<()> {
    if value.map_err(db)? != 1 {
        return Err(UNAVAILABLE);
    }
    Ok(())
}

fn migrate(connection: &mut Connection, profile_id: &str, account: Account) -> Result<()> {
    let tx = connection.transaction().map_err(db)?;
    tx.execute_batch(
        "CREATE TABLE binding (
            id INTEGER PRIMARY KEY CHECK(id=1), profile TEXT NOT NULL CHECK(length(profile)=32),
            account BLOB NOT NULL CHECK(length(account)=32),
            generation BLOB NOT NULL CHECK(length(generation)=8)
        ) STRICT;",
    )
    .map_err(db)?;
    tx.execute_batch(SLOT_SCHEMA).map_err(db)?;
    tx.execute(
        "INSERT INTO binding VALUES (1,?1,?2,?3)",
        params![profile_id, account, [0_u8; 8]],
    )
    .map_err(db)?;
    tx.pragma_update(None, "user_version", SCHEMA).map_err(db)?;
    tx.commit().map_err(db)
}

fn migrate_v1(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction().map_err(db)?;
    tx.execute_batch("DROP INDEX default_by_account; ALTER TABLE slot RENAME TO slot_v1;")
        .map_err(db)?;
    tx.execute_batch(SLOT_SCHEMA).map_err(db)?;
    tx.execute_batch(
        "INSERT INTO slot (slot, account, is_default, marker, state, native_id)
            SELECT slot, account, is_default, marker, state, native_id FROM slot_v1;
        DROP TABLE slot_v1;",
    )
    .map_err(db)?;
    tx.pragma_update(None, "user_version", SCHEMA).map_err(db)?;
    tx.commit().map_err(db)
}

const SLOT_SCHEMA: &str = "CREATE TABLE slot (
            slot BLOB PRIMARY KEY NOT NULL CHECK(length(slot)=16),
            account BLOB NOT NULL CHECK(length(account)=32),
            is_default INTEGER NOT NULL CHECK(is_default IN (0,1)),
            marker BLOB NOT NULL CHECK(length(marker)=16),
            state INTEGER NOT NULL CHECK(state IN (0,1,2)),
            native_id INTEGER CHECK(native_id BETWEEN 0 AND 4294967295),
            native_proof BLOB CHECK(length(native_proof)=68),
            CHECK((state=0 AND native_id IS NULL AND native_proof IS NULL)
                OR (state=1 AND native_id IS NOT NULL)
                OR (state=2 AND (native_proof IS NULL OR native_id IS NOT NULL)))
        ) STRICT;
        CREATE UNIQUE INDEX default_by_account ON slot(account) WHERE is_default=1;";

pub(super) fn derive_key(profile_id: &str, root: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>> {
    if !crate::foundation::valid_hex(profile_id, 32) {
        return Err(BINDING);
    }
    let hkdf = Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), root);
    let mut key = Zeroizing::new([0; 32]);
    hkdf.expand_multi_info(
        &[b"terminal-supervisor-v1\0", profile_id.as_bytes()],
        key.as_mut(),
    )
    .map_err(|_| STORE)?;
    Ok(key)
}

fn set_key(connection: &Connection, key: &[u8; 32]) -> Result<()> {
    let cipher: Option<String> = connection
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .optional()
        .map_err(db)?;
    if cipher.is_none_or(|version| version.is_empty()) {
        return Err(STORE);
    }
    let mut raw = Zeroizing::new([0_u8; 67]);
    raw[..2].copy_from_slice(b"x'");
    hex::encode_to_slice(key, &mut raw[2..66]).map_err(|_| STORE)?;
    raw[66] = b'\'';
    // SAFETY: this is an exclusively owned connection and raw holds exactly a
    // valid repr for a binary SQLCipher key for the duration of the call.
    if unsafe { rusqlite::ffi::sqlite3_key(connection.handle(), raw.as_ptr().cast(), 67) }
        != rusqlite::ffi::SQLITE_OK
    {
        return Err(STORE);
    }
    connection
        .execute_batch("PRAGMA cipher_compatibility=4; PRAGMA cipher_memory_security=ON; PRAGMA cipher_page_size=4096;")
        .map_err(db)
}

fn db(_: rusqlite::Error) -> Error {
    STORE
}
