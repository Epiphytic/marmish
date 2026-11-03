use crate::helper::{
    AuthorizationCategory, AuthorizationFailure, AuthorizationMode, AuthorizationPurpose,
};
use crate::{Error, KeyRef, Label, Metadata, Namespace, Purpose, Secret};
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    boolean::CFBoolean,
    data::CFData,
    dictionary::{CFDictionary, CFDictionaryRef},
    string::{CFString, CFStringRef},
};
use security_framework::{
    item::{ItemClass, ItemSearchOptions, SearchResult},
    os::macos::{
        keychain::{KeychainUserInteractionLock, SecKeychain},
        keychain_item::SecKeychainItem,
    },
};
use std::{collections::HashSet, ffi::c_void, ptr, sync::Mutex};
use zeroize::Zeroizing;

// The high-level add API discards the created reference. Retaining that exact
// reference is necessary for safe rollback; no search-based deletion is used.
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecKeychainSetUserInteractionAllowed(allowed: u8) -> i32;
    fn SecKeychainGetStatus(keychain: *mut c_void, status: *mut u32) -> i32;
    fn SecItemAdd(attributes: CFDictionaryRef, result: *mut CFTypeRef) -> i32;
    fn SecKeychainItemDelete(item: *mut c_void) -> i32;
    static kSecClass: CFStringRef;
    static kSecClassGenericPassword: CFStringRef;
    static kSecValueData: CFStringRef;
    static kSecUseKeychain: CFStringRef;
    static kSecAttrService: CFStringRef;
    static kSecAttrAccount: CFStringRef;
    static kSecAttrLabel: CFStringRef;
    static kSecReturnRef: CFStringRef;
}

struct UiState {
    _guard: Option<KeychainUserInteractionLock>,
    users: usize,
}
static UI_STATE: Mutex<Option<UiState>> = Mutex::new(None);
struct UiLease;
impl UiLease {
    fn acquire() -> Result<Self, Error> {
        let mut state = UI_STATE.lock().map_err(|_| Error::BackendFailure)?;
        if let Some(state) = state.as_mut() {
            state.users = state.users.checked_add(1).ok_or(Error::BackendFailure)?;
        } else {
            // Do not restore UI to enabled if the caller already disabled it.
            let guard = if SecKeychain::user_interaction_allowed().map_err(native)? {
                Some(SecKeychain::disable_user_interaction().map_err(native)?)
            } else {
                None
            };
            *state = Some(UiState {
                _guard: guard,
                users: 1,
            });
        }
        Ok(Self)
    }
}
impl Drop for UiLease {
    fn drop(&mut self) {
        if let Ok(mut state) = UI_STATE.lock() {
            if let Some(active) = state.as_mut() {
                active.users -= 1;
                if active.users == 0 {
                    // Restore only after the final overlapping vault closes,
                    // while the mutex prevents a new lease racing restoration.
                    *state = None;
                }
            }
        }
    }
}

pub(super) struct Provider {
    keychain: SecKeychain,
    namespace: Namespace,
    _ui: Option<UiLease>,
}

#[derive(Clone, Copy)]
enum DetailedFailure {
    Native(i32),
    Local(Error),
    Locked,
}
impl DetailedFailure {
    fn ordinary(self) -> Error {
        match self {
            Self::Native(code) => status(code),
            Self::Local(error) => error,
            Self::Locked => Error::LockedOrDenied,
        }
    }

    fn authorization(self, purpose: AuthorizationPurpose) -> AuthorizationFailure {
        let category = match self {
            Self::Native(-25308) => AuthorizationCategory::InteractionNotAllowed,
            Self::Native(-25293) => AuthorizationCategory::AuthenticationFailed,
            Self::Native(-128) => AuthorizationCategory::UserCancelled,
            Self::Native(-25300) | Self::Local(Error::Missing) => AuthorizationCategory::Missing,
            Self::Native(-25307) => AuthorizationCategory::Uninitialized,
            Self::Locked => AuthorizationCategory::KeychainLocked,
            Self::Local(Error::Ambiguous) => AuthorizationCategory::Ambiguous,
            Self::Local(Error::Malformed) => AuthorizationCategory::Malformed,
            _ => AuthorizationCategory::BackendFailure,
        };
        AuthorizationFailure { category, purpose }
    }
}

/// Private to the dedicated fresh helper; never used by ordinary Vault constructors.
struct AuthorizationUi {
    previous: bool,
    active: bool,
}
impl AuthorizationUi {
    fn set(mode: AuthorizationMode) -> Result<Self, DetailedFailure> {
        let previous = SecKeychain::user_interaction_allowed()
            .map_err(|error| DetailedFailure::Native(error.code()))?;
        let guard = Self {
            previous,
            active: true,
        };
        let allowed = matches!(mode, AuthorizationMode::Interactive);
        // SAFETY: this process-local Security flag accepts a Boolean, and the
        // dedicated operation holds UI_STATE until checked restoration finishes.
        let code = unsafe { SecKeychainSetUserInteractionAllowed(u8::from(allowed)) };
        if code != 0 {
            return Err(DetailedFailure::Native(code));
        }
        Ok(guard)
    }

    fn restore(mut self) -> Result<(), DetailedFailure> {
        // SAFETY: restores the exact prior Boolean in this isolated helper.
        let code = unsafe { SecKeychainSetUserInteractionAllowed(u8::from(self.previous)) };
        if code != 0 {
            return Err(DetailedFailure::Local(Error::BackendFailure));
        }
        self.active = false;
        Ok(())
    }
}
impl Drop for AuthorizationUi {
    fn drop(&mut self) {
        if self.active {
            // Best effort on unwind or failed explicit restoration. A failed
            // checked restoration can never produce a successful response.
            unsafe { SecKeychainSetUserInteractionAllowed(u8::from(self.previous)) };
        }
    }
}

pub(super) fn authorize_existing(
    references: &[KeyRef; 2],
    mode: AuthorizationMode,
) -> Result<(), AuthorizationFailure> {
    let provider_error =
        |error: DetailedFailure| error.authorization(AuthorizationPurpose::Provider);
    let state = UI_STATE
        .lock()
        .map_err(|_| provider_error(DetailedFailure::Local(Error::BackendFailure)))?;
    if state.is_some() {
        return Err(provider_error(DetailedFailure::Local(
            Error::BackendFailure,
        )));
    }
    let ui = AuthorizationUi::set(mode).map_err(provider_error)?;
    let result = (|| {
        let keychain = SecKeychain::default()
            .map_err(|error| provider_error(DetailedFailure::Native(error.code())))?;
        // The sole no-lease constructor: UI_STATE is held and this dedicated
        // helper performs no other Vault operations or native credential writes.
        let provider = Provider {
            keychain,
            namespace: Namespace::MarmishV2,
            _ui: None,
        };
        for reference in references {
            let purpose = match reference.purpose() {
                Purpose::Nostr => AuthorizationPurpose::Nostr,
                Purpose::Sqlcipher => AuthorizationPurpose::Sqlcipher,
            };
            let secret = provider
                .load_detailed(reference)
                .map_err(|error| error.authorization(purpose))?;
            drop(secret);
        }
        Ok(())
    })();
    ui.restore().map_err(provider_error)?;
    drop(state);
    result
}
fn status(code: i32) -> Error {
    match code {
        -25307 => Error::Uninitialized,
        -25308 | -25293 | -128 => Error::LockedOrDenied,
        -25300 => Error::Missing,
        -25299 => Error::Exists,
        _ => Error::BackendFailure,
    }
}
fn native(error: security_framework::base::Error) -> Error {
    status(error.code())
}
fn service(namespace: Namespace, purpose: Purpose) -> String {
    format!("marmish.v{}.{}", namespace.version(), purpose.as_str())
}
unsafe fn key(value: CFStringRef) -> CFString {
    // Framework string constants have static lifetime and are retained here.
    unsafe { CFString::wrap_under_get_rule(value) }
}
impl Provider {
    pub(super) fn open_default(namespace: Namespace) -> Result<Self, Error> {
        let ui = UiLease::acquire()?;
        let keychain = SecKeychain::default().map_err(native)?;
        Ok(Self {
            keychain,
            namespace,
            _ui: Some(ui),
        })
    }
    pub(super) fn open_keychain(
        keychain: &SecKeychain,
        namespace: Namespace,
    ) -> Result<Self, Error> {
        Ok(Self {
            keychain: keychain.clone(),
            namespace,
            _ui: Some(UiLease::acquire()?),
        })
    }
    pub(super) fn probe(&self) -> Result<(), Error> {
        self.probe_detailed().map_err(DetailedFailure::ordinary)
    }
    fn probe_detailed(&self) -> Result<(), DetailedFailure> {
        let mut bits = 0;
        // SAFETY: the retained keychain and writable output pointer outlive call.
        let code =
            unsafe { SecKeychainGetStatus(self.keychain.as_concrete_TypeRef().cast(), &mut bits) };
        if code != 0 {
            return Err(DetailedFailure::Native(code));
        }
        if bits & 1 == 0 {
            Err(DetailedFailure::Locked)
        } else {
            Ok(())
        }
    }
    fn query(
        &self,
        purpose: Purpose,
        reference: Option<&KeyRef>,
        data: bool,
    ) -> Result<Vec<SearchResult>, Error> {
        self.query_detailed(purpose, reference, data)
            .map_err(DetailedFailure::ordinary)
    }
    fn query_detailed(
        &self,
        purpose: Purpose,
        reference: Option<&KeyRef>,
        data: bool,
    ) -> Result<Vec<SearchResult>, DetailedFailure> {
        let mut query = ItemSearchOptions::new();
        query
            .keychains(std::slice::from_ref(&self.keychain))
            .class(ItemClass::generic_password())
            .case_insensitive(Some(false))
            .service(&service(self.namespace, purpose))
            .load_attributes(!data)
            .load_data(data);
        if !data {
            query.limit(if reference.is_some() { 2 } else { 129 });
        }
        // A data query leaves the SDK's native match-limit-one default intact.
        // Legacy Keychain can reject return-data queries with a larger limit.
        if let Some(reference) = reference {
            query.account(reference.id());
        }
        match query.search() {
            Ok(items) => Ok(items),
            Err(e) if e.code() == -25300 => Ok(Vec::new()),
            Err(e) => Err(DetailedFailure::Native(e.code())),
        }
    }
    fn add(
        &self,
        reference: &KeyRef,
        label: &Label,
        secret: &Secret,
    ) -> Result<SecKeychainItem, Error> {
        // SAFETY: all framework constants are valid retained CFStrings. The
        // dictionary retains every input through synchronous SecItemAdd.
        let attributes = unsafe {
            CFDictionary::from_CFType_pairs(&[
                (key(kSecClass), key(kSecClassGenericPassword).as_CFType()),
                (
                    key(kSecValueData),
                    CFData::from_buffer(secret.as_bytes()).as_CFType(),
                ),
                (key(kSecUseKeychain), self.keychain.as_CFType()),
                (
                    key(kSecAttrService),
                    CFString::new(&service(self.namespace, reference.purpose())).as_CFType(),
                ),
                (
                    key(kSecAttrAccount),
                    CFString::new(reference.id()).as_CFType(),
                ),
                (
                    key(kSecAttrLabel),
                    CFString::new(label.as_str()).as_CFType(),
                ),
                (key(kSecReturnRef), CFBoolean::true_value().as_CFType()),
            ])
        };
        let mut created = ptr::null();
        // SAFETY: dictionary and output slot are valid for this synchronous call.
        let code = unsafe { SecItemAdd(attributes.as_concrete_TypeRef(), &mut created) };
        if code != 0 {
            return Err(status(code));
        }
        if created.is_null() {
            return Err(Error::BackendFailure);
        }
        // The returned create-owned reference must be a legacy keychain item.
        // A different type fails closed and is released without arbitrary delete.
        let value = unsafe { CFType::wrap_under_create_rule(created) };
        if value.type_of() != SecKeychainItem::type_id() {
            return Err(Error::BackendFailure);
        }
        Ok(unsafe { SecKeychainItem::wrap_under_get_rule(created.cast_mut().cast()) })
    }
    pub(super) fn create(
        &self,
        reference: &KeyRef,
        label: &Label,
        secret: &Secret,
    ) -> Result<(), Error> {
        self.probe()?;
        match self
            .query(reference.purpose(), Some(reference), false)?
            .len()
        {
            0 => (),
            1 => return Err(Error::Exists),
            _ => return Err(Error::Ambiguous),
        }
        let created = self.add(reference, label, secret)?;
        let checked = self.load(reference).and_then(|loaded| {
            if loaded.as_bytes() == secret.as_bytes() {
                Ok(())
            } else {
                Err(Error::BackendFailure)
            }
        });
        if let Err(error) = checked {
            // SAFETY: this retained reference identifies only this call's item.
            let _ = unsafe { SecKeychainItemDelete(created.as_concrete_TypeRef().cast()) };
            return Err(error);
        }
        Ok(())
    }
    pub(super) fn load(&self, reference: &KeyRef) -> Result<Secret, Error> {
        self.load_detailed(reference)
            .map_err(DetailedFailure::ordinary)
    }
    fn load_detailed(&self, reference: &KeyRef) -> Result<Secret, DetailedFailure> {
        self.probe_detailed()?;
        match self
            .query_detailed(reference.purpose(), Some(reference), false)?
            .len()
        {
            0 => return Err(DetailedFailure::Local(Error::Missing)),
            1 => (),
            _ => return Err(DetailedFailure::Local(Error::Ambiguous)),
        }
        let items = self.query_detailed(reference.purpose(), Some(reference), true)?;
        // Own and zeroize every data result, including duplicate/error paths.
        let mut buffers = Vec::new();
        let mut unexpected = false;
        for item in items {
            match item {
                SearchResult::Data(bytes) => buffers.push(Zeroizing::new(bytes)),
                _ => unexpected = true,
            }
        }
        if unexpected {
            return Err(DetailedFailure::Local(Error::BackendFailure));
        }
        match buffers.len() {
            0 => Err(DetailedFailure::Local(Error::Missing)),
            1 => Secret::from_slice(&buffers[0]).map_err(DetailedFailure::Local),
            _ => Err(DetailedFailure::Local(Error::Ambiguous)),
        }
    }

    pub(super) fn list(&self) -> Result<Vec<Metadata>, Error> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        for purpose in [Purpose::Nostr, Purpose::Sqlcipher] {
            let items = self.query(purpose, None, false)?;
            if result.len() + items.len() > 128 {
                return Err(Error::BackendFailure);
            }
            for item in items {
                let attrs = item.simplify_dict().ok_or(Error::BackendFailure)?;
                if attrs.get("svce").map(String::as_str)
                    != Some(service(self.namespace, purpose).as_str())
                {
                    return Err(Error::Malformed);
                }
                let reference = KeyRef::new(
                    self.namespace.version(),
                    purpose,
                    attrs.get("acct").ok_or(Error::Malformed)?,
                )?;
                if !seen.insert(reference.clone()) {
                    return Err(Error::Ambiguous);
                }
                result.push(Metadata {
                    key_ref: reference,
                    label: Label::new(attrs.get("labl").ok_or(Error::Malformed)?)?,
                });
            }
        }
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespaces_have_disjoint_service_names() {
        assert_eq!(
            service(Namespace::Legacy, Purpose::Nostr),
            "marmish.v1.nostr"
        );
        assert_eq!(
            service(Namespace::MarmishV2, Purpose::Nostr),
            "marmish.v2.nostr"
        );
        assert_eq!(
            service(Namespace::MarmishV2, Purpose::Sqlcipher),
            "marmish.v2.sqlcipher"
        );
    }
    #[test]
    fn native_errors_are_sanitized() {
        for code in [-25308, -25293, -128] {
            assert_eq!(status(code), Error::LockedOrDenied);
        }
        assert_eq!(status(-25307), Error::Uninitialized);
        assert_eq!(status(-25300), Error::Missing);
        assert_eq!(status(-25299), Error::Exists);
        for code in [-1, -50, -25291] {
            assert_eq!(status(code), Error::BackendFailure);
        }
    }

    #[test]
    fn authorization_keeps_native_failures_distinct_without_changing_ordinary_errors() {
        let cases = [
            (
                -25308,
                AuthorizationCategory::InteractionNotAllowed,
                Error::LockedOrDenied,
            ),
            (
                -25293,
                AuthorizationCategory::AuthenticationFailed,
                Error::LockedOrDenied,
            ),
            (
                -128,
                AuthorizationCategory::UserCancelled,
                Error::LockedOrDenied,
            ),
            (-25300, AuthorizationCategory::Missing, Error::Missing),
            (
                -25307,
                AuthorizationCategory::Uninitialized,
                Error::Uninitialized,
            ),
            (
                -99999,
                AuthorizationCategory::BackendFailure,
                Error::BackendFailure,
            ),
        ];
        for (code, category, ordinary) in cases {
            let detailed = DetailedFailure::Native(code);
            assert_eq!(detailed.ordinary(), ordinary);
            assert_eq!(
                detailed
                    .authorization(AuthorizationPurpose::Provider)
                    .category,
                category
            );
        }
        assert_eq!(
            DetailedFailure::Locked
                .authorization(AuthorizationPurpose::Provider)
                .category,
            AuthorizationCategory::KeychainLocked
        );
        assert_eq!(DetailedFailure::Locked.ordinary(), Error::LockedOrDenied);
        for (error, category) in [
            (Error::Missing, AuthorizationCategory::Missing),
            (Error::Ambiguous, AuthorizationCategory::Ambiguous),
            (Error::Malformed, AuthorizationCategory::Malformed),
        ] {
            assert_eq!(
                DetailedFailure::Local(error)
                    .authorization(AuthorizationPurpose::Sqlcipher)
                    .category,
                category
            );
        }
    }

    #[test]
    fn ordinary_ui_lease_disables_ui_and_authorization_restores_exact_prior_policy() {
        let before = SecKeychain::user_interaction_allowed().unwrap();
        let ordinary = UiLease::acquire().unwrap();
        assert!(!SecKeychain::user_interaction_allowed().unwrap());
        // Interactive authorization refuses an active ordinary lease, before
        // default-Keychain lookup or any item access.
        let refs = [
            KeyRef::new(2, Purpose::Nostr, &"a".repeat(64)).unwrap(),
            KeyRef::new(2, Purpose::Sqlcipher, &"b".repeat(64)).unwrap(),
        ];
        assert!(authorize_existing(&refs, AuthorizationMode::Interactive).is_err());
        assert!(!SecKeychain::user_interaction_allowed().unwrap());
        drop(ordinary);
        assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), before);
        let state = UI_STATE.lock().unwrap();
        for mode in [AuthorizationMode::Interactive, AuthorizationMode::Verify] {
            let guard = AuthorizationUi::set(mode).unwrap_or_else(|_| panic!("UI policy"));
            assert_eq!(
                SecKeychain::user_interaction_allowed().unwrap(),
                matches!(mode, AuthorizationMode::Interactive)
            );
            guard.restore().unwrap_or_else(|_| panic!("UI restoration"));
            assert_eq!(SecKeychain::user_interaction_allowed().unwrap(), before);
        }
        drop(state);
    }
}
