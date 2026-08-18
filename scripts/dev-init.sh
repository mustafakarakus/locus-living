#!/usr/bin/env bash
# Create a local HOMEAI_PREFIX tree so homeai-core can run on a developer Mac.
set -euo pipefail
root="${HOMEAI_PREFIX:-$(cd "$(dirname "$0")/.." && pwd)/.run}"
mkdir -p "$root/etc/homeai/tls/tokens" "$root/var/lib/homeai" "$root/var/log/homeai"
chmod 700 "$root/etc/homeai/tls/tokens" 2>/dev/null || true
if [[ ! -f "$root/etc/homeai/config.toml" ]]; then
  cat >"$root/etc/homeai/config.toml" <<'EOF'
[api]
host = "127.0.0.1"
port = 8443

[grpc]
host = "127.0.0.1"
port = 50051

[llm]
url = "http://127.0.0.1:8200"

[stt]
url = "http://127.0.0.1:8100"

[tts]
url = "http://127.0.0.1:8300"

[presence]
exit_delay_ms = 30000

[wake]
keyword = "hey home"
EOF
fi
echo "HOMEAI_PREFIX=$root"
echo "Run: HOMEAI_PREFIX=$root cargo run -p homeai-core"
