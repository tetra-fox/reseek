import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import GridTable from "#lib/testing/GridTable.svelte";
import { box } from "#lib/testing/state.svelte.js";

type Row = { key: string; name: string; size: number };

describe("a grid", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  const start = (rows: Row[], sort: { id: string; dir: "asc" | "desc" } | null = null) => {
    const source = box(rows);
    const kept = box({ sort, dragged: {} as Record<string, number> }).value;
    component = mount(GridTable, { target: document.body, props: { source, kept } });
    flushSync();
    return { source, kept };
  };
  const order = () =>
    [...document.querySelectorAll<HTMLElement>("[data-key]")].map((r) => r.dataset.key);
  const header = (name: string) =>
    [...document.querySelectorAll<HTMLElement>("[role=columnheader]")].find((h) =>
      h.textContent?.includes(name)
    )!;
  const click = (name: string) => {
    header(name).querySelector("button")!.click();
    flushSync();
  };

  const rows = (): Row[] => [
    { key: "a", name: "Beta", size: 5 },
    { key: "b", name: "alpha", size: 9 },
    { key: "c", name: "Alpha", size: 5 }
  ];

  it("sorts on a header's clicks: its first way, the other way, then as the rows came", () => {
    const { kept } = start(rows());
    expect(order()).toEqual(["a", "b", "c"]);
    // size starts largest first; equal sizes go by name
    click("Size");
    expect(kept.sort).toEqual({ id: "size", dir: "desc" });
    expect(order()).toEqual(["b", "c", "a"]);
    expect(header("Size").getAttribute("aria-sort")).toBe("descending");
    click("Size");
    expect(order()).toEqual(["c", "a", "b"]);
    click("Size");
    expect(kept.sort).toBeNull();
    expect(order()).toEqual(["a", "b", "c"]);
    // names ignore case, and equal ones keep the order they came in
    click("Name");
    expect(order()).toEqual(["b", "c", "a"]);
  });

  it("sorts again when a sorted value changes in place", () => {
    const { source } = start(rows(), { id: "size", dir: "asc" });
    expect(order()).toEqual(["c", "a", "b"]);
    source.value[1].size = 1;
    flushSync();
    expect(order()).toEqual(["b", "c", "a"]);
  });

  it("keeps a width dragged from the column's width on screen, and none for a press alone", () => {
    const { kept } = start(rows());
    const cell = header("Size");
    cell.getBoundingClientRect = () => ({ width: 64 }) as DOMRect;
    const edge = cell.querySelector<HTMLElement>("[role=separator]")!;
    const template = () => document.querySelector("[data-template]")!.getAttribute("data-template");
    const press = () => {
      edge.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, clientX: 100 }));
      flushSync();
    };
    const move = (to: number) => {
      document.dispatchEvent(new MouseEvent("mousemove", { clientX: to }));
      flushSync();
    };
    const release = (to: number) => {
      document.dispatchEvent(new MouseEvent("mouseup", { clientX: to }));
      flushSync();
    };
    const drag = (to: number) => {
      press();
      move(to);
      release(to);
    };
    drag(100);
    expect(kept.dragged).toEqual({});
    // the column follows the pointer before the button is let go
    press();
    move(130.4);
    expect(template()).toBe("2rem minmax(10rem, 1fr) 94px");
    release(130.4);
    expect(kept.dragged).toEqual({ size: 94 });
    expect(template()).toBe("2rem minmax(10rem, 1fr) 94px");
    // never narrower than 40 pixels
    cell.getBoundingClientRect = () => ({ width: 94 }) as DOMRect;
    drag(0);
    expect(kept.dragged).toEqual({ size: 40 });
  });
});
