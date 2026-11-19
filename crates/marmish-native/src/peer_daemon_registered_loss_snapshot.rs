//! Bounded independent file witnesses for the private dispatcher-loss test.
use sha2::{Digest, Sha256};
use std::{
    ffi::CStr,
    fs::{File, Metadata, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
    time::{Duration, Instant},
};

const IMAGE_CAP: u64 = 32 * 1024 * 1024;
const IMAGE_TIME: Duration = Duration::from_millis(250);
const FILES: [&[&CStr]; 3] = [
    &[c"mdk.db", c"mdk.db-wal", c"mdk.db-shm", c"mdk.db-journal"],
    &[
        c"journal.db",
        c"journal.db-wal",
        c"journal.db-shm",
        c"journal.db-journal",
    ],
    &[
        c"engine.sqlite3",
        c"engine.sqlite3-wal",
        c"engine.sqlite3-shm",
        c"engine.sqlite3-journal",
    ],
];

#[derive(Eq, PartialEq)]
pub(super) struct Image(Vec<u8>);

impl Image {
    pub(super) fn capture(root: &Path) -> io::Result<Self> {
        let began = Instant::now();
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(root)?;
        check_directory(&directory)?;
        let command = open_at(&directory, c"commands", libc::O_DIRECTORY)?;
        check_directory(&command)?;
        let mut image = Vec::with_capacity(12 * 58);
        let mut budget = IMAGE_CAP;
        for (index, (names, dir)) in FILES
            .into_iter()
            .zip([&directory, &directory, &command])
            .enumerate()
        {
            for (offset, name) in names.iter().enumerate() {
                image.push((index * 4 + offset) as u8);
                match open_at(dir, name, 0) {
                    Err(error) if offset != 0 && error.kind() == io::ErrorKind::NotFound => {
                        image.push(0);
                    }
                    other => {
                        let mut file = other?;
                        let before = checked_file(&file)?;
                        let len = before.len();
                        if len > budget {
                            return Err(invalid());
                        }
                        let mut hash = Sha256::new();
                        let mut count = 0;
                        let mut buffer = [0u8; 16_384];
                        loop {
                            check_time(began)?;
                            let n = file.read(&mut buffer)?;
                            count += n as u64;
                            if count > budget || count > len {
                                return Err(invalid());
                            }
                            if n == 0 {
                                break;
                            }
                            hash.update(&buffer[..n]);
                        }
                        let current = open_at(dir, name, 0)?;
                        if count != len
                            || pin(&checked_file(&file)?) != pin(&before)
                            || pin(&checked_file(&current)?) != pin(&before)
                        {
                            return Err(invalid());
                        }
                        budget -= count;
                        image.push(1);
                        for field in [before.dev(), before.ino(), len] {
                            image.extend_from_slice(&field.to_be_bytes());
                        }
                        image.extend_from_slice(&hash.finalize());
                    }
                }
                check_time(began)?;
            }
        }
        let current = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(root)?;
        let current_command = open_at(&current, c"commands", libc::O_DIRECTORY)?;
        if pin(&current.metadata()?) != pin(&directory.metadata()?)
            || pin(&current_command.metadata()?) != pin(&command.metadata()?)
        {
            return Err(invalid());
        }
        check_time(began)?;
        Ok(Self(image))
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.0
    }
}

fn invalid() -> io::Error {
    io::Error::other("invalid dispatcher-loss file witness")
}

fn check_time(began: Instant) -> io::Result<()> {
    if began.elapsed() >= IMAGE_TIME {
        return Err(io::ErrorKind::TimedOut.into());
    }
    Ok(())
}

fn open_at(dir: &File, path: &CStr, flags: libc::c_int) -> io::Result<File> {
    // SAFETY: a live directory fd and a static NUL-terminated single component.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            path.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK | flags,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returns a new owned fd.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn check_directory(dir: &File) -> io::Result<()> {
    let metadata = dir.metadata()?;
    // SAFETY: geteuid has no preconditions.
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(invalid());
    }
    Ok(())
}

fn checked_file(file: &File) -> io::Result<Metadata> {
    let m = file.metadata()?;
    // SAFETY: geteuid has no preconditions.
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || !matches!(m.mode() & 0o7777, 0o400 | 0o600)
        || m.nlink() != 1
    {
        return Err(invalid());
    }
    Ok(m)
}

fn pin(m: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}

#[test]
fn file_witness_refuses_altered_bytes_or_a_missing_sidecar() {
    use std::{fs, io::Write};
    let root = crate::journal::private_test_directory();
    let command = root.path().join("commands");
    fs::DirBuilder::new().mode(0o700).create(&command).unwrap();
    let put = |path: &Path, bytes: &[u8]| {
        OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    };
    use std::os::unix::fs::DirBuilderExt as _;
    for path in [
        root.path().join("mdk.db"),
        root.path().join("journal.db"),
        command.join("engine.sqlite3"),
    ] {
        put(&path, b"test");
    }
    let sidecar = command.join("engine.sqlite3-wal");
    put(&sidecar, b"sidecar");
    let initial = Image::capture(root.path()).unwrap();
    assert!(initial == Image::capture(root.path()).unwrap());
    put(&command.join("engine.sqlite3"), b"Test");
    assert!(initial != Image::capture(root.path()).unwrap());
    put(&command.join("engine.sqlite3"), b"test");
    assert!(initial == Image::capture(root.path()).unwrap());
    fs::remove_file(sidecar).unwrap();
    assert!(initial != Image::capture(root.path()).unwrap());
}
