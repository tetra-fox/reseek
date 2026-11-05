import type { Schemas } from "#lib/api/client.js";
import { live } from "#lib/live/live.svelte.js";

/** A message from the public room feed, with an id of its own for the list's key */
export type FeedLine = Schemas["PublicRoomMessage"] & { id: number };

/** How many messages the feed keeps; the oldest go past it */
export const FEED_LINES = 500;

/** Whether `line` is one a filter of `words`, in lower case, keeps: each word in its room, name or text */
export const feedMatches = (line: FeedLine, words: string[]): boolean => {
  const haystack = `${line.roomName}\n${line.username}\n${line.message}`.toLowerCase();
  return words.every((word) => haystack.includes(word));
};

/**
 * Every message said in any public room while the feed is open. The daemon asks the server for
 * them only while a page follows `public_rooms` and keeps none of them, so the feed starts empty
 * each time it opens and stops when it closes
 */
export class PublicFeed {
  lines = $state<FeedLine[]>([]);
  #next = 0;
  #release: (() => void)[] = [];

  /** Follows the feed until `close` */
  open() {
    if (this.#release.length) return;
    this.#release.push(
      live.retain("public_rooms"),
      live.on("rooms.public_message", (message) => this.#add(message))
    );
  }

  /** Stops following it, which tells the daemon nobody here wants it */
  close() {
    for (const release of this.#release.splice(0)) release();
  }

  #add(message: Schemas["PublicRoomMessage"]) {
    this.#next += 1;
    this.lines.push({ ...message, id: this.#next });
    if (this.lines.length > FEED_LINES) this.lines.splice(0, this.lines.length - FEED_LINES);
  }
}
