import { describe, expect, it } from "vitest";

import { chatHref, roomHref } from "#lib/nav.js";

import { chatTarget, foundHref, resultsHref, searchKind } from "./route";

const path = (link: string) => link.replace(/^#/, "");

describe("chat routes", () => {
  it("reads a conversation, a room, the search and none", () => {
    expect(chatTarget("/chat")).toBeNull();
    expect(chatTarget("/chat/search")).toBe("search");
    expect(chatTarget("/chat/public")).toBe("public");
    expect(chatTarget("/chat/rooms/indie%20rock")).toEqual({ kind: "room", name: "indie rock" });
    expect(chatTarget("/chat/alice")).toEqual({ kind: "user", name: "alice" });
  });

  it("carries the search through its results and the chats opened from them", () => {
    expect(resultsHref("a&b c")).toBe("#/chat/search?q=a%26b%20c");
    expect(resultsHref("x", "rooms")).toBe("#/chat/search?q=x&in=rooms");
    expect(foundHref({ kind: "room", name: "indie" }, "x", "all", 12)).toBe(
      "#/chat/rooms/indie?at=12&q=x"
    );
    expect(foundHref({ kind: "user", name: "alice" }, "x", "private")).toBe(
      "#/chat/alice?q=x&in=private"
    );
    expect(searchKind("rooms")).toBe("rooms");
    expect(searchKind("nonsense")).toBe("all");
    expect(searchKind(null)).toBe("all");
  });

  it("round-trips names the routes use for themselves, slashes and dots", () => {
    for (const name of ["search", "rooms", "public", "a/b", "..", "50%"]) {
      expect(chatTarget(path(chatHref(name)))).toEqual({ kind: "user", name });
      expect(chatTarget(path(roomHref(name)))).toEqual({ kind: "room", name });
    }
  });
});
