# Terminal grant validation

Validated on Linux x86_64 against the native tmux worker baseline. The optional
`terminal.clients` setting grants access only to an existing execution client
with an explicitly enrolled key. It defaults to empty. Revoking Command
access removes both grants, and restoring Command access alone leaves
Terminal disabled.

The exact candidate passed the focused grant tests (4), command
configuration tests (20), inventory tests (17), and native setup tests
(11). The focused grant tests overlap the other selections. The two
controlling-terminal tests are opt-in and were outside this run. The
inventory tests covered new-client decoding of old replies, explicit
old-client rejection of the new entry shape, and real list/show serialization.

Claude Code approved the design and the exact implementation in session
`e649ba2a-f415-4b3e-9f7d-f565b8fffa44`. Its independent selection passed
48 tests with two ignored, and strict all-target Clippy passed. The new
inventory grant is visible now. Interactive shell availability stays false
pending Terminal transport integration. This phase does not measure
transport latency.
