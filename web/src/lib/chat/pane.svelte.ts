import { api, byRoom, byUser, type Schemas } from "#lib/api/client.js";
import {
  fromPrivate,
  fromRoom,
  type Gap,
  type Message,
  oneLine
} from "#lib/chat/messages/messages.js";
import { Keyed } from "#lib/lists/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";

import { chat } from "./chat.svelte";

/** What a pane shows: the conversation with a user, or a room */
export type Target = { kind: "user"; name: string } | { kind: "room"; name: string };

/** How many messages one page of history holds */
export const PAGE = 100;

let sent = 0;

/** How long a room message the daemon took waits for the server's echo before it shows as not sent */
export const ECHO_WAIT = 30_000;

/** One read of stored messages, oldest first, and whether older ones are stored */
type Page = { messages: Message[]; hasMore: boolean; gaps: Gap[] };

/** 1 for a message still being sent, which goes after the stored ones */
const sending = (m: Message) => (m.localId < 0 ? 1 : 0);

/**
 * The messages of one conversation or room, oldest first: the newest page on open, older pages
 * as the list scrolls up, and new ones as frames arrive. A pane opened at a message holds the
 * pages around it instead, and reads newer pages as the list scrolls down until it reaches the
 * newest, where frames count again. Messages are kept by their id and never change once shown,
 * so the list keeps its nodes. A message we send shows at once and stays until the stored one
 * with its text arrives, which for a room is when the server echoes it; a room message with no
 * echo in time shows as not sent
 */
export class MessagePane {
  /** whose conversation or which room */
  readonly target: Target;
  /** the messages held, by id; those being sent have negative ids */
  readonly messages = new Keyed<Message>((m) => String(m.localId));
  /** the stretches of room history reseek missed */
  gaps = $state.raw<Gap[]>([]);
  /** older messages are stored than the pane holds */
  hasMore = $state(false);
  /**
   * newer messages are stored than the pane holds: it opened at an older message, so this is
   * true from the start until the pages read show otherwise
   */
  hasNewer = $state(false);
  /** counts the times the pane went back to the newest messages, which the list goes down to */
  returned = $state(0);
  /** the first page is here, or the view was emptied before it came */
  loaded = $state(false);
  /** why the last read failed, empty when it didn't */
  error = $state("");
  /** where the unread messages started when the pane opened, null when none were unread */
  unreadFrom = $state<number | null>(null);
  /** the conversation as stored, for its draft; null for a room or a conversation not started */
  conversation = $state.raw<Schemas["Conversation"] | null>(null);
  /** the message the pane opened at, null when it opened at the newest or went to them since */
  at = $state<number | null>(null);
  #self: () => string;
  /**
   * counts the times the messages held were let go for others, by going to the newest or
   * emptying the view; a page read before that belongs to what was let go and is dropped
   */
  #view = 0;

  /** the messages in order: the stored ones by id, then those still being sent */
  readonly ordered = $derived(
    this.messages.rows.toSorted(
      (a, b) => sending(a) - sending(b) || Math.abs(a.localId) - Math.abs(b.localId)
    )
  );

  constructor(target: Target, self: () => string, at: number | null = null) {
    this.target = target;
    this.#self = self;
    this.at = at;
    this.hasNewer = at !== null;
  }

  /** Follows the pane's frames; call from a component's setup, it stops with the component */
  follow() {
    const { kind, name } = this.target;
    if (kind === "user") {
      $effect(() =>
        live.on("conversations.message", ({ username, message }) => {
          if (username === name) this.#arrived(fromPrivate(message, this.#self()));
        })
      );
      return;
    }
    $effect(() =>
      live.on("rooms.message", ({ roomName, message }) => {
        if (roomName === name) this.#arrived(fromRoom(message));
      })
    );
    // a session that began after a gap says so; an open gap is replaced by its end. The frame
    // comes after the rooms are joined again, so a room we left is not given one
    $effect(() =>
      live.on("rooms.gap", (gap) => {
        if (!chat.rooms.get(name)) return;
        this.gaps = [...this.gaps.filter((g) => g.from !== gap.from), gap];
      })
    );
  }

  /**
   * Reads the newest page, or the pages around the message the pane opened at until they have
   * come; the first time it also finds where the unread messages start. A pane still short of the
   * newest messages reads nothing here: they come as the list scrolls down to them
   */
  load = async () => {
    // a room's read marker comes with the joined rooms, which a pane opened by a link or a
    // reload can be quicker than; its first unread message is placed from that marker
    if (!this.loaded && this.target.kind === "room") await chat.ready();
    if (this.loaded && this.hasNewer) return;
    const view = this.#view;
    const at = this.loaded ? null : this.at;
    const page: Outcome<Page & { hasNewer?: boolean }> =
      at === null ? await this.#page({}) : await this.#around(at);
    if (view !== this.#view) return;
    if (!page.ok) return void (this.error = page.error);
    this.error = "";
    for (const m of page.data.messages) this.#store(m);
    this.#gapsFrom(page.data.gaps);
    if (this.loaded) return;
    this.hasMore = page.data.hasMore;
    // only the pages around a message can stop short of the newest
    this.hasNewer = page.data.hasNewer ?? false;
    this.#opened();
    if (at !== null && !this.hasNewer) await this.#reachedNewest();
  };

  /**
   * The pane holds the newest messages again, so frames count from here on; this read brings
   * what arrived while they did not
   */
  #reachedNewest() {
    return this.load();
  }

  /** The first messages are here: where the unread ones start, and a conversation's draft */
  #opened() {
    // what is unread lies past the pages read when newer ones remain
    if (!this.hasNewer) this.unreadFrom = this.#firstUnread();
    if (this.target.kind === "user") void this.#readConversation();
    this.loaded = true;
  }

  /** Reads the page before the oldest message shown */
  older = async () => {
    const oldest = this.ordered.find((m) => m.localId > 0)?.localId;
    if (oldest === undefined || !this.hasMore) return;
    const view = this.#view;
    const page = await this.#page({ before: oldest });
    if (view !== this.#view) return;
    if (!page.ok) return void (this.error = page.error);
    for (const m of page.data.messages) this.#store(m);
    this.#gapsFrom(page.data.gaps);
    this.hasMore = page.data.hasMore;
  };

  /** Reads the page after the newest message shown */
  newer = async () => {
    const newest = this.ordered.findLast((m) => m.localId > 0)?.localId;
    if (newest === undefined || !this.hasNewer) return;
    const view = this.#view;
    const page = await this.#page({ after: newest });
    if (view !== this.#view) return;
    if (!page.ok) return void (this.error = page.error);
    for (const m of page.data.messages) this.#store(m);
    this.#gapsFrom(page.data.gaps);
    this.hasNewer = page.data.hasMore;
    if (!this.hasNewer) await this.#reachedNewest();
  };

  /**
   * Leaves the older messages for the newest page; before the pages around the message the pane
   * opened at are here, the newest page is the first page instead
   */
  latest = async () => {
    if (!this.hasNewer) return;
    const view = ++this.#view;
    const page = await this.#page({});
    if (view !== this.#view) return;
    if (!page.ok) return void (this.error = page.error);
    this.error = "";
    for (const m of this.messages.rows.filter((m) => m.localId > 0)) {
      this.messages.remove(String(m.localId));
    }
    this.gaps = [];
    this.at = null;
    this.hasNewer = false;
    for (const m of page.data.messages) this.#store(m);
    this.#gapsFrom(page.data.gaps);
    this.hasMore = page.data.hasMore;
    if (this.loaded) this.returned += 1;
    else this.#opened();
  };

  /**
   * Empties the view, the stored history kept: opening the chat again shows it, and messages
   * that arrive meanwhile show as usual
   */
  clearView = () => {
    this.#view += 1;
    for (const m of this.messages.rows.filter((m) => m.localId > 0)) {
      this.messages.remove(String(m.localId));
    }
    this.gaps = [];
    this.unreadFrom = null;
    this.at = null;
    this.hasMore = false;
    this.hasNewer = false;
    this.loaded = true;
  };

  /** Sends `text`; it shows at once, marked until the daemon has it, failed if it can't */
  send = async (text: string) => {
    // what we say goes after the newest messages, which the pane goes to first
    if (this.hasNewer) await this.latest();
    const pending: Message = {
      localId: -++sent,
      timestamp: new Date().toISOString(),
      author: this.#self(),
      self: true,
      text: this.target.kind === "user" ? oneLine(text) : text,
      mention: false,
      hidden: false,
      replayed: false,
      unread: false,
      sending: "sending"
    };
    // the row the list keeps, which `#deliver` marks as the request ends
    return this.#deliver(this.messages.upsert(pending));
  };

  /** Tries a failed message again */
  retry = (message: Message) => {
    const row = this.messages.get(String(message.localId));
    if (!row) return;
    this.#unechoed.delete(row.localId);
    row.sending = "sending";
    return this.#deliver(row);
  };

  /** Tells the daemon the newest messages are read: acknowledged or the read marker moved */
  markRead = async () => {
    if (this.#reading) return;
    this.#reading = true;
    try {
      await this.#markRead();
    } finally {
      this.#reading = false;
    }
  };

  /** a read is being told to the daemon; another asked for meanwhile is skipped */
  #reading = false;
  /** room messages the daemon took that the server never echoed, shown as not sent */
  #unechoed = new Set<number>();

  async #markRead() {
    const { kind, name } = this.target;
    if (kind === "user") {
      if (!chat.unreadOf(name) && !this.messages.rows.some((m) => m.unread)) return;
      const outcome = await chat.acknowledge(name);
      if (outcome.ok) for (const m of this.messages.rows) if (m.unread) m.unread = false;
      return;
    }
    // read up to the newest message unless the marker is there already; the unread counts can
    // lag behind the messages shown, so they don't decide it
    const room = chat.rooms.get(name);
    const newest = this.ordered.findLast((m) => m.localId > 0)?.localId;
    // a room we are not in has no marker to move
    if (room && newest !== undefined && newest > room.lastReadId) {
      await chat.markRoomRead(name, newest);
    }
  }

  async #deliver(row: Message) {
    const { kind, name } = this.target;
    const outcome: Outcome<unknown> =
      kind === "user"
        ? await call(() =>
            api.POST("/api/v0/conversations/{username}", { params: byUser(name), body: row.text })
          )
        : await call(() =>
            api.POST("/api/v0/rooms/joined/{roomName}/messages", {
              params: byRoom(name),
              body: row.text
            })
          );
    if (!outcome.ok) {
      row.sending = "failed";
      return outcome;
    }
    // sent: the row stays, unmarked, until the stored message replaces it. A room's is stored
    // only when the server echoes it, and one that doesn't come in time shows as not sent
    row.sending = undefined;
    if (kind === "room") {
      setTimeout(() => {
        if (row.sending !== undefined || !this.messages.get(String(row.localId))) return;
        this.#unechoed.add(row.localId);
        row.sending = "failed";
      }, ECHO_WAIT);
    }
    return outcome;
  }

  /** A message from a frame, which goes on the end of a pane holding the newest messages */
  #arrived(m: Message) {
    if (!this.hasNewer) this.#store(m);
  }

  /** Reads the page ending at the message `localId` and the page after it, as one page */
  async #around(localId: number): Promise<Outcome<Page & { hasNewer: boolean }>> {
    const [upTo, after] = await Promise.all([
      this.#page({ before: localId + 1 }),
      this.#page({ after: localId })
    ]);
    if (!upTo.ok) return upTo;
    if (!after.ok) return after;
    return {
      ok: true,
      data: {
        messages: [...upTo.data.messages, ...after.data.messages],
        gaps: [...upTo.data.gaps, ...after.data.gaps],
        hasMore: upTo.data.hasMore,
        hasNewer: after.data.hasMore
      }
    };
  }

  /**
   * Keeps a stored message; one of ours takes the place of the oldest row still waiting for
   * it, the one sent with the same text, or else of one whose echo was given up on
   */
  #store(m: Message) {
    this.messages.upsert(m);
    if (!m.self) return;
    const mine = (r: Message) => r.localId < 0 && r.text === m.text;
    const waiting =
      this.ordered.find((r) => mine(r) && r.sending !== "failed") ??
      this.ordered.find((r) => mine(r) && this.#unechoed.has(r.localId));
    if (!waiting) return;
    this.#unechoed.delete(waiting.localId);
    this.messages.remove(String(waiting.localId));
  }

  async #readConversation() {
    const outcome = await call(() =>
      api.GET("/api/v0/conversations/{username}", {
        params: {
          path: { username: this.target.name },
          query: { ...byUser(this.target.name).query, includeMessages: false }
        }
      })
    );
    if (outcome.ok) this.conversation = outcome.data;
    // a conversation not started yet has nothing stored, which is no error
    else if (outcome.status !== 404) this.error = outcome.error;
  }

  /** The first unread message: unacknowledged in a conversation, after a room's read marker */
  #firstUnread(): number | null {
    const rows = this.ordered;
    if (this.target.kind === "user") return rows.find((m) => m.unread)?.localId ?? null;
    const marker = chat.rooms.get(this.target.name)?.lastReadId;
    if (marker === undefined) return null;
    return rows.find((m) => m.localId > marker && !m.self && !m.hidden)?.localId ?? null;
  }

  #gapsFrom(gaps: Gap[]) {
    const known = new Set(this.gaps.map((g) => `${g.from}|${g.to}`));
    const fresh = gaps.filter((g) => !known.has(`${g.from}|${g.to}`));
    if (fresh.length) {
      const replaced = new Set(fresh.map((g) => g.from));
      this.gaps = [...this.gaps.filter((g) => !replaced.has(g.from) || g.to), ...fresh];
    }
  }

  async #page(cursor: { before?: number; after?: number }): Promise<Outcome<Page>> {
    const { kind, name } = this.target;
    const none = { ok: true as const, data: { messages: [], hasMore: false, gaps: [] } };
    if (kind === "user") {
      const outcome = await call(() =>
        api.GET("/api/v0/conversations/{username}/messages", {
          params: {
            path: { username: name },
            query: { ...byUser(name).query, ...cursor, limit: PAGE }
          }
        })
      );
      // no conversation yet: an empty one, which the first message starts
      if (!outcome.ok && outcome.status === 404) return none;
      if (!outcome.ok) return outcome;
      const self = this.#self();
      return {
        ok: true,
        data: {
          messages: outcome.data.map((m) => fromPrivate(m, self)),
          hasMore: outcome.data.length === PAGE,
          gaps: []
        }
      };
    }
    const params = {
      path: { roomName: name },
      query: { ...byRoom(name).query, ...cursor, limit: PAGE }
    };
    const outcome = chat.rooms.get(name)
      ? await call(() => api.GET("/api/v0/rooms/joined/{roomName}/messages", { params }))
      : await call(() => api.GET("/api/v0/rooms/history/{roomName}/messages", { params }));
    // a room known by name that never stored a message: an empty one, until it is joined
    if (!outcome.ok && outcome.status === 404) return none;
    if (!outcome.ok) return outcome;
    return {
      ok: true,
      data: {
        messages: outcome.data.messages.map(fromRoom),
        hasMore: outcome.data.hasMore,
        gaps: outcome.data.gaps
      }
    };
  }
}
