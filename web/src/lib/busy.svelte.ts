import type { Outcome } from "./request";
import { toast } from "./toast.svelte";

/**
 * Runs actions one at a time: one asked for while another runs is ignored, so a second click
 * doesn't send it twice. A failure is toasted, and `done` on success when given
 */
export class Busy {
  running = $state(false);

  run = async <T>(
    action: () => Promise<Outcome<T>>,
    done?: string
  ): Promise<Outcome<T> | undefined> => {
    if (this.running) return undefined;
    this.running = true;
    try {
      const outcome = await action();
      if (!outcome.ok) toast(outcome.error, "error");
      else if (done) toast(done);
      return outcome;
    } finally {
      this.running = false;
    }
  };
}
