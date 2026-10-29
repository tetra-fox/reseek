import { describe, expect, it } from "vitest";

import { call, reason, UNREACHABLE } from "./request";

describe("reason", () => {
  it("prefers the daemon's message and adds its detail", () => {
    expect(reason(400, { error: "invalid configuration", detail: "web.port: too big" })).toBe(
      "invalid configuration: web.port: too big"
    );
    expect(reason(409, { error: "config file changed on disk", detail: null })).toBe(
      "config file changed on disk"
    );
  });

  it("falls back to words for the status", () => {
    expect(reason(403)).toBe("Permission denied");
    expect(reason(404, {})).toBe("Not found");
    expect(reason(429)).toBe("Too many attempts. Try again shortly");
    expect(reason(502)).toBe("Server error (HTTP 502)");
    expect(reason(418)).toBe("HTTP 418");
  });

  it("never shows an object", () => {
    expect(reason(400, { errors: { a: 1 } })).toBe("HTTP 400");
  });
});

describe("call", () => {
  it("tells a refused request from one that got no answer", async () => {
    const refused = await call(async () => ({
      error: { error: "nope" },
      response: new Response(null, { status: 403 })
    }));
    expect(refused).toEqual({ ok: false, error: "nope", status: 403, body: { error: "nope" } });
    const lost = await call(() => Promise.reject(new TypeError("Failed to fetch")));
    expect(lost).toEqual({ ok: false, error: UNREACHABLE, status: 0 });
  });
});
