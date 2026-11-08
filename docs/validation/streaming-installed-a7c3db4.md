# Installed qualification chain: a7c3db4

Source `a7c3db4b30a953c3f3413226566e7650b282d33f`; candidate [42424780330/1](https://github.com/Epiphytic/marmish/actions/runs/42424780330) passed all three native package jobs and render. The same-source chain stopped at edge-rsync. **Qualification: none**; no release/tag or installer pin changed.

| Cohort | Run/attempt | Exact observations | Wall seconds | Application goodput B/s |
|---|---:|---:|---:|---:|
| stream-matrix | 42432527249/1 | 9 | 45.825029890–57.530510973 | 3559.850183–4469.173299 |
| rsync-matrix | 42432825785/1 | 18 | 32.203326625–49.256315166 | 665.254798–1017.534629 |
| copy-matrix | 42433236789/1 | 18 | 25.551775491–54.329150166 | 1206.276921–2564.831552 |
| edge-stream | 42433589387/1 | 27 | 0.003002515–9.887613183 | not measured |

Stream validated 98,304 stdin + 98,304 stdout + 8,192 stderr bytes per directed cell, exact nonce-derived hashes/argv and exit 7. Rsync validated 32,768 file bytes across 16 files plus directory, special paths, hashes, mode/mtime and exit 0 in both directions. Copy validated 65,536 bytes per file, exact content/metadata and full committed checkpoint/stream/transfer bindings in both directions. Matching fixture receipts, control bindings and cross-cohort installation/tool/source pins were checked.

Edge-stream retained all 27 functional observations: finite missing/denied distinction, no-seclusion refusal, explicit cancellation, EPIPE/SIGKILL detach continuation followed by second-client Cancel, quota refusal, and pre-Open interruption. Recorded dwell proves only its observation period, not full pipe-window occupancy.

Edge-rsync [42433877249/1](https://github.com/Epiphytic/marmish/actions/runs/42433877249) failed all three platform jobs and rollup. All 12 canonical records are present; duplicate diagnostic copies match. Client/server complete=false, edges=null, no pairs/receipts or retained individual case/stage/binding. Failed client records have empty server-ready pin maps; server records still bind their own server-ready records. Rollup: validation_or_platform_failure, zero passed cells, qualification=none. Every role reports stream_cleanup_complete=true and client_stop_complete=true, but server_stop_complete=false. This neither identifies the failed case nor establishes no remote execution; no root cause is inferred here.

Edge-copy, copy-interrupt-put and copy-interrupt-get were not dispatched. No final eight-cohort auditor was run.

Goodput is validated application bytes divided by monotonic installed CLI wall time, not RTT, network throughput or compression ratio. Each directed case has one sample; no percentile claim. Edge timing includes functional observation/cancel/cleanup and has no payload goodput. Cleanup flags are retained reports, not independent proof of process/filesystem absence.

The accompanying JSON retains exact context/rollup/record/status hashes, row metrics, package test counts and candidate artifact pins. Native suite counts overlap and must not be summed. Candidate uploaded ZIP hashes match upload logs/API; this independent installed audit hashed retained JSON files rather than downloading ZIPs anew.

Historical correction: old 50ad macOS vault count is 13, Linux 12; the old candidate report has been corrected. Prior triage identified the macOS legacy failure as the denial-diagnostic unittest, not native probe; that legacy cause was not re-audited here.

Independent review: root and the actual integrated Claude code review checked the partial report and rate arithmetic; this is not the final eight-cohort qualification audit.
