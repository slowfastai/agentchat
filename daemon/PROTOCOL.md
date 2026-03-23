# WebSocket Protocol Notes

This document covers the memory-layer WebSocket messages added on top of the existing
session lifecycle (`create_session`, `prompt`, `cancel`). All messages are JSON objects
with a top-level `type` field.

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
{"type":"list_skills"}
{"type":"distill_session","session_id":"<session-id>"}
{"type":"list_skills"}
{"type":"get_skill","name":"<skill-name>.md"}
```

What to expect:
- `create_session` returns `session_created`.
- `prompt` streams `delta` / `tool_update` events and ends with `turn_end`.
- `distill_session` returns `distillation_status` with `started`, then `completed` or `failed`.
- Session transcripts are written under `.agentchat/sessions/`.
- Distilled skills are written under `.agentchat/skills/`.

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
< {"type":"session_created","session_id":"session-1"}

> {"type":"prompt","session_id":"session-1","content":"inspect the repo"}
< {"type":"delta","session_id":"session-1","content":"thinking about the request","delta_type":"thinking"}
< {"type":"tool_update","session_id":"session-1","tool_call_id":"tool-1","title":"Demo Tool","status":"InProgress","content":null}
< {"type":"delta","session_id":"session-1","content":"echo: inspect the repo","delta_type":"text"}
< {"type":"turn_end","session_id":"session-1","stop_reason":"EndTurn"}

> {"type":"list_skills"}
< {"type":"skill_list","skills":[]}

> {"type":"distill_session","session_id":"session-1"}
< {"type":"distillation_status","session_id":"session-1","status":"started","message":"distillation started"}
< {"type":"distillation_status","session_id":"session-1","status":"completed","message":"Updated 2 skills"}

> {"type":"list_skills"}
< {"type":"skill_list","skills":[{"name":"memory-layer.md","path":".agentchat/skills/memory-layer.md","size_bytes":64}]}

> {"type":"get_skill","name":"memory-layer.md"}
< {"type":"skill_content","name":"memory-layer.md","content":"# Memory Layer\n- Persist session transcripts under .agentchat/sessions.\n"}
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
< {"type":"session_created","session_id":"session-1"}

> {"type":"prompt","session_id":"session-1","content":"inspect the repo"}
< {"type":"delta","session_id":"session-1","content":"thinking about the request","delta_type":"thinking"}
< {"type":"tool_update","session_id":"session-1","tool_call_id":"tool-1","title":"Demo Tool","status":"InProgress","content":null}
< {"type":"delta","session_id":"session-1","content":"echo: inspect the repo","delta_type":"text"}
< {"type":"turn_end","session_id":"session-1","stop_reason":"EndTurn"}

> {"type":"distill_session","session_id":"session-1"}
< {"type":"distillation_status","session_id":"session-1","status":"started","message":"distillation started"}
< {"type":"distillation_status","session_id":"session-1","status":"completed","message":"Updated 2 skills"}
```

Tip:
- `wscat` is handy when you just want to paste one JSON message at a time and inspect raw responses.

## `list_skills`

List the Markdown skills currently stored under `.agentchat/skills/`.

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
- `name` may be passed with or without `.md`; the daemon normalizes it to a single file in `.agentchat/skills/`.
- Path traversal is rejected.

## `distill_session`

Ask the daemon to turn a completed session transcript into reusable Markdown skills.
The daemon loads the transcript, runs an internal agent session, parses generated skill
blocks, and writes them into `.agentchat/skills/`.

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
