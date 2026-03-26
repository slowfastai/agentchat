export type RelayRole = "daemon" | "app";

export interface Env {
  DEVICE_HUB: DurableObjectNamespace;
  PAIRING_INDEX: DurableObjectNamespace;
  RELAY_DEV_MODE?: string;
}

export interface DaemonRelayToken {
  role: "daemon";
  deviceId: string;
  secret: string;
  peerId: "daemon";
}

export interface AppRelayToken {
  role: "app";
  deviceId: string;
  appInstallationId: string;
  secret: string;
  peerId: `app:${string}`;
}

export interface PairingTicket {
  deviceId: string;
  pairingId: string;
  secret: string;
}

export type ParsedRelayToken = DaemonRelayToken | AppRelayToken;

export interface SocketAttachment {
  role: RelayRole;
  deviceId: string;
  peerId: string;
  connectionId: string;
  appInstallationId?: string;
}

export interface DaemonRecord {
  deviceId: string;
  deviceName?: string;
  tokenHash: string;
  createdAt: number;
  updatedAt: number;
}

export interface AppRecord {
  appInstallationId: string;
  appName?: string;
  tokenHash: string;
  pairedAt: number;
  updatedAt: number;
}

export interface PairingRecord {
  pairingId: string;
  ticketHash: string;
  createdAt: number;
  expiresAt: number;
}

export interface RelayReadyFrame {
  type: "relay_ready";
  id: string;
  timestamp: number;
  protocol_version: "relay-wire/0.1";
  device_id: string;
  role: RelayRole;
  peer_id: string;
  connection_id: string;
}

export interface RelayErrorFrame {
  type: "relay_error";
  id: string;
  timestamp: number;
  code: RelayErrorCode;
  message: string;
  ref_id?: string;
}

export type RelayFrameType =
  | "secure_channel_hello"
  | "secure_channel_accept"
  | "relay_envelope";

export interface RoutableRelayFrame {
  type: RelayFrameType;
  id?: string;
  from: string;
  to: string;
  [key: string]: unknown;
}

export type RelayErrorCode =
  | "FORBIDDEN_SENDER"
  | "UNPAIRED_PEER"
  | "INVALID_SCHEMA"
  | "PEER_OFFLINE";

export interface RouteValidationSuccess {
  ok: true;
  targetPeerId: string;
}

export interface RouteValidationFailure {
  ok: false;
  code: RelayErrorCode;
  message: string;
}

export type RouteValidationResult =
  | RouteValidationSuccess
  | RouteValidationFailure;

export interface DevBootstrapRequest {
  device_id?: string;
  device_name?: string;
}

export interface DevPairRequest {
  device_id: string;
  app_installation_id?: string;
  app_name?: string;
}

export interface PairingOpenRequest {
  ttl_ms?: number;
}

export interface PairingClaimRequest {
  pairing_ticket: string;
  app_installation_id?: string;
  app_name?: string;
}
