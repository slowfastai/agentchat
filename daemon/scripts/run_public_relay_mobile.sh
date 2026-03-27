#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DAEMON_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd -- "$DAEMON_DIR/.." && pwd)"

usage() {
  cat <<'EOF'
Usage:
  run_public_relay_mobile.sh --relay-http https://<relay-host> [options] [-- daemon args...]

Options:
  --relay-http URL     Base HTTP(S) URL for the deployed relay Worker.
                       You can also set AGENTCHAT_RELAY_HTTP_URL.
  --device-id ID       Device ID to bootstrap. Default: derived from hostname.
  --device-name NAME   Human-readable device name. Default: computer name / hostname.
  --http-timeout SEC   Timeout for the bootstrap request. Default: 10.
  --no-mobile          Do not add --mobile automatically when no daemon args are given.
  -h, --help           Show this help text.

This helper does three things:
1. Calls POST /v1/dev/bootstrap on the deployed relay Worker
2. Exports AGENTCHAT_RELAY_WS_URL, AGENTCHAT_RELAY_TOKEN, and AGENTCHAT_RELAY_DEV_CRYPTO=true
3. Prefetches a pairing ticket when launching in mobile mode
4. Starts agentchat-daemon, defaulting to --mobile

Important:
- This script depends on the relay dev bootstrap endpoint being enabled on the deployed Worker.
- The current iPhone relay QR flow still requires AGENTCHAT_RELAY_DEV_CRYPTO=true.
- Any AGENTCHAT_AGENT_* or AGENTCHAT_AGENTS_JSON settings already in your shell are preserved.
- Set AGENTCHAT_RELAY_USER_AGENT to override the default browser-like User-Agent if needed.
EOF
}

require_command() {
  local command="$1"
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "$command is required but was not found in PATH" >&2
    exit 1
  fi
}

sanitize_token_component() {
  local raw="$1"
  local sanitized
  sanitized="$(printf '%s' "$raw" | tr -cs 'A-Za-z0-9_-' '_' | sed 's/^_*//; s/_*$//')"
  if [[ -z "$sanitized" ]]; then
    sanitized="mac"
  fi
  printf '%.96s' "$sanitized"
}

default_device_name() {
  if command -v scutil >/dev/null 2>&1; then
    scutil --get ComputerName 2>/dev/null || hostname
  else
    hostname
  fi
}

RELAY_HTTP_URL="${AGENTCHAT_RELAY_HTTP_URL:-}"
RELAY_USER_AGENT="${AGENTCHAT_RELAY_USER_AGENT:-Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36}"
DEVICE_NAME="${AGENTCHAT_RELAY_DEVICE_NAME:-$(default_device_name)}"
HOST_TOKEN="$(sanitize_token_component "$(hostname -s 2>/dev/null || hostname)")"
DEVICE_ID="${AGENTCHAT_RELAY_DEVICE_ID:-dev_public_${HOST_TOKEN}}"
HTTP_TIMEOUT="${AGENTCHAT_RELAY_HTTP_TIMEOUT:-10}"
AUTO_MOBILE=1
DAEMON_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --relay-http)
      if [[ $# -lt 2 ]]; then
        echo "--relay-http requires a value" >&2
        exit 1
      fi
      RELAY_HTTP_URL="$2"
      shift 2
      ;;
    --device-id)
      if [[ $# -lt 2 ]]; then
        echo "--device-id requires a value" >&2
        exit 1
      fi
      DEVICE_ID="$2"
      shift 2
      ;;
    --device-name)
      if [[ $# -lt 2 ]]; then
        echo "--device-name requires a value" >&2
        exit 1
      fi
      DEVICE_NAME="$2"
      shift 2
      ;;
    --http-timeout)
      if [[ $# -lt 2 ]]; then
        echo "--http-timeout requires a value" >&2
        exit 1
      fi
      HTTP_TIMEOUT="$2"
      shift 2
      ;;
    --no-mobile)
      AUTO_MOBILE=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      DAEMON_ARGS+=("$@")
      break
      ;;
    *)
      DAEMON_ARGS+=("$1")
      shift
      ;;
  esac
done

if [[ -z "$RELAY_HTTP_URL" ]]; then
  echo "missing relay URL: pass --relay-http or set AGENTCHAT_RELAY_HTTP_URL" >&2
  exit 1
fi

DEVICE_ID="$(sanitize_token_component "$DEVICE_ID")"
if [[ -z "$DEVICE_ID" ]]; then
  echo "device id must contain at least one of: A-Z a-z 0-9 _ -" >&2
  exit 1
fi

require_command cargo
require_command python3

if ! bootstrap_output="$(
  RELAY_HTTP_URL="$RELAY_HTTP_URL" \
  RELAY_USER_AGENT="$RELAY_USER_AGENT" \
  DEVICE_ID="$DEVICE_ID" \
  DEVICE_NAME="$DEVICE_NAME" \
  HTTP_TIMEOUT="$HTTP_TIMEOUT" \
  python3 - <<'PY'
import json
import os
import sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

relay_http_url = os.environ["RELAY_HTTP_URL"].rstrip("/")
relay_user_agent = os.environ["RELAY_USER_AGENT"]
device_id = os.environ["DEVICE_ID"]
device_name = os.environ["DEVICE_NAME"]
timeout = float(os.environ["HTTP_TIMEOUT"])

payload = json.dumps(
    {"device_id": device_id, "device_name": device_name},
    separators=(",", ":"),
).encode("utf-8")
request = Request(
    f"{relay_http_url}/v1/dev/bootstrap",
    data=payload,
    headers={
        "content-type": "application/json",
        "user-agent": relay_user_agent,
        "accept": "application/json",
    },
    method="POST",
)

try:
    with urlopen(request, timeout=timeout) as response:
        body = response.read().decode("utf-8")
except HTTPError as error:
    body = error.read().decode("utf-8", errors="replace").strip()
    print(
        f"relay bootstrap failed with HTTP {error.code}: {body}",
        file=sys.stderr,
    )
    sys.exit(1)
except URLError as error:
    print(f"relay bootstrap network error: {error}", file=sys.stderr)
    sys.exit(1)

try:
    parsed = json.loads(body)
except json.JSONDecodeError as error:
    print(
        f"relay bootstrap returned invalid JSON: {error}: {body!r}",
        file=sys.stderr,
    )
    sys.exit(1)

if not isinstance(parsed, dict):
    print(f"relay bootstrap expected a JSON object, got {parsed!r}", file=sys.stderr)
    sys.exit(1)

for key in ("device_id", "ws_url", "relay_token"):
    value = parsed.get(key)
    if not isinstance(value, str) or not value.strip():
        print(
            f"relay bootstrap missing or invalid {key!r}: {parsed!r}",
            file=sys.stderr,
        )
        sys.exit(1)

print(parsed["device_id"])
print(parsed["ws_url"])
print(parsed["relay_token"])
PY
)"; then
  exit 1
fi

mapfile -t bootstrap_values <<< "$bootstrap_output"

if [[ "${#bootstrap_values[@]}" -ne 3 ]]; then
  echo "relay bootstrap did not return the expected fields" >&2
  echo "relay bootstrap raw output:" >&2
  printf '%s\n' "$bootstrap_output" >&2
  exit 1
fi

BOOTSTRAPPED_DEVICE_ID="${bootstrap_values[0]}"
export AGENTCHAT_RELAY_WS_URL="${bootstrap_values[1]}"
export AGENTCHAT_RELAY_TOKEN="${bootstrap_values[2]}"
export AGENTCHAT_RELAY_DEV_CRYPTO=true
export AGENTCHAT_RELAY_USER_AGENT="$RELAY_USER_AGENT"

if [[ "${#DAEMON_ARGS[@]}" -eq 0 && "$AUTO_MOBILE" == "1" ]]; then
  DAEMON_ARGS=(--mobile)
fi

LAUNCH_MOBILE=0
for arg in "${DAEMON_ARGS[@]}"; do
  if [[ "$arg" == "--mobile" ]]; then
    LAUNCH_MOBILE=1
    break
  fi
done

if [[ "$LAUNCH_MOBILE" == "1" ]]; then
  if ! pairing_output="$(
    RELAY_HTTP_URL="$RELAY_HTTP_URL" \
    RELAY_USER_AGENT="$RELAY_USER_AGENT" \
    RELAY_TOKEN="$AGENTCHAT_RELAY_TOKEN" \
    HTTP_TIMEOUT="$HTTP_TIMEOUT" \
    python3 - <<'PY'
import json
import os
import sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

relay_http_url = os.environ["RELAY_HTTP_URL"].rstrip("/")
relay_user_agent = os.environ["RELAY_USER_AGENT"]
relay_token = os.environ["RELAY_TOKEN"]
timeout = float(os.environ["HTTP_TIMEOUT"])

request = Request(
    f"{relay_http_url}/v1/pairing/open",
    data=b"{}",
    headers={
        "content-type": "application/json",
        "accept": "application/json",
        "authorization": f"Bearer {relay_token}",
        "user-agent": relay_user_agent,
    },
    method="POST",
)

try:
    with urlopen(request, timeout=timeout) as response:
        body = response.read().decode("utf-8")
except HTTPError as error:
    body = error.read().decode("utf-8", errors="replace").strip()
    print(
        f"relay pairing-open failed with HTTP {error.code}: {body}",
        file=sys.stderr,
    )
    sys.exit(1)
except URLError as error:
    print(f"relay pairing-open network error: {error}", file=sys.stderr)
    sys.exit(1)

try:
    parsed = json.loads(body)
except json.JSONDecodeError as error:
    print(
        f"relay pairing-open returned invalid JSON: {error}: {body!r}",
        file=sys.stderr,
    )
    sys.exit(1)

if not isinstance(parsed, dict):
    print(f"relay pairing-open expected a JSON object, got {parsed!r}", file=sys.stderr)
    sys.exit(1)

for key in ("pairing_ticket", "ws_url"):
    value = parsed.get(key)
    if not isinstance(value, str) or not value.strip():
        print(
            f"relay pairing-open missing or invalid {key!r}: {parsed!r}",
            file=sys.stderr,
        )
        sys.exit(1)

print(parsed["pairing_ticket"])
print(parsed["ws_url"])
PY
  )"; then
    exit 1
  fi

  mapfile -t pairing_values <<< "$pairing_output"
  if [[ "${#pairing_values[@]}" -ne 2 ]]; then
    echo "relay pairing-open did not return the expected fields" >&2
    echo "relay pairing-open raw output:" >&2
    printf '%s\n' "$pairing_output" >&2
    exit 1
  fi

  export AGENTCHAT_RELAY_PAIRING_TICKET="${pairing_values[0]}"
  export AGENTCHAT_MOBILE_WS_URL="${pairing_values[1]}"
fi

echo "[public-relay-mobile] repo root: $REPO_ROOT"
echo "[public-relay-mobile] relay bootstrap url: ${RELAY_HTTP_URL%/}/v1/dev/bootstrap"
echo "[public-relay-mobile] device_id: $BOOTSTRAPPED_DEVICE_ID"
echo "[public-relay-mobile] relay ws url: $AGENTCHAT_RELAY_WS_URL"
echo "[public-relay-mobile] relay dev crypto: $AGENTCHAT_RELAY_DEV_CRYPTO"
echo "[public-relay-mobile] relay user-agent: $AGENTCHAT_RELAY_USER_AGENT"
if [[ "$LAUNCH_MOBILE" == "1" ]]; then
  echo "[public-relay-mobile] prefetched pairing ticket for mobile QR"
fi
echo "[public-relay-mobile] starting agentchat-daemon ${DAEMON_ARGS[*]}"

cd "$REPO_ROOT"
exec cargo run --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon -- "${DAEMON_ARGS[@]}"
