import { dismiss, toast } from "./toast.svelte";

/** How long an action can be undone before it is carried out */
export const UNDO_MS = 6_000;

/** the dismiss of each toast whose action still waits; dismissing carries the action out */
const waiting = new Map<number, () => void>();

// leaving the page carries out what was waiting, as the toast would have once it went
if (typeof window !== "undefined") {
  window.addEventListener("pagehide", () => {
    for (const leave of waiting.values()) leave();
  });
}

/**
 * An action the user can take back for a while: the screen shows it done at once (`undo` puts
 * that back), a toast says `text` with an Undo button, and `commit` carries it out once the toast
 * is gone without Undo pressed, or when the page is left. The toast's own countdown decides, so
 * holding the toast holds the commit too
 */
export const undoable = (text: string, commit: () => void, undo: () => void, ms = UNDO_MS) => {
  let undone = false;
  const id = toast(text, "info", {
    ms,
    action: {
      label: "Undo",
      run: () => {
        undone = true;
        undo();
      }
    },
    onGone: () => {
      waiting.delete(id);
      if (!undone) commit();
    }
  });
  waiting.set(id, () => dismiss(id));
  return () => dismiss(id);
};
