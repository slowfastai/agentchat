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
  │     └─> agentchat-core      AgentManager, AcpAgent, DaemonClient
  │           └─> agentchat-protocol   Shared types (ResponseEvent, ClientMessage, AgentConfig)
  └─> agentchat-core
  └─> agentchat-protocol
```

External dependency: `agent-client-protocol` crate (ACP SDK) — used by `core` and `server`.

### Key abstractions

- **ACP (Agent Client Protocol)** — JSON-RPC 2.0 over stdio standard for editor↔agent communication. The daemon speaks ACP to agent subprocesses.
- **`AcpAgent`** (`core/src/acp_client.rs`) — Wraps an ACP `ClientSideConnection`. Spawns agent subprocess, runs initialize handshake, manages sessions (`new_session`, `prompt`, `cancel`), streams updates via `mpsc` channel.
- **`DaemonClient`** (`core/src/capabilities.rs`) — Implements ACP `Client` trait. Handles agent→daemon requests: file read/write (scoped to project root), terminal command execution, permission auto-approval (M0).
- **`AgentManager`** (`core/src/agent_manager.rs`) — Registry of `AcpAgent` instances keyed by agent ID. Routes sessions to agents.
- **`WebSocketServer`** (`server/src/ws.rs`) — Accepts iOS WebSocket connections, translates `ClientMessage` to ACP calls, streams `SessionNotification` back as `ResponseEvent` frames.
- **Protocol types** (`protocol/src/lib.rs`) — `ResponseEvent` (daemon→iOS), `ClientMessage` (iOS→daemon), `AgentConfig`, `DeltaType`.

### Design patterns

- **Single-threaded runtime**: Uses `tokio::task::LocalSet` with `current_thread` flavor. Required because ACP SDK's `Client` trait is `!Send` (`async_trait(?Send)`).
- **ACP-first**: All agent communication goes through the ACP protocol. Any agent in the ACP registry can be used by changing `AgentConfig.command`.
- **Process isolation**: Each agent is a separate subprocess communicating via stdio JSON-RPC.
- **Bidirectional RPC**: The daemon is both a client (sends prompts) and a server (handles agent requests for file access, terminal, permissions).
