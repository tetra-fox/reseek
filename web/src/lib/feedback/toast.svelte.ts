export type ToastAction = { label: string; run: () => void };
export type Toast = {
  id: number;
  text: string;
  kind: "info" | "error";
  action?: ToastAction;
  /** how long it shows in ms, not counting time held; Infinity for one that stays until dismissed */
  ms: number;
  /** its countdown is held, while the pointer is over it or it has focus */
  held: boolean;
  /** shown with this key, a later toast with the same key takes its place */
  key?: string;
  /** counts the times a toast with its key took its place, which starts its bar again */
  round: number;
};

export const toasts = $state<Toast[]>([]);

/** What holds a toast's countdown: the pointer over it, or focus inside it */
export type HoldCause = "pointer" | "focus";

/** A toast's countdown: what is left of it, and what holds it */
type Countdown = {
  remaining: number;
  /** when it last started running, by Date.now */
  started: number;
  timer?: ReturnType<typeof setTimeout>;
  causes: Set<HoldCause>;
  gone?: () => void;
};

const countdowns = new Map<number, Countdown>();

let next = 0;

/** How many toasts show at once */
const SHOWN = 5;

/** Takes the toast away, however it goes; what it was shown with runs `onGone` */
export const dismiss = (id: number) => {
  const i = toasts.findIndex((t) => t.id === id);
  if (i === -1) return;
  toasts.splice(i, 1);
  const countdown = countdowns.get(id);
  countdowns.delete(id);
  clearTimeout(countdown?.timer);
  countdown?.gone?.();
};

const run = (id: number, countdown: Countdown) => {
  countdown.started = Date.now();
  countdown.timer = setTimeout(() => dismiss(id), countdown.remaining);
};

/**
 * Shows `text` for a while, errors for longer; `ms: Infinity` keeps it until dismissed. An
 * action's button dismisses it when pressed. `onGone` runs once it is taken away, by its
 * countdown, by the user, or by a toast that takes its place. One shown with a `key` takes the
 * place of the toast with that key, on top and with its countdown started again. Past `SHOWN`
 * toasts the oldest goes, an error only when all the others are errors
 */
export const toast = (
  text: string,
  kind: Toast["kind"] = "info",
  {
    action,
    ms = kind === "error" ? 12_000 : 6_000,
    onGone,
    key
  }: { action?: ToastAction; ms?: number; onGone?: () => void; key?: string } = {}
) => {
  const at = key === undefined ? -1 : toasts.findIndex((t) => t.key === key);
  if (at !== -1) return replace(at, { text, kind, action, ms }, onGone);
  const id = next++;
  toasts.unshift({ id, text, kind, action, ms, held: false, key, round: 0 });
  const countdown: Countdown = { remaining: ms, started: 0, causes: new Set(), gone: onGone };
  countdowns.set(id, countdown);
  if (Number.isFinite(ms)) run(id, countdown);
  while (toasts.length > SHOWN) {
    // the newest stays, so what was just done always shows
    const oldest = toasts.findLastIndex((t) => t.kind !== "error");
    dismiss(toasts[oldest > 0 ? oldest : toasts.length - 1].id);
  }
  return id;
};

/** Puts `fields` in place of the toast at `at`, moved on top, its countdown started again */
const replace = (
  at: number,
  fields: Pick<Toast, "text" | "kind" | "action" | "ms">,
  onGone?: () => void
) => {
  const [shown] = toasts.splice(at, 1);
  const countdown = countdowns.get(shown.id)!;
  clearTimeout(countdown.timer);
  countdown.gone?.();
  Object.assign(shown, fields, { round: shown.round + 1 });
  toasts.unshift(shown);
  countdown.remaining = fields.ms;
  countdown.gone = onGone;
  // a toast held now runs its new countdown once it is let go
  if (Number.isFinite(fields.ms) && !shown.held) run(shown.id, countdown);
  return shown.id;
};

/**
 * Holds or releases a toast's countdown for `cause`. It stops while anything holds it and runs
 * on with the time it had left once nothing does
 */
export const hold = (id: number, cause: HoldCause, on: boolean) => {
  const countdown = countdowns.get(id);
  const shown = toasts.find((t) => t.id === id);
  if (!countdown || !shown || !Number.isFinite(shown.ms)) return;
  if (on) countdown.causes.add(cause);
  else countdown.causes.delete(cause);
  const held = countdown.causes.size > 0;
  if (held === shown.held) return;
  shown.held = held;
  if (held) {
    clearTimeout(countdown.timer);
    countdown.remaining -= Date.now() - countdown.started;
  } else {
    run(id, countdown);
  }
};

/**
 * Keeps why a list's load failed in `error`. A failed first load is the page's to show; a failed
 * reload says so in a toast once per outage, the first time since a load succeeded
 */
export const loadFailed = (list: { loaded: boolean; error: string }, error: string) => {
  if (list.loaded && !list.error) toast(error, "error");
  list.error = error;
};
