import { afterEach, describe, expect, it } from "vitest";

import { app } from "#lib/app.svelte.js";
import { session } from "#lib/session.svelte.js";

import { canManage, reach } from "./actions";

const ids = (name: string) => reach(name).map((a) => a.id);

describe("the ways to reach a user", () => {
  afterEach(() => {
    session.principal = null;
    app.state = null;
  });

  it("are all there for someone else, in one order, the links going where they say", () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.state = { user: { username: "me" } } as never;
    expect(reach("al ice", { folder: "Album" }).map((a) => [a.id, a.href])).toEqual([
      ["profile", "#/users/al%20ice"],
      ["message", "#/chat/al%20ice"],
      ["shares", "#/users/al%20ice/shares"],
      ["search", "#/searches?user=al%20ice&text=Album"]
    ]);
  });

  it("leave out what the account may not do, and messaging ourselves", () => {
    session.principal = { name: "me", role: "ReadOnly" } as never;
    expect(ids("alice")).toEqual(["profile", "shares"]);
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.state = { user: { username: "me" } } as never;
    expect(ids("me")).toEqual(["profile", "shares", "search"]);
  });

  it("never offer server as a user", () => {
    session.principal = { name: "me", role: "Administrator" } as never;
    expect(ids("server")).toEqual([]);
  });
});

describe("who can be a buddy or blocked", () => {
  afterEach(() => {
    session.principal = null;
    app.state = null;
  });

  it("is anyone but ourselves and server, for a ReadWrite account", () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    app.state = { user: { username: "me" } } as never;
    expect(canManage("alice")).toBe(true);
    expect(canManage("me")).toBe(false);
    expect(canManage("server")).toBe(false);
    session.principal = { name: "me", role: "ReadOnly" } as never;
    expect(canManage("alice")).toBe(false);
  });

  it("is nobody before the daemon's state arrives, and anyone while it names no user", () => {
    session.principal = { name: "me", role: "ReadWrite" } as never;
    expect(canManage("me")).toBe(false);
    expect(canManage("alice")).toBe(false);
    app.state = { user: { username: null } } as never;
    expect(canManage("alice")).toBe(true);
  });
});
