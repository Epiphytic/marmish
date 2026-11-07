//! Streaming application protocol on the single leased MDK identity actor.
use super::*;
use crate::journal::{Owner, PublicationKey, StreamPublication};
use crate::stream_service::{
    LiveStream, Rate, StreamLocalRequest, StreamLocalResult, StreamRecord, key,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use marmish_stream::{
    codec::{self, Codec},
    wire as sw,
};
use std::time::{SystemTime, UNIX_EPOCH};

const STREAM_KIND: u64 = 30101;
const STREAM_TAG: [&str; 2] = ["marmish", "stream/1"];
fn invalid() -> Error {
    Error::Invalid("stream_protocol")
}
fn stream_result<T>(result: marmish_stream::Result<T>) -> Result<T> {
    result.map_err(|_| invalid())
}
fn fresh_stream_binding(binding: &sw::Binding) -> Result<()> {
    stream_result(
        sw::Request {
            binding: binding.clone(),
            sequence: 0,
            body: sw::RequestBody::Open {
                argv: vec![b"_".to_vec()],
                codec: Codec::None,
            },
        }
        .validate_at(now()?),
    )
}
fn stream_record(context: &AuthenticatedCommandContext, binding: sw::Binding) -> StreamRecord {
    StreamRecord {
        binding,
        group: context.mls_group().to_vec(),
        server_side: context.direction() == CommandDirection::Server,
        hello: false,
        copy_hello: false,
        open_digest: None,
        launch: crate::stream_service::StreamLaunch::default(),
        codec: Codec::None,
        state: "hello".into(),
        cancel: false,
        active: false,
        terminal: None,
        identity: None,
        child_disposition: None,
        revision: 0,
        sequence: 0,
        stdin_accepted: 0,
        stdin_delivered: 0,
        stdout_eof: None,
        stderr_eof: None,
        completed_at: None,
        last_exchange_digest: None,
        payload_drained: false,
        payload_expired: false,
        projection_invalidated: false,
    }
}
impl MdkOwner {
    pub(crate) fn stream_has_server_state(&self) -> Result<bool> {
        Ok(self
            .journal
            .stream_records()?
            .iter()
            .any(|r| r.server_side && r.open_digest.is_some()))
    }
    pub(crate) fn stream_cleanup_status(
        &self,
    ) -> Result<crate::stream_service::StreamCleanupStatus> {
        let mut status = crate::stream_service::StreamCleanupStatus::default();
        for record in self
            .journal
            .stream_records()?
            .into_iter()
            .filter(|r| r.server_side)
        {
            if record.active {
                status.active += 1;
            }
            if record.cancel && record.active {
                status.cancelling += 1;
            }
            if matches!(
                record.state.as_str(),
                "lost"
                    | "unknown_launch"
                    | "transport_uncertain"
                    | "reconciled_possible_live"
                    | "cleanup_uncertain"
            ) {
                status.uncertain += 1;
            }
        }
        for live in self.streams.live.values() {
            status.observation_pending |= live.preparing || live.operation_id.is_some();
            if let Some(runner) = &live.runner {
                status.owned_runners += 1;
                if !runner.finished() {
                    status.unfinished_runners += 1;
                }
            }
        }
        status.cleanup_ready = status.active == 0
            && status.cancelling == 0
            && status.uncertain == 0
            && status.unfinished_runners == 0
            && !status.observation_pending;
        status.stop_complete = status.cleanup_ready && self.stream_stop_confirmed()?;
        Ok(status)
    }
    pub(crate) fn stream_request_local(
        &mut self,
        request: StreamLocalRequest,
    ) -> Result<StreamLocalResult> {
        match request {
            StreamLocalRequest::Inspect {} => Ok(StreamLocalResult::Inspection {
                snapshot: Box::new(self.journal.stream_inspect()?),
            }),
            StreamLocalRequest::CopyReceiver { request } => {
                let retention = self
                    .commands
                    .as_ref()
                    .map(|service| service.copy_receipt_retention().as_secs())
                    .unwrap_or(marmish_stream::copy_protocol::MIN_RETENTION_SECS as u64);
                let local_client = super::command_adapter::fixed_hex::<32>(&self.identity)?;
                let response =
                    self.journal
                        .copy_local_request(*request, local_client, retention, now()?)?;
                Ok(StreamLocalResult::CopyReceiver {
                    response: Box::new(response),
                })
            }
            StreamLocalRequest::List { remote } => {
                let entries = self
                    .journal
                    .stream_records()?
                    .into_iter()
                    .filter(|r| {
                        remote.as_ref().is_none_or(|peer| {
                            peer == &hex::encode(if r.server_side {
                                r.binding.client
                            } else {
                                r.binding.server
                            })
                        })
                    })
                    .map(|r| r.entry())
                    .collect();
                Ok(StreamLocalResult::Entries { entries })
            }
            StreamLocalRequest::AdminCancel { stream } => {
                let mut found = false;
                for mut r in self.journal.stream_records()? {
                    if hex::encode(r.binding.stream) == stream && r.server_side {
                        found = true;
                        if r.terminal.is_some() && !r.active {
                            continue;
                        }
                        r.cancel = true;
                        r.revision = r.revision.saturating_add(1);
                        if let Some(runner) = self
                            .streams
                            .live
                            .get(&r.key())
                            .and_then(|live| live.runner.as_ref())
                        {
                            r.state = "cancelling".into();
                            runner.cancel();
                            self.journal.stream_save(&r)?;
                        } else if !matches!(
                            r.state.as_str(),
                            "launching" | "lost" | "unknown_launch"
                        ) {
                            r.state = "terminal".into();
                            r.terminal = Some(sw::Terminal::Cancelled);
                            self.journal.stream_finish(&mut r)?;
                        } else {
                            self.journal.stream_save(&r)?;
                        }
                    }
                }
                if !found {
                    return Err(Error::Invalid("stream_not_found"));
                }
                Ok(StreamLocalResult::Pending)
            }
            StreamLocalRequest::AdminReconcile {
                stream,
                acknowledge_possible_live,
            } => {
                if !acknowledge_possible_live {
                    return Err(Error::Invalid("stream_reconcile_confirmation"));
                }
                let mut found = false;
                for mut r in self.journal.stream_records()? {
                    if hex::encode(r.binding.stream) == stream
                        && matches!(r.state.as_str(), "lost" | "unknown_launch")
                    {
                        found = true;
                        r.state = "reconciled_possible_live".into();
                        self.journal.stream_finish(&mut r)?;
                    }
                }
                if !found {
                    return Err(Error::Invalid("stream_not_found"));
                }
                Ok(StreamLocalResult::Pending)
            }
            StreamLocalRequest::Frame {
                frame_base64,
                relays,
            } => {
                if frame_base64.len() > 45000 {
                    return Err(invalid());
                }
                let bytes = STANDARD.decode(frame_base64).map_err(|_| invalid())?;
                let request = stream_result(sw::decode_request(&bytes))?;
                if self.streams.stopping
                    && matches!(
                        request.body,
                        sw::RequestBody::Hello { .. }
                            | sw::RequestBody::Open { .. }
                            | sw::RequestBody::CopyHello { .. }
                            | sw::RequestBody::OpenCopy { .. }
                    )
                {
                    return Err(Error::Invalid("server_stopping"));
                }
                let b = &request.binding;
                self.command_can_prepare(b.server, &relays)?;
                let room = self
                    .journal
                    .command_rooms()?
                    .into_iter()
                    .find(|r| {
                        r.session == b.session
                            && r.peer == b.server
                            && r.direction == CommandDirection::Client
                    })
                    .ok_or(Error::Invalid("stream_room"))?;
                let context = self.command_context(&GroupId::new(room.group.clone()))?;
                if context.local_account() != b.client || context.peer_account() != b.server {
                    return Err(invalid());
                }
                let service = self.commands.as_ref().ok_or(invalid())?;
                if !service.can_open(&b.server) {
                    return Err(Error::Invalid("stream_unauthorized"));
                }
                self.check_stream_request(&request)?;
                let k = key(&room.group, &b.stream);
                let digest = sw::request_digest(&bytes);
                if self.journal.stream_get(&k)?.is_none() {
                    if matches!(request.body, sw::RequestBody::Exchange { .. }) {
                        return Err(Error::Invalid("stream_not_found"));
                    }
                    fresh_stream_binding(b)?;
                    self.journal
                        .stream_insert(&stream_record(&context, b.clone()))?;
                }
                if self
                    .journal
                    .stream_get(&k)?
                    .is_some_and(|r| r.projection_invalidated)
                {
                    return Err(Error::Invalid("stream_projection_invalidated"));
                }
                if self
                    .journal
                    .stream_get(&k)?
                    .is_some_and(|r| r.state == "transport_uncertain")
                    && !matches!(
                        request.body,
                        sw::RequestBody::Status | sw::RequestBody::Cancel
                    )
                {
                    return Err(Error::Invalid("stream_transport_uncertain"));
                }
                if let sw::RequestBody::Open { argv, codec } = &request.body {
                    let mut record = self.journal.stream_get(&k)?.ok_or(invalid())?;
                    let open_digest = stream_result(sw::open_digest(argv, *codec))?;
                    if record.open_digest.is_some_and(|old| old != open_digest) {
                        return Err(Error::Invalid("stream_open_conflict"));
                    }
                    record.open_digest = Some(open_digest);
                    if record.state == "hello" {
                        record.state = "opening".into();
                    }
                    self.journal.stream_save(&record)?;
                }
                if let sw::RequestBody::OpenCopy { launch, codec } = &request.body {
                    let mut record = self.journal.stream_get(&k)?.ok_or(invalid())?;
                    let open_digest = stream_result(launch.digest(*codec))?;
                    if record.open_digest.is_some_and(|old| old != open_digest) {
                        return Err(Error::Invalid("stream_open_conflict"));
                    }
                    record.open_digest = Some(open_digest);
                    record.launch = crate::stream_service::StreamLaunch::NativeCopy {
                        launch: launch.clone(),
                    };
                    record.codec = *codec;
                    if record.state == "hello" {
                        record.state = "opening".into();
                    }
                    self.journal.stream_save(&record)?;
                }
                if let Some((old, answer)) = self.journal.stream_slot(&k, &digest)? {
                    if old != bytes {
                        return Err(invalid());
                    }
                    if let Some(response) = answer {
                        if matches!(
                            request.body,
                            sw::RequestBody::Status | sw::RequestBody::Cancel
                        ) {
                            self.queue_stream_frame(&room.group, &k, &bytes)?;
                        }
                        let decoded = stream_result(sw::decode_response(&response))?;
                        if matches!(
                            decoded.body,
                            sw::ResponseBody::Exchange {
                                terminal: Some(_),
                                stdout: None,
                                stderr: None,
                                ..
                            }
                        ) && let Some(mut record) = self.journal.stream_get(&k)?
                        {
                            record.payload_drained = true;
                            record.completed_at.get_or_insert(now()?);
                            self.journal.stream_save(&record)?;
                            self.journal.stream_clear_slots(&k)?;
                        }
                        return Ok(StreamLocalResult::Frame {
                            frame_base64: STANDARD.encode(response),
                        });
                    }
                    self.queue_stream_frame(&room.group, &k, &bytes)?;
                    return Ok(StreamLocalResult::Pending);
                }
                self.journal
                    .stream_request_slot(&k, &digest, request.sequence, &bytes)?;
                self.queue_stream_frame(&room.group, &k, &bytes)?;
                Ok(StreamLocalResult::Pending)
            }
        }
    }
    fn check_stream_request(&self, request: &sw::Request) -> Result<()> {
        if request.sequence > i64::MAX as u64 {
            return Err(Error::Invalid("stream_sequence"));
        }
        if matches!(request.body, sw::RequestBody::Exchange { .. }) {
            if request.sequence == 0 {
                return Err(invalid());
            }
        } else if request.sequence != 0 {
            return Err(invalid());
        }
        Ok(())
    }
    pub(super) fn is_stream_message(event: &GroupEvent) -> bool {
        if let GroupEvent::MessageReceived { payload, .. } = event
            && let Ok(app) = cgka_traits::app_event::MarmotAppEvent::decode(payload)
        {
            return app.kind == STREAM_KIND
                && app.tags == vec![STREAM_TAG.map(str::to_owned).to_vec()];
        }
        false
    }
    pub(super) fn project_stream_message(&mut self, event: GroupEvent) -> Result<()> {
        let GroupEvent::MessageReceived {
            group_id,
            message_id,
            sender,
            sender_leaf_index,
            payload,
            ..
        } = event
        else {
            return Err(invalid());
        };
        let context = match self.command_context(&group_id) {
            Ok(context) => context,
            Err(Error::Mdk(_) | Error::Invalid(_)) => return self.reject_stream(&message_id),
            Err(error) => return Err(error),
        };
        let row = mdk(self.storage.get_message(&message_id))?;
        if row.group_id != group_id
            || !matches!(
                row.state,
                cgka_traits::MessageState::Sent | cgka_traits::MessageState::Processed
            )
            || sender.as_slice() != context.peer_account()
            || sender_leaf_index.is_none()
            || payload.len() > 36864
        {
            return self.reject_stream(&message_id);
        }
        if mdk(self.storage.list_pending_application_events())?.iter().any(|e|matches!(e,GroupEvent::AppMessageInvalidated{group_id:g,message_id:id,..} if g==&group_id&&id==&message_id)){return Ok(());}
        let app =
            cgka_traits::app_event::MarmotAppEvent::decode(&payload).map_err(|_| invalid())?;
        if app.pubkey != hex::encode(context.peer_account())
            || app.kind != STREAM_KIND
            || app.tags != vec![STREAM_TAG.map(str::to_owned).to_vec()]
        {
            return self.reject_stream(&message_id);
        }
        let result = match context.direction() {
            CommandDirection::Server => match sw::decode_request(app.content.as_bytes()) {
                Ok(request) => {
                    let result = self.project_stream_request(&context, request.clone());
                    let code = match result {
                        Err(Error::Invalid(
                            "stream_active_capacity"
                            | "stream_retained_capacity"
                            | "command_slot_capacity"
                            | "command_retained_capacity"
                            | "command_active_capacity",
                        )) => Some(sw::WireErrorCode::Capacity),
                        Err(Error::Invalid(
                            "stream_admission_closed"
                            | "command_admission_closed"
                            | "server_stopping",
                        )) => Some(sw::WireErrorCode::Closed),
                        _ => None,
                    };
                    if let Some(code) = code {
                        let digest =
                            sw::request_digest(&stream_result(sw::encode_request(&request))?);
                        self.reply_stream_error(&context, &request, digest, code)
                    } else {
                        result
                    }
                }
                Err(_) => Err(invalid()),
            },
            CommandDirection::Client => match sw::decode_response(app.content.as_bytes()) {
                Ok(response) => self.project_stream_response(&context, response),
                Err(_) => Err(invalid()),
            },
        };
        match result {
            Ok(()) => self.reject_stream(&message_id),
            Err(Error::Invalid(
                "stream_capacity" | "stream_slot_capacity" | "stream_publication_capacity",
            )) => Ok(()),
            Err(Error::Invalid(_)) => self.reject_stream(&message_id),
            Err(e) => Err(e),
        }
    }
    fn reject_stream(&self, id: &MessageId) -> Result<()> {
        mdk(self
            .storage
            .delete_pending_application_events(std::slice::from_ref(id)))
    }
    fn check_stream_binding(
        &self,
        context: &AuthenticatedCommandContext,
        b: &sw::Binding,
    ) -> Result<()> {
        let service = self.commands.as_ref().ok_or(invalid())?;
        let allowed = match context.direction() {
            CommandDirection::Server => {
                service.can_accept(&b.client)
                    && b.server == context.local_account()
                    && b.client == context.peer_account()
            }
            CommandDirection::Client => {
                service.can_open(&b.server)
                    && b.client == context.local_account()
                    && b.server == context.peer_account()
            }
        };
        if !allowed || b.session != context.session() {
            return Err(Error::Invalid("stream_unauthorized"));
        }
        Ok(())
    }
    fn project_stream_response(
        &mut self,
        context: &AuthenticatedCommandContext,
        response: sw::Response,
    ) -> Result<()> {
        self.check_stream_binding(context, &response.binding)?;
        let k = key(context.mls_group(), &response.binding.stream);
        if self
            .journal
            .stream_get(&k)?
            .is_some_and(|r| r.projection_invalidated)
        {
            return Err(invalid());
        }
        let Some((request_bytes, previous)) =
            self.journal.stream_slot(&k, &response.request_sha256)?
        else {
            return Err(invalid());
        };
        let request = stream_result(sw::decode_request(&request_bytes))?;
        if request.binding != response.binding || request.sequence != response.sequence {
            return Err(invalid());
        }
        if let Some(previous) = previous {
            let previous = stream_result(sw::decode_response(&previous))?;
            if !response_advances(&previous.body, &response.body) {
                return Ok(());
            }
        }
        let bytes = stream_result(sw::encode_response(&response))?;
        self.journal
            .stream_response_slot(&k, &response.request_sha256, &bytes)?;
        if let Some(mut r) = self.journal.stream_get(&k)? {
            match &response.body {
                sw::ResponseBody::Opened {
                    codec, open_sha256, ..
                } => {
                    r.codec = *codec;
                    r.open_digest = Some(*open_sha256);
                    if r.terminal.is_none() {
                        r.state = "running".into();
                    }
                }
                sw::ResponseBody::Exchange {
                    terminal,
                    cancel_requested,
                    stdin_accepted,
                    stdin_delivered,
                    stdout_eof,
                    stderr_eof,
                    ..
                } => {
                    r.stdin_accepted = *stdin_accepted;
                    r.stdin_delivered = *stdin_delivered;
                    r.stdout_eof = *stdout_eof;
                    r.stderr_eof = *stderr_eof;
                    r.cancel |= *cancel_requested;
                    if let Some(t) = terminal {
                        if terminal_advances(r.terminal, Some(*t)) {
                            r.terminal = Some(*t);
                        }
                        r.state = "terminal".into();
                        r.completed_at.get_or_insert(now()?);
                    }
                }
                _ => {}
            }
            self.journal.stream_save(&r)?;
        }
        Ok(())
    }
    fn project_stream_request(
        &mut self,
        context: &AuthenticatedCommandContext,
        request: sw::Request,
    ) -> Result<()> {
        self.check_stream_binding(context, &request.binding)?;
        self.check_stream_request(&request)?;
        let request_bytes = stream_result(sw::encode_request(&request))?;
        let digest = sw::request_digest(&request_bytes);
        let k = key(context.mls_group(), &request.binding.stream);
        let data = matches!(request.body, sw::RequestBody::Exchange { .. });
        let existing = self.journal.stream_get(&k)?;
        if existing.as_ref().is_some_and(|r| r.projection_invalidated) {
            return Err(invalid());
        }
        if self.streams.stopping && existing.is_none() {
            return Err(Error::Invalid("server_stopping"));
        }
        if self.streams.stopping
            && matches!(
                request.body,
                sw::RequestBody::Hello { .. }
                    | sw::RequestBody::Open { .. }
                    | sw::RequestBody::CopyHello { .. }
                    | sw::RequestBody::OpenCopy { .. }
            )
        {
            return self.reply_stream_error(context, &request, digest, sw::WireErrorCode::Closed);
        }
        if data && existing.is_none() {
            return Err(Error::Invalid("stream_not_found"));
        }
        if self.streams.live.len() >= 32 && !self.streams.live.contains_key(&k) {
            return Err(Error::Invalid("stream_capacity"));
        }
        let peer = self
            .streams
            .peer_rates
            .entry(request.binding.client)
            .or_insert_with(|| (Rate::new(32), Rate::new(8)));
        if !(if data {
            peer.0.take() && self.streams.data_rate.take()
        } else {
            peer.1.take() && self.streams.control_rate.take()
        }) {
            return Err(Error::Invalid("stream_capacity"));
        }
        let live = self
            .streams
            .live
            .entry(k.clone())
            .or_insert_with(LiveStream::new);
        if !(if data {
            live.data_rate.take()
        } else {
            live.control_rate.take()
        }) {
            return Err(Error::Invalid("stream_capacity"));
        }
        if matches!(request.body, sw::RequestBody::OpenCopy { .. }) {
            let record = self
                .journal
                .stream_get(&k)?
                .unwrap_or_else(|| stream_record(context, request.binding.clone()));
            if self
                .commands
                .as_ref()
                .and_then(|s| s.lifetime())
                .is_some_and(|l| l.expired().unwrap_or(true))
            {
                return Err(Error::Invalid("stream_admission_closed"));
            }
            let retention = self
                .commands
                .as_ref()
                .ok_or(invalid())?
                .copy_receipt_retention()
                .as_secs();
            let bytes = self.journal.admit_copy_open(
                &record,
                &request,
                &request_bytes,
                retention,
                now()?,
            )?;
            return self.queue_stream_frame(context.mls_group(), &k, &bytes);
        }
        let mut record = match self.journal.stream_get(&k)? {
            Some(r) => r,
            None => {
                fresh_stream_binding(&request.binding)?;
                let r = stream_record(context, request.binding.clone());
                self.journal.stream_insert(&r)?;
                r
            }
        };
        if record.binding != request.binding {
            return Err(invalid());
        }
        if data && let Some((old, Some(response))) = self.journal.stream_slot(&k, &digest)? {
            if old != request_bytes {
                return Err(invalid());
            }
            return self.queue_stream_frame(context.mls_group(), &k, &response);
        }
        self.journal
            .stream_request_slot(&k, &digest, request.sequence, &request_bytes)?;
        let body = match &request.body {
            sw::RequestBody::CopyHello { versions, codecs } => {
                if !versions.contains(&marmish_stream::copy_protocol::VERSION)
                    || !codecs.contains(&Codec::None)
                {
                    sw::ResponseBody::CopyRefused {
                        code: sw::CopyAdmissionError::Unsupported,
                        prior_binding: None,
                    }
                } else {
                    record.copy_hello = true;
                    sw::ResponseBody::CopyHello {
                        version: marmish_stream::copy_protocol::VERSION,
                        codecs: vec![Codec::None, Codec::Zlib],
                        frame_bytes: marmish_stream::MAX_BLOCK as u32,
                        window_bytes: marmish_stream::WINDOW_BYTES as u32,
                    }
                }
            }
            sw::RequestBody::OpenCopy { .. } => return Err(invalid()),
            sw::RequestBody::Hello { codecs } => {
                if !codecs.contains(&Codec::None) {
                    sw::ResponseBody::Error {
                        code: sw::WireErrorCode::Unsupported,
                    }
                } else {
                    record.hello = true;
                    sw::ResponseBody::Hello {
                        codecs: vec![Codec::None, Codec::Zlib],
                        frame_bytes: marmish_stream::MAX_BLOCK as u32,
                        window_bytes: marmish_stream::WINDOW_BYTES as u32,
                    }
                }
            }
            sw::RequestBody::Open { argv, codec } => {
                let open_digest = stream_result(sw::open_digest(argv, *codec))?;
                if record.cancel {
                    self.stream_snapshot(&record)?
                } else if record.open_digest.is_some_and(|old| old != open_digest) {
                    sw::ResponseBody::Error {
                        code: sw::WireErrorCode::Conflict,
                    }
                } else if matches!(
                    record.state.as_str(),
                    "lost" | "unknown_launch" | "transport_uncertain"
                ) {
                    self.stream_snapshot(&record)?
                } else if record.open_digest.is_some() {
                    sw::ResponseBody::Opened {
                        codec: record.codec,
                        open_sha256: open_digest,
                        disposition: sw::OpenDisposition::Existing,
                    }
                } else if !record.hello || record.state != "hello" {
                    sw::ResponseBody::Error {
                        code: sw::WireErrorCode::Uncertain,
                    }
                } else {
                    if self
                        .commands
                        .as_ref()
                        .and_then(|s| s.lifetime())
                        .is_some_and(|l| l.expired().unwrap_or(true))
                    {
                        return Err(Error::Invalid("stream_admission_closed"));
                    }
                    stream_result(request.validate_at(now()?))?;
                    record.open_digest = Some(open_digest);
                    record.launch =
                        crate::stream_service::StreamLaunch::Exec { argv: argv.clone() };
                    record.codec = *codec;
                    record.state = "preparing".into();
                    record.revision = record.revision.saturating_add(1);
                    self.journal.stream_admit(&mut record)?;
                    sw::ResponseBody::Opened {
                        codec: *codec,
                        open_sha256: open_digest,
                        disposition: sw::OpenDisposition::Created,
                    }
                }
            }
            sw::RequestBody::Cancel => {
                record.cancel = true;
                record.revision = record.revision.saturating_add(1);
                if record.terminal.is_some() && !record.active {
                    self.journal.stream_save(&record)?;
                    return self.reply_stream_snapshot(context, &request, &record, digest);
                }
                if let Some(runner) = self.streams.live.get(&k).and_then(|s| s.runner.as_ref()) {
                    runner.cancel();
                    record.state = "cancelling".into();
                    self.journal.stream_save(&record)?;
                } else if !matches!(
                    record.state.as_str(),
                    "launching" | "lost" | "unknown_launch"
                ) {
                    record.terminal = Some(sw::Terminal::Cancelled);
                    record.state = "terminal".into();
                    self.journal.stream_finish(&mut record)?;
                } else {
                    self.journal.stream_save(&record)?;
                }
                self.stream_snapshot(&record)?
            }
            sw::RequestBody::Status => self.stream_snapshot(&record)?,
            sw::RequestBody::Exchange { .. } => {
                self.exchange_stream_request(&k, &request, &mut record, digest)?
            }
        };
        self.journal.stream_save(&record)?;
        let response = sw::Response {
            binding: request.binding,
            sequence: request.sequence,
            request_sha256: digest,
            body,
        };
        let bytes = stream_result(sw::encode_response(&response))?;
        self.journal.stream_response_slot(&k, &digest, &bytes)?;
        self.queue_stream_frame(context.mls_group(), &k, &bytes)
    }
    fn exchange_stream_request(
        &mut self,
        k: &str,
        request: &sw::Request,
        record: &mut StreamRecord,
        digest: [u8; 32],
    ) -> Result<sw::ResponseBody> {
        let sw::RequestBody::Exchange {
            stdin,
            stdin_eof,
            stdout_ack,
            stderr_ack,
        } = &request.body
        else {
            return Err(invalid());
        };
        Ok(
            if self.streams.stopping && (stdin.is_some() || stdin_eof.is_some()) {
                sw::ResponseBody::Error {
                    code: sw::WireErrorCode::Closed,
                }
            } else if matches!(
                record.state.as_str(),
                "transport_uncertain" | "authority_revoked" | "lost" | "unknown_launch"
            ) {
                sw::ResponseBody::Error {
                    code: sw::WireErrorCode::Uncertain,
                }
            } else if record.payload_expired {
                sw::ResponseBody::Error {
                    code: sw::WireErrorCode::Closed,
                }
            } else if request.sequence == record.sequence && record.payload_drained {
                if record.last_exchange_digest == Some(digest) {
                    self.stream_snapshot(record)?
                } else {
                    sw::ResponseBody::Error {
                        code: sw::WireErrorCode::Conflict,
                    }
                }
            } else if request.sequence < record.sequence && record.terminal.is_some() {
                sw::ResponseBody::Error {
                    code: sw::WireErrorCode::Closed,
                }
            } else if request.sequence != record.sequence.saturating_add(1) {
                sw::ResponseBody::Error {
                    code: sw::WireErrorCode::Gap,
                }
            } else if let Some(runner) = self.streams.live.get(k).and_then(|s| s.runner.as_ref()) {
                let raw = stdin
                    .as_ref()
                    .map(|block| stream_result(codec::decode(block, record.codec)))
                    .transpose()?;
                match runner.exchange(
                    stdin
                        .as_ref()
                        .zip(raw.as_ref())
                        .map(|(b, raw)| (b.offset, raw.as_slice())),
                    *stdin_eof,
                    *stdout_ack,
                    *stderr_ack,
                ) {
                    Ok(_) => {}
                    Err(marmish_stream::Error::Eof) => {
                        // Rejected stdin is never acknowledged. Output ACKs and
                        // natural completion still progress independently.
                        stream_result(runner.exchange(None, None, *stdout_ack, *stderr_ack))?;
                    }
                    Err(_) => return Err(invalid()),
                }
                record.sequence = request.sequence;
                record.last_exchange_digest = Some(digest);
                self.journal.stream_save(record)?;
                self.stream_snapshot(record)?
            } else if record.terminal.is_some() {
                record.sequence = request.sequence;
                record.last_exchange_digest = Some(digest);
                self.stream_snapshot(record)?
            } else {
                return Err(Error::Invalid("stream_capacity"));
            },
        )
    }
    fn reply_stream_error(
        &mut self,
        context: &AuthenticatedCommandContext,
        request: &sw::Request,
        digest: [u8; 32],
        code: sw::WireErrorCode,
    ) -> Result<()> {
        if self
            .journal
            .stream_get(&key(context.mls_group(), &request.binding.stream))?
            .is_none()
        {
            return Err(Error::Invalid("stream_untracked_refusal"));
        }
        let bytes = stream_result(sw::encode_response(&sw::Response {
            binding: request.binding.clone(),
            sequence: request.sequence,
            request_sha256: digest,
            body: sw::ResponseBody::Error { code },
        }))?;
        self.journal.stream_response_slot(
            &key(context.mls_group(), &request.binding.stream),
            &digest,
            &bytes,
        )?;
        self.queue_stream_frame(
            context.mls_group(),
            &key(context.mls_group(), &request.binding.stream),
            &bytes,
        )
    }
    fn reply_stream_snapshot(
        &mut self,
        context: &AuthenticatedCommandContext,
        request: &sw::Request,
        r: &StreamRecord,
        digest: [u8; 32],
    ) -> Result<()> {
        let response = sw::Response {
            binding: request.binding.clone(),
            sequence: request.sequence,
            request_sha256: digest,
            body: self.stream_snapshot(r)?,
        };
        let bytes = stream_result(sw::encode_response(&response))?;
        self.journal
            .stream_response_slot(&r.key(), &digest, &bytes)?;
        self.queue_stream_frame(context.mls_group(), &r.key(), &bytes)
    }
    pub(super) fn stream_snapshot(&self, r: &StreamRecord) -> Result<sw::ResponseBody> {
        if let Some(runner) = self
            .streams
            .live
            .get(&r.key())
            .and_then(|s| s.runner.as_ref())
        {
            let s = runner.snapshot();
            if r.terminal.is_some() && s.terminal.is_none() {
                // A durable failure fence is not physical completion evidence.
                // Report uncertainty without inventing EOF or discarding output.
                return Ok(sw::ResponseBody::Error {
                    code: sw::WireErrorCode::Uncertain,
                });
            }
            return Ok(sw::ResponseBody::Exchange {
                stdin_accepted: s.stdin_accepted,
                stdin_delivered: s.stdin_delivered,
                stdin_closed: s.stdin_closed || r.cancel || r.terminal.is_some(),
                stdout: s
                    .stdout
                    .map(|b| stream_result(codec::encode(b.offset, &b.data, r.codec)))
                    .transpose()?,
                stderr: s
                    .stderr
                    .map(|b| stream_result(codec::encode(b.offset, &b.data, r.codec)))
                    .transpose()?,
                stdout_eof: s.stdout_eof,
                stderr_eof: s.stderr_eof,
                cancel_requested: r.cancel || s.cancel_requested,
                terminal: s.terminal.or(r.terminal),
            });
        }
        Ok(sw::ResponseBody::Exchange {
            stdin_accepted: r.stdin_accepted,
            stdin_delivered: r.stdin_delivered,
            stdin_closed: r.terminal.is_some() || r.cancel,
            stdout: None,
            stderr: None,
            stdout_eof: r.stdout_eof.or(r.terminal.map(|_| 0)),
            stderr_eof: r.stderr_eof.or(r.terminal.map(|_| 0)),
            cancel_requested: r.cancel,
            terminal: r.terminal,
        })
    }
    fn queue_stream_frame(&mut self, group: &[u8], k: &str, bytes: &[u8]) -> Result<()> {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid())?
            .as_millis()
            .try_into()
            .map_err(|_| invalid())?;
        let identity = self.identity.clone();
        self.journal
            .stream_enqueue_attempt(k, bytes, timestamp_ms, |created| {
                let event = cgka_traits::app_event::MarmotAppEvent::new(
                    identity,
                    created,
                    STREAM_KIND,
                    vec![STREAM_TAG.map(str::to_owned).to_vec()],
                    String::from_utf8(bytes.to_vec()).map_err(|_| invalid())?,
                );
                Ok(StreamPublication {
                    group: group.to_vec(),
                    event_id: event.id.clone(),
                    stream_key: k.into(),
                    payload: event.encode().map_err(|_| invalid())?,
                    state: "unsubmitted".into(),
                    native_id: None,
                    evidence: None,
                    intent: None,
                })
            })
    }
    pub(crate) fn progress_streams(&mut self) -> Result<()> {
        self.prepare_streams()?;
        self.apply_stream_preparations()?;
        self.observe_stream_runners()?;
        self.reclaim_stream_state()
    }
    fn copy_helper_context(
        &self,
        record: &StreamRecord,
        launch: &marmish_stream::copy_protocol::Launch,
    ) -> Result<marmish_stream::copy_protocol::HelperContext> {
        let metadata = crate::native_manifest::compiled_release_metadata();
        let mut expected_commit = [0; 20];
        hex::decode_to_slice(metadata.commit, &mut expected_commit)
            .map_err(|_| Error::Invalid("copy_build_metadata"))?;
        let context = marmish_stream::copy_protocol::HelperContext {
            context_version: 1,
            helper_protocol: marmish_stream::copy_protocol::VERSION,
            client: record.binding.client,
            server: record.binding.server,
            group: record.group.clone(),
            session: record.binding.session,
            stream: record.binding.stream,
            transfer: launch.transfer,
            direction: launch.direction,
            launch_digest: stream_result(launch.digest(record.codec))?,
            retention_seconds: self
                .commands
                .as_ref()
                .ok_or(invalid())?
                .copy_receipt_retention()
                .as_secs(),
            expected_version: metadata.version.into(),
            expected_target: metadata.target.into(),
            expected_commit,
        };
        context.validate().map_err(|_| invalid())?;
        Ok(context)
    }
    /// Only definitive failed bootstrap proves these local queues cannot launch.
    /// Cancel/Stop may already have committed inactivity before this sweep.
    fn retire_failed_stream_preparations(&mut self) -> Result<()> {
        if !self
            .commands
            .as_mut()
            .and_then(|s| s.engine_mut())
            .is_some_and(|e| e.readiness() == crate::command_engine::Readiness::Failed)
        {
            return Ok(());
        }
        let keys = self
            .streams
            .live
            .iter()
            .filter(|(_, live)| {
                live.runner.is_none() && (live.preparing || live.operation_id.is_some())
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            let record = self.journal.stream_get(&key)?;
            if record.as_ref().is_some_and(|r| r.active) {
                continue;
            }
            let stream = match record {
                Some(record) => record.binding.stream,
                None => {
                    // Parse only the stable local bookkeeping key, never an
                    // authentication identity. No durable record is synthesized.
                    let Some((_, encoded)) = key.rsplit_once(':') else {
                        continue;
                    };
                    let Ok(stream) = super::command_adapter::fixed_hex::<16>(encoded) else {
                        continue;
                    };
                    stream
                }
            };
            if self
                .commands
                .as_mut()
                .and_then(|s| s.engine_mut())
                .is_some_and(|e| e.retire_failed_bootstrap_live(stream))
                && let Some(live) = self.streams.live.get_mut(&key)
            {
                live.preparing = false;
                live.operation_id = None;
            }
        }
        Ok(())
    }
    // Caller has collected definitive OpenFailed. Every durable and authority
    // fence is refreshed before the transaction; queued IDs survive its failure.
    fn finish_failed_stream_preparation(&mut self, record: &StreamRecord) -> Result<()> {
        let k = record.key();
        let live = self.streams.live.get(&k);
        if live.is_some_and(|live| {
            live.runner.is_some()
                || ((live.preparing || live.operation_id.is_some())
                    && (!live.preparing
                        || live.operation_id.is_none()
                        || live.revision != record.revision))
        }) {
            return Ok(());
        }
        let Some(mut current) = self.journal.stream_get(&k)? else {
            return Ok(());
        };
        if !current.active
            || !current.server_side
            || current.state != "preparing"
            || current.cancel
            || current.projection_invalidated
            || current.revision != record.revision
            || current.binding != record.binding
            || current.group != record.group
        {
            return Ok(());
        }
        let context = match self.command_context(&GroupId::new(current.group.clone())) {
            Ok(context) => context,
            Err(_) => return Ok(()),
        };
        if self
            .check_stream_binding(&context, &current.binding)
            .is_err()
            || !self.journal.copy_attempt_matches(&current)?
        {
            return Ok(());
        }
        current.state = "terminal".into();
        current.terminal = Some(sw::Terminal::Failed {
            code: sw::WireErrorCode::Invalid,
        });
        // On transaction failure retain all queued IDs for the next tick.
        self.journal.stream_finish(&mut current)?;
        if self
            .commands
            .as_mut()
            .and_then(|s| s.engine_mut())
            .is_some_and(|e| e.retire_failed_bootstrap_live(current.binding.stream))
            && let Some(live) = self.streams.live.get_mut(&k)
        {
            live.preparing = false;
            live.operation_id = None;
        }
        Ok(())
    }
    fn prepare_streams(&mut self) -> Result<()> {
        self.retire_failed_stream_preparations()?;
        if self.streams.stopping {
            return Ok(());
        }
        for record in self.journal.stream_active_server_records()? {
            let k = record.key();
            if record.state != "preparing" || record.cancel || record.projection_invalidated {
                continue;
            }
            let context = match self.command_context(&GroupId::new(record.group.clone())) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if self
                .check_stream_binding(&context, &record.binding)
                .is_err()
            {
                continue;
            }
            if !self.journal.copy_attempt_matches(&record)? {
                continue;
            }
            if self
                .commands
                .as_mut()
                .and_then(|s| s.engine_mut())
                .is_some_and(|e| e.readiness() == crate::command_engine::Readiness::Failed)
            {
                self.finish_failed_stream_preparation(&record)?;
                continue;
            }
            let kind = match &record.launch {
                crate::stream_service::StreamLaunch::Exec { argv } => {
                    crate::command_engine::LivePrepareKind::Exec { argv: argv.clone() }
                }
                crate::stream_service::StreamLaunch::NativeCopy { launch } => {
                    crate::command_engine::LivePrepareKind::NativeCopy {
                        context: Box::new(self.copy_helper_context(&record, launch)?),
                    }
                }
            };
            let live = self.streams.live.entry(k).or_insert_with(LiveStream::new);
            if live.preparing || live.runner.is_some() {
                continue;
            }
            let operation_id =
                super::command_adapter::fixed_hex::<16>(&crate::foundation::fresh_id(now()?)?)?;
            let request = crate::command_engine::LivePrepareRequest {
                operation_id,
                stream_id: record.binding.stream,
                revision: record.revision,
                owner: Owner::from_context(&context),
                kind,
            };
            if let Some(engine) = self.commands.as_mut().and_then(|s| s.engine_mut())
                && engine.try_prepare_live(request).is_ok()
            {
                live.preparing = true;
                live.operation_id = Some(operation_id);
                live.revision = record.revision;
            }
        }
        Ok(())
    }
    fn apply_stream_preparations(&mut self) -> Result<()> {
        loop {
            let completion = self
                .commands
                .as_mut()
                .and_then(|s| s.engine_mut())
                .and_then(|e| e.take_live_prepared());
            let Some(completion) = completion else {
                break;
            };
            let record_key = self
                .streams
                .live
                .iter()
                .find(|(_, live)| live.operation_id == Some(completion.operation_id))
                .map(|(k, _)| k.clone());
            let record = match record_key {
                Some(k) => self.journal.stream_get(&k)?,
                None => None,
            };
            let Some(mut record) = record else {
                continue;
            };
            let k = record.key();
            if let Some(live) = self.streams.live.get_mut(&k) {
                live.preparing = false;
                live.operation_id = None;
            }
            if self.streams.stopping
                || record.revision != completion.revision
                || record.cancel
                || record.projection_invalidated
                || record.state != "preparing"
            {
                continue;
            }
            let context = match self.command_context(&GroupId::new(record.group.clone())) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if self
                .check_stream_binding(&context, &record.binding)
                .is_err()
            {
                continue;
            }
            if !self.journal.copy_attempt_matches(&record)? {
                continue;
            }
            match completion.result {
                Ok(prepared) => {
                    record.state = "launching".into();
                    self.journal.stream_save(&record)?;
                    match crate::stream_runner::RunnerHandle::start(
                        prepared,
                        crate::stream_runner::RunnerLimits {
                            drain_grace: Duration::from_secs(2),
                            cancel_grace: Duration::from_secs(2),
                            poll_interval: Duration::from_millis(5),
                        },
                    ) {
                        Ok(runner) => {
                            self.streams
                                .live
                                .entry(k)
                                .or_insert_with(LiveStream::new)
                                .runner = Some(runner);
                        }
                        Err(_) => {
                            record.state = "unknown_launch".into();
                            record.terminal = Some(sw::Terminal::Failed {
                                code: sw::WireErrorCode::Uncertain,
                            });
                            self.journal.stream_save(&record)?;
                        }
                    }
                }
                Err(error) => {
                    let code = match error {
                        marmish_engine::EngineError::CommandNotFound(_) => {
                            sw::WireErrorCode::NotFound
                        }
                        marmish_engine::EngineError::CommandDenied
                        | marmish_engine::EngineError::Unauthorized => sw::WireErrorCode::Denied,
                        marmish_engine::EngineError::WorkerUnavailable
                            if matches!(
                                record.launch,
                                crate::stream_service::StreamLaunch::NativeCopy { .. }
                            ) =>
                        {
                            sw::WireErrorCode::Unsupported
                        }
                        marmish_engine::EngineError::QuotaExceeded => sw::WireErrorCode::Capacity,
                        _ => sw::WireErrorCode::Invalid,
                    };
                    record.terminal = Some(sw::Terminal::Failed { code });
                    record.state = "terminal".into();
                    self.journal.stream_finish(&mut record)?;
                }
            }
        }
        Ok(())
    }
    fn observe_stream_runners(&mut self) -> Result<()> {
        let runner_keys: Vec<_> = self
            .streams
            .live
            .iter()
            .filter(|(_, live)| live.runner.is_some())
            .map(|(key, _)| key.clone())
            .collect();
        for key in runner_keys {
            let Some(mut record) = self.journal.stream_get(&key)? else {
                return Err(invalid());
            };
            let authorized = !record.projection_invalidated
                && self
                    .command_context(&GroupId::new(record.group.clone()))
                    .ok()
                    .is_some_and(|context| {
                        self.check_stream_binding(&context, &record.binding).is_ok()
                    });
            let Some(live) = self.streams.live.get_mut(&record.key()) else {
                continue;
            };
            let Some(runner) = &live.runner else {
                continue;
            };
            if !authorized {
                runner.suspend_input();
                if record.active
                    && !record.projection_invalidated
                    && record.state != "authority_revoked"
                {
                    record.state = "authority_revoked".into();
                    self.journal.stream_save(&record)?;
                }
            }
            if !live.identity_committed
                && let Some(identity) = runner.snapshot().identity
            {
                record.identity = Some(
                    serde_json::to_vec(&identity).map_err(|_| Error::Storage("stream_identity"))?,
                );
                if !record.cancel && authorized && !self.streams.stopping {
                    record.state = "running".into();
                }
                self.journal.stream_save(&record)?;
                live.identity_committed = true;
                if authorized {
                    runner.permit_input();
                }
            }
            if record.cancel {
                runner.cancel();
            }
            let snapshot = runner.snapshot();
            if let Some(terminal) = snapshot.terminal
                && record.active
            {
                record.stdin_accepted = snapshot.stdin_accepted;
                record.stdin_delivered = snapshot.stdin_delivered;
                record.stdout_eof = snapshot.stdout_eof;
                record.stderr_eof = snapshot.stderr_eof;
                record.terminal = Some(terminal);
                if snapshot.uncertain {
                    record.state = if record.identity.is_some() {
                        "lost"
                    } else {
                        "unknown_launch"
                    }
                    .into();
                    self.journal.stream_save(&record)?;
                } else {
                    record.state = if self.streams.stopping {
                        "server_stopped"
                    } else {
                        "terminal"
                    }
                    .into();
                    self.journal.stream_finish(&mut record)?;
                }
            }
            if record.terminal.is_some()
                && !record.active
                && runner.finished()
                && snapshot.stdout.is_none()
                && snapshot.stderr.is_none()
                && !record.payload_drained
            {
                record.payload_drained = true;
                self.journal.stream_save(&record)?;
            }
        }
        Ok(())
    }
    fn reclaim_stream_state(&mut self) -> Result<()> {
        if let Some(done) = self
            .commands
            .as_mut()
            .and_then(|s| s.engine_mut())
            .and_then(|e| e.take_copy_gc())
        {
            self.journal
                .copy_gc_complete(&done.key, done.revision, done.cleaned)?;
        }

        let mut expired = std::collections::BTreeSet::new();
        if !self.streams.stopping
            && self
                .streams
                .last_maintenance
                .is_none_or(|last| last.elapsed() >= std::time::Duration::from_secs(30))
        {
            self.journal.stream_maintain(now()?)?;
            if self
                .commands
                .as_ref()
                .is_some_and(|s| s.engine_present_readonly())
                && let Some((key, attempt)) = self.journal.copy_gc_candidate(now()?)?
            {
                let service = self.commands.as_mut().ok_or(invalid())?;
                let request = crate::command_engine::CopyGcRequest {
                    key: key.clone(),
                    revision: attempt.revision,
                    launch: attempt.launch,
                    stream: attempt.binding.stream,
                    cwd: service.copy_cwd().to_owned(),
                    timestamp: i64::try_from(now()?).map_err(|_| invalid())?,
                };
                if service
                    .engine_mut()
                    .ok_or(invalid())?
                    .try_copy_gc(request)
                    .is_err()
                {
                    self.journal
                        .copy_gc_complete(&key, attempt.revision, false)?;
                }
            }

            // Only live handles can retain expired output. Keep this lookup
            // bounded by the live limit rather than decoding the full ledger.
            for k in self.streams.live.keys() {
                if self
                    .journal
                    .stream_get(k)?
                    .is_some_and(|r| r.payload_expired && !r.active)
                {
                    expired.insert(k.clone());
                }
            }
            self.streams.last_maintenance = Some(std::time::Instant::now());
        }
        self.streams.live.retain(|k, live| {
            if expired.contains(k) && live.runner.as_ref().is_some_and(|r| r.finished()) {
                return false;
            }
            live.preparing
                || live.runner.as_ref().is_some_and(|runner| {
                    let snapshot = runner.snapshot();
                    !runner.finished() || snapshot.stdout.is_some() || snapshot.stderr.is_some()
                })
        });
        Ok(())
    }
    /// Explicit service shutdown starts with output/control grace, never an
    /// attachment-disconnect policy. Close admission before capturing barriers.
    pub(crate) fn stream_begin_stop(&mut self) -> Result<()> {
        if self.streams.stopping {
            return Ok(());
        }
        self.streams.stopping = true;
        if let Some(service) = self.commands.as_mut() {
            service.close_admission();
        }
        for mut record in self.journal.stream_records()? {
            if record.server_side
                && record.active
                && !matches!(
                    record.state.as_str(),
                    "lost" | "unknown_launch" | "launching"
                )
            {
                record.revision = record.revision.saturating_add(1);
                record.state = "server_stopping".into();
                self.journal.stream_save(&record)?;
            }
        }
        Ok(())
    }
    pub(crate) fn stream_runner_barriers(&self) -> Vec<crate::stream_runner::RunnerHandle> {
        self.streams
            .live
            .values()
            .filter_map(|live| live.runner.clone())
            .collect()
    }
    pub(crate) fn stream_cancel_stop(&mut self) -> Result<()> {
        for mut record in self.journal.stream_records()? {
            if !record.server_side || !record.active {
                continue;
            }
            if let Some(runner) = self
                .streams
                .live
                .get(&record.key())
                .and_then(|live| live.runner.as_ref())
            {
                record.cancel = true;
                record.revision = record.revision.saturating_add(1);
                if !matches!(record.state.as_str(), "lost" | "unknown_launch") {
                    record.state = "server_stopping".into();
                }
                self.journal.stream_save(&record)?;
                runner.cancel();
            } else if !matches!(
                record.state.as_str(),
                "lost" | "unknown_launch" | "launching"
            ) {
                record.cancel = true;
                record.state = "server_stopped".into();
                record.terminal = Some(sw::Terminal::Cancelled);
                self.journal.stream_finish(&mut record)?;
            }
        }
        Ok(())
    }
    /// Do not call the full identity tick: it also advances finite-job launches.
    /// Admission is already closed, while existing remote ACK/Cancel can drain.
    pub(crate) async fn stream_stop_progress(&mut self, deadline: Instant) -> Result<()> {
        self.progress_streams()?;
        if self.active && Instant::now() < deadline {
            let effects = tokio::time::timeout_at(deadline, self.runtime.drain())
                .await
                .map_err(|_| Error::Timeout)?;
            self.apply_effects(mdk(effects)?, deadline).await?;
            self.receive_one(deadline).await?;
        }
        self.progress_streams()?;
        self.stream_stop_publish(deadline).await
    }
    pub(crate) async fn stream_stop_publish(&mut self, deadline: Instant) -> Result<()> {
        for record in self.journal.stream_records()? {
            if !record.server_side || record.projection_invalidated {
                continue;
            }
            let context = match self.command_context(&GroupId::new(record.group.clone())) {
                Ok(context) => context,
                Err(_) => continue,
            };
            if self
                .check_stream_binding(&context, &record.binding)
                .is_err()
            {
                continue;
            }
            for bytes in self.journal.stream_requests(&record.key())? {
                if Instant::now() >= deadline {
                    return Ok(());
                }
                let request = match sw::decode_request(&bytes) {
                    Ok(request) => request,
                    Err(_) => continue,
                };
                if matches!(
                    request.body,
                    sw::RequestBody::Status
                        | sw::RequestBody::Cancel
                        | sw::RequestBody::Exchange { .. }
                ) {
                    self.reply_stream_snapshot(
                        &context,
                        &request,
                        &record,
                        sw::request_digest(&bytes),
                    )?;
                }
            }
        }
        if Instant::now() < deadline {
            self.submit_stream_publications(deadline).await?;
        }
        Ok(())
    }
    pub(crate) fn stream_stop_confirmed(&self) -> Result<bool> {
        Ok(self.stream_runners_finished()
            && !self
                .journal
                .stream_records()?
                .iter()
                .any(|record| record.server_side && record.active))
    }
    pub(crate) fn stream_runners_finished(&self) -> bool {
        self.streams
            .live
            .values()
            .all(|live| live.runner.as_ref().is_none_or(|runner| runner.finished()))
    }
    pub(crate) fn streams_idle(&self) -> Result<bool> {
        self.journal.streams_idle()
    }
    pub(super) async fn submit_stream_publications(&mut self, deadline: Instant) -> Result<()> {
        let mut publications = self
            .journal
            .stream_publications()?
            .into_iter()
            .map(|p| {
                self.journal
                    .stream_publication_is_cancel(&p)
                    .map(|cancel| (!cancel, p))
            })
            .collect::<Result<Vec<_>>>()?;
        publications.sort_by_key(|(ordinary, _)| *ordinary);
        for (_, previous) in publications {
            let Some(mut p) = self
                .journal
                .stream_publication(&previous.group, &previous.event_id)?
            else {
                continue;
            };
            if self.reconcile_stream_publication(&mut p)? {
                continue;
            }
            let group = GroupId::new(p.group.clone());
            if p.state != "unsubmitted" {
                continue;
            }
            if self
                .journal
                .stream_get(&p.stream_key)?
                .is_some_and(|r| r.projection_invalidated)
            {
                self.journal
                    .stream_publication_state(&p.group, &p.event_id, "retired_failed")?;
                continue;
            }
            let context = match self.command_context(&group) {
                Ok(c) => c,
                Err(_) => {
                    self.journal
                        .stream_publication_state(&p.group, &p.event_id, "failed")?;
                    continue;
                }
            };
            let app = cgka_traits::app_event::MarmotAppEvent::decode(&p.payload)
                .map_err(|_| invalid())?;
            if app.id != p.event_id
                || app.pubkey != self.identity
                || app.kind != STREAM_KIND
                || app.tags != vec![STREAM_TAG.map(str::to_owned).to_vec()]
            {
                return Err(invalid());
            }
            let binding = if context.direction() == CommandDirection::Client {
                stream_result(sw::decode_request(app.content.as_bytes()))?.binding
            } else {
                stream_result(sw::decode_response(app.content.as_bytes()))?.binding
            };
            if self.check_stream_binding(&context, &binding).is_err() {
                self.journal
                    .stream_publication_state(&p.group, &p.event_id, "failed")?;
                continue;
            }
            if Instant::now() >= deadline {
                break;
            }
            self.journal
                .stream_publication_state(&p.group, &p.event_id, "prepared")?;
            let effects = match tokio::time::timeout_at(
                deadline,
                self.runtime.send(SendIntent::AppMessage {
                    group_id: group,
                    payload: p.payload,
                    expected_epoch: None,
                }),
            )
            .await
            {
                Ok(Ok(effects)) => effects,
                _ => {
                    self.journal
                        .stream_publication_state(&p.group, &p.event_id, "uncertain")?;
                    self.stream_publication_failed(&p.stream_key)?;
                    continue;
                }
            };
            for queued in &effects.queued {
                if queued.group_id.as_slice() != p.group {
                    return Err(invalid());
                }
                self.journal.stream_publication_queued(
                    &p.group,
                    &p.event_id,
                    queued.intent_id.as_slice(),
                )?;
            }
            self.apply_effects(effects, deadline).await?;
        }
        Ok(())
    }
    fn reconcile_stream_publication(&mut self, p: &mut StreamPublication) -> Result<bool> {
        let fanouts = mdk(self.storage.list_outbound_fanouts())?;
        let group = GroupId::new(p.group.clone());
        if p.state == "retired_failed" {
            return Ok(true);
        }
        if p.state == "failed" {
            let retained = fanouts.iter().any(|f| {
                f.group_id() == Some(&group)
                    && f.application_message()
                        .is_some_and(|a| a.app_event_id == p.event_id)
            });
            let queued = mdk(self.storage.list_queued_outbound_intents(&group))?;
            let retained_intent = p.intent.as_ref().map_or(!queued.is_empty(), |intent| {
                queued.iter().any(|q| q.id.as_slice() == intent)
            });
            if !retained && !retained_intent {
                self.journal
                    .stream_publication_state(&p.group, &p.event_id, "retired_failed")?;
                return Ok(true);
            }
        }
        if p.state == "uncertain" {
            self.stream_publication_failed(&p.stream_key)?;
        }
        // MDK's durable fanout is authoritative recovery evidence when a
        // crash occurred before the published effect reached our journal.
        if p.evidence.is_none()
            && let Some(fanout) = fanouts.iter().find(|f| {
                f.group_id() == Some(&group)
                    && f.application_message()
                        .is_some_and(|app| app.app_event_id == p.event_id)
            })
            && let Some(app) = fanout.application_message()
        {
            let published = marmot_session::PublishedApplicationMessage {
                group_id: app.group_id.clone(),
                app_event_id: app.app_event_id.clone(),
                message_id: fanout
                    .published_message_id()
                    .unwrap_or_else(|| fanout.message_id())
                    .clone(),
                source_epoch: app.source_epoch,
                retention: app.retention,
                authority: app.authority,
            };
            self.journal.stream_publication_published(&published)?;
            p.native_id = Some(published.message_id.as_slice().to_vec());
            p.evidence =
                Some(crate::command_evidence::PublishedEvidence::from_native(&published).encode()?);
        }
        if p.evidence.is_some() {
            if let (Some(native), Some(evidence)) = (&p.native_id, &p.evidence) {
                let evidence = crate::command_evidence::PublishedEvidence::decode(evidence)?;
                let published = evidence.to_native(
                    &PublicationKey {
                        group: p.group.clone(),
                        app_event_id: p.event_id.clone(),
                    },
                    native,
                );
                mdk(self
                    .runtime
                    .acknowledge_published_app_messages(&[published]))?;
                let retained = mdk(self.storage.list_outbound_fanouts())?.iter().any(|f| {
                    f.group_id() == Some(&group)
                        && f.application_message()
                            .is_some_and(|a| a.app_event_id == p.event_id)
                });
                if !retained {
                    self.journal
                        .stream_publication_state(&p.group, &p.event_id, "acked")?;
                }
            }
            return Ok(true);
        }
        if p.state == "prepared" || p.state == "queued" {
            let retained = fanouts.iter().any(|f| {
                f.group_id() == Some(&group)
                    && f.application_message()
                        .is_some_and(|a| a.app_event_id == p.event_id)
            });
            if !retained && mdk(self.storage.list_queued_outbound_intents(&group))?.is_empty() {
                self.journal
                    .stream_publication_state(&p.group, &p.event_id, "uncertain")?;
                self.stream_publication_failed(&p.stream_key)?;
            }
            return Ok(true);
        }
        if p.state == "failed" || p.state == "uncertain" {
            return Ok(true);
        }
        Ok(false)
    }
    fn stream_publication_failed(&mut self, k: &str) -> Result<()> {
        if let Some(mut record) = self.journal.stream_get(k)?
            && record.terminal.is_none()
            && !record.projection_invalidated
            && !matches!(
                record.state.as_str(),
                "terminal" | "lost" | "unknown_launch" | "transport_uncertain"
            )
        {
            record.state = "transport_uncertain".into();
            record.terminal = Some(sw::Terminal::Failed {
                code: sw::WireErrorCode::Uncertain,
            });
            self.journal.stream_save(&record)?;
        }
        Ok(())
    }
    pub(super) fn invalidate_stream_projection(&mut self, group: &[u8]) -> Result<()> {
        for mut record in self.journal.stream_records()? {
            if record.group != group || record.projection_invalidated {
                continue;
            }
            record.projection_invalidated = true;
            record.revision = record.revision.saturating_add(1);
            if record.active
                && !matches!(
                    record.state.as_str(),
                    "lost" | "unknown_launch" | "launching"
                )
            {
                record.state = "projection_invalidated".into();
            }
            if let Some(runner) = self
                .streams
                .live
                .get(&record.key())
                .and_then(|live| live.runner.as_ref())
            {
                runner.suspend_input();
            }
            self.journal.stream_save(&record)?;
        }
        Ok(())
    }
    pub(super) fn project_stream_publication_effects(
        &mut self,
        effects: &AccountDeviceEffects,
    ) -> Result<()> {
        for p in &effects.published_app_messages {
            if self
                .journal
                .stream_publication_owned(p.group_id.as_slice(), &p.app_event_id)?
            {
                self.journal.stream_publication_published(p)?;
            }
        }
        for p in &effects.failed_app_messages {
            if self
                .journal
                .stream_publication_owned(p.group_id.as_slice(), &p.app_event_id)?
            {
                self.journal.stream_publication_state(
                    p.group_id.as_slice(),
                    &p.app_event_id,
                    "failed",
                )?;
            }
        }
        for event in &effects.events {
            match event {
                GroupEvent::QueuedIntentRefused {
                    group_id,
                    intent_id,
                    ..
                } => self
                    .journal
                    .stream_publication_refused(group_id.as_slice(), intent_id.as_slice())?,
                GroupEvent::AppMessageInvalidated {
                    group_id,
                    message_id,
                    ..
                } => {
                    self.invalidate_stream_projection(group_id.as_slice())?;
                    self.journal.stream_publication_invalidated(
                        group_id.as_slice(),
                        message_id.as_slice(),
                    )?;
                    for fanout in mdk(self.storage.list_outbound_fanouts())? {
                        if fanout.group_id() == Some(group_id)
                            && fanout.outcome().message_id == *message_id
                            && let Some(app) = fanout.application_message()
                            && self
                                .journal
                                .stream_publication_owned(group_id.as_slice(), &app.app_event_id)?
                        {
                            self.journal.stream_publication_state(
                                group_id.as_slice(),
                                &app.app_event_id,
                                "failed",
                            )?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}

fn terminal_advances(previous: Option<sw::Terminal>, next: Option<sw::Terminal>) -> bool {
    previous.is_none()
        || previous == next
        || (matches!(
            previous,
            Some(sw::Terminal::Failed {
                code: sw::WireErrorCode::Uncertain
            })
        ) && next.is_some())
}
fn response_advances(previous: &sw::ResponseBody, next: &sw::ResponseBody) -> bool {
    match (previous, next) {
        (
            sw::ResponseBody::Exchange {
                stdin_accepted: a,
                stdin_delivered: d,
                stdin_closed: closed,
                cancel_requested: c,
                terminal: t,
                ..
            },
            sw::ResponseBody::Exchange {
                stdin_accepted: na,
                stdin_delivered: nd,
                stdin_closed: next_closed,
                cancel_requested: nc,
                terminal: nt,
                ..
            },
        ) => {
            na >= a
                && nd >= d
                && (!closed || *next_closed)
                && (!c || *nc)
                && terminal_advances(*t, *nt)
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(accepted: u64, cancel: bool, terminal: Option<sw::Terminal>) -> sw::ResponseBody {
        sw::ResponseBody::Exchange {
            stdin_accepted: accepted,
            stdin_delivered: accepted,
            stdin_closed: terminal.is_some() || cancel,
            stdout: None,
            stderr: None,
            stdout_eof: None,
            stderr_eof: None,
            cancel_requested: cancel,
            terminal,
        }
    }
    #[test]
    fn reordered_control_replies_cannot_undo_cancel_or_terminal() {
        let terminal = snapshot(8, true, Some(sw::Terminal::Cancelled));
        assert!(!response_advances(&terminal, &snapshot(8, true, None)));
        assert!(!response_advances(
            &snapshot(8, true, None),
            &snapshot(8, false, None)
        ));
        assert!(!response_advances(
            &snapshot(8, false, None),
            &snapshot(7, false, None)
        ));
        assert!(response_advances(&snapshot(7, false, None), &terminal));
        let uncertain = snapshot(
            8,
            true,
            Some(sw::Terminal::Failed {
                code: sw::WireErrorCode::Uncertain,
            }),
        );
        assert!(response_advances(&uncertain, &terminal));
        assert!(!response_advances(&terminal, &uncertain));
    }
}
