//! Private installed-layout qualification of a registered Command daemon.
use super::{LoadedProfile, PreparedProfile};
use crate::{
    Error, Result,
    command_config::CommandConfig,
    command_engine::Admission,
    command_limits::ServiceLifetime,
    dispatcher_fatal::Fatal,
    identity::{IdentityMaterial, ProfileLease},
    mdk_adapter::MdkOwner,
    peer_relay_trust::candidate_registration::{
        ReceiverPreflight, RegisteredReceiver, RegistrationFailure,
    },
    transport::RelayConnector,
};
use std::{path::Path, sync::Arc};

enum Selection {
    Stock(PreparedProfile, RelayConnector),
    Registered(RegisteredStartup),
    Indeterminate(Arc<ProfileLease>),
}

struct RegisteredStartup {
    lease: Arc<ProfileLease>,
    receiver: RegisteredReceiver,
    connector: RelayConnector,
    material: IdentityMaterial,
    peers: Vec<String>,
    commands: CommandConfig,
    lifetime: Arc<ServiceLifetime>,
    retention: std::time::Duration,
}

struct RegisteredOwner {
    lease: Arc<ProfileLease>,
    _receiver: RegisteredReceiver,
    fatal: Fatal,
    admission: Option<Admission>,
    owner: Option<Box<MdkOwner>>,
}

fn eligible(loaded: &LoadedProfile) -> bool {
    use crate::command_config::peer_intent::Receive;

    loaded.lease.was_ready_at_acquire()
        && !loaded.policy.needs_tor()
        && loaded
            .commands
            .as_ref()
            .is_some_and(|config| config.peer_receive() == Some(&Receive::Manual))
}

fn select(
    prepared: PreparedProfile,
    connector: RelayConnector,
    lifetime: Arc<ServiceLifetime>,
    eligible_before_prepare: bool,
) -> Result<Selection> {
    let PreparedProfile { loaded, material } = prepared;
    if !eligible_before_prepare
        || connector.policy().needs_tor()
        || connector.policy().urls() != loaded.policy.urls()
        || connector.policy().mode() != loaded.policy.mode()
    {
        return Ok(Selection::Stock(
            PreparedProfile { loaded, material },
            connector,
        ));
    }
    let lease = Arc::new(loaded.lease);
    let commands = loaded
        .commands
        .as_ref()
        .ok_or(Error::Invalid("commands_disabled"))?;
    let preflight = ReceiverPreflight::prepare(&lease, &material, commands);
    if let Ok(Some(preflight)) = preflight {
        match preflight.register(&lease, &material) {
            Ok(receiver) => {
                return Ok(Selection::Registered(RegisteredStartup {
                    lease,
                    receiver,
                    connector,
                    material,
                    peers: loaded.peers,
                    commands: loaded
                        .commands
                        .expect("registration requires Command config"),
                    lifetime,
                    retention: loaded.retention,
                }));
            }
            Err(RegistrationFailure::Untouched) => {}
            Err(RegistrationFailure::LocalUnavailable) => {
                return Err(Error::Storage("candidate_registration_conflict"));
            }
            Err(RegistrationFailure::Indeterminate) => {
                return Ok(Selection::Indeterminate(lease));
            }
        }
    }
    let lease = Arc::try_unwrap(lease).map_err(|_| Error::Identity("candidate_startup_custody"))?;
    Ok(Selection::Stock(
        PreparedProfile {
            loaded: LoadedProfile { lease, ..loaded },
            material,
        },
        connector,
    ))
}

impl RegisteredStartup {
    /// Returned resources are put in custody before the next admission.
    fn open(self, executable: &Path) -> (RegisteredOwner, Result<()>) {
        let mut selected = RegisteredOwner {
            fatal: self.receiver.forwarding_verifier().fatal(),
            lease: self.lease,
            _receiver: self.receiver,
            owner: None,
            admission: None,
        };
        let opened = (|| {
            let fence = selected
                ._receiver
                .take_command_fence()
                .map_err(|_| Error::Storage("candidate_dispatcher_lost"))?;
            let admission = Admission::from_registered(fence);
            selected.admission = Some(admission.clone());
            let connector = self.connector.with_fatal(selected.fatal.clone())?;
            let storage = admission.connection_fence();
            crate::native_preflight::check_with_fence(
                &selected.lease,
                &self.material,
                Some(&storage),
            )?;
            let owner = MdkOwner::open_candidate(
                &selected.lease,
                &self.material,
                connector,
                self.peers,
                admission.clone(),
            )?;
            let owner = selected.owner.insert(Box::new(owner));
            owner.set_retention(self.retention)?;
            owner.reconcile_client_inventory(Some(&self.commands))?;
            if !admission.admit() {
                return Err(Error::Storage("candidate_dispatcher_lost"));
            }
            selected
                .lease
                .check_opened_binding(owner.opened_binding())?;
            let worker = crate::native_layout::resolve(executable)?.worker;
            owner.enable_candidate_commands(
                &selected.lease,
                &self.material,
                self.commands,
                worker,
                admission.clone(),
            )?;
            owner.command_set_lifetime(self.lifetime);
            Ok(())
        })();
        (selected, opened)
    }
}
