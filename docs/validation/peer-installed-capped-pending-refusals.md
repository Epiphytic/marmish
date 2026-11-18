# Capped Pending refusals and uncertain completion validation

The author qualified exact source `0bf8107e825d39795676e097bd304e604bc480bb` on
Linux AMD64 with the lockfile at that commit. A warm
`cargo test --locked -j2 -p marmish-native --lib --test peer_cursor_store --no-run`
completed in 49.83 s using debug information disabled for dev and test. The
signed-store check and both WSS selectors used the native executable from that
build. The corresponding standalone cursor executable also ran in full.

| Check | Result | Wall time |
| --- | --- | ---: |
| `peer_installed_custody_reopens_signed_prepared_consent_and_held_delivery` | Passed | 0.78 s |
| `private_v2_pending_survives_cold_cuts` (ignored; selected explicitly) | Passed, three scenarios | 55.76 s |
| `private_v2_pending_recovers_uncertain_join` (ignored; selected explicitly) | Passed | 18.91 s |
| `peer_cursor_store` default suite | 76 passed, 4 ignored | 58.54 s |

The signed-store fixture measured no Welcome poll or custody change for the
legacy start, competing signed Delivery, exact expiry boundary, changed policy,
or damaged retained frame. The last case preserved the Held row and returned
the typed `Package` refusal. The fixture compared the complete schema inventory
before and after the controlled SQL mutation. The native WSS fixture measured
zero polls, closed joins and custody changes for the wrong stage inode,
foreign lease, stale prior-owner token, retired port, and retained epoch
mismatch. The last case reached the exact `Cursor(Stale)` refusal after an
otherwise successful cold Pending readback and exact Delivery replay.

The capacity cut measured one native Welcome poll and zero verified closes or
custody changes. The uncertain cut returned typed `Checkpoint(Source)` to the
caller after one real native Welcome poll and one checked close, with zero
custody changes. A retry was refused in that process. A new process verified the
exact authenticated native join and committed one Pending row without another
Welcome poll. A further cold readback and Delivery replay changed no counters.
Each of the four WSS scenarios recorded zero relay publication attempts
during the bridge windows. Earlier control-plane traffic used the isolated
loopback TLS relay with disposable accounts.

These are test-fixture cuts and cover a successfully persisted native completion
whose result is withheld from custody. The managed hot-journal cut has separate
[recovery evidence](peer-receiver-charge-recovery.md). No installed worker,
cross-platform runtime, or on-disk corruption recovery claim follows from these
selectors. Focused rustfmt and `git diff --check` passed. Strict Clippy on the
final composition is separate from this author run.

The native libtest executable SHA-256 was
`2e4aa63ba2ecd02432c16e6301af175f4d96f1946acb8dec31cbbf11d296bdd4`.
The standalone cursor executable SHA-256 was
`8ac88abc01609c3da804c1d3cae3a1e27254604d42f858beb057e413683863b6`.
