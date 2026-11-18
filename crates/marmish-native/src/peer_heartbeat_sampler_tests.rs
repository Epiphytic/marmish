use super::*;
use crate::peer_status_wire::{Heartbeat, Hex32, MAX_LOAD, Payload, Record};
use native::{
    MAX_INTERFACE_RECORDS, PasswdRead, darwin_unscoped, disk_usage, fixed_load, matching_addresses,
    read_address, read_username, wall_uptime,
};
use std::mem::{size_of, zeroed};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::num::NonZeroU64;
use std::os::unix::ffi::OsStringExt;
use std::ptr::{self, NonNull};
use std::sync::atomic::AtomicU64;
use std::time::Instant;

fn empty_request() -> Request {
    Request::new(BaseFields::default(), Vec::new(), None).unwrap()
}

fn selected_fields() -> BaseFields {
    BaseFields {
        username: true,
        uptime: true,
        load: true,
    }
}

#[test]
fn sampler_core_request_rejects_invalid_bounds() {
    let disk = |label: &str, path: PathBuf| {
        Request::new(BaseFields::default(), vec![(label.to_owned(), path)], None)
    };
    for label in ["", "slash/path", "newline\n", "\u{202e}", "\u{200f}"] {
        assert!(disk(label, "/".into()).is_none());
    }
    assert!(disk(&"é".repeat(17), "/".into()).is_none());
    assert!(disk(&"é".repeat(16), "/".into()).is_some());
    for path in ["relative", "//absolute", "/null\0", ""] {
        assert!(disk("volume", path.into()).is_none());
    }
    assert!(disk("volume", format!("/{}", "a".repeat(4094)).into()).is_some());
    assert!(disk("volume", format!("/{}", "a".repeat(4095)).into()).is_none());
    assert!(
        disk(
            "volume",
            std::ffi::OsString::from_vec(vec![b'/', 0xff]).into()
        )
        .is_some()
    );
    assert!(
        Request::new(
            BaseFields::default(),
            vec![("same".into(), "/".into()), ("same".into(), "/tmp".into())],
            None
        )
        .is_none()
    );
    let too_many = (0..=MAX_DISKS)
        .map(|index| (index.to_string(), "/".into()))
        .collect();
    assert!(Request::new(BaseFields::default(), too_many, None).is_none());

    for name in ["", "name\0"] {
        assert!(AddressSelection::new(vec![(name.into(), Families::V4)]).is_none());
    }
    assert!(AddressSelection::new(vec![("x".repeat(libc::IFNAMSIZ), Families::V4)]).is_none());
    let repeated = vec![("lo".into(), Families::V4), ("lo".into(), Families::V6)];
    assert!(AddressSelection::new(repeated).is_none());
    assert!(AddressSelection::new(Vec::new()).is_none());
    let too_many = (0..=MAX_INTERFACES)
        .map(|index| (index.to_string(), Families::Both))
        .collect();
    assert!(AddressSelection::new(too_many).is_none());
}

fn fixture_name(name: &[u8]) -> Option<String> {
    read_username(|buffer| {
        buffer[..name.len()].copy_from_slice(name);
        Ok(PasswdRead::Found(buffer.as_ptr().cast()))
    })
    .unwrap()
}

#[test]
fn sampler_core_username_and_numeric_validation() {
    assert_eq!(fixture_name(b"sample\0"), Some("sample".into()));
    for rejected in [
        b"\0".as_slice(),
        b"abc",
        b"invalid\xff\0",
        b"control\n\0",
        "user\u{202e}\0".as_bytes(),
        "user\u{2028}\0".as_bytes(),
    ] {
        assert!(fixture_name(rejected).is_none());
    }
    assert!(fixture_name(&[b'a'; 65]).is_none());
    assert!(
        read_username(|buffer| Ok(PasswdRead::Found(buffer.as_ptr().wrapping_sub(1).cast())))
            .unwrap()
            .is_none()
    );
    assert!(
        read_username(|buffer| Ok(PasswdRead::Found(
            buffer.as_ptr().wrapping_add(buffer.len()).cast()
        )))
        .unwrap()
        .is_none()
    );
    let mut attempts = Vec::new();
    assert!(
        read_username(|buffer| {
            attempts.push(buffer.len());
            Ok(PasswdRead::BufferSmall)
        })
        .unwrap()
        .is_none()
    );
    assert_eq!(attempts, [256, 1024, 4096, 16_384]);

    assert_eq!(
        fixed_load(3, [0.0, 1.2348, 1_000_000.0]),
        Some([0, 1235, MAX_LOAD])
    );
    assert!(fixed_load(2, [0.0; 3]).is_none());
    for invalid in [f64::NAN, f64::INFINITY, -0.0001, 1_000_000.1] {
        assert!(fixed_load(3, [invalid, 0.0, 0.0]).is_none());
    }
    let valid = disk_usage("volume", 10, 4, 3, 1024).unwrap();
    assert_eq!((valid.used, valid.available), (6144, 3072));
    for parameters in [
        (0, 0, 0, 1),
        (10, 4, 5, 1),
        (2, 3, 0, 1),
        (2, 0, 0, 0),
        (10, 10, 0, 1),
        (u64::MAX, u64::MAX - 1, 1, 2),
    ] {
        let (blocks, free, available, fragment) = parameters;
        assert!(disk_usage("volume", blocks, free, available, fragment).is_none());
    }
    assert_eq!(wall_uptime(Duration::from_secs(30), 10, 500_000), Some(19));
    assert!(wall_uptime(Duration::from_secs(1), 1, 1).is_none());
    assert!(wall_uptime(Duration::from_secs(1), 0, 0).is_none());
    assert!(wall_uptime(Duration::from_secs(3), 1, 1_000_000).is_none());
}

fn deadline() -> Deadline {
    Deadline {
        clock: Arc::new(|| Some(Duration::from_secs(1))),
        latest: Duration::ZERO,
        end: Duration::from_secs(2),
    }
}

fn v4(octets: [u8; 4]) -> libc::sockaddr_in {
    // SAFETY: sockaddr_in is a C structure of integers with valid zero values.
    let mut addr: libc::sockaddr_in = unsafe { zeroed() };
    addr.sin_family = libc::AF_INET as _;
    addr.sin_addr.s_addr = u32::from_ne_bytes(octets);
    #[cfg(target_os = "macos")]
    {
        addr.sin_len = size_of::<libc::sockaddr_in>() as u8;
    }
    addr
}

fn v6(octets: [u8; 16]) -> libc::sockaddr_in6 {
    // SAFETY: sockaddr_in6 is a C structure of integers with valid zero values.
    let mut addr: libc::sockaddr_in6 = unsafe { zeroed() };
    addr.sin6_family = libc::AF_INET6 as _;
    addr.sin6_addr.s6_addr = octets;
    addr.sin6_scope_id = 1234;
    #[cfg(target_os = "macos")]
    {
        addr.sin6_len = size_of::<libc::sockaddr_in6>() as u8;
    }
    addr
}

fn interface_node(name: &CStr, address: *mut libc::sockaddr) -> libc::ifaddrs {
    // SAFETY: the ifaddrs fixture has only integer and nullable raw pointer fields.
    let mut node: libc::ifaddrs = unsafe { zeroed() };
    node.ifa_name = name.as_ptr().cast_mut();
    node.ifa_addr = address;
    node
}

fn addresses(nodes: &mut [libc::ifaddrs], selection: &AddressSelection) -> Option<Vec<IpAddr>> {
    let length = nodes.len();
    let base = nodes.as_mut_ptr();
    for index in 0..length {
        // SAFETY: both offsets are inside this allocation when used.
        unsafe {
            (*base.add(index)).ifa_next = if index + 1 == length {
                ptr::null_mut()
            } else {
                base.add(index + 1)
            };
        }
    }
    let head = if length == 0 { ptr::null_mut() } else { base };
    // SAFETY: each initialized fixture node and its referenced data stay live.
    unsafe { matching_addresses(head, selection, &mut deadline()) }.unwrap()
}

#[test]
fn sampler_core_per_interface_family_canonical_cap_and_layout() {
    let mut ipv4 = v4([192, 0, 2, 7]);
    let mut ipv6 = v6(Ipv6Addr::LOCALHOST.octets());
    let v4_ptr = ptr::from_mut(&mut ipv4).cast();
    let v6_ptr = ptr::from_mut(&mut ipv6).cast();
    let mut nodes = [
        interface_node(c"four", v6_ptr),
        interface_node(c"six", v4_ptr),
        interface_node(c"unselected", v4_ptr),
    ];
    let config = AddressSelection::new(vec![
        ("four".into(), Families::V4),
        ("six".into(), Families::V6),
    ])
    .unwrap();
    assert!(addresses(&mut nodes, &config).is_none());
    let mut nodes = [
        interface_node(c"four", v4_ptr),
        interface_node(c"four", v4_ptr),
        interface_node(c"six", v6_ptr),
    ];
    assert_eq!(
        addresses(&mut nodes, &config),
        Some(vec![
            Ipv4Addr::new(192, 0, 2, 7).into(),
            Ipv6Addr::LOCALHOST.into()
        ])
    );
    for (ptr, family, extent) in [
        (v4_ptr, libc::AF_INET, size_of::<libc::sockaddr_in>()),
        (v6_ptr, libc::AF_INET6, size_of::<libc::sockaddr_in6>()),
    ] {
        // SAFETY: the fixture is initialized and has the given native extent.
        assert!(unsafe { read_address(NonNull::new(ptr).unwrap(), family, extent) }.is_some());
        // SAFETY: the alternate length is shorter than the live fixture.
        assert!(unsafe { read_address(NonNull::new(ptr).unwrap(), family, extent - 1) }.is_none());
    }

    // Supply readable bytes through the scan bound and a terminator beyond it.
    // The address body for an overlong native name must not be read.
    let mut long_name = [b'x'; libc::IFNAMSIZ + 1];
    long_name[libc::IFNAMSIZ] = 0;
    let mut malformed = interface_node(c"four", v4_ptr);
    malformed.ifa_name = long_name.as_mut_ptr().cast();
    assert!(addresses(&mut [malformed], &config).is_none());

    let mut values: Vec<_> = (1..=12).rev().map(|last| v4([192, 0, 2, last])).collect();
    let mut nodes: Vec<_> = values
        .iter_mut()
        .map(|value| interface_node(c"four", ptr::from_mut(value).cast()))
        .collect();
    let expected: Vec<IpAddr> = (1..=8)
        .map(|last| Ipv4Addr::new(192, 0, 2, last).into())
        .collect();
    assert_eq!(addresses(&mut nodes, &config), Some(expected));
    let mut oversized: Vec<_> = (0..=MAX_INTERFACE_RECORDS)
        .map(|_| interface_node(c"four", v4_ptr))
        .collect();
    assert!(addresses(&mut oversized, &config).is_none());

    for (native, expected) in [
        ("fe80:dead::1", "fe80::1"),
        ("ff01:dead::1", "ff01::1"),
        ("ff02:dead::1", "ff02::1"),
        ("ff32:dead::1", "ff32:dead::1"),
        ("2001:db8::2", "2001:db8::2"),
    ] {
        let native = native.parse::<Ipv6Addr>().unwrap();
        assert_eq!(
            Ipv6Addr::from(darwin_unscoped(native.octets())),
            expected.parse::<Ipv6Addr>().unwrap()
        );
    }
}

struct Fixture {
    username: Box<dyn FnMut() + Send>,
    probes: Arc<std::sync::Mutex<Vec<&'static str>>>,
}

impl Sources for Fixture {
    fn username(&mut self, _: &mut Deadline) -> Result<Option<String>, Stopped> {
        self.probes.lock().unwrap().push("username");
        (self.username)();
        Ok(Some("fixture".into()))
    }

    fn uptime(&mut self) -> Option<u64> {
        self.probes.lock().unwrap().push("uptime");
        None
    }

    fn load(&mut self) -> Option<[u32; 3]> {
        self.probes.lock().unwrap().push("load");
        Some([0; 3])
    }

    fn disk(&mut self, label: &str, _: &CStr) -> Option<DiskStatus> {
        self.probes.lock().unwrap().push("disk");
        disk_usage(label, 10, 4, 3, 1024)
    }

    fn addresses(
        &mut self,
        _: &AddressSelection,
        _: &mut Deadline,
    ) -> Result<Option<Vec<IpAddr>>, Stopped> {
        self.probes.lock().unwrap().push("addresses");
        Ok(Some(vec![Ipv4Addr::LOCALHOST.into()]))
    }
}

fn fixture() -> Fixture {
    Fixture {
        username: Box::new(|| {}),
        probes: Default::default(),
    }
}

fn codec(host: HostStatus) -> Vec<u8> {
    let record = Record::new(
        Hex32([42; 32]),
        Payload::Heartbeat(Heartbeat {
            sequence: NonZeroU64::MIN,
            sampled_at: 1,
            interval: 3600,
            host,
            permission: None,
        }),
    );
    let encoded = record
        .encode()
        .expect("sample must fit the heartbeat codec");
    assert!(Record::decode(&encoded).is_ok());
    encoded
}

#[test]
fn sampler_core_source_control_and_wire_qualification() {
    let mut source = fixture();
    let omitted = sample(&mut source, &empty_request(), &mut deadline()).unwrap();
    assert!(source.probes.lock().unwrap().is_empty());
    assert_eq!(omitted, HostStatus::default());

    let request = Request::new(
        selected_fields(),
        vec![
            ("zeta".into(), "/private/never/display".into()),
            ("alpha".into(), "/tmp".into()),
        ],
        None,
    )
    .unwrap();
    let host = sample(&mut source, &request, &mut deadline()).unwrap();
    assert_eq!(
        *source.probes.lock().unwrap(),
        ["username", "uptime", "load", "disk", "disk"]
    );
    assert!(host.uptime.is_none() && host.ips.is_none());
    let labels: Vec<_> = host
        .disks
        .as_ref()
        .unwrap()
        .iter()
        .map(|disk| disk.label.as_str())
        .collect();
    assert_eq!(labels, ["alpha", "zeta"]);
    let encoded = codec(host);
    assert!(
        !encoded
            .windows(b"/private/never/display".len())
            .any(|entry| entry == b"/private/never/display")
    );

    let selection = AddressSelection::new(vec![("native-fixture-only".into(), Families::V4)]);
    assert!(selection.is_none());
    let selection = AddressSelection::new(vec![("fixture0".into(), Families::V4)]);
    let request = Request::new(BaseFields::default(), Vec::new(), selection).unwrap();
    let with_ip = sample(&mut source, &request, &mut deadline()).unwrap();
    assert_eq!(with_ip.ips, Some(vec![Ipv4Addr::LOCALHOST.into()]));
    assert_eq!(source.probes.lock().unwrap().last(), Some(&"addresses"));
}

fn controlled_clock() -> (Arc<AtomicU64>, Clock) {
    let ticks = Arc::new(AtomicU64::new(1));
    let clock_ticks = Arc::clone(&ticks);
    let clock = Arc::new(move || match clock_ticks.load(Ordering::Acquire) {
        0 => None,
        u64::MAX => Some(Duration::MAX),
        millis => Some(Duration::from_millis(millis)),
    });
    (ticks, clock)
}

fn wait_for<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let stop = Instant::now() + Duration::from_secs(4);
    loop {
        if let Some(value) = check() {
            return value;
        }
        assert!(Instant::now() < stop, "sampler fixture did not finish");
        thread::yield_now();
    }
}

fn wait_progress(sampler: &mut Sampler) -> Progress {
    wait_for(|| match sampler.poll() {
        Progress::Pending => None,
        progress => Some(progress),
    })
}

fn username_only() -> Request {
    Request::new(
        BaseFields {
            username: true,
            ..BaseFields::default()
        },
        Vec::new(),
        None,
    )
    .unwrap()
}

fn blocking_fixture() -> (Fixture, Receiver<()>, SyncSender<()>) {
    let (entered, entered_observer) = sync_channel(1);
    let (release, resume) = sync_channel(1);
    let mut source = fixture();
    source.username = Box::new(move || {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    (source, entered_observer, release)
}

#[test]
fn sampler_core_worker_expiry_draining_and_permit_lifetime() {
    let (ticks, clock) = controlled_clock();
    let domain = Arc::new(AtomicBool::new(false));
    let separate_domain = Arc::new(AtomicBool::new(false));
    let (source, entered, release) = blocking_fixture();
    let mut sampler = Sampler::spawn_with(&domain, clock.clone(), source, spawn_thread).unwrap();
    let budget = Duration::from_millis(100);
    sampler.start(username_only(), budget).unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(matches!(
        Sampler::spawn_with(&domain, clock.clone(), fixture(), spawn_thread),
        Err(StartError::Busy)
    ));
    let mut independent =
        Sampler::spawn_with(&separate_domain, clock.clone(), fixture(), spawn_thread).unwrap();
    independent.start(empty_request(), budget).unwrap();
    assert!(matches!(
        wait_progress(&mut independent),
        Progress::Ready(_)
    ));

    ticks.store(101, Ordering::Release);
    assert!(matches!(sampler.poll(), Progress::Expired));
    assert!(matches!(sampler.poll(), Progress::Pending));
    assert_eq!(
        sampler.start(empty_request(), budget),
        Err(StartError::Busy)
    );
    release.send(()).unwrap();
    assert!(matches!(wait_progress(&mut sampler), Progress::Idle));
    sampler.start(empty_request(), budget).unwrap();
    assert!(matches!(wait_progress(&mut sampler), Progress::Ready(_)));

    sampler.start(username_only(), budget).unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(sampler);
    assert!(matches!(
        Sampler::spawn_with(&domain, clock.clone(), fixture(), spawn_thread),
        Err(StartError::Busy)
    ));
    release.send(()).unwrap();
    wait_for(|| (!domain.load(Ordering::Acquire)).then_some(()));
    let mut replacement = Sampler::spawn_with(&domain, clock, fixture(), spawn_thread).unwrap();
    replacement.start(empty_request(), budget).unwrap();
    assert!(matches!(
        wait_progress(&mut replacement),
        Progress::Ready(_)
    ));
}

#[test]
fn sampler_core_worker_clock_and_launch_failures() {
    let (ticks, clock) = controlled_clock();
    let domain = Arc::new(AtomicBool::new(false));
    let failed = Sampler::spawn_with(&domain, clock.clone(), fixture(), |_| {
        Err(io::Error::other("fixture launch failure"))
    });
    assert!(matches!(failed, Err(StartError::Unavailable)));
    assert!(!domain.load(Ordering::Acquire));

    let (source, entered, release) = blocking_fixture();
    let mut sampler = Sampler::spawn_with(&domain, clock.clone(), source, spawn_thread).unwrap();
    assert_eq!(
        sampler.start(empty_request(), Duration::ZERO),
        Err(StartError::InvalidBudget)
    );
    assert_eq!(
        sampler.start(empty_request(), MAX_BUDGET + Duration::from_nanos(1)),
        Err(StartError::InvalidBudget)
    );
    ticks.store(u64::MAX, Ordering::Release);
    assert_eq!(
        sampler.start(empty_request(), MAX_BUDGET),
        Err(StartError::InvalidBudget)
    );
    ticks.store(0, Ordering::Release);
    assert_eq!(
        sampler.start(empty_request(), MAX_BUDGET),
        Err(StartError::Unavailable)
    );

    ticks.store(10, Ordering::Release);
    sampler.start(username_only(), MAX_BUDGET).unwrap();
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    ticks.store(12, Ordering::Release);
    assert!(matches!(sampler.poll(), Progress::Pending));
    ticks.store(11, Ordering::Release);
    assert!(matches!(sampler.poll(), Progress::Unavailable));
    release.send(()).unwrap();
    assert!(matches!(wait_progress(&mut sampler), Progress::Idle));
    drop(sampler);
    wait_for(|| (!domain.load(Ordering::Acquire)).then_some(()));

    let mut panic_source = fixture();
    panic_source.username = Box::new(|| panic!("fixture source panic"));
    let mut sampler =
        Sampler::spawn_with(&domain, clock.clone(), panic_source, spawn_thread).unwrap();
    sampler.start(username_only(), MAX_BUDGET).unwrap();
    assert!(matches!(wait_progress(&mut sampler), Progress::Unavailable));
    wait_for(|| (!domain.load(Ordering::Acquire)).then_some(()));
    assert_eq!(
        sampler.start(empty_request(), MAX_BUDGET),
        Err(StartError::Unavailable)
    );
    assert!(Sampler::spawn_with(&domain, clock, fixture(), spawn_thread).is_ok());
}

#[test]
fn sampler_core_final_handover_checks_worker_and_current_time() {
    let (ticks, clock) = controlled_clock();
    let (jobs, _queue) = sync_channel(1);
    let mut sampler = Sampler {
        jobs,
        pending: None,
        clock: clock.clone(),
    };
    let queued_result = |completed_at| {
        let (response, receiver) = sync_channel(1);
        response
            .send(Ok((
                Duration::from_millis(completed_at),
                HostStatus::default(),
            )))
            .unwrap();
        Pending {
            response: receiver,
            deadline: Deadline {
                clock: clock.clone(),
                latest: Duration::from_millis(1),
                end: Duration::from_millis(10),
            },
            draining: false,
        }
    };
    ticks.store(10, Ordering::Release);
    sampler.pending = Some(queued_result(6));
    assert!(matches!(sampler.poll(), Progress::Expired));
    assert!(matches!(sampler.poll(), Progress::Idle));
    ticks.store(4, Ordering::Release);
    sampler.pending = Some(queued_result(6));
    assert!(matches!(sampler.poll(), Progress::Unavailable));
    ticks.store(7, Ordering::Release);
    sampler.pending = Some(queued_result(6));
    assert!(matches!(sampler.poll(), Progress::Ready(_)));
    assert!(matches!(sampler.poll(), Progress::Idle));
}

#[test]
#[ignore = "opt-in native platform and latency qualification"]
fn sampler_native_worker_and_benchmark() {
    fn measure(sampler: &mut Sampler, request: Request) -> (HostStatus, Duration) {
        let start = Instant::now();
        sampler.start(request, MAX_BUDGET).unwrap();
        let Progress::Ready(host) = wait_progress(sampler) else {
            panic!("native sample did not complete");
        };
        (host, start.elapsed())
    }

    let fixture = tempfile::tempdir().unwrap();
    let loopback = if cfg!(target_os = "macos") {
        "lo0"
    } else {
        "lo"
    };
    let request = |families: Option<Families>| {
        let addresses = families
            .map(|families| AddressSelection::new(vec![(loopback.to_string(), families)]).unwrap());
        Request::new(
            selected_fields(),
            vec![
                ("present".into(), fixture.path().into()),
                ("missing".into(), fixture.path().join("missing")),
            ],
            addresses,
        )
        .unwrap()
    };
    let mut sampler = Sampler::spawn().unwrap();
    let (disabled, _) = measure(&mut sampler, request(None));
    assert!(disabled.ips.is_none());
    let (enabled, _) = measure(&mut sampler, request(Some(Families::V4)));
    assert!(enabled.username.is_some(), "native username lookup failed");
    assert!(enabled.uptime.is_some(), "native uptime failed");
    assert!(enabled.load.is_some(), "native load failed");
    let disks = enabled
        .disks
        .as_ref()
        .expect("native volume should be present");
    assert!(disks.len() == 1 && disks[0].label == "present");
    assert!(enabled.ips.as_deref() == Some(&[Ipv4Addr::LOCALHOST.into()]));
    assert!(!codec(enabled).is_empty());
    let (ipv6, _) = measure(&mut sampler, request(Some(Families::V6)));
    assert!(
        ipv6.ips
            .as_deref()
            .is_none_or(|addresses| addresses == [IpAddr::V6(Ipv6Addr::LOCALHOST)]),
        "native IPv6 loopback returned an ineligible address"
    );

    let mut elapsed = Vec::with_capacity(200);
    for _ in 0..200 {
        let (host, duration) = measure(&mut sampler, request(Some(Families::V4)));
        assert!(host.valid());
        elapsed.push(duration);
    }
    elapsed.sort_unstable();
    let percentile = |p: usize| elapsed[(p * elapsed.len()).div_ceil(100) - 1].as_micros();
    println!(
        "native private heartbeat samples={} p50_us={} p95_us={} p99_us={} max_us={}",
        elapsed.len(),
        percentile(50),
        percentile(95),
        percentile(99),
        percentile(100)
    );
}
