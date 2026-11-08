import FolderOpenIcon from "@lucide/svelte/icons/folder-open";
import MessageSquareIcon from "@lucide/svelte/icons/message-square";
import SearchIcon from "@lucide/svelte/icons/search";
import UserIcon from "@lucide/svelte/icons/user";

import { api, byUser, type Schemas } from "#lib/api/client.js";
import { app } from "#lib/app.svelte.js";
import { SERVER } from "#lib/chat/notices.js";
import { chatHref, href, sharesHref, userHref } from "#lib/nav.js";
import { call } from "#lib/request.js";
import { session } from "#lib/session.svelte.js";
import { fields } from "#lib/stored.svelte.js";

/**
 * What every list calls the ways to reach a user, in the order every list offers them, with their
 * icons, so a user reads the same wherever they are offered. `of` names the user, for a place
 * that shows no name of its own
 */
export const USER_ACTIONS = {
  profile: {
    label: "View profile",
    of: (name: string) => `View ${name}'s profile`,
    icon: UserIcon
  },
  message: { label: "Message", of: (name: string) => `Message ${name}`, icon: MessageSquareIcon },
  shares: {
    label: "Browse shares",
    of: (name: string) => `Browse ${name}'s shares`,
    icon: FolderOpenIcon
  },
  search: {
    label: "Search shares",
    of: (name: string) => `Search ${name}'s shares`,
    icon: SearchIcon
  }
} as const;

export type UserActionId = keyof typeof USER_ACTIONS;

/** Where the ways to reach `username` go, with the folder it was shown in when there is one */
export const links = (username: string, folder?: string) => ({
  profile: userHref(username),
  browse: sharesHref(username),
  message: chatHref(username),
  search: href(
    `/searches?user=${encodeURIComponent(username)}${folder ? `&text=${encodeURIComponent(folder)}` : ""}`
  )
});

/** Our Soulseek name: the one logged in, or the configured one while disconnected, when the state names none */
export const ownName = (): string | undefined => {
  const configured = fields(fields(app.options).soulseek).username;
  return app.state?.user.username ?? (typeof configured === "string" ? configured : undefined);
};

/**
 * The ways to reach `username` this session has, `only` narrowing them. Messaging and searching
 * take ReadWrite, nobody messages themselves, and `server` is no user, so it has none
 */
export const reach = (
  username: string,
  { only, folder }: { only?: UserActionId[]; folder?: string } = {}
) => {
  if (username === SERVER) return [];
  const to = links(username, folder);
  const write = session.can("ReadWrite");
  const open = {
    profile: to.profile,
    message: write && !!app.state && ownName() !== username ? to.message : null,
    shares: to.browse,
    search: write ? to.search : null
  };
  return (Object.keys(USER_ACTIONS) as UserActionId[]).flatMap((id) => {
    const href = open[id];
    return href && (!only || only.includes(id)) ? [{ id, href, ...USER_ACTIONS[id] }] : [];
  });
};

/**
 * Whether this session can make `username` a buddy or block them: it takes ReadWrite, nobody does
 * either to themselves, and `server` is no user. False until the daemon's state arrives, since
 * before it nobody can tell whether `username` is ours
 */
export const canManage = (username: string) =>
  session.can("ReadWrite") && username !== SERVER && !!app.state && ownName() !== username;

/** Whether `block` comes from the configuration, which the API cannot lift */
export const configBlocked = (block: Schemas["Block"] | null | undefined) =>
  block?.source === "config";

/** A user's profile as stored, or with `refresh` after asking the network about them */
export const profileOf = (username: string, refresh: boolean) =>
  call(() =>
    api.GET("/api/v0/users/{username}/profile", {
      params: { path: byUser(username).path, query: { ...byUser(username).query, refresh } }
    })
  );

/**
 * Makes `username` a buddy, or with `change` sets whether they are trusted and whether we are told
 * of their presence; what `change` leaves out keeps its value
 */
export const befriend = (username: string, change: Schemas["BuddyChange"] = {}) =>
  call(() => api.PUT("/api/v0/users/{username}/buddy", { params: byUser(username), body: change }));

/** Makes `username` a buddy, or takes them off the buddy list when `on` is false */
export const setBuddy = (username: string, on: boolean) =>
  on
    ? befriend(username)
    : call(() => api.DELETE("/api/v0/users/{username}/buddy", { params: byUser(username) }));

/** The names of the user-defined upload groups in the options in effect */
export const userGroups = (): string[] =>
  Object.keys(fields(fields(fields(fields(app.options).transfers).groups).user_defined)).toSorted();

/** Puts a user in a user-defined group, or with null takes them out of it */
export const assignGroup = (username: string, group: string | null) =>
  call(() =>
    api.PUT("/api/v0/users/{username}/group", { params: byUser(username), body: { group } })
  );
