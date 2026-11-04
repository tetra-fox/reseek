import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import NotesEditor from "./NotesEditor.svelte";

describe("the notes on a user", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("are saved at once when the tab changes before typing pauses", async () => {
    const net = fakeNetwork(() => ({ status: 204 }));
    const component = mount(NotesEditor, {
      target: document.body,
      props: { username: "alice", notes: "", disabled: false }
    });
    flushSync();
    const box = document.querySelector("textarea")!;
    box.value = "owes me a rip";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    unmount(component);
    await flush();
    expect(net.requests.map((r) => r.body)).toEqual([{ notes: "owes me a rip" }]);
  });
});
