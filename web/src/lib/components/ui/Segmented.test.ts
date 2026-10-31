import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import Segmented from "./Segmented.svelte";

const OPTIONS = [
  { id: "a", label: "A" },
  { id: "b", label: "B" },
  { id: "c", label: "C" }
] as const;

describe("Segmented", () => {
  let shown: ReturnType<typeof mount> | undefined;
  afterEach(() => {
    if (shown) unmount(shown);
    shown = undefined;
    document.body.replaceChildren();
  });

  const radios = () => [...document.querySelectorAll<HTMLElement>("[role=radio]")];
  const press = (key: string) => {
    (document.activeElement as HTMLElement).dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true })
    );
    flushSync();
  };

  it("keeps one stop in the tab order, and the arrows move it round the ends", () => {
    shown = mount(Segmented, {
      target: document.body,
      props: { options: OPTIONS, value: "a", label: "Choice" }
    });
    flushSync();
    expect(radios().map((r) => r.tabIndex)).toEqual([0, -1, -1]);

    radios()[0].focus();
    press("ArrowRight");
    expect(document.activeElement).toBe(radios()[1]);
    expect(radios()[1].getAttribute("aria-checked")).toBe("true");
    expect(radios().map((r) => r.tabIndex)).toEqual([-1, 0, -1]);

    press("ArrowLeft");
    press("ArrowUp");
    expect(document.activeElement).toBe(radios()[2]);
    expect(radios()[2].getAttribute("aria-checked")).toBe("true");

    press("ArrowDown");
    expect(document.activeElement).toBe(radios()[0]);
  });
});
