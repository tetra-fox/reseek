import { goto } from "$app/navigation";

import type { Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { chatHref, roomHref } from "#lib/nav.js";
import { fields } from "#lib/stored.svelte.js";
import { toast } from "#lib/toast.svelte.js";
import { users } from "#lib/users.svelte.js";

/** The `chat.notifications` options */
export type NotifyOptions = {
  enabled: boolean;
  onlyWhenHidden: boolean;
  sound: boolean;
  privateMessages: boolean;
  mentions: boolean;
  roomMessages: boolean;
};

/** The `chat.notifications` options in effect, their defaults where the options don't say */
export const notifyOptions = (): NotifyOptions => {
  const n = fields(fields(fields(app.options).chat).notifications);
  const flag = (value: unknown, otherwise: boolean) =>
    typeof value === "boolean" ? value : otherwise;
  return {
    enabled: flag(n.enabled, true),
    onlyWhenHidden: flag(n.only_when_hidden, true),
    sound: flag(n.sound, false),
    privateMessages: flag(n.private_messages, true),
    mentions: flag(n.mentions, true),
    roomMessages: flag(n.room_messages, false)
  };
};

/** One arriving message, as far as notifying about it goes */
export type Arrival = {
  kind: "private" | "room";
  /** the conversation's or room's level */
  level: Schemas["NotifyLevel"];
  mention: boolean;
  /** the author is muted */
  muted: boolean;
  /** the tab is visible and the message's pane is open */
  seen: boolean;
};

/** Whether an arriving message gets a notification */
export const shouldNotify = (a: Arrival, o: NotifyOptions): boolean => {
  if (!o.enabled || a.muted || a.level === "none") return false;
  if (o.onlyWhenHidden && a.seen) return false;
  if (a.kind === "private") return o.privateMessages;
  if (a.mention) return o.mentions;
  return o.roomMessages && a.level === "all";
};

/**
 * Turns arriving messages into notifications: the browser's, a short sound, and for a private
 * message while its pane isn't open a toast that opens it
 */
class Notifier {
  /** the browser lets the page notify; asked for from a click, never on its own */
  permission = $state<NotificationPermission | "unsupported">(
    typeof Notification === "undefined" ? "unsupported" : Notification.permission
  );

  /** Asks the browser for permission to notify */
  ask = async () => {
    if (typeof Notification === "undefined") return;
    this.permission = await Notification.requestPermission();
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
    const seen = this.#seen(key, open);
    // the toast is the in-page form of the same notification: the same level, mute and options
    // decide it, whether the tab is visible or not
    const toasts = (a: Arrival) =>
      !seen.paneOpen && shouldNotify(a, { ...notifyOptions(), onlyWhenHidden: false });
    const arrival: Arrival = {
      kind: "private",
      level,
      mention: false,
      muted: false,
      seen: seen.visible && seen.paneOpen
    };
    if (!toasts(arrival) && !shouldNotify(arrival, notifyOptions())) return;
    arrival.muted = await this.#muted(username);
    if (toasts(arrival)) {
      toast(`New message from ${username}`, "info", {
        action: { label: "Open", run: () => void this.#go(chatHref(username)) }
      });
    }
    this.#maybe(arrival, username, message.message, key, chatHref(username));
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
    const seen = this.#seen(key, open);
    const arrival: Arrival = {
      kind: "room",
      level,
      mention: message.isMention,
      muted: false,
      seen: seen.visible && seen.paneOpen
    };
    if (!shouldNotify(arrival, notifyOptions())) return;
    arrival.muted = await this.#muted(message.username);
    this.#maybe(arrival, `${message.username} in ${room}`, message.message, key, roomHref(room));
  }

  /**
   * Notifies of something besides chat, such as a finished download, under the same switch,
   * browser permission and hidden-tab rule as chat's notifications
   */
  other(title: string, body: string, tag: string, to: string) {
    const options = notifyOptions();
    if (!options.enabled) return;
    if (options.onlyWhenHidden && document.visibilityState === "visible") return;
    if (options.sound) chime();
    if (this.permission !== "granted") return;
    const shown = new Notification(title, { body, tag });
    shown.onclick = () => this.#go(to);
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

  #maybe(arrival: Arrival, title: string, body: string, tag: string, to: string) {
    const options = notifyOptions();
    if (!shouldNotify(arrival, options)) return;
    if (options.sound) chime();
    if (this.permission !== "granted") return;
    const shown = new Notification(title, { body, tag });
    shown.onclick = () => this.#go(to);
  }
}

export const notifier = new Notifier();

/** A short tone, made here rather than shipped as a file */
const chime = () => {
  const audio = new AudioContext();
  const tone = audio.createOscillator();
  const level = audio.createGain();
  tone.frequency.value = 880;
  level.gain.setValueAtTime(0.15, audio.currentTime);
  level.gain.exponentialRampToValueAtTime(0.001, audio.currentTime + 0.25);
  tone.connect(level).connect(audio.destination);
  tone.start();
  tone.stop(audio.currentTime + 0.25);
  tone.onended = () => void audio.close();
};
