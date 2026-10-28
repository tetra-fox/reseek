import { toast } from "./toast.svelte";

/**
 * Copies by selecting the text in a hidden box and asking the browser to copy the selection.
 * `execCommand` is deprecated, but it is the one way to copy on a page without a secure context,
 * such as one served over plain http on a LAN address, where `navigator.clipboard` is missing
 */
const copyBySelection = (text: string): boolean => {
  const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  const box = document.createElement("textarea");
  box.value = text;
  box.readOnly = true;
  box.style.position = "fixed";
  box.style.opacity = "0";
  // inside an open dialog, so its focus trap leaves the box focused while it is selected
  (before?.closest("[role=dialog]") ?? document.body).append(box);
  box.select();
  try {
    return document.execCommand("copy");
  } finally {
    box.remove();
    before?.focus();
  }
};

/** Copies `text`, or toasts why the browser wouldn't and gives `false` */
export const copyText = async (text: string): Promise<boolean> => {
  if (navigator.clipboard) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      // refused, such as without focus; copying the selection can still work
    }
  }
  if (copyBySelection(text)) return true;
  const why = isSecureContext
    ? "the browser didn't allow it"
    : "this browser only copies over HTTPS or on localhost";
  toast(`Couldn't copy: ${why}`, "error");
  return false;
};

/** A copy button that says "Copied" for 2 s after a copy works */
export class CopyButton {
  copied = $state(false);
  #timer: ReturnType<typeof setTimeout> | undefined;

  copy = async (text: string): Promise<boolean> => {
    if (!(await copyText(text))) return false;
    this.copied = true;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => (this.copied = false), 2000);
    return true;
  };
}
