import { afterEach, describe, expect, it, vi } from "vitest";

import { fakeNetwork } from "#lib/testing/network.js";

import type { ConfigView } from "./fields";
import fixture from "./fixtures/view.json";
import { writeFromPage } from "./write";

const view = structuredClone(fixture) as unknown as ConfigView;

describe("a write from a page", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("is refused once the options changed since the view, saying what changed", async () => {
    // another admin made a group after this page read the options
    const now = structuredClone(view);
    now.revision = "second";
    (
      now.values as { transfers: { groups: { user_defined: unknown } } }
    ).transfers.groups.user_defined = {
      friends: {}
    };
    const network = fakeNetwork((url, method) => {
      if (method === "PATCH") {
        return {
          status: 409,
          body: { error: "The options changed since they were read", detail: "second" }
        };
      }
      return url.pathname.endsWith("/options/view") ? now : undefined;
    });
    const outcome = await writeFromPage(view, { "transfers.groups.user_defined": { mine: {} } });
    expect(outcome).toMatchObject({
      ok: false,
      status: 409,
      error: "Groups changed meanwhile. Check your changes and save again."
    });
    expect(network.requests[0].body).toMatchObject({ optionsRevision: "fixture" });
  });
});
