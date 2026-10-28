#!/usr/bin/env bash
# Starts the daemon built with the UI embedded, in a fresh application directory, under a URL
# base, for the browser smoke test. The default local account (reseek/reseek) logs in.
#
# Not connected to any server, unless SMOKE_NETWORK is set and soulfind (SOULFIND_BIN or PATH)
# and the soulseek-rs CLI (SOULSEEK_RS_BIN or PATH) are found: then it logs in to a soulfind of
# its own, where the soulseek-rs CLI serves a few files as `smokepeer`, so the daemon can have
# real transfers. The ports after SMOKE_PORT are theirs.
set -euo pipefail
port="${SMOKE_PORT:-5139}"
dir="$(mktemp -d)"
trap 'kill $(jobs -p) 2>/dev/null; rm -rf "$dir"' EXIT

soulfind="${SOULFIND_BIN:-$(command -v soulfind || true)}"
soulseek_rs="${SOULSEEK_RS_BIN:-$(command -v soulseek-rs || true)}"
connect=""
if [ -n "${SMOKE_NETWORK:-}" ] && [ -n "$soulfind" ] && [ -n "$soulseek_rs" ]; then
  server=$((port + 1))
  "$soulfind" -p "$server" -d "$dir/soulfind.db" > "$dir/soulfind.log" 2>&1 &
  until (exec 3<> "/dev/tcp/127.0.0.1/$server") 2> /dev/null; do sleep 0.1; done
  mkdir -p "$dir/peer/share/Album"
  for n in 1 2 3 4 5 6; do head -c 200000 /dev/urandom > "$dir/peer/share/Album/$n.flac"; done
  SOULSEEK_CONFIG_DIR="$dir/peer/config" SOULSEEK_STATE_DIR="$dir/peer/state" \
    "$soulseek_rs" --no-daemon --no-config --quiet --server "127.0.0.1:$server" \
    --username smokepeer --password smokepeer --listener-port $((port + 3)) \
    --shared-dir "$dir/peer/share" --download-dir "$dir/peer/downloads" \
    serve --duration 3600 > "$dir/peer.log" 2>&1 &
  connect="
[soulseek]
address = \"127.0.0.1\"
port = $server
username = \"smoke\"
password = \"smoke\"
listen_port = $((port + 2))
port_mapping = false"
fi

cat > "$dir/config.toml" <<TOML
remote_configuration = true
[web]
port = $port
ip_address = "127.0.0.1"
url_base = "/reseek"
[web.https]
disabled = true
[flags]
no_connect = $([ -n "$connect" ] && echo false || echo true)
no_version_check = true
no_share_scan = $([ -n "$connect" ] && echo false || echo true)
$connect
TOML
"$(dirname "$0")/../../target/debug/reseek" --app-dir "$dir" --no-logo
