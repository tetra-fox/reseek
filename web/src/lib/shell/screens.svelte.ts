import { slots } from "#lib/analysis/slots.svelte.js";
import { href, route, systemTabs } from "#lib/nav.js";
import { fields, stored } from "#lib/stored.svelte.js";

/**
 * The screens whose place is in their address, and which of their routes count as it: System
 * at one of its tabs, Users at its tab, filters and page. A profile under Users is a page of its
 * own, and a System path that is no tab is sent back to System, so it is never one to come back to
 */
const KEPT: Record<string, (path: string) => boolean> = {
  "/system": (path) => systemTabs.some((t) => path === `/system/${t.tab}`),
  "/users": (path) => path === "/users"
};

const addressOf = (path: string, query: URLSearchParams) =>
  `${path}${query.size ? `?${query}` : ""}`;

/**
 * The address each of those screens was last left at, by the screen's path. A saved one is read
 * as a route and written again from its parts, so whatever else the saved text holds is left out
 */
const left = stored("reseek.screens", (saved) => {
  const s = fields(saved);
  const kept: Record<string, string> = {};
  for (const [screen, counts] of Object.entries(KEPT)) {
    const at = s[screen];
    if (typeof at !== "string") continue;
    const { path, query } = route({ hash: at });
    if (counts(path)) kept[screen] = addressOf(path, query);
  }
  return kept;
});

/** Notes that a route was shown, for its screen to come back to it */
export const visited = (path: string, query: URLSearchParams) => {
  for (const [screen, counts] of Object.entries(KEPT)) {
    if (counts(path)) left[screen] = addressOf(path, query);
  }
};

/** The address a screen was last left at, null for one not left yet or not kept */
export const leftAt = (screen: string): string | null => left[screen] ?? null;

/**
 * The link to a screen of the navigation: Analysis, System and Users come back as they were
 * left, the others at their start
 */
export const screenHref = (path: string) =>
  path === "/analysis" ? slots.href : href(leftAt(path) ?? path);
