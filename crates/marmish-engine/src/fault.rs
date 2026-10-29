//! Fixed, off-by-default crash seams. No production environment controls.
use crate::{launcher::StateRoot, model::JobId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FaultPoint {
    AfterNativePendingCreate,
    AfterNativePendingWrite,
    AfterNativePendingSync,
    AfterNativePublish,
    AfterNativeDirectorySync,
    AfterNativeLegacyUnlink,
    AfterNativeLegacyDirectorySync,
    AfterKeyStage,
    AfterKeyPublish,
    AfterKeyDirectorySync,
    AfterDatabaseCreate,
    BeforeSchemaCreate,
    AfterSchemaCreate,
    BeforeAccepted,
    AfterAccepted,
    BeforeClaim,
    AfterClaim,
    BeforeSpawn,
    AfterSpawn,
    BeforeRunning,
    AfterRunning,
    BeforeInputWrite,
    AfterInputWrite,
    BeforeInputProgress,
    AfterInputProgress,
    AfterSpoolDatabaseRemove,
    BeforeSpoolSync,
    AfterSpoolSync,
    BeforeOutputCommit,
    AfterOutputCommit,
    BeforeExitCommit,
    AfterExitCommit,
}

impl FaultPoint {
    #[cfg(feature = "fault-injection")]
    fn label(self) -> &'static str {
        match self {
            Self::AfterNativePendingCreate => "AfterNativePendingCreate",
            Self::AfterNativePendingWrite => "AfterNativePendingWrite",
            Self::AfterNativePendingSync => "AfterNativePendingSync",
            Self::AfterNativePublish => "AfterNativePublish",
            Self::AfterNativeDirectorySync => "AfterNativeDirectorySync",
            Self::AfterNativeLegacyUnlink => "AfterNativeLegacyUnlink",
            Self::AfterNativeLegacyDirectorySync => "AfterNativeLegacyDirectorySync",
            Self::AfterKeyStage => "AfterKeyStage",
            Self::AfterKeyPublish => "AfterKeyPublish",
            Self::AfterKeyDirectorySync => "AfterKeyDirectorySync",
            Self::AfterDatabaseCreate => "AfterDatabaseCreate",
            Self::BeforeSchemaCreate => "BeforeSchemaCreate",
            Self::AfterSchemaCreate => "AfterSchemaCreate",
            Self::BeforeAccepted => "BeforeAccepted",
            Self::AfterAccepted => "AfterAccepted",
            Self::BeforeClaim => "BeforeClaim",
            Self::AfterClaim => "AfterClaim",
            Self::BeforeSpawn => "BeforeSpawn",
            Self::AfterSpawn => "AfterSpawn",
            Self::BeforeRunning => "BeforeRunning",
            Self::AfterRunning => "AfterRunning",
            Self::BeforeInputWrite => "BeforeInputWrite",
            Self::AfterInputWrite => "AfterInputWrite",
            Self::BeforeInputProgress => "BeforeInputProgress",
            Self::AfterInputProgress => "AfterInputProgress",
            Self::AfterSpoolDatabaseRemove => "AfterSpoolDatabaseRemove",
            Self::BeforeSpoolSync => "BeforeSpoolSync",
            Self::AfterSpoolSync => "AfterSpoolSync",
            Self::BeforeOutputCommit => "BeforeOutputCommit",
            Self::AfterOutputCommit => "AfterOutputCommit",
            Self::BeforeExitCommit => "BeforeExitCommit",
            Self::AfterExitCommit => "AfterExitCommit",
        }
    }
}

pub(crate) fn fault_point(root: &StateRoot, job: JobId, point: FaultPoint) {
    #[cfg(feature = "fault-injection")]
    {
        let sentinel = match point {
            FaultPoint::AfterNativePendingCreate
            | FaultPoint::AfterNativePendingWrite
            | FaultPoint::AfterNativePendingSync
            | FaultPoint::AfterNativePublish
            | FaultPoint::AfterNativeDirectorySync
            | FaultPoint::AfterNativeLegacyUnlink
            | FaultPoint::AfterNativeLegacyDirectorySync
            | FaultPoint::BeforeAccepted
            | FaultPoint::AfterAccepted
            | FaultPoint::AfterKeyStage
            | FaultPoint::AfterKeyPublish
            | FaultPoint::AfterKeyDirectorySync
            | FaultPoint::AfterDatabaseCreate
            | FaultPoint::BeforeSchemaCreate
            | FaultPoint::AfterSchemaCreate => root.read_frontend_fault(),
            _ => root.read_fault(job),
        };
        if let Ok(Some(bytes)) = sentinel {
            if bytes.strip_suffix(b"\n").unwrap_or(&bytes) == point.label().as_bytes() {
                std::process::abort();
            }
        }
    }
    #[cfg(not(feature = "fault-injection"))]
    let _ = (root, job, point);
}
