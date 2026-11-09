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

  it("are sent in a request that outlives the page when the tab closes before typing pauses", async () => {
    const sent: { keepalive: boolean; body: string }[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (request: Request) => {
        sent.push({ keepalive: request.keepalive, body: await request.text() });
        return new Response(null, { status: 204 });
      })
    );
    const component = mount(NotesEditor, {
      target: document.body,
      props: { username: "alice", notes: "", disabled: false }
    });
    flushSync();
    const box = document.querySelector("textarea")!;
    box.value = "owes me a rip";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    window.dispatchEvent(new PageTransitionEvent("pagehide"));
    await flush();
    expect(sent).toEqual([{ keepalive: true, body: JSON.stringify({ notes: "owes me a rip" }) }]);

    // notes past the size a keepalive request may carry go as a plain one, which is sent at all
    const long = "é".repeat(32 * 1024);
    box.value = long;
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    window.dispatchEvent(new PageTransitionEvent("pagehide"));
    await flush();
    expect(sent[1]).toEqual({ keepalive: false, body: JSON.stringify({ notes: long }) });
    unmount(component);
  });
});
