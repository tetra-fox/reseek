import { goto } from "$app/navigation";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import type { OptionKey } from "#lib/config/keys.js";
import { toast } from "#lib/feedback/toast.svelte.js";
import { chatHref, roomHref } from "#lib/nav.js";
import { browserTabs } from "#lib/ui/tabs.js";
import { users } from "#lib/users/users.svelte.js";

import { isNotifyMode, type NotifyMode, playsSound, showsDesktop } from "./modes";
import notificationSound from "./notification.wav?url";

/** The notification options, `chat.notifications` and `notifications.*` together */
export type NotifyOptions = {
  onlyWhenHidden: boolean;
  privateMessages: NotifyMode;
  mentions: NotifyMode;
  roomMessages: NotifyMode;
  jobFinished: NotifyMode;
  uploadQueued: NotifyMode;
  buddyPresence: NotifyMode;
  scansFinished: NotifyMode;
  analysisFinished: NotifyMode;
  maintenanceFinished: NotifyMode;
  wishlistFound: NotifyMode;
};

/** The notification options in effect, their defaults where the options don't say */
export const notifyOptions = (): NotifyOptions => {
  const hidden = app.option("chat.notifications.only_when_hidden");
  const mode = (key: OptionKey, otherwise: NotifyMode) => {
    const value = app.option(key);
    return isNotifyMode(value) ? value : otherwise;
  };
  return {
    onlyWhenHidden: typeof hidden === "boolean" ? hidden : true,
    privateMessages: mode("chat.notifications.private_messages", "both"),
    mentions: mode("chat.notifications.mentions", "both"),
    roomMessages: mode("chat.notifications.room_messages", "off"),
    jobFinished: mode("notifications.transfers.job_finished", "both"),
    uploadQueued: mode("notifications.transfers.upload_queued", "off"),
    buddyPresence: mode("notifications.buddies.presence", "both"),
    scansFinished: mode("notifications.jobs.scans", "off"),
    analysisFinished: mode("notifications.jobs.analysis", "off"),
    maintenanceFinished: mode("notifications.jobs.maintenance", "off"),
    wishlistFound: mode("notifications.search.wishlist", "both")
  };
};

/** Whether any type of chat notification asks for desktop notifications */
export const chatWantsDesktop = (o: NotifyOptions) =>
  [o.privateMessages, o.mentions, o.roomMessages].some(showsDesktop);

/** One arriving message, as far as notifying about it goes */
export type Arrival = {
  kind: "private" | "room";
  /** the conversation's or room's level */
  level: Schemas["NotifyLevel"];
  mention: boolean;
  /** the author is muted */
  muted: boolean;
};

/** The mode of an arriving message's type, `off` when a mute or its level leaves it out */
export const arrivalMode = (a: Arrival, o: NotifyOptions): NotifyMode => {
  if (a.muted || a.level === "none") return "off";
  if (a.kind === "private") return o.privateMessages;
  if (a.mention) return o.mentions;
  return a.level === "all" ? o.roomMessages : "off";
};

/** Whether a notification in `mode` goes out, `seen` when what it tells of is already in view */
const shouldNotify = (mode: NotifyMode, seen: boolean, onlyWhenHidden: boolean) =>
  mode !== "off" && !(onlyWhenHidden && seen);

/**
 * Delivers notifications as their type's mode says: the browser's, a short sound, and for a
 * private message while its pane isn't open a toast that opens it
 */
class Notifier {
  /**
   * the browser lets the page notify; asked for from a click, never on its own. `insecure` on a
   * page without a secure context (plain http on a LAN address), where browsers refuse to notify
   */
  permission = $state<NotificationPermission | "unsupported" | "insecure">(
    typeof Notification === "undefined"
      ? "unsupported"
      : isSecureContext
        ? Notification.permission
        : "insecure"
  );

  constructor() {
    if (this.permission !== "unsupported" && this.permission !== "insecure") void this.#follow();
  }

  /** Follows a change made in the browser's site settings, where the browser tells of one */
  async #follow() {
    // a browser without the permissions API, or without this permission in it, has nothing to tell
    const status = await navigator.permissions?.query({ name: "notifications" }).catch(() => null);
    status?.addEventListener("change", () => (this.permission = Notification.permission));
  }

  /** Asks the browser for permission to notify */
  ask = async () => {
    if (this.permission === "unsupported" || this.permission === "insecure") return;
    // an older Safari gives the answer only to a callback and returns no promise; the promise,
    // where there is one, never rejects
    this.permission = await new Promise<NotificationPermission>((answer) => {
      void Notification.requestPermission(answer)?.then(answer);
    });
  };

  /** A private message arrived; `level` is its conversation's, `open` the pane on screen */
  async privateMessage(
    username: string,
    message: Schemas["PrivateMessage"],
    level: Schemas["NotifyLevel"],
    open: string | null
  ) {
    if (message.direction !== "In" || message.wasReplayed) return;
    const key = `user:${username}`;
    const { visible, paneOpen } = this.#seen(key, open);
    const arrival: Arrival = { kind: "private", level, mention: false, muted: false };
    const mode = () => arrivalMode(arrival, notifyOptions());
    const seen = visible && paneOpen;
    // the toast is the in-page form of the same notification: the same level, mute and mode
    // decide it, whether the tab is visible or not
    const toasts = () => !paneOpen && mode() !== "off";
    if (!toasts() && !shouldNotify(mode(), seen, notifyOptions().onlyWhenHidden)) return;
    arrival.muted = await this.#muted(username);
    if (toasts()) {
      // one toast per conversation, which a burst of messages keeps up rather than stacks
      toast(`New message from ${username}`, "info", {
        key,
        action: { label: "Open", run: () => void this.#go(chatHref(username)) }
      });
    }
    this.#deliver(mode(), seen, username, message.message, key, chatHref(username));
  }

  /** A room message arrived; `level` is the room's, `open` the pane on screen */
  async roomMessage(
    room: string,
    message: Schemas["RoomMessage"],
    level: Schemas["NotifyLevel"],
    open: string | null
  ) {
    if (message.self || message.hidden) return;
    const key = `room:${room}`;
    const { visible, paneOpen } = this.#seen(key, open);
    const arrival: Arrival = { kind: "room", level, mention: message.isMention, muted: false };
    const mode = () => arrivalMode(arrival, notifyOptions());
    const seen = visible && paneOpen;
    if (!shouldNotify(mode(), seen, notifyOptions().onlyWhenHidden)) return;
    arrival.muted = await this.#muted(message.username);
    const title = `${message.username} in ${room}`;
    this.#deliver(mode(), seen, title, message.message, key, roomHref(room));
  }

  /**
   * Notifies of something besides chat, such as a finished download, in `mode`, under the same
   * browser permission and hidden-tab rule as chat's notifications
   */
  other(mode: NotifyMode, title: string, body: string, tag: string, to: string) {
    this.#deliver(mode, document.visibilityState === "visible", title, body, tag, to);
  }

  /**
   * Whether `author` is muted, from their summary, read again first when none is held or the one
   * held is over a minute old, so a mute made on a screen that never listed them, or in another
   * browser, keeps them quiet. Asked only for a message that would otherwise notify
   */
  async #muted(author: string): Promise<boolean> {
    await users.load(author);
    return users.get(author)?.muted ?? false;
  }

  #seen(key: string, open: string | null) {
    return { visible: document.visibilityState === "visible", paneOpen: open === key };
  }

  #go(to: string) {
    window.focus();
    void goto(to);
  }

  #deliver(mode: NotifyMode, seen: boolean, title: string, body: string, tag: string, to: string) {
    if (!shouldNotify(mode, seen, notifyOptions().onlyWhenHidden)) return;
    if (playsSound(mode)) chime();
    if (!showsDesktop(mode) || this.permission !== "granted") return;
    const shown = new Notification(title, { body, tag });
    shown.onclick = () => this.#go(to);
  }
}

export const notifier = new Notifier();

/** The least time between two notification sounds, so a burst of messages plays one */
export const CHIME_GAP_MS = 1_000;

/** when the sound last played, by performance.now */
let chimed = -Infinity;

/**
 * Plays the notification sound in the one tab of the browser that leads: every tab hears of the
 * same message or finished download, and a browser without BroadcastChannel has each tab play
 */
const chime = () => {
  if (!(browserTabs()?.leads() ?? true)) return;
  const now = performance.now();
  if (now - chimed < CHIME_GAP_MS) return;
  chimed = now;
  // a browser that refuses until the page is used stays silent
  new Audio(notificationSound).play().catch(() => {});
};
