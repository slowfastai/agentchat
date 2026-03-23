import { nowMs } from "./crypto";
import type {
  RelayErrorCode,
  RelayErrorFrame,
  RelayFrameType,
  RelayReadyFrame,
  RouteValidationResult,
  RoutableRelayFrame,
  SocketAttachment,
} from "./types";

export const RELAY_WIRE_PROTOCOL_VERSION = "relay-wire/0.1" as const;

const ALLOWED_FRAME_TYPES = new Set<RelayFrameType>([
  "secure_channel_hello",
  "secure_channel_accept",
  "relay_envelope",
]);

export function parseRoutableRelayFrame(
  raw: string,
):
  | { ok: true; frame: RoutableRelayFrame }
  | { ok: false; message: string } {
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { ok: false, message: "frame must be valid JSON" };
  }

  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    return { ok: false, message: "frame must be a JSON object" };
  }

  const frame = parsed as Record<string, unknown>;
  if (typeof frame.type !== "string" || !ALLOWED_FRAME_TYPES.has(frame.type as RelayFrameType)) {
    return {
      ok: false,
      message:
        "frame type must be one of secure_channel_hello, secure_channel_accept, relay_envelope",
    };
  }

  if (typeof frame.from !== "string" || frame.from.length === 0) {
    return { ok: false, message: "frame.from must be a non-empty string" };
  }

  if (typeof frame.to !== "string" || frame.to.length === 0) {
    return { ok: false, message: "frame.to must be a non-empty string" };
  }

  return { ok: true, frame: frame as RoutableRelayFrame };
}

export function validateRoute(
  sender: SocketAttachment,
  frame: RoutableRelayFrame,
  pairedAppIds: ReadonlySet<string>,
): RouteValidationResult {
  if (frame.from !== sender.peerId) {
    return {
      ok: false,
      code: "FORBIDDEN_SENDER",
      message: `frame.from ${JSON.stringify(frame.from)} does not match authenticated peer ${JSON.stringify(sender.peerId)}`,
    };
  }

  if (sender.role === "app") {
    if (frame.to !== "daemon") {
      return {
        ok: false,
        code: "UNPAIRED_PEER",
        message: `app connections may only route to daemon, got ${JSON.stringify(frame.to)}`,
      };
    }

    return { ok: true, targetPeerId: "daemon" };
  }

  if (!frame.to.startsWith("app:")) {
    return {
      ok: false,
      code: "UNPAIRED_PEER",
      message: `daemon connections may only route to paired app:* peers, got ${JSON.stringify(frame.to)}`,
    };
  }

  const appInstallationId = frame.to.slice("app:".length);
  if (!pairedAppIds.has(appInstallationId)) {
    return {
      ok: false,
      code: "UNPAIRED_PEER",
      message: `target peer ${JSON.stringify(frame.to)} is not paired for this device`,
    };
  }

  return { ok: true, targetPeerId: frame.to };
}

export function buildRelayReady(sender: SocketAttachment): RelayReadyFrame {
  return {
    type: "relay_ready",
    id: crypto.randomUUID(),
    timestamp: nowMs(),
    protocol_version: RELAY_WIRE_PROTOCOL_VERSION,
    device_id: sender.deviceId,
    role: sender.role,
    peer_id: sender.peerId,
    connection_id: sender.connectionId,
  };
}

export function buildRelayError(
  code: RelayErrorCode,
  message: string,
  refId?: string,
): RelayErrorFrame {
  return {
    type: "relay_error",
    id: crypto.randomUUID(),
    timestamp: nowMs(),
    code,
    message,
    ...(refId ? { ref_id: refId } : {}),
  };
}

export function extractFrameRefId(frame: RoutableRelayFrame): string | undefined {
  return typeof frame.id === "string" && frame.id.length > 0 ? frame.id : undefined;
}
