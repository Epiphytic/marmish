# Candidate startup clock validation

On Linux AMD64 the libtest at `84068b9` passed all seven readiness
checks. One deterministic clock case exercised both edge orders,
duplicate delivery, a late observer, the exact deadline and immediate
stop. Five separate opt-in private WSS daemon lifecycle checks passed
in 22.89 s. They exercised binding refusal, a transient accept error,
unexpected listener exit, continued ordinary service after local
registration refusal and lease retention during stop.

Strict all-target Clippy passed in stock mode on `84068b9`
(28.55 s) and on composed `8b2367d` in candidate mode (28.33 s).

The private receiver remains unmounted. The installed actor still
needs to apply this startup allowance to its local recovery and
initial WSS authentication.
