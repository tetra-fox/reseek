import { expect, it } from "vitest";

import { arrived, landing, markOf, Return, Saver } from "./scroll.svelte";

const mark = { key: "c", index: 2, delta: -10, top: 82 };

it("lands on the row it was left at, wherever that row is now", () => {
  expect(landing(mark, ["new", "a", "b", "c", "d"])).toEqual({ index: 3, delta: -10 });
});

it("lands where that row was while it is gone or not read yet, and waits for enough rows", () => {
  expect(landing(mark, ["a", "b", "x", "d"])).toEqual({ index: 2, delta: -10 });
  expect(landing(mark, ["a", "b"])).toBeNull();
});

it("is kept for the address it was left at, not for the same screen under another filter", () => {
  location.hash = "#/downloads?state=done&user=bob";
  arrived();
  const saver = new Saver("transfers.download");
  saver.note(mark);
  saver.leave();
  // the same address with its parameters in another order
  location.hash = "#/downloads?user=bob&state=done";
  expect(markOf("transfers.download")).toMatchObject(mark);
  location.hash = "#/downloads?state=errored";
  expect(markOf("transfers.download")).toBeNull();
  location.hash = "#/downloads";
  expect(markOf("transfers.download")).toBeNull();
});

it("is kept for the address it was left at, which a search typed after its last scroll moved", () => {
  location.hash = "#/system/settings";
  arrived();
  const saver = new Saver("content./system/settings");
  saver.note(mark);
  // the search goes into the address once typing pauses, which scrolls nothing
  location.hash = "#/system/settings?q=port";
  arrived();
  // the navigation away has moved the address on by the time the box is let go
  location.hash = "#/dashboard";
  saver.leave();
  location.hash = "#/system/settings?q=port";
  expect(markOf("content./system/settings")).toMatchObject(mark);
});

it("leaves a box's place as it was when it was not scrolled", () => {
  location.hash = "#/users";
  arrived();
  const first = new Saver("content./users");
  first.note(mark);
  first.leave();
  location.hash = "#/users?tab=groups";
  arrived();
  new Saver("content./users").leave();
  location.hash = "#/users";
  expect(markOf("content./users")).toMatchObject(mark);
});

it("gives way to the page's own scroll at the next put, before the scroll event comes", () => {
  const box = { scrollTop: 0, scrollHeight: 5000, clientHeight: 300 } as HTMLElement;
  const back = new Return(mark);
  // as a box being brought back puts itself at each change of its lines
  const retry = () => back.holds(box) && back.put(box, 150);
  retry();
  expect(box.scrollTop).toBe(150);
  // the page scrolls to a row a link names and adds a line, all before the next frame
  box.scrollTop = 2832;
  retry();
  expect(box.scrollTop).toBe(2832);
  expect(back.mark).toBeNull();
});

it("goes on when the browser holds the offset within content that shrank", () => {
  const box = { scrollTop: 0, scrollHeight: 5000, clientHeight: 300 } as HTMLElement;
  const back = new Return(mark);
  back.put(box, 900);
  Object.assign(box, { scrollHeight: 800, scrollTop: 500 });
  expect(back.holds(box)).toBe(true);
  expect(back.mark).toBe(mark);
});
