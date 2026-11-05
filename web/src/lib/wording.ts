/** How the UI names a runtime change that ends when reseek stops */
export const UNTIL_RESTART = "until restart";

/** The line where a room's history has a hole, `span` saying when */
export const gapNotice = (span: string) =>
  `Chat history unavailable ${span} (disconnected from Soulseek)`;

/** The words for pinning a user, which keeps them first in lists and is the buddy list */
export const BUDDY = {
  add: "Add buddy",
  remove: "Remove buddy",
  badge: "buddy",
  plural: "Buddies"
} as const;

/** Why Unblock is unavailable for a user the configuration blocks */
export const CONFIG_BLOCKED = "Blocked by the configuration";

/** The action that asks the browser for permission to notify */
export const TURN_ON_NOTIFICATIONS = "Turn on notifications";

/** Why a page on a plain http address gets no desktop notifications */
export const DESKTOP_NEEDS_HTTPS =
  "Browsers show desktop notifications only over HTTPS or on localhost.";

/** The confirmation before restarting reseek, the same from Info and from Settings */
export const RESTART = {
  title: "Restart reseek?",
  body: "Transfers in progress stop and resume after the restart.",
  confirm: "Restart"
} as const;

/** What a screen that failed to render says first; the error's own message follows it */
export const SCREEN_FAILED = "This screen could not be displayed";
