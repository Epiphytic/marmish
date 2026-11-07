//! Test-binary-only, per-worker commit barrier. No production module or env hook.
use std::{
    cell::RefCell,
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Point {
    BeforeRename = 1,
    AfterRename = 2,
}
pub(crate) struct Hook {
    socket: UnixStream,
    point: Point,
}
thread_local! {
    static PENDING: RefCell<Option<Hook>> = const { RefCell::new(None) };
    static CURRENT: RefCell<Option<Hook>> = const { RefCell::new(None) };
}
impl Hook {
    pub(crate) fn new(socket: UnixStream, point: Point) -> io::Result<Self> {
        socket.set_read_timeout(Some(Duration::from_secs(5)))?;
        socket.set_write_timeout(Some(Duration::from_secs(5)))?;
        Ok(Self { socket, point })
    }
}
pub(crate) fn pending(hook: Hook) {
    PENDING.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(hook);
    });
}
pub(crate) fn take_pending() -> Option<Hook> {
    PENDING.with(|slot| slot.borrow_mut().take())
}
pub(crate) fn install(hook: Option<Hook>) {
    CURRENT.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = hook;
    });
}
pub(crate) fn at(point: Point) -> io::Result<()> {
    CURRENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_ref().is_none_or(|hook| hook.point != point) {
            return Ok(());
        }
        let mut hook = slot.take().unwrap();
        hook.socket.write_all(&[point as u8])?;
        let mut release = [0];
        hook.socket.read_exact(&mut release)?;
        if release != [0xa5] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "commit_hook_release",
            ));
        }
        Ok(())
    })
}
