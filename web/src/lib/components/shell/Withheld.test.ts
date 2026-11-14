import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "#lib/app.svelte.js";
import { flush } from "#lib/testing/flush.js";
import { fakeNetwork } from "#lib/testing/network.js";

import Withheld from "./Withheld.svelte";

describe("the screen of a session with no access", () => {
  let component: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    if (component) unmount(component);
    component = null;
    app.signedOut();
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
  });

  it("signs out of the session it keeps", async () => {
    const net = fakeNetwork((url, method) =>
      method === "POST" && url.pathname.endsWith("/api/v0/session/logout")
        ? { redirect: null }
        : undefined
    );
    app.withheld();
    component = mount(Withheld, { target: document.body });
    flushSync();
    const signOut = [...document.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Sign out"
    )!;
    signOut.click();
    await flush();
    expect(net.requests.map((r) => `${r.method} ${r.url.pathname}`)).toContain(
      "POST /api/v0/session/logout"
    );
    expect(app.phase).toBe("login");
  });
});
