/** How the UI names a runtime change that ends when reseek stops; the words are the owner's */
export const UNTIL_RESTART = "until restart";

/** The start of the line where a room's history has a hole; the words are the owner's */
export const GAP_NOTICE = "Chat history unavailable";

/** Why a room's history has a hole, after the gap notice; the words are the owner's */
export const GAP_REASONS = {
  daemonDown: "(reseek was not running)",
  disconnected: "(disconnected from Soulseek)"
} as const;

/**
 * The words for pinning a user, which keeps them first in lists and is the buddy list; the
 * words are the owner's
 */
export const BUDDY = {
  add: "Add buddy",
  remove: "Remove buddy",
  badge: "buddy",
  plural: "Buddies"
} as const;

/** The confirmation before restarting reseek, the same from Info and from Settings */
export const RESTART = {
  title: "Restart reseek?",
  body: "Transfers in progress stop and resume after the restart.",
  confirm: "Restart"
} as const;

/** What a screen that failed to render says first; the error's own message follows it */
export const SCREEN_FAILED = "This screen couldn't be shown";
