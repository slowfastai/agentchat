import { describe, expect, it } from "vitest";

import { generateConnectionId, randomSecret } from "./crypto";

describe("relay crypto helpers", () => {
  it("generates connection ids with the documented prefix", () => {
    const connectionId = generateConnectionId();

    expect(connectionId.startsWith("rc_")).toBe(true);
    expect(connectionId).toMatch(/^rc_[A-Za-z0-9_-]{12,64}$/);
  });

  it("generates base64url secrets without padding", () => {
    const secret = randomSecret();

    expect(secret).toMatch(/^[A-Za-z0-9_-]{16,128}$/);
    expect(secret.includes("=")).toBe(false);
  });
});
