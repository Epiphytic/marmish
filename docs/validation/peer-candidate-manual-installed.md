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

An independent Passive controller passed in 29.86 seconds with the same
installed artifacts. Its Peer access acknowledgement took 149 ms. Ordinary
commands completed in 4,938 ms with the Peer route active and 1,767 ms
after disconnection. The daemon stopped in 77 ms. These are individual
observations over loopback WSS.

| Installed artifact SHA-256 | Candidate | Stock |
| --- | --- | --- |
| Release manifest | `4eff977d0cf9ec6046b83139a0627217f1cb7406bc086956bc7194f412fe59ed` | `6a7dd1da94d7371a1914c45225d73c76390191ca5e7125b82b399004dfa8ded2` |
| Daemon | `bbb620c9bf0913451e526fb0475304beea7cb963017db34b078674de11e68b56` | `127d75c704ed59b1dbffc77b741c9162f3524360b6fc906b5789cec398648a68` |
| Controller libtest | `b981929f2484e4d3c129bc6cd359505bd2a1bd9573ba1e225a4d7179eef2b323` | `942c4ab3b4598aa9ab13be3ae313451c2cd6436b3b9c0afacb9c3ac5315f5aec` |

Private qualification helper SHA-256
`dc85a0060760428d96d112580eab01f992ef708b6bcc45d59053272d915f4711`.

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
