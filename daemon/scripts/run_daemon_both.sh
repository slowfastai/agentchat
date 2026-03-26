#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
DAEMON_DIR="$(cd -- "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd -- "$DAEMON_DIR/.." && pwd)"

WORKING_DIR="${AGENTCHAT_BOTH_WORKING_DIR:-$REPO_ROOT}"

OPENCODE_ID="${AGENTCHAT_OPENCODE_ID:-opencode}"
OPENCODE_NAME="${AGENTCHAT_OPENCODE_NAME:-OpenCode}"
OPENCODE_COMMAND="${AGENTCHAT_OPENCODE_COMMAND:-opencode}"
OPENCODE_ARGS="${AGENTCHAT_OPENCODE_ARGS:-acp}"

CODEX_ID="${AGENTCHAT_CODEX_ID:-codex}"
CODEX_NAME="${AGENTCHAT_CODEX_NAME:-Codex}"
CODEX_COMMAND="${AGENTCHAT_CODEX_COMMAND:-codex}"
CODEX_ARGS="${AGENTCHAT_CODEX_ARGS:-}"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required but was not found in PATH" >&2
  exit 1
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "python3 is required but was not found in PATH" >&2
  exit 1
fi

if ! command -v "$OPENCODE_COMMAND" >/dev/null 2>&1; then
  echo "OpenCode command not found in PATH: $OPENCODE_COMMAND" >&2
  exit 1
fi

if ! command -v "$CODEX_COMMAND" >/dev/null 2>&1; then
  echo "Codex command not found in PATH: $CODEX_COMMAND" >&2
  exit 1
fi

echo "[run-daemon-both] repo root: $REPO_ROOT"
echo "[run-daemon-both] working dir: $WORKING_DIR"
echo "[run-daemon-both] opencode command: $OPENCODE_COMMAND ${OPENCODE_ARGS}"
echo "[run-daemon-both] codex command: $CODEX_COMMAND ${CODEX_ARGS}"

AGENTCHAT_AGENTS_JSON="$(
  WORKING_DIR="$WORKING_DIR" \
  OPENCODE_ID="$OPENCODE_ID" \
  OPENCODE_NAME="$OPENCODE_NAME" \
  OPENCODE_COMMAND="$OPENCODE_COMMAND" \
  OPENCODE_ARGS="$OPENCODE_ARGS" \
  CODEX_ID="$CODEX_ID" \
  CODEX_NAME="$CODEX_NAME" \
  CODEX_COMMAND="$CODEX_COMMAND" \
  CODEX_ARGS="$CODEX_ARGS" \
  python3 - <<'PY'
import json
import os
import shlex

def split_args(value: str) -> list[str]:
    value = value.strip()
    return shlex.split(value) if value else []

configs = [
    {
        "id": os.environ["OPENCODE_ID"],
        "name": os.environ["OPENCODE_NAME"],
        "backend": "acp",
        "command": os.environ["OPENCODE_COMMAND"],
        "args": split_args(os.environ["OPENCODE_ARGS"]),
        "working_dir": os.environ["WORKING_DIR"],
        "env_vars": {},
        "extra": {"kind": "opencode"},
    },
    {
        "id": os.environ["CODEX_ID"],
        "name": os.environ["CODEX_NAME"],
        "backend": "codex_app_server",
        "command": os.environ["CODEX_COMMAND"],
        "args": split_args(os.environ["CODEX_ARGS"]),
        "working_dir": os.environ["WORKING_DIR"],
        "env_vars": {},
        "extra": {"kind": "codex"},
    },
]

print(json.dumps(configs, separators=(",", ":")))
PY
)"

cd "$REPO_ROOT"
export AGENTCHAT_AGENTS_JSON

exec cargo run --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon -- "$@"
