use super::*;
use nostr::key::{Keys, SecretKey};
use rusqlite::OpenFlags;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use storage_sqlite::{SqlCipherHardening, SqlCipherKey, open_hardened_sqlcipher};
use tempfile::TempDir;

const KEY: [u8; 32] = [29; 32];
const AUTHORITY_SCHEMA: &str = "CREATE TABLE main.peer_heartbeat_test_authority (
    profile TEXT, server BLOB, client BLOB, native_group BLOB,
    app_room BLOB, generation BLOB, revision BLOB, role INTEGER, state INTEGER, digest BLOB
) STRICT";

fn number(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}

fn account(value: u64) -> PublicKey {
    let mut secret = [0; 32];
    secret[24..].copy_from_slice(&value.to_be_bytes());
    Keys::new(SecretKey::from_slice(&secret).unwrap()).public_key()
}

fn namespace() -> Namespace {
    Namespace {
        profile: ProfileName::new("test_Profile-1").unwrap(),
        account: account(1),
    }
}

fn scope(generation: u64, client: u64) -> SendScope {
    SendScope::new(
        namespace(),
        account(client),
        generation.to_be_bytes().to_vec(),
        Hex32([generation as u8; 32]),
        number(generation),
    )
    .unwrap()
}

fn digest(value: u8) -> Hex32 {
    Hex32([value; 32])
}

fn policy(value: u8) -> State {
    State::Enabled(Some(digest(value)))
}

fn permission(ordinal: u64, value: u8) -> Generation {
    Generation {
        ordinal: number(ordinal),
        digest: digest(value),
    }
}

struct Fixture {
    root: TempDir,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("send-fences.db");
        fs::write(&path, []).unwrap();
        Self { root, path }
    }

    fn open(&self) -> Connection {
        let connection = encrypted_connection(&self.path, KEY).unwrap();
        connection.busy_timeout(Duration::ZERO).unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
            .unwrap();
        let journal: String = connection
            .query_one("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "delete");
        connection
    }

    fn clean(self) {
        let mut names = fs::read_dir(self.root.path()).unwrap();
        assert_eq!(names.next().unwrap().unwrap().path(), self.path);
        assert!(names.next().is_none(), "fixture sidecar remained");
        let root_path = self.root.path().to_owned();
        self.root.close().unwrap();
        assert!(!root_path.exists());
    }

    fn unchanged(&self, before: &[u8]) {
        assert_eq!(fs::read(&self.path).unwrap(), before);
        assert_eq!(fs::read_dir(self.root.path()).unwrap().count(), 1);
    }
}

fn encrypted_connection(path: &Path, key: [u8; 32]) -> std::result::Result<Connection, String> {
    marmish_engine::initialize_storage().map_err(|error| error.to_string())?;
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(|error| error.to_string())?;
    open_hardened_sqlcipher(
        &connection,
        &SqlCipherKey::raw(key),
        SqlCipherHardening::live_cache(),
    )
    .map_err(|error| error.to_string())?;
    Ok(connection)
}

fn install_fixture(connection: &Connection) {
    connection.execute(AUTHORITY_SCHEMA, []).unwrap();
}

fn authorize(connection: &Connection, scope: &SendScope, revision: NonZeroU64, state: State) {
    connection
        .execute(
            "DELETE FROM main.peer_heartbeat_test_authority WHERE generation=?1",
            [scope.generation.get().to_be_bytes()],
        )
        .unwrap();
    let (tag, digest) = state.columns();
    connection
        .execute(
            "INSERT INTO main.peer_heartbeat_test_authority VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?9)",
            params![
                scope.namespace.profile.0,
                scope.namespace.account.to_bytes(),
                scope.client.to_bytes(),
                scope.native_group,
                scope.app_room.0,
                scope.generation.get().to_be_bytes(),
                revision.get().to_be_bytes(),
                tag,
                digest,
            ],
        )
        .unwrap();
}

#[test]
fn core_exact_policy_and_heartbeat_fences_survive_reopen() {
    let fixture = Fixture::new();
    let first = scope(1, 2);
    let generation = permission(1, 7);
    let revision = number(1);
    {
        let mut connection = fixture.open();
        install_fixture(&connection);
        authorize(&connection, &first, revision, policy(7));
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        let initial = store.synchronize(&first, revision).unwrap();
        assert_eq!(initial.generation, Some(generation));
        assert_eq!(initial.sequence, None);
        assert!(matches!(
            store.reserve_heartbeat(&first, revision, None, number(98), number(99)),
            Err(Error::Conflict)
        ));
        assert!(matches!(
            store.reserve_issue(&first, revision, permission(1, 8), number(99)),
            Err(Error::Conflict)
        ));

        let heartbeat = store
            .reserve_heartbeat(&first, revision, Some(generation), number(98), number(99))
            .unwrap();
        assert_eq!(
            heartbeat.parts(),
            (&first, revision, number(1), Some(generation), number(98))
        );
        let issue = store
            .reserve_issue(&first, revision, generation, number(100))
            .unwrap();
        assert_eq!(issue.parts(), (&first, revision, generation, number(100)));
        assert!(matches!(
            store.reserve_issue(&first, revision, generation, number(100)),
            Err(Error::Clock)
        ));
    }
    {
        let mut connection = fixture.open();
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        assert!(matches!(
            store.reserve_heartbeat(&first, revision, Some(generation), number(100), number(101)),
            Err(Error::Unavailable)
        ));
        let restored = store.restore(&first, revision).unwrap();
        assert_eq!(restored.sequence, Some(number(1)));
        assert_eq!(restored.heartbeat_wall, Some(number(99)));
        assert_eq!(restored.issue_at, Some(number(100)));
        assert_eq!(restored.profile_wall, Some(number(100)));
        assert_eq!(restored.ordinal, Some(number(1)));
        let heartbeat = store
            .reserve_heartbeat(&first, revision, Some(generation), number(100), number(101))
            .unwrap();
        assert_eq!(heartbeat.parts().2, number(2));
    }
    assert!(encrypted_connection(&fixture.path, [11; 32]).is_err());
    assert!(
        !fs::read(&fixture.path)
            .unwrap()
            .starts_with(b"SQLite format 3")
    );
    fixture.clean();
}

#[test]
fn core_revision_changes_disable_and_reenable_with_new_policy_ordinal() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    install_fixture(&connection);
    let first = scope(1, 2);
    let mut store = Store::attach(&mut connection, namespace()).unwrap();
    let revision = number(1);
    authorize(store.connection, &first, revision, State::Disabled);
    assert_eq!(store.synchronize(&first, revision).unwrap().ordinal, None);
    assert!(matches!(
        store.reserve_heartbeat(&first, revision, None, number(10), number(10)),
        Err(Error::Disabled)
    ));

    authorize(store.connection, &first, number(2), policy(7));
    assert_eq!(store.synchronize(&first, revision), Err(Error::Stale));
    assert_eq!(store.restore(&first, number(2)), Err(Error::Stale));
    assert_eq!(
        store.synchronize(&first, number(2)).unwrap().generation,
        Some(permission(1, 7))
    );
    assert_eq!(
        store
            .reserve_issue(&first, number(2), permission(1, 7), number(12))
            .unwrap()
            .parts()
            .3,
        number(12)
    );

    authorize(store.connection, &first, number(3), policy(7));
    assert_eq!(
        store.synchronize(&first, number(3)).unwrap().issue_at,
        Some(number(12))
    );
    authorize(store.connection, &first, number(3), policy(8));
    assert_eq!(store.synchronize(&first, number(3)), Err(Error::Conflict));
    assert_eq!(store.restore(&first, number(3)), Err(Error::Conflict));
    authorize(store.connection, &first, number(2), policy(7));
    assert_eq!(store.synchronize(&first, number(2)), Err(Error::Stale));

    authorize(store.connection, &first, number(4), State::Enabled(None));
    let no_policy = store.synchronize(&first, number(4)).unwrap();
    assert_eq!(no_policy.ordinal, Some(number(1)));
    assert_eq!(no_policy.generation, None);
    assert_eq!(no_policy.issue_at, None);
    assert!(matches!(
        store.reserve_heartbeat(
            &first,
            number(4),
            Some(permission(1, 7)),
            number(13),
            number(13)
        ),
        Err(Error::Conflict)
    ));
    assert_eq!(
        store
            .reserve_heartbeat(&first, number(4), None, number(13), number(13))
            .unwrap()
            .parts()
            .2,
        number(1)
    );

    authorize(store.connection, &first, number(5), policy(7));
    let reenabled = store.synchronize(&first, number(5)).unwrap();
    assert_eq!(reenabled.generation, Some(permission(2, 7)));
    assert_eq!(reenabled.sequence, Some(number(1)));
    authorize(store.connection, &first, number(6), policy(8));
    assert_eq!(
        store.synchronize(&first, number(6)).unwrap().generation,
        Some(permission(3, 8))
    );
    drop(store);
    drop(connection);
    fixture.clean();
}

#[test]
fn core_profile_clock_and_per_room_sample_floors_are_distinct() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    install_fixture(&connection);
    let first = scope(1, 2);
    let second = scope(2, 3);
    let revision = number(1);
    authorize(&connection, &first, revision, State::Enabled(None));
    authorize(&connection, &second, revision, policy(1));
    {
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        store.synchronize(&first, revision).unwrap();
        store.synchronize(&second, revision).unwrap();
        assert!(matches!(
            store.reserve_heartbeat(&first, revision, None, number(11), number(10)),
            Err(Error::Clock)
        ));
        assert_eq!(store.restore(&first, revision).unwrap().profile_wall, None);
        assert_eq!(
            store
                .reserve_heartbeat(&first, revision, None, number(9), number(10))
                .unwrap()
                .parts()
                .2,
            number(1)
        );
        assert_eq!(
            store
                .reserve_issue(&second, revision, permission(1, 1), number(30))
                .unwrap()
                .parts()
                .3,
            number(30)
        );
        assert!(matches!(
            store.reserve_heartbeat(&first, revision, None, number(12), number(29)),
            Err(Error::Clock)
        ));
        assert!(matches!(
            store.reserve_heartbeat(&first, revision, None, number(9), number(30)),
            Err(Error::Clock)
        ));

        // The sample precedes the other room's issue, but meets its own floor.
        let heartbeat = store
            .reserve_heartbeat(&first, revision, None, number(12), number(30))
            .unwrap();
        assert_eq!(heartbeat.parts().2, number(2));
        let other = store.restore(&second, revision).unwrap();
        assert_eq!(other.heartbeat_wall, None);
        assert_eq!(other.sequence, None);
        assert_eq!(other.profile_wall, Some(number(30)));
    }
    drop(connection);
    {
        let mut connection = fixture.open();
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        assert_eq!(
            store.restore(&first, revision).unwrap().heartbeat_wall,
            Some(number(30))
        );
        assert_eq!(
            store.restore(&second, revision).unwrap().issue_at,
            Some(number(30))
        );
        assert!(matches!(
            store.reserve_issue(&second, revision, permission(1, 1), number(29)),
            Err(Error::Clock)
        ));
    }
    fixture.clean();
}

#[test]
fn core_real_commit_abort_burns_no_uncommitted_value_and_requires_exact_readback() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    install_fixture(&connection);
    let first = scope(1, 2);
    let second = scope(2, 3);
    let revision = number(1);
    authorize(&connection, &first, revision, policy(1));
    authorize(&connection, &second, revision, State::Enabled(None));

    let abort = Arc::new(AtomicBool::new(false));
    let armed = Arc::clone(&abort);
    // Locked MDK storage-sqlite explicitly enables rusqlite's hooks feature.
    connection
        .commit_hook(Some(move || armed.swap(false, Ordering::AcqRel)))
        .unwrap();
    {
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        store.synchronize(&first, revision).unwrap();
        store.synchronize(&second, revision).unwrap();
        abort.store(true, Ordering::Release);
        assert!(matches!(
            store.reserve_heartbeat(
                &first,
                revision,
                Some(permission(1, 1)),
                number(10),
                number(10)
            ),
            Err(Error::CommitUnknown)
        ));
        assert!(!abort.load(Ordering::Acquire));
        assert!(store.connection.is_autocommit());
        assert!(matches!(
            store.reserve_issue(&first, revision, permission(1, 1), number(11)),
            Err(Error::ReconciliationRequired)
        ));
        assert!(matches!(
            store.reserve_heartbeat(&second, revision, None, number(12), number(12)),
            Err(Error::ReconciliationRequired)
        ));
        assert_eq!(
            store.restore(&second, revision),
            Err(Error::ReconciliationRequired)
        );

        let restored = store.restore(&first, revision).unwrap();
        assert_eq!(restored.profile_wall, None);
        assert_eq!(restored.sequence, None);
        assert_eq!(restored.heartbeat_wall, None);
        assert_eq!(
            store
                .reserve_heartbeat(
                    &first,
                    revision,
                    Some(permission(1, 1)),
                    number(10),
                    number(10)
                )
                .unwrap()
                .parts()
                .2,
            number(1)
        );
    }
    drop(connection);
    fixture.clean();
}

#[test]
fn core_failed_ddl_rolls_back_and_attaches_only_from_an_empty_namespace() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    install_fixture(&connection);
    let before = fs::read(&fixture.path).unwrap();
    assert!(matches!(
        Store::attach_inner(&mut connection, namespace(), true),
        Err(Error::Storage)
    ));
    assert!(connection.is_autocommit());
    fixture.unchanged(&before);
    assert_eq!(
        connection
            .query_one(MAIN_SCHEMA, [SCHEMA_PREFIX], |_| Ok(()))
            .optional()
            .unwrap(),
        None
    );
    Store::attach(&mut connection, namespace()).unwrap();
    drop(connection);
    fixture.clean();
}

#[test]
fn failed_rollback_poisoning_requires_a_new_store() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization, TransactionOperation};

    let fixture = Fixture::new();
    let mut connection = fixture.open();
    install_fixture(&connection);
    let first = scope(1, 2);
    let revision = number(1);
    authorize(&connection, &first, revision, State::Enabled(None));
    let deny = Arc::new(AtomicBool::new(false));
    let armed = Arc::clone(&deny);
    connection
        .authorizer(Some(move |context: AuthContext<'_>| {
            if armed.load(Ordering::Acquire)
                && matches!(
                    context.action,
                    AuthAction::Transaction {
                        operation: TransactionOperation::Rollback
                    }
                )
            {
                Authorization::Deny
            } else {
                Authorization::Allow
            }
        }))
        .unwrap();
    {
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        store.synchronize(&first, revision).unwrap();
        deny.store(true, Ordering::Release);
        assert!(matches!(
            store.reserve_heartbeat(&first, revision, None, number(11), number(10)),
            Err(Error::Poisoned)
        ));
        assert!(!store.connection.is_autocommit());
        deny.store(false, Ordering::Release);
        store.connection.execute_batch("ROLLBACK").unwrap();
        assert_eq!(store.restore(&first, revision), Err(Error::Poisoned));
    }
    {
        let mut restored = Store::attach(&mut connection, namespace()).unwrap();
        assert_eq!(restored.restore(&first, revision).unwrap().sequence, None);
    }
    drop(connection);
    fixture.clean();
}

#[test]
fn exact_fixture_scope_roles_shapes_and_direction() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    let first = scope(1, 2);
    let competing = scope(2, 2);
    let revision = number(1);
    let mut store = Store::attach(&mut connection, namespace()).unwrap();
    assert_eq!(store.synchronize(&first, revision), Err(Error::Unavailable));
    install_fixture(store.connection);
    assert_eq!(store.synchronize(&first, revision), Err(Error::Unavailable));
    authorize(store.connection, &first, revision, policy(1));

    let mut foreign_profile = first.clone();
    foreign_profile.namespace.profile = ProfileName::new("other").unwrap();
    assert_eq!(
        store.synchronize(&foreign_profile, revision),
        Err(Error::Binding)
    );
    let mut foreign_account = first.clone();
    foreign_account.namespace.account = account(5);
    assert_eq!(
        store.synchronize(&foreign_account, revision),
        Err(Error::Binding)
    );
    let mut wrong_group = first.clone();
    wrong_group.native_group = vec![3];
    assert_eq!(
        store.synchronize(&wrong_group, revision),
        Err(Error::Binding)
    );
    let mut wrong_room = first.clone();
    wrong_room.app_room = Hex32([11; 32]);
    assert_eq!(
        store.synchronize(&wrong_room, revision),
        Err(Error::Binding)
    );
    assert_eq!(
        SendScope::new(namespace(), account(1), vec![1], Hex32([1; 32]), number(1)),
        Err(Error::Binding)
    );
    assert_eq!(
        SendScope::new(
            namespace(),
            account(2),
            vec![1; 257],
            Hex32([1; 32]),
            number(1)
        ),
        Err(Error::Binding)
    );
    assert_eq!(ProfileName::new("x';roll back"), Err(Error::Binding));

    store
        .connection
        .execute("UPDATE main.peer_heartbeat_test_authority SET role=0", [])
        .unwrap();
    assert_eq!(store.synchronize(&first, revision), Err(Error::Binding));
    authorize(store.connection, &first, revision, policy(1));
    store.connection.execute(
        "INSERT INTO main.peer_heartbeat_test_authority SELECT * FROM main.peer_heartbeat_test_authority",
        [],
    ).unwrap();
    assert_eq!(store.synchronize(&first, revision), Err(Error::Schema));
    authorize(store.connection, &first, revision, policy(1));
    store
        .connection
        .execute(
            "UPDATE main.peer_heartbeat_test_authority SET revision=X'13'",
            [],
        )
        .unwrap();
    assert_eq!(store.synchronize(&first, revision), Err(Error::Schema));
    authorize(store.connection, &first, revision, policy(1));
    store
        .connection
        .execute(
            "UPDATE main.peer_heartbeat_test_authority SET generation=X'0000000000000002'",
            [],
        )
        .unwrap();
    assert_eq!(store.synchronize(&first, revision), Err(Error::Unavailable));
    store
        .connection
        .execute("DELETE FROM main.peer_heartbeat_test_authority", [])
        .unwrap();

    authorize(store.connection, &first, revision, State::Disabled);
    authorize(store.connection, &competing, revision, policy(1));
    store.synchronize(&first, revision).unwrap();
    assert_eq!(
        store.synchronize(&competing, revision),
        Err(Error::Conflict)
    );
    assert_eq!(store.retire(&first, revision), Err(Error::Conflict));
    authorize(store.connection, &first, revision, State::Terminal);
    assert_eq!(store.retire(&first, revision), Err(Error::Conflict));
    authorize(store.connection, &first, number(2), State::Terminal);
    assert_eq!(store.synchronize(&first, number(2)), Err(Error::Retired));
    store.retire(&first, number(2)).unwrap();
    store.retire(&first, number(2)).unwrap();
    assert_eq!(
        store.synchronize(&competing, revision).unwrap().generation,
        Some(permission(1, 1))
    );
    authorize(store.connection, &first, number(3), State::Enabled(None));
    assert_eq!(store.restore(&first, number(2)), Err(Error::Retired));
    assert_eq!(store.synchronize(&first, number(3)), Err(Error::Retired));
    drop(store);
    drop(connection);
    fixture.clean();
}

#[test]
fn full_width_counters_exhaust_without_preventing_closure() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    install_fixture(&connection);
    let one = scope(1, 2);
    let rev = number(u64::MAX - 2);
    authorize(&connection, &one, rev, policy(1));
    let mut store = Store::attach(&mut connection, namespace()).unwrap();
    store.synchronize(&one, rev).unwrap();
    let wide = (u64::MAX - 1).to_be_bytes();
    store
        .connection
        .execute(
            "UPDATE main.peer_heartbeat_send_fence_rows
         SET sequence=?1, heartbeat_wall=?1, ordinal=?1",
            [wide],
        )
        .unwrap();
    store
        .connection
        .execute(
            "UPDATE main.peer_heartbeat_send_fence_version SET wall=?1",
            [wide],
        )
        .unwrap();
    let almost = permission(u64::MAX - 1, 1);
    let maximum = number(u64::MAX);
    assert_eq!(
        store
            .reserve_heartbeat(&one, rev, Some(almost), maximum, maximum)
            .unwrap()
            .parts()
            .2,
        maximum
    );
    assert_eq!(
        store
            .reserve_issue(&one, rev, almost, maximum)
            .unwrap()
            .parts()
            .3,
        maximum
    );
    assert!(matches!(
        store.reserve_heartbeat(&one, rev, Some(almost), maximum, maximum),
        Err(Error::Exhausted)
    ));
    assert!(matches!(
        store.reserve_issue(&one, rev, almost, maximum),
        Err(Error::Clock)
    ));

    authorize(store.connection, &one, number(u64::MAX - 1), policy(2));
    assert_eq!(
        store
            .synchronize(&one, number(u64::MAX - 1))
            .unwrap()
            .generation,
        Some(permission(u64::MAX, 2))
    );
    authorize(store.connection, &one, maximum, policy(3));
    assert_eq!(store.synchronize(&one, maximum), Err(Error::Exhausted));
    // Keep the live fence at the previous confirmed revision.
    authorize(store.connection, &one, maximum, State::Disabled);
    assert_eq!(store.synchronize(&one, maximum).unwrap().generation, None);
    drop(store);
    drop(connection);
    fixture.clean();
}

#[test]
fn retained_generations_are_bounded_but_existing_scope_can_close() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    install_fixture(&connection);
    let client = 2;
    let first = scope(1, client);
    authorize(&connection, &first, number(1), State::Enabled(None));
    let mut store = Store::attach(&mut connection, namespace()).unwrap();
    store.synchronize(&first, number(1)).unwrap();
    for generation in 2..=MAX_FENCES as u64 {
        let retired = scope(generation, client);
        authorize(store.connection, &retired, number(1), State::Terminal);
        store.retire(&retired, number(1)).unwrap();
    }
    let unseen = scope(65, 3);
    authorize(store.connection, &unseen, number(1), State::Enabled(None));
    assert_eq!(store.synchronize(&unseen, number(1)), Err(Error::Capacity));
    authorize(store.connection, &unseen, number(2), State::Terminal);
    assert_eq!(store.retire(&unseen, number(2)), Err(Error::Capacity));
    authorize(store.connection, &first, number(2), State::Terminal);
    store.retire(&first, number(2)).unwrap();
    drop(store);
    drop(connection);
    fixture.clean();
}

#[test]
fn unsupported_schema_or_binding_leaves_existing_bytes_unchanged() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    connection
        .execute(
            "CREATE TABLE main.PEER_HEARTBEAT_SEND_FENCE_unused(id INTEGER)",
            [],
        )
        .unwrap();
    let before = fs::read(&fixture.path).unwrap();
    assert!(matches!(
        Store::attach(&mut connection, namespace()),
        Err(Error::Schema)
    ));
    fixture.unchanged(&before);
    connection
        .execute("DROP TABLE main.PEER_HEARTBEAT_SEND_FENCE_unused", [])
        .unwrap();
    Store::attach(&mut connection, namespace()).unwrap();
    let before = fs::read(&fixture.path).unwrap();
    let mut wrong_profile = namespace();
    wrong_profile.profile = ProfileName::new("other").unwrap();
    assert!(matches!(
        Store::attach(&mut connection, wrong_profile),
        Err(Error::Binding)
    ));
    let mut wrong_account = namespace();
    wrong_account.account = account(3);
    assert!(matches!(
        Store::attach(&mut connection, wrong_account),
        Err(Error::Binding)
    ));
    fixture.unchanged(&before);

    connection
        .execute_batch(
            "PRAGMA ignore_check_constraints=ON;
        UPDATE main.peer_heartbeat_send_fence_version SET version=9;
        PRAGMA ignore_check_constraints=OFF;",
        )
        .unwrap();
    let before = fs::read(&fixture.path).unwrap();
    assert!(matches!(
        Store::attach(&mut connection, namespace()),
        Err(Error::Schema)
    ));
    fixture.unchanged(&before);
    connection
        .execute(
            "UPDATE main.peer_heartbeat_send_fence_version SET version=1",
            [],
        )
        .unwrap();

    connection
        .execute("DROP TABLE main.peer_heartbeat_send_fence_rows", [])
        .unwrap();
    connection
        .execute(
            &ROW_SQL.replace("client <> server", "client IS NOT NULL"),
            [],
        )
        .unwrap();
    connection.execute(DIRECTION_SQL, []).unwrap();
    let before = fs::read(&fixture.path).unwrap();
    assert!(matches!(
        Store::attach(&mut connection, namespace()),
        Err(Error::Schema)
    ));
    fixture.unchanged(&before);
    drop(connection);
    fixture.clean();
}

#[test]
fn rejects_schema_drift_temp_triggers_and_invalid_typed_rows() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    let first = scope(1, 2);
    let revision = number(1);
    install_fixture(&connection);
    authorize(&connection, &first, revision, policy(1));
    connection
        .execute(
            "CREATE TEMP TABLE peer_heartbeat_send_fence_rows(id INTEGER)",
            [],
        )
        .unwrap();
    assert!(matches!(
        Store::attach(&mut connection, namespace()),
        Err(Error::Schema)
    ));
    connection
        .execute("DROP TABLE temp.peer_heartbeat_send_fence_rows", [])
        .unwrap();

    {
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        store.synchronize(&first, revision).unwrap();
        // Also ensure an arbitrary trigger name is caught through tbl_name.
        store
            .connection
            .execute(
                "CREATE TEMP TRIGGER hidden AFTER UPDATE ON main.peer_heartbeat_send_fence_rows
             BEGIN SELECT 1; END",
                [],
            )
            .unwrap();
        let before = fs::read(&fixture.path).unwrap();
        assert_eq!(store.restore(&first, revision), Err(Error::Schema));
        assert!(matches!(
            store.reserve_heartbeat(
                &first,
                revision,
                Some(permission(1, 1)),
                number(10),
                number(10)
            ),
            Err(Error::Schema)
        ));
        fixture.unchanged(&before);
        store
            .connection
            .execute("DROP TRIGGER temp.hidden", [])
            .unwrap();
        store
            .connection
            .execute(
                "CREATE VIEW main.peer_heartbeat_send_fence_extra AS SELECT 1",
                [],
            )
            .unwrap();
        assert_eq!(store.restore(&first, revision), Err(Error::Schema));
        store
            .connection
            .execute("DROP VIEW main.peer_heartbeat_send_fence_extra", [])
            .unwrap();

        store
            .connection
            .execute_batch(
                "PRAGMA ignore_check_constraints=ON;
            UPDATE main.peer_heartbeat_send_fence_rows SET sequence=X'0000000000000000';
            PRAGMA ignore_check_constraints=OFF;",
            )
            .unwrap();
        assert_eq!(store.restore(&first, revision), Err(Error::Schema));
    }
    let before = fs::read(&fixture.path).unwrap();
    assert!(matches!(
        Store::attach(&mut connection, namespace()),
        Err(Error::Schema)
    ));
    fixture.unchanged(&before);
    drop(connection);
    fixture.clean();
}

#[test]
fn detects_wrong_hardening_and_busy_writer_without_changes() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    install_fixture(&connection);
    let first = scope(1, 2);
    let revision = number(1);
    authorize(&connection, &first, revision, State::Enabled(None));
    {
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        store.synchronize(&first, revision).unwrap();
    }
    let before = fs::read(&fixture.path).unwrap();
    for (off, on) in [
        ("PRAGMA foreign_keys=OFF", "PRAGMA foreign_keys=ON"),
        ("PRAGMA secure_delete=OFF", "PRAGMA secure_delete=ON"),
        ("PRAGMA synchronous=NORMAL", "PRAGMA synchronous=FULL"),
        ("PRAGMA temp_store=FILE", "PRAGMA temp_store=MEMORY"),
        (
            "PRAGMA ignore_check_constraints=ON",
            "PRAGMA ignore_check_constraints=OFF",
        ),
        ("PRAGMA query_only=ON", "PRAGMA query_only=OFF"),
        ("PRAGMA writable_schema=ON", "PRAGMA writable_schema=OFF"),
        ("ATTACH ':memory:' AS unexpected", "DETACH unexpected"),
        ("BEGIN", "ROLLBACK"),
    ] {
        connection.execute_batch(off).unwrap();
        assert!(
            matches!(
                Store::attach(&mut connection, namespace()),
                Err(Error::Hardening)
            ),
            "{off}"
        );
        connection.execute_batch(on).unwrap();
    }
    {
        let mut other = fixture.open();
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        store.restore(&first, revision).unwrap();
        let held = other
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert!(matches!(
            store.reserve_heartbeat(&first, revision, None, number(10), number(10)),
            Err(Error::Busy)
        ));
        held.rollback().unwrap();
        assert_eq!(store.restore(&first, revision).unwrap().sequence, None);
    }
    fixture.unchanged(&before);
    drop(connection);
    fixture.clean();
}

fn report(name: &str, mut times: Vec<Duration>) {
    let count = times.len();
    times.sort_unstable();
    let percentile = |p: usize| times[count * p / 100 - 1].as_micros();
    eprintln!(
        "{name} n={count} p50_us={} p95_us={} p99_us={} max_us={}",
        percentile(50),
        percentile(95),
        percentile(99),
        times[count - 1].as_micros()
    );
}

#[test]
#[ignore = "local SQLCipher latency qualification"]
fn benchmark_peer_heartbeat_send_fences() {
    let fixture = Fixture::new();
    let mut connection = fixture.open();
    let exact = scope(1, 2);
    let revision = number(1);
    let generation = permission(1, 7);
    install_fixture(&connection);
    authorize(&connection, &exact, revision, policy(7));
    let mut sequences = Vec::new();
    let mut issues = Vec::new();
    let mut reopens = Vec::new();
    {
        let mut store = Store::attach(&mut connection, namespace()).unwrap();
        store.synchronize(&exact, revision).unwrap();
        for sample in 1..=200 {
            let wall = number(sample);
            let start = Instant::now();
            let result = store
                .reserve_heartbeat(&exact, revision, Some(generation), wall, wall)
                .unwrap();
            sequences.push(start.elapsed());
            assert_eq!(result.parts().2, wall);
            let start = Instant::now();
            let issue = store
                .reserve_issue(&exact, revision, generation, wall)
                .unwrap();
            issues.push(start.elapsed());
            assert_eq!(issue.parts().3, wall);
        }
    }
    drop(connection);
    for _ in 0..100 {
        let start = Instant::now();
        {
            let mut connection = fixture.open();
            let mut store = Store::attach(&mut connection, namespace()).unwrap();
            assert_eq!(
                store.restore(&exact, revision).unwrap().sequence,
                Some(number(200))
            );
        }
        reopens.push(start.elapsed());
    }
    report("heartbeat_sqlcipher_commit", sequences);
    report("permission_sqlcipher_commit", issues);
    report("sqlcipher_close_reopen_restore", reopens);
    fixture.clean();
}
