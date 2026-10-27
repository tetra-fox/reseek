import type { Schemas } from "#lib/api/client.js";

/** One message of a room or a conversation, as the message list shows it */
export type Message = {
  /** the stored message's id, or a negative one for a message still being sent */
  localId: number;
  timestamp: string;
  author: string;
  self: boolean;
  text: string;
  /** a room message that mentions us */
  mention: boolean;
  /** from a user blocked for rooms, shown collapsed */
  hidden: boolean;
  /** delivered again by the server because it was not acknowledged before */
  replayed: boolean;
  /** an inbound private message not acknowledged yet */
  unread: boolean;
  /** a message we sent that the daemon has not stored yet, or failed to send */
  sending?: "sending" | "failed";
};

export type Gap = Schemas["HistoryGap"];

/** A line of the message list */
export type Line =
  | { kind: "day"; key: string; date: string }
  | { kind: "gap"; key: string; gap: Gap }
  | { kind: "unread"; key: "unread" }
  | { kind: "message"; key: string; message: Message; head: boolean };

/** Messages from one author this close together share one header */
export const GROUP_MS = 5 * 60_000;

/** A private message as the list shows it; the author of an inbound one is the peer */
export const fromPrivate = (m: Schemas["PrivateMessage"], self: string): Message => ({
  localId: m.localId,
  timestamp: m.timestamp,
  author: m.direction === "Out" ? self : m.username,
  self: m.direction === "Out",
  text: m.message,
  mention: false,
  hidden: false,
  replayed: m.wasReplayed,
  unread: m.direction === "In" && !m.isAcknowledged
});

/** A room message as the list shows it */
export const fromRoom = (m: Schemas["RoomMessage"]): Message => ({
  localId: m.localId,
  timestamp: m.timestamp,
  author: m.username,
  self: m.self,
  text: m.message,
  mention: m.isMention,
  hidden: m.hidden,
  replayed: false,
  unread: false
});

const day = (iso: string) => new Date(iso).toDateString();

/**
 * The list's lines for messages in the order they arrived: a day line where the day changes, a
 * gap line where history is missing, in time order, an unread line before `unreadFrom`, and each
 * message with whether it starts a group (a new author, a gap, a new day, or five minutes since
 * the one before). A gap still open, with no end, goes last
 */
export const layout = (messages: Message[], gaps: Gap[], unreadFrom: number | null): Line[] => {
  const lines: Line[] = [];
  const pending = gaps.toSorted((a, b) => Date.parse(a.from) - Date.parse(b.from));
  let lastDay: string | null = null;
  let previous: Message | null = null;
  const placeGaps = (before: number) => {
    while (pending.length && Date.parse(pending[0].from) < before) {
      const gap = pending.shift()!;
      lines.push({ kind: "gap", key: `gap:${gap.from}`, gap });
      previous = null;
    }
  };
  for (const message of messages) {
    const at = Date.parse(message.timestamp);
    placeGaps(at);
    const today = day(message.timestamp);
    if (today !== lastDay) {
      // keyed by the message that starts it: a replayed message dated earlier can start a day
      // a second time, and two lines would then share a date
      lines.push({ kind: "day", key: `day:${message.localId}`, date: message.timestamp });
      lastDay = today;
      previous = null;
    }
    if (unreadFrom !== null && message.localId === unreadFrom) {
      lines.push({ kind: "unread", key: "unread" });
      previous = null;
    }
    const head =
      !previous ||
      previous.author !== message.author ||
      at - Date.parse(previous.timestamp) > GROUP_MS;
    lines.push({ kind: "message", key: `m:${message.localId}`, message, head });
    previous = message;
  }
  placeGaps(Infinity);
  return lines;
};
