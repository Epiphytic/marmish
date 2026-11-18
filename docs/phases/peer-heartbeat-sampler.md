# Private Peer heartbeat sampler

This increment adds a private local collector for the reviewed
[status codec](peer-status-codec.md). It follows the
[host sampling calibration](peer-host-sampling-proof.md). A stand-alone
integration target named `peer_heartbeat_sampler` mounts the sampler,
`peer_status_wire` and `command_limits` via path modules. It imports the
native crate's `Error` and `Result` for the clock module and scopes
`allow(dead_code)` to the standalone clock mount. The clock and codec
tests compiled by these mounts may run in the standalone target. The
existing codec target stays available. Required CI keeps its existing
target selection. This new target is qualified independently.
The daemon, CLI, configuration, Peer scheduler and transport do not change.

## Activation and visibility

Production status remains opt-in. Enabling it uses a one-hour interval
unless configured otherwise. The human-approved private increment takes
precedence over the older absence-enables wording in
[the heartbeat draft](peer-heartbeats.md). An enabled caller
selects visible username, uptime, load and configured disks. Network
addresses remain off unless the operator explicitly selects local interfaces
and families, the future caller proves the current Peer and relay route
allow address sharing, and the global selection and current per-key grant
allow that audience. A common sample may collect the union of fields visible
to due peers. Its caller narrows the snapshot separately for each peer. The sampler has no route or audience knowledge.
Completion gives the caller a local memory snapshot. It grants no authority
to sign, advertise, route or publish it. No production mount is in scope.

The input is an owned request with selected base fields, at most eight
disk label and absolute path pairs, and an optional explicit address
selection of up to eight unique interface names with a per-interface IPv4,
IPv6 or dual-stack family. Validate inputs before starting native work.
Disk labels follow the codec's existing 1–32 UTF-8 byte validation and are
unique and slash-free. Paths contain native Unix bytes, including non-UTF-8.
They begin with one slash, reject a double leading slash and NUL, and contain
at most 4,095 bytes excluding the terminator. Resolve them through the OS at
sampling time. Paths beyond its own limit are unavailable. Interface names are nonempty, shorter than IFNAMSIZ and contain no NUL.
They are exact local names, with no wildcard. The output is the existing
`HostStatus`. Sort disks by label. Missing, invalid or empty results are
omitted. Paths, interface names, native errors and samples are never logged.
The only shared source edits expose `MAX_DISKS`, `MAX_IPS`, `MAX_UPTIME`,
`MAX_LOAD`, `safe_text`, and the existing `HostStatus::valid` and
`DiskStatus::valid` to the crate. The wire behavior is unchanged.

## Blocking work and deadlines

Use one dedicated named thread with at most one live request and one private
result slot. The production factory uses one process-local permit domain.
Tests can supply independent permit domains and exercise them concurrently.
A permit prevents invalidation or drop followed by replacement while the
worker remains alive. The worker holds the permit until it exits. Failed
spawn and unwind release it. A closed result channel reports unavailable;
an owner may be recreated after the previous worker releases its permit.
The future caller uses the same live worker across due requests.

The owner starts an operation nonblockingly only when idle. Start takes a
budget with `0 < budget <= 2s` and pins its absolute deadline by checked
addition to the current suspend-aware `command_limits::continuous_time()`.
Invalid budgets, unavailable clock and addition overflow refuse the start.
The owner and worker each track the greatest clock value seen during a
request, starting at acceptance. A smaller reading or clock error fails
closed. Poll is nonblocking and yields idle, pending, ready, expired, or
unavailable. An all-omitted sample counts as ready. The worker checks around
each native source, between configured disks, during bounded address
iteration and on completion. Just before handing out a result the owner
re-reads the clock and also checks against the worker's completion time.
An expired or unavailable request is reported at most once and drains its
own result before another start. A late result never escapes after expiry.
A source error before the deadline omits only that source. The owner never waits for a blocked thread. Drop closes input
and discards any pending result without blocking. A wedged native call can
keep sampling unavailable, with no accumulating work or unbounded thread
replacement. The thread and pending request may live until process exit.
This is a deadline on caller-visible collection, rather than asynchronous
cancellation of an operating system call.

## Native sources

- Effective username comes from `geteuid` and `getpwuid_r`. Cap retry
  buffers at 16 KiB. Require the native result to point to the supplied
  initialized record and the name into the supplied buffer. Validate the
  at-most-64-byte NUL-terminated UTF-8 name using the codec's text rule.
  Ignore environment variables. NSS and macOS Directory Services may
  contact administrator-configured backends. Marmish does not perform a
  network lookup or launch a resolver subprocess.
- Linux host uptime is the existing `CLOCK_BOOTTIME` value. On macOS read
  the exact `timeval` from `kern.boottime` and subtract it from the validated
  wall clock, rejecting future or malformed boot times and out-of-range
  results. It is a wall-derived estimate. Production scheduling still uses
  the continuous clock. Actual macOS sleep qualification is separate.
- `getloadavg` must return exactly three finite nonnegative values. Round
  each to thousandths and apply the codec's fixed integer range.
- `statvfs` reads only configured paths. Require nonzero total and fragment size (`f_frsize`), available blocks
  at most free blocks, free blocks at most total, no total-byte or
  component-byte multiplication overflow, and a checked nonzero sum of
  visible used and available bytes. Disk
  usage is used bytes (total minus free) and bytes available to the sampler.
  Filesystems can be remote or can trigger an automount per local policy.
- Use `getifaddrs` only when an explicit address selection exists. Inspect
  only the address bodies for matching names and families. Check the full
  Darwin sockaddr length before body access, and use the getifaddrs
  family-specific layout contract on Linux. Visit the entire list up to
  4,096 nodes, even after eight matches. Omit the entire IP field if the
  bound is exceeded. Match each interface's configured families and retain
  the eight smallest unique addresses in the codec's canonical order.
  Ignore the separate scope identifier. On Darwin remove an embedded
  interface scope word from link-local unicast and the XNU interface-local
  and link-local multicast classes. Preserve the multicast prefix class
  XNU excludes from embedding. The code follows XNU's `IN6_IS_SCOPE_EMBED`. Release the native list on all
  paths. No DNS, outbound IP discovery, subprocess or new dependencies.

Native references.
[Linux clock API](https://www.man7.org/linux/man-pages/man3/clock_gettime.3.html) ·
[Apple sysctl boot time](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/sysctl.3.html) ·
[XNU IPv6 scope definitions](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/netinet6/in6.h)

## Qualification

Use deterministic fixtures for configuration limits, selected source call
counts, invalid buffers, username and label controls and bidi marks, clock,
load and disk arithmetic, both address layouts and truncation, per-interface
family selection, duplicate and Darwin scope handling, and codec encode/decode. Qualify a deliberately blocked collector
without sleeps by coordinating synthetic work and checking no overlap,
expiry, stale result disposal, permit retention across owner drop, and
recovery after release. Exercise worker panic, thread-spawn failure and
independent permit domains. Verify the shared codec diff changes only the
named visibility and the daemon mount remains unchanged.

An ignored native test selects an explicit private filesystem and loopback
IPv4 or IPv6 on a Linux or macOS host, checks the absent addresses path
and exercises the dedicated worker. The fixture verifies that available
base data validates and that an encoded sample includes only selected
disk labels and eligible addresses. Collect at least 200 end-to-end sample
latencies, report sample count and nearest-rank p50/p95, and print no raw
data. Record the native platform and whether macOS and Linux ARM64 were
actually exercised. Keep the opt-in run and timing out of required CI.
Run strict Clippy and the sampler and existing codec tests. Record completed
source and design reviews before the increment is handed to integration.
