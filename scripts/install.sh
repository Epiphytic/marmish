#!/bin/sh
# Release template: replace version and three digest pins AFTER signing all assets.
# The complete generated file is served independently by both source mirrors.
main() {
    set -eu
    umask 077
    LC_ALL=C
    export LC_ALL
    version='0.3.0'
    sha_amd64='5973edf2eb4290a7cdcb268dfbec0b918ec0f474574177ed6a12743364f0c4fc'
    sha_arm64='dcd6cc170954c58c534b8749575f375ec35e5060f65b15996356fe5ba065e50e'
    sha_macos='1cffcbbac77cd46b7ffea7d41bbc946e3b9d044066489d5b0ecbfeb895b698cd'
    debug=no
    quickstart=no
    asset_dir=
    prefix=
    bin_dir=
    state_dir=
    key_storage=auto
    stage=arguments
    fail() { printf 'marmish installer: %s (stage=%s)\n' "$1" "$stage" >&2; exit 1; }
    record() { stage=$1; if [ "$debug" = yes ]; then printf 'marmish installer debug: stage=%s\n' "$stage" >&2; fi; }
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --quickstart) quickstart=yes; shift ;;
            --debug) debug=yes; shift ;;
            --key-storage)
                [ "$#" -ge 2 ] || fail missing_argument
                case "$2" in auto|file|keychain) key_storage=$2 ;; *) fail invalid_key_storage ;; esac
                shift 2 ;;
            --prefix|--bin-dir|--asset-dir|--state-dir)
                option=$1
                [ "$#" -ge 2 ] || fail missing_argument
                case "$2" in /*) ;; *) fail absolute_path_required ;; esac
                case "$option" in --prefix) prefix=$2 ;; --bin-dir) bin_dir=$2 ;; --asset-dir) asset_dir=$2 ;; --state-dir) state_dir=$2 ;; esac
                shift 2 ;;
            --help) printf '%s\n' 'Marmish native installer: --quickstart --debug --prefix DIR --bin-dir DIR --asset-dir DIR --state-dir DIR --key-storage auto|file|keychain'; return 0 ;;
            *) fail invalid_argument ;;
        esac
    done
    for utility in curl uname id mktemp chmod mkdir rm rmdir mv cat wc tr awk; do command -v "$utility" >/dev/null 2>&1 || fail missing_utility; done
    [ "$(id -u)" != 0 ] || fail install_root_forbidden
    case "${HOME-}" in /*) ;; *) fail absolute_home_required ;; esac
    [ -n "$prefix" ] || prefix=$HOME/.local/share/marmish-native
    [ -n "$bin_dir" ] || bin_dir=$HOME/.local/bin
    record target
    system=$(uname -s)
    machine=$(uname -m)
    case "$system/$machine" in
        Linux/x86_64) target=x86_64-unknown-linux-gnu; expected=$sha_amd64 ;;
        Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-gnu; expected=$sha_arm64 ;;
        Darwin/*)
            command -v sysctl >/dev/null 2>&1 || fail missing_utility
            translated=$(sysctl -in sysctl.proc_translated 2>/dev/null || :)
            [ "$translated" != 1 ] || fail native_arm64_shell_required
            [ "$machine" = arm64 ] || fail unsupported_platform
            target=aarch64-apple-darwin; expected=$sha_macos ;;
        *) fail unsupported_platform ;;
    esac
    valid_hex() { [ "${#1}" = 64 ] && case "$1" in *[!0-9a-f]*) return 1 ;; *) return 0 ;; esac; }
    printf '%s\n' "$version" | awk '
        function bounded(s) {return length(s)<20 || (length(s)==20 && ("x" s)<="x18446744073709551615")}
        /^0\.[23]\.(0|[1-9][0-9]*)(-rc\.[1-9][0-9]*)?$/ {s=substr($0,5);n=split(s,a,"-rc[.]");ok=bounded(a[1])&&(n==1||bounded(a[2]))} END {exit NR!=1 || !ok}' || fail unqualified_release_template
    valid_hex "$sha_amd64" && valid_hex "$sha_arm64" && valid_hex "$sha_macos" || fail unqualified_release_template
    if command -v sha256sum >/dev/null 2>&1; then
        checksum() { sha256sum "$1" | awk '{print $1}'; }
    elif command -v shasum >/dev/null 2>&1; then
        checksum() { shasum -a 256 "$1" | awk '{print $1}'; }
    else fail missing_checksum_utility
    fi
    temporary=$(mktemp -d "${TMPDIR:-/tmp}/marmish-install.XXXXXXXX") || fail temporary_directory
    cleanup() {
        rm -f "$temporary/marmish-$version-$target" "$temporary/marmishd-$version-$target" "$temporary/marmish-worker-$version-$target" "$temporary/THIRD-PARTY-LICENSES-$version-$target" "$temporary/marmish-$version-$target.manifest"
        rmdir "$temporary" 2>/dev/null || :
    }
    trap cleanup 0
    trap 'exit 1' 1 2 15
    # Only the fixed bounded asset names below are ever created inside this owned directory.
    fetch() {
        name=$1
        limit=$2
        if [ -n "$asset_dir" ]; then
            [ -f "$asset_dir/$name" ] || fail missing_asset
            size=$(wc -c < "$asset_dir/$name" | tr -d ' ')
            [ "$size" -le "$limit" ] || fail asset_size
            cat "$asset_dir/$name" > "$temporary/$name" || fail asset_read
        else
            curl --proto '=https' --tlsv1.2 -fsSL --connect-timeout 20 --max-time 180 --max-filesize "$limit" --output "$temporary/$name" "https://github.com/Epiphytic/marmish/releases/download/v$version/$name" 2>/dev/null || fail asset_download
        fi
        size=$(wc -c < "$temporary/$name" | tr -d ' ')
        [ "$size" -le "$limit" ] || fail asset_size
    }
    manifest=marmish-$version-$target.manifest
    record manifest
    fetch "$manifest" 4096
    [ "$(checksum "$temporary/$manifest")" = "$expected" ] || fail manifest_digest
    # Exact grammar shared with native_manifest; no eval, sourced data, or metadata URLs.
    awk -v version="$version" -v target="$target" '
        function dec(s,positive) { return s ~ /^(0|[1-9][0-9]*)$/ && (!positive || s != "0") && (length(s)<20 || (length(s)==20 && ("x" s)<="x18446744073709551615")) }
        function hex(s,n) {return length(s)==n && s !~ /[^0-9a-f]/}
        BEGIN {names[1]="marmish";names[2]="marmishd";names[3]="marmish-worker";names[4]="THIRD-PARTY-LICENSES";bad=0;total=0}
        /[^ -~]|\r|\t|^ | $|  / {bad=1}
        NR==1 {if($0!="marmish-manifest 1")bad=1}
        NR==2 {if($0!="version "version)bad=1}
        NR==3 {if($0!="target "target)bad=1}
        NR==4 {if(NF!=2||$1!="commit"||!hex($2,40))bad=1}
        NR==5 {if(NF!=3||$1!="signing"||($3!="notarized=no"&&$3!="notarized=yes"))bad=1; signing=$2;notarized=$3}
        NR==6 {if(NF!=2||$1!="minimum")bad=1;n=split($2,a,"=");m=split(a[2],b,"[.]");if(n!=2||m!=2||!dec(b[1],0)||!dec(b[2],0))bad=1;if(target ~ /linux-gnu$/){if(a[1]!="glibc"||signing!="none"||notarized!="notarized=no")bad=1}else{if(a[1]!="macos"||(signing!="adhoc"&&signing!="developer-id")||(signing=="adhoc"&&notarized!="notarized=no"))bad=1}}
        NR==7 {if(NF!=6||$1!="state")bad=1;keys[2]="profile";keys[3]="journal";keys[4]="engine";keys[5]="config";keys[6]="commands";for(i=2;i<=6;i++){n=split($i,a,"=");if(n!=2||a[1]!=keys[i]||!dec(a[2],1)||a[2]+0>4294967295)bad=1}}
        NR>=8&&NR<=11 {i=NR-7;if(NF!=5||$1!="file"||!hex($2,64)||!dec($3,1)||$5!=names[i]||$4!=(i==4?"0444":"0555")||$3+0>(i==4?16777216:268435456))bad=1;total+=$3}
        NR==12 {if($0!="end")bad=1}
        END {if(NR!=12||bad||total>822083584)exit 1}
    ' "$temporary/$manifest" || fail manifest_grammar
    # AWK accepts an unterminated final record; explicitly require final LF.
    [ "$(wc -l < "$temporary/$manifest" | tr -d ' ')" = 12 ] || fail manifest_grammar
    record asset_download
    for line in 8 9 10 11; do
        name=$(awk -v n="$line" 'NR==n {print $5}' "$temporary/$manifest")
        bytes=$(awk -v n="$line" 'NR==n {print $3}' "$temporary/$manifest")
        digest=$(awk -v n="$line" 'NR==n {print $2}' "$temporary/$manifest")
        file=$name-$version-$target
        fetch "$file" "$bytes"
        record asset_verify
        [ "$(wc -c < "$temporary/$file" | tr -d ' ')" = "$bytes" ] || fail asset_size
        [ "$(checksum "$temporary/$file")" = "$digest" ] || fail asset_digest
    done
    cli=$temporary/marmish-$version-$target
    chmod 0555 "$cli"
    record runtime_probe
    probe_status=0
    probe=$("$cli" release-info </dev/null 2>/dev/null) || probe_status=$?
    case "$probe_status" in
        0) ;;
        126) fail incompatible_runtime_check_TMPDIR_noexec ;;
        127) fail incompatible_runtime_loader_or_userland ;;
        *) fail runtime_probe_failed ;;
    esac
    commit=$(awk 'NR==4 {print $2}' "$temporary/$manifest")
    formats=$(awk 'NR==7 {for(i=2;i<=6;i++){split($i,a,"=");printf "%s%s",(i==2?"":","),a[2]}}' "$temporary/$manifest")
    [ "$probe" = "{\"schema\":1,\"version\":\"$version\",\"target\":\"$target\",\"commit\":\"$commit\",\"formats\":[$formats]}" ] || fail runtime_provenance
    record install_validate
    set -- "$cli" install --source-dir "$temporary" --manifest "$temporary/$manifest" --manifest-sha256 "$expected" --prefix "$prefix" --bin-dir "$bin_dir"
    [ -z "$state_dir" ] || set -- "$@" --state-dir "$state_dir"
    [ "$debug" != yes ] || set -- "$@" --debug
    "$@" </dev/null || fail native_install_failed
    if [ "$quickstart" = yes ]; then
        record setup_choices
        set -- "$bin_dir/marmish" setup --interactive --key-storage "$key_storage"
        [ -z "$state_dir" ] || set -- "$@" --state-dir "$state_dir"
        [ "$debug" != yes ] || set -- "$@" --debug
        "$@" </dev/tty || fail native_setup_failed
    fi
    record complete
    printf 'Installed Marmish %s (%s). Launchers: %s\n' "$version" "$target" "$bin_dir"
}
main "$@"
