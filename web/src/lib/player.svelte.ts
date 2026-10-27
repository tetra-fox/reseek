import { audioType } from "#lib/components/files/links.js";
import { getFileName } from "#lib/format.js";
import { toast } from "#lib/toast.svelte.js";
import { contentUrl } from "#lib/transfers/content.js";
import type { Transfer } from "#lib/transfers/states.js";

/** Whether this browser says it can play `filename` in the page */
export const playable = (filename: string, probe = document.createElement("audio")) => {
  const mime = audioType(filename);
  return !!mime && probe.canPlayType(mime) !== "";
};

/**
 * The audio preview: plays finished downloads in a bar under the page that stays across
 * screens, with the other files of the same folder as the tracks before and after. A file the
 * browser can't play is offered as a download instead
 */
class Player {
  tracks = $state.raw<Transfer[]>([]);
  index = $state(0);
  /** the URL the audio element plays */
  src = $state<string | null>(null);
  playing = $state(false);
  volume = $state(1);

  get current(): Transfer | null {
    return this.tracks[this.index] ?? null;
  }

  /** Plays `track`, with `folder` (the finished files of its folder) as what comes before and after */
  play = async (track: Transfer, folder: Transfer[] = [track]) => {
    const tracks = folder.filter((t) => playable(t.filename));
    if (!playable(track.filename)) return this.#offerDownload(track);
    this.tracks = tracks.includes(track) ? tracks : [track];
    this.index = Math.max(this.tracks.indexOf(track), 0);
    await this.#load();
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
    await this.#load();
  }

  async #load() {
    const track = this.current;
    if (!track) return;
    const url = await contentUrl(track, "inline");
    if (!url.ok) {
      toast(url.error, "error");
      return;
    }
    this.src = url.data;
    this.playing = true;
  }

  #offerDownload(track: Transfer) {
    toast(`This browser can't play ${getFileName(track.filename)}`, "error", {
      action: {
        label: "Download it",
        run: async () => {
          const url = await contentUrl(track, "attachment");
          if (url.ok) window.location.assign(url.data);
          else toast(url.error, "error");
        }
      }
    });
  }
}

export const player = new Player();
