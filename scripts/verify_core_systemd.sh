#!/usr/bin/env bash
# Linux: systemctl start/restart → healthy. Run on Ubuntu 24.04 (or CI).
# Run on Ubuntu 24.04 (or CI). Not executable as proof on macOS.
set -euo pipefail
if ! command -v systemctl >/dev/null; then
  echo "systemctl not found — this script is for Linux." >&2
  exit 2
fi
if [[ "$(id -u)" -ne 0 ]]; then
  echo "run as root on the Core host" >&2
  exit 2
fi

repo="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --release -p homeai-core --manifest-path "$repo/Cargo.toml"
install -m 0755 "$repo/target/release/homeai-core" /usr/local/bin/homeai-core
install -m 0644 "$repo/deploy/systemd/homeai-core.service" /etc/systemd/system/homeai-core.service

mkdir -p /etc/homeai/tls /var/lib/homeai /var/log/homeai
if [[ ! -f /etc/homeai/config.toml ]]; then
  cp "$repo/deploy/config/config.toml" /etc/homeai/config.toml
fi
if [[ ! -f /etc/homeai/tls/cert.pem ]]; then
  openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
    -subj "/CN=homeai" \
    -keyout /etc/homeai/tls/key.pem \
    -out /etc/homeai/tls/cert.pem
  chmod 0600 /etc/homeai/tls/key.pem
fi

systemctl daemon-reload
systemctl enable homeai-core
systemctl restart homeai-core
sleep 1
systemctl is-active --quiet homeai-core
curl -sk --max-time 2 https://127.0.0.1:8443/api/v1/health | grep -q '"status"'

systemctl restart homeai-core
sleep 1
systemctl is-active --quiet homeai-core
curl -sk --max-time 2 https://127.0.0.1:8443/api/v1/health | grep -q '"status"'

if ip route show default >/dev/null 2>&1 && ip route show default | grep -q .; then
  echo "note: default route is present; drop it manually to re-check WAN-off on this host"
fi

echo "systemd: restart → healthy"
