//! Directory descriptors remain held for the complete synchronous install transaction.
use crate::{Error, Result};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

#[derive(Default)]
pub(super) struct Anchors {
    directories: RefCell<BTreeMap<PathBuf, File>>,
}
fn bad() -> Error {
    Error::Invalid("unsafe_install_path")
}
fn name(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| bad())
}
impl Anchors {
    pub fn directory(&self, path: &Path) -> Result<File> {
        if let Some(file) = self.directories.borrow().get(path) {
            return Ok(file.try_clone()?);
        }
        if !path.is_absolute()
            || path
                .components()
                .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        {
            return Err(bad());
        }
        let file = if path == Path::new("/") {
            let fd = unsafe {
                libc::open(
                    c"/".as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            unsafe { File::from_raw_fd(fd) }
        } else {
            let parent = self.directory(path.parent().ok_or_else(bad)?)?;
            let leaf = name(Path::new(path.file_name().ok_or_else(bad)?))?;
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    leaf.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            unsafe { File::from_raw_fd(fd) }
        };
        marmish_engine::validate_executable_directory(&file).map_err(|_| bad())?;
        self.directories
            .borrow_mut()
            .insert(path.into(), file.try_clone()?);
        Ok(file)
    }
    fn parent(&self, path: &Path) -> Result<(File, CString)> {
        Ok((
            self.directory(path.parent().ok_or_else(bad)?)?,
            name(Path::new(path.file_name().ok_or_else(bad)?))?,
        ))
    }
    pub fn open(&self, path: &Path, flags: i32, mode: u32) -> Result<File> {
        let (parent, leaf) = self.parent(path)?;
        // openat is variadic: macOS mode_t is u16 and must be promoted.
        let promoted_mode: libc::c_uint = mode;
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                leaf.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                promoted_mode,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { File::from_raw_fd(fd) })
    }
    pub fn mkdir(&self, path: &Path, mode: u32) -> Result<()> {
        let (parent, leaf) = self.parent(path)?;
        if unsafe { libc::mkdirat(parent.as_raw_fd(), leaf.as_ptr(), mode as libc::mode_t) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    pub fn symlink(&self, target: &Path, path: &Path) -> Result<()> {
        let (parent, leaf) = self.parent(path)?;
        let target = name(target)?;
        if unsafe { libc::symlinkat(target.as_ptr(), parent.as_raw_fd(), leaf.as_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    pub fn unlink(&self, path: &Path, directory: bool) -> Result<()> {
        let (parent, leaf) = self.parent(path)?;
        if unsafe {
            libc::unlinkat(
                parent.as_raw_fd(),
                leaf.as_ptr(),
                if directory { libc::AT_REMOVEDIR } else { 0 },
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        if directory {
            self.directories.borrow_mut().remove(path);
        }
        Ok(())
    }
    pub fn rename(&self, from: &Path, to: &Path, exclusive: bool) -> Result<()> {
        let (source, source_name) = self.parent(from)?;
        let (dest, dest_name) = self.parent(to)?;
        let result = if exclusive {
            #[cfg(target_os = "linux")]
            {
                unsafe {
                    libc::renameat2(
                        source.as_raw_fd(),
                        source_name.as_ptr(),
                        dest.as_raw_fd(),
                        dest_name.as_ptr(),
                        libc::RENAME_NOREPLACE,
                    )
                }
            }
            #[cfg(target_os = "macos")]
            {
                unsafe {
                    libc::renameatx_np(
                        source.as_raw_fd(),
                        source_name.as_ptr(),
                        dest.as_raw_fd(),
                        dest_name.as_ptr(),
                        libc::RENAME_EXCL,
                    )
                }
            }
        } else {
            unsafe {
                libc::renameat(
                    source.as_raw_fd(),
                    source_name.as_ptr(),
                    dest.as_raw_fd(),
                    dest_name.as_ptr(),
                )
            }
        };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if exclusive {
                return Err(Error::Invalid(
                    if matches!(
                        error.raw_os_error(),
                        Some(libc::EINVAL | libc::ENOSYS | libc::ENOTSUP)
                    ) {
                        "unsupported_install_filesystem"
                    } else {
                        "version_conflict"
                    },
                ));
            }
            return Err(error.into());
        }
        let held = self.directories.borrow_mut().remove(from);
        if let Some(file) = held {
            self.directories.borrow_mut().insert(to.into(), file);
        }
        Ok(())
    }
    pub fn chmod_dir(&self, path: &Path, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        self.directory(path)?
            .set_permissions(std::fs::Permissions::from_mode(mode))?;
        Ok(())
    }
    pub fn sync(&self, path: &Path) -> Result<()> {
        self.directory(path)?.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[test]
    fn mutation_uses_held_directory_after_original_name_is_replaced() {
        let root = tempfile::Builder::new()
            .prefix("marmish-dirfd-test-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(std::env::var_os("HOME").unwrap())
            .unwrap();
        let original = root.path().join("owned");
        let retained = root.path().join("retained");
        let unrelated = root.path().join("unrelated");
        std::fs::create_dir(&original).unwrap();
        std::fs::create_dir(&unrelated).unwrap();
        let anchors = Anchors::default();
        let _held = anchors.directory(&original).unwrap();
        std::fs::rename(&original, &retained).unwrap();
        symlink(&unrelated, &original).unwrap();
        anchors
            .open(
                &original.join("proof"),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )
            .unwrap()
            .write_all(b"held")
            .unwrap();
        assert_eq!(std::fs::read(retained.join("proof")).unwrap(), b"held");
        assert!(!unrelated.join("proof").exists());
    }
}
