import { randomSecret, sha256Base64Url } from "./crypto";
import type { AppRelayToken, DaemonRelayToken, ParsedRelayToken } from "./types";

const TOKEN_COMPONENT_PATTERN = /^[A-Za-z0-9_-]{1,128}$/;
const TOKEN_SECRET_PATTERN = /^[A-Za-z0-9_-]{16,128}$/;

export function extractBearerToken(request: Request): string | null {
  const header = request.headers.get("authorization");
  if (!header) {
    return null;
  }

  const match = header.match(/^Bearer\s+(.+)$/i);
  return match?.[1]?.trim() ?? null;
}

export function parseRelayToken(token: string): ParsedRelayToken | null {
  const parts = token.split(".");
  const prefix = parts[0];

  if (prefix === "achdm" && parts.length === 3) {
    const [, deviceId, secret] = parts;
    if (!isTokenComponent(deviceId) || !isTokenSecret(secret)) {
      return null;
    }

    const parsed: DaemonRelayToken = {
      role: "daemon",
      deviceId,
      secret,
      peerId: "daemon",
    };
    return parsed;
  }

  if (prefix === "achapp" && parts.length === 4) {
    const [, deviceId, appInstallationId, secret] = parts;
    if (
      !isTokenComponent(deviceId) ||
      !isTokenComponent(appInstallationId) ||
      !isTokenSecret(secret)
    ) {
      return null;
    }

    const parsed: AppRelayToken = {
      role: "app",
      deviceId,
      appInstallationId,
      secret,
      peerId: `app:${appInstallationId}`,
    };
    return parsed;
  }

  return null;
}

export async function hashRelaySecret(secret: string): Promise<string> {
  return sha256Base64Url(secret);
}

export function buildDaemonRelayToken(deviceId: string): {
  relayToken: string;
  secret: string;
} {
  const secret = randomSecret();
  return {
    relayToken: `achdm.${deviceId}.${secret}`,
    secret,
  };
}

export function buildAppRelayToken(
  deviceId: string,
  appInstallationId: string,
): {
  relayToken: string;
  secret: string;
} {
  const secret = randomSecret();
  return {
    relayToken: `achapp.${deviceId}.${appInstallationId}.${secret}`,
    secret,
  };
}

function isTokenComponent(value: string): boolean {
  return TOKEN_COMPONENT_PATTERN.test(value);
}

function isTokenSecret(value: string): boolean {
  return TOKEN_SECRET_PATTERN.test(value);
}
