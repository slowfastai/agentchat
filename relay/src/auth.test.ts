import { describe, expect, it } from "vitest";

import {
  buildAppRelayToken,
  buildDaemonRelayToken,
  buildPairingTicket,
  parsePairingTicket,
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

describe("parsePairingTicket", () => {
  it("parses pairing tickets", () => {
    const { pairingTicket, pairingId } = buildPairingTicket("dev_123");
    const parsed = parsePairingTicket(pairingTicket);

    expect(parsed).toEqual({
      deviceId: "dev_123",
      pairingId,
      secret: expect.any(String),
    });
  });

  it("rejects malformed pairing tickets", () => {
    expect(parsePairingTicket("bad-ticket")).toBeNull();
    expect(parsePairingTicket("achpair.dev.only-three-parts")).toBeNull();
    expect(parsePairingTicket("achpair.dev.pair.***")).toBeNull();
  });
});
