# Candidate server inventory CLI validation

The private Linux AMD64 candidate client parses `marmish servers list` and
`marmish servers show --server NPUB --offer-id HEX --revision HEX --generation N`.
It accepts the existing local profile and socket options and sends one typed
read request. The candidate listener still returns `peer_unavailable` until
its actor is mounted. The preceding [same-UID IPC proof](peer-candidate-read-ipc.md)
exercises that result on a real socket.

The CLI tests compare parsed argv, trailing arguments, explicit user and state
selection with the ordinary parser. They cover a `servers` remote alias, an
explicit relay-qualified alias, an npub at a relay, explicit `-u`, and the
ordinary rsync parser. Local negative tests cover malformed exact selectors,
a stray delimiter, routing and write options, and option values that resemble
flags. Help and command registration are checked in both client and daemon
parse paths. These are parsing and local request checks. Installed Pending
visibility, restart and the actual mounted worker remain part of the
[daemon mount qualification](../phases/peer-candidate-daemon-mount.md).

| Check | Result |
| --- | --- |
| `cargo test --locked --offline -p marmish-native --lib --target x86_64-unknown-linux-gnu -j 2 cli::tests:: -- --test-threads=1` with candidate cfg, exact `f9c6f57` source stamp and debug info disabled | 65 passed. Build took 39.10 s and tests took 12.25 s. |
| `cargo clippy --locked --offline -p marmish-native --lib --bins --target x86_64-unknown-linux-gnu -j 2 -- -D warnings` with candidate cfg and exact source `1b37232` | Passed in 12.13 s. The follow-up changes tests only. |
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |
| Stock locked native `cargo test -p marmish-native --lib cli::tests:: -- --test-threads=2`, exact composed `cfd68df`, Linux AMD64 | 64 passed, zero failed or ignored. Tests took 9.45 s. |
