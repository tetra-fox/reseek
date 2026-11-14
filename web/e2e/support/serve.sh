#!/usr/bin/env bash
# Starts the daemon built with the UI embedded, in a fresh application directory, under a URL
# base and connected to no server, for the browser tests. The default local account
# (reseek/reseek) logs in.
#
# It listens on E2E_PORT, or on a port it picks when that is 0; daemon.ts reads which from its
# log. The log is kept after the run in target/e2e-logs/<the process id of this script>, for
# reading a failure.
set -euo pipefail
port="${E2E_PORT:?the port is given by e2e/support/daemon.ts}"
# the daemon takes only an absolute log directory
target="${CARGO_TARGET_DIR:-$(cd "$(dirname "$0")/../../.." && pwd)/target}"
logs="$target/e2e-logs/$$"
rm -rf "$logs"
echo "the daemon for the browser tests logs to $logs" >&2
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
# playwright stops the servers with SIGTERM; exiting on it runs the cleanup above
trap 'exit 143' TERM INT

cat > "$dir/config.toml" <<TOML
remote_configuration = true
[web]
port = $port
ip_address = "127.0.0.1"
url_base = "/reseek"
[web.https]
disabled = true
[flags]
no_connect = true
no_version_check = true
no_share_scan = true
TOML
"$target/debug/reseek" --app-dir "$dir" --log-dir "$logs" --no-logo --no-color
