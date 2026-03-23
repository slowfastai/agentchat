#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DAEMON_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd -- "$DAEMON_DIR/.." && pwd)"
RELAY_DIR="$REPO_ROOT/relay"

PORT="${AGENTCHAT_RELAY_CI_PORT:-8787}"
RELAY_HTTP_URL="${AGENTCHAT_RELAY_CI_HTTP_URL:-http://127.0.0.1:${PORT}}"
PROCESS_TIMEOUT="${AGENTCHAT_RELAY_CI_PROCESS_TIMEOUT:-180}"
SKIP_CHECKS="${AGENTCHAT_RELAY_CI_SKIP_CHECKS:-0}"
RELAY_LOG="${AGENTCHAT_RELAY_CI_RELAY_LOG:-$(mktemp -t agentchat-relay-ci.XXXXXX.log)}"
RELAY_PID=""

cleanup() {
  if [[ -n "$RELAY_PID" ]] && kill -0 "$RELAY_PID" >/dev/null 2>&1; then
    kill "$RELAY_PID" >/dev/null 2>&1 || true
    wait "$RELAY_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

wait_for_relay() {
  local attempts=0
  until python3 - <<PY >/dev/null 2>&1
from urllib.request import urlopen
urlopen("${RELAY_HTTP_URL}/healthz", timeout=1).read()
PY
  do
    attempts=$((attempts + 1))
    if [[ "$attempts" -ge 60 ]]; then
      echo "relay Worker did not become healthy at ${RELAY_HTTP_URL}" >&2
      echo "relay log: ${RELAY_LOG}" >&2
      exit 1
    fi
    sleep 1
  done
}

echo "[relay-ci] repo root: ${REPO_ROOT}"
echo "[relay-ci] daemon dir: ${DAEMON_DIR}"
echo "[relay-ci] relay dir: ${RELAY_DIR}"
echo "[relay-ci] relay url: ${RELAY_HTTP_URL}"

if [[ "$SKIP_CHECKS" != "1" ]]; then
  echo "[relay-ci] running relay checks"
  (
    cd "$RELAY_DIR"
    npm run typecheck
    npm test
  )

  echo "[relay-ci] running daemon checks"
  (
    cd "$DAEMON_DIR"
    cargo test
    cargo clippy --all-targets --all-features -- -D warnings
  )
else
  echo "[relay-ci] skipping pre-checks because AGENTCHAT_RELAY_CI_SKIP_CHECKS=1"
fi

echo "[relay-ci] starting local relay Worker"
(
  cd "$RELAY_DIR"
  npm run dev -- --port "$PORT" >"$RELAY_LOG" 2>&1
) &
RELAY_PID="$!"

wait_for_relay

echo "[relay-ci] running main daemon relay end-to-end validation"
(
  cd "$DAEMON_DIR"
  python3 scripts/relay_main_daemon_e2e.py \
    --relay-http "$RELAY_HTTP_URL" \
    --process-timeout "$PROCESS_TIMEOUT"
)

echo "[relay-ci] relay main daemon end-to-end validation succeeded"
