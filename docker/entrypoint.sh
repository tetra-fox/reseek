#!/bin/sh
# Starts reseek in the container with the application directory ready: as the user the container
# was started as, as root, or, with PUID or PGID, as the reseek user moved to those ids.
set -eu

app="${RESEEK_APP_DIR:-/app}"
# every mode inherits this through exec and gosu
umask "${RESEEK_UMASK:-0022}"
# the daemon reads every RESEEK_ variable as an option; this one is the container's own
unset RESEEK_UMASK

warn_unless_mounted() {
  if ! mountpoint -q "$app"; then
    echo "WARNING: $app is not a bind mount or volume. Configuration and data will be lost if the container is removed."
  fi
}

require_id() {
  case "$2" in
    '' | *[!0-9]*)
      echo "ERROR: $1 must be a non-negative integer, got '$2'."
      exit 1
      ;;
  esac
}

if [ "$(id -u)" != 0 ]; then
  if [ -n "${PUID:-}" ] || [ -n "${PGID:-}" ]; then
    echo "ERROR: PUID/PGID are set but the container is running as a non-root user ($(id -u):$(id -g)); remove PUID/PGID or start the container as root."
    exit 1
  fi
  mkdir -p "$app" 2>/dev/null || true
  if [ ! -r "$app" ] || [ ! -w "$app" ]; then
    echo "ERROR: $app is not readable and/or writable by the current user ($(id -u):$(id -g)); currently owned by $(stat -c '%u:%g' "$app" 2>/dev/null || echo unknown)."
    echo "To fix: chown -R $(id -u):$(id -g) /path/to/your/mounted/app/directory"
    exit 1
  fi
  warn_unless_mounted
  exec /usr/bin/reseek "$@"
fi

if [ -z "${PUID:-}" ] && [ -z "${PGID:-}" ]; then
  mkdir -p "$app"
  warn_unless_mounted
  exec /usr/bin/reseek "$@"
fi

puid="${PUID:-1000}"
pgid="${PGID:-1000}"
require_id PUID "$puid"
require_id PGID "$pgid"
if [ "$(id -g reseek)" != "$pgid" ]; then
  groupmod -o -g "$pgid" reseek
fi
if [ "$(id -u reseek)" != "$puid" ]; then
  usermod -o -u "$puid" reseek
fi
mkdir -p "$app"
warn_unless_mounted
if [ "$(stat -c '%u:%g' "$app")" != "$puid:$pgid" ]; then
  chown "$puid:$pgid" "$app"
fi
exec gosu reseek /usr/bin/reseek "$@"
