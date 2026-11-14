import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import { flush } from "#lib/testing/flush.js";

import LongText from "./LongText.svelte";

describe("a long message", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    document.body.innerHTML = "";
  });

  it("is folded between two letters, never inside an emoji", async () => {
    const text = `a${"🔥".repeat(600)}`;
    component = mount(LongText, { target: document.body, props: { text, watch: [] } });
    await flush();
    const shown = document.body.textContent!;
    expect(shown).toContain("Show more");
    // half of a two-unit letter, which draws as a box
    expect(shown).not.toMatch(
      /[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/
    );
    expect(shown.startsWith(`a${"🔥".repeat(499)}…`)).toBe(true);
  });
});
