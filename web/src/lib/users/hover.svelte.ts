/** How long the pointer rests on a username before its card opens */
export const OPEN_DELAY_MS = 400;
/** How long after the pointer leaves the link and the card the card closes */
export const CLOSE_DELAY_MS = 200;
/** How long a touch on a username is held before its menu opens */
export const LONG_PRESS_MS = 500;

/** Which username's card or menu is open: one at a time across the whole page */
class Open {
  owner = $state<symbol | null>(null);
}

export const open = new Open();
