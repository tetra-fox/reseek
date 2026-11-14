import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { live } from "#lib/live/live.svelte.js";
import { session } from "#lib/session.svelte.js";
import { fakeNetwork } from "#lib/testing/network.js";

import ConnectButton from "./ConnectButton.svelte";

describe("the connect button", () => {
  let component: ReturnType<typeof mount> | null = null;
  beforeEach(() => void vi.useFakeTimers());
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    app.signedOut();
    vi.unstubAllGlobals();
    vi.useRealTimers();
    document.body.innerHTML = "";
  });

  it("is taken away from an administrator lowered while the page is open", async () => {
    let role = "Administrator";
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/session") ? { name: "alice", role, session: null } : undefined
    );
    await session.check();
    app.state = { server: { isConnected: false, listen: {} } } as never;
    live.start();
    await vi.advanceTimersByTimeAsync(0);
    net.frame("hello", { principal: { name: "alice", role } } as never);
    component = mount(ConnectButton, { target: document.body });
    flushSync();
    const button = () => document.querySelector<HTMLButtonElement>("button")!;
    expect(button().disabled).toBe(false);

    role = "ReadOnly";
    net.frame("bye", { reason: "roleChanged" } as never);
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(session.role).toBe("ReadOnly");
    expect(button().disabled).toBe(true);
    expect(live.status).toBe("connecting");
  });
});
