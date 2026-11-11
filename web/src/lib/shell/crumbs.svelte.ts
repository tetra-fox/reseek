import { type Crumb, href, navCurrent, systemItem, systemTabs, titleOf } from "#lib/nav.js";

import { screenHref } from "./screens.svelte";

/**
 * The steps a route starts with: its screen, linked as the navigation links it, then the System
 * tab it is on. The screen's step is not a link where that link is the address in view
 */
export const baseCrumbs = (path: string, query = new URLSearchParams()): Crumb[] => {
  const item = navCurrent(path);
  if (path === "/login") return [{ label: "Log in" }];
  if (!item) return [];
  const screen = screenHref(item.path);
  const here = href(`${path}${query.size ? `?${query}` : ""}`);
  const crumbs: Crumb[] = [{ label: item.title, href: screen === here ? undefined : screen }];
  if (item === systemItem) {
    const tab = systemTabs.find((t) => path.startsWith(`/system/${t.tab}`));
    if (tab) crumbs.push({ label: tab.title, href: href(`/system/${tab.tab}`) });
  }
  return crumbs;
};

/** The document title for a route without steps of its own */
export const title = (path: string): string => titleOf(baseCrumbs(path));

/** The steps the screen on show adds under its own in the header's breadcrumb */
class Trail {
  page = $state.raw<Crumb[]>([]);
}

export const trail = new Trail();

/**
 * Puts the steps `make` gives under the screen's own in the breadcrumb, for as long as the
 * calling component is mounted, following whatever `make` reads. Call it while the component is
 * being set up
 */
export const useCrumbs = (make: () => Crumb[]) => {
  $effect(() => {
    const crumbs = make();
    trail.page = crumbs;
    // the steps go once the update that unmounted them is over, unless a component mounted in it
    // put its own in their place, so a hand-off never shows the breadcrumb without them between
    return () => {
      queueMicrotask(() => {
        if (trail.page === crumbs) trail.page = [];
      });
    };
  });
};
