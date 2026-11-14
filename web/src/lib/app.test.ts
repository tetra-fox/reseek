import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { app, WITHHELD_CHECK_MS } from "./app.svelte";
import { FALLBACK_MS, live } from "./live/live.svelte";
import { session } from "./session.svelte";
import { fakeNetwork } from "./testing/network";
import { time } from "./testing/tabs";
import { browserTabs, SETTLE_MS, Tabs } from "./ui/tabs";

// the browser's tabs talk over a bus whose messages arrive at once, rather than over a
// BroadcastChannel, whose messages cross on the event loop. In place before the app's module
// makes this tab
const tabsBus = await vi.hoisted(async () => {
  const { bus } = await import("./testing/tabs");
  const channel = bus();
  vi.stubGlobal(
    "BroadcastChannel",
    class {
      constructor() {
        return channel();
      }
    }
  );
  return channel;
});

describe("the app's bootstrap", () => {
  /** Lets the tabs' settle pass, which the bootstrap waits out before the stream, and ends it */
  const boot = async () => {
    const booting = app.bootstrap();
    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    await booting;
  };

  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    app.signedOut();
    vi.unstubAllGlobals();
    vi.useRealTimers();
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
    await boot();
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
    await boot();
    expect(app.phase).toBe("app");
    expect(app.options).toEqual({ soulseek: { username: "me" } });
    expect(app.optionsRevision).toBe("r");
  });

  it("shows the app once the stream brought both the state and the options", async () => {
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/session")
        ? { name: "admin", role: "Administrator", session: null }
        : undefined
    );
    const booting = app.bootstrap();
    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    expect(live.status).toBe("connecting");
    net.frame("hello", { principal: { name: "admin", role: "Administrator" } } as never);
    net.frame("application.state", { user: {} } as never);
    await vi.advanceTimersByTimeAsync(20);
    expect(app.phase).toBe("initializing");
    net.frame("application.options", { values: { a: 1 }, revision: "r" } as never);
    await booting;
    expect(app.phase).toBe("app");
    expect(app.options).toEqual({ a: 1 });
  });

  it("opens the stream as soon as a session check longer than the tabs' settle ends", async () => {
    const net = fakeNetwork(async (url) => {
      if (!url.pathname.endsWith("/api/v0/session")) return undefined;
      await new Promise((done) => setTimeout(done, SETTLE_MS));
      return { name: "admin", role: "Administrator", session: null };
    });
    const booting = app.bootstrap();
    // the tabs answered while the session was checked, so the stream waits on nothing more
    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    expect(live.status).toBe("connecting");
    net.frame("hello", { principal: { name: "admin", role: "Administrator" } } as never);
    net.frame("application.state", { user: {} } as never);
    net.frame("application.options", { values: {}, revision: "r" } as never);
    await booting;
    expect(app.phase).toBe("app");
  });

  it("says why the options could not be read, and shows the app once they can", async () => {
    let options = Response.json({ error: "The options are being written" }, { status: 503 });
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const request = input instanceof Request ? input : new Request(String(input), init);
        const path = new URL(request.url).pathname;
        if (path.endsWith("/api/v0/session")) {
          return Response.json({ name: "admin", role: "Administrator", session: null });
        }
        if (path.endsWith("/api/v0/application")) return Response.json({ user: {} });
        if (path.endsWith("/api/v0/options")) return options.clone();
        // the stream is refused, so the state and the options are read
        return Response.json({ error: "Too many streams" }, { status: 429 });
      })
    );
    await boot();
    expect(app.phase).toBe("lost");
    expect([app.lostReason, app.lostDetail]).toEqual([
      "Could not load the settings",
      "The options are being written"
    ]);
    options = Response.json({ values: { a: 1 }, revision: "r" });
    await vi.advanceTimersByTimeAsync(FALLBACK_MS);
    expect(app.phase).toBe("app");
    expect(app.options).toEqual({ a: 1 });
  });

  it("waits while the session has no role, comes back once it has one, and signs in once it is gone", async () => {
    const withheld = () => Response.json({ error: "access_withheld" }, { status: 403 });
    let sessionAnswer = withheld();
    let stateAnswer = Response.json({ user: {} });
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
        const request = input instanceof Request ? input : new Request(String(input), init);
        const path = new URL(request.url).pathname;
        if (path.endsWith("/api/v0/session")) return sessionAnswer.clone();
        if (path.endsWith("/api/v0/application")) return stateAnswer.clone();
        if (path.endsWith("/api/v0/options")) return Response.json({ values: {}, revision: "r" });
        // the stream is refused, so the state and the options are read
        return Response.json({ error: "Too many streams" }, { status: 429 });
      })
    );
    await boot();
    expect(app.phase).toBe("withheld");
    await vi.advanceTimersByTimeAsync(WITHHELD_CHECK_MS);
    expect(app.phase).toBe("withheld");

    sessionAnswer = Response.json({ name: "viewer", role: "ReadOnly", session: null });
    await vi.advanceTimersByTimeAsync(WITHHELD_CHECK_MS + SETTLE_MS);
    expect(app.phase).toBe("app");

    // any request may be the first to find the role gone
    sessionAnswer = withheld();
    stateAnswer = withheld();
    await app.refresh();
    await vi.advanceTimersByTimeAsync(0);
    expect(app.phase).toBe("withheld");
    expect(app.state).not.toBeNull();

    sessionAnswer = Response.json({ error: "Authentication required" }, { status: 401 });
    await vi.advanceTimersByTimeAsync(WITHHELD_CHECK_MS);
    expect(app.phase).toBe("login");
  });

  /** Boots to the app over `net`, whose stream brings the state and the options */
  const booted = async (net: ReturnType<typeof fakeNetwork>, role: string) => {
    const booting = app.bootstrap();
    await vi.advanceTimersByTimeAsync(SETTLE_MS);
    net.frame("hello", { principal: { name: "alice", role } } as never);
    net.frame("application.state", { user: {} } as never);
    net.frame("application.options", { values: {}, revision: "r" } as never);
    await booting;
    expect(app.phase).toBe("app");
  };

  /** How often `GET /session` was asked */
  const sessionChecks = (net: ReturnType<typeof fakeNetwork>) =>
    net.requests.filter((r) => r.url.pathname.endsWith("/api/v0/session")).length;

  it("reads the session again when the role changes, and stays in the app", async () => {
    let role = "Administrator";
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/session") ? { name: "alice", role, session: null } : undefined
    );
    await booted(net, role);
    expect(session.role).toBe("Administrator");
    role = "ReadOnly";
    net.frame("bye", { reason: "roleChanged" } as never);
    await vi.advanceTimersByTimeAsync(0);
    expect(session.role).toBe("ReadOnly");
    expect(app.phase).toBe("app");
    // the stream opened again, at the new role
    const opened = vi
      .mocked(fetch)
      .mock.calls.filter(([input]) =>
        String(input instanceof Request ? input.url : input).includes("/live")
      );
    expect(opened).toHaveLength(2);
  });

  it("takes a role changed while the stream was down from the stream that opens after", async () => {
    let role = "Administrator";
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/session") ? { name: "alice", role, session: null } : undefined
    );
    await booted(net, role);
    net.drop();
    role = "ReadOnly";
    // the stream opens again after its wait
    await vi.advanceTimersByTimeAsync(2_000);
    expect(session.role).toBe("Administrator");
    net.frame("hello", { principal: { name: "alice", role } } as never);
    await vi.advanceTimersByTimeAsync(0);
    expect(session.role).toBe("ReadOnly");
    expect(app.phase).toBe("app");
  });

  it("follows what reading the session after a role change finds, unless it was left meanwhile", async () => {
    let answer: unknown = { name: "alice", role: "Administrator", session: null };
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/session") ? answer : undefined
    );
    await booted(net, "Administrator");
    answer = { status: 401, body: { error: "Authentication required" } };
    await app.roleChanged();
    expect(app.phase).toBe("login");

    answer = { name: "alice", role: "Administrator", session: null };
    await booted(net, "Administrator");
    answer = { status: 403, body: { error: "access_withheld" } };
    await app.roleChanged();
    expect(app.phase).toBe("withheld");

    answer = { name: "alice", role: "Administrator", session: null };
    await booted(net, "Administrator");
    let reply!: () => void;
    answer = new Promise(
      (done) => (reply = () => done({ status: 403, body: { error: "access_withheld" } }))
    );
    const reading = app.roleChanged();
    app.signedOut();
    reply();
    await reading;
    expect(app.phase).toBe("login");
  });

  it("checks a session with no role on one timer, stops its stream, and drops the timer when it leaves", async () => {
    const withheld = { status: 403, body: { error: "access_withheld" } };
    let answer: unknown = { name: "alice", role: "ReadOnly", session: null };
    const net = fakeNetwork((url) =>
      url.pathname.endsWith("/api/v0/session") ? answer : undefined
    );
    await booted(net, "ReadOnly");
    expect(live.status).toBe("connected");
    answer = withheld;
    // two requests refused at once
    app.withheld();
    app.withheld();
    expect(live.status).toBe("off");
    const before = sessionChecks(net);
    await vi.advanceTimersByTimeAsync(WITHHELD_CHECK_MS);
    expect(sessionChecks(net) - before).toBe(1);

    for (const leave of [() => app.signedOut(), () => app.close()]) {
      app.withheld();
      leave();
      const left = sessionChecks(net);
      await vi.advanceTimersByTimeAsync(WITHHELD_CHECK_MS * 2);
      expect(sessionChecks(net)).toBe(left);
    }

    app.withheld();
    answer = { status: 401, body: { error: "Authentication required" } };
    await app.bootstrap();
    const started = sessionChecks(net);
    await vi.advanceTimersByTimeAsync(WITHHELD_CHECK_MS * 2);
    expect(sessionChecks(net)).toBe(started);
  });

  it("leaves the sound to the tabs that are signed in while it shows the login", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => Response.json({ error: "Not signed in" }, { status: 401 }))
    );
    await app.bootstrap();
    expect(app.phase).toBe("login");
    // a tab opened after this one, signed in: this one is older, yet it has left the election
    const clock = time();
    const signedIn = new Tabs(tabsBus(), performance.timeOrigin + 60_000, clock.now);
    signedIn.join();
    clock.pass(SETTLE_MS);
    expect(signedIn.leads()).toBe(true);
    expect(browserTabs()?.leads()).toBe(false);
    signedIn.quit();
  });
});
