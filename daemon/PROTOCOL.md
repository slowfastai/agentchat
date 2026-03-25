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
cargo run -p agentchat-daemon
```

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
{"type":"create_session","working_dir":"."}
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
```

What to expect:
- `create_session` returns `session_created`.
- Session-scoped streamed events now carry `event_seq`, which is monotonic within one `session_id`.
- `prompt` streams `delta` / `tool_update` events and ends with `turn_end`.
- `list_sessions` returns currently live daemon sessions.
- `attach_session` returns `session_attached`, then `session_snapshot`, then optional replayed events, then `session_replay_complete`.
- `distill_session` returns `distillation_status` with `started`, then `completed` or `failed`.
- `close_session` returns `session_closed` and removes the live session from the daemon.
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
> {"type":"create_session","working_dir":"."}
< {"type":"session_created","session_id":"session-1","event_seq":1}

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
> {"type":"create_session","working_dir":"."}
< {"type":"session_created","session_id":"session-1","event_seq":1}

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
