import { describe, expect, it } from "vitest";

import { redirectFor, route, safeReturn, title, userHref } from "./nav";

describe("routes after the hash", () => {
  it("read the path and the query from the hash", () => {
    const at = route(new URL("http://host/reseek/#/browse?user=a%20b"));
    expect(at.path).toBe("/browse");
    expect(at.query.get("user")).toBe("a b");
    expect(route(new URL("http://host/reseek/")).path).toBe("/");
  });

  it("link usernames a path could lose", () => {
    expect(userHref("a/b c")).toBe("#/users/a%2Fb%20c");
    expect(userHref("..")).toBe("#/users/%2E%2E");
  });
});

describe("redirects", () => {
  it("send unknown routes to the dashboard and an agent to System", () => {
    expect(redirectFor("/", false)).toBe("/dashboard");
    expect(redirectFor("/nowhere", false)).toBe("/dashboard");
    expect(redirectFor("/searches/42", false)).toBeNull();
    expect(redirectFor("/searches/42", true)).toBe("/system");
    expect(redirectFor("/system/logs", true)).toBeNull();
  });

  it("only return to a route of this app after login", () => {
    expect(safeReturn("/browse?user=x")).toBe("/browse?user=x");
    expect(safeReturn(null)).toBe("/dashboard");
    expect(safeReturn("//evil.example")).toBe("/dashboard");
    expect(safeReturn("https://evil.example/")).toBe("/dashboard");
    expect(safeReturn("/login?to=/x")).toBe("/dashboard");
  });
});

describe("titles", () => {
  it("name the screen innermost first", () => {
    expect(title("/downloads")).toBe("Downloads - reseek");
    expect(title("/searches/1")).toBe("Search - reseek");
    expect(title("/system/settings")).toBe("Settings - System - reseek");
    expect(title("/login")).toBe("Log in - reseek");
  });
});
