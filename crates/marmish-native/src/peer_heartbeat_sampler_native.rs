//! Audited local OS calls for the bounded heartbeat worker.
use super::{AddressSelection, Deadline, Sources, Stopped};
#[cfg(target_os = "linux")]
use crate::command_limits::continuous_time;
use crate::peer_status_wire::{DiskStatus, MAX_IPS, MAX_LOAD, MAX_UPTIME, safe_text};
use std::collections::BTreeSet;
use std::ffi::{CStr, c_char};
use std::mem::{MaybeUninit, size_of};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ptr::{self, NonNull};

const MAX_USERNAME_BYTES: usize = 64;
pub(super) const MAX_INTERFACE_RECORDS: usize = 4096;

pub(super) struct Native;

impl Sources for Native {
    fn username(&mut self, deadline: &mut Deadline) -> Result<Option<String>, Stopped> {
        // SAFETY: geteuid has no arguments or preconditions.
        let uid = unsafe { libc::geteuid() };
        read_username(|buffer| {
            deadline.check()?;
            Ok(passwd_name(uid, buffer))
        })
    }

    fn uptime(&mut self) -> Option<u64> {
        host_uptime().filter(|uptime| *uptime <= MAX_UPTIME)
    }

    fn load(&mut self) -> Option<[u32; 3]> {
        let mut values = [0.0; 3];
        // SAFETY: the output buffer is valid for three doubles.
        let count = unsafe { libc::getloadavg(values.as_mut_ptr(), 3) };
        fixed_load(count, values)
    }

    fn disk(&mut self, label: &str, path: &CStr) -> Option<DiskStatus> {
        let mut output = MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: path is NUL-terminated and output is writable.
        if unsafe { libc::statvfs(path.as_ptr(), output.as_mut_ptr()) } != 0 {
            return None;
        }
        // SAFETY: successful statvfs initialized the output structure.
        let output = unsafe { output.assume_init() };
        let narrow = |value: u128| u64::try_from(value).ok();
        disk_usage(
            label,
            narrow(output.f_blocks.into())?,
            narrow(output.f_bfree.into())?,
            narrow(output.f_bavail.into())?,
            narrow(output.f_frsize.into())?,
        )
    }

    fn addresses(
        &mut self,
        selection: &AddressSelection,
        deadline: &mut Deadline,
    ) -> Result<Option<Vec<IpAddr>>, Stopped> {
        let Some(list) = AddressList::read() else {
            return Ok(None);
        };
        // SAFETY: list retains ownership while the initialized nodes are read.
        unsafe { matching_addresses(list.0, selection, deadline) }
    }
}

pub(super) enum PasswdRead {
    Found(*const c_char),
    BufferSmall,
    Missing,
}

pub(super) fn read_username(
    mut read: impl FnMut(&mut [u8]) -> Result<PasswdRead, Stopped>,
) -> Result<Option<String>, Stopped> {
    for capacity in [256, 1024, 4096, 16_384] {
        let mut buffer = vec![0xff; capacity];
        let name = match read(&mut buffer)? {
            PasswdRead::Found(name) => name,
            PasswdRead::BufferSmall => continue,
            PasswdRead::Missing => return Ok(None),
        };
        return Ok(buffered_name(&buffer, name));
    }
    Ok(None)
}

fn buffered_name(buffer: &[u8], name: *const c_char) -> Option<String> {
    let offset = name.addr().checked_sub(buffer.as_ptr().addr())?;
    let remaining = buffer.get(offset..)?;
    let bounded = &remaining[..remaining.len().min(MAX_USERNAME_BYTES + 1)];
    let end = bounded.iter().position(|byte| *byte == 0)?;
    let value = std::str::from_utf8(&bounded[..end]).ok()?;
    safe_text(value, MAX_USERNAME_BYTES).then(|| value.to_owned())
}

fn passwd_name(uid: libc::uid_t, buffer: &mut [u8]) -> PasswdRead {
    let mut record = MaybeUninit::<libc::passwd>::uninit();
    let mut result = ptr::null_mut();
    // SAFETY: valid exclusive record, buffer, and result storage remain live
    // through this call. Any returned name is consumed within the buffer's life.
    let status = unsafe {
        libc::getpwuid_r(
            uid,
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    match status {
        libc::ERANGE => PasswdRead::BufferSmall,
        0 if result == record.as_mut_ptr() => {
            // SAFETY: success with this exact result pointer initialized record.
            PasswdRead::Found(unsafe { record.assume_init() }.pw_name)
        }
        _ => PasswdRead::Missing,
    }
}

#[cfg(target_os = "linux")]
fn host_uptime() -> Option<u64> {
    Some(continuous_time().ok()?.as_secs())
}

#[cfg(target_os = "macos")]
fn host_uptime() -> Option<u64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut boot = MaybeUninit::<libc::timeval>::uninit();
    let mut length = size_of::<libc::timeval>();
    // SAFETY: the name is NUL-terminated, the output buffer has the specified
    // size, and the null input buffer requests a read-only query.
    if unsafe {
        libc::sysctlbyname(
            c"kern.boottime".as_ptr(),
            boot.as_mut_ptr().cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    } != 0
        || length != size_of::<libc::timeval>()
    {
        return None;
    }
    // SAFETY: the successful native call returned exactly a complete timeval.
    let boot = unsafe { boot.assume_init() };
    let wall = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    wall_uptime(wall, boot.tv_sec, boot.tv_usec.into())
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn wall_uptime(wall: std::time::Duration, seconds: i64, micros: i64) -> Option<u64> {
    if seconds <= 0 || !(0..1_000_000).contains(&micros) {
        return None;
    }
    let boot = std::time::Duration::new(seconds as u64, micros as u32 * 1000);
    Some(wall.checked_sub(boot)?.as_secs())
}

pub(super) fn fixed_load(count: i32, values: [f64; 3]) -> Option<[u32; 3]> {
    if count != 3 {
        return None;
    }
    let mut result = [0; 3];
    for (slot, value) in result.iter_mut().zip(values) {
        let milli = (value * 1000.0).round();
        if !value.is_finite() || value < 0.0 || !(0.0..=f64::from(MAX_LOAD)).contains(&milli) {
            return None;
        }
        *slot = milli as u32;
    }
    Some(result)
}

pub(super) fn disk_usage(
    label: &str,
    blocks: u64,
    free: u64,
    available: u64,
    fragment_bytes: u64,
) -> Option<DiskStatus> {
    if blocks == 0 || fragment_bytes == 0 || available > free {
        return None;
    }
    blocks.checked_mul(fragment_bytes)?;
    let used = blocks.checked_sub(free)?.checked_mul(fragment_bytes)?;
    let available = available.checked_mul(fragment_bytes)?;
    let disk = DiskStatus {
        label: label.to_owned(),
        used,
        available,
    };
    disk.valid().then_some(disk)
}

struct AddressList(*mut libc::ifaddrs);

impl AddressList {
    fn read() -> Option<Self> {
        let mut head = ptr::null_mut();
        // SAFETY: the out-pointer is valid. Ownership transfers on success.
        if unsafe { libc::getifaddrs(&mut head) } != 0 {
            return None;
        }
        Some(Self(head))
    }
}

impl Drop for AddressList {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this guard is the sole owner of the successful allocation.
            unsafe { libc::freeifaddrs(self.0) };
        }
    }
}

/// The caller keeps the initialized list and its referenced native fields alive.
pub(super) unsafe fn matching_addresses(
    mut next: *mut libc::ifaddrs,
    selection: &AddressSelection,
    deadline: &mut Deadline,
) -> Result<Option<Vec<IpAddr>>, Stopped> {
    let mut addresses = BTreeSet::new();
    for _ in 0..MAX_INTERFACE_RECORDS {
        if next.is_null() {
            break;
        }
        deadline.check()?;
        // SAFETY: the caller owns a live getifaddrs list. The next pointer
        // advances to its next initialized node or null.
        let item = unsafe { &*next };
        next = item.ifa_next;
        // SAFETY: this native node and its name and address remain live.
        let Some(value) = (unsafe { selected_address(item, selection) }) else {
            continue;
        };
        addresses.insert(value);
        if addresses.len() > MAX_IPS {
            addresses.pop_last();
        }
    }
    if !next.is_null() || addresses.is_empty() {
        return Ok(None);
    }
    Ok(Some(addresses.into_iter().collect()))
}

/// The caller ensures that node fields come from a live, valid getifaddrs list.
unsafe fn selected_address(item: &libc::ifaddrs, selection: &AddressSelection) -> Option<IpAddr> {
    if item.ifa_name.is_null() {
        return None;
    }
    let name = item.ifa_name.cast::<u8>();
    let end = (0..libc::IFNAMSIZ).find(|index| {
        // SAFETY: the native name is readable through its terminator. find
        // stops at that byte without touching the remainder of the allocation.
        unsafe { name.add(*index).read() == 0 }
    })?;
    // SAFETY: all end bytes were just read from this live native string.
    let name = unsafe { std::slice::from_raw_parts(name, end) };
    let families = selection.families(name)?;
    let address = NonNull::new(item.ifa_addr)?;
    // SAFETY: a non-null getifaddrs sockaddr has a readable family header.
    let family =
        i32::from(unsafe { ptr::addr_of!((*address.as_ptr()).sa_family).read_unaligned() });
    if !families.includes(family) {
        return None;
    }
    #[cfg(target_os = "linux")]
    let extent = family_extent(family)?;
    #[cfg(target_os = "macos")]
    // SAFETY: Darwin's sockaddr header includes the native length.
    let extent = usize::from(unsafe { ptr::addr_of!((*address.as_ptr()).sa_len).read_unaligned() });
    // SAFETY: the Linux API supplies the selected family-specific allocation.
    // On Darwin read_address checks the actual length against the selected family.
    unsafe { read_address(address, family, extent) }
}

fn family_extent(family: i32) -> Option<usize> {
    match family {
        libc::AF_INET => Some(size_of::<libc::sockaddr_in>()),
        libc::AF_INET6 => Some(size_of::<libc::sockaddr_in6>()),
        _ => None,
    }
}

/// The caller ensures that the supplied extent belongs to an initialized
/// address of the declared family.
pub(super) unsafe fn read_address(
    address: NonNull<libc::sockaddr>,
    family: i32,
    extent: usize,
) -> Option<IpAddr> {
    if extent < family_extent(family)? {
        return None;
    }
    match family {
        libc::AF_INET => {
            // SAFETY: the caller and extent check establish the IPv4 layout.
            let field = unsafe {
                ptr::addr_of!(
                    (*address.as_ptr().cast::<libc::sockaddr_in>())
                        .sin_addr
                        .s_addr
                )
                .read_unaligned()
            };
            Some(Ipv4Addr::from(field.to_ne_bytes()).into())
        }
        libc::AF_INET6 => {
            // SAFETY: the caller and extent check establish the IPv6 layout.
            let octets = unsafe {
                ptr::addr_of!(
                    (*address.as_ptr().cast::<libc::sockaddr_in6>())
                        .sin6_addr
                        .s6_addr
                )
                .read_unaligned()
            };
            #[cfg(target_os = "macos")]
            let octets = darwin_unscoped(octets);
            Some(Ipv6Addr::from(octets).into())
        }
        _ => None,
    }
}

#[cfg(any(target_os = "macos", test))]
pub(super) fn darwin_unscoped(mut octets: [u8; 16]) -> [u8; 16] {
    let link_local_unicast = octets[0] == 0xfe && octets[1] & 0xc0 == 0x80;
    let multicast = octets[0] == 0xff;
    let scope = octets[1] & 0x0f;
    let prefix_based = octets[1] & 0xf0 == 0x30;
    // Match XNU's IN6_IS_SCOPE_EMBED, including its multicast prefix exception.
    if link_local_unicast || multicast && (scope == 1 || scope == 2 && !prefix_based) {
        octets[2..4].fill(0);
    }
    octets
}
