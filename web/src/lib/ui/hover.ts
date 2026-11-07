import { createSuperHover } from "super-hover";

/**
 * The rows app.css tints and opens while the pointer is on them: list lines, `group` rows, the
 * line-tint utilities and table rows. Its rules key on the attribute set here in place of :hover
 */
const ROWS =
  "[data-line], .group, .line-tint, .pick-row, .list-row, .log-row, .data-table tbody tr";

/**
 * Marks the row under the pointer with `data-super-hover-active`, tested again on the frame after
 * the pointer moves, the page scrolls or the rows change. The browser holds :hover still while a
 * list scrolls, which leaves the tint on a row the pointer has left. Returns what stops it
 */
export const followPointer = () => {
  const hover = createSuperHover({ selector: ROWS });
  // rows added, removed or moved under a resting pointer, which no pointer or scroll event reports
  const changes = new MutationObserver(() => hover.refresh());
  changes.observe(document.body, {
    childList: true,
    subtree: true,
    attributeFilter: ["style", "class"]
  });
  return () => {
    changes.disconnect();
    hover.destroy();
  };
};
