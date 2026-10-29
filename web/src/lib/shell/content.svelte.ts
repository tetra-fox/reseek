/** The shell's content area, which scrolls on its own between the header and the panels below */
class Content {
  /** the element that scrolls, for screens that follow or set their scroll position */
  scroller = $state<HTMLElement | null>(null);
}

export const content = new Content();
