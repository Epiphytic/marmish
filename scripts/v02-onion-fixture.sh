#!/usr/bin/env bash
# Native-only, disposable CI relay fixture and closed public artifact rendezvous.
set -Eeuo pipefail
umask 077
fail() { printf '%s\n' 'v02 fixture: operation failed' >&2; exit 1; }
number() { [[ $1 =~ ^[1-9][0-9]{0,11}$ ]]; }
binding() { number "$run" && number "$attempt" && [[ $sha =~ ^[0-9a-f]{40}$ ]] || fail; }
require_tools() { local tool; for tool in "$@"; do command -v "$tool" >/dev/null || fail; done; }
# Every external API call has a local watchdog, including on macOS without timeout(1).
bounded() {
  local seconds=$1 child until_at status=0; shift
  if [[ -n ${operation_end:-} ]]; then
    (( SECONDS < operation_end )) || return 124
    (( seconds <= operation_end-SECONDS )) || seconds=$((operation_end-SECONDS))
  fi
  "$@" & child=$!; until_at=$((SECONDS+seconds))
  while kill -0 "$child" 2>/dev/null; do
    if (( SECONDS >= until_at )); then
      kill -TERM "$child" 2>/dev/null || true; sleep 1
      # Bash can reap before wait; kill -0 narrows, but cannot eliminate, PID reuse races.
      if kill -0 "$child" 2>/dev/null; then kill -KILL "$child" 2>/dev/null || true; fi
      wait "$child" 2>/dev/null || true; return 124
    fi
    sleep 1
  done
  wait "$child" || status=$?
  return "$status"
}
# Download exactly one immutable artifact from this run, not a mutable branch name.
artifact() {
  local name=$1 member=$2 output=$3 meta=$scratch/meta.json zip=$scratch/artifact.zip id
  bounded 30 gh api "repos/$GH_REPO/actions/runs/$run/artifacts?per_page=100" > "$meta" 2>/dev/null || return 1
  id=$(jq -er --arg name "$name" --argjson run "$run" --arg sha "$sha" '
    [.artifacts[] | select(.name==$name and .expired==false and .size_in_bytes>0 and .size_in_bytes<=65536 and .workflow_run.id==$run and .workflow_run.head_sha==$sha)]
    | if length==1 then .[0].id else error("artifact unavailable") end' "$meta" 2>/dev/null) || return 1
  [[ $id =~ ^[1-9][0-9]*$ ]] || return 1
  bounded 30 gh api "repos/$GH_REPO/actions/artifacts/$id/zip" > "$zip" 2>/dev/null || return 1
  [[ $(wc -c < "$zip") -le 65536 ]] || return 1
  [[ $(unzip -Z -1 "$zip" 2>/dev/null) == "$member" ]] || return 1
  # Check uncompressed size before extraction; there is one exact non-path member.
  unzip -l "$zip" | awk 'END {exit !($1 <= 65536 && $2 == 1)}' || return 1
  unzip -p "$zip" "$member" | head -c 65537 > "$output" || return 1
  [[ $(wc -c < "$output") -le 65536 ]] || return 1
}
validate_binding() {
  jq -e --argjson run "$run" --argjson attempt "$attempt" --arg sha "$sha" '
    .schema==1 and .run_id==$run and .run_attempt==$attempt and .source_sha==$sha' "$1" >/dev/null
}
serve() {
  [[ $# == 8 ]] || fail
  local relay=$1 marmish=$2 root=$3 ready=$4; run=$5; attempt=$6; sha=$7
  local lifetime=$8 work='' native_tmp='' relay_pid='' check_pid='' native_clean=false cleaned=false
  unset GH_TOKEN GITHUB_TOKEN ACTIONS_RUNTIME_TOKEN
  binding; number "$lifetime" && (( lifetime <= 1500 )) || fail
  [[ $relay == /* && -x $relay && $marmish == /* && -x $marmish && $root == /* && -d $root && ! -L $root && -O $root && $ready == "$root/readiness.json" ]] || fail
  [[ ! -e $root/stop && ! -e $ready && ! -e $root/cleanup.json ]] || fail
  chmod 700 "$root"
  cleanup() {
    local status=$? pid until_at reaped=true
    trap - EXIT ERR INT TERM
    for pid in "$check_pid" "$relay_pid"; do
      [[ -n $pid ]] || continue
      if kill -0 "$pid" 2>/dev/null; then
        # deaddrop handles SIGINT; native driver has a finite internal deadline.
        if [[ $pid == "$relay_pid" ]]; then kill -INT "$pid" 2>/dev/null || true; else kill -TERM "$pid" 2>/dev/null || true; fi
        until_at=$((SECONDS+20))
        while kill -0 "$pid" 2>/dev/null && (( SECONDS < until_at )); do sleep 1; done
        # Pure Bash cannot atomically test child identity and signal it.
        if kill -0 "$pid" 2>/dev/null; then kill -KILL "$pid" 2>/dev/null || true; fi
      fi
      wait "$pid" 2>/dev/null || true
      if kill -0 "$pid" 2>/dev/null; then reaped=false; fi
    done
    if [[ $reaped == true ]]; then
      [[ -z $work ]] || rm -rf -- "$work"
      [[ -z $native_tmp ]] || rm -rf -- "$native_tmp"
      if [[ ( -z $work || ! -e $work ) && ( -z $native_tmp || ! -e $native_tmp ) ]]; then cleaned=true; fi
    fi
    jq -n --argjson run "$run" --argjson attempt "$attempt" --arg sha "$sha" --argjson reaped "$reaped" --argjson cleaned "$cleaned" --argjson native "$native_clean" \
      '{schema:1,run_id:$run,run_attempt:$attempt,source_sha:$sha,relay_children_reaped:$reaped,private_state_removed:$cleaned,readiness_native_cleanup_ok:$native,supervisor_reaped_by_this_step:false}' > "$root/cleanup.json.tmp"
    mv "$root/cleanup.json.tmp" "$root/cleanup.json"
    [[ $reaped == true && $cleaned == true ]] || status=1
    exit "$status"
  }
  trap cleanup EXIT ERR; trap 'exit 130' INT; trap 'exit 143' TERM
  # Install cleanup before either allocation; never nest runtime socket paths in CI's long roots.
  # Guard substitutions so inherited ERR cannot run cleanup in their subshells.
  work=$(mktemp -d "$root/fixture.XXXXXXXX") || fail
  native_tmp=$(mktemp -d /tmp/mv2.XXXXXXXX) || fail
  chmod 700 "$native_tmp"
  mkdir -m 700 "$work/relay"
  local end=$((SECONDS+lifetime)) startup=$((SECONDS+180)) url
  "$relay" relay --data-dir "$work/relay" > "$work/startup.json" 2> "$work/relay-private.log" & relay_pid=$!
  while [[ ! -s $work/startup.json ]]; do
    kill -0 "$relay_pid" 2>/dev/null || fail
    (( SECONDS < startup && SECONDS < end )) || fail
    [[ ! -e $root/stop ]] || fail; sleep 1
  done
  [[ $(wc -c < "$work/startup.json") -le 4096 ]] || fail
  url=$(jq -er 'select(keys==["onion_url","relay_url"]) | .relay_url | select(type=="string" and test("^ws://[a-z2-7]{56}\\.onion/relay$"))' "$work/startup.json") || fail
  # Actual encrypted two-identity exchange/restart/receipt, not descriptor existence.
  TMPDIR="$native_tmp" "$marmish" --relay "$url" --tor always foundation-check > "$work/native.json" 2> "$work/native-private.log" & check_pid=$!
  local native_end=$((SECONDS+660)) check_status=0
  while kill -0 "$check_pid" 2>/dev/null; do
    (( SECONDS < native_end && SECONDS < end )) || fail
    kill -0 "$relay_pid" 2>/dev/null || fail
    [[ ! -e $root/stop ]] || fail; sleep 1
  done
  wait "$check_pid" || check_status=$?; check_pid=''
  (( check_status == 0 )) || fail
  [[ $(wc -c < "$work/native.json") -le 65536 ]] || fail
  jq -e '.schema==1 and .protocol=="marmish-foundation/2" and .phase=="v0.2-phase1" and .ok==true and .cleanup_ok==true and .runtime_cleanup_ok==true and .state_cleanup_ok==true and .route=="always"' "$work/native.json" >/dev/null || fail
  native_clean=true
  jq -n --argjson run "$run" --argjson attempt "$attempt" --arg sha "$sha" --arg url "$url" \
    '{schema:1,run_id:$run,run_attempt:$attempt,source_sha:$sha,ready:true,readiness:"native_encrypted_foundation",relay_url:$url}' > "$ready.tmp"
  mv "$ready.tmp" "$ready"
  while (( SECONDS < end )); do
    [[ ! -e $root/stop ]] || exit 0
    kill -0 "$relay_pid" 2>/dev/null || fail; sleep 1
  done
  fail
}
await_readiness() {
  [[ $# == 5 ]] || fail
  local output=$1; run=$2; attempt=$3; sha=$4; local seconds=$5 until_at
  binding; number "$seconds" && (( seconds<=1200 )) || fail
  scratch=$(mktemp -d); trap 'rm -rf -- "$scratch"' EXIT
  until_at=$((SECONDS+seconds)); operation_end=$until_at
  while (( SECONDS < until_at )); do
    if artifact "v02-onion-ready-$run-$attempt" readiness.json "$scratch/body.json" && validate_binding "$scratch/body.json" && \
      jq -e 'keys==["readiness","ready","relay_url","run_attempt","run_id","schema","source_sha"] and .ready==true and .readiness=="native_encrypted_foundation" and (.relay_url|type=="string" and test("^ws://[a-z2-7]{56}\\.onion/relay$"))' "$scratch/body.json" >/dev/null; then
      cp "$scratch/body.json" "$output"; return 0
    fi
    local pause=$((until_at-SECONDS))
    (( pause > 0 )) || break
    (( pause <= 10 )) || pause=10
    sleep "$pause"
  done
  fail
}
wait_clients() {
  [[ $# == 5 ]] || fail
  local output=$1; run=$2; attempt=$3; sha=$4; local seconds=$5 until_at platform
  binding; number "$seconds" && (( seconds<=1500 )) || fail
  scratch=$(mktemp -d); trap 'rm -rf -- "$scratch"' EXIT
  until_at=$((SECONDS+seconds)); operation_end=$until_at
  while (( SECONDS < until_at )); do
    for platform in linux-amd64 linux-arm64 macos; do
      [[ ! -f $scratch/$platform.json ]] || continue
      if artifact "v02-terminal-$platform-$run-$attempt" terminal.json "$scratch/candidate.json" && validate_binding "$scratch/candidate.json" && \
        jq -e --arg p "$platform" 'keys==["ok","platform","run_attempt","run_id","schema","source_sha"] and .platform==$p and (.ok|type=="boolean")' "$scratch/candidate.json" >/dev/null; then
        cp "$scratch/candidate.json" "$scratch/$platform.json"
      fi
    done
    if [[ -f $scratch/linux-amd64.json && -f $scratch/linux-arm64.json && -f $scratch/macos.json ]]; then
      jq -s '{schema:1,clients:.,ok:all(.[];.ok==true)}' "$scratch/linux-amd64.json" "$scratch/linux-arm64.json" "$scratch/macos.json" > "$output"
      jq -e '.ok==true' "$output" >/dev/null; return $?
    fi
    local pause=$((until_at-SECONDS))
    (( pause > 0 )) || break
    (( pause <= 10 )) || pause=10
    sleep "$pause"
  done
  jq -n '{schema:1,ok:false,category:"terminal_artifact_timeout"}' > "$output"; fail
}
require_tools jq
mode=${1:-}; [[ $# -ge 1 ]] || fail; shift
case "$mode" in
  serve) serve "$@";;
  await-readiness) require_tools gh unzip; : "${GH_REPO:?}"; await_readiness "$@";;
  wait-clients) require_tools gh unzip; : "${GH_REPO:?}"; wait_clients "$@";;
  *) fail;;
esac
