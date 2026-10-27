import { api, byRoom, byUser, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call, type Outcome } from "#lib/request.js";

import { chat } from "./chat.svelte";
import { fromPrivate, fromRoom, type Gap, type Message } from "./messages";

/** What a pane shows: the conversation with a user, or a room */
export type Target = { kind: "user"; name: string } | { kind: "room"; name: string };

/** How many messages one page of history holds */
export const PAGE = 100;

let sent = 0;

/** How long a room message the daemon took waits for the server's echo before it shows as not sent */
export const ECHO_WAIT = 30_000;

/** 1 for a message still being sent, which goes after the stored ones */
const sending = (m: Message) => (m.localId < 0 ? 1 : 0);

/**
 * The messages of one conversation or room, oldest first: the newest page on open, older pages
 * as the list scrolls up, and new ones as frames arrive. Messages are kept by their id and never
 * change once shown, so the list keeps its nodes. A message we send shows at once and stays
 * until the stored one with its text arrives, which for a room is when the server echoes it; a
 * room message with no echo in time shows as not sent
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
  /** the first page is here */
  loaded = $state(false);
  /** why the last read failed, empty when it didn't */
  error = $state("");
  /** where the unread messages started when the pane opened, null when none were unread */
  unreadFrom = $state<number | null>(null);
  /** the conversation as stored, for its draft; null for a room or a conversation not started */
  conversation = $state.raw<Schemas["Conversation"] | null>(null);
  #self: () => string;

  /** the messages in order: the stored ones by id, then those still being sent */
  readonly ordered = $derived(
    this.messages.rows.toSorted(
      (a, b) => sending(a) - sending(b) || Math.abs(a.localId) - Math.abs(b.localId)
    )
  );

  constructor(target: Target, self: () => string) {
    this.target = target;
    this.#self = self;
  }

  /** Follows the pane's frames; call from a component's setup, it stops with the component */
  follow() {
    const { kind, name } = this.target;
    if (kind === "user") {
      $effect(() =>
        live.on("conversations.message", ({ username, message }) => {
          if (username === name) this.#store(fromPrivate(message, this.#self()));
        })
      );
      return;
    }
    $effect(() =>
      live.on("rooms.message", ({ roomName, message }) => {
        if (roomName === name) this.#store(fromRoom(message));
      })
    );
    // a session that began after a gap says so; an open gap is replaced by its end
    $effect(() =>
      live.on("rooms.gap", (gap) => {
        this.gaps = [...this.gaps.filter((g) => g.from !== gap.from), gap];
      })
    );
  }

  /** Reads the newest page; the first time it also finds where the unread messages start */
  load = async () => {
    const page = await this.#page(undefined);
    if (!page.ok) {
      this.error = page.error;
      this.loaded = true;
      return;
    }
    this.error = "";
    for (const m of page.data.messages) this.#store(m);
    this.#gapsFrom(page.data.gaps);
    if (!this.loaded) {
      this.hasMore = page.data.hasMore;
      this.unreadFrom = this.#firstUnread();
      if (this.target.kind === "user") void this.#readConversation();
    }
    this.loaded = true;
  };

  /** Reads the page before the oldest message shown */
  older = async () => {
    const oldest = this.ordered.find((m) => m.localId > 0)?.localId;
    if (oldest === undefined || !this.hasMore) return;
    const page = await this.#page(oldest);
    if (!page.ok) return void (this.error = page.error);
    for (const m of page.data.messages) this.#store(m);
    this.#gapsFrom(page.data.gaps);
    this.hasMore = page.data.hasMore;
  };

  /** Reads older pages until the message `localId` is in the pane, a few at most */
  reach = async (localId: number) => {
    for (
      let pages = 0;
      pages < 20 && this.hasMore && !this.messages.get(String(localId));
      pages++
    ) {
      await this.older();
    }
  };

  /** Sends `text`; it shows at once, marked until the daemon has it, failed if it can't */
  send = async (text: string) => {
    const pending: Message = {
      localId: -++sent,
      timestamp: new Date().toISOString(),
      author: this.#self(),
      self: true,
      text,
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

  /** a read is being told to the daemon; another asked for meanwhile is the same */
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
    const newest = this.ordered.findLast((m) => m.localId > 0)?.localId;
    if (newest !== undefined && chat.roomUnread(name).unread) {
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
    // a conversation not started yet has nothing stored, which is no error
    if (outcome.ok) this.conversation = outcome.data;
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

  async #page(
    before: number | undefined
  ): Promise<Outcome<{ messages: Message[]; hasMore: boolean; gaps: Gap[] }>> {
    const { kind, name } = this.target;
    if (kind === "user") {
      const outcome = await call(() =>
        api.GET("/api/v0/conversations/{username}/messages", {
          params: {
            path: { username: name },
            query: { ...byUser(name).query, before, limit: PAGE }
          }
        })
      );
      // no conversation yet: an empty one, which the first message starts
      if (!outcome.ok && outcome.status === 404) {
        return { ok: true, data: { messages: [], hasMore: false, gaps: [] } };
      }
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
      query: { ...byRoom(name).query, before, limit: PAGE }
    };
    const outcome = chat.rooms.get(name)
      ? await call(() => api.GET("/api/v0/rooms/joined/{roomName}/messages", { params }))
      : await call(() => api.GET("/api/v0/rooms/history/{roomName}/messages", { params }));
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
