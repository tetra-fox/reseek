import { SvelteMap } from "svelte/reactivity";

import { api, byRoom, byUser, type Schemas } from "#lib/api/client.js";
import { Keyed } from "#lib/keyed.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { call } from "#lib/request.js";

import { notifier } from "./notify.svelte";

export type Conversation = Schemas["Conversation"];
export type JoinedRoom = Schemas["JoinedRoom"];
export type ChatUnread = Schemas["ChatUnread"];

/** What the sidebar shows under a conversation or room: its latest message seen in this page */
export type Preview = { author: string; text: string; at: string };

/** The key of a conversation's or a room's pane */
export const paneKey = (kind: "user" | "room", name: string) => `${kind}:${name}`;

/**
 * Every conversation and joined room with their unread counts, kept while the app is open so the
 * navigation's badge and the notifications work on any screen. Conversations and rooms are
 * loaded, then patched by their frames; `chat.unread` frames carry the counts
 */
class Chat {
  readonly conversations = new Keyed<Conversation>((c) => c.username);
  readonly rooms = new Keyed<JoinedRoom>((r) => r.name);
  /** the counts from the last `chat.unread` frame, null until one came */
  unread = $state.raw<ChatUnread | null>(null);
  /** the latest message of each pane seen since the page loaded, by pane key */
  readonly previews = new SvelteMap<string, Preview>();
  /** the pane on screen, by pane key, which gets no notification while the tab is visible */
  openPane = $state<string | null>(null);
  #release: (() => void)[] = [];

  constructor() {
    live.on("conversations.list", (list) => this.conversations.reconcile(list));
    live.on("conversations.update", ({ conversation }) => this.#upsertConversation(conversation));
    live.on("conversations.message", ({ username, message }) => {
      this.#preview(paneKey("user", username), {
        author: message.direction === "Out" ? "" : username,
        text: message.message,
        at: message.timestamp
      });
      notifier.privateMessage(
        username,
        message,
        this.conversations.get(username)?.notify ?? "all",
        this.openPane
      );
    });
    live.on("rooms.list", (list) => this.rooms.reconcile(list));
    live.on("rooms.message", ({ roomName, message }) => {
      this.#preview(paneKey("room", roomName), {
        author: message.username,
        text: message.message,
        at: message.timestamp
      });
      const room = this.rooms.get(roomName);
      if (room) room.lastMessageAt = message.timestamp;
      notifier.roomMessage(roomName, message, room?.notify ?? "mentions", this.openPane);
    });
    live.on("chat.unread", (counts) => (this.unread = counts));
  }

  /** The total for the navigation's badge: unread private messages and room mentions */
  get total(): number {
    if (this.unread) return this.unread.total;
    let n = 0;
    for (const c of this.conversations.rows)
      n += c.isServerNotice ? 0 : c.unAcknowledgedMessageCount;
    for (const r of this.rooms.rows) n += r.mentions;
    return n;
  }

  get totalMentions(): number {
    return this.unread?.totalMentions ?? this.rooms.rows.reduce((n, r) => n + r.mentions, 0);
  }

  /** A conversation's unread messages */
  unreadOf(username: string): number {
    return (
      this.unread?.conversations[username] ??
      this.conversations.get(username)?.unAcknowledgedMessageCount ??
      0
    );
  }

  /** A room's unread messages and mentions */
  roomUnread(name: string): { unread: number; mentions: number } {
    const room = this.rooms.get(name);
    return this.unread?.rooms[name] ?? { unread: room?.unread ?? 0, mentions: room?.mentions ?? 0 };
  }

  /** Follows conversations and rooms on the event stream while the app is open */
  start() {
    if (this.#release.length) return;
    this.#release.push(live.retain("conversations"), live.retain("rooms"));
    void this.load();
  }

  stop() {
    for (const release of this.#release.splice(0)) release();
  }

  clear() {
    this.stop();
    this.conversations.clear();
    this.rooms.clear();
    this.unread = null;
    this.previews.clear();
  }

  /** Reads the open conversations and the joined rooms */
  load = async () => {
    const [conversations, rooms] = await Promise.all([
      call(() => api.GET("/api/v0/conversations")),
      call(() => api.GET("/api/v0/rooms/joined"))
    ]);
    if (conversations.ok) this.conversations.reconcile(conversations.data);
    if (rooms.ok) this.rooms.reconcile(rooms.data);
  };

  /** Tells the server we read a conversation; the counts follow by frame */
  acknowledge = (username: string) =>
    call(() => api.PUT("/api/v0/conversations/{username}", { params: byUser(username) }));

  /** Moves a room's read marker to `localId` */
  markRoomRead = (name: string, localId: number) =>
    call(() =>
      api.PUT("/api/v0/rooms/joined/{roomName}/read", { params: byRoom(name), body: { localId } })
    );

  /** Moves a room's read marker to its newest stored message */
  markRoomReadNewest = async (name: string) => {
    const page = await call(() =>
      api.GET("/api/v0/rooms/joined/{roomName}/messages", {
        params: { path: { roomName: name }, query: { ...byRoom(name).query, limit: 1 } }
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
        params: { path: { username }, query: { ...byUser(username).query, purge } }
      })
    );
    // a conversation already gone is closed all the same
    if (outcome.ok || outcome.status === 404) this.conversations.remove(username);
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
    if (!conversation.isActive) return this.conversations.remove(conversation.username);
    // a frame carries no messages; the list never holds them either
    this.conversations.upsert({ ...conversation, messages: null });
  }

  #preview(key: string, preview: Preview) {
    this.previews.set(key, preview);
  }
}

export const chat = new Chat();
