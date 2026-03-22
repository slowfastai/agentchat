# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace rooted at `daemon/`. The workspace manifest is `daemon/Cargo.toml`; shared dependencies are declared there. Crates are split by responsibility:

- `daemon/protocol`: shared types and the `AgentAdapter` trait.
- `daemon/core`: runtime coordination such as `agent_manager`, `event_bus`, and `session_store`.
- `daemon/server`: transport layer, currently WebSocket code in `src/ws.rs`.
- `daemon/adapters/claude-code`: adapter implementation for a specific agent backend.
- `daemon/bin`: the `agentchat-daemon` entrypoint.

Add new adapters under `daemon/adapters/<adapter-name>/`. Keep crate boundaries narrow and move shared contracts into `protocol` instead of duplicating types.

## Build, Test, and Development Commands

Run commands from `daemon/`:

- `cargo build`: compile the full workspace.
- `cargo run -p agentchat-daemon`: start the daemon locally on the default port.
- `cargo test`: run unit tests and doc-tests across all crates.
- `cargo clippy --all-targets --all-features`: catch lint issues before review.
- `cargo fmt`: apply standard Rust formatting.

`cargo fmt --check` currently reports formatting drift, so run `cargo fmt` before opening a PR.

## Coding Style & Naming Conventions

Use Rust 2021 idioms and default `rustfmt` output with 4-space indentation. Follow standard naming: `snake_case` for modules/functions, `CamelCase` for types and enums, and `SCREAMING_SNAKE_CASE` for constants such as `DEFAULT_PORT`. Keep public APIs documented when they define cross-crate contracts, especially in `protocol`.

## Testing Guidelines

The workspace builds cleanly, but there are no meaningful tests yet. Add unit tests alongside the code they cover using `#[cfg(test)]` modules or crate-level tests. Prefer focused tests per crate, for example around session lifecycle, adapter error handling, and protocol serialization. Run `cargo test` before every commit.

## Commit & Pull Request Guidelines

Recent commits use short, imperative subjects with leading capitals, for example `Add Rust workspace...` and `Rename oversight daemon crates to agentchat`. Keep subjects concise and specific to the change. PRs should include a brief summary, affected crates, validation commands run, and linked issues if applicable. Include logs or screenshots only when transport behavior or external integration changes need proof.
