# Candidate quota and route ownership validation

Linux AMD64 source `685476c` built the focused native libtest in
41.36 s. The one-shot route selector passed six isolated private WSS
cases in 27.61 s. It kept the attached quota usable across the exact
route move, authenticated empty page, and refused second take.
The independent registration selector passed seven isolated SQLCipher
cases in 30.19 s. The test that displaces the dispatcher directly
compares the controller's private quota reference with the returned
borrow. The clock and cold physical-route regressions both passed
in 10.36 s.

Candidate all-target strict Clippy passed in 31.21 s. Stock strict
Clippy passed on the same implementation at `fe671f6` before the
pointer-assertion-only update. In a separate disposable candidate
worktree, a sibling probe attempting direct controller construction
failed only with E0451. Dropping the registration and then using
its borrowed controller failed only with E0505. Neither probe
entered source.

This is a candidate ownership prerequisite. The combined receiver
still needs to prove that a recovered keyed cursor remains available
across the live transfer and that the installed daemon handles shutdown.
