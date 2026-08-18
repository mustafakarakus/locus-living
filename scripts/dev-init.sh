#!/usr/bin/env bash
# Create a local HOMEAI_PREFIX tree so homeai-core can run on a developer Mac.
set -euo pipefail
root="${HOMEAI_PREFIX:-$(cd "$(dirname "$0")/.." && pwd)/.run}"
mkdir -p "$root/etc/homeai/tls" "$root/var/lib/homeai" "$root/var/log/homeai"
if [[ ! -f "$root/etc/homeai/config.toml" ]]; then
  cat >"$root/etc/homeai/config.toml" <<'EOF'
[api]
host = "127.0.0.1"
port = 8443

[grpc]
host = "127.0.0.1"
port = 50051
EOF
fi
echo "HOMEAI_PREFIX=$root"
echo "Run: HOMEAI_PREFIX=$root cargo run -p homeai-core"
