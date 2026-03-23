import { describe, expect, it } from "vitest";

import {
  buildAppRelayToken,
  buildDaemonRelayToken,
  parseRelayToken,
} from "./auth";

describe("parseRelayToken", () => {
  it("parses daemon tokens", () => {
    const { relayToken } = buildDaemonRelayToken("dev_123");
    const parsed = parseRelayToken(relayToken);

    expect(parsed).toEqual({
      role: "daemon",
      deviceId: "dev_123",
      secret: expect.any(String),
      peerId: "daemon",
    });
  });

  it("parses app tokens", () => {
    const { relayToken } = buildAppRelayToken("dev_123", "app_456");
    const parsed = parseRelayToken(relayToken);

    expect(parsed).toEqual({
      role: "app",
      deviceId: "dev_123",
      appInstallationId: "app_456",
      secret: expect.any(String),
      peerId: "app:app_456",
    });
  });

  it("rejects malformed tokens", () => {
    expect(parseRelayToken("bad-token")).toBeNull();
    expect(parseRelayToken("achdm.dev.***")).toBeNull();
    expect(parseRelayToken("achapp.dev.app.secret.with.too.many.parts")).toBeNull();
  });
});
