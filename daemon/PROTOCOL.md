# WebSocket Protocol Notes

This document covers the memory-layer WebSocket messages added on top of the existing
session lifecycle (`create_session`, `prompt`, `cancel`). All messages are JSON objects
with a top-level `type` field.

For the next-step design that keeps the daemon alive across iOS disconnects and adds
session re-attach / replay semantics, see `daemon/RECONNECT.md`.

## Manual Smoke Test

You do not need an iOS app to try these messages. Any WebSocket client works.

Start the daemon in one terminal:

```bash
cd daemon
cargo run -p agentchat-daemon --bin agentchat-daemon
```

If you want to connect the iPhone app directly over LAN, you can also ask the daemon to print a scannable QR code in the terminal:

```bash
cd /path/to/agentchat
AGENTCHAT_AGENT_ID=opencode \
AGENTCHAT_AGENT_NAME="OpenCode (ACP)" \
AGENTCHAT_AGENT_COMMAND=opencode \
AGENTCHAT_AGENT_ARGS="acp" \
cargo run --manifest-path daemon/Cargo.toml -p agentchat-daemon --bin agentchat-daemon -- --mobile
```

Notes:
- The QR encodes `ws://<detected-lan-ip>:9390` by default.
- Override it explicitly with `AGENTCHAT_MOBILE_WS_URL=ws://<your-mac-ip>:9390` if auto-detection picks the wrong interface.
- Your phone and Mac must be on the same Wi-Fi / LAN.

Then connect from another terminal with either tool:

```bash
websocat ws://127.0.0.1:9390
```

```bash
wscat -c ws://127.0.0.1:9390
```

Or run the bundled Python smoke test from `daemon/`:

```bash
python3 scripts/ws_smoke_test.py
```

Paste this sequence interactively:

```json
{"type":"list_agents"}
{"type":"create_session","agent_id":"opencode","working_dir":"."}
```

Copy the returned `session_id`, then send:

```json
{"type":"prompt","session_id":"<session-id>","content":"inspect the repo"}
{"type":"list_sessions"}
{"type":"attach_session","session_id":"<session-id>"}
{"type":"list_skills"}
{"type":"distill_session","session_id":"<session-id>"}
{"type":"list_skills"}
{"type":"get_skill","name":"<skill-name>.md"}
{"type":"close_session","session_id":"<session-id>"}
{"type":"close_thread","thread_id":"<thread-id>"}
```

What to expect:
- `list_agents` returns configured daemon agents and their status.
- `create_session` returns `session_created` and echoes the chosen `agent_id`.
- Session-scoped streamed events now carry `event_seq`, which is monotonic within one `session_id`.
- `prompt` streams `delta` / `tool_update` events and ends with `turn_end`.
- `list_sessions` returns currently live daemon sessions.
- `attach_session` returns `session_attached`, then `session_snapshot`, then optional replayed events, then `session_replay_complete`.
- `distill_session` returns `distillation_status` with `started`, then `completed` or `failed`.
- `close_session` returns `session_closed` and removes the live session from the daemon.
- `close_thread` returns `thread_closed`, removes the live thread from the daemon, and tears down its backing live sessions.
- Session transcripts are written under `.agentchat/sessions/`.
- Session event journals are appended under `.agentchat/sessions/<session_id>.events.jsonl`.
- Distilled skills are written under `.agentchat/skills/shared/` for all agents, or `.agentchat/skills/agents/<agent-id>/` for agent-specific memory.

### Python Smoke Test

`daemon/scripts/ws_smoke_test.py` is a stdlib-only client, so it does not need any extra Python packages.
It creates a temporary seed skill so `list_skills` and `get_skill` can be exercised even in an empty project,
then runs `create_session`, `prompt`, and `distill_session`, and finally checks that the session transcript file exists.

### Example: `websocat`

This is a concrete interactive example. Lines prefixed with `>` are what you send;
lines prefixed with `<` are representative daemon responses.

```text
$ websocat ws://127.0.0.1:9390
> {"type":"list_agents"}
< {"type":"agent_list","agents":[{"agent_id":"opencode","name":"OpenCode (ACP)","kind":"opencode","status":"online","default_working_dir":null,"capabilities":["session","prompt","cancel","distill"]}]}

> {"type":"create_session","agent_id":"opencode","working_dir":"."}
< {"type":"session_created","session_id":"session-1","agent_id":"opencode","event_seq":1}

> {"type":"prompt","session_id":"session-1","content":"inspect the repo"}
< {"type":"delta","session_id":"session-1","event_seq":2,"content":"thinking about the request","delta_type":"thinking"}
< {"type":"tool_update","session_id":"session-1","event_seq":3,"tool_call_id":"tool-1","title":"Demo Tool","status":"InProgress","content":null}
< {"type":"delta","session_id":"session-1","event_seq":4,"content":"echo: inspect the repo","delta_type":"text"}
< {"type":"turn_end","session_id":"session-1","event_seq":5,"stop_reason":"EndTurn"}

> {"type":"list_skills"}
< {"type":"skill_list","skills":[]}

> {"type":"distill_session","session_id":"session-1"}
< {"type":"distillation_status","session_id":"session-1","status":"started","message":"distillation started"}
< {"type":"distillation_status","session_id":"session-1","status":"completed","message":"Updated 2 skills"}

> {"type":"list_skills"}
< {"type":"skill_list","skills":[{"name":"shared/memory-layer.md","path":".agentchat/skills/shared/memory-layer.md","size_bytes":64}]}

> {"type":"get_skill","name":"shared/memory-layer.md"}
< {"type":"skill_content","name":"shared/memory-layer.md","content":"# Memory Layer\n- Persist session transcripts under .agentchat/sessions.\n"}
```

Notes:
- Actual `session_id`, skill names, and file sizes depend on the agent output.
- With a real agent, `prompt` and `distill_session` responses will usually differ from the sample above.
- If `distill_session` fails, look for a `distillation_status` event with `status: "failed"` and inspect the message.

### Example: `wscat`

`wscat` is also interactive, but it prefixes outgoing and incoming frames differently.
This example shows the same flow using its usual output format.

```text
$ wscat -c ws://127.0.0.1:9390
Connected (press CTRL+C to quit)
> {"type":"list_agents"}
< {"type":"agent_list","agents":[{"agent_id":"opencode","name":"OpenCode (ACP)","kind":"opencode","status":"online","default_working_dir":null,"capabilities":["session","prompt","cancel","distill"]}]}

> {"type":"create_session","agent_id":"opencode","working_dir":"."}
< {"type":"session_created","session_id":"session-1","agent_id":"opencode","event_seq":1}

> {"type":"prompt","session_id":"session-1","content":"inspect the repo"}
< {"type":"delta","session_id":"session-1","event_seq":2,"content":"thinking about the request","delta_type":"thinking"}
< {"type":"tool_update","session_id":"session-1","event_seq":3,"tool_call_id":"tool-1","title":"Demo Tool","status":"InProgress","content":null}
< {"type":"delta","session_id":"session-1","event_seq":4,"content":"echo: inspect the repo","delta_type":"text"}
< {"type":"turn_end","session_id":"session-1","event_seq":5,"stop_reason":"EndTurn"}

> {"type":"distill_session","session_id":"session-1"}
< {"type":"distillation_status","session_id":"session-1","status":"started","message":"distillation started"}
< {"type":"distillation_status","session_id":"session-1","status":"completed","message":"Updated 2 skills"}
```

Tip:
- `wscat` is handy when you just want to paste one JSON message at a time and inspect raw responses.

## `list_agents`

List configured daemon agents and their current status.

Request:

```json
{"type":"list_agents"}
```

Success response:

```json
{
  "type": "agent_list",
  "agents": [
    {
      "agent_id": "opencode",
      "name": "OpenCode (ACP)",
      "kind": "opencode",
      "status": "online",
      "default_working_dir": null,
      "capabilities": ["session", "prompt", "cancel", "distill"]
    }
  ]
}
```

Notes:
- Clients should usually call `list_agents` before `create_session`.
- `status` is currently coarse and mainly indicates whether the daemon still sees the agent process as alive.

## `create_session`

Create a new live session for a selected agent.

Request:

```json
{"type":"create_session","agent_id":"opencode","working_dir":"."}
```

Backward-compatible request using the daemon default agent:

```json
{"type":"create_session","working_dir":"."}
```

Success response:

```json
{
  "type": "session_created",
  "session_id": "session-1",
  "agent_id": "opencode",
  "event_seq": 1
}
```

Error responses:

```json
{"type":"error","code":"agent_not_found","message":"no agent with this id"}
```

```json
{"type":"error","code":"agent_unavailable","message":"agent is not online"}
```

## `create_thread`

Create a new live thread for group chat.

Request:

```json
{"type":"create_thread","title":"Review","working_dir":"."}
```

Success response:

```json
{
  "type": "thread_created",
  "thread_id": "thread-1",
  "created_at_ms": 1774257600000
}
```

## `list_threads`

List live daemon threads.

Request:

```json
{"type":"list_threads"}
```

Success response:

```json
{
  "type": "thread_list",
  "threads": [
    {
      "thread_id": "thread-1",
      "title": "Review",
      "working_dir": ".",
      "created_at_ms": 1774257600000,
      "state": "idle",
      "participant_count": 3,
      "last_thread_seq": 5
    }
  ]
}
```

## `attach_thread`

Attach the current connection to an existing thread.

Request without replay:

```json
{"type":"attach_thread","thread_id":"thread-1"}
```

Request with replay cursor:

```json
{"type":"attach_thread","thread_id":"thread-1","after_seq":3}
```

Success responses:

```json
{"type":"thread_attached","thread_id":"thread-1"}
```

```json
{
  "type": "thread_snapshot",
  "snapshot": {
    "thread_id": "thread-1",
    "title": "Review",
    "working_dir": ".",
    "created_at_ms": 1774257600000,
    "last_thread_seq": 5,
    "participants": [
      {
        "participant_id": "participant-user",
        "kind": "human",
        "display_name": "You",
        "agent_id": null,
        "session_id": null,
        "state": "idle"
      },
      {
        "participant_id": "participant-1",
        "kind": "agent",
        "display_name": "Pi",
        "agent_id": "pi",
        "session_id": "session-1",
        "state": "idle"
      }
    ]
  }
}
```

If `after_seq` is provided and older than the current tail, the daemon replays all thread events
with `thread_seq > after_seq`, then sends:

```json
{
  "type": "thread_replay_complete",
  "thread_id": "thread-1",
  "last_thread_seq": 5
}
```

Error response when `after_seq` is ahead of the daemon tail:

```json
{
  "type": "error",
  "code": "thread_replay_after_seq_ahead_of_tail",
  "message": "requested after_seq 999 is ahead of current thread tail 5"
}
```

## `add_thread_participant`

Add an agent-backed participant to an existing thread. The daemon creates a backing live session automatically.

Request:

```json
{"type":"add_thread_participant","thread_id":"thread-1","agent_id":"pi"}
```

Success response:

```json
{
  "type": "thread_participant_added",
  "thread_id": "thread-1",
  "thread_seq": 1,
  "participant": {
    "participant_id": "participant-1",
    "kind": "agent",
    "display_name": "Pi",
    "agent_id": "pi",
    "session_id": "session-1",
    "state": "idle"
  }
}
```

## `close_thread`

Close and remove one live thread from the daemon.

Request:

```json
{"type":"close_thread","thread_id":"thread-1"}
```

Success response:

```json
{"type":"thread_closed","thread_id":"thread-1"}
```

Notes:
- `close_thread` is currently **idle-only**. If any backing agent session is still running a prompt, the daemon rejects the request.
- Closing a thread also removes its live backing sessions from the daemon.
- Thread and session journal files are retained on disk for now; this is a live close, not a permanent wipe.

Busy-thread error:

```json
{
  "type": "error",
  "code": "thread_busy",
  "message": "cannot close a thread while agent work is in progress"
}
```

## `send_thread_message`

Record one user message in the thread and fan it out to one or more agent participants.

Request to broadcast to all agent participants:

```json
{"type":"send_thread_message","thread_id":"thread-1","content":"review this diff"}
```

Request targeting a subset of participants:

```json
{"type":"send_thread_message","thread_id":"thread-1","content":"only beta","target_participant_ids":["participant-2"]}
```

Success response sequence:

```json
{
  "type": "thread_message",
  "thread_id": "thread-1",
  "thread_seq": 1,
  "message_id": "message-1",
  "sender": {
    "kind": "human",
    "participant_id": "participant-user",
    "display_name": "You"
  },
  "content": "review this diff",
  "target_participant_ids": ["participant-1", "participant-2"]
}
```

Then the daemon emits thread-scoped agent events such as:

```json
{
  "type": "thread_agent_delta",
  "thread_id": "thread-1",
  "thread_seq": 2,
  "participant_id": "participant-1",
  "agent_id": "pi",
  "session_id": "session-1",
  "session_event_seq": 4,
  "content": "echo: review this diff",
  "delta_type": "text"
}
```

```json
{
  "type": "thread_agent_turn_end",
  "thread_id": "thread-1",
  "thread_seq": 5,
  "participant_id": "participant-1",
  "agent_id": "pi",
  "session_id": "session-1",
  "session_event_seq": 7,
  "stop_reason": "EndTurn"
}
```

Notes:
- The daemon still emits session-scoped events for the backing sessions.
- Thread events are the recommended stream for group chat UI.
- Thread-scoped timeline events are appended under `.agentchat/threads/<thread_id>.events.jsonl`.
- `attach_thread { after_seq }` is the recommended reconnect path for group chat UI.

## `list_sessions`

List live sessions currently owned by the daemon.

Request:

```json
{"type":"list_sessions"}
```

Success response:

```json
{
  "type": "session_list",
  "sessions": [
    {
      "session_id": "session-1",
      "agent_id": "fake",
      "working_dir": ".",
      "created_at_ms": 1774257600000,
      "state": "idle",
      "last_event_seq": 5,
      "last_stop_reason": "EndTurn"
    }
  ]
}
```

Notes:
- Only live daemon sessions are returned.
- Session discovery after daemon restart will be expanded further in the reconnect work described in `daemon/RECONNECT.md`.

## `attach_session`

Attach the current client connection to an existing live session.

Request without replay:

```json
{"type":"attach_session","session_id":"session-1"}
```

Request with replay cursor:

```json
{"type":"attach_session","session_id":"session-1","after_seq":3}
```

Success responses:

```json
{"type":"session_attached","session_id":"session-1"}
```

```json
{
  "type": "session_snapshot",
  "snapshot": {
    "session_id": "session-1",
    "agent_id": "fake",
    "working_dir": ".",
    "created_at_ms": 1774257600000,
    "state": "idle",
    "last_event_seq": 5,
    "last_stop_reason": "EndTurn",
    "last_error": null
  }
}
```

If `after_seq` is provided and older than the current tail, the daemon replays all session events
with `event_seq > after_seq`, then sends:

```json
{
  "type": "session_replay_complete",
  "session_id": "session-1",
  "last_event_seq": 5
}
```

Error response when the session does not exist:

```json
{
  "type": "error",
  "session_id": "session-1",
  "code": "session_not_found",
  "message": "no live session with this id"
}
```

Error response when `after_seq` is ahead of the daemon tail:

```json
{
  "type": "error",
  "session_id": "session-1",
  "code": "replay_after_seq_ahead_of_tail",
  "message": "requested after_seq 999 is ahead of current tail 5"
}
```

## `close_session`

Close a live session and remove it from the daemon.

Request:

```json
{"type":"close_session","session_id":"session-1"}
```

Success response:

```json
{"type":"session_closed","session_id":"session-1"}
```

Possible error response:

```json
{
  "type": "error",
  "session_id": "session-1",
  "code": "session_busy",
  "message": "cannot close a session while a prompt is in progress"
}
```

## `list_skills`

List the Markdown skills currently stored under `.agentchat/skills/`, including shared subdirectories such as `.agentchat/skills/shared/` and agent namespaces such as `.agentchat/skills/agents/<agent-id>/`.

Request:

```json
{"type":"list_skills"}
```

Success response:

```json
{
  "type": "skill_list",
  "skills": [
    {
      "name": "testing.md",
      "path": ".agentchat/skills/testing.md",
      "size_bytes": 128
    }
  ]
}
```

Notes:
- Returns an empty `skills` array when no skills exist.
- The `path` is project-relative and can be read by the agent through normal file access.

## `get_skill`

Read the full Markdown content of a single stored skill.

Request:

```json
{"type":"get_skill","name":"testing.md"}
```

Success response:

```json
{
  "type": "skill_content",
  "name": "testing.md",
  "content": "# Testing\n- Use the fake ACP agent in websocket tests.\n"
}
```

Error response:

```json
{
  "type": "error",
  "session_id": null,
  "code": "skill_not_found",
  "message": "failed to read skill ..."
}
```

Notes:
- `name` may be passed with or without `.md`; the daemon normalizes it to a single file in `.agentchat/skills/` and accepts nested names such as `shared/testing-notes.md` or `agents/opencode/testing-notes.md`.
- Path traversal is rejected.

## `distill_session`

Ask the daemon to turn a completed session transcript into reusable Markdown skills.
The daemon loads the transcript, runs an internal agent session, parses generated skill
blocks, and writes them into `.agentchat/skills/shared/` or `.agentchat/skills/agents/<agent-id>/`.

Request:

```json
{"type":"distill_session","session_id":"session-1"}
```

Progress response:

```json
{
  "type": "distillation_status",
  "session_id": "session-1",
  "status": "started",
  "message": "distillation started"
}
```

Completion response:

```json
{
  "type": "distillation_status",
  "session_id": "session-1",
  "status": "completed",
  "message": "Updated 2 skills"
}
```

Failure response:

```json
{
  "type": "distillation_status",
  "session_id": "session-1",
  "status": "failed",
  "message": "failed to read transcript session-1: ..."
}
```

Notes:
- Distillation uses an internal ACP session and does not stream those agent updates back to the iOS client.
- The target session can be loaded from memory or from `.agentchat/sessions/{session_id}.json`.
- Resulting skills are ordinary Markdown files and are immediately available through `list_skills` and `get_skill`.
- Use `shared/<topic-name>` for skills every agent should read, and `agents/<agent-id>/<topic-name>` for memory that should only be injected for that agent.
