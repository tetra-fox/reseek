import { describe, expect, it } from "vitest";

import { browseTarget, changesScreen, route, safeReturn, unknownTarget, userHref } from "./nav";

describe("routes after the hash", () => {
  it("read the path and the query from the hash", () => {
    const at = route(new URL("http://host/reseek/#/searches?user=a%20b"));
    expect(at.path).toBe("/searches");
    expect(at.query.get("user")).toBe("a b");
    expect(route(new URL("http://host/reseek/")).path).toBe("/");
  });

  it("link usernames a path could lose", () => {
    expect(userHref("a/b c")).toBe("#/users/a%2Fb%20c");
    expect(userHref("..")).toBe("#/users/%2E%2E");
  });
});

describe("redirects", () => {
  it("send an old browse link to the user's Shares tab, the folder and odd names kept", () => {
    const target = (query: string) => browseTarget(new URLSearchParams(query));
    expect(target("user=a%2Fb%20c&path=Music%5CAlbum")).toBe(
      "#/users/a%2Fb%20c/shares?path=Music%5CAlbum"
    );
    expect(target("user=bob")).toBe("#/users/bob/shares");
    expect(target("")).toBe("#/searches");
    expect(target("user=%20")).toBe("#/searches");
  });

  it("send a path no screen renders to the screen it belongs to, else the dashboard", () => {
    expect(unknownTarget("/nowhere")).toBe("/dashboard");
    expect(unknownTarget("/downloads/x/y")).toBe("/downloads");
    expect(unknownTarget("/system/nothing")).toBe("/system");
    expect(unknownTarget("/rooms")).toBe("/chat");
    expect(unknownTarget("/rooms/indie")).toBe("/chat");
  });

  it("only return to a route of this app after login", () => {
    expect(safeReturn("/searches?user=x")).toBe("/searches?user=x");
    expect(safeReturn(null)).toBe("/dashboard");
    expect(safeReturn("//evil.example")).toBe("/dashboard");
    expect(safeReturn("https://evil.example/")).toBe("/dashboard");
    expect(safeReturn("/login?to=/x")).toBe("/dashboard");
  });
});

describe("screen changes", () => {
  it("count only a move to another destination of the navigation", () => {
    const moves: [string, string, boolean][] = [
      ["/dashboard", "/searches", true],
      ["/searches/42", "/downloads", true],
      ["/users/alice/shares", "/users/bob", false],
      ["/users", "/users/alice", false],
      ["/system/info", "/system/settings", false],
      ["/system/info", "/dashboard", true],
      ["/profile", "/users", true]
    ];
    for (const [from, to, changes] of moves)
      expect([from, to, changesScreen(from, to)]).toEqual([from, to, changes]);
  });
});
