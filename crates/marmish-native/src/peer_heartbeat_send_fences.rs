//! Standalone SQLCipher proof for Peer sender reservations. The fixed authority
//! projection is a fixture until the accepted Peer owner can replace it.
use crate::peer_status_wire::{Generation, Hex32};
use nostr::key::PublicKey;
use rusqlite::{
    Connection, ErrorCode, OptionalExtension, Row, Transaction, TransactionBehavior, params,
};
use std::collections::BTreeMap;
use std::num::NonZeroU64;

const MAX_FENCES: i64 = 64;
const SCHEMA_PREFIX: &str = "peer_heartbeat_send_fence";
const VERSION_TABLE: &str = "peer_heartbeat_send_fence_version";
const ROW_TABLE: &str = "peer_heartbeat_send_fence_rows";
const DIRECTION_INDEX: &str = "peer_heartbeat_send_fence_direction";
const VERSION_SQL: &str = "CREATE TABLE peer_heartbeat_send_fence_version (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    version INTEGER NOT NULL CHECK (version = 1),
    profile TEXT NOT NULL CHECK (length(profile) BETWEEN 1 AND 64),
    account BLOB NOT NULL CHECK (length(account) = 32),
    wall BLOB CHECK (length(wall) = 8 AND wall <> X'0000000000000000')
) STRICT";
const ROW_SQL: &str = "CREATE TABLE peer_heartbeat_send_fence_rows (
    generation BLOB PRIMARY KEY CHECK (length(generation) = 8 AND generation <> X'0000000000000000'),
    server BLOB NOT NULL CHECK (length(server) = 32),
    client BLOB NOT NULL CHECK (length(client) = 32 AND client <> server),
    native_group BLOB NOT NULL CHECK (length(native_group) BETWEEN 1 AND 256),
    app_room BLOB NOT NULL CHECK (length(app_room) = 32),
    revision BLOB NOT NULL CHECK (length(revision) = 8 AND revision <> X'0000000000000000'),
    state INTEGER NOT NULL CHECK (state BETWEEN 0 AND 2),
    digest BLOB CHECK (digest IS NULL OR
                       (length(digest) = 32 AND state = 1 AND ordinal IS NOT NULL)),
    sequence BLOB CHECK (length(sequence) = 8 AND sequence <> X'0000000000000000'),
    heartbeat_wall BLOB CHECK (length(heartbeat_wall) = 8 AND heartbeat_wall <> X'0000000000000000'),
    ordinal BLOB CHECK (length(ordinal) = 8 AND ordinal <> X'0000000000000000'),
    issue_at BLOB CHECK (issue_at IS NULL OR
                         (length(issue_at) = 8 AND issue_at <> X'0000000000000000'
                          AND digest IS NOT NULL)),
    CHECK ((sequence IS NULL) = (heartbeat_wall IS NULL))
) STRICT, WITHOUT ROWID";
const DIRECTION_SQL: &str = "CREATE UNIQUE INDEX peer_heartbeat_send_fence_direction
    ON peer_heartbeat_send_fence_rows (server, client) WHERE state <> 2";
const MAIN_SCHEMA: &str = "SELECT type, name, tbl_name,
                                  CASE WHEN length(CAST(sql AS BLOB)) <= 4096 THEN sql END
    FROM main.sqlite_schema
    WHERE substr(lower(name), 1, length(?1)) = ?1
       OR substr(lower(tbl_name), 1, length(?1)) = ?1
    ORDER BY name LIMIT 4";
const TEMP_SCHEMA: &str = "SELECT 1 FROM temp.sqlite_schema
    WHERE substr(lower(name), 1, length(?1)) = ?1
       OR substr(lower(tbl_name), 1, length(?1)) = ?1 LIMIT 1";
const AUTHORITY_TABLE: &str = "peer_heartbeat_test_authority";
const AUTHORITY_SQL: &str = "SELECT profile, server, client, native_group, app_room, generation,
                                   revision, role, state, digest
    FROM main.peer_heartbeat_test_authority WHERE generation = ?1 LIMIT 2";
const ALL_FENCES_SQL: &str = "SELECT server, client, native_group, app_room, generation, revision,
                                   state, digest, sequence, heartbeat_wall, ordinal, issue_at
    FROM main.peer_heartbeat_send_fence_rows LIMIT 65";
const ONE_FENCE_SQL: &str = "SELECT server, client, native_group, app_room, generation, revision,
                                  state, digest, sequence, heartbeat_wall, ordinal, issue_at
    FROM main.peer_heartbeat_send_fence_rows WHERE generation = ?1";
const WRITE_FENCE_SQL: &str = "INSERT INTO main.peer_heartbeat_send_fence_rows
    (generation, server, client, native_group, app_room, revision, state, digest, sequence,
     heartbeat_wall, ordinal, issue_at)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
    ON CONFLICT(generation) DO UPDATE SET revision=excluded.revision, state=excluded.state,
        digest=excluded.digest, sequence=excluded.sequence, heartbeat_wall=excluded.heartbeat_wall,
        ordinal=excluded.ordinal, issue_at=excluded.issue_at";

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Binding,
    Busy,
    Capacity,
    Clock,
    CommitUnknown,
    Conflict,
    Disabled,
    Exhausted,
    Hardening,
    Poisoned,
    ReconciliationRequired,
    Retired,
    Schema,
    Stale,
    Storage,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ProfileName(String);

impl ProfileName {
    pub(crate) fn new(value: &str) -> Result<Self> {
        // Same verified name grammar as identity::validate_name. The standalone
        // proof does not mount credential providers or profile filesystem code.
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(Error::Binding);
        }
        Ok(Self(value.to_owned()))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Namespace {
    pub(crate) profile: ProfileName,
    pub(crate) account: PublicKey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SendScope {
    namespace: Namespace,
    client: PublicKey,
    native_group: Vec<u8>,
    app_room: Hex32,
    generation: NonZeroU64,
}

impl SendScope {
    pub(crate) fn new(
        namespace: Namespace,
        client: PublicKey,
        native_group: Vec<u8>,
        app_room: Hex32,
        generation: NonZeroU64,
    ) -> Result<Self> {
        if namespace.account == client || !(1..=256).contains(&native_group.len()) {
            return Err(Error::Binding);
        }
        Ok(Self {
            namespace,
            client,
            native_group,
            app_room,
            generation,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum State {
    Disabled,
    Enabled(Option<Hex32>),
    Terminal,
}

impl State {
    fn from_columns(tag: i64, digest: Option<[u8; 32]>) -> Result<Self> {
        match (tag, digest) {
            (0, None) => Ok(Self::Disabled),
            (1, digest) => Ok(Self::Enabled(digest.map(Hex32))),
            (2, None) => Ok(Self::Terminal),
            _ => Err(Error::Schema),
        }
    }

    fn columns(self) -> (i64, Option<[u8; 32]>) {
        match self {
            Self::Disabled => (0, None),
            Self::Enabled(digest) => (1, digest.map(|digest| digest.0)),
            Self::Terminal => (2, None),
        }
    }

    fn digest(self) -> Option<Hex32> {
        if let Self::Enabled(digest) = self {
            digest
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Floors {
    pub(crate) state: State,
    pub(crate) sequence: Option<NonZeroU64>,
    pub(crate) generation: Option<Generation>,
    pub(crate) ordinal: Option<NonZeroU64>,
    pub(crate) issue_at: Option<NonZeroU64>,
    pub(crate) heartbeat_wall: Option<NonZeroU64>,
    pub(crate) profile_wall: Option<NonZeroU64>,
}

// These are burned counter receipts, suitable only for a later owner adapter
// that implements the separate write-ahead native publication obligation.
#[must_use]
#[derive(Debug)]
pub(crate) struct HeartbeatReservation {
    scope: SendScope,
    revision: NonZeroU64,
    sequence: NonZeroU64,
    permission: Option<Generation>,
    sampled_at: NonZeroU64,
}

impl HeartbeatReservation {
    pub(crate) fn parts(
        &self,
    ) -> (
        &SendScope,
        NonZeroU64,
        NonZeroU64,
        Option<Generation>,
        NonZeroU64,
    ) {
        (
            &self.scope,
            self.revision,
            self.sequence,
            self.permission,
            self.sampled_at,
        )
    }
}

#[must_use]
#[derive(Debug)]
pub(crate) struct IssueReservation {
    scope: SendScope,
    revision: NonZeroU64,
    generation: Generation,
    issued_at: NonZeroU64,
}

impl IssueReservation {
    pub(crate) fn parts(&self) -> (&SendScope, NonZeroU64, Generation, NonZeroU64) {
        (&self.scope, self.revision, self.generation, self.issued_at)
    }
}

struct Fence {
    revision: NonZeroU64,
    state: State,
    sequence: Option<NonZeroU64>,
    heartbeat_wall: Option<NonZeroU64>,
    ordinal: Option<NonZeroU64>,
    issue_at: Option<NonZeroU64>,
}

impl Fence {
    fn initial(revision: NonZeroU64, state: State) -> Self {
        Self {
            revision,
            state,
            sequence: None,
            heartbeat_wall: None,
            ordinal: state.digest().map(|_| NonZeroU64::MIN),
            issue_at: None,
        }
    }

    fn generation(&self) -> Option<Generation> {
        let (digest, ordinal) = self.state.digest().zip(self.ordinal)?;
        Some(Generation { ordinal, digest })
    }

    fn floors(&self, profile_wall: Option<NonZeroU64>) -> Floors {
        Floors {
            state: self.state,
            sequence: self.sequence,
            generation: self.generation(),
            ordinal: self.ordinal,
            issue_at: self.issue_at,
            heartbeat_wall: self.heartbeat_wall,
            profile_wall,
        }
    }

    fn synchronize(&mut self, revision: NonZeroU64, state: State) -> Result<bool> {
        use std::cmp::Ordering;
        match revision.cmp(&self.revision) {
            Ordering::Less => return Err(Error::Stale),
            Ordering::Equal if state != self.state => return Err(Error::Conflict),
            Ordering::Equal => return Ok(false),
            Ordering::Greater => {}
        }
        let old_digest = self.state.digest();
        let new_digest = state.digest();
        if new_digest != old_digest {
            if new_digest.is_some() {
                self.ordinal = Some(next(self.ordinal)?);
            }
            self.issue_at = None;
        }
        self.state = state;
        self.revision = revision;
        Ok(true)
    }
}

struct Authority {
    revision: NonZeroU64,
    state: State,
}

pub(crate) struct Store<'db> {
    connection: &'db mut Connection,
    namespace: Namespace,
    open_revisions: BTreeMap<NonZeroU64, NonZeroU64>,
    reconcile: Option<NonZeroU64>,
    poisoned: bool,
}

impl<'db> Store<'db> {
    pub(crate) fn attach(connection: &'db mut Connection, namespace: Namespace) -> Result<Self> {
        Self::attach_inner(connection, namespace, false)
    }

    fn attach_inner(
        connection: &'db mut Connection,
        namespace: Namespace,
        fail_after_first_ddl: bool,
    ) -> Result<Self> {
        check_hardening(connection)?;
        let tx = begin(connection)?;
        let result = attach_schema(&tx, &namespace, fail_after_first_ddl);
        let result = finish(tx, result);
        if !connection.is_autocommit() {
            return Err(Error::Poisoned);
        }
        result?;
        Ok(Self {
            connection,
            namespace,
            open_revisions: BTreeMap::new(),
            reconcile: None,
            poisoned: false,
        })
    }

    pub(crate) fn synchronize(
        &mut self,
        scope: &SendScope,
        revision: NonZeroU64,
    ) -> Result<Floors> {
        self.confirm(scope, revision, true)
    }

    pub(crate) fn restore(&mut self, scope: &SendScope, revision: NonZeroU64) -> Result<Floors> {
        self.confirm(scope, revision, false)
    }

    fn confirm(&mut self, scope: &SendScope, revision: NonZeroU64, sync: bool) -> Result<Floors> {
        let result = self.run(scope, |tx, profile_wall| {
            let authority = read_authority(tx, scope)?;
            let existing = load_fence(tx, scope, profile_wall)?;
            if authority.state == State::Terminal
                || existing
                    .as_ref()
                    .is_some_and(|row| row.state == State::Terminal)
            {
                return Err(Error::Retired);
            }
            check_revision(&authority, revision)?;
            let floor = match existing {
                Some(mut row) if sync => {
                    if row.synchronize(revision, authority.state)? {
                        write_fence(tx, scope, &row)?;
                    }
                    row
                }
                Some(row) => {
                    require_synchronized(&row, &authority)?;
                    row
                }
                None if sync => {
                    check_new_fence(tx, scope, false)?;
                    let row = Fence::initial(revision, authority.state);
                    write_fence(tx, scope, &row)?;
                    row
                }
                None => return Err(Error::Unavailable),
            };
            Ok(floor.floors(profile_wall))
        })?;
        self.open_revisions.insert(scope.generation, revision);
        self.confirm_reconciliation(scope);
        Ok(result)
    }

    pub(crate) fn reserve_heartbeat(
        &mut self,
        scope: &SendScope,
        revision: NonZeroU64,
        expected: Option<Generation>,
        sampled_at: NonZeroU64,
        qualified_now: NonZeroU64,
    ) -> Result<HeartbeatReservation> {
        let sequence = self.reserve(scope, revision, |row, profile_wall| {
            if expected != row.generation() {
                return Err(Error::Conflict);
            }
            if sampled_at > qualified_now
                || row.heartbeat_wall.is_some_and(|floor| sampled_at < floor)
                || profile_wall.is_some_and(|floor| qualified_now < floor)
            {
                return Err(Error::Clock);
            }
            let sequence = next(row.sequence)?;
            row.sequence = Some(sequence);
            row.heartbeat_wall = Some(qualified_now);
            Ok((sequence, qualified_now))
        })?;
        Ok(HeartbeatReservation {
            scope: scope.clone(),
            revision,
            sequence,
            permission: expected,
            sampled_at,
        })
    }

    pub(crate) fn reserve_issue(
        &mut self,
        scope: &SendScope,
        revision: NonZeroU64,
        expected: Generation,
        qualified_now: NonZeroU64,
    ) -> Result<IssueReservation> {
        self.reserve(scope, revision, |row, profile_wall| {
            let current = row.generation().ok_or(Error::Disabled)?;
            if current != expected {
                return Err(Error::Conflict);
            }
            if profile_wall.is_some_and(|floor| qualified_now < floor)
                || row.issue_at.is_some_and(|floor| qualified_now <= floor)
            {
                return Err(Error::Clock);
            }
            row.issue_at = Some(qualified_now);
            Ok(((), qualified_now))
        })?;
        Ok(IssueReservation {
            scope: scope.clone(),
            revision,
            generation: expected,
            issued_at: qualified_now,
        })
    }

    fn reserve<T>(
        &mut self,
        scope: &SendScope,
        revision: NonZeroU64,
        reserve: impl FnOnce(&mut Fence, Option<NonZeroU64>) -> Result<(T, NonZeroU64)>,
    ) -> Result<T> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        if self.reconcile.is_some() {
            return Err(Error::ReconciliationRequired);
        }
        let open_revision = self.open_revisions.get(&scope.generation).copied();
        self.run(scope, |tx, profile_wall| {
            let authority = read_authority(tx, scope)?;
            let mut row = load_fence(tx, scope, profile_wall)?.ok_or(Error::Unavailable)?;
            if authority.state == State::Terminal || row.state == State::Terminal {
                return Err(Error::Retired);
            }
            check_revision(&authority, revision)?;
            require_synchronized(&row, &authority)?;
            if !matches!(row.state, State::Enabled(_)) {
                return Err(Error::Disabled);
            }
            if open_revision != Some(revision) {
                return Err(Error::Unavailable);
            }
            let (result, wall) = reserve(&mut row, profile_wall)?;
            write_fence(tx, scope, &row)?;
            let changes = tx.execute(
                "UPDATE main.peer_heartbeat_send_fence_version SET wall = ?1 WHERE singleton = 1",
                [wall.get().to_be_bytes()],
            ).map_err(database_error)?;
            if changes != 1 {
                return Err(Error::Schema);
            }
            Ok(result)
        })
    }

    pub(crate) fn retire(&mut self, scope: &SendScope, revision: NonZeroU64) -> Result<()> {
        self.run(scope, |tx, profile_wall| {
            let authority = read_authority(tx, scope)?;
            check_revision(&authority, revision)?;
            if authority.state != State::Terminal {
                return Err(Error::Conflict);
            }
            let mut row = match load_fence(tx, scope, profile_wall)? {
                Some(mut row) => {
                    if !row.synchronize(revision, State::Terminal)? {
                        return Ok(());
                    }
                    row
                }
                None => {
                    check_new_fence(tx, scope, true)?;
                    Fence::initial(revision, State::Terminal)
                }
            };
            row.issue_at = None;
            write_fence(tx, scope, &row)
        })?;
        self.open_revisions.remove(&scope.generation);
        self.confirm_reconciliation(scope);
        Ok(())
    }

    fn confirm_reconciliation(&mut self, scope: &SendScope) {
        if self.reconcile == Some(scope.generation) {
            self.reconcile = None;
        }
    }

    fn run<T>(
        &mut self,
        scope: &SendScope,
        operation: impl FnOnce(&Transaction<'_>, Option<NonZeroU64>) -> Result<T>,
    ) -> Result<T> {
        if self.poisoned {
            return Err(Error::Poisoned);
        }
        if self
            .reconcile
            .is_some_and(|generation| generation != scope.generation)
        {
            return Err(Error::ReconciliationRequired);
        }
        if scope.namespace != self.namespace {
            return Err(Error::Binding);
        }
        check_hardening(self.connection)?;
        let tx = begin(self.connection)?;
        let result = validated_schema(&tx, &self.namespace)
            .and_then(|profile_wall| operation(&tx, profile_wall));
        let result = finish(tx, result);
        if !self.connection.is_autocommit() {
            self.poisoned = true;
            return Err(Error::Poisoned);
        }
        if matches!(result, Err(Error::CommitUnknown)) {
            self.open_revisions.remove(&scope.generation);
            self.reconcile = Some(scope.generation);
        }
        result
    }
}

fn check_revision(authority: &Authority, expected: NonZeroU64) -> Result<()> {
    if authority.revision == expected {
        Ok(())
    } else {
        Err(Error::Stale)
    }
}

fn require_synchronized(row: &Fence, authority: &Authority) -> Result<()> {
    if row.revision != authority.revision {
        return Err(Error::Stale);
    }
    if row.state != authority.state {
        return Err(Error::Conflict);
    }
    Ok(())
}

fn next(last: Option<NonZeroU64>) -> Result<NonZeroU64> {
    last.map_or(Some(NonZeroU64::MIN), |current| current.checked_add(1))
        .ok_or(Error::Exhausted)
}

fn database_error(error: rusqlite::Error) -> Error {
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Error::Busy,
        _ => Error::Storage,
    }
}

fn check_hardening(connection: &Connection) -> Result<()> {
    if !connection.is_autocommit() {
        return Err(Error::Hardening);
    }
    let cipher: String = connection
        .query_one("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(|_| Error::Hardening)?;
    let journal: String = connection
        .query_one("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(|_| Error::Hardening)?;
    if cipher.is_empty() || journal != "delete" {
        return Err(Error::Hardening);
    }
    for (sql, expected) in [
        ("PRAGMA foreign_keys", 1),
        ("PRAGMA secure_delete", 1),
        ("PRAGMA synchronous", 2),
        ("PRAGMA temp_store", 2),
        ("PRAGMA ignore_check_constraints", 0),
        ("PRAGMA query_only", 0),
        ("PRAGMA writable_schema", 0),
        (
            "SELECT count(*) FROM pragma_database_list WHERE name NOT IN ('main', 'temp')",
            0,
        ),
    ] {
        let actual: i64 = connection
            .query_one(sql, [], |row| row.get(0))
            .map_err(|_| Error::Hardening)?;
        if actual != expected {
            return Err(Error::Hardening);
        }
    }
    Ok(())
}

fn begin(connection: &mut Connection) -> Result<Transaction<'_>> {
    connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error)
}

fn finish<T>(tx: Transaction<'_>, result: Result<T>) -> Result<T> {
    match result {
        Ok(value) => {
            tx.commit().map_err(|_| Error::CommitUnknown)?;
            Ok(value)
        }
        Err(error) => {
            if !tx.is_autocommit() {
                tx.rollback().map_err(database_error)?;
            }
            Err(error)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SchemaObject {
    kind: String,
    name: String,
    table: String,
    sql: Option<String>,
}

fn schema_objects(tx: &Transaction<'_>) -> Result<Vec<SchemaObject>> {
    let temporary = tx
        .query_row(TEMP_SCHEMA, [SCHEMA_PREFIX], |_| Ok(()))
        .optional()
        .map_err(|_| Error::Schema)?;
    if temporary.is_some() {
        return Err(Error::Schema);
    }
    let mut statement = tx.prepare(MAIN_SCHEMA).map_err(|_| Error::Schema)?;
    statement
        .query_map([SCHEMA_PREFIX], |row| {
            Ok(SchemaObject {
                kind: row.get(0)?,
                name: row.get(1)?,
                table: row.get(2)?,
                sql: row.get(3)?,
            })
        })
        .and_then(Iterator::collect)
        .map_err(|_| Error::Schema)
}

fn validate_shape(objects: &[SchemaObject]) -> Result<()> {
    let expected = [
        ("index", DIRECTION_INDEX, ROW_TABLE, DIRECTION_SQL),
        ("table", ROW_TABLE, ROW_TABLE, ROW_SQL),
        ("table", VERSION_TABLE, VERSION_TABLE, VERSION_SQL),
    ];
    if objects.len() != expected.len() {
        return Err(Error::Schema);
    }
    for (actual, (kind, name, table, sql)) in objects.iter().zip(expected) {
        if actual.kind != kind
            || actual.name != name
            || actual.table != table
            || actual.sql.as_deref() != Some(sql)
        {
            return Err(Error::Schema);
        }
    }
    Ok(())
}

fn attach_schema(
    tx: &Transaction<'_>,
    namespace: &Namespace,
    fail_after_first_ddl: bool,
) -> Result<()> {
    let objects = schema_objects(tx)?;
    let profile_wall = if objects.is_empty() {
        tx.execute(VERSION_SQL, []).map_err(database_error)?;
        if fail_after_first_ddl {
            tx.execute("CREATE TABLE", []).map_err(database_error)?;
        }
        tx.execute(ROW_SQL, []).map_err(database_error)?;
        tx.execute(DIRECTION_SQL, []).map_err(database_error)?;
        tx.execute(
            "INSERT INTO main.peer_heartbeat_send_fence_version VALUES (1, 1, ?1, ?2, NULL)",
            params![namespace.profile.0, namespace.account.to_bytes()],
        )
        .map_err(database_error)?;
        let profile_wall = read_version(tx, namespace)?;
        validate_shape(&schema_objects(tx)?)?;
        profile_wall
    } else {
        if !objects
            .iter()
            .any(|object| object.kind == "table" && object.name == VERSION_TABLE)
        {
            return Err(Error::Schema);
        }
        let profile_wall = read_version(tx, namespace)?;
        validate_shape(&objects)?;
        profile_wall
    };

    let mut statement = tx.prepare(ALL_FENCES_SQL).map_err(|_| Error::Schema)?;
    let mut rows = statement.query([]).map_err(|_| Error::Schema)?;
    let mut count = 0;
    while let Some(row) = rows.next().map_err(|_| Error::Schema)? {
        count += 1;
        if count > MAX_FENCES {
            return Err(Error::Capacity);
        }
        parse_fence(row, namespace, profile_wall)?;
    }
    Ok(())
}

fn read_version(tx: &Transaction<'_>, expected: &Namespace) -> Result<Option<NonZeroU64>> {
    let (singleton, version, profile, account, wall) = tx
        .query_one(
            "SELECT singleton, version,
                        CASE WHEN length(CAST(profile AS BLOB)) <= 64 THEN profile END,
                        account, wall
         FROM main.peer_heartbeat_send_fence_version LIMIT 2",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, [u8; 32]>(3)?,
                    row.get::<_, Option<[u8; 8]>>(4)?,
                ))
            },
        )
        .map_err(|_| Error::Schema)?;
    if singleton != 1 || version != 1 {
        return Err(Error::Schema);
    }
    if ProfileName::new(&profile).map_err(|_| Error::Schema)? != expected.profile
        || account != expected.account.to_bytes()
    {
        return Err(Error::Binding);
    }
    decode_optional(wall)
}

fn validated_schema(tx: &Transaction<'_>, namespace: &Namespace) -> Result<Option<NonZeroU64>> {
    validate_shape(&schema_objects(tx)?)?;
    read_version(tx, namespace)
}

fn decode_required(bytes: [u8; 8]) -> Result<NonZeroU64> {
    NonZeroU64::new(u64::from_be_bytes(bytes)).ok_or(Error::Schema)
}

fn decode_optional(bytes: Option<[u8; 8]>) -> Result<Option<NonZeroU64>> {
    bytes.map(decode_required).transpose()
}

fn field<T: rusqlite::types::FromSql>(row: &Row<'_>, index: usize) -> Result<T> {
    row.get(index).map_err(|_| Error::Schema)
}

fn bounded_group(row: &Row<'_>, index: usize) -> Result<Vec<u8>> {
    let value = row.get_ref(index).map_err(|_| Error::Schema)?;
    let bytes = value.as_blob().map_err(|_| Error::Schema)?;
    if !(1..=256).contains(&bytes.len()) {
        return Err(Error::Binding);
    }
    Ok(bytes.to_vec())
}

fn read_scope(row: &Row<'_>, first: usize, namespace: &Namespace) -> Result<SendScope> {
    let server: [u8; 32] = field(row, first)?;
    if server != namespace.account.to_bytes() {
        return Err(Error::Binding);
    }
    let client: [u8; 32] = field(row, first + 1)?;
    let client = PublicKey::from_slice(&client).map_err(|_| Error::Schema)?;
    SendScope::new(
        namespace.clone(),
        client,
        bounded_group(row, first + 2)?,
        Hex32(field(row, first + 3)?),
        decode_required(field(row, first + 4)?)?,
    )
}

fn read_authority(tx: &Transaction<'_>, scope: &SendScope) -> Result<Authority> {
    let table_type: Option<String> = tx
        .query_one(
            "SELECT type FROM main.sqlite_schema WHERE name = ?1",
            [AUTHORITY_TABLE],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| Error::Schema)?;
    match table_type.as_deref() {
        None => return Err(Error::Unavailable),
        Some("table") => {}
        Some(_) => return Err(Error::Schema),
    }
    let mut statement = tx.prepare(AUTHORITY_SQL).map_err(|_| Error::Schema)?;
    let mut rows = statement
        .query([scope.generation.get().to_be_bytes()])
        .map_err(|_| Error::Schema)?;
    let row = rows
        .next()
        .map_err(|_| Error::Schema)?
        .ok_or(Error::Unavailable)?;
    let profile = row.get_ref(0).map_err(|_| Error::Schema)?;
    let profile = profile.as_str().map_err(|_| Error::Schema)?;
    if ProfileName::new(profile).map_err(|_| Error::Schema)? != scope.namespace.profile
        || read_scope(row, 1, &scope.namespace)? != *scope
        || field::<i64>(row, 7)? != 1
    {
        return Err(Error::Binding);
    }
    let authority = Authority {
        revision: decode_required(field(row, 6)?)?,
        state: State::from_columns(field(row, 8)?, field(row, 9)?)?,
    };
    if rows.next().map_err(|_| Error::Schema)?.is_some() {
        return Err(Error::Schema);
    }
    Ok(authority)
}

fn parse_fence(
    row: &Row<'_>,
    namespace: &Namespace,
    profile_wall: Option<NonZeroU64>,
) -> Result<(SendScope, Fence)> {
    let scope = read_scope(row, 0, namespace)?;
    let fence = Fence {
        revision: decode_required(field(row, 5)?)?,
        state: State::from_columns(field(row, 6)?, field(row, 7)?)?,
        sequence: decode_optional(field(row, 8)?)?,
        heartbeat_wall: decode_optional(field(row, 9)?)?,
        ordinal: decode_optional(field(row, 10)?)?,
        issue_at: decode_optional(field(row, 11)?)?,
    };
    if (fence.state.digest().is_some() && fence.ordinal.is_none())
        || (fence.issue_at.is_some() && fence.generation().is_none())
        || fence.sequence.is_some() != fence.heartbeat_wall.is_some()
        || fence.heartbeat_wall > profile_wall
        || fence.issue_at > profile_wall
    {
        return Err(Error::Schema);
    }
    Ok((scope, fence))
}

fn load_fence(
    tx: &Transaction<'_>,
    scope: &SendScope,
    profile_wall: Option<NonZeroU64>,
) -> Result<Option<Fence>> {
    let mut statement = tx.prepare(ONE_FENCE_SQL).map_err(|_| Error::Schema)?;
    let mut rows = statement
        .query([scope.generation.get().to_be_bytes()])
        .map_err(|_| Error::Schema)?;
    let Some(row) = rows.next().map_err(|_| Error::Schema)? else {
        return Ok(None);
    };
    let (stored_scope, fence) = parse_fence(row, &scope.namespace, profile_wall)?;
    if stored_scope != *scope {
        return Err(Error::Binding);
    }
    Ok(Some(fence))
}

fn check_new_fence(tx: &Transaction<'_>, scope: &SendScope, terminal: bool) -> Result<()> {
    let count: i64 = tx
        .query_one(
            "SELECT count(*) FROM (SELECT 1 FROM main.peer_heartbeat_send_fence_rows LIMIT 65)",
            [],
            |row| row.get(0),
        )
        .map_err(database_error)?;
    if count >= MAX_FENCES {
        return Err(Error::Capacity);
    }
    if !terminal {
        let active: bool = tx
            .query_one(
                "SELECT EXISTS (SELECT 1 FROM main.peer_heartbeat_send_fence_rows
                            WHERE server = ?1 AND client = ?2 AND state <> 2)",
                params![scope.namespace.account.to_bytes(), scope.client.to_bytes()],
                |row| row.get(0),
            )
            .map_err(database_error)?;
        if active {
            return Err(Error::Conflict);
        }
    }
    Ok(())
}

fn write_fence(tx: &Transaction<'_>, scope: &SendScope, row: &Fence) -> Result<()> {
    let (state, digest) = row.state.columns();
    let bytes = |value: NonZeroU64| value.get().to_be_bytes();
    let changes = tx
        .execute(
            WRITE_FENCE_SQL,
            params![
                bytes(scope.generation),
                scope.namespace.account.to_bytes(),
                scope.client.to_bytes(),
                scope.native_group,
                scope.app_room.0,
                bytes(row.revision),
                state,
                digest,
                row.sequence.map(bytes),
                row.heartbeat_wall.map(bytes),
                row.ordinal.map(bytes),
                row.issue_at.map(bytes),
            ],
        )
        .map_err(database_error)?;
    if changes == 1 {
        Ok(())
    } else {
        Err(Error::Schema)
    }
}

#[cfg(test)]
#[path = "peer_heartbeat_send_fences_tests.rs"]
pub(crate) mod core_tests;
