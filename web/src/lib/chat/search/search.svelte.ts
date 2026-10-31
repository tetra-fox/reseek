import { api } from "#lib/api/client.js";
import { chat } from "#lib/chat/chat.svelte.js";
import { call, type Outcome } from "#lib/request.js";
import type { UserSummary } from "#lib/users/users.svelte.js";

import {
  type Found,
  fullTextQuery,
  matchNames,
  type MessageHit,
  plainQuery,
  type RoomFound,
  type SearchKind
} from "./match";

/** How many users are read for each text, the closest of them are listed */
const CANDIDATES = 100;

const hitsIn = (q: string, kind: SearchKind, offset: number, limit: number) =>
  call(() => api.GET("/api/v0/messages/search", { params: { query: { q, kind, offset, limit } } }));

/**
 * The stored messages `text` finds. A stray quote or a lone operator is no mistake in a box that
 * takes names too, so a query the daemon can't read is read again as plain words
 */
const readHits = async (
  text: string,
  kind: SearchKind,
  offset: number,
  limit: number
): Promise<Outcome<{ hits: MessageHit[]; total: number }>> => {
  const written = fullTextQuery(text);
  if (!written) return { ok: true, data: { hits: [], total: 0 } };
  const outcome = await hitsIn(written, kind, offset, limit);
  const plain = plainQuery(text);
  if (outcome.ok || outcome.status !== 400 || !plain || plain === written) return outcome;
  return hitsIn(plain, kind, offset, limit);
};

/**
 * What one text finds across the chats: rooms joined or known by name, users with a conversation
 * or in the directory, and stored messages. The names held match as the text changes, and `read`
 * asks the daemon for the rest
 */
export class ChatSearch {
  /** the rooms the server lists and those with stored messages */
  #rooms = $state.raw<RoomFound[]>([]);
  /** the users the directory matched for the text; tags and notes match too */
  #users = $state.raw<UserSummary[]>([]);
  /** the stored messages that match, the newest first */
  hits = $state.raw<MessageHit[]>([]);
  /** how many stored messages match, on every page together */
  total = $state(0);
  /** why the messages could not be read, empty when they could */
  error = $state("");
  /** the categories shown whole, past the first few */
  whole = $state({ rooms: false, users: false });
  /** how far the results' list was scrolled, and its line last opened, to come back to */
  scrollTop = 0;
  line: string | null = null;

  #text: () => string;
  #kind: () => SearchKind;
  #limit: number;
  /** what the last finished read was for */
  #read = $state("");
  #asked = 0;
  /** the read under way and what it is for, so one text is read once however often it is asked */
  #reading: { asking: string; done: Promise<void> } | null = null;
  /** the read of the room names under way or done, null until one holds the server's list */
  #namesRead: Promise<void> | null = null;

  /** the rooms and users whose names hold the text, and the messages read for it */
  readonly found: Found = $derived.by(() => {
    const text = this.#text();
    if (!text) return { rooms: [], users: [], hits: [] };
    const joined = chat.rooms.rows.map((r) => ({
      name: r.name,
      joined: true,
      userCount: r.userCount
    }));
    const talked = chat.conversations.rows.map((c) => ({ name: c.username }));
    const known = this.#users.map((summary) => ({ name: summary.username, summary }));
    return {
      ...matchNames([...this.#rooms, ...joined], [...talked, ...known], text),
      hits: this.hits
    };
  });

  /** the daemon has answered for the text and the kind as they are now */
  readonly current = $derived(this.#read === this.#asking());

  constructor(text: () => string, kind: () => SearchKind, limit: number) {
    this.#text = text;
    this.#kind = kind;
    this.#limit = limit;
  }

  #asking() {
    return `${this.#kind()}:${this.#text()}`;
  }

  /** Resolves once the daemon has answered for the text as it is now, room names included */
  settled = async () => {
    if (!this.current) await this.read();
    await this.#namesRead;
  };

  /** Reads what the daemon holds for the text, in place of what it held for another */
  read = (): Promise<void> => {
    const asking = this.#asking();
    if (this.#reading?.asking === asking) return this.#reading.done;
    const reading = {
      asking,
      done: this.#readNow(asking).finally(() => {
        if (this.#reading === reading) this.#reading = null;
      })
    };
    this.#reading = reading;
    return reading.done;
  };

  async #readNow(asking: string) {
    const text = this.#text();
    const kind = this.#kind();
    const asked = ++this.#asked;
    if (!text) {
      this.#users = [];
      this.hits = [];
      this.total = 0;
      this.error = "";
      this.#read = asking;
      return;
    }
    // the server's list can take a moment and everything else waits for nothing
    this.#namesRead ??= this.#readNames();
    const [users, found] = await Promise.all([
      call(() => api.GET("/api/v0/users", { params: { query: { q: text, limit: CANDIDATES } } })),
      readHits(text, kind, 0, this.#limit)
    ]);
    if (asked !== this.#asked) return;
    // a directory that can't be read leaves the users already listed
    if (users.ok) this.#users = users.data.users;
    if (found.ok) {
      this.hits = found.data.hits;
      this.total = found.data.total;
      this.error = "";
    } else this.error = found.error;
    this.whole = { rooms: false, users: false };
    this.scrollTop = 0;
    this.line = null;
    this.#read = asking;
  }

  /** Drops what was read, so the next look at the text reads it anew */
  forget = () => {
    this.#read = "";
  };

  /** Reads the next page of messages */
  more = async () => {
    const asked = this.#asked;
    const found = await readHits(this.#text(), this.#kind(), this.hits.length, this.#limit);
    if (asked !== this.#asked) return;
    if (!found.ok) return void (this.error = found.error);
    this.error = "";
    this.hits = [...this.hits, ...found.data.hits];
    this.total = found.data.total;
  };

  /**
   * Reads the rooms the daemon knows by name; the server's list needs a connection to it, so a
   * read without one is tried again with the next text
   */
  async #readNames() {
    const [listed, stored] = await Promise.all([
      call(() => api.GET("/api/v0/rooms/available")),
      call(() => api.GET("/api/v0/rooms/history"))
    ]);
    if (!listed.ok) this.#namesRead = null;
    this.#rooms = [
      ...(stored.ok ? stored.data.rooms : []).map((r) => ({
        name: r.name,
        joined: false,
        userCount: null
      })),
      ...(listed.ok ? listed.data : []).map((r) => ({
        name: r.name,
        joined: r.joined,
        userCount: r.userCount
      }))
    ];
  }
}
