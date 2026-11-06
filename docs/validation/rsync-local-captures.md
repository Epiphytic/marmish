# Local rsync bootstrap qualification

Linux, installed rsync **3.2.7**, protocol 31. The following stdlib test passed
all **8 methods** (including three iconv subcases):

```sh
python3 -m unittest discover -s tools -p 'test_marmish_rsync_capture.py' -v
```

This runs real rsync with a disposable local capture executable. It never invokes
Marmish, launches a remote process or contacts a relay. The capture executable
records its argv then exits; rsync exit **12** is expected because no protocol
peer exists. It is not file-transfer success. Temporary private state is removed,
process waits are bounded, and owned unreaped process groups are cleaned on
failure. Environment inherited by fixture children excludes unrelated secrets.
Python compilation also passed.

Observed bootstrap tails after the host/login prefix:

| Case | Captured executable and tail |
| --- | --- |
| Push `-as` | `rsync --server -slogDtpre.iLsfxCIvu` |
| Pull `-as` | `rsync --server --sender -slogDtpre.iLsfxCIvu` |
| `--iconv=utf8` | previous push tail plus `--iconv=utf8` |
| `--iconv=UTF-8,ISO-8859-1` | previous push tail plus `--iconv=ISO-8859-1` |
| `--iconv=.` | previous push tail plus `--iconv=.` |

`npub1test@relay.example:PATH` generates prefix `-l npub1test relay.example`.
A bare `npub1test:PATH` generates only `npub1test`; explicitly placing
`-l remote-alias` in the remote-shell invocation preserves that prefix literally.
With `-s`, special destination paths and remote `--temp-dir` never appear in the
bootstrap. With `-a` alone, the capability suffix contains a misleading `s` but
there is no secluded `s` before the first `e`; the bootstrap also includes a dot
and pathname tail. The adapter must reject that form. `-as --old-args` fails
locally before invoking the remote shell.

These shapes agree with the approved design and production Rust parser fixtures.
The capture tool itself does not invoke the production parser or prove native
Marmot transfer success; those are separately scoped Rust and live tests. Missing
rsync or versions below 3.2 skip with an explicit explanation, including old macOS
system rsync. Iconv subcases skip when rsync was built without iconv. Other modern
versions are checked against the supported grammar; exact 3.2.7 clusters are
asserted only for the grounded version. No macOS capture was run here.
