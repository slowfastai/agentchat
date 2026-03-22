# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

All Rust code lives under `daemon/`. Run Cargo commands from that directory:

```bash
cd daemon
cargo build            # build all crates
cargo test             # run all tests
cargo test -p agentchat-core  # test a single crate
cargo run              # run the daemon binary (agentchat-daemon)
cargo clippy           # lint
cargo fmt              # format
```

## Architecture

This is a Rust workspace (`daemon/Cargo.toml`) implementing a daemon that bridges an iOS app (via WebSocket) to AI coding agents (via CLI subprocesses). The daemon manages agent lifecycles, sessions, and streams responses back to the client.

### Crate dependency graph

```
agentchat-daemon (bin)
  └─> agentchat-server          WebSocket listener (port 9390)
        └─> agentchat-core      AgentManager, EventBus, SessionStore
              └─> agentchat-protocol   AgentAdapter trait, all shared types
  └─> agentchat-adapter-claude-code    Claude Code CLI adapter
        └─> agentchat-protocol
```

### Key abstractions

- **`AgentAdapter` trait** (`protocol/src/lib.rs`) — The central extension point. Every agent backend (Claude Code, future others) implements this async trait. Defines `init`, `create_session`, `send_prompt`, `abort`, `health_check`, `shutdown`, and session management methods.
- **`AgentManager`** (`core/src/agent_manager.rs`) — Registry of adapters keyed by agent ID. Delegates all operations to the appropriate adapter.
- **`EventBus<T>`** (`core/src/event_bus.rs`) — Generic tokio broadcast-based pub/sub.
- **`SessionStore`** (`core/src/session_store.rs`) — In-memory session storage (planned SQLite migration).
- **`ClaudeCodeAdapter`** (`adapters/claude-code/src/lib.rs`) — Spawns `claude` CLI as a subprocess. Streams JSON output via `--output-format stream-json`. Tracks child processes per session for abort/shutdown.
- **`WebSocketServer`** (`server/src/ws.rs`) — Stub; will accept connections and bridge to AgentManager.

### Design patterns

- **Async-first**: All I/O uses tokio. Responses stream via `mpsc::Sender<ResponseEvent>`.
- **Trait-based adapters with capability declarations**: Adapters declare supported features (streaming, abort, resume, etc.) via `AdapterCapabilities`, letting the server adapt its behavior per-agent.
- **Process isolation**: Each agent session is a separate CLI subprocess.
