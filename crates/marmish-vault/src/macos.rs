use crate::{Error, KeyRef, Label, Metadata, Purpose, Secret};
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
    _ui: UiLease,
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
fn service(purpose: Purpose) -> String {
    format!("marmish.v1.{}", purpose.as_str())
}
unsafe fn key(value: CFStringRef) -> CFString {
    // Framework string constants have static lifetime and are retained here.
    unsafe { CFString::wrap_under_get_rule(value) }
}
impl Provider {
    pub(super) fn open_default() -> Result<Self, Error> {
        let ui = UiLease::acquire()?;
        let keychain = SecKeychain::default().map_err(native)?;
        Ok(Self { keychain, _ui: ui })
    }
    pub(super) fn open_keychain(keychain: &SecKeychain) -> Result<Self, Error> {
        Ok(Self {
            keychain: keychain.clone(),
            _ui: UiLease::acquire()?,
        })
    }
    pub(super) fn probe(&self) -> Result<(), Error> {
        let mut bits = 0;
        // SAFETY: the retained keychain and writable output pointer outlive call.
        let code =
            unsafe { SecKeychainGetStatus(self.keychain.as_concrete_TypeRef().cast(), &mut bits) };
        if code != 0 {
            return Err(status(code));
        }
        if bits & 1 == 0 {
            Err(Error::LockedOrDenied)
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
        let mut query = ItemSearchOptions::new();
        query
            .keychains(std::slice::from_ref(&self.keychain))
            .class(ItemClass::generic_password())
            .case_insensitive(Some(false))
            .service(&service(purpose))
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
            Err(e) => Err(native(e)),
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
                    CFString::new(&service(reference.purpose())).as_CFType(),
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
        self.probe()?;
        match self
            .query(reference.purpose(), Some(reference), false)?
            .len()
        {
            0 => return Err(Error::Missing),
            1 => (),
            _ => return Err(Error::Ambiguous),
        }
        let items = self.query(reference.purpose(), Some(reference), true)?;
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
            return Err(Error::BackendFailure);
        }
        match buffers.len() {
            0 => Err(Error::Missing),
            1 => Secret::from_slice(&buffers[0]),
            _ => Err(Error::Ambiguous),
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
                if attrs.get("svce").map(String::as_str) != Some(service(purpose).as_str()) {
                    return Err(Error::Malformed);
                }
                let reference =
                    KeyRef::new(1, purpose, attrs.get("acct").ok_or(Error::Malformed)?)?;
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
}
