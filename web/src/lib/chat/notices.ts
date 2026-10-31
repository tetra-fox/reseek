import type { Target } from "./pane.svelte";

/** The reserved name the Soulseek server's notices come from, which no user can hold */
export const SERVER = "server";

/** The pane that shows the server's notices */
export const NOTICES: Target = { kind: "user", name: SERVER };

/** Whether `target` is the pane of the server's notices */
export const isNotices = (target: Target) => target.kind === "user" && target.name === SERVER;

/** What a chat is called on screen */
export const chatName = (target: Target) => (isNotices(target) ? "Server" : target.name);
