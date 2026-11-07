//! Same-UID context is a consistency check, not authentication. These direct
//! reader tests do not claim hidden CLI missing-locator dispatch coverage.
use super::*;
use std::{
    os::fd::{FromRawFd, OwnedFd},
    sync::mpsc,
};

struct PinnedLocator {
    fd: i32,
    identity: (libc::dev_t, libc::ino_t),
    _pin: File,
}
fn identity(fd: i32) -> Option<(libc::dev_t, libc::ino_t)> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    Some((stat.st_dev, stat.st_ino))
}
fn relocate(file: File) -> File {
    let fd = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 4) };
    assert!(fd >= 4);
    unsafe { File::from_raw_fd(fd) }
}
impl PinnedLocator {
    fn new(file: &File) -> Self {
        let fd = file.as_raw_fd();
        let duplicate = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 4) };
        assert!(duplicate >= 4);
        Self {
            fd,
            identity: identity(fd).unwrap(),
            _pin: unsafe { File::from_raw_fd(duplicate) },
        }
    }
    fn released(&self) {
        assert_ne!(identity(self.fd), Some(self.identity));
    }
}
fn valid_context() -> HelperContext {
    let metadata = crate::native_manifest::compiled_release_metadata();
    let mut commit = [0; 20];
    hex::decode_to_slice(metadata.commit, &mut commit).unwrap();
    HelperContext {
        context_version: protocol::CONTEXT_VERSION,
        helper_protocol: protocol::VERSION,
        client: [1; 32],
        server: [2; 32],
        group: vec![3; 32],
        session: [4; 16],
        stream: [5; 16],
        transfer: [6; 32],
        direction: Direction::Put,
        launch_digest: [7; 32],
        retention_seconds: protocol::MIN_RETENTION_SECS as u64,
        expected_version: metadata.version.into(),
        expected_target: metadata.target.into(),
        expected_commit: commit,
    }
}
fn incoming(bytes: &[u8]) -> File {
    let (reader, mut writer) = std::io::pipe().unwrap();
    writer.write_all(bytes).unwrap();
    drop(writer);
    relocate(File::from(OwnedFd::from(reader)))
}
fn protocol_rejection(file: File) {
    let locator = PinnedLocator::new(&file);
    assert!(matches!(
        context(file),
        Err(Failure::File(CopyError::Protocol))
    ));
    locator.released();
}

#[test]
fn exact_compiled_context_is_read_and_its_inherited_locator_is_closed() {
    let expected = valid_context();
    let file = incoming(&expected.encode().unwrap());
    let locator = PinnedLocator::new(&file);
    let Ok(actual) = context(file) else {
        panic!("valid compiled context rejected");
    };
    assert!(actual == expected);
    locator.released();
}

#[test]
fn missing_truncated_wrong_domain_and_entrypoint_contexts_are_closed_on_rejection() {
    let bytes = valid_context().encode().unwrap();
    let mut wrong_domain = bytes.clone();
    wrong_domain[0] ^= 1;
    let mut wrong_entrypoint = bytes.clone();
    *wrong_entrypoint.last_mut().unwrap() ^= 1;
    for invalid in [
        Vec::new(),
        bytes[..bytes.len() - 1].to_vec(),
        wrong_domain,
        wrong_entrypoint,
    ] {
        protocol_rejection(incoming(&invalid));
    }
}

#[test]
fn context_build_mismatches_close_the_locator_without_launching_filesystem_work() {
    for field in 0..3 {
        let mut forged = valid_context();
        match field {
            0 => forged.expected_version.push('x'),
            1 => forged.expected_target.push('x'),
            _ => forged.expected_commit[0] ^= 1,
        }
        let file = incoming(&forged.encode().unwrap());
        let locator = PinnedLocator::new(&file);
        assert!(matches!(context(file), Err(Failure::Build)));
        locator.released();
    }
}

#[test]
fn regular_files_and_write_only_pipe_locators_are_consumed_and_rejected() {
    protocol_rejection(relocate(tempfile::tempfile().unwrap()));
    let (_reader, writer) = std::io::pipe().unwrap();
    protocol_rejection(relocate(File::from(OwnedFd::from(writer))));
}

#[test]
fn oversized_context_is_bounded_and_closes_locator_even_with_live_writer() {
    let (reader, mut writer) = std::io::pipe().unwrap();
    let mut bytes = valid_context().encode().unwrap();
    let valid_length = bytes.len();
    bytes.resize(protocol::MAX_CONTEXT + 1, 0);
    let flags = unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_GETFL) };
    assert!(flags >= 0);
    assert_eq!(
        unsafe { libc::fcntl(writer.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) },
        0
    );
    // Queue before starting the nonblocking reader, retaining the writer. A
    // concurrent producer could lose a scheduling race and test EAGAIN only.
    // Smaller pipe capacities also reject promptly, without assuming a minimum
    // capacity greater than PIPE_BUF or blocking a producer thread.
    let mut queued = 0;
    while queued < bytes.len() {
        match writer.write(&bytes[queued..]) {
            Ok(0) => panic!("pipe accepted no context bytes"),
            Ok(n) => queued += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("context prefill failed: {error}"),
        }
    }
    assert!(queued > valid_length);
    eprintln!("oversized context prefill: {queued}/{} bytes", bytes.len());
    let file = relocate(File::from(OwnedFd::from(reader)));
    let locator = PinnedLocator::new(&file);
    let result = context(file);
    if queued == bytes.len() {
        assert!(matches!(result, Err(Failure::File(CopyError::Protocol))));
    } else {
        assert!(matches!(result, Err(Failure::File(CopyError::LocalIo))));
    }
    locator.released();
    drop(writer);
}

#[test]
fn open_writer_cannot_hold_context_startup_waiting_for_eof() {
    let (reader, mut writer) = std::io::pipe().unwrap();
    writer
        .write_all(&valid_context().encode().unwrap())
        .unwrap();
    let file = relocate(File::from(OwnedFd::from(reader)));
    let locator = PinnedLocator::new(&file);
    let (send, receive) = mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        send.send(context(file)).ok();
    });
    let outcome = receive.recv_timeout(Duration::from_secs(2));
    // Always release and join before asserting, including a regression where
    // context incorrectly performs a blocking read until EOF.
    drop(writer);
    worker.join().unwrap();
    assert!(matches!(
        outcome,
        Ok(Err(Failure::File(CopyError::LocalIo)))
    ));
    locator.released();
}
