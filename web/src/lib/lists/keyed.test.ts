import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import Rows from "#lib/testing/Rows.svelte";

import { interaction } from "./interaction.svelte";
import { Keyed, patch, same } from "./keyed.svelte";

type Row = { id: string; name: string; size: number };

const rows = (...items: [string, string, number][]): Row[] =>
  items.map(([id, name, size]) => ({ id, name, size }));

describe("same and patch", () => {
  it("compare inside arrays and objects", () => {
    expect(same({ a: [1, { b: 2 }] }, { a: [1, { b: 2 }] })).toBe(true);
    expect(same({ a: [1, 2] }, { a: [2, 1] })).toBe(false);
    expect(same({ a: null }, { a: undefined })).toBe(false);
  });

  it("write only what differs", () => {
    const tags = ["x"];
    const target = { name: "a", tags, gone: 1 } as Record<string, unknown>;
    patch(target, { name: "b", tags: ["x"] });
    expect(target).toEqual({ name: "b", tags: ["x"] });
    expect(target.tags).toBe(tags);
  });
});

describe("a keyed list on screen", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  const render = (list: Keyed<Row>) => {
    component = mount(Rows, { target: document.body, props: { list } });
    flushSync();
    return () => [...document.querySelectorAll("li")];
  };

  /** Selects from the start of the first row's name to the end of the second's */
  const selectAcross = (items: HTMLLIElement[]) => {
    const range = document.createRange();
    range.setStart(items[0].querySelector(".name")!.firstChild!, 0);
    range.setEnd(items[1].querySelector(".name")!.firstChild!, 3);
    const selection = document.getSelection()!;
    selection.removeAllRanges();
    selection.addRange(range);
    return selection;
  };

  it("keeps the nodes and the selection through a push patch and a poll", () => {
    const list = new Keyed<Row>((row) => row.id);
    list.reconcile(rows(["1", "one", 10], ["2", "two", 20], ["3", "six", 30]));
    const items = render(list);
    const before = items();
    const kept = list.rows.map((row) => list.get(row.id));
    const names = before.map((li) => li.querySelector(".name")!.firstChild);
    const selection = selectAcross(before);
    const selected = selection.toString();
    expect(selected).toContain("one");

    // a frame changes a row outside the selection
    list.upsert({ id: "3", name: "six", size: 35 });
    flushSync();
    // a poll answers with every row, one more changed and one new
    list.reconcile(rows(["1", "one", 10], ["2", "two", 20], ["3", "six", 36], ["4", "new", 40]));
    flushSync();

    const after = items();
    expect(after.slice(0, 3)).toEqual(before);
    expect(after.slice(0, 3).every((li, i) => li === before[i])).toBe(true);
    expect(after.map((li) => li.querySelector(".name")!.firstChild).slice(0, 3)).toEqual(names);
    expect(after[2].querySelector(".size")!.textContent).toBe("36");
    expect(after).toHaveLength(4);
    // the rows are the same objects, patched, not a fresh array of fresh rows
    expect(list.rows.slice(0, 3).every((row, i) => row === kept[i])).toBe(true);
    expect(document.getSelection()!.toString()).toBe(selected);
    expect(document.getSelection()!.getRangeAt(0).startContainer).toBe(names[0]);
  });

  it("keeps a removed row while the user is in the list, and drops it after", () => {
    const list = new Keyed<Row>((row) => row.id);
    list.reconcile(rows(["1", "one", 1], ["2", "two", 2]));
    const items = render(list);
    const release = interaction.hold();
    list.reconcile(rows(["2", "two", 2]));
    flushSync();
    expect(items()).toHaveLength(2);
    expect(items()[0].classList.contains("removed")).toBe(true);
    release();
    flushSync();
    expect(items().map((li) => li.dataset.id)).toEqual(["2"]);
  });
});
