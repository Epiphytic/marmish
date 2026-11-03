//! Pure local command preparation and printable locators, never execution authority.
use crate::command_wire::{self as wire, Account, Binding, Digest, Id, Request, RequestOp};
use crate::journal::ClientInvocation;
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

const HANDLE_LIMIT: usize = 1024;
/// A local locator. It deliberately excludes request IDs, group IDs and command bytes.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "HandleFields")]
pub(crate) struct CommandHandle {
    format: u8,
    profile: String,
    remote: String,
    session: String,
    invocation: String,
    submit_sha256: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HandleFields {
    format: u8,
    profile: String,
    remote: String,
    session: String,
    invocation: String,
    submit_sha256: String,
}
impl TryFrom<HandleFields> for CommandHandle {
    type Error = &'static str;
    fn try_from(fields: HandleFields) -> std::result::Result<Self, Self::Error> {
        let handle = Self {
            format: fields.format,
            profile: fields.profile,
            remote: fields.remote,
            session: fields.session,
            invocation: fields.invocation,
            submit_sha256: fields.submit_sha256,
        };
        handle.validate().map_err(|_| "command_handle")?;
        Ok(handle)
    }
}
pub(crate) struct PreparedClientOperation {
    pub request: Request,
    pub encoded: Vec<u8>,
    pub digest: Digest,
}
pub(crate) fn prepare_operation(
    binding: Binding,
    id: Id,
    op: RequestOp,
) -> Result<PreparedClientOperation> {
    let request = Request { binding, id, op };
    let encoded = wire::encode_request(&request)?;
    let digest = wire::request_digest(&encoded);
    Ok(PreparedClientOperation {
        request,
        encoded,
        digest,
    })
}
impl CommandHandle {
    pub(crate) fn for_submit(profile: &str, prepared: &PreparedClientOperation) -> Result<Self> {
        if !matches!(prepared.request.op, RequestOp::Submit(_))
            || wire::encode_request(&prepared.request)? != prepared.encoded
            || wire::request_digest(&prepared.encoded) != prepared.digest
        {
            return Err(Error::Invalid("command_handle"));
        }
        Self::new(
            profile,
            prepared.request.binding.server,
            prepared.request.binding.session,
            prepared.request.op.invocation(),
            prepared.digest,
        )
    }
    pub(crate) fn from_invocation(profile: &str, entry: &ClientInvocation) -> Result<Self> {
        if entry.owner.server_side() {
            return Err(Error::Invalid("command_owner"));
        }
        Self::new(
            profile,
            entry.owner.binding().server,
            entry.owner.binding().session,
            entry.invocation,
            entry.submit_request_sha256,
        )
    }
    fn new(
        profile: &str,
        remote: Account,
        session: Id,
        invocation: Id,
        digest: Digest,
    ) -> Result<Self> {
        let handle = Self {
            format: 1,
            profile: profile.into(),
            remote: hex::encode(remote),
            session: hex::encode(session),
            invocation: hex::encode(invocation),
            submit_sha256: hex::encode(digest),
        };
        handle.validate()?;
        Ok(handle)
    }
    fn validate(&self) -> Result<()> {
        // Same profile-name grammar as identity::validate_name; no filesystem or
        // key-provider access is appropriate when decoding a printable locator.
        if self.format != 1
            || self.profile.is_empty()
            || self.profile.len() > 64
            || !self
                .profile
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || !crate::foundation::valid_hex(&self.remote, 64)
            || !crate::foundation::valid_hex(&self.submit_sha256, 64)
        {
            return Err(Error::Invalid("command_handle"));
        }
        crate::foundation::id_timestamp(&self.session)
            .map_err(|_| Error::Invalid("command_handle"))?;
        crate::foundation::id_timestamp(&self.invocation)
            .map_err(|_| Error::Invalid("command_handle"))?;
        Ok(())
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > HANDLE_LIMIT {
            return Err(Error::Invalid("command_handle"));
        }
        serde_json::from_slice(bytes).map_err(|_| Error::Invalid("command_handle"))
    }
    pub(crate) fn encode(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|_| Error::Invalid("command_handle"))?;
        if bytes.len() > HANDLE_LIMIT {
            return Err(Error::Invalid("command_handle"));
        }
        Ok(bytes)
    }
    pub(crate) fn profile(&self) -> &str {
        &self.profile
    }
    pub(crate) fn remote(&self) -> Account {
        hex::decode(&self.remote)
            .expect("validated handle")
            .try_into()
            .expect("account")
    }
    pub(crate) fn session(&self) -> Id {
        hex::decode(&self.session)
            .expect("validated handle")
            .try_into()
            .expect("session")
    }
    pub(crate) fn invocation(&self) -> Id {
        hex::decode(&self.invocation)
            .expect("validated handle")
            .try_into()
            .expect("invocation")
    }
    pub(crate) fn submit_sha256(&self) -> Digest {
        hex::decode(&self.submit_sha256)
            .expect("validated handle")
            .try_into()
            .expect("digest")
    }
    pub(crate) fn validate_entry(
        &self,
        profile: &str,
        local: Account,
        entry: &ClientInvocation,
    ) -> Result<()> {
        self.validate()?;
        if self.profile != profile
            || entry.owner.server_side()
            || entry.owner.binding().client != local
            || entry.owner.binding().server != self.remote()
            || entry.owner.binding().session != self.session()
            || entry.invocation != self.invocation()
            || entry.submit_request_sha256 != self.submit_sha256()
        {
            return Err(Error::Invalid("command_handle_binding"));
        }
        if !entry.authority_valid {
            return Err(Error::Invalid("command_authority_revoked"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prepared() -> PreparedClientOperation {
        let id = hex::decode(crate::foundation::fresh_id(1_000_000).unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        prepare_operation(
            Binding {
                client: [1; 32],
                server: [2; 32],
                session: id,
            },
            id,
            RequestOp::Submit(wire::Submit {
                invocation: id,
                argv: vec![b"/bin/cat".to_vec(), vec![255, 1]],
                stdin: (0..=255).collect(),
            }),
        )
        .unwrap()
    }
    #[test]
    fn handle_is_closed_bounded_and_excludes_execution_bytes_and_request_id() {
        let operation = prepared();
        assert!(wire::decode_request(&operation.encoded).unwrap() == operation.request);
        let handle = CommandHandle::for_submit("test-identity", &operation).unwrap();
        assert_eq!(handle.profile(), "test-identity");
        assert_eq!(handle.remote(), [2; 32]);
        assert_eq!(handle.session(), operation.request.binding.session);
        assert_eq!(handle.invocation(), operation.request.op.invocation());
        assert_eq!(handle.submit_sha256(), operation.digest);
        let bytes = handle.encode().unwrap();
        assert!(CommandHandle::decode(&bytes).unwrap() == handle);
        let text = String::from_utf8(bytes).unwrap();
        for malformed in [
            text.replace("\"format\":1", "\"format\":true"),
            text.replace("\"format\":1", "\"format\":1,\"format\":1"),
            text.replace("\"format\":1", "\"format\":1,\"argv\":[]"),
            text.replace("test-identity", "../identity"),
            text.replace(&handle.remote, "AA"),
            text.replace(&handle.session, "noncanonical"),
        ] {
            assert!(CommandHandle::decode(malformed.as_bytes()).is_err());
        }
        assert!(CommandHandle::decode(&vec![b' '; 1025]).is_err());
        assert!(CommandHandle::for_submit("", &operation).is_err());
        let mut tampered = prepared();
        tampered.digest[0] ^= 1;
        assert!(CommandHandle::for_submit("default", &tampered).is_err());
        for op in [
            RequestOp::Status {
                invocation: handle.invocation(),
            },
            RequestOp::Cancel {
                invocation: handle.invocation(),
            },
            RequestOp::Poll {
                invocation: handle.invocation(),
                stdout_cursor: 0,
                stderr_cursor: 0,
                max_bytes: 16384,
            },
        ] {
            let read =
                prepare_operation(operation.request.binding.clone(), operation.request.id, op)
                    .unwrap();
            assert!(wire::decode_request(&read.encoded).unwrap() == read.request);
            assert!(CommandHandle::for_submit("default", &read).is_err());
        }
    }
}
