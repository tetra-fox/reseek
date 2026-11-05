import type { Schemas } from "#lib/api/client.js";
import { notifier, notifyOptions } from "#lib/chat/notify.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { userHref } from "#lib/nav.js";

/** What a buddy's new presence says, as a notification's title */
export const presenceNotice = (change: Schemas["BuddyPresenceChanged"]) => {
  const now = { Online: "is online", Away: "is away", Offline: "went offline" }[change.presence];
  return `${change.username} ${now}`;
};

/**
 * Tells of a buddy coming online, going away or going offline, for the buddies set to notify;
 * the daemon sends the frame only for those
 */
class PresenceNotices {
  constructor() {
    live.on("users.presence", (change) =>
      notifier.other(
        notifyOptions().buddyPresence,
        presenceNotice(change),
        "",
        `presence:${change.username}`,
        userHref(change.username)
      )
    );
  }
}

// the shell imports this module for the notices alone; nothing reads them
new PresenceNotices();
