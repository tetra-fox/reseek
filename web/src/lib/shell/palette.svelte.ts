/** Whether the command palette is open; the shell has one */
class Palette {
  open = $state(false);
  /** what had focus when the palette opened, for what it opens next to hand focus back to */
  from: Element | null = null;

  show = () => {
    if (!this.open) this.from = document.activeElement;
    this.open = true;
  };
  toggle = () => (this.open ? (this.open = false) : this.show());
}

export const palette = new Palette();
