# Daemon Persistence and Session Resume Design

This document describes the next protocol and runtime step after the current M0
single-connection flow.

The target user experience is:

- `agentchat-daemon` starts once and keeps running until the Mac or server shuts down,
  or the user explicitly stops the service
- iOS can disconnect and reconnect without destroying the active coding session
- an in-flight prompt may continue running while the app is offline
- when iOS reconnects, it can re-attach to the existing session, replay missed events,
  and continue streaming live updates

This is a design document for the next iteration. It does not describe the current
implementation exactly.

## Current behavior and gap

Today the daemon behaves like a connection-scoped session host instead of a
long-lived service:

- WebSocket connection handling is centered around a per-connection
  `AppProtocolSession`
- when the client disconnects, `session.shutdown()` is called
- shutdown cancels the active prompt, flushes transcripts, removes created sessions,
  and clears the session -> agent routing state

That behavior is fine for M0 smoke tests, but it breaks the desired product model:

- a dropped phone connection kills the session
- there is no way to re-attach to an existing session
- there is no replay cursor for missed streaming events
- daemon lifetime is implicitly tied to client activity instead of host lifetime

There is already a useful persistence base:

- `SessionStore` records user prompts, raw agent notifications, and `turn_end`
- session transcripts are flushed to `.agentchat/sessions/<session_id>.json`

However, transcript persistence alone is not enough for seamless resume, because the
transcript is not the same thing as the app-facing event stream. The iOS client needs
ordered daemon events with stable sequence numbers, not just a best-effort rebuild from
raw ACP notifications.

## Product goals

### Required

- daemon is long-lived and survives iOS disconnects
- session lifetime is independent from transport lifetime
- disconnect does not cancel an active prompt by default
- iOS can list existing sessions after reconnecting
- iOS can attach to a session and continue from the latest state
- iOS can request replay of missed events using a sequence cursor
- replayed events preserve the same ordering and payloads as the original live stream

### Nice to have

- multiple app clients can observe the same session
- iOS can restore UI state from a compact snapshot before replay completes
- session summaries survive daemon restarts
- launchd keeps the daemon running on macOS without a visible terminal

### Explicit non-goals for this phase

- cross-device collaborative editing semantics
- exact token-level replay of agent-internal reasoning after daemon crash
- resumable execution after the agent subprocess itself exits unexpectedly
- protocol compatibility with every old client version without negotiation

## Target model

The core shift is:

- WebSocket connection == transport attachment
- session == daemon-owned runtime object
- agent subprocess == execution backend for one or more daemon-owned sessions

The iOS app should be able to come and go without owning the session.

```text
current M0

WebSocket connection
  -> AppProtocolSession
    -> session lifetime
    -> active prompt lifetime

proposed M1

WebSocket connection
  -> attachment to daemon-owned session

Daemon process
  -> SessionRegistry
    -> LiveSession(session-1)
    -> LiveSession(session-2)
```

## Proposed runtime architecture

### 1. Daemon process is always-on

`agentchat-daemon` should be treated as a background service:

- started manually during development
- started by `launchd` on macOS in normal usage
- stopped only by explicit user action, logout, reboot, or host shutdown

Service ownership moves from "who is connected right now" to "the host machine is
running the daemon".

### 2. Global SessionRegistry

Introduce a daemon-global registry that owns all live sessions.

Each `LiveSession` stores at least:

- `session_id`
- `agent_id`
- `working_dir`
- `created_at_ms`
- `state`: `idle | prompting | distilling | failed`
- `last_event_seq`
- `last_stop_reason`
- `last_error`
- `attachments`: zero or more subscribed transport sinks
- `event_log`: append-only journal of app-facing events

This registry replaces the current connection-owned `created_sessions` bookkeeping.

### 3. Session event log distinct from transcript

Keep both:

- transcript store: durable session history for distillation, inspection, and recovery
- event log: ordered app-facing stream for replay and reconnect

Why both are needed:

- transcript stores semantic history: prompts, ACP notifications, `turn_end`
- event log stores delivery history: `delta`, `tool_update`, `error`, replay cursor order

Recommended storage model:

- in-memory ring buffer of recent events for fast replay
- append-to-disk JSONL journal under `.agentchat/sessions/<session_id>.events.jsonl`
- transcript snapshot remains in `.agentchat/sessions/<session_id>.json`

### 4. Attachments instead of ownership

A WebSocket connection does not own a session. It only subscribes to it.

Attachment lifecycle:

1. client connects
2. client lists sessions or creates a new one
3. client attaches to a session with an optional replay cursor
4. daemon replays missed events
5. daemon streams new live events to that attachment
6. client disconnects
7. daemon removes the attachment only; the session keeps running

### 5. Prompt execution continues while detached

If a prompt is in progress and the app disconnects:

- daemon keeps the prompt running
- incoming ACP updates are still mapped into app events
- those events are appended to the session event log
- if no attachments exist, the events are only journaled
- when a client re-attaches, missed events are replayed in order

Default rule: transport loss must not imply prompt cancellation.

### 6. Explicit cleanup policy

Because sessions will outlive connections, cleanup must become intentional.

Recommended policy:

- idle sessions remain in memory for a bounded TTL, such as 30 to 120 minutes
- all sessions are persisted to disk before eviction
- users can explicitly close a session
- daemon restart can load lightweight session summaries from disk

## Protocol design

The current protocol is optimized for live streaming only. Resume requires three new
capabilities:

- session discovery
- session attachment
- ordered replay

### A. Add sequence numbers to session-scoped daemon events

Every session-scoped event sent from daemon to iOS should carry a monotonically
increasing `event_seq` that is unique within that `session_id`.

This includes at least:

- `session_created`
- `delta`
- `plan_update`
- `tool_update`
- `turn_end`
- `distillation_status`
- session-scoped `error`

Suggested rule:

- `event_seq` starts at `1` for each new session
- replayed events keep their original `event_seq`
- live events continue from `last_event_seq + 1`

Non-session request/response events such as `skill_list` and `skill_content` do not
need replay cursors.

### B. New client messages

#### `list_sessions`

Returns attachable session summaries known to the daemon.

Request:

```json
{"type":"list_sessions"}
```

Response:

```json
{
  "type": "session_list",
  "sessions": [
    {
      "session_id": "session-1",
      "agent_id": "pi",
      "working_dir": "/Users/lj/Downloads/agentchat",
      "created_at_ms": 1774257600000,
      "state": "prompting",
      "last_event_seq": 42,
      "last_stop_reason": null
    }
  ]
}
```

#### `attach_session`

Attaches the current transport to an existing session and optionally replays events after
`after_seq`.

Request:

```json
{"type":"attach_session","session_id":"session-1","after_seq":17}
```

Rules:

- if `after_seq` is omitted, daemon attaches at the current tail with no replay
- if `after_seq` is smaller than the latest event, daemon replays `(after_seq, last_event_seq]`
- if `after_seq` is greater than the current tail, daemon returns an error
- if the requested replay window has been compacted away, daemon returns a replay gap error

#### `close_session`

Explicitly terminates a daemon-owned session and releases runtime resources.

Request:

```json
{"type":"close_session","session_id":"session-1"}
```

M1 can implement this conservatively:

- cancel in-flight prompt if one exists
- flush transcript and event log
- remove the session from the live registry

### C. New daemon responses

#### `session_attached`

Acknowledges successful attachment and provides the authoritative tail cursor.

```json
{
  "type": "session_attached",
  "session_id": "session-1",
  "state": "prompting",
  "last_event_seq": 42,
  "replay_from": 18
}
```

#### `session_snapshot`

Provides enough state for the iOS UI to rebuild the screen before or during replay.

```json
{
  "type": "session_snapshot",
  "session_id": "session-1",
  "agent_id": "pi",
  "working_dir": "/Users/lj/Downloads/agentchat",
  "state": "prompting",
  "last_event_seq": 42,
  "last_stop_reason": "EndTurn",
  "last_error": null
}
```

The snapshot is intentionally compact. Full transcript rendering remains a separate
feature.

#### `session_replay_complete`

Marks the handoff from replay to live stream.

```json
{
  "type": "session_replay_complete",
  "session_id": "session-1",
  "last_event_seq": 42
}
```

### D. Error codes for reconnect flow

Add explicit, machine-friendly errors:

- `session_not_found`
- `session_not_attachable`
- `replay_after_seq_ahead_of_tail`
- `replay_gap`
- `session_closed`

## Recommended reconnect flow

### Fresh connect with no existing context

```text
1. iOS opens WebSocket
2. iOS sends list_sessions
3. iOS either:
   - creates a new session, or
   - attaches to an existing one
```

### Reconnect to an existing session

```text
1. iOS opens WebSocket
2. iOS restores local tuple: (session_id, last_seen_seq)
3. iOS sends attach_session { session_id, after_seq: last_seen_seq }
4. daemon sends session_attached
5. daemon sends session_snapshot
6. daemon replays all events with seq > last_seen_seq
7. daemon sends session_replay_complete
8. daemon continues live streaming new events
```

### Disconnect during an in-flight prompt

```text
1. iOS sends prompt
2. daemon starts prompt and journals streamed events
3. iOS disconnects unexpectedly
4. daemon keeps prompt running
5. daemon keeps appending events to the session event log
6. iOS reconnects with after_seq from before disconnect
7. daemon replays missed events and continues live streaming
```

## State machines

### Session state machine

```text
idle
  -> prompting     on prompt
  -> distilling    on distill_session
  -> closed        on close_session / eviction

prompting
  -> idle          on turn_end
  -> idle          on prompt_failed
  -> idle          on cancel completion
  -> failed        on agent crash

distilling
  -> idle          on completed
  -> idle          on failed
  -> failed        on agent crash

failed
  -> closed        on close_session / eviction
```

### Attachment state machine

```text
disconnected
  -> connected         on websocket open
  -> attaching         on attach_session
  -> replaying         after session_attached when replay is needed
  -> live              after replay_complete or immediate attach
  -> disconnected      on websocket close
```

### Daemon service state machine

```text
stopped
  -> starting          on launchd/manual start
  -> running           after agent initialization
  -> stopping          on explicit shutdown signal
  -> stopped           after cleanup or host shutdown
```

## Internal refactor plan

### Step 1: separate connection scope from session scope

Refactor `AppProtocolSession` into two roles:

- `AppConnection` or `TransportConnection`: per-WebSocket request parsing and sink management
- `LiveSession` handling owned by a daemon-global `SessionRegistry`

The connection object should stop owning:

- session lifetime
- prompt lifetime
- session -> agent routing cleanup

### Step 2: journal app-facing events

Create a small event journal abstraction:

- append(event)
- replay_after(seq)
- tail_seq()
- snapshot()

All session-scoped daemon events should be produced through this layer so live delivery and
replay use the same payload path.

### Step 3: allow detached execution

When a connection drops:

- unsubscribe its sink from the session
- do not cancel the prompt
- do not remove the session from the registry

### Step 4: add new protocol messages

Implement in order:

1. `list_sessions`
2. `attach_session`
3. `session_snapshot`
4. `session_replay_complete`
5. `close_session`

### Step 5: bounded retention and eviction

Add:

- idle timeout
- in-memory replay window limit
- flush-on-evict behavior
- startup summary loading from disk

## macOS daemon persistence

For normal desktop usage, run the daemon as a `launchd` user service.

Desired properties:

- `RunAtLoad = true`
- `KeepAlive = true`
- logs written to a stable path
- environment configured once instead of per terminal tab

This yields the user-visible behavior we want:

- reboot/login starts the daemon
- closing Terminal does not stop the daemon
- iOS reconnects to the same long-lived service instance

A launchd plist and install helper can be added after the session resume protocol is stable.

## Compatibility and rollout

This change is large enough that it should be rolled out in phases.

### Phase 1

- daemon keeps sessions alive across disconnects
- no replay yet
- iOS can reconnect and continue with future prompts

### Phase 2

- add `event_seq`
- add `list_sessions` and `attach_session`
- add replay from `after_seq`

### Phase 3

- add startup recovery from persisted summaries
- add bounded retention and explicit close semantics
- package daemon as a launchd service on macOS

## Open questions

- Should multiple iOS clients be allowed to send prompts to the same session, or should
  only one attachment be writable at a time?
- How much replay history should stay in memory before falling back to disk?
- Should `session_snapshot` include a compact rendered transcript for faster UI restore?
- Should daemon restart attempt to restore in-flight prompts, or only restore idle sessions?
- Do we want protocol version negotiation before introducing `event_seq` and attach flow?

## Recommended first implementation slice

If we optimize for fastest user-visible progress, the first code change should be:

- stop cancelling prompts and deleting sessions on WebSocket disconnect

Then immediately add:

- daemon-global live session registry
- `list_sessions`
- `attach_session` without replay

That already gets most of the desired product feel:

- daemon keeps running
- iOS can disconnect and reconnect
- users can re-open the same session and continue working

After that, event replay can be added without changing the high-level model again.

## Implementation checklist

This section turns the design into a concrete execution plan. Each milestone should be
small enough to land as one PR or a tightly scoped PR stack.

### Ground rules before coding

- [ ] Lock the product semantics for M1:
  - disconnect does **not** cancel an in-flight prompt by default
  - session lifetime is daemon-owned, not connection-owned
  - one session has at most one active prompt at a time
  - multiple attachments may observe the same session, but write access policy must be explicit
- [ ] Decide whether protocol changes are allowed to be breaking for the current iOS prototype,
      or whether we want explicit protocol versioning before adding `event_seq`
- [ ] Pick initial operational defaults:
  - idle session TTL
  - in-memory replay window size
  - disk journal retention policy
- [ ] Confirm the first delivery slice is:
  - keep sessions alive across disconnects
  - allow re-attach
  - defer full replay until the next milestone

Definition of done:

- [ ] A short decision summary is added to this document or linked from it

### Milestone 1: daemon-owned live sessions, no replay yet

Goal: disconnecting the iOS app must no longer destroy the session or cancel the running
prompt.

Primary files:

- `daemon/server/src/ws.rs`
- `daemon/server/src/app.rs`
- `daemon/server/src/relay.rs`
- `daemon/core/src/session_store.rs`
- `daemon/core/src/agent_manager.rs`
- `daemon/server/tests/ws_e2e.rs`

Recommended new files/modules:

- `daemon/core/src/session_registry.rs`
- `daemon/core/src/live_session.rs`

Checklist:

- [ ] Introduce a daemon-global `SessionRegistry` owned above the transport layer
- [ ] Move live session ownership out of `AppProtocolSession`
- [ ] Refactor `AppProtocolSession` into a connection-scoped request handler only
- [ ] Replace per-connection `created_sessions` cleanup semantics with explicit daemon-owned
      session registration
- [ ] On WebSocket disconnect:
  - [ ] unsubscribe the connection sink
  - [ ] do **not** cancel the prompt
  - [ ] do **not** remove the session from routing state
- [ ] Apply the same detach semantics to relay transport in `daemon/server/src/relay.rs`
- [ ] Keep transcript flushes on `turn_end`
- [ ] Add an explicit daemon shutdown path that still flushes and cleans up live sessions

Tests:

- [ ] Add/adjust `ws_e2e` coverage for: disconnect during active prompt does not cancel prompt
- [ ] Add/adjust `ws_e2e` coverage for: reconnect can still interact with the same session later
- [ ] Add/adjust relay tests if relay transport shares the same app session ownership path

Validation:

- [ ] `cd daemon && cargo test -p agentchat-server`
- [ ] `cd daemon && cargo test`
- [ ] Existing smoke flow still works: `cd daemon && python3 scripts/ws_smoke_test.py`

Exit criteria:

- [ ] A prompt can continue running after the app disconnects
- [ ] Session routing survives disconnect
- [ ] No prompt/session cleanup is triggered merely because a transport closed

### Milestone 2: attachable sessions and basic reconnect

Goal: after reconnect, the client can discover and re-attach to a live session, even before
replay is implemented.

Primary files:

- `daemon/protocol/src/lib.rs`
- `daemon/server/src/app.rs`
- `daemon/server/src/ws.rs`
- `daemon/server/src/relay.rs`
- `daemon/server/tests/ws_e2e.rs`
- `daemon/PROTOCOL.md`

Checklist:

- [ ] Add `ClientMessage::ListSessions`
- [ ] Add `ClientMessage::AttachSession { session_id }`
- [ ] Add `ClientMessage::CloseSession { session_id }`
- [ ] Add `ResponseEvent::SessionList`
- [ ] Add `ResponseEvent::SessionAttached`
- [ ] Add `ResponseEvent::SessionSnapshot`
- [ ] Define a compact session summary shape:
  - [ ] `session_id`
  - [ ] `agent_id`
  - [ ] `working_dir`
  - [ ] `state`
  - [ ] `created_at_ms`
  - [ ] `last_stop_reason`
- [ ] Implement `list_sessions` over the new global registry
- [ ] Implement `attach_session` without replay:
  - [ ] subscribe the current connection to the live session
  - [ ] send `session_attached`
  - [ ] send `session_snapshot`
- [ ] Implement `close_session` with conservative semantics:
  - [ ] cancel active prompt if needed
  - [ ] flush transcript
  - [ ] remove session from registry
- [ ] Update `daemon/PROTOCOL.md` with the new commands and responses

Tests:

- [ ] Add `ws_e2e` test: create session, disconnect, reconnect, list sessions, attach session
- [ ] Add `ws_e2e` test: attached session can accept a new prompt after reconnect
- [ ] Add `ws_e2e` test: close_session removes it from list_sessions

Validation:

- [ ] `cd daemon && cargo test -p agentchat-protocol`
- [ ] `cd daemon && cargo test -p agentchat-server`
- [ ] Manual websocket smoke for: create -> disconnect -> reconnect -> list -> attach

Exit criteria:

- [ ] Reconnect to the same daemon process shows prior live sessions
- [ ] iOS can re-open a session and continue future work without creating a new one

### Milestone 3: app-facing event journal and sequence numbers

Goal: make reconnect resumable, not just attachable.

Primary files:

- `daemon/protocol/src/lib.rs`
- `daemon/server/src/app.rs`
- `daemon/core/src/session_store.rs`
- `daemon/core/src/session_registry.rs`
- `daemon/core/src/live_session.rs`
- `daemon/server/tests/ws_e2e.rs`
- `daemon/PROTOCOL.md`

Recommended new files/modules:

- `daemon/core/src/session_event_log.rs`

Checklist:

- [ ] Introduce session-scoped `event_seq`
- [ ] Decide event representation strategy:
  - [ ] add `event_seq` directly onto session-scoped `ResponseEvent` variants, or
  - [ ] introduce a session event envelope that wraps current response payloads
- [ ] Implement an append-only app-facing event journal abstraction with:
  - [ ] `append`
  - [ ] `tail_seq`
  - [ ] `replay_after`
  - [ ] `snapshot`
- [ ] Ensure all session-scoped outbound events flow through the journal path
- [ ] Keep replay ordering identical to live ordering
- [ ] Persist event log to disk, ideally as JSONL beside the transcript
- [ ] Define and implement replay errors:
  - [ ] `replay_after_seq_ahead_of_tail`
  - [ ] `replay_gap`
  - [ ] `session_not_found`
  - [ ] `session_closed`

Tests:

- [ ] Add `ws_e2e` test: reconnect with `after_seq` replays only missed events
- [ ] Add `ws_e2e` test: replay preserves original ordering and `event_seq`
- [ ] Add `ws_e2e` test: replay gap returns a deterministic error
- [ ] Add persistence test for event journal round-trip

Validation:

- [ ] `cd daemon && cargo test -p agentchat-core`
- [ ] `cd daemon && cargo test -p agentchat-server`
- [ ] Update or add a smoke script that simulates disconnect mid-stream and resume from cursor

Exit criteria:

- [ ] iOS can reconnect with `(session_id, after_seq)` and receive only the missing events
- [ ] Replay and live streaming share the same payload model

### Milestone 4: full resume protocol

Goal: complete the attach + replay handoff with explicit protocol markers.

Primary files:

- `daemon/protocol/src/lib.rs`
- `daemon/server/src/app.rs`
- `daemon/server/tests/ws_e2e.rs`
- `daemon/PROTOCOL.md`

Checklist:

- [ ] Extend `attach_session` to accept `after_seq`
- [ ] Add `ResponseEvent::SessionReplayComplete`
- [ ] Make attach flow deterministic:
  1. [ ] `session_attached`
  2. [ ] `session_snapshot`
  3. [ ] replayed events
  4. [ ] `session_replay_complete`
  5. [ ] live stream continues
- [ ] Define behavior for `after_seq` omission vs exact cursor use
- [ ] Define behavior when the client attaches to an idle vs prompting session
- [ ] Update protocol docs with reconnect examples

Tests:

- [ ] Add `ws_e2e` test covering the full reconnect state machine
- [ ] Add `ws_e2e` test for reconnect during an in-flight prompt
- [ ] Add relay-path test if replay is meant to work identically over relay

Validation:

- [ ] `cd daemon && cargo test`
- [ ] Manual reconnect demo documented in `daemon/PROTOCOL.md`

Exit criteria:

- [ ] The reconnect handshake is stable and documented
- [ ] Client can move from snapshot to replay to live without ambiguous boundaries

### Milestone 5: retention, eviction, and startup recovery

Goal: make the long-lived daemon operationally safe.

Primary files:

- `daemon/core/src/session_registry.rs`
- `daemon/core/src/session_store.rs`
- `daemon/bin/src/main.rs`
- `daemon/server/tests/ws_e2e.rs`
- `daemon/RECONNECT.md`

Checklist:

- [ ] Add idle session TTL eviction
- [ ] Flush transcript and event log before eviction
- [ ] Bound in-memory replay window size
- [ ] Load persisted session summaries on daemon startup
- [ ] Decide whether startup recovery restores only idle sessions or also failed/incomplete ones
- [ ] Add explicit operator logs for session load, eviction, and replay gap conditions

Tests:

- [ ] Add unit tests for eviction policy
- [ ] Add startup recovery tests from persisted summaries/event logs

Validation:

- [ ] `cd daemon && cargo test -p agentchat-core`
- [ ] Manual restart test: stop daemon, restart daemon, verify session summaries are visible

Exit criteria:

- [ ] Long-lived sessions do not leak memory unboundedly
- [ ] Restart behavior is predictable and documented

### Milestone 6: macOS launchd packaging

Goal: daemon stays up even when Terminal is closed.

Primary files:

- new `daemon/launchd/*.plist`
- new install/uninstall helper script(s)
- `daemon/RECONNECT.md`
- top-level usage docs as needed

Checklist:

- [ ] Add a user-level `launchd` plist with `RunAtLoad`
- [ ] Add `KeepAlive`
- [ ] Choose stable stdout/stderr log locations
- [ ] Document environment variable setup for agent config and relay config
- [ ] Add install instructions
- [ ] Add uninstall/disable instructions

Validation:

- [ ] Install daemon as LaunchAgent
- [ ] Log out / close Terminal / re-open and confirm daemon stays available
- [ ] iOS can reconnect without a terminal session being open

Exit criteria:

- [ ] Daemon behaves like a background service on macOS

## Suggested PR breakdown

To keep reviewable units small, use roughly this order:

1. PR 1: disconnect no longer destroys live sessions
2. PR 2: `list_sessions` + `attach_session` without replay
3. PR 3: `close_session` + session summaries/snapshot
4. PR 4: event journal + `event_seq`
5. PR 5: replay protocol completion
6. PR 6: retention/startup recovery
7. PR 7: launchd packaging

## Suggested acceptance demo sequence

When the checklist is complete through Milestone 4, the following demo should work:

- [ ] start daemon once on the Mac
- [ ] create a session from iOS
- [ ] send a long-running prompt
- [ ] background or kill the iOS app while the prompt is still running
- [ ] reopen the app
- [ ] reconnect to the daemon
- [ ] list live sessions
- [ ] attach to the existing session using the last seen sequence cursor
- [ ] receive replayed missed events
- [ ] continue receiving live events
- [ ] send another prompt in the same session

## Tracking note

When implementation starts, each checkbox above should be turned into an issue, PR checklist,
or milestone item. Keep this document as the source-of-truth rollout plan, and update it when
we intentionally change semantics or scope.
