//! Exact SQLite callback tests on private, measured rollback images.

use super::*;

pub(crate) fn probe(
    quota: &ReceiverQuota<'_>,
    mode: &str,
    request: RecoveryRequest<'_>,
    recreate: impl FnOnce() -> crate::peer_invitation::RollbackFiles,
) {
    if mode == "stale-generation" {
        let previous = request.generation;
        let key = request.key;
        let charged_other = request.charged_other;
        let next = NonZeroU64::new(previous.get() + 1).unwrap();
        drop(
            quota
                .admit_recovery(RecoveryRequest {
                    generation: next,
                    ..request
                })
                .unwrap(),
        );
        let stale = RecoveryRequest {
            generation: previous,
            files: recreate(),
            charged_other,
            key,
            interrupt_after_write: false,
        };
        assert_eq!(
            quota.admit_recovery(stale).err(),
            Some("dispatch_stage_generation")
        );
        return;
    }

    let mut turn = quota.admit_recovery(request).unwrap();
    let paths = turn
        .quota
        .files
        .each_ref()
        .map(|path| std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap());
    let initial = turn.quota.account.lock().unwrap().lengths;
    turn.enter().unwrap();
    let vfs = quota.dispatch.wrapper;

    // SAFETY: the retained registered VFS owns the callback pointers. Names
    // allocated by SQLite survive the callback calls and all open handles.
    unsafe {
        let name = sql::sqlite3_create_filename(
            paths[0].as_ptr(),
            paths[1].as_ptr(),
            c"".as_ptr(),
            0,
            ptr::null_mut(),
        );
        assert!(!name.is_null());
        let journal = sql::sqlite3_filename_journal(name);
        if mode == "wrong-access" {
            let access = (*vfs).xAccess.unwrap();
            let mut present = 0;
            assert_eq!(
                access(vfs, journal, sql::SQLITE_ACCESS_EXISTS, &mut present),
                sql::SQLITE_OK
            );
            assert_eq!(present, 1);
            assert_eq!(
                access(
                    vfs,
                    c"foreign.db".as_ptr(),
                    sql::SQLITE_ACCESS_EXISTS,
                    &mut present
                ),
                sql::SQLITE_IOERR_ACCESS
            );
        } else {
            file_probe(vfs, name, journal, mode, initial);
        }
        sql::sqlite3_free_filename(name);
    }
}

unsafe fn file_probe(
    vfs: *mut sql::sqlite3_vfs,
    name: *const c_char,
    journal: *const c_char,
    mode: &str,
    initial: [u64; 2],
) {
    let (file_name, kind, access) = match mode {
        "journal-write" | "journal-truncate" => (
            journal,
            sql::SQLITE_OPEN_MAIN_JOURNAL,
            sql::SQLITE_OPEN_READWRITE,
        ),
        "main-readonly" => (name, sql::SQLITE_OPEN_MAIN_DB, sql::SQLITE_OPEN_READONLY),
        "main-cap" | "combined-cap" => (name, sql::SQLITE_OPEN_MAIN_DB, sql::SQLITE_OPEN_READWRITE),
        _ => panic!("unknown recovery guard"),
    };

    // SAFETY: the caller keeps the stock and wrapper registered; the allocated
    // file has the wrapper's required size and is closed before freeing it.
    unsafe {
        let file = sql::sqlite3_malloc64((*vfs).szOsFile as u64).cast::<sql::sqlite3_file>();
        assert!(!file.is_null());
        file.cast::<u8>().write_bytes(0, (*vfs).szOsFile as usize);
        let mut actual = 0;
        let open = (*vfs).xOpen.unwrap()(vfs, file_name, file, kind | access, &mut actual);
        if mode == "main-readonly" {
            assert_eq!(open, sql::SQLITE_CANTOPEN);
            assert!((*file).pMethods.is_null());
        } else {
            assert_eq!(open, sql::SQLITE_OK);
            let methods = &*(*file).pMethods;
            let truncate = methods.xTruncate.unwrap();
            let byte = 0u8;
            let result = match mode {
                "journal-write" => {
                    methods.xWrite.unwrap()(file, ptr::from_ref(&byte).cast(), 1, initial[1] as i64)
                }
                "journal-truncate" => truncate(file, (initial[1] + 1) as i64),
                "main-cap" => truncate(file, (CLOSED_STAGE_LIMIT + 1) as i64),
                "combined-cap" => truncate(file, (initial[0] + 1) as i64),
                _ => unreachable!(),
            };
            let expected = if kind == sql::SQLITE_OPEN_MAIN_JOURNAL {
                sql::SQLITE_READONLY
            } else {
                sql::SQLITE_FULL
            };
            assert_eq!(result, expected);
            assert_eq!(methods.xClose.unwrap()(file), sql::SQLITE_OK);
        }
        sql::sqlite3_free(file.cast());
    }
}
