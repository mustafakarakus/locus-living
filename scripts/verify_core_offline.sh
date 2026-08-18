#!/usr/bin/env bash
# Run Core with WAN denied (macOS sandbox) and prove the API still answers.
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd)"
prefix="${HOMEAI_PREFIX:-$repo/.run}"
sb="$repo/scripts/offline.sb"

cargo build -p homeai-core --manifest-path "$repo/Cargo.toml"
"$repo/scripts/dev-init.sh" >/dev/null
bin="$repo/target/debug/homeai-core"

sandbox-exec -f "$sb" env HOMEAI_PREFIX="$prefix" "$bin" &
pid=$!
cleanup() { kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; }
trap cleanup EXIT

ok=0
for _ in $(seq 1 50); do
  if curl -sk --max-time 1 https://127.0.0.1:8443/api/v1/health | grep -q '"status"'; then
    ok=1
    break
  fi
  sleep 0.1
done
if [[ "$ok" -ne 1 ]]; then
  echo "health never answered under WAN-deny sandbox" >&2
  exit 1
fi

# Confirm the sandbox still blocks the public internet.
if sandbox-exec -f "$sb" /usr/bin/curl -sS --max-time 3 https://example.com -o /dev/null; then
  echo "sandbox failed to block WAN" >&2
  exit 1
fi

echo "offline: API responded with WAN denied"
