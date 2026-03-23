import { hashRelaySecret } from "./auth";
import { generateConnectionId, nowMs } from "./crypto";
import {
  buildRelayError,
  buildRelayReady,
  extractFrameRefId,
  parseRoutableRelayFrame,
  validateRoute,
} from "./protocol";
import type { AppRecord, DaemonRecord, Env, SocketAttachment } from "./types";

const DAEMON_RECORD_KEY = "daemon_record";
const APP_RECORD_PREFIX = "app_record:";
const INTERNAL_BOOTSTRAP_DAEMON_PATH = "/internal/bootstrap-daemon";
const INTERNAL_REGISTER_APP_PATH = "/internal/register-app";
const INTERNAL_WS_PATH = "/internal/ws";
const SOCKET_CLOSE_REPLACED = 4001;
const SOCKET_CLOSE_UNAUTHENTICATED = 4401;
const SOCKET_CLOSE_INVALID_PAYLOAD = 4400;

interface BootstrapDaemonPayload {
  device_id: string;
  device_name?: string;
  token_hash: string;
}

interface RegisterAppPayload {
  app_installation_id: string;
  app_name?: string;
  token_hash: string;
}

export class DeviceHubDO {
  private readonly ready: Promise<void>;
  private daemonRecord: DaemonRecord | null = null;
  private readonly pairedApps = new Map<string, AppRecord>();
  private daemonSocket: WebSocket | null = null;
  private readonly appSockets = new Map<string, WebSocket>();

  constructor(
    private readonly ctx: DurableObjectState,
    private readonly _env: Env,
  ) {
    this.ready = this.loadState();
  }

  async fetch(request: Request): Promise<Response> {
    await this.ready;

    const url = new URL(request.url);
    if (request.method === "POST" && url.pathname === INTERNAL_BOOTSTRAP_DAEMON_PATH) {
      return this.handleBootstrapDaemon(request);
    }

    if (request.method === "POST" && url.pathname === INTERNAL_REGISTER_APP_PATH) {
      return this.handleRegisterApp(request);
    }

    if (request.method === "GET" && url.pathname === INTERNAL_WS_PATH) {
      return this.handleWebSocketConnect(request);
    }

    return json({ error: "not_found" }, 404);
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer): Promise<void> {
    await this.ready;

    const attachment = this.readAttachment(ws);
    if (!attachment) {
      ws.close(SOCKET_CLOSE_UNAUTHENTICATED, "missing socket attachment");
      return;
    }

    if (typeof message !== "string") {
      this.sendJson(
        ws,
        buildRelayError(
          "INVALID_SCHEMA",
          "relay expects UTF-8 JSON text frames only",
        ),
      );
      return;
    }

    const parsed = parseRoutableRelayFrame(message);
    if (!parsed.ok) {
      this.sendJson(ws, buildRelayError("INVALID_SCHEMA", parsed.message));
      return;
    }

    const route = validateRoute(
      attachment,
      parsed.frame,
      new Set(this.pairedApps.keys()),
    );
    if (!route.ok) {
      this.sendJson(
        ws,
        buildRelayError(route.code, route.message, extractFrameRefId(parsed.frame)),
      );
      return;
    }

    const target = this.lookupSocket(route.targetPeerId);
    if (!target) {
      this.sendJson(
        ws,
        buildRelayError(
          "PEER_OFFLINE",
          `target peer ${JSON.stringify(route.targetPeerId)} is offline`,
          extractFrameRefId(parsed.frame),
        ),
      );
      return;
    }

    target.send(message);
  }

  async webSocketClose(ws: WebSocket): Promise<void> {
    await this.ready;
    this.forgetSocket(ws);
  }

  async webSocketError(ws: WebSocket): Promise<void> {
    await this.ready;
    this.forgetSocket(ws);
  }

  private async loadState(): Promise<void> {
    this.daemonRecord =
      (await this.ctx.storage.get<DaemonRecord>(DAEMON_RECORD_KEY)) ?? null;

    const appEntries = await this.ctx.storage.list<AppRecord>({
      prefix: APP_RECORD_PREFIX,
    });
    for (const appRecord of appEntries.values()) {
      this.pairedApps.set(appRecord.appInstallationId, appRecord);
    }

    this.rebuildSocketIndexes();
  }

  private rebuildSocketIndexes(): void {
    this.daemonSocket = null;
    this.appSockets.clear();

    for (const ws of this.ctx.getWebSockets()) {
      const attachment = this.readAttachment(ws);
      if (!attachment) {
        continue;
      }

      if (attachment.role === "daemon") {
        this.daemonSocket = ws;
      } else if (attachment.appInstallationId) {
        this.appSockets.set(attachment.appInstallationId, ws);
      }
    }
  }

  private async handleBootstrapDaemon(request: Request): Promise<Response> {
    const payload = await readJson<BootstrapDaemonPayload>(request);
    if (!payload || !payload.device_id || !payload.token_hash) {
      return json({ error: "invalid_request" }, 400);
    }

    const timestamp = nowMs();
    const record: DaemonRecord = {
      deviceId: payload.device_id,
      deviceName: payload.device_name,
      tokenHash: payload.token_hash,
      createdAt: this.daemonRecord?.createdAt ?? timestamp,
      updatedAt: timestamp,
    };

    await this.ctx.storage.put(DAEMON_RECORD_KEY, record);
    this.daemonRecord = record;

    return json({ ok: true });
  }

  private async handleRegisterApp(request: Request): Promise<Response> {
    const payload = await readJson<RegisterAppPayload>(request);
    if (!payload || !payload.app_installation_id || !payload.token_hash) {
      return json({ error: "invalid_request" }, 400);
    }

    const timestamp = nowMs();
    const existing = this.pairedApps.get(payload.app_installation_id);
    const record: AppRecord = {
      appInstallationId: payload.app_installation_id,
      appName: payload.app_name,
      tokenHash: payload.token_hash,
      pairedAt: existing?.pairedAt ?? timestamp,
      updatedAt: timestamp,
    };

    await this.ctx.storage.put(this.appStorageKey(payload.app_installation_id), record);
    this.pairedApps.set(payload.app_installation_id, record);

    return json({ ok: true });
  }

  private async handleWebSocketConnect(request: Request): Promise<Response> {
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return json({ error: "expected_websocket_upgrade" }, 426);
    }

    const role = request.headers.get("x-relay-role");
    const deviceId = request.headers.get("x-relay-device-id");
    const peerId = request.headers.get("x-relay-peer-id");
    const secret = request.headers.get("x-relay-token-secret");
    const appInstallationId = request.headers.get("x-relay-app-installation-id") ?? undefined;

    if (!role || !deviceId || !peerId || !secret) {
      return json({ error: "missing_auth_context" }, 400);
    }

    const isAuthenticated = await this.verifySecret(role, secret, appInstallationId);
    if (!isAuthenticated) {
      return json({ error: "unauthorized" }, 403);
    }

    const socketPair = new WebSocketPair();
    const client = socketPair[0];
    const server = socketPair[1];
    const attachment: SocketAttachment = {
      role: role === "daemon" ? "daemon" : "app",
      deviceId,
      peerId,
      connectionId: generateConnectionId(),
      ...(appInstallationId ? { appInstallationId } : {}),
    };

    this.replaceExistingSocket(attachment);

    this.ctx.acceptWebSocket(server);
    server.serializeAttachment(attachment);

    if (attachment.role === "daemon") {
      this.daemonSocket = server;
    } else if (attachment.appInstallationId) {
      this.appSockets.set(attachment.appInstallationId, server);
    }

    this.sendJson(server, buildRelayReady(attachment));

    return new Response(null, {
      status: 101,
      webSocket: client,
    });
  }

  private async verifySecret(
    role: string,
    secret: string,
    appInstallationId?: string,
  ): Promise<boolean> {
    const candidateHash = await hashRelaySecret(secret);

    if (role === "daemon") {
      return this.daemonRecord?.tokenHash === candidateHash;
    }

    if (role === "app" && appInstallationId) {
      return this.pairedApps.get(appInstallationId)?.tokenHash === candidateHash;
    }

    return false;
  }

  private replaceExistingSocket(attachment: SocketAttachment): void {
    if (attachment.role === "daemon") {
      this.closeSocket(this.daemonSocket);
      return;
    }

    if (!attachment.appInstallationId) {
      return;
    }

    this.closeSocket(this.appSockets.get(attachment.appInstallationId) ?? null);
  }

  private lookupSocket(peerId: string): WebSocket | null {
    if (peerId === "daemon") {
      return this.daemonSocket;
    }

    if (!peerId.startsWith("app:")) {
      return null;
    }

    return this.appSockets.get(peerId.slice("app:".length)) ?? null;
  }

  private forgetSocket(ws: WebSocket): void {
    const attachment = this.readAttachment(ws);
    if (!attachment) {
      return;
    }

    if (attachment.role === "daemon") {
      if (this.daemonSocket === ws) {
        this.daemonSocket = null;
      }
      return;
    }

    if (attachment.appInstallationId && this.appSockets.get(attachment.appInstallationId) === ws) {
      this.appSockets.delete(attachment.appInstallationId);
    }
  }

  private readAttachment(ws: WebSocket): SocketAttachment | null {
    const attachment = ws.deserializeAttachment();
    if (!attachment || typeof attachment !== "object") {
      return null;
    }

    return attachment as SocketAttachment;
  }

  private closeSocket(ws: WebSocket | null): void {
    if (!ws) {
      return;
    }

    try {
      ws.close(SOCKET_CLOSE_REPLACED, "superseded by a newer connection");
    } catch {
      // Ignore close races.
    }
  }

  private sendJson(ws: WebSocket, payload: unknown): void {
    ws.send(JSON.stringify(payload));
  }

  private appStorageKey(appInstallationId: string): string {
    return `${APP_RECORD_PREFIX}${appInstallationId}`;
  }
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
