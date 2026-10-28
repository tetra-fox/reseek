/**
 * A new random id in the form of an RFC 4122 version 4 UUID. Built from `getRandomValues`, which
 * every browser has on any origin; `crypto.randomUUID` exists only over HTTPS or on localhost
 */
export const newId = (): string => {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  // the version (4) in the high nibble of byte 6, the variant (binary 10) in the top of byte 8
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return [
    hex.slice(0, 8),
    hex.slice(8, 12),
    hex.slice(12, 16),
    hex.slice(16, 20),
    hex.slice(20)
  ].join("-");
};
