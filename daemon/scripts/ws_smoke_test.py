#!/usr/bin/env python3
"""Small stdlib-only WebSocket smoke test for the agentchat daemon.

Run from `daemon/` while the daemon is listening on ws://127.0.0.1:9390:

    python3 scripts/ws_smoke_test.py

The script exercises:
- `list_skills`
- `get_skill`
- `create_session`
- `prompt`
- `distill_session`

It also verifies that the daemon writes a transcript under `.agentchat/sessions/`.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import secrets
import socket
import struct
import sys
import time
from pathlib import Path
from typing import Any
from urllib.parse import urlparse

GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


class SmokeTestError(RuntimeError):
    pass


class SimpleWebSocketClient:
    def __init__(self, url: str, timeout: float) -> None:
        self.url = urlparse(url)
        self.timeout = timeout
        self.socket: socket.socket | None = None
        self.reader = None

    def connect(self) -> None:
        if self.url.scheme != "ws":
            raise SmokeTestError("only ws:// URLs are supported by this script")

        host = self.url.hostname or "127.0.0.1"
        port = self.url.port or 80
        path = self.url.path or "/"
        if self.url.query:
            path = f"{path}?{self.url.query}"

        self.socket = socket.create_connection((host, port), timeout=self.timeout)
        self.socket.settimeout(self.timeout)
        self.reader = self.socket.makefile("rb")

        key = base64.b64encode(secrets.token_bytes(16)).decode("ascii")
        request = (
            f"GET {path} HTTP/1.1\r\n"
            f"Host: {host}:{port}\r\n"
            "Upgrade: websocket\r\n"
            "Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {key}\r\n"
            "Sec-WebSocket-Version: 13\r\n"
            "\r\n"
        )
        self.socket.sendall(request.encode("ascii"))

        status_line = self._read_http_line()
        if not status_line.startswith("HTTP/1.1 101"):
            raise SmokeTestError(f"websocket handshake failed: {status_line}")

        headers = self._read_http_headers()
        accept = headers.get("sec-websocket-accept")
        expected = base64.b64encode(
            hashlib.sha1((key + GUID).encode("ascii")).digest()
        ).decode("ascii")
        if accept != expected:
            raise SmokeTestError("invalid websocket handshake response")

    def close(self) -> None:
        if self.socket is not None:
            try:
                self._send_frame(0x8, b"")
            except (OSError, SmokeTestError):
                pass

        if self.reader is not None:
            self.reader.close()
        if self.socket is not None:
            self.socket.close()

    def send_json(self, payload: dict[str, Any]) -> None:
        text = json.dumps(payload, separators=(",", ":"))
        print(f"> {text}")
        self._send_frame(0x1, text.encode("utf-8"))

    def receive_json(self, timeout: float) -> dict[str, Any]:
        text = self.receive_text(timeout)
        print(f"< {text}")
        try:
            message = json.loads(text)
        except json.JSONDecodeError as exc:
            raise SmokeTestError(f"received invalid JSON: {text}") from exc
        if not isinstance(message, dict):
            raise SmokeTestError(f"received non-object JSON: {message!r}")
        return message

    def receive_text(self, timeout: float) -> str:
        if self.socket is None:
            raise SmokeTestError("websocket is not connected")
        self.socket.settimeout(timeout)

        while True:
            header = self._read_exact(2)
            first, second = header[0], header[1]
            opcode = first & 0x0F
            masked = (second & 0x80) != 0
            length = second & 0x7F

            if length == 126:
                length = struct.unpack("!H", self._read_exact(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", self._read_exact(8))[0]

            mask = self._read_exact(4) if masked else b""
            payload = self._read_exact(length)
            if masked:
                payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))

            if opcode == 0x1:
                return payload.decode("utf-8")
            if opcode == 0x8:
                raise SmokeTestError("websocket connection closed by server")
            if opcode == 0x9:
                self._send_frame(0xA, payload)
                continue
            if opcode == 0xA:
                continue

            raise SmokeTestError(f"unsupported websocket opcode: {opcode}")

    def _read_http_line(self) -> str:
        if self.reader is None:
            raise SmokeTestError("websocket reader is not available")
        line = self.reader.readline()
        if not line:
            raise SmokeTestError("unexpected EOF during websocket handshake")
        return line.decode("ascii", "replace").rstrip("\r\n")

    def _read_http_headers(self) -> dict[str, str]:
        headers: dict[str, str] = {}
        while True:
            line = self._read_http_line()
            if not line:
                return headers
            if ":" not in line:
                raise SmokeTestError(f"invalid websocket header line: {line}")
            name, value = line.split(":", 1)
            headers[name.strip().lower()] = value.strip()

    def _send_frame(self, opcode: int, payload: bytes) -> None:
        if self.socket is None:
            raise SmokeTestError("websocket is not connected")

        first_byte = 0x80 | (opcode & 0x0F)
        header = bytearray([first_byte])
        length = len(payload)
        if length < 126:
            header.append(0x80 | length)
        elif length < (1 << 16):
            header.append(0x80 | 126)
            header.extend(struct.pack("!H", length))
        else:
            header.append(0x80 | 127)
            header.extend(struct.pack("!Q", length))

        mask = secrets.token_bytes(4)
        header.extend(mask)
        masked_payload = bytes(byte ^ mask[index % 4] for index, byte in enumerate(payload))
        self.socket.sendall(bytes(header) + masked_payload)

    def _read_exact(self, size: int) -> bytes:
        if self.reader is None:
            raise SmokeTestError("websocket reader is not available")
        data = self.reader.read(size)
        if data is None or len(data) != size:
            raise SmokeTestError("unexpected EOF while reading websocket frame")
        return data


class SmokeTester:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.project_root = Path(__file__).resolve().parent.parent
        self.skills_dir = self.project_root / ".agentchat" / "skills"
        self.sessions_dir = self.project_root / ".agentchat" / "sessions"
        self.seed_name = "_smoke_test_seed.md"
        self.seed_path = self.skills_dir / self.seed_name
        self.seed_content = "# Smoke Test Seed\n- This file is created temporarily by scripts/ws_smoke_test.py.\n"
        self.ws = SimpleWebSocketClient(args.url, args.connect_timeout)

    def run(self) -> None:
        self._write_seed_skill()
        try:
            self.ws.connect()
            try:
                skills_before = self._list_skills()
                self._assert_skill_present(skills_before, self.seed_name)
                self._get_skill(self.seed_name, expected_content=self.seed_content)

                session_id = self._create_session()
                self._run_prompt(session_id)
                self._wait_for_transcript(session_id)
                self._distill_session(session_id)

                skills_after = self._list_skills()
                self._assert_skill_present(skills_after, self.seed_name)

                other_skill = next((skill for skill in skills_after if skill != self.seed_name), None)
                if other_skill is not None:
                    self._get_skill(other_skill)
                else:
                    print("! distillation completed, but no additional skill files are visible yet")

                print("Smoke test completed successfully.")
            finally:
                self.ws.close()
        finally:
            self._cleanup_seed_skill()

    def _create_session(self) -> str:
        self.ws.send_json(
            {"type": "create_session", "working_dir": self.args.working_dir}
        )
        message = self._wait_for(
            lambda event: event.get("type") == "session_created",
            self.args.event_timeout,
            "session_created",
        )
        session_id = message.get("session_id")
        if not isinstance(session_id, str) or not session_id:
            raise SmokeTestError(f"invalid session_created payload: {message}")
        return session_id

    def _run_prompt(self, session_id: str) -> None:
        self.ws.send_json(
            {
                "type": "prompt",
                "session_id": session_id,
                "content": self.args.prompt,
            }
        )

        saw_turn_end = False
        saw_stream_event = False
        deadline = time.monotonic() + self.args.prompt_timeout
        while time.monotonic() < deadline:
            event = self.ws.receive_json(deadline - time.monotonic())
            self._raise_if_error(event)
            event_type = event.get("type")
            if event.get("session_id") != session_id:
                continue
            if event_type in {"delta", "tool_update", "plan_update"}:
                saw_stream_event = True
            if event_type == "turn_end":
                saw_turn_end = True
                break

        if not saw_turn_end:
            raise SmokeTestError("prompt did not finish with turn_end")
        if not saw_stream_event:
            print("! prompt completed without delta/tool/plan events")

    def _distill_session(self, session_id: str) -> None:
        self.ws.send_json({"type": "distill_session", "session_id": session_id})

        started = False
        completed = False
        deadline = time.monotonic() + self.args.distill_timeout
        while time.monotonic() < deadline:
            event = self.ws.receive_json(deadline - time.monotonic())
            self._raise_if_error(event)
            if event.get("type") != "distillation_status":
                continue
            if event.get("session_id") != session_id:
                continue

            status = event.get("status")
            message = event.get("message")
            if status == "started":
                started = True
            elif status == "completed":
                completed = True
                print(f"! distillation completed: {message}")
                break
            elif status == "failed":
                raise SmokeTestError(f"distillation failed: {message}")

        if not started:
            raise SmokeTestError("did not receive distillation started status")
        if not completed:
            raise SmokeTestError("did not receive distillation completed status")

    def _list_skills(self) -> list[str]:
        self.ws.send_json({"type": "list_skills"})
        message = self._wait_for(
            lambda event: event.get("type") == "skill_list",
            self.args.event_timeout,
            "skill_list",
        )
        skills = message.get("skills")
        if not isinstance(skills, list):
            raise SmokeTestError(f"invalid skill_list payload: {message}")

        names: list[str] = []
        for item in skills:
            if not isinstance(item, dict) or not isinstance(item.get("name"), str):
                raise SmokeTestError(f"invalid skill entry: {item!r}")
            names.append(item["name"])
        return names

    def _get_skill(self, name: str, expected_content: str | None = None) -> None:
        self.ws.send_json({"type": "get_skill", "name": name})
        message = self._wait_for(
            lambda event: event.get("type") == "skill_content" and event.get("name") == name,
            self.args.event_timeout,
            f"skill_content for {name}",
        )
        content = message.get("content")
        if not isinstance(content, str):
            raise SmokeTestError(f"invalid skill_content payload: {message}")
        if expected_content is not None and content != expected_content:
            raise SmokeTestError(f"skill content mismatch for {name}")

    def _wait_for_transcript(self, session_id: str) -> None:
        transcript_path = self.sessions_dir / f"{session_id}.json"
        deadline = time.monotonic() + self.args.fs_timeout
        while time.monotonic() < deadline:
            if transcript_path.exists():
                try:
                    with transcript_path.open("r", encoding="utf-8") as handle:
                        transcript = json.load(handle)
                except json.JSONDecodeError as exc:
                    raise SmokeTestError(
                        f"transcript exists but is invalid JSON: {transcript_path}"
                    ) from exc
                if transcript.get("session_id") != session_id:
                    raise SmokeTestError(
                        f"transcript session_id mismatch in {transcript_path}"
                    )
                return
            time.sleep(0.1)
        raise SmokeTestError(f"transcript file was not written: {transcript_path}")

    def _wait_for(
        self,
        predicate: Any,
        timeout: float,
        description: str,
    ) -> dict[str, Any]:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            event = self.ws.receive_json(deadline - time.monotonic())
            self._raise_if_error(event)
            if predicate(event):
                return event
        raise SmokeTestError(f"timed out waiting for {description}")

    def _raise_if_error(self, event: dict[str, Any]) -> None:
        if event.get("type") != "error":
            return
        code = event.get("code", "unknown_error")
        message = event.get("message", "")
        raise SmokeTestError(f"daemon returned error {code}: {message}")

    def _assert_skill_present(self, skills: list[str], expected_name: str) -> None:
        if expected_name not in skills:
            raise SmokeTestError(
                f"expected skill {expected_name!r} in skill_list, got {skills!r}"
            )

    def _write_seed_skill(self) -> None:
        self.skills_dir.mkdir(parents=True, exist_ok=True)
        self.seed_path.write_text(self.seed_content, encoding="utf-8")

    def _cleanup_seed_skill(self) -> None:
        try:
            if self.seed_path.exists():
                self.seed_path.unlink()
        except OSError as exc:
            print(f"! failed to remove temporary seed skill: {exc}", file=sys.stderr)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run a small end-to-end WebSocket smoke test against the daemon."
    )
    parser.add_argument(
        "--url",
        default="ws://127.0.0.1:9390",
        help="WebSocket URL for the daemon (default: ws://127.0.0.1:9390)",
    )
    parser.add_argument(
        "--working-dir",
        default=".",
        help="Working directory used for create_session (default: .)",
    )
    parser.add_argument(
        "--prompt",
        default="inspect the repo",
        help="Prompt text sent after create_session",
    )
    parser.add_argument(
        "--connect-timeout",
        type=float,
        default=5.0,
        help="Timeout in seconds for the websocket handshake",
    )
    parser.add_argument(
        "--event-timeout",
        type=float,
        default=10.0,
        help="Timeout in seconds for ordinary request/response messages",
    )
    parser.add_argument(
        "--prompt-timeout",
        type=float,
        default=60.0,
        help="Timeout in seconds for prompt completion",
    )
    parser.add_argument(
        "--distill-timeout",
        type=float,
        default=120.0,
        help="Timeout in seconds for distillation completion",
    )
    parser.add_argument(
        "--fs-timeout",
        type=float,
        default=10.0,
        help="Timeout in seconds while waiting for transcript files to appear",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        SmokeTester(args).run()
    except (OSError, SmokeTestError, json.JSONDecodeError) as exc:
        print(f"Smoke test failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
