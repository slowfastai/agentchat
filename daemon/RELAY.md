# Relay Support and Smoke Tests

This repository currently has two layers of relay support:

- production daemon relay transport integrated into the main `agentchat-daemon` runtime
- smoke / fixture / end-to-end validation tools for local protocol and crypto debugging

The current implementation covers:

- connecting to `/v1/ws`
- receiving `relay_ready`
- app -> daemon `secure_channel_hello`
- daemon -> app `secure_channel_accept`
- real `Ed25519` signatures for hello / accept
- real `X25519 + HKDF-SHA256` session key derivation
- matching `channel_id` derivation on both sides
- app -> daemon encrypted `relay_envelope`
- daemon -> app encrypted `relay_envelope`
- local `seq`-based replay rejection
- decrypted relay `ClientMessage` frames entering the real daemon application protocol path
- daemon `ResponseEvent` values being re-encrypted into `relay_envelope` frames for the app

## Production daemon relay mode

The main daemon binary now supports relay transport.

If both of the following environment variables are set, `agentchat-daemon`
starts in relay mode instead of starting the local direct WebSocket server:

- `AGENTCHAT_RELAY_WS_URL`
- `AGENTCHAT_RELAY_TOKEN`

You must also provide relay identity configuration.

### Development mode

```bash
AGENTCHAT_RELAY_DEV_CRYPTO=true
```

This uses fixed development identities from the repository so the daemon can interoperate
with `relay_smoke_app` and the local Worker setup.

### Explicit identity configuration

```bash
AGENTCHAT_RELAY_IDENTITY_SEED_B64URL=...
AGENTCHAT_RELAY_REMOTE_IDENTITY_PUBLIC_KEY_B64URL=...
```

In relay mode, the daemon-side transport decrypts `ClientMessage` values and forwards them
into the same application protocol handling path used by the direct WebSocket server.

## iPhone app QR flow in relay pairing mode

The Apple client now understands a relay QR payload in addition to direct `ws://` / `wss://` URLs.

If the daemon is running in relay mode and the current app build is using development relay crypto:

```bash
AGENTCHAT_RELAY_DEV_CRYPTO=true
agentchat-daemon --mobile
```

then `--mobile` calls the relay pairing API and prints a QR payload shaped like:

```text
agentchat://connect?relay_url=<wss-or-ws-relay-endpoint>&pairing_ticket=<pairing-ticket>&relay_pairing=claim&relay_crypto=dev
```

The iPhone app will:

1. parse the relay QR payload
2. call `POST /v1/pairing/claim` with the one-time `pairing_ticket` and its app installation id
3. connect to `/v1/ws` with the returned app relay token
4. complete the signed `secure_channel_hello` / `secure_channel_accept` handshake
5. exchange encrypted `relay_envelope` frames carrying the normal daemon app protocol

Notes:
- this removes the previous dependency on `POST /v1/dev/pair` for iPhone relay onboarding
- the current Apple build still uses `relay_crypto=dev`; custom per-device production relay identities are not implemented yet
- unlike direct LAN mode, the phone and Mac do **not** need to share the same Wi-Fi once both can reach the relay

## Public relay helper for iPhone testing

If you have already deployed the relay Worker to a public URL and want the daemon to:

1. bootstrap a daemon relay token
2. connect to the public relay
3. print a QR code for the iPhone app

without hand-writing a `curl` request, use:

```bash
cd /path/to/agentchat
AGENTCHAT_AGENT_ID=opencode \
AGENTCHAT_AGENT_NAME="OpenCode (ACP)" \
AGENTCHAT_AGENT_COMMAND=opencode \
AGENTCHAT_AGENT_ARGS="acp" \
bash daemon/scripts/run_public_relay_mobile.sh \
  --relay-http https://agentchat-relay.example.workers.dev
```

The helper script:

1. calls `POST /v1/dev/bootstrap` on the deployed relay Worker
2. exports `AGENTCHAT_RELAY_WS_URL` and `AGENTCHAT_RELAY_TOKEN` from the JSON response
3. forces `AGENTCHAT_RELAY_DEV_CRYPTO=true`
4. sends a browser-like `User-Agent` to avoid Cloudflare browser-signature blocks on non-browser clients
5. prefetches `POST /v1/pairing/open` itself when launching in mobile mode
6. starts `agentchat-daemon --mobile`

Useful flags:

- `--device-id <id>` to control the daemon device id stored in the relay
- `--device-name <name>` to label the daemon more clearly in relay state
- `--no-mobile -- ...` if you want relay bootstrapping without automatically appending `--mobile`
- `AGENTCHAT_RELAY_USER_AGENT=...` if you need to override the default browser-like `User-Agent`

Important:

- this helper still relies on the relay **dev** bootstrap endpoint, so the deployed Worker must keep `POST /v1/dev/bootstrap` enabled
- if you disable `RELAY_DEV_MODE` for the deployed Worker, this helper will fail and you will need another daemon-token provisioning path
- the current iPhone relay QR flow still requires a development relay-crypto app build

## Recommended local end-to-end flow

Start the local relay Worker first:

```bash
cd relay
npm run dev
```

Then run the basic relay smoke script from another terminal:

```bash
cd daemon
python3 scripts/relay_smoke_e2e.py
```

This script automatically:

1. calls the relay dev bootstrap / pair endpoints
2. starts `relay_smoke_daemon`
3. starts `relay_smoke_app`
4. waits for both sides to complete the real signed handshake
5. verifies that both sides print the same `channel_id`
6. verifies that both sides report `has_session_keys=true`
7. verifies that the app -> daemon encrypted envelope is decrypted successfully
8. verifies that the daemon -> app encrypted envelope is decrypted successfully
9. verifies that replay protection triggers `SEQ_REPLAY`

## Stronger end-to-end validation against the real daemon binary

To validate the full application protocol path through relay, run:

```bash
cd daemon
python3 scripts/relay_main_daemon_e2e.py
```

For CI or one-command local validation, use the wrapper script:

```bash
bash daemon/scripts/relay_ci_main_daemon.sh
```

Or from `relay/`:

```bash
npm run test:e2e:main-daemon
```

This stronger validation uses:

- the local relay Worker
- the real `agentchat-daemon` main binary in relay mode
- `fake_acp_agent` as the backend agent
- an app-side relay client that sends real `ClientMessage` values over encrypted envelopes

The CI-friendly wrapper does the following automatically:

1. runs relay `typecheck` and `test`
2. runs daemon `cargo test` and `cargo clippy`
3. starts the local relay Worker
4. waits for `/healthz`
5. runs the stronger relay main-daemon end-to-end validation

It exercises the real application path:

1. `create_session`
2. `prompt`
3. streamed `delta` / `tool_update`
4. `turn_end`

and also verifies that the fake ACP agent recorded the expected `new_session` and `prompt` events.

## Run the daemon smoke client manually

```bash
cd daemon
AGENTCHAT_RELAY_WS_URL='ws://127.0.0.1:8787/v1/ws' \
AGENTCHAT_RELAY_TOKEN='achdm.dev_local_1.<secret>' \
cargo run -p agentchat-daemon --bin relay_smoke_daemon
```

The daemon-side smoke client will:

1. connect to the relay
2. log `relay_ready`
3. verify the app `secure_channel_hello` signature
4. send a real signed `secure_channel_accept`
5. derive session keys and log the resulting `channel_id`

## Run the app smoke client manually

```bash
cd daemon
AGENTCHAT_RELAY_WS_URL='ws://127.0.0.1:8787/v1/ws' \
AGENTCHAT_RELAY_TOKEN='achapp.dev_local_1.app_local_1.<secret>' \
cargo run -p agentchat-daemon --bin relay_smoke_app
```

The app-side smoke client will:

1. connect to the relay
2. receive `relay_ready`
3. send a real signed `secure_channel_hello`
4. verify the `secure_channel_accept` signature
5. derive session keys and log the resulting `channel_id`

## Implementation locations

- relay protocol helpers: `daemon/protocol/src/relay.rs`
- relay crypto: `daemon/protocol/src/relay_crypto.rs`
- crypto fixture: `daemon/protocol/fixtures/relay/crypto/handshake_v1.json`
- fixture generator: `daemon/bin/src/bin/relay_crypto_fixture.rs`
- Rust relay client: `daemon/core/src/relay_client.rs`
- relay integration tests: `daemon/core/tests/relay_integration.rs`
- shared application protocol handler: `daemon/server/src/app.rs`
- relay transport: `daemon/server/src/relay.rs`
- direct WebSocket transport: `daemon/server/src/ws.rs`
- daemon smoke binary: `daemon/bin/src/bin/relay_smoke_daemon.rs`
- app smoke binary: `daemon/bin/src/bin/relay_smoke_app.rs`
- app protocol smoke binary: `daemon/bin/src/bin/relay_app_protocol_smoke.rs`
- local relay smoke script: `daemon/scripts/relay_smoke_e2e.py`
- main daemon relay app protocol script: `daemon/scripts/relay_main_daemon_e2e.py`
- CI-friendly relay wrapper: `daemon/scripts/relay_ci_main_daemon.sh`
