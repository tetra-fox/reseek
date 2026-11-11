import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import MessageAll from "./MessageAll.svelte";

describe("messaging several users", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("sends to nobody more once canceled, keeping the message", async () => {
    let release!: () => void;
    const held = new Promise<void>((done) => (release = done));
    const net = fakeNetwork(async () => {
      await held;
      return {};
    });
    const component = mount(MessageAll, {
      target: document.body,
      props: { open: true, usernames: ["alice", "bob", "carol"], title: "", description: "" }
    });
    flushSync();
    const box = document.querySelector("textarea")!;
    box.value = "thanks";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const button = (text: string) =>
      [...document.querySelectorAll("button")].find((b) => b.textContent?.trim().startsWith(text))!;
    button("Send").click();
    await flush();
    button("Cancel").click();
    release();
    await flush();
    const sent = net.requests.filter((r) => r.method === "POST");
    expect(sent.map((r) => r.url.pathname.split("/").at(-1))).toEqual(["alice"]);
    // the message and the dialog stay, to send it again
    expect(document.querySelector("textarea")!.value).toBe("thanks");
    unmount(component);
  });
});
