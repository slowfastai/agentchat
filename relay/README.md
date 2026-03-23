# AgentChat Relay Worker

This is a minimal Cloudflare relay skeleton. It currently implements:

- `GET /v1/ws`
- relay token authentication
- `connection_id` allocation
- `relay_ready` delivery
- `from` validation
- routing by `to` for `secure_channel_hello`, `secure_channel_accept`, and `relay_envelope`
- minimal protocol-level errors for:
  - `PEER_OFFLINE`
  - `FORBIDDEN_SENDER`
  - `UNPAIRED_PEER`
  - `INVALID_SCHEMA`

The following are **not** implemented yet:

- the full bootstrap / pairing flow
- real end-to-end encryption inside the Worker
- presence / `peer_upsert` / `peer_remove`
- offline message storage

## Local development

Install dependencies:

```bash
cd relay
npm install
```

Generate Worker types:

```bash
npx wrangler types worker-configuration.d.ts
```

Run checks:

```bash
npm run typecheck
npm test
```

Start the local Worker:

```bash
npm run dev
```

## Current development helper endpoints

To get `/v1/ws`, authentication, and routing working quickly, the Worker currently exposes two
**dev-only** helper endpoints.

### `POST /v1/dev/bootstrap`

Creates a daemon token and initializes the corresponding `DeviceHubDO`.

Request:

```json
{
  "device_id": "dev_local_1",
  "device_name": "local daemon"
}
```

Response:

```json
{
  "device_id": "dev_local_1",
  "relay_token": "achdm.dev_local_1.<secret>",
  "ws_url": "ws://127.0.0.1:8787/v1/ws"
}
```

### `POST /v1/dev/pair`

Registers an app for the given device and returns an app relay token.

Request:

```json
{
  "device_id": "dev_local_1",
  "app_installation_id": "app_local_1",
  "app_name": "local app"
}
```

Response:

```json
{
  "device_id": "dev_local_1",
  "app_installation_id": "app_local_1",
  "peer_id": "app:app_local_1",
  "relay_token": "achapp.dev_local_1.app_local_1.<secret>",
  "ws_url": "ws://127.0.0.1:8787/v1/ws"
}
```

## Manual validation flow

1. Call `/v1/dev/bootstrap` to get a daemon token.
2. Call `/v1/dev/pair` to get an app token.
3. Connect two WebSocket clients to `/v1/ws`.
4. Send `Authorization: Bearer <relay_token>` in the request headers.
5. Both sides should receive `relay_ready` first.
6. The app sends `secure_channel_hello`.
7. The daemon should receive the frame unchanged.
8. The daemon sends `secure_channel_accept`.
9. The app should receive the frame unchanged.

## Project structure

```text
relay/
├── package.json
├── wrangler.jsonc
├── tsconfig.json
├── README.md
└── src/
    ├── index.ts
    ├── device-hub.ts
    ├── pairing-index.ts
    ├── auth.ts
    ├── crypto.ts
    ├── protocol.ts
    └── types.ts
```
