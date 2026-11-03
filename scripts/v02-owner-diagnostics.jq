# Fixture-only finite public projection. No native/provider strings pass through.
def pair:
  if type != "object" then false
  elif keys != ["category","stage"] then false
  elif (.stage|type)!="string" or (.category|type)!="string" then false
  else .stage as $s | .category as $c |
    {arti_bootstrap:["begin","ready"], relay_slot:["waiting"],
     arti_connect:["begin"], direct_connect:["begin"], tls:["begin"],
     relay_stream:["connected"], websocket_handshake:["begin","ready"],
     relay_startup_attempt:["one","two","three"],
     relay_startup_result:["ok","error"],
     relay_startup_error:["protocol","io","transport","database","gossip","policy","timeout","not_found","invalid","state","rejected","unsupported","limit_exceeded","other"],
     relay_startup_status:["initialized","pending","connecting","connected","disconnected","terminated","banned","sleeping","shutdown"],
     connector_failure:["timeout","io","proxy_forbidden","relay_not_configured","not_ready","shutdown","arti_not_ready","arti_connect","direct_connect","websocket_handshake","tls_configuration","tls_hostname","tls_verification","other"],
     identity_prepare:["begin"], activation_cold:["begin"],
     conversation_open:["begin"], conversation_join:["begin"],
     alice_to_bob:["begin"], bob_to_alice:["begin"],
     restart_shutdown:["begin"], restart_open:["begin"],
     restart_activation_warm:["begin"], restart_alice_to_bob:["begin"],
     restart_bob_to_alice:["begin"], completed:["begin"]} as $pairs |
    ($s|type)=="string" and ($c|type)=="string" and
    (($pairs[$s] // []) | index($c) != null)
  end;
def events:
  type=="array" and length<=32 and all(.[]; pair);
def stage:
  . == "allocate" or . == "relay_start" or . == "relay_endpoint" or
  . == "native_check" or . == "native_validate" or . == "ready";
if $mode == "extract" then
  reduce (inputs | select(utf8bytelength<=4096) | fromjson? |
    select(type=="object") |
    select(keys==["category","event","schema","stage"] and .schema==1 and .event=="diagnostic") |
    {stage,category} | select(pair)) as $event
    ([]; if .[-1] == $event then . else (. + [$event])[-32:] end)
elif $mode == "events" then length==1 and (.[0]|events)
elif $mode == "validate" then
  if type != "object" then false else
    keys==["diagnostics","fixture_stage","private_state_removed","readiness_native_cleanup_ok","relay_children_reaped","run_attempt","run_id","schema","source_sha","supervisor_reaped_by_this_step"] and
    .schema==1 and .run_id==$run and .run_attempt==$attempt and .source_sha==$sha and
    (.run_id|type)=="number" and (.run_attempt|type)=="number" and
    (.run_id>=1 and .run_id<=999999999999 and .run_id==(.run_id|floor)) and
    (.run_attempt>=1 and .run_attempt<=999999999999 and .run_attempt==(.run_attempt|floor)) and
    (.source_sha|type)=="string" and (.source_sha|test("^[0-9a-f]{40}$")) and
    (.fixture_stage|stage) and (.diagnostics|events) and
    (.private_state_removed|type)=="boolean" and
    (.readiness_native_cleanup_ok|type)=="boolean" and
    (.relay_children_reaped|type)=="boolean" and .supervisor_reaped_by_this_step==false
  end
else error("invalid mode") end
