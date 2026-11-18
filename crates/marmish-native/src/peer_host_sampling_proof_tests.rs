//! Opt-in calibration only. Raw host data stays in this test process.
use std::ffi::{CStr, c_char};
use std::io::{Read, Write};
use std::mem::{MaybeUninit, size_of};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::process::{Child, Command, Stdio};
use std::ptr::{self, NonNull};
use std::time::{Duration, Instant};

const USER_BYTES: usize = 64;
const RESULT_LIMIT: usize = 8;
// A load sample above one million whole load units is treated as invalid.
const LOAD_CAP_MILLI: u32 = 1_000_000_000;
const CHILD_MODE: &str = "MARMISH_HOST_PROOF_CHILD";
const CHILD_TEST: &str = "peer_host_sampling_proof_tests::host_sampling_child";
const BOGUS_ENV: [(&str, &str); 3] = [
    ("USER", "marmish-fixture-ignored-user"),
    ("LOGNAME", "marmish-fixture-ignored-logname"),
    ("HOME", "/marmish-fixture-ignored-home"),
];

enum PasswdRead {
    Found(*const c_char),
    BufferSmall,
    Missing,
}

fn bounded_username(mut read: impl FnMut(&mut [u8]) -> PasswdRead) -> Option<String> {
    for capacity in [256, 1024, 4096, 16_384] {
        let mut buffer = vec![0_u8; capacity];
        let name = match read(&mut buffer) {
            PasswdRead::Found(name) => name,
            PasswdRead::BufferSmall => continue,
            PasswdRead::Missing => return None,
        };
        let offset = name.addr().checked_sub(buffer.as_ptr().addr())?;
        let remaining = buffer.get(offset..)?;
        let suffix = &remaining[..remaining.len().min(USER_BYTES + 1)];
        let end = suffix.iter().position(|byte| *byte == 0)?;
        let name = std::str::from_utf8(&suffix[..end]).ok()?;
        return (!name.is_empty() && !name.chars().any(char::is_control)).then(|| name.to_owned());
    }
    None
}

fn effective_username() -> Option<String> {
    // SAFETY: geteuid takes no arguments and has no preconditions.
    let uid = unsafe { libc::geteuid() };
    bounded_username(|buffer| {
        let mut record = MaybeUninit::<libc::passwd>::uninit();
        let mut result = ptr::null_mut();
        // SAFETY: all out-pointers are valid exclusive storage for this call.
        // The initialized byte buffer stays live until pw_name is consumed.
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
                // SAFETY: success with the supplied result pointer initialized the record.
                PasswdRead::Found(unsafe { record.assume_init() }.pw_name)
            }
            _ => PasswdRead::Missing,
        }
    })
}

fn read_clock(clock: libc::clockid_t) -> Option<Duration> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime receives a valid writable timespec.
    if unsafe { libc::clock_gettime(clock, &mut time) } != 0 {
        return None;
    }
    checked_clock(time)
}

fn checked_clock(time: libc::timespec) -> Option<Duration> {
    if !(0..1_000_000_000).contains(&time.tv_nsec) {
        return None;
    }
    Some(Duration::new(
        u64::try_from(time.tv_sec).ok()?,
        u32::try_from(time.tv_nsec).ok()?,
    ))
}

#[cfg(target_os = "linux")]
fn host_uptime() -> Option<Duration> {
    read_clock(libc::CLOCK_BOOTTIME)
}

#[cfg(target_os = "macos")]
fn host_uptime() -> Option<Duration> {
    Some(mac_clocks()?.wall_uptime)
}

#[cfg(target_os = "macos")]
struct MacClocks {
    elapsed: Duration,
    awake: Duration,
    wall_uptime: Duration,
    boot_wall: Duration,
}

#[cfg(target_os = "macos")]
fn mac_clocks() -> Option<MacClocks> {
    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    unsafe extern "C" {
        fn mach_continuous_time() -> u64;
        fn mach_timebase_info(info: *mut Timebase) -> i32;
    }
    let mut base = Timebase { numer: 0, denom: 0 };
    // SAFETY: the out-pointer is exclusive and valid for a complete Timebase.
    if unsafe { mach_timebase_info(&mut base) } != 0 || base.numer == 0 || base.denom == 0 {
        return None;
    }
    // SAFETY: this system clock takes no arguments and has no preconditions.
    let ticks = unsafe { mach_continuous_time() };
    let nanos = u128::from(ticks) * u128::from(base.numer) / u128::from(base.denom);
    let elapsed = Duration::new(
        u64::try_from(nanos / 1_000_000_000).ok()?,
        (nanos % 1_000_000_000) as u32,
    );
    let awake = read_clock(libc::CLOCK_UPTIME_RAW)?;
    let mut boot_time = MaybeUninit::<libc::timeval>::uninit();
    let mut length = size_of::<libc::timeval>();
    // SAFETY: the name is NUL-terminated. The writable buffer has the provided
    // size. The last arguments request a read only and provide no input buffer.
    if unsafe {
        libc::sysctlbyname(
            c"kern.boottime".as_ptr(),
            boot_time.as_mut_ptr().cast(),
            &mut length,
            ptr::null_mut(),
            0,
        )
    } != 0
        || length != size_of::<libc::timeval>()
    {
        return None;
    }
    // SAFETY: success with the exact documented structure size initialized it.
    let boot_time = unsafe { boot_time.assume_init() };
    if boot_time.tv_sec <= 0 || !(0..1_000_000).contains(&boot_time.tv_usec) {
        return None;
    }
    let boot_wall = Duration::new(
        u64::try_from(boot_time.tv_sec).ok()?,
        u32::try_from(boot_time.tv_usec).ok()? * 1000,
    );
    let wall_uptime = read_clock(libc::CLOCK_REALTIME)?.checked_sub(boot_wall)?;
    Some(MacClocks {
        elapsed,
        awake,
        wall_uptime,
        boot_wall,
    })
}

fn fixed_load(count: i32, values: [f64; 3]) -> Option<[u32; 3]> {
    let mut result = [0; 3];
    if count != 3 {
        return None;
    }
    for (slot, value) in result.iter_mut().zip(values) {
        let milli = (value * 1000.0).round();
        if !value.is_finite() || value < 0.0 || !(0.0..=f64::from(LOAD_CAP_MILLI)).contains(&milli)
        {
            return None;
        }
        *slot = milli as u32;
    }
    Some(result)
}

fn load_average() -> Option<[u32; 3]> {
    let mut values = [0.0_f64; 3];
    // SAFETY: libc can write up to three initialized c_double slots.
    let count = unsafe { libc::getloadavg(values.as_mut_ptr(), 3) };
    fixed_load(count, values)
}

struct DiskUsage {
    used_bytes: u64,
    available_bytes: u64,
    used_permyriad: u16,
}

fn disk_usage(blocks: u64, free: u64, available: u64, block_size: u64) -> Option<DiskUsage> {
    if blocks == 0 || block_size == 0 || available > free {
        return None;
    }
    let used = blocks.checked_sub(free)?;
    let used_bytes = used.checked_mul(block_size)?;
    let available_bytes = available.checked_mul(block_size)?;
    blocks.checked_mul(block_size)?;
    let visible_blocks = used.checked_add(available)?;
    let used_permyriad = (u128::from(used) * 10_000).checked_div(u128::from(visible_blocks))?;
    Some(DiskUsage {
        used_bytes,
        available_bytes,
        used_permyriad: u16::try_from(used_permyriad).ok()?,
    })
}

fn filesystem(label: &str, path: &CStr) -> Option<DiskUsage> {
    if label.is_empty()
        || label.len() > 32
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path owns a NUL-terminated string and stat is writable for the call.
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: a successful statvfs initialized the complete output struct.
    let stat = unsafe { stat.assume_init() };
    // Narrowing rejects values if a future platform widens these fields.
    let narrow = |number: u128| u64::try_from(number).ok();
    disk_usage(
        narrow(u128::from(stat.f_blocks))?,
        narrow(u128::from(stat.f_bfree))?,
        narrow(u128::from(stat.f_bavail))?,
        narrow(u128::from(stat.f_frsize))?,
    )
}

struct AddressList {
    head: *mut libc::ifaddrs,
    release: unsafe extern "C" fn(*mut libc::ifaddrs),
}

impl Drop for AddressList {
    fn drop(&mut self) {
        if !self.head.is_null() {
            // SAFETY: the matching allocator transferred exclusive list ownership
            // into this guard, and this is the sole call to its release function.
            unsafe { (self.release)(self.head) };
        }
    }
}

fn family_extent(family: i32) -> Option<usize> {
    match family {
        libc::AF_INET => Some(size_of::<libc::sockaddr_in>()),
        libc::AF_INET6 => Some(size_of::<libc::sockaddr_in6>()),
        _ => None,
    }
}

/// The platform supplies at least `extent` initialized bytes for this address.
unsafe fn address_value(
    address: NonNull<libc::sockaddr>,
    family: i32,
    extent: usize,
) -> Option<IpAddr> {
    if extent < family_extent(family)? {
        return None;
    }
    match family {
        libc::AF_INET => {
            // SAFETY: extent and the address family establish the IPv4 layout.
            let addr = unsafe {
                ptr::addr_of!(
                    (*address.as_ptr().cast::<libc::sockaddr_in>())
                        .sin_addr
                        .s_addr
                )
                .read_unaligned()
            };
            Some(Ipv4Addr::from(addr.to_ne_bytes()).into())
        }
        libc::AF_INET6 => {
            // SAFETY: extent and the address family establish the IPv6 layout.
            let octets = unsafe {
                ptr::addr_of!(
                    (*address.as_ptr().cast::<libc::sockaddr_in6>())
                        .sin6_addr
                        .s6_addr
                )
                .read_unaligned()
            };
            Some(Ipv6Addr::from(octets).into())
        }
        _ => None,
    }
}

fn selected_addresses(eligible: bool, names: &[&[u8]], families: &[i32]) -> Option<Vec<IpAddr>> {
    if !eligible || names.is_empty() || families.is_empty() {
        return Some(Vec::new());
    }
    if names.len() > RESULT_LIMIT
        || families.len() > 2
        || families
            .iter()
            .any(|&family| family_extent(family).is_none())
        || names
            .iter()
            .any(|name| name.is_empty() || name.len() >= libc::IFNAMSIZ || name.contains(&0))
    {
        return None;
    }
    let mut head = ptr::null_mut();
    // SAFETY: the C API writes the resulting list pointer to live storage.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return None;
    }
    let list = AddressList {
        head,
        release: libc::freeifaddrs,
    };
    matching_addresses(&list, names, families)
}

fn matching_addresses(
    list: &AddressList,
    names: &[&[u8]],
    families: &[i32],
) -> Option<Vec<IpAddr>> {
    let mut addresses = Vec::new();
    let mut next = list.head;
    for _ in 0..4096 {
        if next.is_null() || addresses.len() == RESULT_LIMIT {
            return Some(addresses);
        }
        // SAFETY: the libc list stays live under the guard. Each next pointer
        // is either null or the next initialized record in that list.
        let item = unsafe { &*next };
        next = item.ifa_next;
        if item.ifa_name.is_null() {
            continue;
        }
        // SAFETY: getifaddrs provides a live, NUL-terminated name per entry.
        let name = unsafe { CStr::from_ptr(item.ifa_name) }.to_bytes();
        if !names.contains(&name) {
            continue;
        }
        let Some(address) = NonNull::new(item.ifa_addr) else {
            continue;
        };
        // SAFETY: a non-null sockaddr from getifaddrs has a family header.
        let family =
            i32::from(unsafe { ptr::addr_of!((*address.as_ptr()).sa_family).read_unaligned() });
        if !families.contains(&family) {
            continue;
        }
        let Some(expected) = family_extent(family) else {
            continue;
        };
        // Linux supplies the family-specific structure with no sa_len field.
        #[cfg(target_os = "linux")]
        let extent = expected;
        #[cfg(target_os = "macos")]
        // SAFETY: Darwin's sockaddr header contains the actual bound.
        let extent =
            usize::from(unsafe { ptr::addr_of!((*address.as_ptr()).sa_len).read_unaligned() });
        if extent < expected {
            continue;
        }
        // SAFETY: the platform API supplies initialized address fields of the
        // selected family to the checked extent. No unselected body is read.
        if let Some(value) = unsafe { address_value(address, family, extent) }
            && !addresses.contains(&value)
        {
            addresses.push(value);
        }
    }
    None
}

struct TrialChild(Child);

impl Drop for TrialChild {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.terminate();
        }
    }
}

impl TrialChild {
    fn terminate(&mut self) -> Result<(), &'static str> {
        self.0.kill().map_err(|_| "child_kill")?;
        let started = Instant::now();
        loop {
            if self.0.try_wait().map_err(|_| "child_reap")?.is_some() {
                return Ok(());
            }
            if started.elapsed() >= Duration::from_millis(500) {
                return Err("child_reap_deadline");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn child_run(mode: &str, limit: Duration) -> Result<String, &'static str> {
    let mut child = TrialChild(
        Command::new(std::env::current_exe().map_err(|_| "child_executable")?)
            .args(["--ignored", "--exact", CHILD_TEST, "--nocapture"])
            .env_clear()
            .envs(BOGUS_ENV)
            .env(CHILD_MODE, mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "child_spawn")?,
    );
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().map_err(|_| "child_wait")? {
            break Some(status);
        }
        if start.elapsed() >= limit {
            child.terminate()?;
            if mode != "stall" {
                return Err("child_deadline");
            }
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut output = String::new();
    child
        .0
        .stdout
        .take()
        .ok_or("child_stdout")?
        .take(4097)
        .read_to_string(&mut output)
        .map_err(|_| "child_stdout")?;
    let mut stderr = String::new();
    child
        .0
        .stderr
        .take()
        .ok_or("child_stderr")?
        .take(4097)
        .read_to_string(&mut stderr)
        .map_err(|_| "child_stderr")?;
    if status.is_some_and(|exit| !exit.success()) || output.len() > 4096 || !stderr.is_empty() {
        return Err("child_failed");
    }
    if mode == "stall"
        && (status.is_some()
            || !output
                .lines()
                .any(|line| line == "host-proof stall=started"))
    {
        return Err("blocked_worker_deadline");
    }
    Ok(output)
}

fn report_is_private(output: &str) -> bool {
    !BOGUS_ENV.iter().any(|(_, marker)| output.contains(marker))
        && !output
            .split(|character: char| {
                !character.is_ascii_hexdigit() && character != '.' && character != ':'
            })
            .any(|token| token.parse::<IpAddr>().is_ok())
}

const SAMPLES: usize = 200;
const FIELDS: [&str; 6] = ["account", "uptime", "load", "disk", "addresses", "whole"];

fn safe_metric(line: &str, field: &str) -> bool {
    let parts: Vec<_> = line.split_ascii_whitespace().collect();
    let ["host-proof", entry, "samples=200", p50, p95] = parts.as_slice() else {
        return false;
    };
    let quantile = |part: &str, prefix: &str| {
        part.strip_prefix(prefix)
            .filter(|value| value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse::<u128>().ok())
    };
    entry.strip_prefix("field=") == Some(field)
        && matches!((quantile(p50, "p50_ns="), quantile(p95, "p95_ns=")), (Some(a), Some(b)) if a <= b)
}

fn calibrate() {
    const WARMUP: usize = 16;
    #[cfg(target_os = "linux")]
    let loopback: &[u8] = b"lo";
    #[cfg(target_os = "macos")]
    let loopback: &[u8] = b"lo0";
    let mut samples: [Vec<Duration>; 6] = std::array::from_fn(|_| Vec::with_capacity(SAMPLES));
    for iteration in 0..WARMUP + SAMPLES {
        let start = Instant::now();
        let (username, account_time) = timed(effective_username);
        let (uptime, clock_time) = timed(host_uptime);
        let (load, load_time) = timed(load_average);
        let (disk, disk_time) = timed(|| filesystem("root", c"/"));
        let (addresses, address_time) =
            timed(|| selected_addresses(true, &[loopback], &[libc::AF_INET, libc::AF_INET6]));
        let total = start.elapsed();
        let name = username.as_deref().expect("account_unavailable");
        assert!(
            BOGUS_ENV.iter().all(|(_, marker)| name != *marker),
            "account_used_environment"
        );
        assert!(uptime.is_some(), "uptime_unavailable");
        assert!(load.is_some(), "load_unavailable");
        let disk = disk.expect("disk_unavailable");
        assert!(
            disk.used_bytes.checked_add(disk.available_bytes).is_some()
                && disk.used_permyriad <= 10_000,
            "disk_bounds"
        );
        let addresses = addresses.expect("address_unavailable");
        assert!(
            addresses.len() <= RESULT_LIMIT && addresses.iter().any(IpAddr::is_loopback),
            "address_bounds"
        );
        if iteration >= WARMUP {
            for (timings, sample) in samples.iter_mut().zip([
                account_time,
                clock_time,
                load_time,
                disk_time,
                address_time,
                total,
            ]) {
                timings.push(sample);
            }
        }
    }
    #[cfg(target_os = "linux")]
    crosscheck_linux_uptime();
    for (field, timings) in FIELDS.into_iter().zip(&mut samples) {
        timings.sort_unstable();
        println!(
            "host-proof field={field} samples={SAMPLES} p50_ns={} p95_ns={}",
            timings[99].as_nanos(),
            timings[189].as_nanos()
        );
    }
}

fn timed<T>(operation: impl FnOnce() -> T) -> (T, Duration) {
    let started = Instant::now();
    (operation(), started.elapsed())
}

#[cfg(target_os = "linux")]
fn crosscheck_linux_uptime() {
    let before = host_uptime().expect("clock_unavailable");
    let contents = std::fs::read_to_string("/proc/uptime").expect("clock_reference_unavailable");
    let seconds: f64 = contents
        .split_ascii_whitespace()
        .next()
        .and_then(|value| value.parse().ok())
        .expect("clock_reference_format");
    let after = host_uptime().expect("clock_unavailable");
    assert!(
        seconds >= before.as_secs_f64() - 0.1 && seconds <= after.as_secs_f64() + 0.1,
        "clock_reference_disagreement"
    );
}

#[cfg(target_os = "macos")]
fn mac_sleep_calibration() {
    let before = mac_clocks().expect("clock_unavailable");
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        std::thread::sleep(Duration::from_millis(250));
        let after = mac_clocks().expect("clock_unavailable");
        let elapsed = after
            .elapsed
            .checked_sub(before.elapsed)
            .expect("clock_regressed");
        let awake = after
            .awake
            .checked_sub(before.awake)
            .expect("clock_regressed");
        if elapsed.saturating_sub(awake) < Duration::from_secs(2) {
            continue;
        }
        let wall_delta = after.wall_uptime.checked_sub(before.wall_uptime);
        if after.boot_wall != before.boot_wall
            || wall_delta.is_none_or(|delta| delta.abs_diff(elapsed) > Duration::from_secs(2))
        {
            println!("host-proof sleep=observed origin=indeterminate uptime=omitted");
            return;
        }
        let origin = if [
            before.wall_uptime.abs_diff(before.elapsed),
            after.wall_uptime.abs_diff(after.elapsed),
        ]
        .into_iter()
        .all(|offset| offset <= Duration::from_secs(5))
        {
            "boot_aligned"
        } else {
            "offset"
        };
        println!("host-proof sleep=observed origin={origin} uptime=wall_derived");
        return;
    }
    println!("host-proof sleep=not_measured");
}

#[test]
#[ignore = "internal host proof subprocess"]
fn host_sampling_child() {
    match std::env::var(CHILD_MODE).as_deref() {
        Ok("sample") => std::thread::spawn(calibrate)
            .join()
            .expect("sample_worker_failed"),
        Ok("stall") => std::thread::spawn(|| {
            println!("host-proof stall=started");
            std::io::stdout().flush().expect("stall_start_marker");
            loop {
                std::thread::park();
            }
        })
        .join()
        .expect("stall_worker_failed"),
        #[cfg(target_os = "macos")]
        Ok("sleep") => std::thread::spawn(mac_sleep_calibration)
            .join()
            .expect("sleep_worker_failed"),
        _ => (),
    }
}

#[test]
#[ignore = "local native collector proof and timing"]
fn peer_host_sampling_proof() {
    synthetic_bounds();
    child_run("stall", Duration::from_secs(3)).expect("blocked_worker_deadline");
    let report = child_run("sample", Duration::from_secs(15)).expect("sample_process_failed");
    let metrics: Vec<_> = report
        .lines()
        .filter(|line| line.starts_with("host-proof field="))
        .collect();
    assert_eq!(metrics.len(), FIELDS.len(), "report_metric_count");
    for (metric, field) in metrics.into_iter().zip(FIELDS) {
        assert!(report_is_private(metric), "report_privacy");
        assert!(safe_metric(metric, field), "report_metric_shape");
        println!("{metric}");
    }
    println!("host-proof sleep=not_measured");
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "operator-assisted real Mac sleep/wake proof"]
fn peer_host_sampling_macos_sleep() {
    let report = child_run("sleep", Duration::from_secs(75)).expect("sleep_process_failed");
    let disposition = report
        .lines()
        .find(|line| {
            matches!(
                *line,
                "host-proof sleep=not_measured"
                    | "host-proof sleep=observed origin=indeterminate uptime=omitted"
                    | "host-proof sleep=observed origin=boot_aligned uptime=wall_derived"
                    | "host-proof sleep=observed origin=offset uptime=wall_derived"
            )
        })
        .expect("sleep_report_missing");
    assert!(report_is_private(disposition), "report_privacy");
    println!("{disposition}");
}

fn synthetic_bounds() {
    for (tv_sec, tv_nsec) in [(-1, 0), (1, -1), (1, 1_000_000_000)] {
        assert!(checked_clock(libc::timespec { tv_sec, tv_nsec }).is_none());
    }
    let mut attempts = 0;
    assert!(
        bounded_username(|_| {
            attempts += 1;
            PasswdRead::BufferSmall
        })
        .is_none()
    );
    assert_eq!(attempts, 4);
    assert!(bounded_username(|_| PasswdRead::Missing).is_none());
    assert!(bounded_username(|_| PasswdRead::Found(ptr::null())).is_none());
    assert!(
        bounded_username(|buffer| PasswdRead::Found(buffer.as_ptr_range().end.cast())).is_none()
    );
    for bytes in [&[0xff, 0][..], b"\n\0", &[b'a'; USER_BYTES + 1]] {
        assert!(
            bounded_username(|buffer| {
                buffer[..bytes.len()].copy_from_slice(bytes);
                PasswdRead::Found(buffer.as_ptr().cast())
            })
            .is_none()
        );
    }
    let name = bounded_username(|buffer| {
        if buffer.len() == 256 {
            return PasswdRead::BufferSmall;
        }
        buffer[16..21].copy_from_slice(b"user\0");
        PasswdRead::Found(buffer[16..].as_ptr().cast())
    });
    assert!(name.as_deref() == Some("user"));
    assert_eq!(fixed_load(3, [0.003, 2.0, 7.521]), Some([3, 2000, 7521]));
    assert!(fixed_load(2, [0.0; 3]).is_none());
    assert_eq!(
        fixed_load(3, [999_999.999; 3]),
        Some([LOAD_CAP_MILLI - 1; 3])
    );
    for value in [-1.0, f64::NAN, f64::INFINITY, 1_000_000.001] {
        assert!(fixed_load(3, [value; 3]).is_none());
    }
    let usage = disk_usage(100, 30, 20, 4096).expect("fake_disk_invalid");
    assert_eq!(
        (
            usage.used_bytes,
            usage.available_bytes,
            usage.used_permyriad
        ),
        (286_720, 81_920, 7777)
    );
    for (blocks, free, available, size) in [
        (0, 0, 0, 1),
        (1, 1, 2, 1),
        (1, 2, 0, 1),
        (1, 1, 0, 1),
        (u64::MAX, u64::MAX, 1, 2),
        (1, 0, 0, 0),
    ] {
        assert!(disk_usage(blocks, free, available, size).is_none());
    }
    synthetic_addresses();
}

fn synthetic_addresses() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static RELEASED: AtomicBool = AtomicBool::new(false);
    unsafe extern "C" fn release_fixture(_list: *mut libc::ifaddrs) {
        RELEASED.store(true, Ordering::Relaxed);
    }
    let early_return = || -> Option<()> {
        let _guard = AddressList {
            head: ptr::dangling_mut(),
            release: release_fixture,
        };
        None
    };
    assert!(early_return().is_none());
    assert!(RELEASED.load(Ordering::Relaxed));
    assert!(
        selected_addresses(false, &[b"unselected"], &[libc::AF_INET])
            .is_some_and(|addresses| addresses.is_empty())
    );
    assert!(selected_addresses(true, &[b"bad\0name"], &[libc::AF_INET]).is_none());
    assert!(
        selected_addresses(
            true,
            &[b"many".as_slice(); RESULT_LIMIT + 1],
            &[libc::AF_INET]
        )
        .is_none()
    );
    // SAFETY: sockaddr_in is a plain integer/byte C struct; zero is valid.
    let mut v4: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    v4.sin_addr.s_addr = u32::from_ne_bytes([198, 51, 100, 4]);
    let address = NonNull::from(&mut v4).cast();
    // SAFETY: this fixture has exactly sockaddr_in storage. The decoder must
    // reject a short or mismatched size before touching the address body.
    unsafe {
        assert!(address_value(address, libc::AF_INET, 0).is_none());
        assert!(address_value(address, libc::AF_INET6, size_of::<libc::sockaddr_in>()).is_none());
        assert!(address_value(address, libc::AF_UNIX, size_of::<libc::sockaddr_in>()).is_none());
        assert!(
            address_value(address, libc::AF_INET, size_of::<libc::sockaddr_in>())
                == Some(Ipv4Addr::new(198, 51, 100, 4).into())
        );
    }
    assert!(!report_is_private("value=198.51.100.4"));
    assert!(!report_is_private("value=2001:db8::4"));
    assert!(!safe_metric(
        "host-proof field=load samples=200 p50_ns=1 p95_ns=2 path=/private",
        "load"
    ));
    assert!(selected_addresses(true, &[b"selected"], &[libc::AF_UNIX]).is_none());
    assert_eq!(
        selected_addresses(true, &[b"selected"], &[]),
        Some(Vec::new())
    );
    synthetic_address_list();
}

fn synthetic_address_list() {
    unsafe extern "C" fn release_borrowed_fixture(_list: *mut libc::ifaddrs) {}
    let make_c_address = |last| {
        // SAFETY: sockaddr_in is a plain integer/byte C struct; zero is valid.
        let mut address: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        address.sin_family = libc::AF_INET as libc::sa_family_t;
        #[cfg(target_os = "macos")]
        {
            address.sin_len = size_of::<libc::sockaddr_in>() as u8;
        }
        address.sin_addr.s_addr = u32::from_ne_bytes([198, 51, 100, last]);
        address
    };
    let mut bodies: [_; RESULT_LIMIT + 1] =
        std::array::from_fn(|index| make_c_address(index as u8));
    let mut head = ptr::null_mut();
    let mut entries: [libc::ifaddrs; RESULT_LIMIT + 3] = std::array::from_fn(|_| {
        // SAFETY: ifaddrs can safely start as a null list of all-zero fields.
        unsafe { std::mem::zeroed() }
    });
    let mut selected = bodies.iter_mut();
    let mut unselected = make_c_address(100);
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.ifa_next = head;
        let (name, body) = if index == RESULT_LIMIT + 2 {
            (c"selected-but-extra", Some(&mut unselected))
        } else {
            (c"selected", selected.next())
        };
        entry.ifa_name = name.as_ptr().cast_mut();
        entry.ifa_addr = body.map_or(ptr::null_mut(), |body| NonNull::from(body).cast().as_ptr());
        head = entry;
    }
    // SAFETY: sockaddr_in6 contains only integer and byte fields.
    let mut body_v6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
    body_v6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
    #[cfg(target_os = "macos")]
    {
        body_v6.sin6_len = size_of::<libc::sockaddr_in6>() as u8;
    }
    body_v6.sin6_addr.s6_addr = Ipv6Addr::LOCALHOST.octets();
    // SAFETY: zero initializes the nullable fields in ifaddrs.
    let mut entry_v6: libc::ifaddrs = unsafe { std::mem::zeroed() };
    entry_v6.ifa_next = head;
    entry_v6.ifa_name = c"selected".as_ptr().cast_mut();
    entry_v6.ifa_addr = NonNull::from(&mut body_v6).cast().as_ptr();
    let list = AddressList {
        head: &mut entry_v6,
        release: release_borrowed_fixture,
    };
    let matched =
        matching_addresses(&list, &[b"selected"], &[libc::AF_INET]).expect("fixture_addresses");
    assert_eq!(matched.len(), RESULT_LIMIT);
    assert!(
        matching_addresses(&list, &[b"selected"], &[libc::AF_INET6])
            .is_some_and(|addresses| addresses == [IpAddr::V6(Ipv6Addr::LOCALHOST)])
    );
    assert!(
        matched.iter().copied().eq((1..=RESULT_LIMIT)
            .rev()
            .map(|last| IpAddr::V4(Ipv4Addr::new(198, 51, 100, last as u8))))
    );
}
