#!/bin/sh
# Pin verified online against release SHA256SUMS and the downloaded asset;
# see docs/phases/platform-validation.md (bootstrap provenance).
set -eu
python_release=20261021
python_asset='cpython-3.13.17%2B20261021-aarch64-apple-darwin-install_only.tar.gz'
python_sha256=9c985cbf327425873469896ce748c7134b6deca8edb81e93c589ad2fc5aff445
if [ "$(uname -s)" = Darwin ]; then
    if [ "$(uname -m)" != arm64 ]; then
        echo macos_arm64_required >&2
        exit 1
    fi
    destination=$(mktemp -d "$RUNNER_TEMP/marmish-python.XXXXXX")
    archive="$destination/bootstrap.tar.gz"
    complete=false
    cleanup() {
        rm -f "$archive"
        if [ "$complete" != true ]; then rm -rf "$destination"; fi
    }
    trap cleanup EXIT HUP INT TERM
    curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
      --retry 3 --max-time 180 \
      "https://github.com/astral-sh/python-build-standalone/releases/download/$python_release/$python_asset" \
      --output "$archive"
    actual=$(shasum -a 256 "$archive")
    test "${actual%% *}" = "$python_sha256"
    tar -xzf "$archive" -C "$destination"
    "$destination/python/bin/python3" -c 'import sys,tarfile; assert sys.version_info >= (3,11) and hasattr(tarfile,"data_filter"), "python_capability_required"'
    echo "$destination/python/bin" >> "$GITHUB_PATH"
    complete=true
else
    python3 -c 'import sys,tarfile; assert sys.version_info >= (3,11) and hasattr(tarfile,"data_filter"), "python_capability_required"'
fi
