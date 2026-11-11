/** Whether the keyboard shortcuts sheet is open; the shell has one */
class Shortcuts {
  open = $state(false);
  /** where focus goes back to when the sheet closes */
  from: HTMLElement | null = null;

  /** Opens the sheet, to hand focus back to `from` once it closes */
  show = (from: Element | null) => {
    this.from = from instanceof HTMLElement ? from : null;
    this.open = true;
  };
}

export const shortcuts = new Shortcuts();
