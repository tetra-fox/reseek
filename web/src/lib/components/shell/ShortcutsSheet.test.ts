import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";

import Select from "#lib/components/ui/Select.svelte";
import { shortcuts } from "#lib/shell/shortcuts.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { press } from "#lib/testing/keys.js";
import List from "#lib/testing/List.svelte";
import { box } from "#lib/testing/state.svelte.js";
import { SHEET } from "#lib/ui/bindings.js";

import ShortcutsSheet from "./ShortcutsSheet.svelte";

vi.mock("$app/state", () => ({ page: { url: new URL("http://localhost/#/downloads") } }));

afterEach(() => {
  shortcuts.open = false;
  document.body.innerHTML = "";
});

const open = () => document.querySelector('[role="dialog"]');

it("opens on ? outside a text field, lists every key and hands focus back", async () => {
  document.body.innerHTML = `<input id="field" /><button id="before">b</button>`;
  const sheet = mount(ShortcutsSheet, { target: document.body });
  flushSync();
  press(document.getElementById("field")!, "?", { shiftKey: true });
  expect(open()).toBeNull();

  const before = document.getElementById("before")!;
  before.focus();
  press(before, "?", { shiftKey: true });
  await vi.waitFor(() => expect(open()).not.toBeNull());

  const titles = [...open()!.querySelectorAll("h3")].map((h) => h.textContent);
  // the screen in view comes first, the rest in the order they are declared
  expect(titles).toEqual([
    "Transfers",
    ...SHEET.map((g) => g.title).filter((t) => t !== "Transfers")
  ]);
  const labels = [...open()!.querySelectorAll("dt")].map((d) => d.textContent);
  expect(labels).toEqual(
    expect.arrayContaining(SHEET.flatMap((g) => g.bindings.map((b) => b.label)))
  );
  expect(labels).toHaveLength(SHEET.reduce((n, g) => n + g.bindings.length, 0));

  press(document.activeElement!, "Escape");
  await vi.waitFor(() => expect(open()).toBeNull());
  await vi.waitFor(() => expect(document.activeElement).toBe(before));
  unmount(sheet);
});

it("keeps the keys whose words have what is typed", async () => {
  const sheet = mount(ShortcutsSheet, { target: document.body });
  flushSync();
  press(document.body, "?");
  await vi.waitFor(() => expect(open()).not.toBeNull());
  const filter = open()!.querySelector<HTMLInputElement>('input[type="search"]')!;
  filter.value = "ctrl+j";
  filter.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  expect([...open()!.querySelectorAll("dt")].map((d) => d.textContent)).toEqual([
    "Show or hide activity log"
  ]);
  filter.value = "nothing like this";
  filter.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  expect(open()!.textContent).toContain("No shortcuts match");

  // closed and opened again, it lists every key
  press(document.activeElement!, "Escape");
  await vi.waitFor(() => expect(open()).toBeNull());
  press(document.body, "?");
  await vi.waitFor(() => expect(open()).not.toBeNull());
  expect(open()!.querySelector<HTMLInputElement>('input[type="search"]')!.value).toBe("");
  expect(open()!.textContent).not.toContain("No shortcuts match");
  unmount(sheet);
});

it("does not open over a select's open list", () => {
  const select = mount(Select, {
    target: document.body,
    props: {
      "value": "",
      "options": [{ value: "", label: "Any presence" }],
      "aria-label": "Presence"
    }
  });
  const sheet = mount(ShortcutsSheet, { target: document.body });
  flushSync();
  press(document.querySelector('[aria-label="Presence"]')!, "Enter");
  const list = document.querySelector('[role="listbox"]')!;
  expect(list).not.toBeNull();
  press(list, "?", { shiftKey: true });
  expect(shortcuts.open).toBe(false);
  unmount(sheet);
  unmount(select);
});

it("leaves ? to a focused list, which goes to the row it names", async () => {
  const lines = box([
    { key: "alpha", group: "g" },
    { key: "?odd", group: "g" }
  ]);
  const list = mount(List, { target: document.body, props: { source: lines, named: true } });
  const sheet = mount(ShortcutsSheet, { target: document.body });
  flushSync();
  const grid = document.querySelector<HTMLElement>("[role=grid]")!;
  grid.focus();
  press(grid, "ArrowDown");
  press(grid, "?", { shiftKey: true });
  await flush();
  expect(open()).toBeNull();
  expect(document.querySelector('[data-line="?odd"]')!.id).toBe(
    grid.getAttribute("aria-activedescendant")
  );
  unmount(sheet);
  unmount(list);
});

for (const named of [true, false]) {
  it(`keeps the sheet closed on ? in a list ${named ? "with no row it names" : "that names no rows"}`, async () => {
    const lines = box([
      { key: "alpha", group: "g" },
      { key: "beta", group: "g" }
    ]);
    const list = mount(List, { target: document.body, props: { source: lines, named } });
    const sheet = mount(ShortcutsSheet, { target: document.body });
    flushSync();
    const grid = document.querySelector<HTMLElement>("[role=grid]")!;
    grid.focus();
    press(grid, "ArrowDown");
    press(grid, "?", { shiftKey: true });
    // a control in a row is in the list too
    press(document.querySelector('[data-line="beta"] button')!, "?", { shiftKey: true });
    await flush();
    expect(open()).toBeNull();
    unmount(sheet);
    unmount(list);
  });
}
