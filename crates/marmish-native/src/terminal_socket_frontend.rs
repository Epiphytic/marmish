//! Adapter from framed same-UID IPC to the foreground terminal loop.

use super::terminal_frontend::Channel;
use super::terminal_socket_session::{ForegroundSocket, FrontendOutput};
use super::{Error, Result};
use crate::terminal_ipc_wire::Close;
use std::{os::fd::RawFd, time::Instant};

impl Channel for ForegroundSocket {
    fn fd(&self) -> RawFd {
        self.fd()
    }

    fn read_output(&mut self, bytes: &mut [u8]) -> Result<Option<usize>> {
        let output = bytes
            .try_into()
            .map_err(|_| Error::Invalid("terminal_local_channel"))?;
        match self.read(output, Instant::now())? {
            FrontendOutput::Pending => Ok(None),
            FrontendOutput::Bytes(count) => Ok(Some(count)),
            FrontendOutput::Closed(_) => Ok(Some(0)),
        }
    }

    fn write_input(&mut self, bytes: &[u8]) -> Result<Option<usize>> {
        let count = self.write(bytes, Instant::now())?;
        Ok((count != 0).then_some(count))
    }

    fn resize(&mut self, rows: u16, columns: u16) -> Result<bool> {
        self.submit_resize((rows, columns))?;
        Ok(true)
    }

    fn framed(&self) -> bool {
        true
    }

    fn wants_read(&self) -> bool {
        self.wants_read()
    }

    fn buffered(&self) -> bool {
        self.buffered()
    }

    fn wants_write(&self, has_input: bool) -> bool {
        self.wants_write(has_input)
    }

    fn wait_for_input_progress(&mut self, has_input: bool, now: Instant) {
        self.wait_for_input_progress(has_input, now);
    }

    fn deadline(&self) -> Option<Instant> {
        self.deadline()
    }

    fn display_committed(&mut self) -> Result<()> {
        self.display_committed()
    }

    fn begin_detach(&mut self, now: Instant) {
        self.begin_detach(now);
    }

    fn detach_complete(&self) -> bool {
        self.detach_complete()
    }

    fn peer_close(&self) -> Option<Close> {
        self.peer_close()
    }
}
