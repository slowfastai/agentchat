import {
  buildAppRelayToken,
  buildDaemonRelayToken,
  extractBearerToken,
  hashRelaySecret,
  parseRelayToken,
} from "./auth";
import { DeviceHubDO } from "./device-hub";
import { PairingIndexDO } from "./pairing-index";
import type { DevBootstrapRequest, DevPairRequest, Env } from "./types";

export { DeviceHubDO, PairingIndexDO };

const INTERNAL_BOOTSTRAP_DAEMON_URL = "https://device-hub.internal/internal/bootstrap-daemon";
const INTERNAL_REGISTER_APP_URL = "https://device-hub.internal/internal/register-app";
const INTERNAL_WS_URL = "https://device-hub.internal/internal/ws";

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

function deviceHubStub(env: Env, deviceId: string): DurableObjectStub {
  return env.DEVICE_HUB.get(env.DEVICE_HUB.idFromName(deviceId));
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
