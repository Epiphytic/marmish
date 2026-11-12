#!/bin/sh
# CI telemetry only: preserve command streams/status; never print arguments.
set -u
if [ "$#" -lt 2 ]; then
  echo 'marmish CI timing: stage and command required' >&2
  exit 64
fi
stage=$1
shift
case "$stage" in ''|*[!a-z0-9-]*) echo 'marmish CI timing: invalid stage name' >&2; exit 64 ;; esac
started=$(date +%s)
if "$@"; then status=0; else status=$?; fi
elapsed=$(($(date +%s)-started))
printf 'marmish CI timing: stage=%s elapsed_seconds=%s exit=%s\n' "$stage" "$elapsed" "$status"
if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
  printf -- '- %s: %s seconds (exit %s).\n' "$stage" "$elapsed" "$status" >> "$GITHUB_STEP_SUMMARY"
fi
exit "$status"
