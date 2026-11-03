# Installer source fallback check

Checked 2026-11-02 against source base `f6f251d4b39e7da2b1473631a23e191baaf49f19`.
This checks the README's legacy v0.1 download command, not a native v0.2 installer
or completed installation.

- GitHub `https://raw.githubusercontent.com/Epiphytic/marmish/master/scripts/install.sh`:
  HTTP 200; downloaded bytes match the reviewed local `scripts/install.sh`.
  SHA256: `eac417b4748f0252adef82fe18c1260ef744523ec0ed6b624927dc6b803d0768`.
- Iris `https://iris.radicle.network/raw/rad:z3x7GVHywRtV2nR9qA5V4VqAVvCDg/head/scripts/install.sh`:
  HTTP 200; downloaded bytes match GitHub and the reviewed local script, with
  the same SHA256 above; downloaded script passes `sh -n`.
- Iris immutable README at that source commit separately returned HTTP 500.
  A successful script download does not establish every source path works;
  a successful Radicle push alone is not source availability.
- Downloaded GitHub script passes `sh -n`. Offline shell checks of the exact
  README fallback block passed: primary success runs the script; primary failure
  and backup success runs it; both failures do not run it.

Both requests use HTTPS-only curl, a 10-second connection timeout and a 60-second
overall timeout. HTTP errors fail the download. An HTTP 200 containing stale or
incorrect script bytes does not trigger fallback. Pin both source URLs to the
same reviewed commit to avoid mixing immutable and mutable sources.

Both script sources still download archives/checksums from GitHub Releases.
This is script-source redundancy, not independent release hosting. No native
v0.2 quickstart is accepted yet. These observations qualify both current legacy
script URLs only; native installer publication requires fresh checks.
