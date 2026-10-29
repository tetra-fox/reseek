import { toast } from "#lib/feedback/toast.svelte.js";
import { getFileName } from "#lib/format.js";
import type { Outcome } from "#lib/request.js";

import { audioType } from "./kinds";

/** A file the player can play: a finished download or a file under the download directories */
export type Track = {
  /** what tells two tracks apart */
  key: string;
  /** its name or path, the last part shown */
  filename: string;
  /** the URL that loads it, played in the page or saved */
  url: (disposition: "inline" | "attachment") => Promise<Outcome<string>>;
  /** its folder on the Files screen, null when it isn't under a root the screen shows */
  folder: { root: "downloads" | "incomplete"; path: string } | null;
};

/** Whether this browser says it can play `filename` in the page */
export const playable = (filename: string, probe = document.createElement("audio")) => {
  const mime = audioType(filename);
  return !!mime && probe.canPlayType(mime) !== "";
};

/**
 * The audio preview: plays finished files in a bar under the page that stays across
 * screens, with the other files of the same folder as the tracks before and after. A file the
 * browser can't play is offered as a download instead
 */
class Player {
  tracks = $state.raw<Track[]>([]);
  index = $state(0);
  /** the URL the audio element plays */
  src = $state<string | null>(null);
  playing = $state(false);
  volume = $state(1);
  /** the audio element the bar plays through, while it is on the page */
  element = $state.raw<HTMLAudioElement | null>(null);
  /** where to start the next track once it has loaded, in seconds */
  #startAt = 0;

  get current(): Track | null {
    return this.tracks[this.index] ?? null;
  }

  /**
   * Plays `track` from `startAt` seconds, with `folder` (the finished files of its folder) as what
   * comes before and after
   */
  play = async (track: Track, folder: Track[] = [track], startAt = 0) => {
    const tracks = folder.filter((t) => playable(t.filename));
    if (!playable(track.filename)) return this.#offerDownload(track);
    const at = tracks.findIndex((t) => t.key === track.key);
    this.tracks = at === -1 ? [track] : tracks;
    this.index = Math.max(at, 0);
    this.#startAt = startAt;
    await this.#load();
  };

  /** Whether the player has `key` loaded, playing or paused */
  holds = (key: string) => !!this.src && this.current?.key === key;

  /** Where the track is, in seconds, read from the element as it plays */
  position = (): number => this.element?.currentTime ?? 0;

  /** Moves the track to `seconds` */
  seek = (seconds: number) => {
    if (this.element) this.element.currentTime = seconds;
  };

  /** Where a track that has just loaded starts, taken once */
  takeStart = (): number => {
    const at = this.#startAt;
    this.#startAt = 0;
    return at;
  };

  next = () => this.#move(1);
  previous = () => this.#move(-1);

  /** The audio element could not play what it was given */
  failed = () => {
    const track = this.current;
    this.playing = false;
    if (track) this.#offerDownload(track);
  };

  close = () => {
    this.tracks = [];
    this.src = null;
    this.playing = false;
  };

  async #move(step: number) {
    const to = this.index + step;
    if (to < 0 || to >= this.tracks.length) return;
    this.index = to;
    this.#startAt = 0;
    await this.#load();
  }

  async #load() {
    const track = this.current;
    if (!track) return;
    const url = await track.url("inline");
    if (!url.ok) {
      toast(url.error, "error");
      return;
    }
    this.src = url.data;
    this.playing = true;
  }

  #offerDownload(track: Track) {
    toast(`This browser cannot play ${getFileName(track.filename)}`, "error", {
      action: {
        label: "Download",
        run: async () => {
          const url = await track.url("attachment");
          if (url.ok) window.location.assign(url.data);
          else toast(url.error, "error");
        }
      }
    });
  }
}

export const player = new Player();
