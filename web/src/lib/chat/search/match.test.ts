import { describe, expect, it } from "vitest";

import type { UserFound } from "#lib/users/match.js";

import type { Found, MessageHit, RoomFound } from "./match";
import {
  dropdown,
  fullTextQuery,
  groupHits,
  hardResult,
  LISTED,
  matchNames,
  plainQuery,
  rankHits
} from "./match";

const room = (name: string): RoomFound => ({ name, joined: false, userCount: null });
const user = (name: string): UserFound => ({ name });
const hit = (message: string, localId: number, where: string | null = null): MessageHit => ({
  kind: where ? "room" : "private",
  room: where,
  username: "alice",
  timestamp: "2026-10-26T10:00:00Z",
  message,
  localId,
  snippet: message
});
const found = (rooms: string[], users: string[], hits: MessageHit[] = []): Found => ({
  rooms: rooms.map(room),
  users: users.map(user),
  hits
});

describe("the query a text makes", () => {
  it("keeps plain words plain and matches the word being typed", () => {
    expect(fullTextQuery("pink flo")).toBe('"pink" "flo"*');
    expect(fullTextQuery("sys-admin a:b")).toBe('"sys-admin" "a:b"*');
  });

  it("passes on what is written as a query", () => {
    for (const q of ['"pink floyd"', "pink OR floyd", "pink NOT floyd", "flo*"]) {
      expect(fullTextQuery(q)).toBe(q);
    }
  });

  it("reads a broken query as words, and no words as nothing", () => {
    expect(plainQuery('"open phrase')).toBe('"open" "phrase"*');
    expect(plainQuery("rock OR")).toBe('"rock" "OR"*');
    expect(plainQuery('" "')).toBe("");
  });

  it("stays inside what the daemon reads for the longest text the box takes", () => {
    expect(plainQuery("a ".repeat(50).trim()).length).toBeLessThanOrEqual(200);
  });
});

describe("what a search lists", () => {
  it("ranks each category by how close the name is, and counts a name once", () => {
    const joined = { name: "indie", joined: true, userCount: 8 };
    const { rooms, users } = matchNames(
      [room("deep indie"), room("indie"), joined],
      [user("xindie"), user("indie_kid"), { name: "indie_kid", summary: {} as never }],
      "indie"
    );
    expect(rooms).toEqual([joined, room("deep indie")]);
    expect(users.map((u) => u.name)).toEqual(["indie_kid", "xindie"]);
    expect(users[0].summary).toBeDefined();
  });

  it("calls an exact name the hard result, alone", () => {
    expect(hardResult(found(["indie"], ["indies"]), "indie")?.kind).toBe("room");
    expect(hardResult(found(["indies"], ["indie"]), "indie")?.kind).toBe("user");
  });

  it("finds no hard result in a part of a name, another case or a name taken twice", () => {
    expect(hardResult(found(["indies"], []), "indie")).toBeNull();
    expect(hardResult(found(["INDIE"], ["Indie"]), "indie")).toBeNull();
    expect(hardResult(found(["indie"], ["indie"]), "indie")).toBeNull();
  });

  it("keeps the hard result apart and a few of each category", () => {
    const names = Array.from({ length: LISTED + 2 }, (_, n) => `bob${n}`);
    const listed = dropdown(found(["bob", ...names], names), "bob");
    expect(listed.hard).toEqual({ kind: "room", room: room("bob") });
    expect(listed.rooms.map((r) => r.name)).toEqual(names.slice(0, LISTED));
    expect(listed.users.length).toBe(LISTED);
  });

  it("puts the hits holding the whole text ahead, the newest first among them", () => {
    const hits = [hit("floyd and then pink", 3), hit("a pink floyd", 2), hit("pink floyd", 1)];
    expect(rankHits(hits, "pink floyd").map((h) => h.localId)).toEqual([1, 2, 3]);
    const newest = [hit("a floyd", 9), hit("a floyd", 8)];
    expect(rankHits(newest, "floyd").map((h) => h.localId)).toEqual([9, 8]);
  });

  it("groups the hits by the chat they are in, in the order they came", () => {
    const groups = groupHits([hit("a", 1, "indie"), hit("b", 2), hit("c", 3, "indie")]);
    expect(groups.map((g) => [g.key, g.hits.map((h) => h.localId)])).toEqual([
      ["room:indie", [1, 3]],
      ["user:alice", [2]]
    ]);
  });
});
