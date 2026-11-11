#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
# The image filesystem is read-only (docker cp cannot write to it); stream into /tmp (tmpfs).
docker compose exec -T wega sh -c 'cat > /tmp/wega-download-server.py' < tests/download_server.py
docker compose exec -T --user browser wega sh -eu -c '
    test_dir=$(mktemp -d /tmp/wega-download-ui.XXXXXX)
    Xvfb :95 -screen 0 1440x900x24 -nolisten tcp -ac >"$test_dir/display.log" 2>&1 &
    display_pid=$!
    python3 /tmp/wega-download-server.py 8771 >"$test_dir/http.log" 2>&1 &
    server_pid=$!
    trap "kill $display_pid $server_pid 2>/dev/null || true" EXIT INT TERM
    export DISPLAY=:95 WEGA_TEST_ALLOW_LOOPBACK=1
    attempts=0
    until xdpyinfo >/dev/null 2>&1 && curl -fsS http://127.0.0.1:8771/ >/dev/null 2>&1; do
        attempts=$((attempts+1)); test "$attempts" -le 50; sleep 0.2
    done
    timeout 75 wega --download-test --profile-dir "$test_dir" http://127.0.0.1:8771/
'
