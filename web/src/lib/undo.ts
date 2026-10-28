import { dismiss, toast } from "./toast.svelte";

/** How long an action can be undone before it is carried out */
export const UNDO_MS = 6_000;

const waiting = new Map<number, () => void>();

// leaving the page carries out what was waiting, as the toast would have once it went
if (typeof window !== "undefined") {
  window.addEventListener("pagehide", () => {
    for (const commit of waiting.values()) commit();
  });
}

/**
 * An action the user can take back for a while: the screen shows it done at once (`undo` puts
 * that back), a toast says `text` with an Undo button, and `commit` carries it out once the toast
 * is gone without Undo pressed, or when the page is left
 */
export const undoable = (text: string, commit: () => void, undo: () => void, ms = UNDO_MS) => {
  let id = -1;
  const settle = () => {
    clearTimeout(timer);
    waiting.delete(id);
  };
  const done = () => {
    settle();
    commit();
  };
  const timer = setTimeout(done, ms);
  id = toast(text, "info", {
    ms,
    action: {
      label: "Undo",
      run: () => {
        settle();
        undo();
      }
    }
  });
  waiting.set(id, done);
  return () => {
    dismiss(id);
    done();
  };
};
