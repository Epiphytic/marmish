use crate::{
    launcher,
    model::*,
    store::{AccountSpec, InputOwner, OwnerKey},
};
use sha2::{Digest, Sha256};
use std::time::Duration;

pub(crate) fn validate_policy(policy: &EnginePolicy) -> Result<AccountSpec> {
    if !policy.state_root.is_absolute()
        || !policy.worker_executable.is_absolute()
        || policy.allowed_principals.is_empty()
        || policy.allowed_principals.len() > 4096
    {
        return Err(EngineError::InvalidPolicy);
    }
    validate_limits(&policy.limits)?;
    if let Some(commands) = &policy.command_policy {
        commands.validate(&policy.allowed_principals)?;
    }
    let account = launcher::local_account(policy.cwd.as_deref())?;
    // Trusted executable validation is also performed at launch. Canonicalization
    // here never selects an implicit executable or PATH fallback.
    launcher::trusted_worker_executable(&policy.worker_executable)?;
    Ok(account)
}

pub(crate) fn validate_limits(l: &EngineLimits) -> Result<()> {
    let ms = Duration::from_millis(1);
    if !(1..=128).contains(&l.active_jobs)
        || !(1..=100_000).contains(&l.invocation_records)
        || !(1..=4096).contains(&l.outstanding_tokens)
        || !(1..=16).contains(&l.starting_workers)
        || l.token_lifetime < Duration::from_secs(1)
        || l.token_lifetime > Duration::from_secs(300)
        || !(1..=64 * 1024 * 1024).contains(&l.spool_bytes_per_stream)
        || l.aggregate_spool_bytes < l.spool_bytes_per_stream * 2
        || l.aggregate_spool_bytes > 8 * 1024 * 1024 * 1024
        || l.tombstone_retention < Duration::from_secs(48 * 3600)
        || l.tombstone_retention > Duration::from_secs(30 * 86400)
        || l.cancel_grace < ms
        || l.cancel_grace > Duration::from_secs(30)
        || l.drain_grace < ms
        || l.drain_grace > Duration::from_secs(30)
        || l.active_jobs > l.invocation_records
    {
        return Err(EngineError::InvalidPolicy);
    }
    Ok(())
}

pub(crate) fn authorize(
    policy: &EnginePolicy,
    account: &AccountSpec,
    principal: PrincipalId,
    endpoint: EndpointId,
    server: ServerId,
    target_uid: u32,
) -> Result<AuthorizedContext> {
    if account.uid == 0 {
        return Err(EngineError::RootForbidden);
    }
    if policy.server != server
        || !policy.allowed_principals.contains(&principal)
        || target_uid != account.uid
    {
        return Err(EngineError::Unauthorized);
    }
    Ok(AuthorizedContext {
        principal,
        endpoint,
        server,
        target_uid,
    })
}
pub(crate) fn context_owner(
    policy: &EnginePolicy,
    account: &AccountSpec,
    ctx: &AuthorizedContext,
) -> Result<InputOwner> {
    authorize(
        policy,
        account,
        ctx.principal,
        ctx.endpoint,
        ctx.server,
        ctx.target_uid,
    )?;
    Ok(InputOwner {
        owner: OwnerKey {
            principal: ctx.principal,
            server: ctx.server,
            target_uid: ctx.target_uid,
        },
        endpoint: ctx.endpoint,
    })
}
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).map_err(|_| EngineError::StorageFailure)?;
    Ok(bytes)
}
pub(crate) fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub(crate) fn validate_request(request: &CommandRequest, allow_shell: bool) -> Result<()> {
    match request {
        CommandRequest::Exec { argv } => {
            if argv.is_empty() || argv.len() > 256 || argv[0].is_empty() || argv[0].len() > 4096 {
                return Err(EngineError::InvalidRequest);
            }
            let total = argv
                .iter()
                .try_fold(0usize, |n, a| {
                    if a.contains(&0) {
                        return None;
                    }
                    n.checked_add(a.len())?.checked_add(1)
                })
                .ok_or(EngineError::InvalidRequest)?;
            if total > 64 * 1024 {
                return Err(EngineError::InvalidRequest);
            }
        }
        CommandRequest::Shell { command } => {
            if !allow_shell || command.len() > 64 * 1024 || command.contains(&0) {
                return Err(EngineError::InvalidRequest);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "admission/tests.rs"]
mod tests;
