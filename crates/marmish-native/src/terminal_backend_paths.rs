//! Checked host paths and credentials for the local terminal worker.
use crate::{Error, Result};
use std::collections::VecDeque;
use std::ffi::{OsString, c_char};
use std::fs;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Component, Path, PathBuf};

pub(super) const ACCOUNT_ERROR: Error = Error::Invalid("terminal_account");
const TMUX_ERROR: Error = Error::Invalid("terminal_tmux_path");
const SOCKET_ERROR: Error = Error::Invalid("terminal_socket");

pub(super) fn same_nonroot_ids(ids: &[libc::uid_t], groups: &[libc::gid_t]) -> bool {
    ids.first()
        .is_some_and(|uid| *uid != 0 && ids.iter().all(|value| value == uid))
        && groups
            .first()
            .is_some_and(|gid| *gid != 0 && groups.iter().all(|value| value == gid))
}

pub(crate) fn checked_uid() -> Result<libc::uid_t> {
    #[cfg(target_os = "linux")]
    {
        let (mut real, mut effective, mut saved) = (0, 0, 0);
        let (mut real_group, mut effective_group, mut saved_group) = (0, 0, 0);
        // SAFETY: each output points to a separate live ID value.
        if unsafe { libc::getresuid(&mut real, &mut effective, &mut saved) } != 0
            || unsafe { libc::getresgid(&mut real_group, &mut effective_group, &mut saved_group) }
                != 0
            || !same_nonroot_ids(
                &[real, effective, saved],
                &[real_group, effective_group, saved_group],
            )
        {
            return Err(ACCOUNT_ERROR);
        }
        Ok(effective)
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: these calls take no parameters and only inspect process identity.
        let (real, effective, group, effective_group, restricted) = unsafe {
            (
                libc::getuid(),
                libc::geteuid(),
                libc::getgid(),
                libc::getegid(),
                libc::issetugid(),
            )
        };
        if restricted != 0 || !same_nonroot_ids(&[real, effective], &[group, effective_group]) {
            return Err(ACCOUNT_ERROR);
        }
        Ok(effective)
    }
}

pub(super) struct User {
    pub name: String,
    pub home: PathBuf,
    pub shell: PathBuf,
}

fn passwd_field(buffer: &[u8], pointer: *const c_char, limit: usize) -> Option<&str> {
    let offset = pointer.addr().checked_sub(buffer.as_ptr().addr())?;
    let remaining = buffer.get(offset..)?;
    let value = &remaining[..remaining.len().min(limit + 1)];
    let end = value.iter().position(|byte| *byte == 0)?;
    let field = std::str::from_utf8(&value[..end]).ok()?;
    (!field.is_empty() && !field.chars().any(char::is_control)).then_some(field)
}

pub(super) fn user(uid: libc::uid_t) -> Result<User> {
    for length in [256, 1024, 4096, 16_384] {
        let mut buffer = vec![0_u8; length];
        let mut passwd = MaybeUninit::<libc::passwd>::uninit();
        let mut result = std::ptr::null_mut();
        // SAFETY: all output buffers are exclusive and have the specified capacity.
        let status = unsafe {
            libc::getpwuid_r(
                uid,
                passwd.as_mut_ptr(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                &mut result,
            )
        };
        if status == libc::ERANGE {
            continue;
        }
        if status != 0 || result != passwd.as_mut_ptr() {
            return Err(ACCOUNT_ERROR);
        }
        // SAFETY: success with a pointer to our record initializes it.
        let passwd = unsafe { passwd.assume_init() };
        let name = passwd_field(&buffer, passwd.pw_name, 64).ok_or(ACCOUNT_ERROR)?;
        let home = Path::new(passwd_field(&buffer, passwd.pw_dir, 4096).ok_or(ACCOUNT_ERROR)?);
        let shell = Path::new(passwd_field(&buffer, passwd.pw_shell, 4096).ok_or(ACCOUNT_ERROR)?);
        if !home.is_absolute() || !shell.is_absolute() {
            return Err(ACCOUNT_ERROR);
        }
        return Ok(User {
            name: name.to_owned(),
            home: home.to_owned(),
            shell: shell.to_owned(),
        });
    }
    Err(ACCOUNT_ERROR)
}

pub(super) fn daemon(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path.file_name().is_none_or(|name| name != "marmishd")
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return Err(Error::Invalid("terminal_daemon_path"));
    }
    crate::native_layout::resolve(path).map(|_| ())
}

enum Part {
    Root,
    Parent,
    Name(OsString),
}

fn parts(path: &Path) -> impl DoubleEndedIterator<Item = Part> + '_ {
    path.components().filter_map(|component| match component {
        Component::RootDir => Some(Part::Root),
        Component::ParentDir => Some(Part::Parent),
        Component::Normal(name) => Some(Part::Name(name.to_owned())),
        _ => None,
    })
}

pub(super) fn tmux(path: &Path, uid: libc::uid_t) -> Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|part| part == Component::ParentDir) {
        return Err(TMUX_ERROR);
    }
    let mut queue: VecDeque<_> = parts(path).collect();
    let mut resolved = PathBuf::from("/");
    let root = fs::symlink_metadata(&resolved).map_err(|_| TMUX_ERROR)?;
    if !root.is_dir() || root.uid() != 0 || root.mode() & 0o022 != 0 {
        return Err(TMUX_ERROR);
    }
    let mut links = 0;
    while let Some(part) = queue.pop_front() {
        match part {
            Part::Root => resolved = PathBuf::from("/"),
            Part::Parent => {
                resolved.pop();
            }
            Part::Name(name) => {
                let candidate = resolved.join(name);
                let metadata = fs::symlink_metadata(&candidate).map_err(|_| TMUX_ERROR)?;
                if metadata.uid() != 0 && metadata.uid() != uid {
                    return Err(TMUX_ERROR);
                }
                if metadata.is_symlink() {
                    links += 1;
                    if links > 16 {
                        return Err(TMUX_ERROR);
                    }
                    let target = fs::read_link(candidate).map_err(|_| TMUX_ERROR)?;
                    for part in parts(&target).rev() {
                        queue.push_front(part);
                    }
                } else if metadata.mode() & 0o6022 != 0 {
                    return Err(TMUX_ERROR);
                } else if queue.is_empty() {
                    if !metadata.is_file() || metadata.mode() & 0o111 == 0 {
                        return Err(TMUX_ERROR);
                    }
                    return Ok(candidate);
                } else if metadata.is_dir() {
                    resolved = candidate;
                } else {
                    return Err(TMUX_ERROR);
                }
            }
        }
    }
    Err(TMUX_ERROR)
}

pub(super) fn socket(path: &Path, uid: libc::uid_t) -> Result<()> {
    if !path.is_absolute()
        || path.as_os_str().as_bytes().len() > 103
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return Err(SOCKET_ERROR);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| SOCKET_ERROR)?;
    let parent = path.parent().ok_or(SOCKET_ERROR)?;
    let directory = fs::symlink_metadata(parent).map_err(|_| SOCKET_ERROR)?;
    if !metadata.file_type().is_socket()
        || metadata.uid() != uid
        || !directory.is_dir()
        || directory.uid() != uid
        || directory.mode() & 0o7777 != 0o700
    {
        return Err(SOCKET_ERROR);
    }
    for ancestor in parent.ancestors().skip(1) {
        marmish_engine::validate_executable_ancestor(ancestor).map_err(|_| SOCKET_ERROR)?;
    }
    Ok(())
}

pub(super) fn session(name: &str) -> Result<()> {
    if !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        Ok(())
    } else {
        Err(Error::Invalid("terminal_session"))
    }
}
