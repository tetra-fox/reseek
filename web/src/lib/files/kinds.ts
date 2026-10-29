/** What a file is by its extension, for its icon */
export type FileKind = "audio" | "video" | "other";

/** Each known extension's kind, with the type a browser plays it as where it can */
const KINDS: Record<string, { kind: FileKind; type?: string }> = {
  aac: { kind: "audio", type: "audio/aac" },
  aif: { kind: "audio" },
  aiff: { kind: "audio" },
  alac: { kind: "audio" },
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

/** The type a browser is asked to play an audio file as, "" for anything it can't play */
export const audioType = (path: string) => known(path)?.type ?? "";

/** Whether the browser can play a file in the page, judged by its name */
export const isAudio = (path: string) => audioType(path) !== "";
