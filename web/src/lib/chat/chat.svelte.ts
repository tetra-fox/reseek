import { api, byRoom, byUser, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/lists/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";
import { users } from "#lib/users/users.svelte.js";

import { SERVER } from "./notices";
import { notifier } from "./notify.svelte";
import { pins } from "./pins.svelte";

export type Conversation = Schemas["Conversation"];
export type JoinedRoom = Schemas["JoinedRoom"];
export type ChatUnread = Schemas["ChatUnread"];

/** How many conversation peers have their summaries read at once, the newest conversations first */
const PEERS = 200;

/** The key of a conversation's or a room's pane */
export const paneKey = (kind: "user" | "room", name: string) => `${kind}:${name}`;

/** The pane a key names, the other way from `paneKey` */
export const paneOf = (key: string): { kind: "user" | "room"; name: string } => {
  const cut = key.indexOf(":");
  return { kind: key.slice(0, cut) === "room" ? "room" : "user", name: key.slice(cut + 1) };
};

/**
 * Every conversation and joined room with their unread counts, kept while the app is open so the
 * navigation's badge and the notifications work on any screen. Conversations and rooms are
 * loaded, then patched by their frames; `chat.unread` frames carry the counts
 */
class Chat {
  /** the open conversations, by peer */
  readonly conversations = new Keyed<Conversation>((c) => c.username);
  /** the joined rooms, by name */
  readonly rooms = new Keyed<JoinedRoom>((r) => r.name);
  /** the newest stored notice from the server, held apart as it is no conversation */
  latestNotice = $state.raw<Schemas["PrivateMessage"] | null>(null);
  /**
   * the counts from the last `chat.unread` frame, null until one came. They are current only
   * while the stream is up; while it is down the rows, which reads and the actions here keep,
   * are counted instead
   */
  unread = $state.raw<ChatUnread | null>(null);
  /** the pane on screen, by pane key, which gets no notification while the tab is visible */
  openPane = $state<string | null>(null);
  /** why the last read of the conversations, the rooms or both failed, empty when neither did */
  error = $state("");
  #release: (() => void)[] = [];
  /** counts the frames about the notices, which are newer than any read already on its way */
  #noticeFrames = 0;

  constructor() {
    live.on("conversations.list", (list) => {
      this.conversations.reconcile(list);
      this.#peers();
      // the list leaves the server's notices out, and a lost stream may have missed one
      void this.#readNotices();
    });
    live.on("conversations.update", ({ conversation }) => this.#upsertConversation(conversation));
    live.on("conversations.message", ({ username, message }) => {
      if (username === SERVER) {
        this.#noticeFrames += 1;
        if ((this.latestNotice?.localId ?? 0) <= message.localId) this.latestNotice = message;
        return;
      }
      const conversation = this.conversations.get(username);
      if (conversation && (conversation.lastMessage?.localId ?? 0) <= message.localId) {
        this.conversations.assign(username, { lastMessage: message });
      }
      void notifier.privateMessage(
        username,
        message,
        this.conversations.get(username)?.notify ?? "all",
        this.openPane
      );
    });
    live.on("rooms.list", (list) => this.rooms.reconcile(list));
    live.on("rooms.message", ({ roomName, message }) => {
      const room = this.rooms.get(roomName);
      if (room) {
        // a frame older than what the row shows moves nothing; a hidden message is shown
        // collapsed, so the row goes on showing the one before it
        const later =
          !room.lastMessageAt || Date.parse(room.lastMessageAt) <= Date.parse(message.timestamp);
        const newest = !message.hidden && (room.lastMessage?.localId ?? 0) <= message.localId;
        this.rooms.assign(roomName, {
          ...(later ? { lastMessageAt: message.timestamp } : {}),
          ...(newest ? { lastMessage: message } : {})
        });
      }
      void notifier.roomMessage(roomName, message, room?.notify ?? "mentions", this.openPane);
    });
    live.on("chat.unread", (counts) => (this.unread = counts));
    live.onAdded((topics) => {
      if (topics.includes("conversations") || topics.includes("rooms")) void this.load();
    });
  }

  /** the counts from the frames while they come, null while the rows are counted */
  get #counts(): ChatUnread | null {
    return live.status === "connected" ? this.unread : null;
  }

  /** The total for the navigation's badge: unread private messages and room mentions */
  get total(): number {
    if (this.#counts) return this.#counts.total;
    let n = 0;
    for (const c of this.conversations.rows) n += c.unAcknowledgedMessageCount;
    for (const r of this.rooms.rows) n += r.mentions;
    return n;
  }

  /** The unread room mentions, which turn the badge to the accent color */
  get totalMentions(): number {
    return this.#counts?.totalMentions ?? this.rooms.rows.reduce((n, r) => n + r.mentions, 0);
  }

  /** A conversation's unread messages */
  unreadOf(username: string): number {
    return (
      this.#counts?.conversations[username] ??
      this.conversations.get(username)?.unAcknowledgedMessageCount ??
      0
    );
  }

  /** A room's unread messages and mentions */
  roomUnread(name: string): { unread: number; mentions: number } {
    const room = this.rooms.get(name);
    return (
      this.#counts?.rooms[name] ?? { unread: room?.unread ?? 0, mentions: room?.mentions ?? 0 }
    );
  }

  /** Follows conversations and rooms on the event stream while the app is open */
  start() {
    if (this.#release.length) return;
    this.#release.push(live.retain("conversations"), live.retain("rooms"));
    void this.load();
  }

  /** Stops following them, as the session ends */
  stop() {
    for (const release of this.#release.splice(0)) release();
  }

  /** Forgets everything held for the session that ended */
  clear() {
    this.stop();
    this.#loading = null;
    this.conversations.clear();
    this.rooms.clear();
    this.latestNotice = null;
    this.error = "";
    this.unread = null;
    pins.clear();
  }

  /** the read of the conversations and rooms under way, or the last one */
  #loading: Promise<void> | null = null;

  /** Resolves once the conversations and rooms have been read, reading them if nobody has */
  ready = (): Promise<void> => this.#loading ?? this.load();

  /** Reads the open conversations and the joined rooms */
  load = (): Promise<void> => (this.#loading = this.#read());

  #read = async () => {
    const turns = { conversations: this.conversations.reading(), rooms: this.rooms.reading() };
    const [conversations, rooms] = await Promise.all([
      call(() => api.GET("/api/v0/conversations")),
      call(() => api.GET("/api/v0/rooms/joined")),
      this.#readNotices(),
      pins.read()
    ]);
    // a read that began after this one answers with what is newer, and says why it failed; the
    // two turns begin together, so one check covers both
    if (!this.conversations.latest(turns.conversations)) return;
    if (conversations.ok) {
      this.conversations.reconcile(conversations.data, turns.conversations);
      this.#peers();
    } else {
      this.conversations.abandon(turns.conversations);
    }
    if (rooms.ok) this.rooms.reconcile(rooms.data, turns.rooms);
    else this.rooms.abandon(turns.rooms);
    const reasons = [];
    if (!conversations.ok) reasons.push(`Conversations: ${conversations.error}`);
    if (!rooms.ok) reasons.push(`Rooms: ${rooms.error}`);
    this.error = reasons.join(". ");
  };

  /**
   * Reads the newest notice. This asks for the one conversation, not the list, because a history
   * cleared stays closed and the list leaves it out when a new notice comes. A read that fails
   * leaves what is held; the next list frame reads it again. A frame that comes while it is on
   * its way says more than it can, so it is dropped: opening a screen reads the notices just as
   * something done there may bring one
   */
  #readNotices = async () => {
    const frames = this.#noticeFrames;
    const outcome = await call(() =>
      api.GET("/api/v0/conversations/{username}", {
        params: {
          path: { username: SERVER },
          query: { ...byUser(SERVER).query, includeMessages: false }
        }
      })
    );
    if (frames !== this.#noticeFrames) return;
    if (outcome.ok) this.latestNotice = outcome.data.lastMessage ?? null;
    else if (outcome.status === 404) this.latestNotice = null;
  };

  /**
   * Tells the server we read a conversation. Its count drops here at once, so the pane does not
   * tell it again before the counts' frame comes
   */
  acknowledge = async (username: string) => {
    const outcome = await call(() =>
      api.PUT("/api/v0/conversations/{username}", { params: byUser(username) })
    );
    if (!outcome.ok) return outcome;
    const row = this.conversations.get(username);
    if (row) {
      row.unAcknowledgedMessageCount = 0;
      row.hasUnAcknowledgedMessages = false;
    }
    if (this.unread && username in this.unread.conversations) {
      const { [username]: read, ...others } = this.unread.conversations;
      this.unread = { ...this.unread, total: this.unread.total - read, conversations: others };
    }
    return outcome;
  };

  /**
   * Moves a room's read marker to `localId`, never back. A marker at the newest message the row
   * shows leaves nothing unread here at once
   */
  markRoomRead = async (name: string, localId: number) => {
    const outcome = await call(() =>
      api.PUT("/api/v0/rooms/joined/{roomName}/read", { params: byRoom(name), body: { localId } })
    );
    const room = this.rooms.get(name);
    if (!outcome.ok || !room) return outcome;
    const all = localId >= (room.lastMessage?.localId ?? 0);
    this.rooms.assign(name, {
      lastReadId: Math.max(room.lastReadId, localId),
      ...(all ? { unread: 0, mentions: 0 } : {})
    });
    const counted = this.unread?.rooms[name];
    if (all && this.unread && counted) {
      this.unread = {
        ...this.unread,
        total: this.unread.total - counted.mentions,
        totalMentions: this.unread.totalMentions - counted.mentions,
        rooms: { ...this.unread.rooms, [name]: { unread: 0, mentions: 0 } }
      };
    }
    return outcome;
  };

  /** Moves a room's read marker to its newest stored message */
  markRoomReadNewest = async (name: string) => {
    const page = await call(() =>
      api.GET("/api/v0/rooms/joined/{roomName}/messages", {
        params: { path: byRoom(name).path, query: { ...byRoom(name).query, limit: 1 } }
      })
    );
    const newest = page.ok ? page.data.messages.at(-1)?.localId : undefined;
    return newest === undefined ? page : this.markRoomRead(name, newest);
  };

  /** Deletes a room's stored history */
  clearRoom = (name: string) =>
    call(() => api.DELETE("/api/v0/rooms/history/{roomName}", { params: byRoom(name) }));

  /** Closes a conversation; the history stays unless `purge` */
  close = async (username: string, purge = false) => {
    const outcome = await call(() =>
      api.DELETE("/api/v0/conversations/{username}", {
        params: { path: byUser(username).path, query: { ...byUser(username).query, purge } }
      })
    );
    // a conversation already gone is closed all the same
    if (outcome.ok || outcome.status === 404) {
      this.conversations.remove(username);
      if (purge && username === SERVER) this.latestNotice = null;
    }
    return outcome;
  };

  /** Leaves a room */
  leave = async (name: string) => {
    const outcome = await call(() =>
      api.DELETE("/api/v0/rooms/joined/{roomName}", { params: byRoom(name) })
    );
    if (outcome.ok || outcome.status === 404) this.rooms.remove(name);
    return outcome;
  };

  /** Joins a room, creating it, private when asked, if it doesn't exist */
  join = async (name: string, isPrivate: boolean) => {
    const outcome = await call(() =>
      api.POST("/api/v0/rooms/joined", { body: { name, private: isPrivate } })
    );
    if (outcome.ok) await this.load();
    return outcome;
  };

  /** Sets which messages of a conversation or room notify */
  setNotify = async (kind: "user" | "room", name: string, notify: Schemas["NotifyLevel"]) => {
    const outcome =
      kind === "user"
        ? await call(() =>
            api.PUT("/api/v0/conversations/{username}/notify", {
              params: byUser(name),
              body: { notify }
            })
          )
        : await call(() =>
            api.PUT("/api/v0/rooms/joined/{roomName}/notify", {
              params: byRoom(name),
              body: { notify }
            })
          );
    if (outcome.ok) {
      const row = kind === "user" ? this.conversations.get(name) : this.rooms.get(name);
      if (row) row.notify = notify;
    }
    return outcome;
  };

  #upsertConversation(conversation: Conversation) {
    if (conversation.isServerNotice) {
      this.#noticeFrames += 1;
      this.latestNotice = conversation.lastMessage ?? null;
      return;
    }
    if (!conversation.isActive) return this.conversations.remove(conversation.username);
    // a frame carries no messages; the list never holds them either
    this.conversations.upsert({ ...conversation, messages: null });
    void users.load(conversation.username);
  }

  /**
   * Reads the summaries of the conversation peers, for their buddy mark, presence, block and mute
   * on any screen. A summary already held and less than a minute old is not read again
   */
  #peers() {
    const newest = this.conversations.rows.toSorted(
      (a, b) => (b.lastMessage?.localId ?? 0) - (a.lastMessage?.localId ?? 0)
    );
    for (const c of newest.slice(0, PEERS)) void users.load(c.username);
  }
}

export const chat = new Chat();
