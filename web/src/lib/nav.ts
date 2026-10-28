import ChartColumnIcon from "@lucide/svelte/icons/chart-column";
import CircleUserIcon from "@lucide/svelte/icons/circle-user";
import DownloadIcon from "@lucide/svelte/icons/download";
import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
import MessageSquareIcon from "@lucide/svelte/icons/message-square";
import SearchIcon from "@lucide/svelte/icons/search";
import SettingsIcon from "@lucide/svelte/icons/settings";
import UploadIcon from "@lucide/svelte/icons/upload";
import UsersIcon from "@lucide/svelte/icons/users";
import type { Component } from "svelte";

/** The UI's routes live after the `#`, so the app works under any URL base */
export const href = (path: string) => `#${path}`;

/** The route a URL shows and its query, both written after the `#` */
export const route = (url: { hash: string }): { path: string; query: URLSearchParams } => {
  const hash = url.hash.replace(/^#/, "");
  const cut = hash.indexOf("?");
  const path = (cut === -1 ? hash : hash.slice(0, cut)) || "/";
  return { path, query: new URLSearchParams(cut === -1 ? "" : hash.slice(cut + 1)) };
};

/** A name as one path segment; a name of dots alone is encoded so the path keeps it */
export const segment = (name: string) =>
  /^\.+$/.test(name) ? name.replaceAll(".", "%2E") : encodeURIComponent(name);

/** The link to a user's profile, or to one of its tabs */
export const userHref = (username: string, tab?: string) =>
  href(`/users/${segment(username)}${tab ? `/${tab}` : ""}`);

/** The link to the conversation with a user; names the chat routes use are written encoded */
export const chatHref = (username: string) =>
  href(
    `/chat/${
      username === "search" || username === "rooms"
        ? `%${username.charCodeAt(0).toString(16)}${username.slice(1)}`
        : segment(username)
    }`
  );

/** The link to a room's pane */
export const roomHref = (room: string) => href(`/chat/rooms/${segment(room)}`);

export type NavItem = { title: string; path: string; icon: Component };

/** The screens in the order the navigation lists them */
export const navItems: NavItem[] = [
  { title: "Dashboard", path: "/dashboard", icon: ChartColumnIcon },
  { title: "Search", path: "/searches", icon: SearchIcon },
  { title: "Downloads", path: "/downloads", icon: DownloadIcon },
  { title: "Uploads", path: "/uploads", icon: UploadIcon },
  { title: "Chat", path: "/chat", icon: MessageSquareIcon },
  { title: "Users", path: "/users", icon: UsersIcon },
  { title: "Browse", path: "/browse", icon: FolderOpenIcon }
];

/** The System screen, kept apart from the screens above to sit at the navigation's foot */
export const systemItem: NavItem = { title: "System", path: "/system", icon: SettingsIcon };

/** Our own profile, reached from the account menu rather than the navigation */
export const profileItem: NavItem = {
  title: "Your profile",
  path: "/profile",
  icon: CircleUserIcon
};

/** The tabs of the System area, in order */
export const systemTabs = [
  { tab: "info", title: "Info" },
  { tab: "settings", title: "Settings" },
  { tab: "account", title: "Account" },
  { tab: "shares", title: "Shares" },
  { tab: "files", title: "Files" },
  { tab: "data", title: "Data" },
  { tab: "events", title: "Events" },
  { tab: "logs", title: "Logs" }
] as const;

/** The navigation item a route is under: `/searches/<id>` is under Search */
export const navCurrent = (path: string): NavItem | undefined =>
  [...navItems, systemItem, profileItem].find(
    (item) => path === item.path || path.startsWith(`${item.path}/`)
  );

/**
 * Whether going from one path to another changes the screen, a different destination of the
 * navigation, rather than a tab, a sub-page or a query within the same one
 */
export const changesScreen = (from: string, to: string): boolean =>
  navCurrent(from) !== navCurrent(to);

/**
 * One step of the header's breadcrumb: a link back to it, or an action for a step that is the
 * same route (closing a drawer). The last step is where the user is. `copy` gives the step a
 * copy button for its text, such as a folder's full path
 */
export type Crumb = { label: string; href?: string; onclick?: () => void; copy?: string };

/** The steps a route starts with: its screen, then the System tab it is on */
export const baseCrumbs = (path: string): Crumb[] => {
  const item = navCurrent(path);
  if (path === "/login") return [{ label: "Log in" }];
  if (!item) return [];
  const crumbs: Crumb[] = [{ label: item.title, href: href(item.path) }];
  if (item === systemItem) {
    const tab = systemTabs.find((t) => path.startsWith(`/system/${t.tab}`));
    if (tab) crumbs.push({ label: tab.title, href: href(`/system/${tab.tab}`) });
  }
  return crumbs;
};

/** The document title for a breadcrumb, innermost first: "Settings - System - reseek" */
export const titleOf = (crumbs: Crumb[]): string =>
  [...crumbs.map((c) => c.label).toReversed(), "reseek"].join(" - ");

/** The document title for a route without steps of its own */
export const title = (path: string): string => titleOf(baseCrumbs(path));

/**
 * Where a route goes instead of rendering, or null when it renders: a relay agent shows only
 * the System area, and the root is the dashboard
 */
export const redirectFor = (path: string, agent: boolean): string | null => {
  if (agent) return path.startsWith("/system") ? null : "/system";
  return path === "/" ? "/dashboard" : null;
};

/**
 * Where a path that no screen renders goes: the chat page for the old rooms paths, the screen
 * whose path it starts with, such as Browse for /browse/carol, else the dashboard
 */
export const unknownTarget = (path: string): string => {
  if (path === "/rooms" || path.startsWith("/rooms/")) return "/chat";
  return navCurrent(path)?.path ?? "/dashboard";
};

/** Where to go after login: a route of this app, never the login itself, else the dashboard */
export const safeReturn = (to: string | null): string => {
  if (!to || !to.startsWith("/") || to.startsWith("//") || to.includes("\\")) return "/dashboard";
  if (to === "/login" || to.startsWith("/login?") || to.startsWith("/login/")) return "/dashboard";
  return to;
};
