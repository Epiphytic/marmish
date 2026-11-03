#!/usr/bin/env bash
# Offline tests only: inert child executables, no relay/provider/network access.
set -Eeuo pipefail
umask 077
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
filter=$here/v02-owner-diagnostics.jq
root=$(mktemp -d /tmp/mv2-diag-test.XXXXXXXX)
trap 'rm -rf -- "$root"' EXIT
sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
extract() { jq -Rn --arg mode extract --argjson run 1 --argjson attempt 1 --arg sha "$sha" -f "$filter"; }
validate() { jq -e --arg mode validate --argjson run 1 --argjson attempt 1 --arg sha "$sha" -f "$filter" "$1" >/dev/null; }
# Independent explicit vector table pins every approved pair.
cat > "$root/pairs" <<'PAIRS'
arti_bootstrap begin ready
relay_slot waiting
arti_connect begin
direct_connect begin
tls begin
relay_stream connected
websocket_handshake begin ready
relay_startup_attempt one two three
relay_startup_result ok error
relay_startup_error protocol io transport database gossip policy timeout not_found invalid state rejected unsupported limit_exceeded other
relay_startup_status initialized pending connecting connected disconnected terminated banned sleeping shutdown
connector_failure timeout io proxy_forbidden relay_not_configured not_ready shutdown arti_not_ready arti_connect direct_connect websocket_handshake tls_configuration tls_hostname tls_verification other
identity_prepare begin
activation_cold begin
conversation_open begin
conversation_join begin
alice_to_bob begin
bob_to_alice begin
restart_shutdown begin
restart_open begin
restart_activation_warm begin
restart_alice_to_bob begin
restart_bob_to_alice begin
completed begin
PAIRS
while read -r stage categories; do
  for category in $categories; do
    event=$(jq -cn --arg s "$stage" --arg c "$category" '{schema:1,event:"diagnostic",stage:$s,category:$c}')
    [[ $(printf '%s\n' "$event" | extract | jq length) == 1 ]]
  done
done < "$root/pairs"
# Every stage/category combination is accepted iff explicitly listed for that row.
awk '{for(i=2;i<=NF;i++)print $i}' "$root/pairs" | sort -u > "$root/categories"
while read -r stage categories; do
  jq -Rcn --arg s "$stage" 'inputs | {schema:1,event:"diagnostic",stage:$s,category:.}' < "$root/categories" |
    extract > "$root/cross-pairs"
  jq -e --arg allowed "$categories" '([.[].category]|sort)==($allowed|split(" ")|sort)' "$root/cross-pairs" >/dev/null
done < "$root/pairs"
valid='{"schema":1,"event":"diagnostic","stage":"arti_bootstrap","category":"begin"}'
{
  printf '%s\n' "$valid" 'not JSON private nsec1secret ws://private.onion' '[1]' 'null' \
    '{"schema":1,"event":"context","stage":"arti_bootstrap","category":"begin"}' \
    '{"schema":1,"event":"diagnostic","stage":"arti_bootstrap","category":"begin","secret":"nsec1secret"}' \
    '{"schema":1,"event":"diagnostic","stage":[],"category":"begin"}' \
    '{"schema":1,"event":"diagnostic","stage":"ws://private.onion","category":"nsec1secret"}'
  printf '%4097s%s\n' '' "$valid"
  printf '{"schema":'
} | extract > "$root/filtered"
[[ $(jq length "$root/filtered") == 1 ]]
# Invalid records do not split consecutive duplicates; status flood preserves begin.
{ printf '%s\n' "$valid"; for ((i=0;i<200;i++)); do printf '%s\n' '{"schema":1,"event":"diagnostic","stage":"relay_startup_status","category":"pending"}' 'invalid nsec1private ws://private.onion'; done; } | extract > "$root/flood"
[[ $(jq length "$root/flood") == 2 ]]
{ for ((i=0;i<40;i++)); do printf '%s\n' "$valid" '{"schema":1,"event":"diagnostic","stage":"arti_bootstrap","category":"ready"}'; done; } | extract > "$root/capped"
[[ $(jq length "$root/capped") == 32 ]]
# Real serve cleanup integration. Each child is owned and inert.
mkdir "$root/bin"
cat > "$root/bin/relay" <<'RELAY'
#!/usr/bin/env bash
if [[ $CASE == relay_failure ]]; then exit 1; fi
if [[ $CASE == endpoint_failure ]]; then echo '{"private":"nsec1secret"}'; exit 1; fi
printf '{"onion_url":"private","relay_url":"ws://%056d.onion/relay"}\n' 0 | tr 0 a
trap 'exit 0' INT TERM
sleep 3
exit 0
RELAY
cat > "$root/bin/native" <<'NATIVE'
#!/usr/bin/env bash
[[ $1 == --debug ]] || exit 9
printf '%s\n' '{"schema":1,"event":"diagnostic","stage":"identity_prepare","category":"begin"}' >&2
printf '%s\n' 'nsec1secret ws://private.onion' '{"schema":1,"event":"diagnostic","stage":"private","category":"secret"}' >&2
if [[ $CASE == native_validation ]]; then echo '{}'; exit 0; fi
if [[ $CASE == native_ready ]]; then
  echo '{"schema":1,"protocol":"marmish-foundation/2","phase":"v0.2-phase1","ok":true,"cleanup_ok":true,"runtime_cleanup_ok":true,"state_cleanup_ok":true,"route":"always"}'
  exit 0
fi
exit 7
NATIVE
chmod 700 "$root/bin/relay" "$root/bin/native"
for case_stage in relay_failure:relay_start endpoint_failure:relay_endpoint native_failure:native_check native_validation:native_validate native_ready:ready; do
  case_name=${case_stage%:*}; expected_stage=${case_stage#*:}
  mkdir "$root/$case_name"
  if CASE=$case_name bash "$here/v02-onion-fixture.sh" serve "$root/bin/relay" "$root/bin/native" "$root/$case_name" "$root/$case_name/readiness.json" 1 1 "$sha" 20 > /dev/null 2>&1; then echo 'expected failure' >&2; exit 1; fi
  report=$root/$case_name/cleanup.json
  validate "$report"
  jq -e --arg s "$expected_stage" '.fixture_stage==$s and .relay_children_reaped and .private_state_removed and (.readiness_native_cleanup_ok==($s=="ready"))' "$report" >/dev/null
  files=1; [[ $expected_stage != ready ]] || files=2
  [[ $(find "$root/$case_name" -mindepth 1 -maxdepth 1 | wc -l) == "$files" ]]
  ! grep -Eq 'nsec|private.onion' "$report"
done
# First allocation failure cannot inspect an unowned absolute /native-private.log.
printf '#!/usr/bin/env bash\nexit 9\n' > "$root/bin/mktemp"
chmod 700 "$root/bin/mktemp"
mkdir "$root/allocation_failure"
if PATH="$root/bin:$PATH" CASE=native_failure bash "$here/v02-onion-fixture.sh" serve "$root/bin/relay" "$root/bin/native" "$root/allocation_failure" "$root/allocation_failure/readiness.json" 1 1 "$sha" 20 >/dev/null 2>&1; then exit 1; fi
validate "$root/allocation_failure/cleanup.json"
jq -e '.fixture_stage=="allocate" and .diagnostics==[] and .private_state_removed and .relay_children_reaped' "$root/allocation_failure/cleanup.json" >/dev/null
rm "$root/bin/mktemp"
# Force the extraction pipeline to fail without changing report construction jq.
real_jq=$(command -v jq)
printf '#!/usr/bin/env bash\nif [[ $1 == -Rn ]]; then exit 3; fi\nexec "%s" "$@"\n' "$real_jq" > "$root/bin/jq"
chmod 700 "$root/bin/jq"
mkdir "$root/extractor_failure"
if PATH="$root/bin:$PATH" CASE=native_failure bash "$here/v02-onion-fixture.sh" serve "$root/bin/relay" "$root/bin/native" "$root/extractor_failure" "$root/extractor_failure/readiness.json" 1 1 "$sha" 20 >/dev/null 2>&1; then exit 1; fi
validate "$root/extractor_failure/cleanup.json"
jq -e '.diagnostics==[] and .private_state_removed and .relay_children_reaped' "$root/extractor_failure/cleanup.json" >/dev/null
# A successful extractor emitting multiple JSON values must also fall back safely.
printf '#!/usr/bin/env bash\nif [[ $1 == -Rn ]]; then printf '\''[]\\n[]\\n'\''; exit 0; fi\nexec "%s" "$@"\n' "$real_jq" > "$root/bin/jq"
mkdir "$root/extractor_multiple"
if PATH="$root/bin:$PATH" CASE=native_failure bash "$here/v02-onion-fixture.sh" serve "$root/bin/relay" "$root/bin/native" "$root/extractor_multiple" "$root/extractor_multiple/readiness.json" 1 1 "$sha" 20 >/dev/null 2>&1; then exit 1; fi
validate "$root/extractor_multiple/cleanup.json"
jq -e '.diagnostics==[] and .private_state_removed and .relay_children_reaped' "$root/extractor_multiple/cleanup.json" >/dev/null
# Strict publication validator rejects extra fields, wrong binding and cross-pairs.
for expression in '.extra=true' '.run_id=2' '.run_attempt=2' '.source_sha="private"' '.fixture_stage="private"' '.diagnostics=[{stage:"relay_slot",category:"begin"}]'; do
  jq "$expression" "$root/extractor_failure/cleanup.json" > "$root/invalid"
  if validate "$root/invalid"; then exit 1; fi
done
printf '%s\n' 'owner diagnostics: PASS'
