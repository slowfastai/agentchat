const BASE64_PADDING = /=+$/;

export function utf8Bytes(input: string): Uint8Array {
  return new TextEncoder().encode(input);
}

export function bytesToBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (const byte of bytes) {
    binary += String.fromCharCode(byte);
  }

  return btoa(binary)
    .replace(BASE64_PADDING, "")
    .replace(/\+/g, "-")
    .replace(/\//g, "_");
}

export async function sha256Base64Url(input: string): Promise<string> {
  const bytes = Uint8Array.from(utf8Bytes(input));
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return bytesToBase64Url(new Uint8Array(digest));
}

export function randomBase64Url(byteLength: number): string {
  const bytes = new Uint8Array(byteLength);
  crypto.getRandomValues(bytes);
  return bytesToBase64Url(bytes);
}

export function generateConnectionId(): string {
  return `rc_${randomBase64Url(16)}`;
}

export function randomSecret(): string {
  return randomBase64Url(32);
}

export function nowMs(): number {
  return Date.now();
}
