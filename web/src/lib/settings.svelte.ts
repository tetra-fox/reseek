import { readLayout } from "./lists/layout";
import { fields, stored } from "./stored.svelte";

/** The UI's own preferences in this browser */
export const settings = stored("reseek.ui.settings", (saved) => ({
  sidebar: fields(saved).sidebar === "collapsed" ? ("collapsed" as const) : ("expanded" as const)
}));

/** How a search's results are sorted */
export type ResultSort = "uploadSpeed" | "queueLength";

/** A queue longer than this is a long one, which the results can hide */
export const LONG_QUEUE = 50;

/** The toggles that hide search results as they start, before the user changes any */
export const HIDE_DEFAULTS = {
  hideLocked: true,
  hideNoFreeSlots: false,
  hideOffline: false,
  hideLongQueues: false
};

/**
 * The search results' layout, sort and toggles, kept between searches; the filter text is not. A
 * layout saved as `view`, "cards" or "dense", reads as the tree or the list
 */
export const searchOptions = stored("reseek.search.options", (saved) => {
  const s = fields(saved);
  const flag = (value: unknown, otherwise: boolean) =>
    typeof value === "boolean" ? value : otherwise;
  return {
    layout: readLayout(s.layout, s.view === "dense" ? "list" : "tree"),
    sort: (s.sort === "queueLength" ? "queueLength" : "uploadSpeed") as ResultSort,
    hideLocked: flag(s.hideLocked, HIDE_DEFAULTS.hideLocked),
    hideNoFreeSlots: flag(s.hideNoFreeSlots, HIDE_DEFAULTS.hideNoFreeSlots),
    hideOffline: flag(s.hideOffline, HIDE_DEFAULTS.hideOffline),
    hideLongQueues: flag(s.hideLongQueues, HIDE_DEFAULTS.hideLongQueues),
    fold: flag(s.fold, false)
  };
});
