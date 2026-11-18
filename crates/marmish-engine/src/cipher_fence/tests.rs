use super::*;
use std::cell::Cell;
use std::sync::atomic::AtomicUsize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Boundary {
    BeforeOpen,
    Opened,
    OpenFailed,
    Partial,
    RawClose,
    OwnedClose,
    Hardened,
    PrefixRead,
    Reconcile,
}

thread_local! {
    static LAST: Cell<Option<Boundary>> = const { Cell::new(None) };
    static OPENED: Cell<usize> = const { Cell::new(0) };
    static CLOSES: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn mark(boundary: Boundary) {
    LAST.set(Some(boundary));
    match boundary {
        Boundary::Opened | Boundary::OpenFailed | Boundary::Partial => {
            OPENED.set(OPENED.get() + 1);
        }
        Boundary::RawClose | Boundary::OwnedClose => CLOSES.set(CLOSES.get() + 1),
        _ => {}
    }
}

pub(crate) fn counts() -> (usize, usize) {
    (OPENED.get(), CLOSES.get())
}

pub(crate) fn cut_at(boundary: Boundary, visit: usize) -> (ConnectionFence, Arc<AtomicUsize>) {
    LAST.set(None);
    let checks = AtomicUsize::new(0);
    let notified = Arc::new(AtomicUsize::new(0));
    let notify = notified.clone();
    (
        ConnectionFence::new(
            move || LAST.get() != Some(boundary) || checks.fetch_add(1, Ordering::Relaxed) < visit,
            move || {
                notify.fetch_add(1, Ordering::Relaxed);
            },
        ),
        notified,
    )
}

/// All callers of this helper deliberately allow real SQLite ownership to
/// survive until the child exits. The parent owns and removes its test files.
pub(crate) fn child(selector: &str) -> Option<std::path::PathBuf> {
    const DIRECTORY: &str = "MARMISH_CIPHER_FENCE_TEST_DIRECTORY";
    if let Some(path) = std::env::var_os(DIRECTORY) {
        return Some(path.into());
    }
    let root = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", selector, "--test-threads=1"])
        .env(DIRECTORY, root.path())
        .env("TMPDIR", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    None
}

#[test]
fn raw_open_error_owns_nonnull_handle_and_checks_before_close() {
    let Some(dir) = child(
        concat!(
            module_path!(),
            "::raw_open_error_owns_nonnull_handle_and_checks_before_close"
        )
        .trim_start_matches("marmish_engine::"),
    ) else {
        return;
    };
    let path = dir.join("missing");
    let (healthy, _) = cut_at(Boundary::Partial, usize::MAX);
    assert!(matches!(
        Owned::guarded(&path, &healthy),
        Err(EngineError::StorageFailure)
    ));
    assert!(!path.exists());
    assert_eq!(counts(), (1, 1));

    let (refused, notified) = cut_at(Boundary::Partial, 0);
    assert!(matches!(
        Owned::guarded(&path, &refused),
        Err(EngineError::StorageAdmissionRefused)
    ));
    assert_eq!(counts(), (2, 1));
    assert_eq!(notified.load(Ordering::Relaxed), 0);
}

unsafe fn leave_unfinalized(handle: *mut ffi::sqlite3) {
    let mut statement = ptr::null_mut();
    // SAFETY: handle is live; the child intentionally retains this statement
    // to force a real SQLITE_BUSY on close.
    assert_eq!(
        unsafe {
            ffi::sqlite3_prepare_v2(
                handle,
                c"SELECT 1".as_ptr(),
                -1,
                &mut statement,
                ptr::null_mut(),
            )
        },
        ffi::SQLITE_OK
    );
    assert!(!statement.is_null());
}

#[test]
fn actual_close_failures_park_raw_and_converted_owners_once() {
    let Some(dir) = child(
        concat!(
            module_path!(),
            "::actual_close_failures_park_raw_and_converted_owners_once"
        )
        .trim_start_matches("marmish_engine::"),
    ) else {
        return;
    };
    for (name, raw_owner, unwind) in [
        ("raw", true, false),
        ("converted", false, false),
        ("unwinding", false, true),
    ] {
        let path = dir.join(name);
        std::fs::File::create(&path).unwrap();
        let (fence, notified) = cut_at(Boundary::Opened, usize::MAX);
        let before = counts();
        if raw_owner {
            let mut owner = Raw {
                handle: ptr::null_mut(),
                fence: &fence,
            };
            crate::initialize_storage().unwrap();
            let path = CString::new(path.as_os_str().as_bytes()).unwrap();
            // SAFETY: the raw guard owns the out slot, and no rusqlite
            // owner is created for this independent handle.
            assert_eq!(
                unsafe {
                    ffi::sqlite3_open_v2(
                        path.as_ptr(),
                        &mut owner.handle,
                        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_FULLMUTEX,
                        ptr::null(),
                    )
                },
                ffi::SQLITE_OK
            );
            mark(Boundary::Opened);
            // SAFETY: owner has the actual open handle.
            unsafe { leave_unfinalized(owner.handle) };
            assert!(matches!(
                owner.close(),
                Err(EngineError::StorageAdmissionRefused)
            ));
        } else {
            let db = Owned::guarded(&path, &fence).unwrap();
            // SAFETY: db owns the actual open handle.
            unsafe { leave_unfinalized(db.handle()) };
            if unwind {
                assert!(
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                        let _held_during_unwind = db;
                        panic!("fixture unwinds past the exact SQLite owner");
                    }))
                    .is_err()
                );
            } else {
                assert!(matches!(
                    db.close(),
                    Err(EngineError::StorageAdmissionRefused)
                ));
            }
        }
        assert_eq!(counts(), (before.0 + 1, before.1 + 1));
        assert_eq!(notified.load(Ordering::Relaxed), 1);
        assert!(matches!(
            Owned::guarded(&path, &fence),
            Err(EngineError::StorageAdmissionRefused)
        ));
        assert_eq!(counts(), (before.0 + 1, before.1 + 1));
        assert_eq!(notified.load(Ordering::Relaxed), 1);
    }
}
