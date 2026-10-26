#!/usr/bin/env bash
# Starts the daemon built with the UI embedded, in a fresh application directory, under a URL
# base, for the browser smoke test. The default local account (reseek/reseek) logs in.
set -euo pipefail
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
cat > "$dir/config.toml" <<TOML
remote_configuration = true
[web]
port = ${SMOKE_PORT:-5139}
ip_address = "127.0.0.1"
url_base = "/reseek"
[web.https]
disabled = true
[flags]
no_connect = true
no_version_check = true
no_share_scan = true
TOML
"$(dirname "$0")/../../target/debug/reseek" --app-dir "$dir" --no-logo
