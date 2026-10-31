/** What a file is by its extension, for its icon */
export type FileKind = "audio" | "video" | "other";

/** Each known extension's kind, with the type a browser plays it as where it can */
const KINDS: Record<string, { kind: FileKind; type?: string; wav?: boolean }> = {
  aac: { kind: "audio", type: "audio/aac" },
  aif: { kind: "audio", wav: true },
  aifc: { kind: "audio", wav: true },
  aiff: { kind: "audio", wav: true },
  alac: { kind: "audio", wav: true },
  ape: { kind: "audio" },
  flac: { kind: "audio", type: "audio/flac" },
  m4a: { kind: "audio", type: "audio/mp4" },
  mp3: { kind: "audio", type: "audio/mpeg" },
  oga: { kind: "audio", type: "audio/ogg" },
  ogg: { kind: "audio", type: "audio/ogg" },
  opus: { kind: "audio", type: "audio/ogg; codecs=opus" },
  wav: { kind: "audio", type: "audio/wav" },
  webm: { kind: "audio", type: "audio/webm" },
  wma: { kind: "audio" },
  avi: { kind: "video" },
  flv: { kind: "video" },
  m4v: { kind: "video" },
  mkv: { kind: "video" },
  mov: { kind: "video" },
  mp4: { kind: "video" },
  wmv: { kind: "video" }
};

const known = (path: string) => KINDS[path.split(".").pop()?.toLowerCase() ?? ""];

/** What a file is by its extension */
export const kindOf = (path: string): FileKind => known(path)?.kind ?? "other";

/**
 * Whether a browser can only play a file as a WAV the daemon makes of it: AIFF by its name, and
 * ALAC by the codec an analysis found, since an .m4a can as well hold AAC, which plays as it is
 */
export const needsWav = (path: string, codec?: string) =>
  codec === "alac" || known(path)?.wav === true;

/** The type a browser is asked to play an audio file as, "" for anything it can't play */
export const audioType = (path: string, codec?: string) =>
  needsWav(path, codec) ? "audio/wav" : (known(path)?.type ?? "");
