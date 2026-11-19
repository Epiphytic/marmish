//! Footprint evidence for one live near-cap copy, outside the timed cohorts.
use std::{
    ffi::CStr,
    fs::{self, File, Metadata, OpenOptions},
    io,
    mem::MaybeUninit,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, OpenOptionsExt},
        },
    },
    path::Path,
    time::Duration,
};

use tokio::time::{Instant, sleep};

use crate::{
    peer_invitation::store::installed::RESERVED_BYTES,
    peer_quota_vfs::dispatch::{CLOSED_STAGE_LIMIT, TOTAL_ADMISSION},
};

const CURSOR: &CStr = c"cursor.db";
const ORIGINAL: &CStr = c"provisional-unknown.db";
const SNAPSHOT: &CStr = c"receiver-join-snapshot.db";
const NAMES: [&CStr; 3] = [CURSOR, ORIGINAL, SNAPSHOT];
const PERIOD: Duration = Duration::from_millis(2);
const LIMIT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Extent {
    identity: (u64, u64),
    logical: u64,
    allocated: u64,
}

impl Extent {
    fn charge(self) -> u64 {
        self.logical.max(self.allocated)
    }
}

fn dir_identity(meta: &Metadata, uid: u32) -> Option<(u64, u64)> {
    (meta.is_dir() && meta.mode() & 0o7777 == 0o700 && meta.uid() == uid)
        .then(|| (meta.dev(), meta.ino()))
}

fn extent(dir: &File, name: &CStr, device: u64, uid: u32) -> io::Result<Option<Extent>> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the descriptor and terminated name are live, and stat is writable.
    if unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        let error = io::Error::last_os_error();
        return if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        };
    }
    // SAFETY: successful fstatat initialized the result.
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG
        || stat.st_mode & 0o7777 != 0o600
        || stat.st_nlink != 1
        || stat.st_uid != uid
        || stat.st_dev != device
    {
        return Err(io::Error::other("copy observation file type"));
    }

    let logical = u64::try_from(stat.st_size).map_err(io::Error::other)?;
    let allocated = u64::try_from(stat.st_blocks)
        .ok()
        .and_then(|blocks| blocks.checked_mul(512))
        .ok_or_else(|| io::Error::other("copy observation allocation"))?;
    Ok(Some(Extent {
        identity: (stat.st_dev, stat.st_ino),
        logical,
        allocated,
    }))
}

fn read_extents(dir: &File, device: u64, uid: u32) -> io::Result<[Option<Extent>; 3]> {
    let [cursor, original, snapshot] = NAMES;
    Ok([
        extent(dir, cursor, device, uid)?,
        extent(dir, original, device, uid)?,
        extent(dir, snapshot, device, uid)?,
    ])
}

fn sample(
    dir: &File,
    descriptor_path: &Path,
    device: u64,
    uid: u32,
    expected: Extent,
) -> io::Result<Option<Extent>> {
    let before = read_extents(dir, device, uid)?;
    let [Some(cursor), Some(original), Some(copy)] = before else {
        return Ok(None);
    };
    if original != expected {
        return Err(io::Error::other("copy observation original witness"));
    }
    if copy.logical != original.logical
        || copy.identity == original.identity
        || copy.identity == cursor.identity
        || cursor.identity == original.identity
    {
        return Ok(None);
    }
    // Linux's procfs fd link anchors enumeration to the already-open directory.
    // Any Known file, sidecar or unexpected directory entry invalidates the sample.
    let mut count = 0;
    for entry in fs::read_dir(descriptor_path)? {
        let name = entry?.file_name();
        count += 1;
        if count > NAMES.len()
            || !NAMES
                .iter()
                .any(|allowed| allowed.to_bytes() == name.as_bytes())
        {
            return Ok(None);
        }
    }
    if count == NAMES.len() && before == read_extents(dir, device, uid)? {
        return Ok(Some(copy));
    }
    Ok(None)
}

fn filesystem_magic(dir: &File) -> io::Result<libc::c_long> {
    let mut stat = MaybeUninit::<libc::statfs>::uninit();
    // SAFETY: the descriptor is live and stat is writable.
    if unsafe { libc::fstatfs(dir.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful fstatfs initialized the result.
    Ok(unsafe { stat.assume_init() }.f_type)
}

pub(super) async fn measure(
    peer_dir: &Path,
    original_identity: (u64, u64),
    original_logical: u64,
    original_allocated: u64,
) -> Result<(), &'static str> {
    let expected = Extent {
        identity: original_identity,
        logical: original_logical,
        allocated: original_allocated,
    };
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(peer_dir)
        .map_err(|_| "copy_observation_directory")?;
    // SAFETY: reads this process's own effective UID.
    let uid = unsafe { libc::geteuid() };
    let (device, inode) = dir_identity(&dir.metadata().map_err(|_| "copy_observation_dir")?, uid)
        .ok_or("copy_observation_dir")?;
    let descriptor_path = format!("/proc/self/fd/{}", dir.as_raw_fd());
    let fs_magic = filesystem_magic(&dir).map_err(|_| "copy_observation_filesystem")?;
    let started = Instant::now();
    let mut prior = None;
    let copy = loop {
        if started.elapsed() >= LIMIT {
            return Err("copy_observation_no_stable_sample");
        }
        let current = sample(&dir, Path::new(&descriptor_path), device, uid, expected)
            .map_err(|_| "copy_observation_sample")?;
        if let Some(copy) = current
            && Some(copy) == prior
        {
            break copy;
        }
        prior = current;
        sleep(PERIOD).await;
    };
    if dir_identity(
        &fs::symlink_metadata(peer_dir).map_err(|_| "copy_observation_dir")?,
        uid,
    ) != Some((device, inode))
    {
        return Err("copy_observation_dir_replaced");
    }
    let charged = RESERVED_BYTES
        .checked_add(expected.charge())
        .and_then(|charged| charged.checked_add(copy.charge()))
        .ok_or("copy_observation_charge")?;
    println!(
        "near_cap_copy filesystem_magic={fs_magic} original_logical={} original_allocated={} copy_logical={} copy_allocated={} aggregate_charged={charged} observed_ms={}",
        expected.logical,
        expected.allocated,
        copy.logical,
        copy.allocated,
        started.elapsed().as_millis(),
    );
    if !(1..=CLOSED_STAGE_LIMIT).contains(&copy.logical) || charged >= TOTAL_ADMISSION {
        return Err("copy_observation_charge");
    }
    Ok(())
}
