# Registered Command admission and Peer route revocation

Private Linux AMD64 candidate source `c51e92b52779517c328c9a97bc4d0cde76a79803`
pairs the Command worker with the exact registration's fatal signal,
default verifier and custody callback. It includes the independently
[reviewed SQLCipher guard](peer-candidate-engine-sqlcipher-fence.md).
This is source validation of the worker and route boundaries. The installed
daemon still needs to mount them.

| Exact composed check | Linux AMD64 result |
| --- | --- |
| Stock Engine library | 154 passed, 1 exec-only child fixture ignored, 37.73 s |
| Stock native Command engine and lifecycle | 31 passed with 1 ignored, 7.59 s. Lifecycle 6 passed, 4.10 s |
| Candidate native Command engine and lifecycle | 31 passed with 1 ignored, 10.67 s. Lifecycle 6 passed, 3.64 s |
| Candidate dispatcher tests | 3 passed |
| Candidate SQLCipher registration subprocess suite | 14 fresh-process modes passed, 55.71 s |
| Stock native and engine test links and strict all-target Clippy | 42.28 s and 28.03 s |
| Candidate native test link and strict all-target Clippy | 44.55 s and 29.37 s |
| Non-test candidate negative compile | Only E0599, arbitrary Trip cannot create `Admission::new` |

The registration matrix invokes the stored production `owner_failed`
callback from the exact admission under the held registry and during
the delayed notification cut. The first case observes the original trip
without relocking the registry. The second observes the registered waiter
wake before the delayed notifier is released. Independent Trip stays
healthy. A separate Engine test verifies custody after failed native close.
A single cross-crate test that forces that actual failed close through the
registered native worker remains unmeasured.

\`\`\`text
4 exact libtest checksums (SHA-256)
f051ebc78d087d5e8c565002a3c0b0883b6bf3156cfd8d96e593ea007b05d7db  c51e92b stock Engine
f822b01fbd89d652f106e42e5a6a8e84c06c5e81690a8d00abc9a328a1ca1a46  c51e92b stock native
6e14e67012e3785c7f90282d5f4468b619a244014378a4dcab42ffa56bf2b318  c51e92b candidate native
df07e97d6474ecb4e8dbe3ec7eb1ef4324342a9ac218cc4b4f9c5fc7af574354  fa58722 candidate native
\`\`\`

Candidate revocation source `fa5872282b9c9b9b3d9b79f665e9b402ad9eb540`
passed all eight fresh-process registered-route cases over private WSS
with SQLCipher in 35.50 s. Candidate strict all-target Clippy passed in
27.90 s. Revoke before open did zero TCP attempts. The live route refused
reap while authorized, then refused use after revoke. The port dropped
before the paired raw permit returned, and reap completed. Both cases
left the exact dispatcher healthy and sent no Nostr subscription CLOSE.
Reap demonstrates local ownership return. The new cases do not separately
time when a relay observes the remote disconnect. Real fatal socket
displacement is covered by the
[raw WSS evidence](peer-candidate-raw-transport-fatal.md).
