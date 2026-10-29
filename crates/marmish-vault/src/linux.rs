use crate::{Error, KeyRef, Label, Metadata, Purpose, Secret};
use secret_service::{
    blocking::{Item, SecretService},
    EncryptionType,
};
use std::collections::{HashMap, HashSet};
use zeroize::Zeroizing;

pub(super) struct Provider {
    service: SecretService<'static>,
}
fn bus_error(error: zbus::Error) -> Error {
    match error {
        zbus::Error::InputOutput(e) | zbus::Error::Connection(e, _)
            if e.kind() == std::io::ErrorKind::NotFound =>
        {
            Error::Unavailable
        }
        zbus::Error::InputOutput(e) | zbus::Error::Connection(e, _)
            if e.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            Error::LockedOrDenied
        }
        zbus::Error::Address(_)
            if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none()
                && std::env::var_os("XDG_RUNTIME_DIR").is_none() =>
        {
            Error::Unavailable
        }
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.ServiceUnknown"
            | "org.freedesktop.DBus.Error.NameHasNoOwner" => Error::Unavailable,
            "org.freedesktop.DBus.Error.AccessDenied" | "org.freedesktop.Secret.Error.IsLocked" => {
                Error::LockedOrDenied
            }
            _ => Error::BackendFailure,
        },
        zbus::Error::FDO(e) => match *e {
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_) => {
                Error::Unavailable
            }
            zbus::fdo::Error::AccessDenied(_) => Error::LockedOrDenied,
            _ => Error::BackendFailure,
        },
        _ => Error::BackendFailure,
    }
}
fn native(error: secret_service::Error) -> Error {
    match error {
        secret_service::Error::Locked
        | secret_service::Error::Prompt
        | secret_service::Error::PromptDisconnected => Error::LockedOrDenied,
        secret_service::Error::Zbus(e) => bus_error(e),
        secret_service::Error::ZbusFdo(zbus::fdo::Error::AccessDenied(_)) => Error::LockedOrDenied,
        secret_service::Error::ZbusFdo(
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_),
        ) => Error::Unavailable,
        _ => Error::BackendFailure,
    }
}
fn attributes(reference: &KeyRef) -> HashMap<&str, &str> {
    HashMap::from([
        ("application", "marmish"),
        ("version", "1"),
        ("purpose", reference.purpose().as_str()),
        ("id", reference.id()),
    ])
}
impl Provider {
    pub(super) fn open_default() -> Result<Self, Error> {
        let connection = zbus::blocking::Connection::session().map_err(bus_error)?;
        let service =
            SecretService::connect_with_existing(EncryptionType::Dh, connection).map_err(native)?;
        Ok(Self { service })
    }
    pub(super) fn probe(&self) -> Result<(), Error> {
        let collection = self.service.get_default_collection().map_err(|e| match e {
            secret_service::Error::NoResult => Error::Uninitialized,
            e => native(e),
        })?;
        if collection.is_locked().map_err(native)? {
            Err(Error::LockedOrDenied)
        } else {
            Ok(())
        }
    }
    fn matches(&self, reference: &KeyRef) -> Result<Vec<Item<'_>>, Error> {
        let found = self
            .service
            .search_items(attributes(reference))
            .map_err(native)?;
        let mut items = found.unlocked;
        items.extend(found.locked);
        Ok(items)
    }
    pub(super) fn create(
        &self,
        reference: &KeyRef,
        label: &Label,
        secret: &Secret,
    ) -> Result<(), Error> {
        match self.matches(reference)?.len() {
            0 => (),
            1 => return Err(Error::Exists),
            _ => return Err(Error::Ambiguous),
        }
        self.probe()?;
        let collection = self.service.get_default_collection().map_err(native)?;
        if collection.is_locked().map_err(native)? {
            return Err(Error::LockedOrDenied);
        }
        // The wrapper can prompt on a provider relock race; callers bound this process.
        let created = collection
            .create_item(
                label.as_str(),
                attributes(reference),
                secret.as_bytes(),
                false,
                "application/octet-stream",
            )
            .map_err(native)?;
        let checked = self.load(reference).and_then(|loaded| {
            if loaded.as_bytes() == secret.as_bytes() {
                Ok(())
            } else {
                Err(Error::BackendFailure)
            }
        });
        if let Err(error) = checked {
            // Only remove the exact item this call created, never search matches.
            let _ = created.delete();
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn load(&self, reference: &KeyRef) -> Result<Secret, Error> {
        let items = self.matches(reference)?;
        match items.len() {
            0 => return Err(Error::Missing),
            1 => (),
            _ => return Err(Error::Ambiguous),
        }
        if items[0].is_locked().map_err(native)? {
            return Err(Error::LockedOrDenied);
        }
        let bytes = Zeroizing::new(items[0].get_secret().map_err(native)?);
        Secret::from_slice(&bytes)
    }
    pub(super) fn list(&self) -> Result<Vec<Metadata>, Error> {
        let found = self
            .service
            .search_items(HashMap::from([
                ("application", "marmish"),
                ("version", "1"),
            ]))
            .map_err(native)?;
        if found.unlocked.len() + found.locked.len() > 128 {
            return Err(Error::BackendFailure);
        }
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for item in found.unlocked.into_iter().chain(found.locked) {
            let attrs = item.get_attributes().map_err(native)?;
            let purpose = Purpose::parse(attrs.get("purpose").ok_or(Error::Malformed)?)?;
            let reference = KeyRef::new(1, purpose, attrs.get("id").ok_or(Error::Malformed)?)?;
            if !seen.insert(reference.clone()) {
                return Err(Error::Ambiguous);
            }
            result.push(Metadata {
                key_ref: reference,
                label: Label::new(&item.get_label().map_err(native)?)?,
            });
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uncertain_errors_never_mean_unavailable() {
        assert_eq!(
            native(secret_service::Error::PromptDisconnected),
            Error::LockedOrDenied
        );
        assert_eq!(
            native(secret_service::Error::Unavailable),
            Error::BackendFailure
        );
        assert_eq!(native(secret_service::Error::Locked), Error::LockedOrDenied);
        assert_eq!(native(secret_service::Error::Prompt), Error::LockedOrDenied);
        assert_eq!(
            native(secret_service::Error::NoResult),
            Error::BackendFailure
        );
        assert_eq!(
            bus_error(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()),
            Error::LockedOrDenied
        );
        assert_eq!(
            bus_error(std::io::Error::from(std::io::ErrorKind::TimedOut).into()),
            Error::BackendFailure
        );
    }
}
