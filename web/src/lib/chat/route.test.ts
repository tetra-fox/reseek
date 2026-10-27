import { describe, expect, it } from "vitest";

import { chatHref, roomHref } from "#lib/nav.js";

import { chatTarget } from "./route";

const path = (link: string) => link.replace(/^#/, "");

describe("chat routes", () => {
  it("reads a conversation, a room, the search and none", () => {
    expect(chatTarget("/chat")).toBeNull();
    expect(chatTarget("/chat/search")).toBe("search");
    expect(chatTarget("/chat/rooms/indie%20rock")).toEqual({ kind: "room", name: "indie rock" });
    expect(chatTarget("/chat/alice")).toEqual({ kind: "user", name: "alice" });
  });

  it("round-trips names the routes use for themselves, slashes and dots", () => {
    for (const name of ["search", "rooms", "a/b", "..", "50%"]) {
      expect(chatTarget(path(chatHref(name)))).toEqual({ kind: "user", name });
      expect(chatTarget(path(roomHref(name)))).toEqual({ kind: "room", name });
    }
  });
});
