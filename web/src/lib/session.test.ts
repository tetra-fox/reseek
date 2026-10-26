import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api, onUnauthorized } from "./api/client";
import { IDLE_CHECK_MS, session } from "./session.svelte";

/** Answers the daemon's session routes from `answers`, path by path */
const serve = (answers: Record<string, () => Response>) => {
  const fetcher = vi.fn(async (request: Request) => {
    const path = new URL(request.url).pathname;
    const answer = answers[`${request.method} ${path}`];
    return answer ? answer() : new Response(null, { status: 404 });
  });
  vi.stubGlobal("fetch", fetcher);
  return fetcher;
};

const json = (body: unknown, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });

const principal = (expiresAt: string | null) => ({
  subject: "local:admin",
  name: "admin",
  role: "ReadWrite",
  via: { kind: "local" },
  provider: "local",
  session: expiresAt && { id: "s", createdAt: "2026-10-26T00:00:00Z", expiresAt }
});

describe("session", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-26T12:00:00Z"));
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    session.clear();
  });

  it("reads who is signed in and what they may do", async () => {
    serve({
      "GET /api/v0/session/enabled": () => json(true),
      "GET /api/v0/session": () => json(principal("2026-10-27T12:00:00Z"))
    });
    expect(await session.check()).toBe("in");
    expect(session.can("ReadOnly")).toBe(true);
    expect(session.can("ReadWrite")).toBe(true);
    expect(session.can("Administrator")).toBe(false);
    expect(session.canSignOut).toBe(true);
  });

  it("tells a session that is gone from a daemon that can't be reached", async () => {
    serve({
      "GET /api/v0/session/enabled": () => json(true),
      "GET /api/v0/session": () => json({ error: "authentication required" }, 401)
    });
    expect(await session.check()).toBe("out");
    expect(session.principal).toBeNull();
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("Failed to fetch")));
    expect(await session.check()).toBe("unreachable");
  });

  it("checks again when the session is due to expire, and says it ended", async () => {
    let signedIn = true;
    serve({
      "GET /api/v0/session/enabled": () => json(true),
      "GET /api/v0/session": () =>
        signedIn ? json(principal("2026-10-26T12:10:00Z")) : json({ error: "no" }, 401)
    });
    const ended = vi.fn();
    session.onEnded(ended);
    await session.check();
    signedIn = false;
    await vi.advanceTimersByTimeAsync(10 * 60_000);
    expect(ended).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1_000);
    expect(ended).toHaveBeenCalledOnce();
  });

  it("checks again when the tab comes back after a long time hidden", async () => {
    const fetcher = serve({
      "GET /api/v0/session/enabled": () => json(true),
      "GET /api/v0/session": () => json(principal(null))
    });
    await session.check();
    const calls = fetcher.mock.calls.length;
    session.visibility(true, 0);
    session.visibility(false, IDLE_CHECK_MS - 1);
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher.mock.calls.length).toBe(calls);
    session.visibility(true, 0);
    session.visibility(false, IDLE_CHECK_MS + 1);
    await vi.advanceTimersByTimeAsync(0);
    expect(fetcher.mock.calls.length).toBe(calls + 2);
  });

  it("ends the session on a 401 from any route but the session's own", async () => {
    const gone = vi.fn();
    onUnauthorized(gone);
    serve({
      "GET /api/v0/session": () => json({ error: "authentication required" }, 401),
      "POST /api/v0/session": () => json({ error: "invalid credentials" }, 401),
      "GET /api/v0/application": () => json({ error: "authentication required" }, 401)
    });
    await api.GET("/api/v0/session");
    expect(gone).not.toHaveBeenCalled();
    const login = await session.login("admin", "wrong");
    expect(login).toMatchObject({ ok: false, error: "Invalid username or password" });
    expect(gone).not.toHaveBeenCalled();
    await api.GET("/api/v0/application");
    expect(gone).toHaveBeenCalledOnce();
  });
});
