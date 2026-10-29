/**
 * Reads older pages of history as the view reaches the top: one page when the view is pulled
 * there, and page after page for as long as the scrollbar is held at the top, so dragging it up
 * and holding it reads back to the start. `settle` places the view after each page: on the line
 * it showed, or at the top while held, so the next page is wanted at once
 */
export const pullOlder = async ({
  older,
  more,
  held,
  settle
}: {
  /** reads the page before the oldest message held */
  older: () => Promise<void>;
  /** older messages are still stored */
  more: () => boolean;
  /** the scrollbar is held with the view at the top */
  held: () => boolean;
  settle: (held: boolean) => Promise<void>;
}) => {
  for (;;) {
    await older();
    const holding = held();
    await settle(holding);
    if (!holding || !more()) return;
  }
};
