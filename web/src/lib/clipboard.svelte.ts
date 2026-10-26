import { toast } from "./toast.svelte";

/** Copies `text`, or toasts why the browser wouldn't and gives `false` */
export const copyText = async (text: string): Promise<boolean> => {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // a page served over plain http has no navigator.clipboard at all
    const why = isSecureContext
      ? "the browser didn't allow it"
      : "the browser only copies over https or localhost";
    toast(`Couldn't copy, ${why}`, "error");
    return false;
  }
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
