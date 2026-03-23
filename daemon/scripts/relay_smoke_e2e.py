#!/usr/bin/env python3
"""Local relay end-to-end smoke test.

This script uses the relay dev helper endpoints to:
- bootstrap a daemon token
- pair an app token
- spawn the Rust daemon/app relay smoke clients
- wait for both sides to complete the real crypto handshake
- send a real encrypted relay_envelope from app to daemon
- receive and decrypt a real encrypted reply from daemon to app
- replay the first envelope and verify replay protection triggers

Run from `daemon/` while `relay/` is serving locally with `npm run dev`:

    python3 scripts/relay_smoke_e2e.py
"""

from __future__ import annotations

import argparse
import json
import os
import queue
import re
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable
from urllib.request import Request, urlopen

CHANNEL_ID_PATTERN = re.compile(r"channel_id=(?P<channel_id>[A-Za-z0-9_-]{22})")
ANSI_ESCAPE_PATTERN = re.compile(r"\x1b\[[0-9;]*m")


class SmokeTestError(RuntimeError):
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
        project_root: Path,
        binary_name: str,
        ws_url: str,
        relay_token: str,
    ) -> "ManagedProcess":
        env = os.environ.copy()
        env.update(
            {
                "AGENTCHAT_RELAY_WS_URL": ws_url,
                "AGENTCHAT_RELAY_TOKEN": relay_token,
                "RUST_LOG": env.get("RUST_LOG", "info"),
                "CARGO_TERM_COLOR": "never",
            }
        )
        process = subprocess.Popen(
            [
                "cargo",
                "run",
                "--quiet",
                "-p",
                "agentchat-daemon",
                "--bin",
                binary_name,
            ],
            cwd=project_root,
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
                raise SmokeTestError(
                    f"{self.name} process exited before {description} was observed"
                )
            remaining = deadline - time.time()
            try:
                line = self.line_queue.get(timeout=max(0.05, min(0.25, remaining)))
            except queue.Empty:
                continue
            if predicate(line):
                return line

        raise SmokeTestError(f"timed out waiting for {self.name} {description}")

    def wait_for_channel_id(self, timeout: float) -> str:
        line = self.wait_for(
            lambda candidate: CHANNEL_ID_PATTERN.search(candidate) is not None,
            timeout,
            "channel_id",
        )
        match = CHANNEL_ID_PATTERN.search(line)
        if not match:
            raise SmokeTestError(f"failed to parse channel_id from {self.name} line: {line}")
        return match.group("channel_id")

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
        raise SmokeTestError(f"expected object JSON from {url}, got {parsed!r}")
    return parsed


def require_string(payload: dict[str, Any], key: str) -> str:
    value = payload.get(key)
    if not isinstance(value, str) or not value:
        raise SmokeTestError(f"missing or invalid string field {key!r}: {payload!r}")
    return value


class RelayE2ESmokeTester:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.project_root = Path(__file__).resolve().parent.parent
        self.daemon_process: ManagedProcess | None = None
        self.app_process: ManagedProcess | None = None

    def run(self) -> None:
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
        app_peer_id = require_string(pair, "peer_id")

        print(f"[setup] device_id={bootstrap['device_id']}")
        print(f"[setup] daemon token ready")
        print(f"[setup] app peer_id={app_peer_id}")

        try:
            self.daemon_process = ManagedProcess.start(
                "daemon",
                self.project_root,
                "relay_smoke_daemon",
                daemon_ws_url,
                daemon_token,
            )
            self.app_process = ManagedProcess.start(
                "app",
                self.project_root,
                "relay_smoke_app",
                app_ws_url,
                app_token,
            )

            self.daemon_process.wait_for(
                lambda line: "relay_ready received" in line,
                self.args.process_timeout,
                "relay_ready",
            )
            self.app_process.wait_for(
                lambda line: "relay_ready received" in line,
                self.args.process_timeout,
                "relay_ready",
            )

            daemon_handshake_line = self.daemon_process.wait_for(
                lambda line: "accepted secure_channel_hello" in line,
                self.args.process_timeout,
                "handshake completion",
            )
            app_handshake_line = self.app_process.wait_for(
                lambda line: "secure channel established" in line,
                self.args.process_timeout,
                "handshake completion",
            )

            daemon_channel_id = self.daemon_process.wait_for_channel_id(
                self.args.process_timeout
            )
            app_channel_id = self.app_process.wait_for_channel_id(self.args.process_timeout)

            if "has_session_keys=true" not in daemon_handshake_line:
                raise SmokeTestError(
                    "daemon handshake did not report derived session keys"
                )
            if "has_session_keys=true" not in app_handshake_line:
                raise SmokeTestError("app handshake did not report derived session keys")
            if daemon_channel_id != app_channel_id:
                raise SmokeTestError(
                    f"channel_id mismatch: daemon={daemon_channel_id} app={app_channel_id}"
                )

            self.app_process.wait_for(
                lambda line: "sent encrypted relay_envelope" in line,
                self.args.process_timeout,
                "encrypted envelope send",
            )
            daemon_decrypt_line = self.daemon_process.wait_for(
                lambda line: "decrypted relay_envelope" in line,
                self.args.process_timeout,
                "encrypted envelope decrypt",
            )
            self.daemon_process.wait_for(
                lambda line: "sent encrypted relay_envelope" in line,
                self.args.process_timeout,
                "encrypted envelope reply",
            )
            app_decrypt_line = self.app_process.wait_for(
                lambda line: "decrypted relay_envelope" in line,
                self.args.process_timeout,
                "encrypted reply decrypt",
            )
            replay_line = self.daemon_process.wait_for(
                lambda line: "rejected replayed relay_envelope" in line,
                self.args.process_timeout,
                "replay protection",
            )

            if '"hello relay"' not in daemon_decrypt_line:
                raise SmokeTestError(
                    "daemon did not log the decrypted app payload as expected"
                )
            if '"daemon received hello relay"' not in app_decrypt_line:
                raise SmokeTestError(
                    "app did not log the decrypted daemon reply as expected"
                )
            if "SEQ_REPLAY" not in replay_line:
                raise SmokeTestError("daemon replay rejection did not mention SEQ_REPLAY")

            print(f"[result] daemon channel_id={daemon_channel_id}")
            print(f"[result] app channel_id={app_channel_id}")
            print("[result] daemon and app derived real session keys")
            print("[result] app -> daemon encrypted envelope decrypted successfully")
            print("[result] daemon -> app encrypted envelope decrypted successfully")
            print("[result] replay protection triggered as expected")
            print("[result] relay handshake smoke test completed successfully")
        finally:
            if self.app_process is not None:
                self.app_process.stop()
            if self.daemon_process is not None:
                self.daemon_process.stop()


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--relay-http",
        default="http://127.0.0.1:8787",
        help="base URL for the local relay Worker dev server",
    )
    parser.add_argument("--device-id", default="dev_local_1")
    parser.add_argument("--device-name", default="local daemon")
    parser.add_argument("--app-installation-id", default="app_local_1")
    parser.add_argument("--app-name", default="local app")
    parser.add_argument("--http-timeout", type=float, default=10.0)
    parser.add_argument(
        "--process-timeout",
        type=float,
        default=90.0,
        help="how long to wait for each Rust smoke client to finish the handshake",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    tester = RelayE2ESmokeTester(args)
    try:
        tester.run()
    except SmokeTestError as exc:
        print(f"[error] {exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("[error] interrupted", file=sys.stderr)
        return 130
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
