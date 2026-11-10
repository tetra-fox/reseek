import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "./app.svelte";
import { live } from "./live/live.svelte";
import { browserTabs, SETTLE_MS, Tabs } from "./ui/tabs";

describe("the app's bootstrap", () => {
  afterEach(() => {
    app.signedOut();
    vi.unstubAllGlobals();
  });

  it("shows the login when the session goes while it waits for the first state", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const request = input instanceof Request ? input : new Request(String(input), init);
        const path = new URL(request.url).pathname;
        if (path.endsWith("/api/v0/session")) {
          return Response.json({ name: "admin", role: "Administrator", session: null });
        }
        // the session ran out between the check and the stream
        return new Response(null, { status: 401 });
      })
    );
    await app.bootstrap();
    expect(app.phase).toBe("login");
    expect(live.status).toBe("off");
  });

  it("reads the options beside the state when the stream can't open", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const request = input instanceof Request ? input : new Request(String(input), init);
        const path = new URL(request.url).pathname;
        if (path.endsWith("/api/v0/session")) {
          return Response.json({ name: "editor", role: "ReadWrite", session: null });
        }
        if (path.endsWith("/api/v0/application")) return Response.json({ user: {} });
        if (path.endsWith("/api/v0/options"))
          return Response.json({ values: { soulseek: { username: "me" } }, revision: "r" });
        // more tabs than the daemon streams to
        return Response.json({ error: "Too many streams" }, { status: 429 });
      })
    );
    await app.bootstrap();
    expect(app.phase).toBe("app");
    expect(app.options).toEqual({ soulseek: { username: "me" } });
    expect(app.optionsRevision).toBe("r");
  });

  it("leaves the sound to the tabs that are signed in while it shows the login", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Response.json({ error: "Not signed in" }, { status: 401 }))
    );
    await app.bootstrap();
    expect(app.phase).toBe("login");
    // a tab opened after this one, signed in: this one is older, yet it has left the election
    const channel = new BroadcastChannel("reseek-tabs");
    const signedIn = new Tabs(channel, performance.timeOrigin + 60_000);
    signedIn.join();
    await new Promise((done) => setTimeout(done, SETTLE_MS + 20));
    expect(signedIn.leads()).toBe(true);
    expect(browserTabs()?.leads()).toBe(false);
    signedIn.quit();
    channel.close();
  });
});
