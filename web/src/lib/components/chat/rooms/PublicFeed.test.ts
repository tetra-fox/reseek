import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import { app } from "#lib/app.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { connected } from "#lib/testing/live.js";

import PublicFeed from "./PublicFeed.svelte";

describe("the public room feed", () => {
  afterEach(() => {
    live.stop();
    app.state = null;
    document.body.innerHTML = "";
  });

  it("follows the feed only while it is open, and filters what came", async () => {
    app.state = { server: { isConnected: true } } as never;
    const net = connected(() => undefined, "Administrator");
    const feed = mount(PublicFeed, { target: document.body, props: {} });
    await flush();
    expect(live.topics).toContain("public_rooms");
    expect(document.body.textContent).toContain("Waiting for messages");

    const said = (roomName: string, username: string, message: string) =>
      net.frame("rooms.public_message", {
        roomName,
        username,
        message,
        receivedAt: "2026-11-05T10:00:00Z"
      });
    said("indie", "alice", "anyone have the new rip?");
    said("jazz", "bob", "coltrane all night");
    await flush();
    const lines = () =>
      [...document.querySelectorAll('[role="log"] > div')].map((l) => l.textContent);
    expect(lines()).toHaveLength(2);
    expect(lines()[0]).toContain("#indie");

    const box = document.querySelector<HTMLInputElement>(
      'input[aria-label="Filter the messages"]'
    )!;
    box.value = "JAZZ";
    box.dispatchEvent(new Event("input", { bubbles: true }));
    await flush();
    expect(lines()).toHaveLength(1);
    expect(lines()[0]).toContain("coltrane");

    unmount(feed);
    expect(live.topics).not.toContain("public_rooms");
  });
});
