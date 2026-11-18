//! Process-lifetime wake pipe and a single foreground signal owner.

use super::{Error, Result};
use std::mem::MaybeUninit;
use std::os::fd::RawFd;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

const SIGNAL: Error = Error::Invalid("terminal_signals");
pub(super) const MANAGED: [libc::c_int; 9] = [
    libc::SIGWINCH,
    libc::SIGTSTP,
    libc::SIGCONT,
    libc::SIGTERM,
    libc::SIGHUP,
    libc::SIGINT,
    libc::SIGQUIT,
    libc::SIGTTIN,
    libc::SIGTTOU,
];

static OWNER: AtomicBool = AtomicBool::new(false);
static PIPE: OnceLock<(RawFd, RawFd)> = OnceLock::new();
static WAKE_WRITER: AtomicI32 = AtomicI32::new(-1);
static PENDING: AtomicU32 = AtomicU32::new(0);
static TERMINATION: AtomicI32 = AtomicI32::new(0);

pub(super) struct Signals {
    read: RawFd,
    installed: Vec<(libc::c_int, libc::sigaction)>,
}

#[derive(Default)]
pub(super) struct Events {
    bits: u32,
    pub(super) termination: Option<libc::c_int>,
}

impl Events {
    pub(super) fn has(&self, signal: libc::c_int) -> bool {
        self.bits & bit(signal) != 0
    }
}

impl Signals {
    /// Call before starting other threads, or with managed signals blocked on
    /// every auxiliary thread. The owner returns directly to process exit.
    pub(super) fn install() -> Result<Self> {
        Self::install_inner(None)
    }

    fn install_inner(fail_at: Option<usize>) -> Result<Self> {
        if OWNER.swap(true, Ordering::AcqRel) {
            return Err(SIGNAL);
        }
        let result = Self::install_owned(fail_at);
        if result.is_err() {
            OWNER.store(false, Ordering::Release);
        }
        result
    }

    fn install_owned(fail_at: Option<usize>) -> Result<Self> {
        let mask = managed_mask()?;
        let old_mask = mask_change(libc::SIG_BLOCK, &mask)?;
        let result = Self::register(&mask, fail_at);
        let mut signals = match result {
            Ok(signals) => signals,
            Err(error) => {
                let _ = mask_change(libc::SIG_SETMASK, &old_mask);
                return Err(error);
            }
        };
        if mask_change(libc::SIG_UNBLOCK, &mask).is_err() {
            signals.restore_actions();
            let _ = mask_change(libc::SIG_SETMASK, &old_mask);
            return Err(SIGNAL);
        }
        Ok(signals)
    }

    fn register(mask: &libc::sigset_t, fail_at: Option<usize>) -> Result<Self> {
        let (read, write) = match PIPE.get() {
            Some(&pipe) => pipe,
            None => {
                let pipe = create_pipe()?;
                // OWNER serializes creation. A failed syscall leaves PIPE empty.
                PIPE.set(pipe)
                    .expect("first signal owner initializes the pipe");
                pipe
            }
        };
        WAKE_WRITER.store(write, Ordering::Release);
        let mut signals = Self {
            read,
            installed: Vec::with_capacity(MANAGED.len()),
        };
        signals.clear();
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = signal_handler as *const () as usize;
        action.sa_mask = *mask;
        for (index, signal) in MANAGED.into_iter().enumerate() {
            let mut saved = MaybeUninit::zeroed();
            if fail_at == Some(index)
                || unsafe { libc::sigaction(signal, &action, saved.as_mut_ptr()) } != 0
            {
                signals.restore_actions();
                return Err(SIGNAL);
            }
            signals
                .installed
                .push((signal, unsafe { saved.assume_init() }));
        }
        Ok(signals)
    }

    pub(super) fn fd(&self) -> RawFd {
        self.read
    }

    pub(super) fn take(&self) -> Events {
        drain(self.read);
        let bits = PENDING.swap(0, Ordering::AcqRel);
        let termination = TERMINATION.load(Ordering::Acquire);
        Events {
            bits,
            termination: (termination != 0).then_some(termination),
        }
    }

    pub(super) fn pending(&self) -> bool {
        PENDING.load(Ordering::Acquire) != 0
    }

    fn clear(&self) {
        drain(self.read);
        PENDING.store(0, Ordering::Release);
        TERMINATION.store(0, Ordering::Release);
    }

    fn restore_actions(&mut self) {
        while let Some((signal, action)) = self.installed.pop() {
            unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) };
        }
        self.clear();
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        if !self.installed.is_empty() {
            if let Ok(mask) = managed_mask() {
                let _ = mask_change(libc::SIG_BLOCK, &mask);
            }
            self.restore_actions();
            // Keep managed signals blocked from successful teardown through
            // process exit. A second termination cannot change the outcome.
            OWNER.store(false, Ordering::Release);
        }
    }
}

fn bit(signal: libc::c_int) -> u32 {
    1 << (signal as u32 - 1)
}

extern "C" fn signal_handler(signal: libc::c_int) {
    #[cfg(target_os = "linux")]
    let errno = unsafe { libc::__errno_location() };
    #[cfg(target_os = "macos")]
    let errno = unsafe { libc::__error() };
    let saved = unsafe { *errno };
    PENDING.fetch_or(bit(signal), Ordering::Release);
    if matches!(
        signal,
        libc::SIGTERM | libc::SIGHUP | libc::SIGINT | libc::SIGQUIT
    ) {
        let _ = TERMINATION.compare_exchange(0, signal, Ordering::AcqRel, Ordering::Relaxed);
    }
    let fd = WAKE_WRITER.load(Ordering::Acquire);
    if fd != -1 {
        let wake = 1u8;
        unsafe { libc::write(fd, (&wake as *const u8).cast(), 1) };
    }
    unsafe { *errno = saved };
}

fn managed_mask() -> Result<libc::sigset_t> {
    let mut mask = MaybeUninit::zeroed();
    if unsafe { libc::sigemptyset(mask.as_mut_ptr()) } != 0 {
        return Err(SIGNAL);
    }
    let mut mask = unsafe { mask.assume_init() };
    for signal in MANAGED {
        if unsafe { libc::sigaddset(&mut mask, signal) } != 0 {
            return Err(SIGNAL);
        }
    }
    Ok(mask)
}

fn mask_change(how: libc::c_int, mask: &libc::sigset_t) -> Result<libc::sigset_t> {
    let mut old = MaybeUninit::zeroed();
    if unsafe { libc::pthread_sigmask(how, mask, old.as_mut_ptr()) } != 0 {
        return Err(SIGNAL);
    }
    Ok(unsafe { old.assume_init() })
}

fn create_pipe() -> Result<(RawFd, RawFd)> {
    let mut fd = [-1; 2];
    #[cfg(target_os = "linux")]
    if unsafe { libc::pipe2(fd.as_mut_ptr(), libc::O_CLOEXEC | libc::O_NONBLOCK) } != 0 {
        return Err(SIGNAL);
    }
    #[cfg(target_os = "macos")]
    {
        if unsafe { libc::pipe(fd.as_mut_ptr()) } != 0 {
            return Err(SIGNAL);
        }
        for descriptor in fd {
            let ok = unsafe {
                libc::fcntl(descriptor, libc::F_SETFD, libc::FD_CLOEXEC) == 0
                    && libc::fcntl(descriptor, libc::F_SETFL, libc::O_NONBLOCK) == 0
            };
            if !ok {
                for descriptor in fd {
                    unsafe { libc::close(descriptor) };
                }
                return Err(SIGNAL);
            }
        }
    }
    Ok((fd[0], fd[1]))
}

fn drain(fd: RawFd) {
    let mut wake = [0u8; 256];
    // The atomic flags, not the pipe volume, determine what work is ready.
    for _ in 0..64 {
        let read = unsafe { libc::read(fd, wake.as_mut_ptr().cast(), wake.len()) };
        if read <= 0 {
            break;
        }
    }
}

#[cfg(test)]
pub(super) fn verify_install_rollback() {
    fn current_action(signal: libc::c_int) -> libc::sigaction {
        let mut action = MaybeUninit::zeroed();
        assert_eq!(
            unsafe { libc::sigaction(signal, std::ptr::null(), action.as_mut_ptr()) },
            0
        );
        unsafe { action.assume_init() }
    }

    let mask = managed_mask().unwrap();
    let saved_mask = mask_change(libc::SIG_BLOCK, &mask).unwrap();
    let saved_actions = MANAGED.map(current_action);
    let public_flags = libc::SA_NOCLDSTOP
        | libc::SA_NOCLDWAIT
        | libc::SA_NODEFER
        | libc::SA_ONSTACK
        | libc::SA_RESETHAND
        | libc::SA_RESTART
        | libc::SA_SIGINFO;
    for fault in 0..MANAGED.len() {
        assert!(Signals::install_inner(Some(fault)).is_err());
        for (signal, before) in MANAGED.into_iter().zip(saved_actions.iter()) {
            let after = current_action(signal);
            assert_eq!(after.sa_sigaction, before.sa_sigaction);
            // libc may set platform-private flags when restoring SIG_DFL.
            assert_eq!(
                after.sa_flags & public_flags,
                before.sa_flags & public_flags
            );
            for member in MANAGED {
                assert_eq!(
                    unsafe { libc::sigismember(&after.sa_mask, member) },
                    unsafe { libc::sigismember(&before.sa_mask, member) }
                );
            }
        }
        let after = mask_change(libc::SIG_BLOCK, &mask).unwrap();
        for signal in MANAGED {
            assert_eq!(unsafe { libc::sigismember(&after, signal) }, 1);
        }
    }
    let descriptors = *PIPE.get().unwrap();
    for fd in [descriptors.0, descriptors.1] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        let fd_flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
        assert!(flags >= 0 && flags & libc::O_NONBLOCK != 0);
        assert!(fd_flags >= 0 && fd_flags & libc::FD_CLOEXEC != 0);
    }
    let owner = Signals::install().unwrap();
    assert_eq!(owner.fd(), descriptors.0);
    drop(owner);
    mask_change(libc::SIG_SETMASK, &saved_mask).unwrap();
}
