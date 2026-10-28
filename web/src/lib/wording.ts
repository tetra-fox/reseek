/** How the UI names a runtime change that ends when reseek stops; the owner's to word */
export const UNTIL_RESTART = "until restart";

/** The start of the line where a room's history has a hole; the owner's to word */
export const GAP_NOTICE = "Chat history unavailable";

/** Why a room's history has a hole, after the gap notice; the owner's to word */
export const GAP_REASONS = {
  daemonDown: "(reseek was not running)",
  disconnected: "(disconnected from Soulseek)"
} as const;
