import { fields, stored } from "./stored.svelte";

/** The UI's own preferences in this browser */
export const settings = stored("reseek.ui.settings", (saved) => ({
  sidebar: fields(saved).sidebar === "collapsed" ? ("collapsed" as const) : ("expanded" as const)
}));

/** How a search's results are sorted */
export type ResultSort = "uploadSpeed" | "queueLength";

/** The search results' sort and toggles, kept between searches; the filter text is not */
export const searchOptions = stored("reseek.search.options", (saved) => {
  const s = fields(saved);
  const flag = (value: unknown, otherwise: boolean) =>
    typeof value === "boolean" ? value : otherwise;
  return {
    sort: (s.sort === "queueLength" ? "queueLength" : "uploadSpeed") as ResultSort,
    hideLocked: flag(s.hideLocked, true),
    hideNoFreeSlots: flag(s.hideNoFreeSlots, false),
    fold: flag(s.fold, false)
  };
});
