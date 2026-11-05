import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ActiveFilter } from "#lib/lists/filtered.js";

import Summary from "./Summary.svelte";

describe("the line of filters on", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  const start = (labels: string[]) => {
    const refocus = vi.fn();
    const props = $state({
      filters: [] as ActiveFilter[],
      shown: 3,
      total: 10,
      noun: ["download", "downloads"] as const,
      refocus
    });
    props.filters = labels.map((label) => ({
      id: label,
      label,
      clear: () => (props.filters = props.filters.filter((f) => f.id !== label))
    }));
    component = mount(Summary, { target: document.body, props });
    flushSync();
    return { props, refocus };
  };
  const chip = (label: string) =>
    document.querySelector<HTMLButtonElement>(`button[aria-label="Remove filter ${label}"]`)!;
  const status = () => document.querySelector("[role=status]")!.textContent;

  it("names what shows and each filter, and says nothing until they change", () => {
    start(["Active", "Failed"]);
    expect(document.body.textContent).toContain("Showing 3 of 10 downloads");
    expect(chip("Active")).not.toBeNull();
    expect(status()).toBe("");
  });

  it("moves focus to the next chip as one goes, and to the list after the last", async () => {
    const { refocus } = start(["A", "B", "C"]);
    chip("B").click();
    flushSync();
    expect(document.activeElement).toBe(chip("C"));
    expect(status()).toBe("Filters on: A and C");
    chip("C").click();
    flushSync();
    expect(document.activeElement).toBe(chip("A"));
    chip("A").click();
    flushSync();
    await tick();
    expect(refocus).toHaveBeenCalledOnce();
    expect(status()).toBe("Filters cleared");
    // nothing is left to name
    expect(document.body.textContent).not.toContain("Showing");
  });

  it("clears every filter at once and gives focus to the list", async () => {
    const { props, refocus } = start(["A", "B"]);
    [...document.querySelectorAll("button")]
      .find((b) => b.textContent === "Clear filters")!
      .click();
    await tick();
    expect(props.filters).toEqual([]);
    expect(refocus).toHaveBeenCalledOnce();
  });

  it("leaves Clear filters to an emptied list's own", () => {
    const { props } = start(["A"]);
    props.shown = 0;
    flushSync();
    const clear = [...document.querySelectorAll("button")].filter(
      (b) => b.textContent === "Clear filters"
    );
    expect(clear).toHaveLength(0);
  });
});
