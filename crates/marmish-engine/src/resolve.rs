//! Bounded executable lookup. Only the engine actor calls this filesystem API.
use crate::model::*;
use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};

fn filesystem(error: std::io::Error) -> EngineError {
    match error.raw_os_error() {
        Some(libc::ENOENT | libc::ENOTDIR) => {
            EngineError::CommandNotFound(NotFoundReason::ExecutableMissing)
        }
        Some(libc::EACCES | libc::EPERM) => {
            EngineError::CommandNotExecutable(NotExecutableReason::FilesystemPermission)
        }
        Some(libc::ELOOP) => EngineError::CommandResolutionFailed(ResolutionReason::SymlinkLoop),
        _ => EngineError::CommandResolutionFailed(ResolutionReason::FilesystemIo),
    }
}

fn candidate(path: &Path) -> Result<Vec<u8>> {
    let canonical = std::fs::canonicalize(path).map_err(filesystem)?;
    let bytes = canonical.as_os_str().as_bytes();
    if bytes.len() > 4095 {
        return Err(EngineError::InvalidRequest);
    }
    let metadata = std::fs::metadata(&canonical).map_err(filesystem)?;
    if !metadata.is_file() {
        return Err(EngineError::CommandNotExecutable(
            NotExecutableReason::NotRegularFile,
        ));
    }
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(EngineError::CommandNotExecutable(
            NotExecutableReason::ExecuteBitMissing,
        ));
    }
    let name = CString::new(bytes).map_err(|_| EngineError::InvalidRequest)?;
    // The engine refuses differing real/effective identities; AT_EACCESS also
    // makes this explicitly the same permission check used for execution.
    if unsafe { libc::faccessat(libc::AT_FDCWD, name.as_ptr(), libc::X_OK, libc::AT_EACCESS) } != 0
    {
        return Err(filesystem(std::io::Error::last_os_error()));
    }
    Ok(bytes.to_vec())
}

fn lexical(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                output.pop();
            }
            Component::CurDir => {}
            component => output.push(component.as_os_str()),
        }
    }
    output
}

fn resolved(argv: &[Vec<u8>], path: &Path) -> Result<CommandRequest> {
    let executable = candidate(path)?;
    let request = CommandRequest::ResolvedExec {
        executable,
        invoked_path: lexical(path).as_os_str().as_bytes().to_vec(),
        argv: argv.to_vec(),
    };
    crate::admission::validate_request(&request, false)?;
    Ok(request)
}

pub(crate) fn resolve_exec(
    request: &CommandRequest,
    search_path: Option<&OsStr>,
    cwd: &Path,
) -> Result<CommandRequest> {
    crate::admission::validate_request(request, false)?;
    let CommandRequest::Exec { argv } = request else {
        return Err(EngineError::InvalidRequest);
    };
    let name = Path::new(OsStr::from_bytes(&argv[0]));
    if argv[0].contains(&b'/') {
        return resolved(argv, &cwd.join(name));
    }
    let Some(search_path) = search_path else {
        return Err(EngineError::CommandNotFound(
            NotFoundReason::PathSearchExhausted,
        ));
    };
    let bytes = search_path.as_bytes();
    if bytes.len() > 65536 || bytes.split(|b| *b == b':').count() > 1024 || bytes.contains(&0) {
        return Err(EngineError::CommandResolutionFailed(
            ResolutionReason::InvalidSearchPath,
        ));
    }
    let mut unusable = None;
    for entry in bytes.split(|b| *b == b':') {
        let path = cwd.join(Path::new(OsStr::from_bytes(entry))).join(name);
        match resolved(argv, &path) {
            Ok(request) => return Ok(request),
            Err(EngineError::CommandNotFound(_)) => {}
            Err(error @ EngineError::CommandNotExecutable(_)) => {
                if unusable.is_none() {
                    unusable = Some(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
    Err(unusable.unwrap_or(EngineError::CommandNotFound(
        NotFoundReason::PathSearchExhausted,
    )))
}

#[cfg(test)]
mod tests;
