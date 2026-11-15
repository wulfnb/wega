#!/bin/sh
# Native end-to-end DevTools inspector check: real window, real Page, real wega-engine process,
# including a SIGSTOP-stalled worker. Needs a display and a build of both binaries:
#   cargo build --features test-harness --bin wega
#   cargo build --manifest-path engines/Cargo.toml -p wega-engine --bin wega-engine
set -eu
cd "$(dirname "$0")/.."
work=$(mktemp -d "${TMPDIR:-/tmp}/inspector-test-XXXXXX")
cp tests/fixtures/interaction.html "$work/"
(cd "$work" && python3 -m http.server 8791 >/dev/null 2>&1 & echo $! > "$work/pid")
trap 'kill "$(cat "$work/pid")" 2>/dev/null || true' EXIT
sleep 1
engine=${WEGA_ENGINE_BINARY:-engines/target/debug/wega-engine}
WEGA_TEST_ALLOW_LOOPBACK=1 WEGA_UNSAFE_DEVELOPMENT_WORKER=1 WEGA_ENGINE_BINARY="$PWD/$engine" \
    timeout 120 target/debug/wega --inspector-test --engine wega --profile-dir "$work/profile" \
    http://127.0.0.1:8791/interaction.html | tee "$work/log"
grep -q "INSPECTOR TEST PASSED" "$work/log"
