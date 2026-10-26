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

/** A colour for an upload group's chip: the built-in groups by meaning, the user's own alike */
export const groupTone = (group: string) =>
  group === "privileged"
    ? ("warning" as const)
    : group === "leechers" || group === "blacklisted"
      ? ("danger" as const)
      : group === "default"
        ? ("neutral" as const)
        : ("accent" as const);

/** A country code as its flag, from the two regional indicator letters */
export const flag = (code: string | null | undefined) =>
  code && /^[A-Za-z]{2}$/.test(code)
    ? String.fromCodePoint(...[...code.toUpperCase()].map((c) => 0x1f1e6 + c.charCodeAt(0) - 65))
    : null;
