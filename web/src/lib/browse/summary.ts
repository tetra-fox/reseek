import type { Schemas } from "#lib/api/client.js";

type File = Schemas["File"];

/** The quality of several files in a word: all VBR, one bit rate, or "Mixed"; "" when unknown */
export const attributesOf = (files: File[]): string => {
  if (!files.length) return "";
  const vbr = files.filter((f) => f.isVariableBitRate).length;
  if (vbr === files.length) return "VBR";
  if (vbr > 0) return "Mixed";
  const rates = new Set(files.map((f) => f.bitRate).filter((r) => r));
  if (rates.size === 1) return `${[...rates][0]} kbps`;
  return rates.size > 1 ? "Mixed" : "";
};

/** The lengths of several files added up, in seconds */
export const lengthOf = (files: File[]) => files.reduce((sum, f) => sum + (f.length ?? 0), 0);

/** The sizes of several files added up, in bytes */
export const sizeOf = (files: File[]) => files.reduce((sum, f) => sum + f.size, 0);
