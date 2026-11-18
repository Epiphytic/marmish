//! Test-gated local terminal registry and sole owner of PTY effects.
#[path = "terminal_live_supervisor.rs"]
mod live;
#[path = "terminal_live_supervisor_recovery.rs"]
mod recovery;
#[path = "terminal_supervisor_store.rs"]
mod store;
#[path = "terminal_supervisor_tests.rs"]
mod tests;
#[path = "terminal_supervisor_tmux.rs"]
mod tmux;

use super::{self as backend, Attachment, CheckedLaunch, ReadStatus, Role, Shutdown, WriteStatus};
use crate::command_config::CommandConfig;
use crate::command_wire::Account;
use crate::identity::{IdentityMaterial, ProfileLease};
use crate::{Error, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use store::{Registry, Slot, State};
use tmux::{ClientChange, Inventory, Manager};

const DENIED: Error = Error::Invalid("terminal_grant_denied");
const MISSING: Error = Error::Invalid("terminal_slot_missing");
const UNAVAILABLE: Error = Error::Invalid("terminal_slot_unavailable");
const STALE: Error = Error::Invalid("terminal_claim_stale");
const CAPACITY: Error = Error::Invalid("terminal_capacity");
const CLEANUP: Error = Error::Invalid("terminal_cleanup_pending");
const REQUEST: Error = Error::Invalid("terminal_attach_request");
const MAX_SLOTS: usize = 16;
const MAX_ATTACHMENTS: usize = 4;
const REAP_TURN: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SlotId([u8; 16]);

#[derive(Clone, Copy, PartialEq, Eq)]
struct Claim {
    random: [u8; 16],
    generation: u64,
}

#[derive(Clone, Copy)]
enum Selection {
    Default,
    Existing(SlotId),
    New,
}

#[derive(Clone, Copy)]
enum Mode {
    Writer,
    Observe,
    Takeover(Claim),
}

#[derive(Clone, Copy)]
enum Admission {
    Attached { slot: SlotId, claim: Claim },
    Busy { slot: SlotId, writer: Claim },
}

struct Link {
    claim: Claim,
    role: Role,
    io: Attachment,
}

#[derive(Default)]
struct Active {
    links: Vec<Link>,
    reaping: Vec<Attachment>,
}

struct Supervisor<'a> {
    _profile: &'a ProfileLease,
    config: CommandConfig,
    registry: Registry,
    tmux: Manager,
    daemon: PathBuf,
    active: HashMap<SlotId, Active>,
}

impl<'a> Supervisor<'a> {
    fn open(
        profile: &'a ProfileLease,
        material: &IdentityMaterial,
        daemon: &Path,
        tmux: &Path,
    ) -> Result<Self> {
        let config = CommandConfig::load(profile.root())?;
        backend::paths::daemon(daemon)?;
        let registry = Registry::open(profile, material)?;
        let tmux = Manager::new(tmux, profile.root())?;
        let mut result = Self {
            _profile: profile,
            config,
            registry,
            tmux,
            daemon: daemon.to_owned(),
            active: HashMap::new(),
        };
        result.reconcile()?;
        Ok(result)
    }

    fn reconcile(&mut self) -> Result<()> {
        for slot in self.registry.slots()? {
            if !self.config.terminal_allowed(&slot.account)
                || matches!(slot.state, State::Retiring(_))
            {
                let native = match slot.state {
                    State::Prepared => None,
                    State::Ready(id) => Some(id),
                    State::Retiring(id) => id,
                };
                if !matches!(slot.state, State::Retiring(_)) {
                    self.registry.retiring(slot)?;
                }
                if let Ok(inventory) = self.tmux.inventory(slot, native)
                    && self.tmux.terminate(slot.id, inventory).is_ok()
                {
                    self.registry.retired(slot)?;
                }
            } else if slot.state == State::Prepared
                && let Ok(inventory) = self.tmux.inventory(slot, None)
            {
                self.registry.ready(slot, inventory.session, None)?;
            }
        }
        Ok(())
    }

    fn attach(
        &mut self,
        account: Account,
        selection: Selection,
        mode: Mode,
        rows: u16,
        cols: u16,
    ) -> Result<Admission> {
        self.authorize(account)?;
        if !(2..=250).contains(&rows) || !(2..=500).contains(&cols) {
            return Err(REQUEST);
        }
        let (slot, inventory) = self.select(account, selection, mode, rows, cols)?;
        let active = self.active.entry(slot).or_default();
        active.maintain()?;
        let writer = active.writer();
        let role = match (mode, writer) {
            (Mode::Writer, Some(writer)) => return Ok(Admission::Busy { slot, writer }),
            (Mode::Observe, Some(_)) => Role::Observer,
            (Mode::Observe, None) => return Err(STALE),
            (Mode::Takeover(expected), Some(writer)) if expected == writer => Role::Writer,
            (Mode::Takeover(_), _) => return Err(STALE),
            (Mode::Writer, None) => Role::Writer,
        };
        let taking_over = matches!(mode, Mode::Takeover(_));
        if !taking_over && active.links.len() + active.reaping.len() >= MAX_ATTACHMENTS {
            return Err(CAPACITY);
        }
        let checked = CheckedLaunch::for_role(
            &self.daemon,
            &self.tmux.tmux,
            &self.tmux.socket(slot),
            tmux::SESSION,
            role,
        )?;
        let claim = Claim {
            random: random()?,
            generation: self.registry.allocate()?,
        };
        if taking_over {
            active.fence_all();
            active.reap()?;
        }
        if !active.reaping.is_empty() {
            return Err(CLEANUP);
        }
        self.tmux
            .wait_clients(slot, inventory, active.links.len(), ClientChange::Departing)?;
        let io = match checked.launch_owned(claim.generation, rows, cols) {
            Ok(io) => io,
            Err(failure) => {
                if let Some(pending) = failure.pending {
                    active.retire(pending);
                }
                return Err(failure.error);
            }
        };
        if let Err(error) = self.tmux.wait_clients(
            slot,
            inventory,
            active.links.len() + 1,
            ClientChange::Arriving,
        ) {
            active.retire(io);
            return Err(error);
        }
        active.links.push(Link { claim, role, io });
        Ok(Admission::Attached { slot, claim })
    }

    fn select(
        &mut self,
        account: Account,
        selection: Selection,
        mode: Mode,
        rows: u16,
        cols: u16,
    ) -> Result<(SlotId, Inventory)> {
        let slots = self.registry.slots()?;
        let selected = slots.iter().find(|slot| {
            slot.account == account
                && match selection {
                    Selection::Default => slot.default,
                    Selection::Existing(id) => slot.id == id,
                    Selection::New => false,
                }
        });
        if let Some(slot) = selected {
            let State::Ready(id) = slot.state else {
                return Err(UNAVAILABLE);
            };
            return Ok((slot.id, self.tmux.inventory(*slot, Some(id))?));
        }
        if matches!(selection, Selection::Existing(_)) {
            return Err(MISSING);
        }
        if !matches!(mode, Mode::Writer) {
            return Err(STALE);
        }
        if slots.len() == MAX_SLOTS {
            return Err(CAPACITY);
        }
        let slot = Slot {
            id: SlotId(random()?),
            account,
            default: matches!(selection, Selection::Default),
            marker: random()?,
            state: State::Prepared,
            proof: None,
        };
        self.registry.prepare(slot)?;
        let inventory = self.tmux.create(slot, rows, cols)?;
        self.registry.ready(slot, inventory.session, None)?;
        Ok((slot.id, inventory))
    }

    fn authorize(&self, account: Account) -> Result<()> {
        if !self.config.terminal_allowed(&account) {
            return Err(DENIED);
        }
        Ok(())
    }

    fn owned_slot(&self, account: Account, slot: SlotId) -> Result<Slot> {
        self.authorize(account)?;
        self.registry
            .slots()?
            .into_iter()
            .find(|row| row.account == account && row.id == slot)
            .ok_or(MISSING)
    }

    fn owner_turn(&mut self, account: Account, slot: SlotId) -> Result<&mut Active> {
        let record = self.owned_slot(account, slot)?;
        if !matches!(record.state, State::Ready(_)) {
            return Err(UNAVAILABLE);
        }
        let active = self.active.get_mut(&slot).ok_or(STALE)?;
        active.detect_exit()?;
        Ok(active)
    }

    fn writer_turn(
        &mut self,
        account: Account,
        slot: SlotId,
        claim: Claim,
    ) -> Result<&mut Attachment> {
        let link = self.owner_turn(account, slot)?.link(claim)?;
        if link.role != Role::Writer {
            return Err(backend::READ_ONLY);
        }
        Ok(&mut link.io)
    }

    fn write(
        &mut self,
        account: Account,
        slot: SlotId,
        claim: Claim,
        bytes: &[u8],
    ) -> Result<WriteStatus> {
        if bytes.len() > 4096 {
            return Err(REQUEST);
        }
        self.writer_turn(account, slot, claim)?
            .write(claim.generation, bytes)
    }

    fn resize(
        &mut self,
        account: Account,
        slot: SlotId,
        claim: Claim,
        rows: u16,
        cols: u16,
    ) -> Result<()> {
        if !(2..=250).contains(&rows) || !(2..=500).contains(&cols) {
            return Err(REQUEST);
        }
        self.writer_turn(account, slot, claim)?
            .resize(claim.generation, rows, cols)
    }

    fn read(
        &mut self,
        account: Account,
        slot: SlotId,
        claim: Claim,
        out: &mut [u8],
    ) -> Result<ReadStatus> {
        if out.len() > 4096 {
            return Err(REQUEST);
        }
        self.owner_turn(account, slot)?.link(claim)?.io.read(out)
    }

    fn detach(&mut self, account: Account, slot: SlotId, claim: Claim) -> Result<()> {
        let active = self.owner_turn(account, slot)?;
        if active.link(claim)?.role == Role::Writer {
            active.fence_all();
        } else {
            let position = active
                .links
                .iter()
                .position(|link| link.claim == claim)
                .ok_or(STALE)?;
            active.fence(position);
        }
        active.reap()
    }
}

impl Drop for Supervisor<'_> {
    fn drop(&mut self) {
        for active in self.active.values_mut() {
            active.fence_all();
            let _ = active.reap();
        }
    }
}

impl Active {
    fn writer(&self) -> Option<Claim> {
        self.links
            .iter()
            .find(|link| link.role == Role::Writer)
            .map(|link| link.claim)
    }

    fn link(&mut self, claim: Claim) -> Result<&mut Link> {
        self.links
            .iter_mut()
            .find(|link| link.claim == claim)
            .ok_or(STALE)
    }

    fn fence(&mut self, index: usize) {
        let link = self.links.remove(index);
        self.retire(link.io);
    }

    fn fence_all(&mut self) {
        // A writer is always created first. Close observers before the writer.
        while let Some(link) = self.links.pop() {
            self.retire(link.io);
        }
    }

    fn retire(&mut self, mut io: Attachment) {
        drop(io.master.take());
        self.reaping.push(io);
    }

    fn detect_exit(&mut self) -> Result<()> {
        for index in (0..self.links.len()).rev() {
            let link = &self.links[index];
            let status = link.io.poll(false, None, Instant::now())?;
            if status.child_exited || status.hangup {
                if link.role == Role::Writer {
                    self.fence_all();
                    break;
                }
                self.fence(index);
            }
        }
        Ok(())
    }

    fn maintain(&mut self) -> Result<()> {
        self.detect_exit()?;
        self.reap()
    }

    fn reap(&mut self) -> Result<()> {
        let deadline = Instant::now() + REAP_TURN;
        while let Some(first) = self.reaping.first_mut() {
            if first.shutdown(deadline)? == Shutdown::Pending {
                return Err(CLEANUP);
            }
            self.reaping.remove(0);
        }
        Ok(())
    }
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| Error::Invalid("terminal_random"))?;
    Ok(bytes)
}
