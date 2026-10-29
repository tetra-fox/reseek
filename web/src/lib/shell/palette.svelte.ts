/** Whether the command palette is open; the shell has one */
class Palette {
  open = $state(false);

  show = () => (this.open = true);
  toggle = () => (this.open = !this.open);
}

export const palette = new Palette();
