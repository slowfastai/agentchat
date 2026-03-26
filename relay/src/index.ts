import {
  buildAppRelayToken,
  buildDaemonRelayToken,
  buildPairingTicket,
  extractBearerToken,
  hashRelaySecret,
  parsePairingTicket,
  parseRelayToken,
} from "./auth";
import { DeviceHubDO } from "./device-hub";
import { PairingIndexDO } from "./pairing-index";
import type {
  DevBootstrapRequest,
  DevPairRequest,
  Env,
  PairingClaimRequest,
  PairingOpenRequest,
} from "./types";

export { DeviceHubDO, PairingIndexDO };

const INTERNAL_BOOTSTRAP_DAEMON_URL = "https://device-hub.internal/internal/bootstrap-daemon";
const INTERNAL_REGISTER_APP_URL = "https://device-hub.internal/internal/register-app";
const INTERNAL_OPEN_PAIRING_URL = "https://device-hub.internal/internal/open-pairing";
const INTERNAL_CLAIM_PAIRING_URL = "https://device-hub.internal/internal/claim-pairing";
const INTERNAL_WS_URL = "https://device-hub.internal/internal/ws";
const DEFAULT_PAIRING_TTL_MS = 5 * 60 * 1000;
const MIN_PAIRING_TTL_MS = 30 * 1000;
const MAX_PAIRING_TTL_MS = 15 * 60 * 1000;

export default {
  async fetch(request, env): Promise<Response> {
    const url = new URL(request.url);

    if (request.method === "GET" && url.pathname === "/healthz") {
      return json({ ok: true });
    }

    if (request.method === "GET" && url.pathname === "/v1/ws") {
      return handleWebSocketUpgrade(request, env);
    }

    if (request.method === "POST" && url.pathname === "/v1/dev/bootstrap") {
      return handleDevBootstrap(request, env);
    }

    if (request.method === "POST" && url.pathname === "/v1/dev/pair") {
      return handleDevPair(request, env);
    }

    if (request.method === "POST" && url.pathname === "/v1/pairing/open") {
      return handlePairingOpen(request, env);
    }

    if (request.method === "POST" && url.pathname === "/v1/pairing/claim") {
      return handlePairingClaim(request, env);
    }

    return json({ error: "not_found" }, 404);
  },
} satisfies ExportedHandler<Env>;

async function handleWebSocketUpgrade(
  request: Request,
  env: Env,
): Promise<Response> {
  if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
    return json({ error: "expected_websocket_upgrade" }, 426);
  }

  const bearerToken = extractBearerToken(request);
  if (!bearerToken) {
    return json({ error: "missing_bearer_token" }, 401);
  }

  const parsedToken = parseRelayToken(bearerToken);
  if (!parsedToken) {
    return json({ error: "invalid_relay_token" }, 401);
  }

  const stub = deviceHubStub(env, parsedToken.deviceId);
  const headers = new Headers(request.headers);
  headers.set("x-relay-role", parsedToken.role);
  headers.set("x-relay-device-id", parsedToken.deviceId);
  headers.set("x-relay-peer-id", parsedToken.peerId);
  headers.set("x-relay-token-secret", parsedToken.secret);
  if (parsedToken.role === "app") {
    headers.set("x-relay-app-installation-id", parsedToken.appInstallationId);
  }

  return stub.fetch(
    new Request(INTERNAL_WS_URL, {
      method: "GET",
      headers,
    }),
  );
}

async function handleDevBootstrap(
  request: Request,
  env: Env,
): Promise<Response> {
  if (!isDevModeEnabled(env)) {
    return json({ error: "not_found" }, 404);
  }

  const payload = await readJson<DevBootstrapRequest>(request);
  const deviceId = payload?.device_id ?? `dev_${crypto.randomUUID()}`;
  const deviceName = payload?.device_name;
  const { relayToken, secret } = buildDaemonRelayToken(deviceId);
  const tokenHash = await hashRelaySecret(secret);

  const stub = deviceHubStub(env, deviceId);
  const initResponse = await stub.fetch(
    new Request(INTERNAL_BOOTSTRAP_DAEMON_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        device_id: deviceId,
        device_name: deviceName,
        token_hash: tokenHash,
      }),
    }),
  );

  if (!initResponse.ok) {
    return json({ error: "device_init_failed" }, 500);
  }

  return json({
    device_id: deviceId,
    relay_token: relayToken,
    ws_url: buildWebSocketUrl(request),
  });
}

async function handleDevPair(request: Request, env: Env): Promise<Response> {
  if (!isDevModeEnabled(env)) {
    return json({ error: "not_found" }, 404);
  }

  const payload = await readJson<DevPairRequest>(request);
  if (!payload?.device_id) {
    return json({ error: "device_id_required" }, 400);
  }

  const deviceId = payload.device_id;
  const appInstallationId = payload.app_installation_id ?? crypto.randomUUID();
  const appName = payload.app_name;
  const { relayToken, secret } = buildAppRelayToken(deviceId, appInstallationId);
  const tokenHash = await hashRelaySecret(secret);

  const stub = deviceHubStub(env, deviceId);
  const initResponse = await stub.fetch(
    new Request(INTERNAL_REGISTER_APP_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        app_installation_id: appInstallationId,
        app_name: appName,
        token_hash: tokenHash,
      }),
    }),
  );

  if (!initResponse.ok) {
    return json({ error: "app_registration_failed" }, 500);
  }

  return json({
    device_id: deviceId,
    app_installation_id: appInstallationId,
    peer_id: `app:${appInstallationId}`,
    relay_token: relayToken,
    ws_url: buildWebSocketUrl(request),
  });
}

async function handlePairingOpen(request: Request, env: Env): Promise<Response> {
  const bearerToken = extractBearerToken(request);
  if (!bearerToken) {
    return json({ error: "missing_bearer_token" }, 401);
  }

  const parsedToken = parseRelayToken(bearerToken);
  if (!parsedToken) {
    return json({ error: "invalid_relay_token" }, 401);
  }
  if (parsedToken.role !== "daemon") {
    return json({ error: "daemon_token_required" }, 403);
  }

  const payload = await readJson<PairingOpenRequest>(request);
  const ttlMs = clampPairingTTL(payload?.ttl_ms);
  const expiresAt = Date.now() + ttlMs;
  const { pairingTicket, pairingId, secret } = buildPairingTicket(parsedToken.deviceId);
  const ticketHash = await hashRelaySecret(secret);

  const stub = deviceHubStub(env, parsedToken.deviceId);
  const openResponse = await stub.fetch(
    new Request(INTERNAL_OPEN_PAIRING_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        daemon_secret: parsedToken.secret,
        pairing_id: pairingId,
        ticket_hash: ticketHash,
        expires_at: expiresAt,
      }),
    }),
  );

  if (!openResponse.ok) {
    return proxyErrorResponse(openResponse, "pairing_open_failed");
  }

  return json({
    pairing_ticket: pairingTicket,
    expires_at: expiresAt,
    ws_url: buildWebSocketUrl(request),
  });
}

async function handlePairingClaim(request: Request, env: Env): Promise<Response> {
  const payload = await readJson<PairingClaimRequest>(request);
  if (!payload?.pairing_ticket) {
    return json({ error: "pairing_ticket_required" }, 400);
  }

  const parsedTicket = parsePairingTicket(payload.pairing_ticket);
  if (!parsedTicket) {
    return json({ error: "invalid_pairing_ticket" }, 400);
  }

  const appInstallationId = payload.app_installation_id ?? crypto.randomUUID();
  const appName = payload.app_name;
  const { relayToken, secret } = buildAppRelayToken(
    parsedTicket.deviceId,
    appInstallationId,
  );
  const tokenHash = await hashRelaySecret(secret);

  const stub = deviceHubStub(env, parsedTicket.deviceId);
  const claimResponse = await stub.fetch(
    new Request(INTERNAL_CLAIM_PAIRING_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        pairing_id: parsedTicket.pairingId,
        ticket_secret: parsedTicket.secret,
        app_installation_id: appInstallationId,
        app_name: appName,
        token_hash: tokenHash,
      }),
    }),
  );

  if (!claimResponse.ok) {
    return proxyErrorResponse(claimResponse, "pairing_claim_failed");
  }

  return json({
    device_id: parsedTicket.deviceId,
    app_installation_id: appInstallationId,
    peer_id: `app:${appInstallationId}`,
    relay_token: relayToken,
    ws_url: buildWebSocketUrl(request),
  });
}

function deviceHubStub(env: Env, deviceId: string): DurableObjectStub {
  return env.DEVICE_HUB.get(env.DEVICE_HUB.idFromName(deviceId));
}

function clampPairingTTL(value: number | undefined): number {
  if (!Number.isFinite(value)) {
    return DEFAULT_PAIRING_TTL_MS;
  }

  return Math.min(MAX_PAIRING_TTL_MS, Math.max(MIN_PAIRING_TTL_MS, Math.trunc(value!)));
}

async function proxyErrorResponse(response: Response, fallbackError: string): Promise<Response> {
  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    payload = { error: fallbackError };
  }

  return Response.json(payload, { status: response.status });
}

function isDevModeEnabled(env: Env): boolean {
  return env.RELAY_DEV_MODE === "true";
}

function buildWebSocketUrl(request: Request): string {
  const url = new URL(request.url);
  url.pathname = "/v1/ws";
  url.search = "";
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.toString();
}

async function readJson<T>(request: Request): Promise<T | null> {
  try {
    return (await request.json<T>()) ?? null;
  } catch {
    return null;
  }
}

function json(payload: unknown, status = 200): Response {
  return Response.json(payload, { status });
}
