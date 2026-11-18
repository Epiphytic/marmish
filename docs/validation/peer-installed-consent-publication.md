# Prepared Consent publication proof

Linux AMD64, source `3b8308f7baf2dc69f269f66bde50a95dc5009cbf`. The library
fixture used the capped installed SQLCipher cursor, a fresh `ProfileLease`,
real staged native SQLCipher storage, a qualified loopback WSS relay, and
disposable accounts. All times are wall time for independent scenarios and
include fixture setup. The test selector is opt-in.

| Scenario | Time | Evidence |
| --- | ---: | --- |
| Accepted | 12.13 s | The authenticated port sent exactly the queued bytes. Reopening the cursor showed attempt 1 and a durable acknowledgement. |
| Staged image replaced | 11.19 s | The stage inode was replaced after `start_send`. The exact file check refused before flush, retired the socket, and the relay recorded zero Consent writes. The retry remained queued. |
| Explicit relay rejection | 11.52 s | The relay recorded one exact write. The reopened cursor retained attempt 1 and the retry timestamp without an acknowledgement. |
| Settlement database read-only | 12.61 s | The relay recorded one exact write. The positive ACK reached the settlement turn, which failed at the SQL write and retired the port. The reopened row remained queued. |
| Lost ACK and retry | 74.06 s | A fresh cursor, source, and socket could send nothing before the durable retry time. After the real 60-second interval the relay received the same carrier with identical digest and byte count. Cold readback showed attempt 2, one durable acknowledgement, and no third send. |
| Stale attempt and expiry | 11.01 s | Deadline check refused at the bound and the new checkpoint rejected an attempt from the old owner. The relay recorded zero Consent writes. |

Each scenario also checked that a stale socket deadline, wrong account, and
wrong route left sequence, retry, acknowledgement, and relay capture unchanged.
The legacy generic attester still refused the installed layout. The suite
passed in 132.52 seconds. The existing real authenticated WSS test for foreign,
current, and retired ACK handling also passed in 20.30 seconds.

## Commands

```sh
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo clippy --offline --locked -j2 -p marmish-native --features installed-checks --all-targets -- -D warnings
cargo test --offline --locked -j2 -p marmish-native --features installed-checks --test peer_cursor_store
cargo test --offline --locked -j2 -p marmish-native --features installed-checks --lib peer_authenticated_route
cargo test --offline --locked -j2 -p marmish-native --features installed-checks --lib \
  peer_invitation::socket::owner::recipient::tests::peer_authenticated_route_private_wss_installed_marker_and_exact_send \
  -- --ignored --exact --nocapture
cargo test --offline --locked -j2 -p marmish-native --features installed-checks --lib \
  peer_invitation::prepared_consent::installed_consent::tests::peer_installed_consent_private_wss \
  -- --ignored --exact --nocapture
git diff --check b9ba9076543e5bd73344bd1e9801c43b7228d6b8
```

Strict Clippy passed. Standalone cursor tests reported 72 passed and four
ignored. The focused authenticated-route suite reported four passed and one
ignored. The ignored private WSS route case was run separately as listed above.

The [approved design](../phases/peer-installed-consent-publication.md) qualifies
this library proof on a local relay. Installed daemon startup, native charged
admission, Delivery and Welcome are outside this test. No public relay was used.

## Integrated source results

The Linux AMD64 integrated run tested
`2c18a7de05d14a0e589fcd0f1c20cb424c3d4638` on `515557a`.
The publication source `af22ce8` has identical non-documentation files
and includes matrix evidence in its parent. The tested binary's source
stamp identifies the run at `2c18a7d`. The parent suite passed all six
independent WSS scenarios in 127.15 seconds.

| Scenario | Integrated time |
| --- | ---: |
| Accepted | 11.112 s |
| Staged image replaced | 10.677 s |
| Explicit relay rejection | 10.795 s |
| Settlement database read-only | 11.367 s |
| Lost ACK and retry | 71.493 s |
| Stale attempt and expiry | 11.699 s |

The integrated run used the same tests and the same offline two-worker
settings above. Its broad suite selector also matched the child launcher
in the top-level process. Scenario counts here come from the parent-run
child trials only.

The separate exact live-ACK regression passed in 20.05 seconds. The
four fast route cases, all 72 standalone cursor cases (four opt-in cases
ignored), and full workspace formatting passed. Strict installed-checks
all-target Clippy passed in 43.93 seconds. The ordinary Command during
Peer outage regression passed in 24.61 seconds using the same test binary.
