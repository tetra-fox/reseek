import { analysisRoute, type Layout, type Target } from "#lib/analysis/analysis.js";
import { root, type Schemas, sessionGone } from "#lib/api/client.js";
import { type Outcome, reason, UNREACHABLE } from "#lib/request.js";

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
  const route = analysisRoute(t);
  const query = new URLSearchParams(route.query);
  if (tile !== undefined) query.set("tile", String(tile));
  const search = query.size ? `?${query}` : "";
  let response: Response;
  try {
    response = await globalThis.fetch(`${root()}${route.path}/${piece}${search}`, {
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

/** The first and last tile over `from` to `to` seconds, inside the file */
const tileRange = (layout: Layout, from: number, to: number) => ({
  first: Math.max(0, Math.floor(from / layout.tileSeconds)),
  last: Math.min(layout.tiles - 1, Math.floor(to / layout.tileSeconds))
});

/**
 * Whether the tiles over `from` to `to` seconds fit in those kept. A stretch that needs more is
 * drawn from the overview alone, since tiles over part of it would change the detail partway
 */
export const tilesFit = (layout: Layout, from: number, to: number) => {
  const { first, last } = tileRange(layout, from, to);
  return last - first + 1 <= KEPT_TILES;
};
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
  /** why a piece beside the overview could not be loaded, by piece; `tile` for the latest tile */
  lost = $state<{ waveform?: string; spectra?: string; tile?: string }>({});
  version = $state(0);
  readonly #tiles = new Map<string, Cells>();
  readonly #wanted: string[] = [];
  readonly #loading = new Set<string>();
  /** tiles whose load failed, which are not asked for again */
  readonly #failed = new Set<string>();
  #overview: Promise<void> | null = null;
  #started = false;

  constructor(target: Target, layout: Layout) {
    this.target = target;
    this.layout = layout;
  }

  /** Loads the whole file's spectrogram, once, which is all a thumbnail needs */
  overview() {
    this.#overview ??= load(this.target, "spectrogram").then((outcome) => {
      if (outcome.ok) this.spectrogram = spectrogramCells(outcome.data);
      else this.error = outcome.error;
      this.version++;
    });
    return this.#overview;
  }

  /** Loads the rest of what the viewer shows, once: the waveform and the window spectra */
  async start() {
    if (this.#started) return;
    this.#started = true;
    const [, waveform, spectra] = await Promise.all([
      this.overview(),
      load(this.target, "waveform"),
      load(this.target, "spectra")
    ]);
    if (this.error) return;
    // without the waveform its strip stays hatched as unread, and the view says why
    if (waveform.ok) this.waveform = waveformCells(waveform.data);
    else this.lost.waveform = waveform.error;
    if (spectra.ok) {
      const { header, bytes } = unframe<Schemas["WindowSpectra"]>(spectra.data);
      this.spectra = { ...header, bytes };
    } else {
      this.lost.spectra = spectra.error;
    }
    this.version++;
  }

  /** The tiles of `kind` that have arrived and overlap `from` to `to` seconds */
  tilesOver(kind: "spectrogram" | "waveform", from: number, to: number): Cells[] {
    const found: Cells[] = [];
    for (const [key, cells] of this.#tiles) {
      if (key.startsWith(`${kind}:`) && cells.from <= to && from < cells.to) found.push(cells);
    }
    return found;
  }

  /**
   * Asks for the tiles of both kinds from `from` to `to` seconds, at most as many as are kept:
   * more would push each other out, so a longer stretch gets those around its middle
   */
  want(from: number, to: number) {
    const seconds = this.layout.tileSeconds;
    const { first, last } = tileRange(this.layout, from, to);
    const count = Math.min(KEPT_TILES, last - first + 1);
    const middle = Math.floor((from + to) / 2 / seconds);
    const start = Math.max(first, Math.min(last - count + 1, middle - Math.floor(count / 2)));
    for (let tile = start; tile < start + count; tile++) {
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
          this.lost.tile = outcome.error;
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
