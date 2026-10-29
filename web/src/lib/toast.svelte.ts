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
 * countdown or by the user
 */
export const toast = (
  text: string,
  kind: Toast["kind"] = "info",
  {
    action,
    ms = kind === "error" ? 12_000 : 6_000,
    onGone
  }: { action?: ToastAction; ms?: number; onGone?: () => void } = {}
) => {
  const id = next++;
  toasts.unshift({ id, text, kind, action, ms, held: false });
  const countdown: Countdown = { remaining: ms, started: 0, causes: new Set(), gone: onGone };
  countdowns.set(id, countdown);
  if (Number.isFinite(ms)) run(id, countdown);
  return id;
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
