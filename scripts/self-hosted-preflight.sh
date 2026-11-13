#!/bin/sh
# Inventory only. Never print raw subprocess output or private runner identifiers.
set -eu
export LC_ALL=C
image=ubuntu@sha256:2198e4c65076e9cb0b3d695219df667c0e3b0072da64ca3c2648aca17dcecaf5
fail() { printf 'preflight_error=%s\n' "$1" >&2; exit 1; }
case ${PREFLIGHT_PLATFORM:-} in
    linux-amd64) expected_os=Linux; expected_arch=x86_64 ;;
    linux-arm64) expected_os=Linux; expected_arch=aarch64 ;;
    macos-arm64) expected_os=Darwin; expected_arch=arm64 ;;
    *) fail platform ;;
esac
case ${GITHUB_RUN_ID:-} in ''|*[!0-9]*) fail run_id ;; esac
case ${GITHUB_RUN_ATTEMPT:-} in ''|*[!0-9]*) fail run_attempt ;; esac
name=marmish-preflight-$GITHUB_RUN_ID-$GITHUB_RUN_ATTEMPT-$PREFLIGHT_PLATFORM
case ${1:-probe} in
    cleanup)
        # The probe exports readiness before its first operation that can create
        # a container. An unavailable engine after that is cleanup uncertainty.
        if [ "${PREFLIGHT_DOCKER_READY:-}" != true ]; then
            printf 'container_cleanup=not_attempted\n'
            exit 0
        fi
        docker rm -f "$name" >/dev/null 2>&1 || :
        remaining=$(docker container ls -a --filter "name=^/$name$" --format '{{.Names}}' 2>/dev/null) || fail cleanup_uncertain
        [ -z "$remaining" ] || fail cleanup_uncertain
        printf 'container_cleanup=absent\n'
        exit 0
        ;;
    probe) ;;
    *) fail arguments ;;
esac
[ "${PREFLIGHT_RUNNER_ENVIRONMENT:-}" = self-hosted ] || fail runner_environment
[ -n "${EPIPHYTIC_EXECUTION_ID:-}" ] || fail execution_identity
actual_uid=$(id -u 2>/dev/null) || fail uid
case $actual_uid in ''|*[!0-9]*|0) fail nonroot ;; esac
[ "$(uname -s 2>/dev/null)" = "$expected_os" ] || fail operating_system
[ "$(uname -m 2>/dev/null)" = "$expected_arch" ] || fail architecture
printf 'identity=self-hosted_nonroot_native\nplatform=%s\n' "$PREFLIGHT_PLATFORM"
version() {
    key=$1; shift
    raw=$("$@" 2>/dev/null) || { printf '%s=missing\n' "$key"; return; }
    number=$(printf '%s\n' "$raw" | sed -n 's/^[^0-9]*\([0-9][0-9]*\.[0-9][0-9]*\(\.[0-9][0-9]*\)*\).*$/\1/p' | head -n 1)
    [ "${#number}" -le 64 ] || number=
    case $number in ''|*[!0-9.]*) printf '%s=missing\n' "$key" ;; *) printf '%s=%s\n' "$key" "$number" ;; esac
}
present() {
    if command -v "$2" >/dev/null 2>&1; then printf '%s=available\n' "$1"; else printf '%s=missing\n' "$1"; fi
}
version python python3 --version
if python3 -c 'import sys; raise SystemExit(sys.version_info < (3,11))' >/dev/null 2>&1; then
    printf 'python_3_11=available\n'
else printf 'python_3_11=missing\n'; fi
version node node --version
if node -e 'process.exit(Number(process.versions.node.split(".")[0]) < 20 ? 1 : 0)' >/dev/null 2>&1; then
    printf 'node_20=available\n'
else printf 'node_20=missing\n'; fi
version git git --version
version curl curl --version
version cc cc --version
version make make --version
version perl perl -e 'printf "%vd\n", $^V'
present tar tar
present gzip gzip
present shasum shasum
if [ "$expected_os" = Darwin ]; then
    if xcode-select -p >/dev/null 2>&1; then printf 'xcode_selection=available\n'; else printf 'xcode_selection=missing\n'; fi
    version macos sw_vers -productVersion
    version sdk xcrun --show-sdk-version
    present codesign codesign
    present security security
    present otool otool
    exit 0
fi
version glibc getconf GNU_LIBC_VERSION
present readelf readelf
present dbus_daemon dbus-daemon
present dbus_send dbus-send
present gnome_keyring gnome-keyring-daemon
present setpriv setpriv
if ! command -v docker >/dev/null 2>&1; then
    printf 'docker=missing\ncontainer_smoke=missing\n'
    exit 0
fi
version docker docker --version
security_options=$(docker info --format '{{json .SecurityOptions}}' 2>/dev/null) || {
    printf 'docker_rootless=missing\ncontainer_smoke=missing\n'; exit 0;
}
case $security_options in *'"name=rootless"'*) ;; *) fail docker_not_rootless ;; esac
printf 'docker_rootless=available\n'
# Refuse an occupied name before declaring ownership to the cleanup step.
existing=$(docker container ls -a --filter "name=^/$name$" --format '{{.Names}}' 2>/dev/null) || fail container_inventory
[ -z "$existing" ] || fail container_name_occupied
# Mark ownership before pull/run so failures and cancellation still clean up.
[ -n "${GITHUB_OUTPUT:-}" ] || fail output_channel
printf 'docker_ready=true\n' >> "$GITHUB_OUTPUT"
if ! docker pull "$image" >/dev/null 2>&1; then
    printf 'container_smoke=missing_image\n'; exit 0
fi
printf 'container_image=ubuntu24_pinned\ncontainer_image_retained=true\n'
if docker run --name "$name" --rm --network none --read-only --cap-drop ALL \
    --security-opt no-new-privileges --user 65534:65534 \
    --env EPIPHYTIC_EXECUTION_ID --env "PREFLIGHT_ARCH=$expected_arch" "$image" timeout 20 /bin/sh -c \
    'test "$(id -u)" = 65534 && test -n "$EPIPHYTIC_EXECUTION_ID" && test "$(uname -m)" = "$PREFLIGHT_ARCH" && test "$(getconf GNU_LIBC_VERSION)" = "glibc 2.39"' >/dev/null 2>&1; then
    printf 'container_smoke=available\ncontainer_glibc=2.39\n'
else
    printf 'container_smoke=missing\n'
fi
