use super::super::{IdentityMaterial, MdkOwner, RelayConnector};
use super::*;
use crate::dispatcher_fatal::Fatal;
use cgka_traits::storage::MaintenanceStorage;
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    fs,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    rc::Rc,
};
use zeroize::Zeroizing;

#[derive(Clone, Copy)]
enum Fault {
    Cut(Step),
    Panic(Step),
    Error(Step),
}

fn fault(case: &str) -> Option<Fault> {
    use Step::*;
    Some(match case {
        "cut_initialize" => Fault::Cut(Initialize),
        "cut_smoke" => Fault::Cut(AccountSmoke),
        "cut_smoke_close" => Fault::Cut(AccountSmokeClose),
        "cut_account" => Fault::Cut(AccountOpen),
        "cut_journal" => Fault::Cut(JournalSmoke),
        "cut_journal_close" => Fault::Cut(JournalSmokeClose),
        "cut_journal_open" => Fault::Cut(JournalOpen),
        "cut_lifecycle_read" => Fault::Cut(LifecycleRead),
        "cut_lifecycle_write" => Fault::Cut(LifecycleWrite),
        "cut_runtime_open" => Fault::Cut(Runtime),
        "cut_runtime" => Fault::Cut(RuntimeOpened),
        "cut_return" => Fault::Cut(Return),
        "panic_account_reopen" => Fault::Panic(AccountOpen),
        "panic_journal_smoke" => Fault::Panic(JournalSmoke),
        "panic_journal_reopen" => Fault::Panic(JournalOpen),
        "panic_partial" => Fault::Panic(LifecycleRead),
        "panic_binding" => Fault::Panic(Binding),
        "panic_revoked" => Fault::Panic(Revoked),
        "panic_forget" => Fault::Panic(ForgetGroup),
        "panic_orphans" => Fault::Panic(Orphans),
        "panic_maintain" => Fault::Panic(Maintain),
        "panic_routes" => Fault::Panic(Routes),
        "panic_invalidations" => Fault::Panic(Invalidations),
        "healthy_error" => Fault::Error(Orphans),
        _ => return None,
    })
}

pub(crate) fn qualify(
    case: &str,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    connector: RelayConnector,
    admission: &Admission,
    fatal: &Fatal,
    cut: fn(),
) {
    crate::native_preflight::check_with_fence(lease, material, Some(&admission.connection_fence()))
        .unwrap();
    if case == "active_cut" {
        active_cut(lease, material, connector, admission, fatal, cut);
        return;
    }
    if case == "panic_forget" {
        seed_revoked(lease, material, admission);
    }
    let lease_baseline = Arc::strong_count(lease);
    let sockets = socket_count();
    let threads = fs::read_dir("/proc/self/task").unwrap().count();
    let account_path = lease.mdk_database();
    let journal_path = lease.journal_database();
    let orig_journal = evidence(journal_path);
    let rc = Rc::new(RefCell::new(None));
    let reached = Rc::new(Cell::new(false));
    let result = if let Some(fault) = fault(case) {
        let journal_path = journal_path.to_path_buf();
        let observed = rc.clone();
        let hit = reached.clone();
        let mode = StartupMode::Candidate {
            lease: lease.clone(),
            admission: admission.clone(),
            probe: Some(Box::new(move |step| {
                let selected = match fault {
                    Fault::Cut(selected) | Fault::Panic(selected) | Fault::Error(selected) => {
                        selected
                    }
                };
                if step == selected {
                    assert!(!hit.replace(true));
                    *observed.borrow_mut() = Some(evidence(&journal_path));
                    match fault {
                        Fault::Cut(_) => cut(),
                        Fault::Panic(_) => panic!("startup qualification cut"),
                        Fault::Error(_) => return Err(Error::Invalid("startup_probe")),
                    }
                }
                Ok(())
            })),
        };
        catch_unwind(AssertUnwindSafe(|| {
            MdkOwner::construct(lease, material, connector, vec![], mode)
        }))
    } else {
        let mut bad = Zeroizing::new(*material.sqlcipher_root());
        bad[0] ^= 1;
        let wrong = material.with_test_sqlcipher_root(*bad);
        Ok(MdkOwner::open_candidate(
            lease,
            if case == "wrong_key" {
                &wrong
            } else {
                material
            },
            connector,
            vec![],
            admission.clone(),
        ))
    };
    assert_eq!(socket_count(), sockets);
    assert_eq!(fs::read_dir("/proc/self/task").unwrap().count(), threads);

    match (case, result) {
        ("healthy" | "drop" | "close_cut" | "close_failure", Ok(Ok(mut owner))) => {
            assert_eq!(
                owner.status().unwrap().identity,
                material.public_key().to_hex()
            );
            assert!(owner.storage.key_package_lifecycle().unwrap().is_some());
            let survivor = owner.storage.clone();
            match case {
                "healthy" => {
                    owner.finish_candidate_startup().unwrap();
                    assert!(admission.admit());
                    assert_eq!(descriptors(account_path), 0);
                    assert_eq!(descriptors(journal_path), 0);
                }
                "drop" => drop(owner),
                "close_cut" => {
                    cut();
                    assert!(matches!(
                        owner.finish_candidate_startup(),
                        Err(Error::Storage("dispatcher_admission"))
                    ));
                }
                "close_failure" => {
                    owner.journal.close_in_place().unwrap();
                    let custody = owner.startup_custody.take().unwrap();
                    assert!(matches!(
                        custody.close_with(|_| Err(Error::Storage("injected_close"))),
                        Err(Error::Storage("dispatcher_admission"))
                    ));
                    drop(owner);
                    assert_eq!(descriptors(journal_path), 0);
                }
                _ => unreachable!(),
            }
            assert!(survivor.key_package_lifecycle().is_err());
            drop(survivor);
        }
        ("healthy_error", Ok(Err(Error::Invalid("startup_probe"))))
        | ("wrong_key", Ok(Err(Error::Mdk("runtime")))) => {
            assert!(admission.admit());
            assert_eq!(descriptors(account_path), 0);
            assert_eq!(descriptors(journal_path), 0);
        }
        (_, Ok(Err(Error::Storage("dispatcher_admission")))) if case.starts_with("cut_") => {
            assert!(reached.get());
        }
        (_, Err(_)) if case.starts_with("panic_") => {
            assert!(reached.get());
        }
        _ => panic!("unexpected startup disposition: {case}"),
    }

    if matches!(
        case,
        "cut_initialize" | "cut_smoke" | "cut_account" | "panic_account_reopen"
    ) {
        assert_eq!(fatal.is_tripped(), case != "panic_account_reopen");
        assert_eq!(descriptors(account_path), 0);
        assert_eq!(descriptors(journal_path), 0);
        assert_eq!(Arc::strong_count(lease), lease_baseline);
    } else if case.starts_with("cut_")
        || case.starts_with("panic_")
        || matches!(case, "drop" | "close_cut" | "close_failure")
    {
        assert!(fatal.is_tripped());
        assert_eq!(Arc::strong_count(lease), lease_baseline + 1);
        assert!(descriptors(account_path) > 0);
        let journal_live = !matches!(
            case,
            "cut_smoke_close"
                | "cut_journal"
                | "cut_journal_open"
                | "panic_journal_smoke"
                | "panic_journal_reopen"
                | "close_failure"
        );
        assert_eq!(descriptors(journal_path) > 0, journal_live);
    } else {
        assert!(!fatal.is_tripped());
        assert_eq!(Arc::strong_count(lease), lease_baseline);
    }
    match case {
        "wrong_key"
        | "cut_initialize"
        | "cut_smoke"
        | "cut_smoke_close"
        | "cut_account"
        | "cut_journal"
        | "panic_account_reopen"
        | "panic_journal_smoke" => {
            assert_eq!(evidence(journal_path), orig_journal);
        }
        case if case.starts_with("cut_") || case.starts_with("panic_") => {
            assert_eq!(evidence(journal_path), rc.borrow().unwrap());
        }
        _ => {}
    }
}

fn active_cut(
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    connector: RelayConnector,
    admission: &Admission,
    fatal: &Fatal,
    displace: fn(),
) {
    use super::super::DISPATCHER_LOST;
    use std::future::{Future, poll_fn};
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};
    use std::time::{Duration, Instant};

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _entered = runtime.enter();
    let mut owner =
        MdkOwner::open_candidate(lease, material, connector, vec![], admission.clone()).unwrap();
    assert_eq!(
        owner.status().unwrap().identity,
        material.public_key().to_hex()
    );
    let journal = evidence(lease.journal_database());
    let account = evidence(lease.mdk_database());
    let sockets = socket_count();
    let mode = owner.active_mode.clone();
    let polls = Cell::new(0);
    {
        let mut native = pin!(mode.native(poll_fn(|_| {
            polls.set(polls.get() + 1);
            assert!(owner.storage.key_package_lifecycle().unwrap().is_some());
            Poll::<()>::Pending
        })));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(native.as_mut().poll(&mut cx).is_pending());
        assert_eq!(polls.get(), 1);
        displace();
        assert!(
            !fatal.is_tripped(),
            "next admitted poll must detect displacement"
        );
        assert!(matches!(
            native.as_mut().poll(&mut cx),
            Poll::Ready(Err(Error::Storage(DISPATCHER_LOST)))
        ));
    }
    assert!(fatal.is_tripped());
    assert_eq!(polls.get(), 1);
    let deadline = Instant::now() + Duration::from_secs(1);
    runtime.block_on(async {
        assert!(matches!(
            owner.activate(deadline).await,
            Err(Error::Storage(DISPATCHER_LOST))
        ));
        assert!(matches!(
            owner.tick(deadline).await,
            Err(Error::Storage(DISPATCHER_LOST))
        ));
    });
    assert!(matches!(
        owner.status(),
        Err(Error::Storage(DISPATCHER_LOST))
    ));
    assert!(matches!(
        owner.command_count_exit_ready(),
        Err(Error::Storage(DISPATCHER_LOST))
    ));
    assert!(!owner.active && owner.forwarder.is_none());
    assert_eq!(socket_count(), sockets);
    assert_eq!(evidence(lease.journal_database()), journal);
    assert_eq!(evidence(lease.mdk_database()), account);
    // The active leaf does not retire storage after displacement. The existing
    // startup custody drops into the registered holder pending physical stop.
    assert!(descriptors(lease.mdk_database()) > 0);
}

fn seed_revoked(lease: &ProfileLease, material: &IdentityMaterial, admission: &Admission) {
    let mut key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), material.sqlcipher_root())
        .expand(b"journal", key.as_mut())
        .unwrap();
    let mut journal = Journal::open_with_fence(
        lease.journal_database(),
        &key,
        &material.public_key().to_hex(),
        Some(&admission.connection_fence()),
    )
    .unwrap();
    let group = [1; 32];
    journal
        .accept_conversation(&group, &"22".repeat(16), &"44".repeat(32))
        .unwrap();
    journal.reject_group_at(&group, None, 1).unwrap();
    journal.close().unwrap();
}

fn fd_targets() -> impl Iterator<Item = PathBuf> {
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(|entry| fs::read_link(entry.ok()?.path()).ok())
}

fn descriptors(path: &Path) -> usize {
    fd_targets().filter(|target| target == path).count()
}

fn socket_count() -> usize {
    fd_targets()
        .filter(|target| target.to_string_lossy().starts_with("socket:["))
        .count()
}

fn evidence(path: &Path) -> [Option<[u8; 32]>; 3] {
    ["", "-wal", "-shm"].map(|suffix| {
        let mut file = path.as_os_str().to_owned();
        file.push(suffix);
        match fs::read(file) {
            Ok(bytes) => Some(Sha256::digest(bytes).into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("startup evidence read: {error}"),
        }
    })
}
