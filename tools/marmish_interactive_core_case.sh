#!/bin/sh
# Cargo succeeds when a filter matches no tests. Require a passing case.
set -eu

if output=$(cargo test --locked "$@" -- --format terse --color never); then
    printf '%s\n' "$output"
    if printf '%s\n' "$output" | grep -Eq '^test result: ok\. [1-9][0-9]* passed; 0 failed;'; then
        exit 0
    fi
    printf '%s\n' 'Interactive core selector ran no passing tests.' >&2
    exit 1
else
    status=$?
    printf '%s\n' "$output"
    exit "$status"
fi
