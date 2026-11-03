#!/usr/bin/env bash
# Offline tests only: inert child executables, no relay/provider/network access.
set -Eeuo pipefail
umask 077
here=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
filter=$here/v02-owner-diagnostics.jq
root=$(mktemp -d /tmp/mv2-diag-test.XXXXXXXX)
trap 'rm -rf -- "$root"' EXIT
sha=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
filter_mode() { local mode=$1 flags=$2; shift 2; jq "$flags" --arg mode "$mode" --argjson run 1 --argjson attempt 1 --arg sha "$sha" -f "$filter" "$@"; }
extract() { filter_mode extract -Rn; }
validate() { filter_mode validate -e "$1" >/dev/null; }
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
# Native report extraction uses the actual compact serde writer's framing.
native_extract() { filter_mode native-extract -Rs; }
native_valid() { filter_mode native-outcome -se >/dev/null; }
jq -cnS '{schema:1,protocol:"marmish-foundation/2",phase:"v0.2-phase1",stage:"alice_to_bob",route:"always",ok:false,error:"deadline",category:"deadline",target:"x86_64",platform:"linux",relay_count:1,elapsed_ms:570001,samples:[],cleanup_ok:true,runtime_cleanup_ok:true,state_cleanup_ok:true,diagnostic:{operation:"sender_tick",last_recoverable:{role:"sender",label:"timeout"},terminal_label:null}}' > "$root/native-report"
native_extract < "$root/native-report" > "$root/native-projected"
native_valid < "$root/native-projected"
jq -e '.stage=="alice_to_bob" and .category=="deadline" and .elapsed_ms==570001' "$root/native-projected" >/dev/null
# Byte-exact actual serde CLI output from hosted41569702092 LinuxAMD64 direct.
# Its route is intentionally rejected. Change only that literal to exercise the
# same writer bytes through the always-only fixture projection; no Tor pass claim.
cat > "$root/real-serde-off" <<'SERDE_REPORT'
{"category":null,"cleanup_ok":true,"diagnostic":{"last_recoverable":null,"operation":"none","terminal_label":null},"elapsed_ms":14672,"error":null,"ok":true,"phase":"v0.2-phase1","platform":"linux","protocol":"marmish-foundation/2","relay_count":1,"route":"off","runtime_cleanup_ok":true,"samples":[{"bytes":0,"bytes_per_second":null,"milliseconds":161,"sha256":"","stage":"activation_cold"},{"bytes":66,"bytes_per_second":190,"milliseconds":347,"sha256":"75ca6ae85a4c66ef6c19afcad7a472b5512dea41ba1a6149a39367e5ebbea6c4","stage":"alice_to_bob"},{"bytes":66,"bytes_per_second":186,"milliseconds":353,"sha256":"75ca6ae85a4c66ef6c19afcad7a472b5512dea41ba1a6149a39367e5ebbea6c4","stage":"bob_to_alice"},{"bytes":0,"bytes_per_second":null,"milliseconds":408,"sha256":"","stage":"restart_activation_warm"},{"bytes":66,"bytes_per_second":82,"milliseconds":795,"sha256":"75ca6ae85a4c66ef6c19afcad7a472b5512dea41ba1a6149a39367e5ebbea6c4","stage":"restart_alice_to_bob"},{"bytes":66,"bytes_per_second":187,"milliseconds":351,"sha256":"75ca6ae85a4c66ef6c19afcad7a472b5512dea41ba1a6149a39367e5ebbea6c4","stage":"restart_bob_to_alice"}],"schema":1,"stage":"completed","state_cleanup_ok":true,"target":"x86_64"}
SERDE_REPORT
native_extract < "$root/real-serde-off" | jq -e '.==null' >/dev/null
sed 's/"route":"off"/"route":"always"/' "$root/real-serde-off" | native_extract |
  jq -e '.ok and .elapsed_ms==14672 and (.samples|length)==6' >/dev/null
# Pin all finite operation/stage/label paths, including recoverable-vs-terminal distinction.
for stage in identity_prepare activation_cold conversation_open conversation_join alice_to_bob bob_to_alice restart_shutdown restart_open restart_activation_warm restart_alice_to_bob restart_bob_to_alice completed; do
  jq -cS --arg s "$stage" '.stage=$s' "$root/native-report" | native_extract | jq -e 'type=="object"' >/dev/null
done
for operation in none activation open_conversation join receiver_tick sender_tick received sent_status initial_send duplicate_send conflict_send; do
  jq -cS --arg o "$operation" '.diagnostic.operation=$o' "$root/native-report" | native_extract | jq -e 'type=="object"' >/dev/null
done
for label in timeout transport_recovery_pending subscription_sync journal_backpressure relay_reconnect recovery_subscription notification_loss group_state_unavailable add_relay relay_connect activate arti_initialization arti_bootstrap other; do
  for role in sender receiver; do
    jq -cS --arg l "$label" --arg r "$role" '.diagnostic.last_recoverable={role:$r,label:$l}' "$root/native-report" | native_extract | jq -e 'type=="object"' >/dev/null
  done
done
for label in tls_verification tls_hostname proxy_forbidden arti_configuration relay_not_configured shutdown delivery_closed notification_forwarder_stopped; do
  jq -cS --arg l "$label" '.diagnostic.terminal_label=$l | .diagnostic.last_recoverable=null' "$root/native-report" | native_extract | jq -e 'type=="object"' >/dev/null
  jq -cS --arg l "$label" '.diagnostic.last_recoverable.label=$l' "$root/native-report" | native_extract | jq -e '.==null' >/dev/null
done
for category in identity storage transport marmot local_setup deadline; do
  jq -cS --arg c "$category" '.category=$c|.error=$c' "$root/native-report" | native_extract | jq -e 'type=="object"' >/dev/null
done
jq -cS '.category="cleanup"|.error="cleanup"|.stage="cleanup"|.cleanup_ok=false|.runtime_cleanup_ok=false' "$root/native-report" | native_extract | jq -e 'type=="object"' >/dev/null
# Valid six-sample success report and a late cleanup measurement <=660 seconds.
jq -cS '.ok=true|.error=null|.category=null|.stage="completed"|.elapsed_ms=610000|.diagnostic={operation:"none",last_recoverable:null,terminal_label:null}|.samples=(["activation_cold","alice_to_bob","bob_to_alice","restart_activation_warm","restart_alice_to_bob","restart_bob_to_alice"]|map(. as $s|{stage:$s,milliseconds:1,bytes:(if ($s=="activation_cold" or $s=="restart_activation_warm") then 0 else 64 end),sha256:(if ($s=="activation_cold" or $s=="restart_activation_warm") then "" else ("a"*64) end),bytes_per_second:(if ($s=="activation_cold" or $s=="restart_activation_warm") then null else 1000 end)}))' "$root/native-report" > "$root/native-success"
native_extract < "$root/native-success" | jq -e '.ok and (.samples|length)==6 and .elapsed_ms==610000' >/dev/null
for expression in '.private="nsec1secret"' 'del(.category)' '.category="nsec1secret"' '.diagnostic.operation="ws://private.onion"' '.diagnostic.extra=true' 'del(.diagnostic.terminal_label)' '.diagnostic.last_recoverable.role="private"' '.diagnostic.last_recoverable.label="private"' '.diagnostic.terminal_label="private"' '.elapsed_ms=660001' '.elapsed_ms=-1' '.elapsed_ms=1.5' '.elapsed_ms=true' '.cleanup_ok=false' '.category="cleanup"|.error="cleanup"' '.stage="cleanup"' '.schema=true' '.relay_count=0' '.route="off"' '.error="storage"' '.samples=[null]' '.samples=[{stage:"private"}]' '.samples=[.samples[0]]'; do
  jq -cS "$expression" "$root/native-report" | native_extract | jq -e '.==null' >/dev/null
done
for expression in '.samples[0].sha256="private"' '.samples[1].sha256="private"' '.samples[1].bytes_per_second=9007199254740992' '.samples[1].bytes_per_second=-1' '.samples[1].bytes_per_second=null' '.samples[0].milliseconds=660001' '.samples[1].bytes=16385' '.samples[1].bytes=0' '.samples[0].bytes=1' '.samples[0].extra=true' 'del(.samples[0].sha256)' '.samples|=reverse' '.samples += [.samples[-1]]' '.samples[1]=.samples[0]'; do
  jq -cS "$expression" "$root/native-success" | native_extract | jq -e '.==null' >/dev/null
done
for value in '' '{}' 'null' '{"schema":' '[]' '1' 'NaN'; do
  printf '%s\n' "$value" | native_extract | jq -e '.==null' >/dev/null
done
# Canonical bytes reject duplicate keys (also nested), extra values and whitespace.
for expression in 'sub("\\{\"category\":";"{\"category\":null,\"category\":")' 'sub("\"operation\":";"\"operation\":\"none\",\"operation\":")'; do
  jq -Rr "$expression" "$root/native-report" | native_extract | jq -e '.==null' >/dev/null
done
{ cat "$root/native-report"; cat "$root/native-report"; } | native_extract | jq -e '.==null' >/dev/null
{ printf ' '; cat "$root/native-report"; } | native_extract | jq -e '.==null' >/dev/null
for size in 65536 65537; do printf '%*s' "$size" '' | native_extract | jq -e '.==null' >/dev/null; done
if printf '%s\n%s\n' null null | native_valid; then exit 1; fi
# A null category cannot conceal failed cleanup in either parser boundary.
jq -cS '.ok=false|.category=null|.error=null|.stage="completed"|.cleanup_ok=false|.runtime_cleanup_ok=false' "$root/native-report" | native_extract | jq -e '.==null' >/dev/null
if jq '.ok=false|.category=null|.stage="completed"|.cleanup_ok=false|.runtime_cleanup_ok=false' "$root/native-projected" | native_valid; then exit 1; fi

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
if [[ $CASE == outcome_failed ]]; then cat "$NATIVE_REPORT"; exit 7; fi
if [[ $CASE == outcome_success ]]; then cat "$NATIVE_SUCCESS"; exit 0; fi
if [[ $CASE == outcome_symlink || $CASE == outcome_directory ]]; then
  for report in "$FIXTURE_CASE_ROOT"/fixture.*/native.json; do
    rm -- "$report"
    if [[ $CASE == outcome_symlink ]]; then ln -s "$NATIVE_REPORT" "$report"; else mkdir "$report"; fi
  done
  exit 7
fi
if [[ $CASE == outcome_oversize ]]; then printf '%65537s' ''; exit 7; fi
if [[ $CASE == outcome_partial ]]; then printf '{"schema":'; exit 7; fi
if [[ $CASE == native_validation ]]; then echo '{}'; exit 0; fi
if [[ $CASE == native_ready ]]; then
  echo '{"schema":1,"protocol":"marmish-foundation/2","phase":"v0.2-phase1","ok":true,"cleanup_ok":true,"runtime_cleanup_ok":true,"state_cleanup_ok":true,"route":"always"}'
  exit 0
fi
exit 7
NATIVE
chmod 700 "$root/bin/relay" "$root/bin/native"
export NATIVE_REPORT="$root/native-report" NATIVE_SUCCESS="$root/native-success"
for case_stage in outcome_failed:native_check outcome_success:ready outcome_partial:native_check outcome_symlink:native_check outcome_directory:native_check outcome_oversize:native_check relay_failure:relay_start endpoint_failure:relay_endpoint native_failure:native_check native_validation:native_validate native_ready:ready; do
  case_name=${case_stage%:*}; expected_stage=${case_stage#*:}
  mkdir "$root/$case_name"
  if FIXTURE_CASE_ROOT="$root/$case_name" CASE=$case_name bash "$here/v02-onion-fixture.sh" serve "$root/bin/relay" "$root/bin/native" "$root/$case_name" "$root/$case_name/readiness.json" 1 1 "$sha" 20 > /dev/null 2>&1; then echo 'expected failure' >&2; exit 1; fi
  report=$root/$case_name/cleanup.json
  validate "$report"
  jq -e --arg s "$expected_stage" '.fixture_stage==$s and .relay_children_reaped and .private_state_removed and (.readiness_native_cleanup_ok==($s=="ready"))' "$report" >/dev/null
  files=1; [[ $expected_stage != ready ]] || files=2
  [[ $(find "$root/$case_name" -mindepth 1 -maxdepth 1 | wc -l) == "$files" ]]
  if grep -Eq 'nsec|private.onion' "$report"; then exit 1; fi
  if [[ $case_name == outcome_failed ]]; then jq -e '.native_outcome.category=="deadline" and .diagnostics!=[] and (.readiness_native_cleanup_ok|not)' "$report" >/dev/null
  elif [[ $case_name == outcome_success ]]; then jq -e '.native_outcome.ok and .readiness_native_cleanup_ok' "$report" >/dev/null
  else jq -e '.native_outcome==null' "$report" >/dev/null; fi
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
# Native extraction failure/multiple values cannot break cleanup or erase diagnostics.
for injection in failure multiple; do
  if [[ $injection == failure ]]; then body='exit 3'; else body="printf 'null\\nnull\\n'; exit 0"; fi
  printf '#!/usr/bin/env bash\nif [[ $1 == -Rs ]]; then %s; fi\nexec "%s" "$@"\n' "$body" "$real_jq" > "$root/bin/jq"
  mkdir "$root/native_extractor_$injection"
  if PATH="$root/bin:$PATH" CASE=outcome_failed bash "$here/v02-onion-fixture.sh" serve "$root/bin/relay" "$root/bin/native" "$root/native_extractor_$injection" "$root/native_extractor_$injection/readiness.json" 1 1 "$sha" 20 >"$root/public-stderr" 2>&1; then exit 1; fi
  validate "$root/native_extractor_$injection/cleanup.json"
  jq -e '.native_outcome==null and .diagnostics!=[] and .private_state_removed and .relay_children_reaped' "$root/native_extractor_$injection/cleanup.json" >/dev/null
  if grep -Eq 'nsec|private.onion' "$root/public-stderr"; then exit 1; fi
done
rm "$root/bin/jq"
printf '#!/usr/bin/env bash\nprintf '\''nsec1readerprivate ws://private.onion/owned-path\\n'\'' >&2\nexit 3\n' > "$root/bin/head"
chmod 700 "$root/bin/head"
mkdir "$root/native_reader_failure"
if PATH="$root/bin:$PATH" CASE=outcome_failed bash "$here/v02-onion-fixture.sh" serve "$root/bin/relay" "$root/bin/native" "$root/native_reader_failure" "$root/native_reader_failure/readiness.json" 1 1 "$sha" 20 >"$root/reader-stderr" 2>&1; then exit 1; fi
validate "$root/native_reader_failure/cleanup.json"
jq -e '.native_outcome==null and .diagnostics!=[] and .private_state_removed and .relay_children_reaped' "$root/native_reader_failure/cleanup.json" >/dev/null
if grep -Eq 'nsec|private.onion|owned-path' "$root/reader-stderr"; then exit 1; fi
rm "$root/bin/head"
# Strict publication validator rejects extra fields, wrong binding and cross-pairs.
for expression in '.extra=true' '.run_id=2' '.run_attempt=2' '.source_sha="private"' '.fixture_stage="private"' '.diagnostics=[{stage:"relay_slot",category:"begin"}]' 'del(.native_outcome)' '.native_outcome={private:"nsec1secret"}'; do
  jq "$expression" "$root/extractor_failure/cleanup.json" > "$root/invalid"
  if validate "$root/invalid"; then exit 1; fi
done
printf '%s\n' 'owner diagnostics: PASS'
