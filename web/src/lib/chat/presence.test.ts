import { describe, expect, it } from "vitest";

import { burstText, fold, FOLD_MS, type PresenceEvent, type PresenceKind } from "./presence";

let id = 0;
const at = (seconds: number) => new Date(Date.UTC(2026, 9, 27, 12, 0, seconds)).toISOString();
const event = (seconds: number, kind: PresenceKind, username: string): PresenceEvent => ({
  id: ++id,
  at: at(seconds),
  kind,
  username
});

describe("joins and leaves in a room", () => {
  it("fold a burst of one kind into one line keyed by its first change", () => {
    const first = event(0, "joined", "alice");
    const bursts = fold([
      first,
      event(2, "joined", "bob"),
      event(4, "joined", "alice"),
      event(5, "left", "carol"),
      event(6, "joined", "dave")
    ]);
    expect(bursts.map((b) => [b.key, b.kind, b.names])).toEqual([
      [`presence:${first.id}`, "joined", ["alice", "bob"]],
      [expect.any(String), "left", ["carol"]],
      [expect.any(String), "joined", ["dave"]]
    ]);
    expect(bursts[0].at).toBe(first.at);
  });

  it("start a new line after a pause, measured from the last change of the burst", () => {
    const step = FOLD_MS / 1000;
    const bursts = fold([
      event(0, "joined", "a"),
      event(step, "joined", "b"),
      event(2 * step, "joined", "c"),
      event(3 * step + 1, "joined", "d")
    ]);
    expect(bursts.map((b) => b.names)).toEqual([["a", "b", "c"], ["d"]]);
  });

  it("say who in plain words, counting the rest", () => {
    const line = (kind: PresenceKind, names: string[]) =>
      burstText({ key: "k", at: at(0), kind, names });
    expect(line("joined", ["alice"])).toBe("alice joined the room");
    expect(line("left", ["alice", "bob"])).toBe("alice and bob left");
    expect(line("joined", ["a", "b", "c", "d"])).toBe("a, b, c and d joined");
    expect(line("joined", ["a", "b", "c", "d", "e"])).toBe("a, b and 3 others joined");
    expect(line("away", ["alice"])).toBe("alice has gone away");
    expect(line("returned", ["alice", "bob"])).toBe("alice and bob have returned");
    expect(line("operator", ["op"])).toBe("op was made an operator");
  });
});
