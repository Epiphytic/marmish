# Healthy registered Command daemon on Linux AMD64

Source `62665aaac2cddc3c3e7fc8959f7fe029b5ebc0fb` was built for Linux
AMD64 with the glibc 2.39 baseline. The native installer verified the
digest-pinned release manifest and built the versioned layout. The gated
qualification example reused that layout from a fresh process. The
test verified the shipped daemon executable as part of the installation
receipt. The qualification example was the entry point exercised here.

| Run | End to end | Daemon readiness | Finite Command |
| --- | ---: | ---: | ---: |
| Author | 36.74 s | 5.518 s | 5.381 s |
| Independent Claude run | 37.50 s | 5.622 s | 5.472 s |

The timings describe two private WSS fixture runs. Readiness includes the
real fork and startup handshake. The finite Command measurement covers
a staged request through result collection. The fixture ran `/bin/cat`
via the installed worker and preserved the exact result and exit
status. The input included NUL, quotes, spaces, a semicolon, a wildcard,
slashes, a non-UTF-8 byte and CRLF.

The fixture required four authenticated qualification connections before
issuing the Manual receipt. It verified ordinary Command traffic on a
different origin with zero Peer publications on either relay. Wrong
markers and the wrong route failed before readiness. The real Unix
R/A/C report and listener became active with the profile lease held.
The socket refused Terminal. SIGTERM exited cleanly, removed the
socket and released the lease. A fresh foreground run preserved an
unrelated inherited descriptor. A separate stock owner reopened the
native SQLCipher binding. The marked private fallback refused startup,
and the unmarked stock path accepted the downgraded profile.

Strict Clippy passed in 29.57 seconds for stock and 29.69 seconds
for the candidate. The exact candidate
artifact also passed 36 launch tests in 5.00 seconds, five Command
startup tests in 36.55 seconds and three healthy-stop tests in
0.30 seconds.

The manifest SHA-256 was
`de871184421a7fb69c46da6f431669b13b896ad79de1ece52e279a3b9afbf26e`.
The installed worker SHA-256 was
`57ef19570d09d8daec3c16204987ec1912680245aab31db51b4dc362515a1c66`.
The installed daemon SHA-256 was
`d2170d01c0bad6fed65b0523427000d8272610b4c1d0612807ad59ecba1cc002`.
The qualification example SHA-256 was
`0ef1c207b06dfdafee0d36e53c90d9f2e79d9babafca0b75f9d24d327d1d161b`
and the test runner SHA-256 was
`56e93dc499fbb34957f207bbca0d0dd9009d330f4ac8c659c171818d84ee5acc`.

This private run exercised a Client profile with no local Command
engine. It leaves the stock installer, launcher and runtime behaviors
as currently shipped. [Independent review](../reviews/peer-candidate-registered-daemon-healthy.md).
