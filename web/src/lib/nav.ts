import ChartColumnIcon from "@lucide/svelte/icons/chart-column";
import DownloadIcon from "@lucide/svelte/icons/download";
import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
import MessageSquareIcon from "@lucide/svelte/icons/message-square";
import MessagesSquareIcon from "@lucide/svelte/icons/messages-square";
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

/** The link to a user's profile; a name of dots alone is encoded so the path keeps it */
export const userHref = (username: string) =>
  href(
    `/users/${/^\.+$/.test(username) ? username.replaceAll(".", "%2E") : encodeURIComponent(username)}`
  );

export type NavItem = { title: string; path: string; icon: Component };

/** The screens in the order the navigation lists them */
export const navItems: NavItem[] = [
  { title: "Dashboard", path: "/dashboard", icon: ChartColumnIcon },
  { title: "Search", path: "/searches", icon: SearchIcon },
  { title: "Downloads", path: "/downloads", icon: DownloadIcon },
  { title: "Uploads", path: "/uploads", icon: UploadIcon },
  { title: "Rooms", path: "/rooms", icon: MessagesSquareIcon },
  { title: "Chat", path: "/chat", icon: MessageSquareIcon },
  { title: "Users", path: "/users", icon: UsersIcon },
  { title: "Browse", path: "/browse", icon: FolderOpenIcon }
];

export const systemItem: NavItem = { title: "System", path: "/system", icon: SettingsIcon };

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
  [...navItems, systemItem].find((item) => path === item.path || path.startsWith(`${item.path}/`));

/** The document title for a route, innermost first: "Settings - System - reseek" */
export const title = (path: string): string => {
  const item = navCurrent(path);
  const parts = ["reseek"];
  if (item) parts.unshift(item.title);
  if (item === systemItem) {
    const tab = systemTabs.find((t) => path.startsWith(`/system/${t.tab}`));
    if (tab) parts.unshift(tab.title);
  }
  if (path === "/login") parts.unshift("Log in");
  return parts.join(" - ");
};

/**
 * Where a route goes instead of rendering, or null when it renders: a relay agent shows only
 * the System area, and anything unknown goes to the dashboard
 */
export const redirectFor = (path: string, agent: boolean): string | null => {
  if (agent) return path.startsWith("/system") ? null : "/system";
  if (path === "/" || !navCurrent(path)) return "/dashboard";
  return null;
};

/** Where to go after login: a route of this app, never the login itself, else the dashboard */
export const safeReturn = (to: string | null): string => {
  if (!to || !to.startsWith("/") || to.startsWith("//") || to.includes("\\")) return "/dashboard";
  if (to === "/login" || to.startsWith("/login?") || to.startsWith("/login/")) return "/dashboard";
  return to;
};
