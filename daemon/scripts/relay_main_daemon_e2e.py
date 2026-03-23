#!/usr/bin/env python3
"""End-to-end relay validation against the real agentchat-daemon binary.

This script validates the full path:
- local relay Worker
- main `agentchat-daemon` binary in relay mode
- fake ACP agent backend
- encrypted relay transport
- real application protocol messages over relay

It exercises:
- create_session
- prompt
- streamed delta/tool_update
- turn_end

Run from `daemon/` while the local relay Worker is serving from `../relay`:

    python3 scripts/relay_main_daemon_e2e.py
"""

from __future__ import annotations

import argparse
import json
import os
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable
from urllib.request import Request, urlopen

ANSI_ESCAPE_PATTERN = __import__("re").compile(r"\x1b\[[0-9;]*m")


class RelayMainDaemonE2EError(RuntimeError):
    pass


@dataclass
class ManagedProcess:
    name: str
    process: subprocess.Popen[str]
    lines: list[str]
    line_queue: queue.Queue[str]
    thread: threading.Thread

    @classmethod
    def start(
        cls,
        name: str,
        command: list[str],
        cwd: Path,
        env: dict[str, str],
    ) -> "ManagedProcess":
        process = subprocess.Popen(
            command,
            cwd=cwd,
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
        )

        line_queue: queue.Queue[str] = queue.Queue()
        lines: list[str] = []

        def reader() -> None:
            assert process.stdout is not None
            for line in process.stdout:
                text = line.rstrip("\n")
                print(f"[{name}] {text}")
                plain_text = ANSI_ESCAPE_PATTERN.sub("", text)
                lines.append(plain_text)
                line_queue.put(plain_text)

        thread = threading.Thread(target=reader, daemon=True)
        thread.start()
        return cls(name=name, process=process, lines=lines, line_queue=line_queue, thread=thread)

    def wait_for(
        self,
        predicate: Callable[[str], bool],
        timeout: float,
        description: str,
    ) -> str:
        deadline = time.time() + timeout
        for line in self.lines:
            if predicate(line):
                return line

        while time.time() < deadline:
            if self.process.poll() is not None:
                raise RelayMainDaemonE2EError(
                    f"{self.name} exited before {description} was observed"
                )
            remaining = deadline - time.time()
            try:
                line = self.line_queue.get(timeout=max(0.05, min(0.25, remaining)))
            except queue.Empty:
                continue
            if predicate(line):
                return line

        raise RelayMainDaemonE2EError(
            f"timed out waiting for {self.name} {description}"
        )

    def stop(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)


def post_json(url: str, payload: dict[str, Any], timeout: float) -> dict[str, Any]:
    data = json.dumps(payload).encode("utf-8")
    request = Request(
        url,
        data=data,
        headers={"content-type": "application/json"},
        method="POST",
    )
    with urlopen(request, timeout=timeout) as response:
        body = response.read().decode("utf-8")
    parsed = json.loads(body)
    if not isinstance(parsed, dict):
        raise RelayMainDaemonE2EError(
            f"expected object JSON from {url}, got {parsed!r}"
        )
    return parsed


def require_string(payload: dict[str, Any], key: str) -> str:
    value = payload.get(key)
    if not isinstance(value, str) or not value:
        raise RelayMainDaemonE2EError(
            f"missing or invalid string field {key!r}: {payload!r}"
        )
    return value


class RelayMainDaemonE2ETester:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.daemon_dir = Path(__file__).resolve().parent.parent
        self.workspace_root = self.daemon_dir.parent
        self.temp_dir = Path(tempfile.mkdtemp(prefix="agentchat-relay-main-daemon-"))
        self.project_root = self.temp_dir / "project"
        self.project_root.mkdir(parents=True, exist_ok=True)
        self.fake_agent_events_path = self.temp_dir / "fake-agent-events.log"
        self.daemon_process: ManagedProcess | None = None
        self.app_process: ManagedProcess | None = None

    def run(self) -> None:
        try:
            self._build_binaries()
            relay_base = self.args.relay_http.rstrip("/")
            bootstrap = post_json(
                f"{relay_base}/v1/dev/bootstrap",
                {
                    "device_id": self.args.device_id,
                    "device_name": self.args.device_name,
                },
                timeout=self.args.http_timeout,
            )
            pair = post_json(
                f"{relay_base}/v1/dev/pair",
                {
                    "device_id": bootstrap["device_id"],
                    "app_installation_id": self.args.app_installation_id,
                    "app_name": self.args.app_name,
                },
                timeout=self.args.http_timeout,
            )

            daemon_ws_url = require_string(bootstrap, "ws_url")
            daemon_token = require_string(bootstrap, "relay_token")
            app_ws_url = require_string(pair, "ws_url")
            app_token = require_string(pair, "relay_token")

            print(f"[setup] project_root={self.project_root}")
            print(f"[setup] fake_agent_events_path={self.fake_agent_events_path}")

            self.daemon_process = self._start_main_daemon(daemon_ws_url, daemon_token)
            self.daemon_process.wait_for(
                lambda line: "relay transport connected; waiting for secure channel" in line,
                self.args.process_timeout,
                "relay transport connection",
            )

            self.app_process = self._start_app_protocol_smoke(app_ws_url, app_token)
            self.app_process.wait_for(
                lambda line: "relay application protocol flow succeeded" in line,
                self.args.process_timeout,
                "relay app protocol success",
            )
            self.daemon_process.wait_for(
                lambda line: "relay secure channel active" in line,
                self.args.process_timeout,
                "secure channel activation",
            )

            self._assert_fake_agent_events()
            print("[result] main daemon relay app protocol flow completed successfully")
        finally:
            if self.app_process is not None:
                self.app_process.stop()
            if self.daemon_process is not None:
                self.daemon_process.stop()
            shutil.rmtree(self.temp_dir, ignore_errors=True)

    def _build_binaries(self) -> None:
        command = [
            "cargo",
            "build",
            "-p",
            "agentchat-daemon",
            "--bin",
            "agentchat-daemon",
            "--bin",
            "relay_app_protocol_smoke",
            "-p",
            "agentchat-server",
            "--bin",
            "fake_acp_agent",
        ]
        print(f"[build] {' '.join(command)}")
        subprocess.run(command, cwd=self.daemon_dir, check=True)

    def _start_main_daemon(self, ws_url: str, relay_token: str) -> ManagedProcess:
        env = os.environ.copy()
        env.update(
            {
                "RUST_LOG": env.get("RUST_LOG", "info"),
                "AGENTCHAT_RELAY_WS_URL": ws_url,
                "AGENTCHAT_RELAY_TOKEN": relay_token,
                "AGENTCHAT_RELAY_DEV_CRYPTO": "true",
                "AGENTCHAT_AGENT_ID": "fake",
                "AGENTCHAT_AGENT_NAME": "Fake ACP Agent",
                "AGENTCHAT_AGENT_COMMAND": str(self.daemon_dir / "target" / "debug" / "fake_acp_agent"),
                "FAKE_ACP_MODE": "normal",
                "FAKE_ACP_EVENTS_PATH": str(self.fake_agent_events_path),
                "CARGO_TERM_COLOR": "never",
            }
        )
        return ManagedProcess.start(
            "main-daemon",
            [str(self.daemon_dir / "target" / "debug" / "agentchat-daemon")],
            self.project_root,
            env,
        )

    def _start_app_protocol_smoke(self, ws_url: str, relay_token: str) -> ManagedProcess:
        env = os.environ.copy()
        env.update(
            {
                "RUST_LOG": env.get("RUST_LOG", "info"),
                "AGENTCHAT_RELAY_WS_URL": ws_url,
                "AGENTCHAT_RELAY_TOKEN": relay_token,
                "CARGO_TERM_COLOR": "never",
            }
        )
        return ManagedProcess.start(
            "relay-app",
            [str(self.daemon_dir / "target" / "debug" / "relay_app_protocol_smoke")],
            self.project_root,
            env,
        )

    def _assert_fake_agent_events(self) -> None:
        deadline = time.time() + self.args.process_timeout
        while time.time() < deadline:
            if self.fake_agent_events_path.exists():
                lines = self.fake_agent_events_path.read_text().splitlines()
                if any(line.startswith("new_session:session-1:") for line in lines) and any(
                    line == "prompt:session-1:say hello" for line in lines
                ):
                    print("[result] fake ACP agent recorded new_session and prompt")
                    return
            time.sleep(0.1)

        raise RelayMainDaemonE2EError(
            "fake ACP agent did not record the expected new_session/prompt events"
        )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--relay-http",
        default="http://127.0.0.1:8787",
        help="base URL for the local relay Worker dev server",
    )
    parser.add_argument("--device-id", default="dev_main_daemon_1")
    parser.add_argument("--device-name", default="main daemon relay e2e")
    parser.add_argument("--app-installation-id", default="app_main_daemon_1")
    parser.add_argument("--app-name", default="relay app protocol test")
    parser.add_argument("--http-timeout", type=float, default=10.0)
    parser.add_argument(
        "--process-timeout",
        type=float,
        default=120.0,
        help="how long to wait for the daemon/app processes to complete the relay flow",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    tester = RelayMainDaemonE2ETester(args)
    try:
        tester.run()
    except RelayMainDaemonE2EError as exc:
        print(f"[error] {exc}", file=sys.stderr)
        return 1
    except subprocess.CalledProcessError as exc:
        print(f"[error] command failed: {exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("[error] interrupted", file=sys.stderr)
        return 130
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
