import type { Outcome } from "./request";

/** Where an edit saved as it is typed stands */
export type SaveState = "idle" | "waiting" | "saving" | "saved" | "failed";

/** How large a body browsers send with `keepalive`: they refuse a request past 64 KiB */
const KEEPALIVE_BYTES = 64 * 1024;

/**
 * Whether `body`, as JSON, fits in a `keepalive` request. One that doesn't goes as a plain
 * request, which may not outlive the page but is sent at all
 */
export const fitsKeepalive = (body: unknown) =>
  new TextEncoder().encode(JSON.stringify(body)).length <= KEEPALIVE_BYTES;

/**
 * Saves a value as it is edited, once the edits pause for `ms`. One save runs at a time and the
 * newest edit goes last, so an older save can't land after a newer one. An edit still waiting
 * when the component goes is saved at once, so leaving the view doesn't lose it, and when the
 * page goes it is saved with `keepalive`, which `save` puts on its request so the request
 * outlives the page. That page-leaving save is the one exception to the order: it is sent
 * without waiting for a save that runs. Make it while a component is being set up
 */
export class Autosave<T> {
  state = $state<SaveState>("idle");
  /** why the last save failed, empty when it didn't */
  error = $state("");
  #save: (value: T, keepalive: boolean) => Promise<Outcome<unknown>>;
  #ms: number;
  #waiting: { value: T } | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #running: Promise<void> | null = null;

  constructor(save: (value: T, keepalive: boolean) => Promise<Outcome<unknown>>, ms: number) {
    this.#save = save;
    this.#ms = ms;
    $effect(() => {
      const leave = () => this.#leave();
      window.addEventListener("pagehide", leave);
      return () => {
        window.removeEventListener("pagehide", leave);
        void this.flush();
      };
    });
  }

  /** `value` is the newest edit, saved once the edits pause */
  change(value: T) {
    this.#waiting = { value };
    this.state = "waiting";
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.flush(), this.#ms);
  }

  /** Drops the edit waiting to be saved, as when the text is back to what is saved */
  cancel() {
    clearTimeout(this.#timer);
    if (!this.#waiting) return;
    this.#waiting = null;
    if (!this.#running) this.state = "idle";
  }

  /** Saves the edit waiting now, once the save that runs is done */
  async flush() {
    clearTimeout(this.#timer);
    while (this.#running) await this.#running;
    const next = this.#waiting;
    if (!next) return;
    this.#waiting = null;
    this.#running = this.#run(next.value, false).finally(() => (this.#running = null));
    await this.#running;
  }

  /**
   * The page goes: the edit waiting is sent now, without waiting for a save that runs, since the
   * page may be gone before that one answers
   */
  #leave() {
    clearTimeout(this.#timer);
    const next = this.#waiting;
    if (!next) return;
    this.#waiting = null;
    void this.#run(next.value, true);
  }

  async #run(value: T, keepalive: boolean) {
    this.state = "saving";
    const outcome = await this.#save(value, keepalive);
    this.error = outcome.ok ? "" : outcome.error;
    if (!outcome.ok) this.state = "failed";
    else this.state = this.#waiting ? "waiting" : "saved";
  }
}
