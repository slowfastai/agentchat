import { describe, expect, it } from "vitest";

import {
  buildRelayReady,
  parseRoutableRelayFrame,
  validateRoute,
} from "./protocol";
import type { SocketAttachment } from "./types";

const appSocket: SocketAttachment = {
  role: "app",
  deviceId: "dev_123",
  peerId: "app:app_456",
  appInstallationId: "app_456",
  connectionId: "rc_test_connection",
};

const daemonSocket: SocketAttachment = {
  role: "daemon",
  deviceId: "dev_123",
  peerId: "daemon",
  connectionId: "rc_test_connection_2",
};

describe("parseRoutableRelayFrame", () => {
  it("accepts routable relay frames", () => {
    const result = parseRoutableRelayFrame(
      JSON.stringify({
        type: "secure_channel_hello",
        from: "app:app_456",
        to: "daemon",
      }),
    );

    expect(result.ok).toBe(true);
  });

  it("rejects unsupported frame types", () => {
    const result = parseRoutableRelayFrame(
      JSON.stringify({
        type: "peer_upsert",
        from: "daemon",
        to: "app:app_456",
      }),
    );

    expect(result).toEqual({
      ok: false,
      message:
        "frame type must be one of secure_channel_hello, secure_channel_accept, relay_envelope",
    });
  });
});

describe("validateRoute", () => {
  it("rejects forged from values", () => {
    const result = validateRoute(
      appSocket,
      {
        type: "secure_channel_hello",
        from: "daemon",
        to: "daemon",
      },
      new Set(["app_456"]),
    );

    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.code).toBe("FORBIDDEN_SENDER");
    }
  });

  it("allows app to daemon routing", () => {
    const result = validateRoute(
      appSocket,
      {
        type: "secure_channel_hello",
        from: "app:app_456",
        to: "daemon",
      },
      new Set(["app_456"]),
    );

    expect(result).toEqual({ ok: true, targetPeerId: "daemon" });
  });

  it("requires daemon targets to be paired apps", () => {
    const result = validateRoute(
      daemonSocket,
      {
        type: "relay_envelope",
        from: "daemon",
        to: "app:missing",
      },
      new Set(["app_456"]),
    );

    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.code).toBe("UNPAIRED_PEER");
    }
  });
});

describe("buildRelayReady", () => {
  it("includes the wire protocol version and connection id", () => {
    const frame = buildRelayReady(appSocket);

    expect(frame.type).toBe("relay_ready");
    expect(frame.protocol_version).toBe("relay-wire/0.1");
    expect(frame.connection_id).toBe("rc_test_connection");
    expect(frame.peer_id).toBe("app:app_456");
  });
});
