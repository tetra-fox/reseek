import { toast } from "#lib/feedback/toast.svelte.js";
import { audioType, needsWav } from "#lib/files/kinds.js";
import type { Outcome } from "#lib/request.js";

/** A file the analysis popup can play */
export type Track = {
  /** its name or path */
  filename: string;
  /** the URL that loads it as it is stored, or as the WAV the daemon makes of it */
  url: (as: "content" | "wav") => Promise<Outcome<string>>;
};

/** Whether this browser can play `filename`, read by an analysis as `codec` */
export const playable = (
  filename: string,
  codec: string,
  probe = document.createElement("audio")
) => {
  const type = audioType(filename, codec);
  return !!type && probe.canPlayType(type) !== "";
};

/** What the daemon said when it refused a URL, null when it did not */
const refusal = async (url: string): Promise<string | null> => {
  try {
    const answer = await fetch(url, { headers: { Range: "bytes=0-0" } });
    if (answer.ok) return null;
    const body = (await answer.json()) as { error?: string };
    return body.error ?? null;
  } catch {
    // the daemon could not be asked, which says nothing about the file
    return null;
  }
};

/**
 * The file being checked, played in the analysis popup: the audio element is in the popup, so the
 * sound stops when it closes and nothing else in the page plays
 */
export class Playback {
  /** the element that plays, there once a URL is */
  element = $state.raw<HTMLAudioElement | null>(null);
  /** the URL the element plays, set the first time Play is pressed */
  src = $state<string | null>(null);
  playing = $state(false);
  /** why the file does not play, once that is known */
  problem = $state<string | null>(null);
  /** where to start once the file has loaded, in seconds */
  #startAt = 0;

  /** Plays `track` from `from` seconds, or pauses and resumes it once it is loaded */
  toggle = async (track: Track, codec: string, from: number) => {
    if (this.src) {
      this.playing = !this.playing;
      return;
    }
    const url = await track.url(needsWav(track.filename, codec) ? "wav" : "content");
    if (!url.ok) {
      toast(url.error, "error");
      return;
    }
    this.#startAt = from;
    this.src = url.data;
    this.playing = true;
  };

  /** Makes the element do what `playing` says */
  apply = () => {
    if (!this.element || !this.src) return;
    if (this.playing)
      this.element.play().catch(() => {
        // refused, as a browser does without a user's click; a file that fails is reported by
        // the element's error event
        this.playing = false;
      });
    else this.element.pause();
  };

  /** The file has loaded: it starts where it was asked to */
  loaded = () => {
    if (this.element && this.#startAt > 0) this.element.currentTime = this.#startAt;
    this.#startAt = 0;
  };

  /** Where the file is, in seconds, read from the element as it plays */
  position = () => this.element?.currentTime ?? 0;

  /** Moves the file to `seconds` */
  seek = (seconds: number) => {
    if (this.element) this.element.currentTime = seconds;
  };

  /** The element could not play what it was given */
  failed = async () => {
    const url = this.src;
    this.playing = false;
    this.src = null;
    this.problem = (url && (await refusal(url))) ?? "This file could not be played.";
  };
}
