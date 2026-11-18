//! Exercise actual local tmux effects from records delivered over a private WSS relay.
use super::super::SlotId;
use super::*;
use crate::identity::{IdentityMaterial, ProfileLease};
use crate::terminal_backend::supervisor::{Manager, Registry};
use crate::terminal_frames::{self as frames, Arrival, Data, Size};
use std::path::PathBuf;

use super::super::{
    LiveSupervisor, guard, io, recovery,
    tests::{event, send_input},
};

pub(crate) struct NativeProof {
    authority: LiveSupervisor,
    tokens: SourceTokens,
    manager: Manager,
    registry: Registry,
}

impl NativeProof {
    pub(crate) fn start(lease: Arc<ProfileLease>, material: &IdentityMaterial) -> Result<Self> {
        let daemon = PathBuf::from(
            std::env::var_os("MARMISH_TEST_DAEMON")
                .ok_or(Error::Invalid("fixture_set_marmish_test_daemon"))?,
        )
        .canonicalize()?;
        let tmux = std::env::var_os("MARMISH_TEST_TMUX")
            .map_or_else(|| PathBuf::from("/usr/bin/tmux"), PathBuf::from);
        let manager = Manager::new(&tmux, lease.root())?;
        let registry = Registry::open(&lease, material)?;
        let authority = LiveSupervisor::start(lease, material, &daemon, &tmux)?;
        Ok(Self {
            authority,
            tokens: SourceTokens::default(),
            manager,
            registry,
        })
    }

    pub(crate) async fn attach(
        &mut self,
        client: ReadyEnrollment,
        server: ReadyEnrollment,
        new: bool,
    ) -> Result<Option<NativePair>> {
        let request_record = Request {
            choice: if new { Choice::New } else { Choice::Regular },
            target: Target::Default,
            size: Size { rows: 24, cols: 80 },
        };
        let start = tokio::time::Instant::now();
        let (client, server) = tokio::try_join!(
            request(client, request_record, &|| true),
            accept(server, &self.authority.control, &mut self.tokens, &|| true),
        )?;
        match (client, server) {
            (
                ClientOutcome::Attached {
                    room: client,
                    role: Role::Writer,
                    slot,
                },
                ServerOutcome::Attached(server),
            ) => {
                assert!(SlotId(slot) == server.guard.admitted.slot);
                assert_eq!(
                    client.frame_binding().attachment,
                    server.room.frame_binding().attachment
                );
                assert_eq!(
                    client.frame_binding().generation,
                    server.room.frame_binding().generation
                );
                Ok(Some(NativePair {
                    client,
                    server,
                    admission: start.elapsed(),
                    output_sequence: 0,
                }))
            }
            (ClientOutcome::Busy(_), ServerOutcome::Busy) => Ok(None),
            (ClientOutcome::Refused(client), ServerOutcome::Refused(server)) => {
                assert_eq!(client, server);
                let error = match server {
                    Refusal::Permission => "fixture_selection_permission",
                    Refusal::Capacity => "fixture_selection_capacity",
                    _ => "fixture_selection_unavailable",
                };
                Err(Error::Invalid(error))
            }
            _ => Err(SOURCE),
        }
    }

    pub(crate) async fn rejected_selection(
        &mut self,
        mut client: ReadyEnrollment,
        server: ReadyEnrollment,
        fault: SelectionFault,
    ) -> Result<()> {
        let before = self.count()?;
        let admission = client.admission_ceiling();
        let session = Session {
            transcript: client.binding().transcript,
            transaction: random()?,
        };
        let early = matches!(fault, SelectionFault::Early);
        let grant = || true;
        let drive = async {
            if early {
                return send(
                    &mut client,
                    session,
                    Record::Confirm(Binding {
                        role: Role::Writer,
                        claim: [1; 16],
                        generation: NonZeroU64::MIN,
                    }),
                    admission,
                    &grant,
                )
                .await;
            }
            send(
                &mut client,
                session,
                Record::Request(Request {
                    choice: Choice::New,
                    target: Target::Default,
                    size: Size { rows: 24, cols: 80 },
                }),
                admission,
                &grant,
            )
            .await?;
            let (_, Record::Accepted { mut binding, .. }) = receive(
                &mut client,
                Some(session.transaction),
                Direction::FromServer,
                admission,
                &grant,
            )
            .await?
            else {
                return Err(INVALID);
            };
            assert_eq!(self.registry.slots()?.len(), before + 1);
            match fault {
                SelectionFault::WrongClaim => {
                    binding.claim[0] ^= 1;
                    send(
                        &mut client,
                        session,
                        Record::Confirm(binding),
                        admission,
                        &grant,
                    )
                    .await?;
                }
                SelectionFault::WrongRole => {
                    binding.role = Role::Observer;
                    send(
                        &mut client,
                        session,
                        Record::Confirm(binding),
                        admission,
                        &grant,
                    )
                    .await?;
                }
                SelectionFault::Missing => (),
                SelectionFault::Early => unreachable!(),
            }
            Ok(())
        };
        let (sent, result) = tokio::join!(
            drive,
            accept(server, &self.authority.control, &mut self.tokens, &grant),
        );
        sent?;
        match fault {
            SelectionFault::Early | SelectionFault::WrongClaim | SelectionFault::WrongRole => {
                assert!(matches!(
                    result,
                    Err(Error::Invalid("terminal_selection_record"))
                ));
            }
            SelectionFault::Missing => assert!(matches!(result, Err(Error::Timeout))),
        }
        client.close().await?;
        assert_eq!(self.count()?, if early { before } else { before + 1 });
        if !early {
            assert_eq!(before, 0, "failure matrix uses an empty fixture");
            let new = self.registry.slots()?.pop().ok_or(SOURCE)?;
            self.release(new.id.0).await?;
        }
        assert_eq!(self.count()?, before);
        Ok(())
    }

    pub(crate) fn count(&self) -> Result<usize> {
        Ok(self.registry.slots()?.len())
    }

    pub(crate) async fn release(&mut self, slot: [u8; 16]) -> Result<()> {
        let row = self
            .registry
            .slots()?
            .into_iter()
            .find(|entry| entry.id == SlotId(slot))
            .ok_or(SOURCE)?;
        let inventory = recovery::inventory(
            &mut self.registry,
            &mut self.manager,
            row,
            std::time::Instant::now() + Duration::from_secs(2),
        )?;
        self.manager.kill_session(
            row.id,
            inventory,
            std::time::Instant::now() + Duration::from_secs(2),
        )?;
        let limit = tokio::time::Instant::now() + Duration::from_secs(4);
        loop {
            let (reply, returned) = oneshot::channel();
            if self.authority.control.release(SlotId(slot), reply).is_ok()
                && matches!(timeout_at(limit, returned).await, Ok(Ok(Ok(()))))
            {
                return Ok(());
            }
            if tokio::time::Instant::now() >= limit {
                return Err(Error::Timeout);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub(crate) fn empty(&self) -> Result<bool> {
        Ok(self.registry.slots()?.is_empty())
    }
}

impl Drop for NativeProof {
    fn drop(&mut self) {
        self.authority.stop();
        let deadline = std::time::Instant::now() + Duration::from_secs(4);
        while self.authority.join_if_finished().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        // Fail-safe for a panic before normal exact release. This registry and
        // directory belong to the private fixture, and the manager rechecks
        // the owned pane proof before it terminates a session.
        for slot in self.registry.slots().unwrap_or_default() {
            let limit = std::time::Instant::now() + Duration::from_secs(2);
            if let Ok(inventory) =
                recovery::inventory(&mut self.registry, &mut self.manager, slot, limit)
            {
                let _ = self.manager.terminate(slot.id, inventory);
            }
        }
    }
}

pub(crate) enum SelectionFault {
    Early,
    WrongClaim,
    WrongRole,
    Missing,
}

pub(crate) struct NativePair {
    client: AttachedRoom,
    server: ServerAttachment,
    pub(crate) admission: Duration,
    output_sequence: u64,
}

impl NativePair {
    pub(crate) fn slot(&self) -> [u8; 16] {
        self.server.guard.admitted.slot.0
    }

    pub(crate) fn binding(&self) -> ([u8; 32], [u8; 16], u64) {
        let frames = self.client.frame_binding();
        (frames.transcript, frames.attachment, frames.generation)
    }

    pub(crate) async fn echo(
        &mut self,
        sequence: u64,
        input: Vec<u8>,
        marker: &[u8],
    ) -> Result<Duration> {
        self.server.check_live().await?;
        let start = tokio::time::Instant::now();
        let wanted = frames::Record::Ordered(sequence, Data::Input(input.clone()));
        let received = transfer(
            &mut self.client,
            &mut self.server.room,
            Direction::ToServer,
            &wanted,
        )
        .await?;
        let frames::Record::Ordered(remote, Data::Input(bytes)) = received else {
            return Err(INVALID);
        };
        assert_eq!(remote, sequence);
        assert_eq!(bytes, input);
        let deadline = guard::sample().ok_or(SOURCE)?.continuous + Duration::from_secs(4);
        send_input(
            self.server.guard.admitted(),
            io::Operation::new(remote, deadline, io::Input::Bytes(bytes)).ok_or(INVALID)?,
        )
        .await;
        let mut committed = false;
        let mut output = Vec::new();
        while !committed || !output.windows(marker.len()).any(|bytes| bytes == marker) {
            assert!(output.len() <= 64 * 1024);
            let claim = self.server.guard.admitted();
            let next = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if let Some(event) = claim.try_event() {
                        break event;
                    }
                    claim.changed().await;
                }
            })
            .await;
            let event = next.unwrap_or_else(|_| {
                panic!(
                    "native echo {sequence} timeout, committed={committed}, bytes={}, marker={}",
                    output.len(),
                    output.windows(marker.len()).any(|bytes| bytes == marker),
                )
            });
            match event {
                io::Event::Output(bytes) => output.extend_from_slice(&bytes),
                io::Event::Complete(io::Completion::Written(id, len)) => {
                    assert_eq!((id, len), (sequence, input.len()));
                    committed = true;
                }
                _ => return Err(SOURCE),
            }
        }
        let mut echoed = Vec::new();
        for part in output.chunks(frames::MAX_DATA) {
            self.output_sequence += 1;
            let received = transfer(
                &mut self.server.room,
                &mut self.client,
                Direction::FromServer,
                &frames::Record::Ordered(self.output_sequence, Data::Output(part.to_vec())),
            )
            .await?;
            let frames::Record::Ordered(_, Data::Output(bytes)) = received else {
                return Err(INVALID);
            };
            echoed.extend(bytes);
        }
        assert_eq!(echoed, output);
        assert!(echoed.windows(marker.len()).any(|bytes| bytes == marker));
        Ok(start.elapsed())
    }

    pub(crate) async fn resize(&mut self, sequence: u64, size: Size) -> Result<()> {
        self.server.check_live().await?;
        let received = transfer(
            &mut self.client,
            &mut self.server.room,
            Direction::ToServer,
            &frames::Record::Ordered(sequence, Data::Resize(size)),
        )
        .await?;
        let frames::Record::Ordered(remote, Data::Resize(remote_size)) = received else {
            return Err(INVALID);
        };
        assert_eq!(remote, sequence);
        assert!(remote_size == size);
        let deadline = guard::sample().ok_or(SOURCE)?.continuous + Duration::from_secs(4);
        send_input(
            self.server.guard.admitted(),
            io::Operation::new(
                remote,
                deadline,
                io::Input::Resize {
                    rows: size.rows,
                    cols: size.cols,
                },
            )
            .ok_or(INVALID)?,
        )
        .await;
        loop {
            match event(self.server.guard.admitted()).await {
                io::Event::Complete(io::Completion::Resized(id)) if id == sequence => return Ok(()),
                io::Event::Output(_) => (),
                _ => return Err(SOURCE),
            }
        }
    }

    pub(crate) async fn await_revocation(
        &mut self,
        owned_directory: &std::path::Path,
    ) -> Result<()> {
        let marker = owned_directory.join("retired-write");
        assert!(!marker.exists());
        let source = self.server.guard.admitted();
        let deadline = guard::sample().ok_or(SOURCE)?.continuous + Duration::from_secs(2);
        let payload = format!("printf leaked > '{}'", marker.display());
        let input = io::Operation::new(
            2,
            deadline,
            io::Input::Bytes(format!("{payload}\r").into_bytes()),
        )
        .ok_or(INVALID)?;
        // Submission itself may race delivery of the fence notification.
        // Only the physical owner can acknowledge an actual write.
        assert!(matches!(
            source.submit(input),
            Ok(()) | Err(io::PushError::Closed(Reason::Authority) | io::PushError::Contended(_))
        ));
        loop {
            match event(self.server.guard.admitted()).await {
                io::Event::Closed(Reason::Authority) => break,
                io::Event::Output(_) => (),
                _ => return Err(SOURCE),
            }
        }
        assert!(self.server.check_live().await.is_err());
        assert!(!marker.exists());
        Ok(())
    }

    pub(crate) async fn close(self) -> Result<()> {
        let Self { client, server, .. } = self;
        tokio::try_join!(client.close(), server.close())?;
        Ok(())
    }
}

async fn transfer(
    sender: &mut AttachedRoom,
    receiver: &mut AttachedRoom,
    direction: Direction,
    wanted: &frames::Record,
) -> Result<frames::Record> {
    let binding = sender.frame_binding();
    let author = sender.binding().local_terminal;
    sender
        .send_frame(frames::encode_app(&author, now()?, binding, direction, wanted).ok_or(INVALID)?)
        .await?;
    let app = receiver.receive_frame().await?;
    let accepted = frames::decode_app(
        &app.bytes,
        &author,
        binding,
        direction,
        Arrival {
            retention: Some(app.retention),
            wall: Some(now()?),
            monotonic: Some(guard::sample().ok_or(SOURCE)?.continuous),
        },
    )
    .ok_or(INVALID)?;
    Ok(accepted.record)
}
