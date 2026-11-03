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
def integer($max): type=="number" and .>=0 and .<=$max and .==floor;
def terminal_label:
  IN("tls_verification","tls_hostname","proxy_forbidden","arti_configuration",
     "relay_not_configured","shutdown","delivery_closed","notification_forwarder_stopped");
def diagnostic_label:
  terminal_label or IN("timeout","transport_recovery_pending","subscription_sync",
    "journal_backpressure","relay_reconnect","recovery_subscription","notification_loss",
    "group_state_unavailable","add_relay","relay_connect","activate","arti_initialization",
    "arti_bootstrap","other");
def native_stage:
  IN("identity_prepare","activation_cold","conversation_open","conversation_join",
    "alice_to_bob","bob_to_alice","restart_shutdown","restart_open",
    "restart_activation_warm","restart_alice_to_bob","restart_bob_to_alice","completed","cleanup");
def sample_stages:
  ["activation_cold","alice_to_bob","bob_to_alice","restart_activation_warm",
   "restart_alice_to_bob","restart_bob_to_alice"];
def activation_sample: .stage=="activation_cold" or .stage=="restart_activation_warm";
def native_diagnostic:
  type=="object" and keys==["last_recoverable","operation","terminal_label"] and
  (.operation|IN("none","activation","open_conversation","join","receiver_tick","sender_tick",
    "received","sent_status","initial_send","duplicate_send","conflict_send")) and
  (.terminal_label==null or (.terminal_label|diagnostic_label)) and
  (.last_recoverable==null or (.last_recoverable|
    type=="object" and keys==["label","role"] and (.role|IN("sender","receiver")) and
    (.label|diagnostic_label and (terminal_label|not))));
def outcome_object:
  type=="object" and
  keys==["category","cleanup_ok","diagnostic","elapsed_ms","ok","runtime_cleanup_ok","samples","schema","stage","state_cleanup_ok"] and
  .schema==1 and (.ok|type)=="boolean" and (.cleanup_ok|type)=="boolean" and
  (.runtime_cleanup_ok|type)=="boolean" and (.state_cleanup_ok|type)=="boolean" and
  .cleanup_ok==(.runtime_cleanup_ok and .state_cleanup_ok) and
  .ok==(.category==null and .cleanup_ok) and
  (.category!=null or .cleanup_ok) and
  (.category==null or (.category|IN("deadline","identity","storage","transport","marmot","local_setup","cleanup"))) and
  (.stage|native_stage) and ((.stage=="cleanup")==(.category=="cleanup")) and
  (.category!="cleanup" or .cleanup_ok==false) and
  (.ok==false or .stage=="completed") and (.diagnostic|native_diagnostic) and
  (.elapsed_ms|integer(660000)) and
  (.elapsed_ms as $elapsed | .samples|type=="array" and length<=6 and
    ([.[].stage]==(sample_stages[0:length])) and all(.[];
      type=="object" and keys==["bytes","milliseconds","stage"] and
      (.milliseconds|integer($elapsed)) and (.bytes|integer(16384)) and
      (if activation_sample then .bytes==0 else .bytes>0 end)));
def native_outcome: try (.==null or outcome_object) catch false;
def native_source:
  type=="object" and
  keys==["category","cleanup_ok","diagnostic","elapsed_ms","error","ok","phase","platform","protocol","relay_count","route","runtime_cleanup_ok","samples","schema","stage","state_cleanup_ok","target"] and
  .schema==1 and .protocol=="marmish-foundation/2" and .phase=="v0.2-phase1" and
  .route=="always" and .error==.category and (.platform|IN("linux","macos")) and
  (.target|IN("x86_64","aarch64")) and (.relay_count|integer(16) and .>=1) and
  (.samples|type=="array" and length<=6 and all(.[];
    type=="object" and keys==["bytes","bytes_per_second","milliseconds","sha256","stage"] and
    (if activation_sample then .sha256=="" and .bytes_per_second==null
     else (.sha256|type)=="string" and (.sha256|test("^[0-9a-f]{64}$")) and
       (.bytes_per_second|integer(9007199254740991)) end)));
def project_native:
  {schema,ok,stage,category,diagnostic,elapsed_ms,
   samples:[.samples[]|{stage,milliseconds,bytes}],cleanup_ok,runtime_cleanup_ok,state_cleanup_ok};
if $mode == "extract" then
  reduce (inputs | select(utf8bytelength<=4096) | fromjson? |
    select(type=="object") |
    select(keys==["category","event","schema","stage"] and .schema==1 and .event=="diagnostic") |
    {stage,category} | select(pair)) as $event
    ([]; if .[-1] == $event then . else (. + [$event])[-32:] end)
elif $mode == "native-extract" then
  try ((if utf8bytelength<=65536 then
    . as $raw | fromjson | select($raw==(tojson+"\n")) |
    select(native_source) | project_native | select(outcome_object)
    else empty end) // null) catch null
elif $mode == "native-outcome" then length==1 and (.[0]|native_outcome)
elif $mode == "events" then length==1 and (.[0]|events)
elif $mode == "validate" then
  if type != "object" then false else
    keys==["diagnostics","fixture_stage","native_outcome","private_state_removed","readiness_native_cleanup_ok","relay_children_reaped","run_attempt","run_id","schema","source_sha","supervisor_reaped_by_this_step"] and
    .schema==1 and .run_id==$run and .run_attempt==$attempt and .source_sha==$sha and
    (.run_id|type)=="number" and (.run_attempt|type)=="number" and
    (.run_id>=1 and .run_id<=999999999999 and .run_id==(.run_id|floor)) and
    (.run_attempt>=1 and .run_attempt<=999999999999 and .run_attempt==(.run_attempt|floor)) and
    (.source_sha|type)=="string" and (.source_sha|test("^[0-9a-f]{40}$")) and
    (.fixture_stage|stage) and (.diagnostics|events) and (.native_outcome|native_outcome) and
    (.private_state_removed|type)=="boolean" and
    (.readiness_native_cleanup_ok|type)=="boolean" and
    (.relay_children_reaped|type)=="boolean" and .supervisor_reaped_by_this_step==false
  end
else error("invalid mode") end
