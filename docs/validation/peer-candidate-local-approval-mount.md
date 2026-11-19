# Installed candidate-local approval validation

The private Linux AMD64 candidate and stock were built as optimized native
packages and installed into separate fresh prefixes with the shipped Rust
installer. Neither package was published. The candidate used source
`dca025b35a71f17be860a914890995d89e033070` and manifest SHA-256
`9754147e2275c5c8bc70343dbd3bf98181d3f5d7d7ffb4e02b31f4ea63d48794`.
The stock Command package used source
`06f7de75b4590074102a338965a9f829c678ea15` and manifest SHA-256
`ec441683117f72a33b45b6c48a118213a1cef9e5fd1edcbb9a46aa8339460102`.
The manifest's payload digests and the installer receipt were checked
through the normal installation and executable-layout paths.

The corrected test harness from `bcc2a7a` used the original candidate's
commit stamp in its test executable so the executable-layout check matches
the installed package. The installed binaries were the original optimized
archives. Source after the candidate package adds test changes and an
extraction of the existing Manual lifecycle check, reviewed as behavior
preserving. The stock and candidate libtests from `44c4c25` both include the
fresh-epoch expectation changes.

The stock custody and CLI suite passed 12 tests. The candidate approval
suite passed 16 tests with its installed opt-in case deselected. The
corrected candidate CLI and actual same-UID approval socket selection
passed 12 tests, including two shared command-line checks. These selections
overlap and should be counted individually. The new retired-port case
exercised the port refusal and the lifecycle recheck without a relay.

The first installed run received success from the CLI and an immediate
retry before timing out on an invalid harness expectation. The private
reader's only production projection is a verified Pending entry. After
the approval invalidates that proof, it answers Unavailable. The corrected
case checks that response independently from continued registered route
health and the offline SQLCipher decision record.

A second attempt stopped at native layout verification before startup. Its
new test executable initially carried a newer source stamp than the
installed candidate manifest. Rebuilding the test executable with the
candidate package stamp allowed the original installation checks to run.
That run recorded the decision, passed the invalidated read projection
and found the exact Activating row and both monotonic floors. Its keeper
assertion had searched fixed Active-stage slots. The candidate's joined
file is the checked provisional file. Harness revision `f557090` uses
the stored device and inode with the same binding and checked reopen
path as the product.

## Installed result

The corrected installed case passed in 54.36 s on Linux AMD64. It used
disposable identities, a private registered WSS Peer relay, a separate
private ordinary Command relay and the installed native applications.
An authenticated founder drove Offer, Consent and Delivery. The recipient
projected the verified Pending invitation, refused the wrong generation,
and ran the matching installed `marmish servers approve` command. The
command emitted exactly `recorded local approval` and exited zero.
An immediate exact retry succeeded. List and Show withheld the invalidated
Pending proof while the registered route was still live. The test checked
that the Peer relay saw exactly the three allowed control publications.

When the private Peer route closed, ordinary Command still returned the
exact expected bytes. The cold SQLCipher read found the Activating row,
checked both persisted wall floors and reopened the original keeper by
recorded device and inode. After daemon restart the original selection
again returned success, with no second decision or observable Peer egress.
The test checked the same decision timestamp and keeper, and monotonic
floors, after the restart. The separately installed stock daemon read both
ordinary command results from the profile.

| Measurement | Elapsed |
| --- | ---: |
| First offer through verified Pending | 22,643 ms |
| Installed CLI process launch through the approval reply | 799 ms |
| Immediate typed Unix socket retry | 289 ms |
| Cold candidate restart through exact retry and projection check | 1,785 ms |
| First ordinary command | 4,489 ms |
| Ordinary command after Peer route close | 3,872 ms |

These are single-run loopback measurements. The CLI timing includes process
startup and foreground waiting. An internal decision-only latency was not
measured. The test does not activate a server or publish a receipt, heartbeat
or group message. Installed old-epoch startup refusal, a committed reply
lost in flight and mutations during authenticated history still lack
composed live coverage. Their underlying schema, local socket and SQL
transaction tests are separate evidence.
