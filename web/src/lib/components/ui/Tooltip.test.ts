import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import Tip from "#lib/testing/Tip.svelte";

describe("a tooltip", () => {
  let mounted: ReturnType<typeof mount>[] = [];
  afterEach(() => {
    for (const component of mounted) unmount(component);
    mounted = [];
    document.body.innerHTML = "";
  });

  /** The tab index of a static element with a tooltip, dormant or with its trigger */
  const stop = (props: { text?: string; dense?: boolean }, dormant: boolean) => {
    const target = document.createElement("div");
    document.body.append(target);
    mounted.push(mount(Tip, { target, props: { ...props, dormant } }));
    flushSync();
    return target.querySelector("[role=note]")!.getAttribute("tabindex");
  };

  it("makes a static element the same tab stop dormant as with its trigger", () => {
    for (const props of [{ text: "why" }, { text: "why", dense: true }, {}]) {
      expect(stop(props, true)).toBe(stop(props, false));
    }
    expect(stop({ text: "why" }, true)).toBe("0");
  });
});
