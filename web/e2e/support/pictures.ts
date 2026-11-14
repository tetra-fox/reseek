/**
 * The spectrogram, waveform and window spectra a fake report serves, made up as the daemon frames
 * them, and a short WAV for the Analysis page to play
 */

import type { Schemas } from "../../src/lib/api/client";

const BINS = 512;
const COLUMNS_PER_SECOND = 16;
const PEAKS_PER_SECOND = 200;
const TILE_SECONDS = 16;
const MAX_HZ = 22_050;
/** where the made-up content stops, as a 128 kbps transcode's does */
const CUTOFF_HZ = 16_774;

/** A binary answer: the header's length, the header as JSON, then the bytes */
const framed = (header: object, bytes: Uint8Array) => {
  const json = Buffer.from(JSON.stringify(header));
  const length = Buffer.alloc(4);
  length.writeUInt32LE(json.length);
  return Buffer.concat([length, json, bytes]);
};

/** A column at time `t`: loud and swelling below the cutoff, silent above */
const column = (t: number, out: Uint8Array, at: number) => {
  for (let bin = 0; bin < BINS; bin++) {
    const hz = ((bin + 0.5) * MAX_HZ) / BINS;
    out[at + bin] = hz < CUTOFF_HZ ? 150 + Math.round(40 * Math.sin(t / 3 + bin / 40)) : 0;
  }
};

/** The cells at full detail a file `duration` seconds long has */
const detail = (duration: number, perSecond: number) => Math.ceil(duration * perSecond - 1e-9);

/** The layout of the report of a file `duration` seconds long */
export const layout = (duration: number, scale: Schemas["SpectrogramScale"]) =>
  ({
    scale,
    maxHz: MAX_HZ,
    durationSeconds: duration,
    sampled: false,
    guides: [
      { hz: 16000, label: "MP3 128k", kind: "encoder" },
      { hz: CUTOFF_HZ, label: "analyzer's cutoff", kind: "cutoff" },
      { hz: 18600, label: "MP3 192k", kind: "encoder" },
      { hz: 19530, label: "MP3 224-256k", kind: "encoder" },
      { hz: 20000, label: "MP3 320k / AAC", kind: "encoder" },
      { hz: MAX_HZ, label: "file limit", kind: "file_limit" }
    ],
    bins: BINS,
    columnsPerSecond: COLUMNS_PER_SECOND,
    peaksPerSecond: PEAKS_PER_SECOND,
    tileSeconds: TILE_SECONDS,
    tiles: Math.ceil(detail(duration, COLUMNS_PER_SECOND) / (TILE_SECONDS * COLUMNS_PER_SECOND))
  }) satisfies Schemas["SpectrogramLayout"];

/** The spectrogram's overview, or with `tile` that tile, of a file `duration` seconds long */
export const spectrogram = (duration: number, tile: number | null) => {
  const total = detail(duration, COLUMNS_PER_SECOND);
  const first = tile === null ? 0 : tile * TILE_SECONDS * COLUMNS_PER_SECOND;
  const end = tile === null ? total : Math.min(total, first + TILE_SECONDS * COLUMNS_PER_SECOND);
  const count = tile === null ? Math.min(1024, total) : end - first;
  const span = tile === null ? total / count : 1;
  const bytes = new Uint8Array(count * BINS);
  for (let c = 0; c < count; c++)
    column((first + (c + 0.5) * span) / COLUMNS_PER_SECOND, bytes, c * BINS);
  return framed(
    {
      fromSeconds: first / COLUMNS_PER_SECOND,
      toSeconds: (tile === null ? total : end) / COLUMNS_PER_SECOND,
      columns: count,
      bins: BINS,
      maxHz: MAX_HZ,
      minDb: -160,
      maxDb: 0,
      unread: []
    },
    bytes
  );
};

/** The waveform's overview, or with `tile` that tile: a tone whose loudness swells and fades */
export const waveform = (duration: number, tile: number | null) => {
  const total = detail(duration, PEAKS_PER_SECOND);
  const first = tile === null ? 0 : tile * TILE_SECONDS * PEAKS_PER_SECOND;
  const end = tile === null ? total : Math.min(total, first + TILE_SECONDS * PEAKS_PER_SECOND);
  const count = tile === null ? Math.min(2048, total) : end - first;
  const span = tile === null ? total / count : 1;
  const bytes = new Uint8Array(count * 2);
  for (let b = 0; b < count; b++) {
    const t = (first + (b + 0.5) * span) / PEAKS_PER_SECOND;
    const peak = Math.round(20 + 90 * Math.abs(Math.sin(t / 2)));
    bytes[b * 2] = (256 - peak) & 255;
    bytes[b * 2 + 1] = peak;
  }
  return framed(
    {
      fromSeconds: first / PEAKS_PER_SECOND,
      toSeconds: (tile === null ? total : end) / PEAKS_PER_SECOND,
      peaks: count,
      unread: []
    },
    bytes
  );
};

/** Every window's spectrum: each reaches a little higher than the one before */
export const spectra = (windows: number) => {
  const bytes = new Uint8Array(windows * BINS);
  for (let w = 0; w < windows; w++) {
    for (let bin = 0; bin < BINS; bin++) {
      bytes[w * BINS + bin] = bin < 100 + w * 8 ? 200 - Math.round(bin / 6) : 10;
    }
  }
  return framed({ windows, bins: BINS, maxHz: MAX_HZ, minDb: -160, maxDb: 0 }, bytes);
};

/** A mono 16-bit WAV of a quiet tone `seconds` long at 8 kHz */
export const wav = (seconds: number) => {
  const rate = 8000;
  const samples = rate * seconds;
  const out = Buffer.alloc(44 + samples * 2);
  out.write("RIFF", 0);
  out.writeUInt32LE(36 + samples * 2, 4);
  out.write("WAVEfmt ", 8);
  out.writeUInt32LE(16, 16);
  out.writeUInt16LE(1, 20);
  out.writeUInt16LE(1, 22);
  out.writeUInt32LE(rate, 24);
  out.writeUInt32LE(rate * 2, 28);
  out.writeUInt16LE(2, 32);
  out.writeUInt16LE(16, 34);
  out.write("data", 36);
  out.writeUInt32LE(samples * 2, 40);
  for (let i = 0; i < samples; i++) {
    out.writeInt16LE(Math.round(2000 * Math.sin((2 * Math.PI * 440 * i) / rate)), 44 + i * 2);
  }
  return out;
};
