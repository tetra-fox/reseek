import { goto } from "$app/navigation";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { RoomMembers, type RoomUser } from "#lib/chat/room.svelte.js";
import { active, press, ticked } from "#lib/testing/keys.js";
import { fakeNetwork } from "#lib/testing/network.js";

import RoomUsers from "./RoomUsers.svelte";

vi.mock("$app/navigation", () => ({ goto: vi.fn() }));

const member = (username: string, status: RoomUser["status"]): RoomUser =>
  ({
    username,
    status,
    averageSpeed: 0,
    uploadCount: 0,
    fileCount: 0,
    directoryCount: 0,
    slotsFull: null,
    countryCode: null,
    self: false
  }) as RoomUser;

describe("a room's users", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("lists operators then members, online first, and moves, selects and opens by key", () => {
    fakeNetwork(() => undefined);
    const members = new RoomMembers("indie");
    members.room = {
      name: "indie",
      isPrivate: true,
      owner: "olga",
      operators: [],
      users: [],
      tickers: [],
      messages: null
    } as never;
    for (const u of [member("zed", "Online"), member("amy", "Away"), member("olga", "Offline")]) {
      members.users.upsert(u);
    }
    component = mount(RoomUsers, { target: document.body, props: { members } });
    flushSync();
    expect(
      [...document.querySelectorAll("[data-line]")].map((l) => l.getAttribute("data-line"))
    ).toEqual(["h:operators", "olga", "h:members", "zed", "amy"]);
    expect(document.body.textContent).toContain("3 users · 1 online");
    const grid = document.querySelector("[role=grid]")!;
    press(grid, "ArrowDown");
    press(grid, "ArrowDown");
    expect(active(grid)).toBe("zed");
    press(grid, "a", { ctrlKey: true });
    expect(ticked()).toEqual(["zed", "amy"]);
    press(grid, "Escape");
    expect(ticked()).toEqual([]);
    press(grid, "Enter");
    expect(goto).toHaveBeenCalledWith("#/users/zed");
  });
});
