//! Immutable, parent-prepared storage for an allocation-free direct execve.
use super::*;

pub(super) struct ExecImage {
    program: CString,
    _argv: Vec<CString>,
    _environment: Vec<CString>,
    argv_pointers: Vec<*const libc::c_char>,
    environment_pointers: Vec<*const libc::c_char>,
}
// SAFETY: the constructor is the only writer. Pointers reference CString heap
// allocations owned by this same image, never movable inline storage. Moving
// the image does not move those allocations. No method mutates any field or
// dereferences a pointer for writes; execve reads buffers synchronously. Keeping
// the whole image captured retains every owner until execve succeeds or fails.
unsafe impl Send for ExecImage {}
unsafe impl Sync for ExecImage {}

impl ExecImage {
    pub(super) fn new(
        command: &Command,
        argv0: &OsStr,
        environment: &[(OsString, OsString)],
    ) -> std::io::Result<Self> {
        let invalid = || std::io::Error::from_raw_os_error(libc::EINVAL);
        let program = CString::new(command.get_program().as_bytes()).map_err(|_| invalid())?;
        let argv = std::iter::once(argv0)
            .chain(command.get_args())
            .map(|arg| CString::new(arg.as_bytes()).map_err(|_| invalid()))
            .collect::<std::io::Result<Vec<_>>>()?;
        let mut keys = BTreeSet::new();
        let mut values = Vec::with_capacity(environment.len());
        for (key, value) in environment {
            let key = key.as_bytes();
            if key.is_empty() || key.contains(&b'=') || key.contains(&0) || !keys.insert(key) {
                return Err(invalid());
            }
            let mut entry = Vec::with_capacity(key.len() + value.len() + 1);
            entry.extend_from_slice(key);
            entry.push(b'=');
            entry.extend_from_slice(value.as_bytes());
            values.push(CString::new(entry).map_err(|_| invalid())?);
        }
        let pointers = |strings: &[CString]| {
            strings
                .iter()
                .map(|s| s.as_ptr())
                .chain(std::iter::once(std::ptr::null()))
                .collect()
        };
        let argv_pointers = pointers(&argv);
        let environment_pointers = pointers(&values);
        Ok(Self {
            program,
            _argv: argv,
            _environment: values,
            argv_pointers,
            environment_pointers,
        })
    }

    /// Called only after fork's stdio/cwd setup and platform pre-exec syscalls.
    /// Never returns success to std's execvp, which could interpret plain text.
    pub(super) unsafe fn execve(&self) -> std::io::Error {
        unsafe {
            libc::execve(
                self.program.as_ptr(),
                self.argv_pointers.as_ptr(),
                self.environment_pointers.as_ptr(),
            );
        }
        std::io::Error::last_os_error()
    }
}
