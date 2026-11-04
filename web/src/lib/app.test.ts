import { afterEach, describe, expect, it, vi } from "vitest";

import { app } from "./app.svelte";
import { live } from "./live/live.svelte";

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
});
