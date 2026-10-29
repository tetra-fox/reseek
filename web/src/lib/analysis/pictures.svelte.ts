import { root, type Schemas, sessionGone } from "#lib/api/client.js";
import { type Outcome, reason, UNREACHABLE } from "#lib/request.js";

import { analysisRoute, type Layout, type Target } from "./analysis";

/** A run of cells the analysis did not read: the first and one past the last */
type Run = [number, number];

/**
 * Cells over a stretch of a file's time, evenly spread from `from` to `to` seconds: spectrogram
 * columns of `width` level bytes each, or waveform buckets of two signed bytes
 */
export type Cells = {
  from: number;
  to: number;
  count: number;
  /** the bytes of one cell */
  width: number;
  bytes: Uint8Array;
  unread: Run[];
  /** the levels of byte 0 and byte 255 of spectrogram columns, in dB */
  minDb?: number;
  maxDb?: number;
};

/** The spectrum of every analyzed window over its second, `bins` level bytes each */
export type Spectra = Schemas["WindowSpectra"] & { bytes: Uint8Array };

/** What a binary answer is: a four-byte little-endian header length, the header, then bytes */
export const unframe = <H>(buffer: ArrayBuffer): { header: H; bytes: Uint8Array } => {
  const length = new DataView(buffer).getUint32(0, true);
  const header = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 4, length))) as H;
  return { header, bytes: new Uint8Array(buffer, 4 + length) };
};

type Piece = "spectrogram" | "waveform" | "spectra";

/** Loads a piece of a target's pictures, the overview or tile `tile`, as its bytes */
const load = async (t: Target, piece: Piece, tile?: number): Promise<Outcome<ArrayBuffer>> => {
  const query = tile === undefined ? "" : `?tile=${tile}`;
  let response: Response;
  try {
    response = await globalThis.fetch(`${root()}${analysisRoute(t)}/${piece}${query}`, {
      credentials: "same-origin"
    });
  } catch {
    // fetch rejects only when no response came back at all
    return { ok: false, error: UNREACHABLE, status: 0 };
  }
  if (response.status === 401) sessionGone();
  if (!response.ok) {
    // an error answer carries its JSON body; one without leaves the status to say it
    const body: unknown = await response.json().catch(() => undefined);
    return { ok: false, error: reason(response.status, body), status: response.status, body };
  }
  return { ok: true, data: await response.arrayBuffer() };
};

const spectrogramCells = (buffer: ArrayBuffer): Cells => {
  const { header, bytes } = unframe<Schemas["SpectrogramTile"]>(buffer);
  return {
    from: header.fromSeconds,
    to: header.toSeconds,
    count: header.columns,
    width: header.bins,
    bytes,
    unread: header.unread as Run[],
    minDb: header.minDb,
    maxDb: header.maxDb
  };
};

const waveformCells = (buffer: ArrayBuffer): Cells => {
  const { header, bytes } = unframe<Schemas["WaveformTile"]>(buffer);
  return {
    from: header.fromSeconds,
    to: header.toSeconds,
    count: header.peaks,
    width: 2,
    bytes,
    unread: header.unread as Run[]
  };
};

/** How many tiles of each kind are kept, the least recently drawn going first */
const KEPT_TILES = 48;
/** How many tiles load at once */
const LOADING = 4;

/**
 * One report's spectrogram, waveform and window spectra as the browser has them: the overviews
 * and the spectra once loaded, and the tiles at full detail asked for as the view needs them.
 * `version` changes whenever something arrives, for a view to draw again
 */
export class Pictures {
  readonly target: Target;
  readonly layout: Layout;
  spectrogram = $state.raw<Cells | null>(null);
  waveform = $state.raw<Cells | null>(null);
  spectra = $state.raw<Spectra | null>(null);
  /** why the overview could not be loaded, empty while it loads or once it has */
  error = $state("");
  /** why a piece beside the overview could not be loaded: the waveform, the spectra or a tile */
  lost = $state("");
  version = $state(0);
  readonly #tiles = new Map<string, Cells>();
  readonly #wanted: string[] = [];
  readonly #loading = new Set<string>();
  /** tiles whose load failed, which are not asked for again */
  readonly #failed = new Set<string>();

  constructor(target: Target, layout: Layout) {
    this.target = target;
    this.layout = layout;
  }

  /** Loads the overviews and the window spectra */
  async start() {
    const [spectrogram, waveform, spectra] = await Promise.all([
      load(this.target, "spectrogram"),
      load(this.target, "waveform"),
      load(this.target, "spectra")
    ]);
    if (!spectrogram.ok) {
      this.error = spectrogram.error;
      return;
    }
    this.spectrogram = spectrogramCells(spectrogram.data);
    // without the waveform its strip stays hatched as unread, and the view says why
    if (waveform.ok) this.waveform = waveformCells(waveform.data);
    else this.lost = waveform.error;
    if (spectra.ok) {
      const { header, bytes } = unframe<Schemas["WindowSpectra"]>(spectra.data);
      this.spectra = { ...header, bytes };
    } else {
      this.lost = spectra.error;
    }
    this.version++;
  }

  /** The tile of `kind` numbered `tile`, when it has arrived */
  tile(kind: "spectrogram" | "waveform", tile: number): Cells | undefined {
    return this.#tiles.get(`${kind}:${tile}`);
  }

  /** Asks for the tiles of both kinds from `from` to `to` seconds, the nearest first */
  want(from: number, to: number) {
    const seconds = this.layout.tileSeconds;
    const first = Math.max(0, Math.floor(from / seconds));
    const last = Math.min(this.layout.tiles - 1, Math.floor(to / seconds));
    for (let tile = first; tile <= last; tile++) {
      for (const kind of ["spectrogram", "waveform"] as const) {
        const key = `${kind}:${tile}`;
        const kept = this.#tiles.get(key);
        if (kept) {
          // drawn again, so the most recently used
          this.#tiles.delete(key);
          this.#tiles.set(key, kept);
        } else if (
          !this.#loading.has(key) &&
          !this.#failed.has(key) &&
          !this.#wanted.includes(key)
        ) {
          this.#wanted.push(key);
        }
      }
    }
    this.#next();
  }

  #next() {
    while (this.#loading.size < LOADING && this.#wanted.length) {
      const key = this.#wanted.shift()!;
      this.#loading.add(key);
      const [kind, tile] = key.split(":") as ["spectrogram" | "waveform", string];
      void load(this.target, kind, Number(tile)).then((outcome) => {
        this.#loading.delete(key);
        // a tile that failed would fail again, most often as its kept file is gone, so the view
        // keeps the overview there and says why
        if (!outcome.ok) {
          this.#failed.add(key);
          this.lost = outcome.error;
        } else {
          this.#tiles.set(
            key,
            kind === "spectrogram" ? spectrogramCells(outcome.data) : waveformCells(outcome.data)
          );
          while (this.#tiles.size > KEPT_TILES * 2) {
            this.#tiles.delete(this.#tiles.keys().next().value!);
          }
          this.version++;
        }
        this.#next();
      });
    }
  }

  /** Drops what was asked for and has not started loading, as the view moved elsewhere */
  forget() {
    this.#wanted.length = 0;
  }
}
