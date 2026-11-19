# Installed Manual Pending and command isolation

This run used native Linux AMD64 candidate and stock artifacts stamped
`78b4d398e5d943089a6ff896f4495b4f9cc4640f`, locked dependencies, separate
private install prefixes, and the native installer with pinned manifest
SHA-256. Each daemon used disposable identities and two independently
authenticated loopback WSS relays. The ordinary command identity, the
independent Peer founder, and the fixture's extra Offer writer were
distinct. Strict all-target Clippy passed for the stock and candidate
builds.

The installed Manual controller passed in 47.83 seconds. Before startup
commit it observed no Peer dial, no receiver cursor and no local Peer
read. After commit, it observed the exact authenticated live and history
subscriptions. The independent founder completed one Offer, one verified
Consent and one Delivery. No Pending read succeeded before Delivery.
After native Welcome ingest, List and Show returned the same exact
Pending metadata, and a wrong selector was refused.

The first ordinary `cat` command completed while the Peer route remained
active. After that exact route closed, Peer reads became unavailable and
the daemon completed a second ordinary command without redialing Peer.
The commands preserved distinct binary inputs including NUL and non-UTF-8.
A fresh Manual process restored the identical Pending entry without
new control publications. A separately installed stock process then
read exactly the two retained ordinary command results.

| Measurement | One observed run |
| --- | ---: |
| Startup commit to ready Peer route | 183 ms |
| Offer publication to first exact Pending | 11,738 ms |
| Ordinary command with Peer route active | 1,671 ms |
| Peer loss to local unavailability | 10 ms |
| Ordinary command after Peer loss | 1,496 ms |
| Manual daemon stop | 129 ms |
| Cold Manual reopen to exact Pending | 6,455 ms |
| Cold stock reopen and ordinary result validation | 290 ms |

Earlier runs at `1be3388` and `a73c9af` reached the authenticated live
route but timed out waiting for Consent. The fixture had put the ordinary
command identity in the relay's primary founder slot. Its extra writer
could send an Offer to the recipient, but the relay only permitted the
recipient to reply to the configured primary founder. Correcting the
roles preserved the relay ACL. A preview with the corrected controller
passed in 51.42 seconds before the exact-source rerun above.

The validation covered the private Linux AMD64 candidate and stock
command compatibility. The near-cap Pending refresh qualification,
other release targets, application acceptance, and public Peer
activation remain separate.

[Integrated review](../reviews/peer-candidate-manual-installed.md)
