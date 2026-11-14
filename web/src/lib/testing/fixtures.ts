import type { Schemas } from "#lib/api/client.js";
import type { Conversation, JoinedRoom } from "#lib/chat/chat.svelte.js";
import { newId } from "#lib/ids.js";
import type { Transfer, TransferState } from "#lib/transfers/states.js";

/**
 * Every transfer state, the transient ones first. Written as a record so a state the daemon adds
 * fails the type check until it is listed here
 */
export const TRANSFER_STATES = Object.keys({
  "Requested": true,
  "Queued, Locally": true,
  "Queued, Remotely": true,
  "Initializing": true,
  "InProgress": true,
  "Completed, Succeeded": true,
  "Completed, Cancelled": true,
  "Completed, TimedOut": true,
  "Completed, Errored": true,
  "Completed, Rejected": true,
  "Completed, Aborted": true
} satisfies Record<TransferState, true>) as TransferState[];

/** A queued download from alice, with `fields` over it */
export const transfer = (fields: Partial<Transfer> = {}): Transfer => ({
  id: newId(),
  username: "alice",
  direction: "Download",
  filename: "@@a\\Music\\Album\\01 One.flac",
  size: 1000,
  state: "Queued, Remotely",
  requestedAt: "2026-10-26T10:00:00Z",
  bytesTransferred: 0,
  averageSpeed: 0,
  percentComplete: 0,
  attempts: 1,
  removed: false,
  paused: false,
  ...fields
});

/** A joined room with nothing unread, `fields` over it */
export const joinedRoom = (name: string, fields: Partial<JoinedRoom> = {}): JoinedRoom => ({
  name,
  unread: 0,
  mentions: 0,
  notify: "mentions",
  userCount: 3,
  lastMessageAt: null,
  lastMessage: null,
  lastReadId: 0,
  refused: false,
  ...fields
});

/** An open conversation with `username` with nothing unread */
export const conversation = (username: string): Conversation => ({
  username,
  isActive: true,
  isServerNotice: false,
  unAcknowledgedMessageCount: 0,
  hasUnAcknowledgedMessages: false,
  notify: "all",
  draft: "",
  lastMessage: null,
  messages: null
});

/** A known user in the default group with nothing going on, `fields` over it */
export const userSummary = (
  username: string,
  fields: Partial<Schemas["UserSummary"]> = {}
): Schemas["UserSummary"] => ({
  username,
  known: true,
  isPrivileged: false,
  groups: [],
  uploads: { queue: "default", slots: 10, speed: "default", speedLimit: null, limits: [] },
  muted: false,
  buddy: null,
  lastSeenAt: null,
  figures: {
    downloads: { succeeded: 0, failed: 0, active: 0, queued: 0 },
    uploads: { succeeded: 0, failed: 0, active: 0, queued: 0 },
    messages: { in: 0, out: 0, unread: 0 }
  },
  unreadMessages: 0,
  activeTransfers: 0,
  ...fields
});

/** A network search for "album" still running, `fields` over it */
export const search = (fields: Partial<Schemas["Search"]> = {}): Schemas["Search"] => ({
  id: newId(),
  searchText: "album",
  token: 1,
  state: "InProgress",
  startedAt: "2026-10-26T10:00:00Z",
  endedAt: null,
  responseCount: 0,
  fileCount: 0,
  lockedFileCount: 0,
  responseBytes: 0,
  responses: null,
  scope: { type: "network", subjects: [] },
  ...fields
});

/** A shared file of 4 MB, its extension taken from its name */
export const sharedFile = (filename: string): Schemas["File"] => ({
  filename,
  size: 4_000_000,
  code: 1,
  extension: /\.(\w+)$/.exec(filename)?.[1] ?? "",
  isLocked: false,
  attributeCount: 0,
  attributes: []
});

/** The daemon's answer to a change of the options, which leaves them at `optionsRevision` */
export const changed = (optionsRevision = "changed"): Schemas["OptionsChanged"] => ({
  applied: [],
  reconnectRequired: [],
  restartRequired: [],
  optionsRevision
});

/** The daemon's answer to a saved option or file, with nothing applied unless `fields` says */
export const written = (fields: Partial<Schemas["FileWritten"]> = {}): Schemas["FileWritten"] => ({
  revision: "sha256:2",
  optionsRevision: "written",
  applied: [],
  reconnectRequired: [],
  restartRequired: [],
  ...fields
});
