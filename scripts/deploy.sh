#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="/opt/jimeng-gateway"
BINARY_NAME="jimeng-gateway"
SERVICE_NAME="jimeng-gateway"

cd "$PROJECT_DIR"

echo "==> Pulling latest code..."
git fetch origin main
git reset --hard origin/main

echo "==> Building Rust backend (release)..."
source "$HOME/.cargo/env"
cargo build --release

echo "==> Building React frontend..."
cd web
npm install --no-audit --no-fund
npm run build
cd ..

echo "==> Restarting service..."
systemctl restart "$SERVICE_NAME"

echo "==> Waiting for health check..."
sleep 3
if curl -sf http://127.0.0.1:5100/ping > /dev/null 2>&1; then
  echo "==> Deploy successful! Service is healthy."
else
  echo "==> WARNING: Health check failed. Check logs: journalctl -u $SERVICE_NAME -n 50"
  exit 1
fi
