/** How the UI names a runtime change that ends when reseek stops; the owner's to word */
export const UNTIL_RESTART = "until restart";

/** The start of the line where a room's history has a hole; the owner's to word */
export const GAP_NOTICE = "Chat history unavailable";

/** Why a room's history has a hole, after the gap notice; the owner's to word */
export const GAP_REASONS = {
  daemonDown: "(reseek was not running)",
  disconnected: "(disconnected from Soulseek)"
} as const;

/**
 * The words for pinning a user, which keeps them first in lists and is the buddy list; the
 * owner's to word
 */
export const BUDDY = {
  add: "Add buddy",
  remove: "Remove buddy",
  badge: "buddy",
  plural: "Buddies"
} as const;
