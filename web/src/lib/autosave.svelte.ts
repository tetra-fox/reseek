import type { Outcome } from "./request";

/** Where an edit saved as it is typed stands */
export type SaveState = "idle" | "waiting" | "saving" | "saved" | "failed";

/**
 * Saves a value as it is edited, once the edits pause for `ms`. One save runs at a time and the
 * newest edit goes last, so an older save can't land after a newer one. An edit still waiting
 * when the component goes is saved at once, so leaving the view doesn't lose it. Make it while
 * a component is being set up
 */
export class Autosave<T> {
  state = $state<SaveState>("idle");
  /** why the last save failed, empty when it didn't */
  error = $state("");
  #save: (value: T) => Promise<Outcome<unknown>>;
  #ms: number;
  #waiting: { value: T } | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #running: Promise<void> | null = null;

  constructor(save: (value: T) => Promise<Outcome<unknown>>, ms: number) {
    this.#save = save;
    this.#ms = ms;
    $effect(() => () => void this.flush());
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
    this.#running = this.#run(next.value).finally(() => (this.#running = null));
    await this.#running;
  }

  async #run(value: T) {
    this.state = "saving";
    const outcome = await this.#save(value);
    this.error = outcome.ok ? "" : outcome.error;
    if (!outcome.ok) this.state = "failed";
    else this.state = this.#waiting ? "waiting" : "saved";
  }
}
