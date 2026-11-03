# Public native command CLI observation — 2026-11-03

The uncommitted native v0.2 increment passed a real Linux AMD64 public CLI
exchange through the explicitly selected `wss://relay.us.whitenoise.chat/` relay.
Both endpoints used fresh file-backed disposable identities, offline `setup`,
explicit execution/trust grants and separate Rust `marmishd` processes. The
client addressed the server as `npub@relay.us.whitenoise.chat` and invoked the
canonical `/usr/bin/cat` executable with finite stdin. Its 61 literal/Unicode
input bytes returned exactly, with remote exit 0 in 5044 ms. This is one
end-to-end wall-time observation, not a sustained throughput or RTT benchmark.

The same server profile was reopened after joined ordinary shutdown. Local
`command-prepare-cleanup` completed a new observation fence and actual consuming
engine cleanup. Cached status reported quiesced; every server row was fresh at
that fence, retained authority and proved worker exit. Joined stop then reported
engine quiescence and engine/MDK/runtime closure. No engine state was removed;
private fixtures were preserved. Ordinary stop alone was not treated as deletion
authority.

The [sanitized machine-readable result](public-cli-cat-2026-11-03.json) records
hashes, actual counters and limitations. This did not exercise installation or
cross-platform interoperability and does not qualify a release. A separate
`/bin/printf` invocation correctly failed canonical executable validation. A
canonical `/usr/bin/printf` invocation preserved its output but exposed an
empty-stdin `InputFailed` defect; its correction and repeat remain required.
