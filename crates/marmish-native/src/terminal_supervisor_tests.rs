use super::*;
use crate::identity::BackendChoice;
use crate::native_setup::{SetupOptions, SetupRole};
use hkdf::Hkdf;
use nostr::key::Keys;
use nostr::nips::nip19::ToBech32;
use sha2::{Digest as _, Sha256};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::process::{Child, Command, Stdio};
use std::thread;
use zeroize::Zeroizing;

const WAIT: Duration = Duration::from_secs(4);
const PROFILE_ID: &str = "00112233445566778899aabbccddeeff";

struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn core_tests_terminal_supervisor_management_grammar() {
    assert_eq!(tmux::version(b"tmux 3.3a\n"), Some((3, 3)));
    assert_eq!(tmux::version(b"tmux 3.4\n"), Some((3, 4)));
    for invalid in [
        "tmux 3.2a\n",
        "tmux 03.4\n",
        "tmux 3.4\r\n",
        "tmux 3.4\nextra",
        "tmux 3.4aa\n",
    ] {
        assert_eq!(tmux::version(invalid.as_bytes()), None);
    }
    assert!(tmux::validate_length(Path::new(&"x".repeat(103))).is_ok());
    assert!(matches!(
        tmux::validate_length(Path::new(&"x".repeat(104))),
        Err(Error::Invalid("terminal_socket_path_length"))
    ));
    let marker = [1; 16];
    let raw = format!("shell|$4294967295|{}\n", hex::encode(marker));
    assert_eq!(
        tmux::parse_inventory(raw.as_bytes(), marker),
        Some(u32::MAX)
    );
    assert!(tmux::parse_inventory(raw.as_bytes(), [2; 16]).is_none());
    assert!(tmux::parse_inventory(raw.repeat(2).as_bytes(), marker).is_none());
    assert!(tmux::parse_inventory(raw.replace("shell", "shell2").as_bytes(), marker).is_none());
}

#[test]
fn core_tests_terminal_supervisor_encrypted_registry() {
    let profile = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let root = profile.path();
    let account = [3; 32];
    let key = [4; 32];
    let terminal = store::derive_key(PROFILE_ID, &key).unwrap();
    let hkdf = Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), &key);
    for label in [b"mdk".as_slice(), b"journal"] {
        let mut existing = Zeroizing::new([0; 32]);
        hkdf.expand(label, existing.as_mut()).unwrap();
        assert!(*terminal != *existing);
    }
    let command = crate::command_identity::derive(PROFILE_ID, account, &key).unwrap();
    assert!(*terminal != *command.master);
    let mut db = Registry::open_at(root, PROFILE_ID, account, &key).unwrap();
    assert_eq!(db.allocate().unwrap(), 1);
    for idx in 0..MAX_SLOTS as u8 {
        db.prepare(prepared(idx, account, false)).unwrap();
    }
    assert!(matches!(
        db.prepare(prepared(17, account, false)),
        Err(Error::Invalid("terminal_capacity"))
    ));
    assert_eq!(db.slots().unwrap().len(), MAX_SLOTS);
    assert!(
        db.slots()
            .unwrap()
            .iter()
            .all(|slot| slot.state == State::Prepared)
    );
    drop(db);
    assert!(Registry::open_at(root, PROFILE_ID, account, &[5; 32]).is_err());
    assert!(Registry::open_at(root, PROFILE_ID, [5; 32], &key).is_err());
    assert!(Registry::open_at(root, "102132435465768798a9bacbdcedfe0f", account, &key).is_err());
    let mut db = Registry::open_at(root, PROFILE_ID, account, &key).unwrap();
    assert_eq!(db.allocate().unwrap(), 2);
    let ciphertext = fs::read(root.join("terminal.sqlite3")).unwrap();
    assert!(!ciphertext.starts_with(b"SQLite format 3"));
    assert!(!ciphertext.windows(32).any(|window| window == account));
    db.connection
        .execute("UPDATE binding SET generation=?1", [u64::MAX.to_be_bytes()])
        .unwrap();
    assert!(matches!(
        db.allocate(),
        Err(Error::Storage("terminal_registry_generation"))
    ));
    db.connection
        .pragma_update(None, "user_version", 2)
        .unwrap();
    drop(db);
    assert!(matches!(
        Registry::open_at(root, PROFILE_ID, account, &key),
        Err(Error::Storage("terminal_registry_format"))
    ));
    symlink(
        root.join("terminal.sqlite3"),
        root.join("terminal.sqlite3-journal"),
    )
    .unwrap();
    assert!(Registry::open_at(root, PROFILE_ID, account, &key).is_err());
}

fn prepared(index: u8, account: Account, default: bool) -> Slot {
    Slot {
        id: SlotId([index; 16]),
        account,
        default,
        marker: [index + 1; 16],
        state: State::Prepared,
    }
}

#[test]
#[ignore = "uses an interrupted SQLCipher child and a private disposable registry"]
fn terminal_supervisor_registry_interrupted_final_commit() {
    const CHILD_PATH: &str = "MARMISH_TEST_TERMINAL_REGISTRY_CHILD";
    const ACCOUNT: Account = [3; 32];
    const KEY: [u8; 32] = [4; 32];
    let intent = prepared(1, ACCOUNT, true);
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let root = Path::new(&path);
        assert!(root.join("terminal.sqlite3").is_file());
        let mut registry = Registry::open_at(root, PROFILE_ID, ACCOUNT, &KEY).unwrap();
        let transaction = registry.connection.transaction().unwrap();
        assert_eq!(
            transaction
                .execute(
                    "UPDATE slot SET state=1,native_id=0 WHERE slot=?1 AND state=0",
                    [intent.id.0],
                )
                .unwrap(),
            1
        );
        transaction.cache_flush().unwrap();
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(root.join("transaction-active"))
            .unwrap();
        thread::sleep(WAIT * 3);
        panic!("fixture parent did not interrupt the transaction");
    }

    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let mut registry = Registry::open_at(root.path(), PROFILE_ID, ACCOUNT, &KEY).unwrap();
    registry.prepare(intent).unwrap();
    drop(registry);
    let mut child = KillOnDrop(
        Command::new(std::env::current_exe().unwrap())
            .env_clear()
            .env(CHILD_PATH, root.path())
            .args([
                "--ignored",
                "--exact",
                concat!(
                    "terminal_backend::supervisor::tests::",
                    "terminal_supervisor_registry_interrupted_final_commit"
                ),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let until = Instant::now() + WAIT;
    while !root.path().join("transaction-active").exists() {
        assert!(Instant::now() < until && child.0.try_wait().unwrap().is_none());
        thread::sleep(Duration::from_millis(5));
    }
    let journal = root.path().join("terminal.sqlite3-journal");
    let journal_bytes = fs::read(&journal).unwrap();
    assert!(journal_bytes.starts_with(&[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]));
    drop(child);
    let registry = Registry::open_at(root.path(), PROFILE_ID, ACCOUNT, &KEY).unwrap();
    let rows = registry.slots().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].id == intent.id && rows[0].state == State::Prepared);
    assert!(!journal.exists());
}

struct Fixture {
    base: tempfile::TempDir,
    daemon: PathBuf,
    tmux: PathBuf,
    accounts: [Account; 2],
}

impl Fixture {
    async fn new() -> Self {
        let daemon = Path::new(
            &std::env::var_os("MARMISH_TEST_DAEMON")
                .expect("set MARMISH_TEST_DAEMON to the built marmishd"),
        )
        .canonicalize()
        .unwrap();
        let tmux = PathBuf::from(
            std::env::var_os("MARMISH_TEST_TMUX").unwrap_or_else(|| "/usr/bin/tmux".into()),
        );
        let base = tempfile::Builder::new()
            .prefix("ms-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(Path::new("/tmp").canonicalize().unwrap())
            .unwrap();
        let keys = [Keys::generate(), Keys::generate()];
        let accounts = keys.each_ref().map(|key| key.public_key().to_bytes());
        let options = SetupOptions {
            state_dir: base.path().into(),
            identity: "t".into(),
            backend: BackendChoice::File,
            helper: PathBuf::new(),
            interactive: false,
            role: Some(SetupRole::Server),
            relays: Some(vec!["wss://127.0.0.1:9/".into()]),
            tor: Some("off".into()),
            grants: keys
                .iter()
                .map(|key| key.public_key().to_bech32().unwrap())
                .collect(),
            identifier: None,
            revoke: vec![],
            trust: vec![],
            untrust: vec![],
            update: false,
            no_grants: false,
            debug: false,
        };
        crate::native_setup::run(options).await.unwrap();
        let result = Self {
            base,
            daemon,
            tmux,
            accounts,
        };
        result.set_terminal_config(&accounts);
        result
    }

    fn set_terminal_config(&self, accounts: &[Account]) {
        let root = self.base.path().join("t");
        let path = root.join("config.json");
        let mut config: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        config["terminal"] =
            serde_json::json!({"clients": accounts.iter().map(hex::encode).collect::<Vec<_>>()});
        let lease = ProfileLease::acquire_ready(self.base.path(), "t").unwrap();
        let mut file = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&path)
            .unwrap();
        file.write_all(&serde_json::to_vec(&config).unwrap())
            .unwrap();
        file.sync_all().unwrap();
        drop(lease);
    }

    fn identity(&self) -> (ProfileLease, IdentityMaterial) {
        let mut profile = ProfileLease::acquire_ready(self.base.path(), "t").unwrap();
        let material = profile
            .prepare(BackendChoice::File, Path::new(""), Instant::now() + WAIT)
            .unwrap();
        (profile, material)
    }

    fn supervisor<'a>(
        &self,
        profile: &'a ProfileLease,
        material: &IdentityMaterial,
    ) -> Supervisor<'a> {
        Supervisor::open(profile, material, &self.daemon, &self.tmux).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.base.path().join("t/t");
        if let Ok(entries) = fs::read_dir(root) {
            for directory in entries.flatten() {
                let path = directory.path().join("s");
                if backend::paths::checked_uid()
                    .is_ok_and(|uid| backend::paths::socket(&path, uid).is_ok())
                {
                    let child = Command::new(&self.tmux)
                        .env_clear()
                        .args(["-N", "-S"])
                        .arg(path)
                        .args(["-f", "/dev/null", "kill-server"])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn();
                    if let Ok(mut child) = child {
                        let until = Instant::now() + Duration::from_secs(1);
                        while !matches!(child.try_wait(), Ok(Some(_))) {
                            if Instant::now() >= until {
                                let _ = child.kill();
                                let _ = child.wait();
                                break;
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
            }
        }
    }
}

fn attached(value: Admission) -> (SlotId, Claim) {
    match value {
        Admission::Attached { slot, claim } => (slot, claim),
        Admission::Busy { .. } => panic!("unexpected occupied writer"),
    }
}

fn bytes(
    supervisor: &mut Supervisor,
    owner: Account,
    slot: SlotId,
    claim: Claim,
    mut input: &[u8],
) {
    let until = Instant::now() + WAIT;
    loop {
        assert!(Instant::now() < until, "PTY input deadline");
        match supervisor.write(owner, slot, claim, input).unwrap() {
            WriteStatus::Complete => break,
            WriteStatus::Partial(count) => input = &input[count..],
            WriteStatus::WouldBlock => thread::sleep(Duration::from_millis(5)),
            WriteStatus::Hangup => panic!("PTY closed"),
        }
    }
}

fn marker(
    supervisor: &mut Supervisor,
    owner: Account,
    slot: SlotId,
    claim: Claim,
    expected: &[u8],
) {
    let until = Instant::now() + WAIT;
    let mut seen = Vec::new();
    loop {
        assert!(Instant::now() < until, "PTY output marker deadline");
        let mut buffer = [0; 4096];
        match supervisor.read(owner, slot, claim, &mut buffer).unwrap() {
            ReadStatus::Data(count) => seen.extend_from_slice(&buffer[..count]),
            ReadStatus::WouldBlock => thread::sleep(Duration::from_millis(5)),
            ReadStatus::Hangup => panic!("PTY closed"),
        }
        if seen.windows(expected.len()).any(|bytes| bytes == expected) {
            break;
        }
        assert!(seen.len() <= 65536, "PTY output evidence bounds");
    }
}

fn query(supervisor: &mut Supervisor, slot: SlotId, args: &[&str]) -> Vec<u8> {
    let mut command = supervisor.tmux.control(slot).unwrap();
    command.args(args);
    supervisor.tmux.run(command).unwrap()
}

fn dims(supervisor: &mut Supervisor, slot: SlotId) -> Vec<u8> {
    query(
        supervisor,
        slot,
        &[
            "display-message",
            "-p",
            "-t",
            "shell:0.0",
            "#{pane_width}x#{pane_height}",
        ],
    )
}

fn ordinary_stores(profile: &ProfileLease) -> [Option<[u8; 32]>; 3] {
    let commands = profile.root().join("commands/engine.sqlite3");
    [
        profile.mdk_database(),
        profile.journal_database(),
        &commands,
    ]
    .map(|path| match fs::read(path) {
        Ok(bytes) => Some(Sha256::digest(bytes).into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => panic!("ordinary store snapshot failed: {error:?}"),
    })
}

#[tokio::test]
#[ignore = "requires checked daemon and tmux binaries on a nonroot Unix host"]
async fn terminal_supervisor_native_tmux_and_resume() {
    let fixture = Fixture::new().await;
    let [account, other] = fixture.accounts;
    let (profile, material) = fixture.identity();
    let ordinary = ordinary_stores(&profile);
    assert!(ordinary.iter().any(Option::is_some));
    let mut supervisor = fixture.supervisor(&profile, &material);
    let start = Instant::now();
    let (slot, writer) = attached(
        supervisor
            .attach(account, Selection::Default, Mode::Writer, 24, 80)
            .unwrap(),
    );
    let launch = start.elapsed();
    bytes(
        &mut supervisor,
        account,
        slot,
        writer,
        b"MARMISH_TEST_RESUME=verified; printf 'MARMISH_TEST_%s\\n' ready\r",
    );
    marker(
        &mut supervisor,
        account,
        slot,
        writer,
        b"MARMISH_TEST_ready",
    );
    let first_render = start.elapsed();
    assert!(
        matches!(supervisor.attach(account, Selection::Default, Mode::Writer, 24, 80), Ok(Admission::Busy { writer: expected, slot: same }) if expected == writer && same == slot)
    );
    assert!(matches!(
        supervisor.attach(other, Selection::Existing(slot), Mode::Writer, 24, 80),
        Err(Error::Invalid("terminal_slot_missing"))
    ));
    for result in [
        supervisor
            .write(other, slot, writer, b"discard")
            .map(|_| ()),
        supervisor.resize(other, slot, writer, 31, 93),
        supervisor
            .read(other, slot, writer, &mut [0; 1])
            .map(|_| ()),
        supervisor.detach(other, slot, writer),
    ] {
        assert!(matches!(
            result,
            Err(Error::Invalid("terminal_slot_missing"))
        ));
    }
    let start = Instant::now();
    let (_, observer) = attached(
        supervisor
            .attach(account, Selection::Existing(slot), Mode::Observe, 61, 121)
            .unwrap(),
    );
    marker(
        &mut supervisor,
        account,
        slot,
        observer,
        b"MARMISH_TEST_ready",
    );
    let observe = start.elapsed();
    assert_eq!(dims(&mut supervisor, slot), b"80x24\n");
    assert!(matches!(
        supervisor.write(account, slot, observer, b"discard"),
        Err(Error::Invalid("terminal_attachment_read_only"))
    ));
    assert!(matches!(
        supervisor.resize(account, slot, observer, 72, 134),
        Err(Error::Invalid("terminal_attachment_read_only"))
    ));
    assert_eq!(dims(&mut supervisor, slot), b"80x24\n");
    supervisor.resize(account, slot, writer, 31, 93).unwrap();
    let until = Instant::now() + WAIT;
    while dims(&mut supervisor, slot) != b"93x31\n" {
        assert!(Instant::now() < until, "PTY geometry deadline");
        thread::sleep(Duration::from_millis(5));
    }
    let wrong = Claim {
        random: [9; 16],
        ..writer
    };
    assert!(matches!(
        supervisor.attach(
            account,
            Selection::Existing(slot),
            Mode::Takeover(wrong),
            24,
            80
        ),
        Err(Error::Invalid("terminal_claim_stale"))
    ));
    assert!(supervisor.write(account, slot, writer, b"").is_ok());
    let (_, replacement) = attached(
        supervisor
            .attach(
                account,
                Selection::Existing(slot),
                Mode::Takeover(writer),
                24,
                80,
            )
            .unwrap(),
    );
    assert!(replacement.generation > observer.generation);
    assert!(matches!(
        supervisor.write(account, slot, writer, b"discard"),
        Err(Error::Invalid("terminal_claim_stale"))
    ));
    assert!(matches!(
        supervisor.resize(account, slot, writer, 31, 93),
        Err(Error::Invalid("terminal_claim_stale"))
    ));
    assert!(matches!(
        supervisor.read(account, slot, observer, &mut [0; 1]),
        Err(Error::Invalid("terminal_claim_stale"))
    ));
    assert!(matches!(
        supervisor.detach(account, slot, writer),
        Err(Error::Invalid("terminal_claim_stale"))
    ));
    let (new_slot, new_writer) = attached(
        supervisor
            .attach(account, Selection::New, Mode::Writer, 24, 80)
            .unwrap(),
    );
    assert!(new_slot != slot);
    bytes(
        &mut supervisor,
        account,
        new_slot,
        new_writer,
        b"printf 'MARMISH_NEW_%s\\n' \"${MARMISH_TEST_RESUME:-empty}\"\r",
    );
    marker(
        &mut supervisor,
        account,
        new_slot,
        new_writer,
        b"MARMISH_NEW_empty",
    );
    supervisor.detach(account, new_slot, new_writer).unwrap();
    let start = Instant::now();
    supervisor.detach(account, slot, replacement).unwrap();
    let detach = start.elapsed();
    drop(supervisor);
    drop(material);
    drop(profile);
    let (profile, material) = fixture.identity();
    let mut supervisor = fixture.supervisor(&profile, &material);
    let start = Instant::now();
    let (same, resumed) = attached(
        supervisor
            .attach(account, Selection::Default, Mode::Writer, 24, 80)
            .unwrap(),
    );
    assert!(same == slot && resumed.generation > new_writer.generation);
    bytes(
        &mut supervisor,
        account,
        slot,
        resumed,
        b"printf 'MARMISH_OLD_%s\\n' \"$MARMISH_TEST_RESUME\"\r",
    );
    marker(
        &mut supervisor,
        account,
        slot,
        resumed,
        b"MARMISH_OLD_verified",
    );
    let resume = start.elapsed();
    let mut samples = Vec::new();
    for index in 0..30 {
        let start = Instant::now();
        let command = format!("printf 'MARMISH_ECHO_%s_END\\n' {index}\r");
        let expected = format!("MARMISH_ECHO_{index}_END");
        bytes(&mut supervisor, account, slot, resumed, command.as_bytes());
        marker(&mut supervisor, account, slot, resumed, expected.as_bytes());
        samples.push(start.elapsed());
    }
    samples.sort_unstable();
    supervisor.detach(account, slot, resumed).unwrap();
    let paths = [
        supervisor.tmux.socket(slot),
        supervisor.tmux.socket(new_slot),
    ];
    drop(supervisor);
    profile.cleanup_runtime().unwrap();
    drop(profile);
    drop(material);
    fixture.set_terminal_config(&[other]);
    let (profile, material) = fixture.identity();
    let mut supervisor = fixture.supervisor(&profile, &material);
    assert!(matches!(
        supervisor.attach(account, Selection::Default, Mode::Writer, 24, 80),
        Err(Error::Invalid("terminal_grant_denied"))
    ));
    assert!(paths.iter().all(|path| !path.exists()));
    assert!(supervisor.registry.slots().unwrap().is_empty());
    assert!(ordinary_stores(&profile) == ordinary);
    profile.cleanup_runtime().unwrap();
    println!(
        "terminal_supervisor samples=30 p50_us={} p95_us={} launch_us={} first_render_us={} observer_us={} detach_us={} resume_us={}",
        samples[14].as_micros(),
        samples[28].as_micros(),
        launch.as_micros(),
        first_render.as_micros(),
        observe.as_micros(),
        detach.as_micros(),
        resume.as_micros()
    );
}

#[tokio::test]
#[ignore = "requires checked daemon and tmux binaries on a nonroot Unix host"]
async fn terminal_supervisor_native_restart_quarantines_ambiguous_intents() {
    let fixture = Fixture::new().await;
    let [account, _] = fixture.accounts;
    let (profile, material) = fixture.identity();
    let mut supervisor = fixture.supervisor(&profile, &material);
    let missing = prepared(1, account, false);
    let marked = prepared(2, account, false);
    let unmarked = prepared(3, account, false);
    let foreign = prepared(4, account, false);
    let stale = prepared(5, account, false);
    let retiring = prepared(6, account, true);
    for slot in [missing, marked, unmarked, foreign, stale, retiring] {
        supervisor.registry.prepare(slot).unwrap();
    }
    supervisor.registry.retiring(retiring).unwrap();
    let native = supervisor.tmux.create(marked, 24, 80).unwrap();
    let different = supervisor.tmux.create(unmarked, 24, 80).unwrap();
    assert!(matches!(
        supervisor.tmux.terminate(marked.id, different),
        Err(Error::Invalid("terminal_slot_unavailable"))
    ));
    assert!(!tmux::connection_refused(&supervisor.tmux.socket(marked.id)).unwrap());
    query(
        &mut supervisor,
        unmarked.id,
        &["set-option", "-u", "-t", "$0", "@marmish-proof"],
    );
    supervisor
        .tmux
        .create(
            Slot {
                marker: [42; 16],
                ..foreign
            },
            24,
            80,
        )
        .unwrap();
    let stale_socket = supervisor.tmux.socket(stale.id);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(stale_socket.parent().unwrap())
        .unwrap();
    drop(std::os::unix::net::UnixListener::bind(&stale_socket).unwrap());
    use std::os::unix::fs::MetadataExt as _;
    let stale_inode = fs::symlink_metadata(&stale_socket).unwrap().ino();
    drop(supervisor);
    drop(profile);
    drop(material);

    let (profile, material) = fixture.identity();
    let mut supervisor = fixture.supervisor(&profile, &material);
    let rows = supervisor.registry.slots().unwrap();
    assert!(
        rows.iter()
            .any(|slot| slot.id == marked.id && slot.state == State::Ready(native.session))
    );
    for expected in [missing, unmarked, foreign, stale] {
        assert!(
            rows.iter()
                .any(|slot| slot.id == expected.id && slot.state == State::Prepared)
        );
        assert!(matches!(
            supervisor.attach(
                account,
                Selection::Existing(expected.id),
                Mode::Writer,
                24,
                80
            ),
            Err(Error::Invalid("terminal_slot_unavailable"))
        ));
    }
    assert_eq!(
        fs::symlink_metadata(&stale_socket).unwrap().ino(),
        stale_inode
    );
    assert!(!supervisor.tmux.socket(missing.id).exists());
    assert!(
        rows.iter()
            .any(|row| row.id == retiring.id && row.state == State::Retiring(None))
    );
    assert!(matches!(
        supervisor.attach(account, Selection::Default, Mode::Writer, 24, 80),
        Err(Error::Invalid("terminal_slot_unavailable"))
    ));
    let (id, claim) = attached(
        supervisor
            .attach(
                account,
                Selection::Existing(marked.id),
                Mode::Writer,
                24,
                80,
            )
            .unwrap(),
    );
    bytes(
        &mut supervisor,
        account,
        id,
        claim,
        b"printf 'MARMISH_ADOPT_%s\\n' ready\r",
    );
    marker(&mut supervisor, account, id, claim, b"MARMISH_ADOPT_ready");
    supervisor.detach(account, id, claim).unwrap();
    profile.cleanup_runtime().unwrap();
}

#[tokio::test]
#[ignore = "requires checked daemon and tmux binaries on a nonroot Unix host"]
async fn terminal_supervisor_native_writer_exit_and_observer_cap() {
    let fixture = Fixture::new().await;
    let [account, _] = fixture.accounts;
    let (profile, material) = fixture.identity();
    let mut supervisor = fixture.supervisor(&profile, &material);
    let (slot, writer) = attached(
        supervisor
            .attach(account, Selection::Default, Mode::Writer, 24, 80)
            .unwrap(),
    );
    let observers: [Claim; 3] = std::array::from_fn(|_| {
        attached(
            supervisor
                .attach(account, Selection::Existing(slot), Mode::Observe, 25, 81)
                .unwrap(),
        )
        .1
    });
    assert!(matches!(
        supervisor.attach(account, Selection::Existing(slot), Mode::Observe, 25, 81),
        Err(Error::Invalid("terminal_capacity"))
    ));
    // The exact writer can exit without the owner initiating detach, which
    // must fence even the observer PTYs that remain connected to the pane.
    let clients = query(
        &mut supervisor,
        slot,
        &["list-clients", "-F", "#{client_readonly}|#{client_tty}"],
    );
    let writer_tty = std::str::from_utf8(&clients)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("0|"))
        .collect::<Vec<_>>();
    assert_eq!(writer_tty.len(), 1);
    query(
        &mut supervisor,
        slot,
        &["detach-client", "-t", writer_tty[0]],
    );
    let until = Instant::now() + WAIT;
    let mut buffer = [0; 4096];
    loop {
        assert!(Instant::now() < until, "external writer exit deadline");
        match supervisor.read(account, slot, writer, &mut buffer) {
            Err(Error::Invalid("terminal_claim_stale")) => break,
            Ok(_) => thread::sleep(Duration::from_millis(5)),
            Err(error) => panic!("unexpected supervisor result: {error:?}"),
        }
    }
    for observer in observers {
        assert!(matches!(
            supervisor.read(account, slot, observer, &mut buffer),
            Err(Error::Invalid("terminal_claim_stale"))
        ));
    }
    assert!(matches!(
        supervisor.attach(account, Selection::Existing(slot), Mode::Observe, 25, 81),
        Err(Error::Invalid("terminal_claim_stale"))
    ));
    let (_, replacement) = attached(
        supervisor
            .attach(account, Selection::Existing(slot), Mode::Writer, 24, 80)
            .unwrap(),
    );
    supervisor.detach(account, slot, replacement).unwrap();
    profile.cleanup_runtime().unwrap();
}

#[tokio::test]
#[ignore = "requires checked daemon and tmux binaries on a nonroot Unix host"]
async fn terminal_supervisor_native_quarantine_caps_before_launch() {
    let fixture = Fixture::new().await;
    let [account, _] = fixture.accounts;
    let (profile, material) = fixture.identity();
    let mut registry = Registry::open(&profile, &material).unwrap();
    for entry in 0..MAX_SLOTS as u8 {
        registry.prepare(prepared(entry, account, false)).unwrap();
    }
    drop(registry);
    let mut supervisor = fixture.supervisor(&profile, &material);
    let t = profile.root().join("t");
    for selection in [Selection::Default, Selection::New] {
        assert!(matches!(
            supervisor.attach(account, selection, Mode::Writer, 24, 80),
            Err(Error::Invalid("terminal_capacity"))
        ));
    }
    assert_eq!(fs::read_dir(t).unwrap().count(), 0);
    assert!(supervisor.active.is_empty());
    assert_eq!(supervisor.registry.allocate().unwrap(), 1);
    profile.cleanup_runtime().unwrap();
}
